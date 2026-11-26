use std::path::{Path, PathBuf};

use egui::Ui;
use serde::Deserialize;
use zen_i18n::t;

use super::model::{
    DEFAULT_IDE_ZOOM, DEFAULT_UI_FONT_SIZE, IDE_ZOOM_PRESETS, IDE_ZOOMS, PRESENTATION_FONT_SIZES, UI_FONT_SIZES,
};
use super::{Dispatch, SettingsDialog, widgets};
use crate::ide::IdePalette;
use crate::theme;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
struct ThemeList {
    current: String,
    themes: Vec<ThemeEntry>,
    #[serde(default)]
    appearance: ThemeAppearance,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ThemeAppearance {
    follow_system: bool,
    dark: String,
    light: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct ThemeEntry {
    id: String,
    name: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct ThemeImported {
    name: String,
    mapped: usize,
    #[serde(default)]
    unmapped: Vec<String>,
    #[serde(default)]
    adjusted: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThemeRow {
    id: String,
    name: String,
    user: bool,
    selected: bool,
    enabled: bool,
    hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ThemeMessage {
    Info(String),
    Error(String),
}

#[derive(Default)]
pub(super) struct ThemeChoice {
    list: Option<ThemeList>,
    original: Option<String>,
    original_follow: Option<bool>,
    message: Option<ThemeMessage>,
}

fn theme_rows(list: &ThemeList) -> Vec<ThemeRow> {
    let mut rows: Vec<ThemeRow> = list
        .themes
        .iter()
        .map(|entry| ThemeRow {
            id: entry.id.clone(),
            name: entry.name.clone(),
            user: entry.source == "user",
            selected: entry.id == list.current,
            enabled: entry.error.is_none(),
            hint: entry.error.as_ref().map(|error| t!("settings-theme-broken", error = error.as_str())),
        })
        .collect();
    rows.sort_by_key(|row| row.user);
    rows
}

fn imported_message(imported: &ThemeImported) -> String {
    let message = t!(
        "settings-theme-imported",
        name = imported.name.as_str(),
        mapped = imported.mapped,
        unmapped = imported.unmapped.len()
    );
    if imported.adjusted.is_empty() {
        message
    } else {
        format!("{message}; {}", t!("settings-theme-adjusted", count = imported.adjusted.len()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_theme_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(t!("settings-theme-load-title"))
        .add_filter(t!("settings-theme-files"), &["json", "icls", "xml"])
        .pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_theme_file() -> Option<PathBuf> {
    None
}

fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

impl ThemeChoice {
    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        *self = Self::default();
        let _ = dispatch("ui.theme.reload", serde_json::json!({}));
        self.refresh(dispatch);
        self.original = self.list.as_ref().map(|list| list.current.clone());
        self.original_follow = self.list.as_ref().map(|list| list.appearance.follow_system);
    }

    fn follow_system(&mut self, enabled: bool, dispatch: &mut Dispatch<'_>) {
        self.message = None;
        if let Err(error) = dispatch("ui.theme.followSystem", serde_json::json!({ "enabled": enabled })) {
            self.message = Some(ThemeMessage::Error(error));
        }
        self.refresh(dispatch);
    }

    fn refresh(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("ui.theme.list", serde_json::json!({})).and_then(parse::<ThemeList>) {
            Ok(list) => self.list = Some(list),
            Err(error) => self.message = Some(ThemeMessage::Error(error)),
        }
    }

    fn select(&mut self, id: &str, dispatch: &mut Dispatch<'_>) {
        self.message = None;
        if let Err(error) = dispatch("ui.theme.select", serde_json::json!({ "id": id })) {
            self.message = Some(ThemeMessage::Error(error));
        }
        self.refresh(dispatch);
    }

    fn import(&mut self, path: &Path, dispatch: &mut Dispatch<'_>) {
        let result = dispatch("ui.theme.import", serde_json::json!({ "path": path, "select": true }))
            .and_then(parse::<ThemeImported>);
        self.message = Some(match result {
            Ok(imported) => ThemeMessage::Info(imported_message(&imported)),
            Err(error) => ThemeMessage::Error(t!("settings-theme-import-failed", error = error)),
        });
        self.refresh(dispatch);
    }

    pub(super) fn commit(&mut self) {
        if let Some(list) = &self.list {
            self.original = Some(list.current.clone());
            self.original_follow = Some(list.appearance.follow_system);
        }
    }

    pub(super) fn cancel(&mut self, dispatch: &mut Dispatch<'_>) {
        if let Some(follow) = self.original_follow.take()
            && self.list.as_ref().is_some_and(|list| list.appearance.follow_system != follow)
        {
            let _ = dispatch("ui.theme.followSystem", serde_json::json!({ "enabled": follow }));
            self.refresh(dispatch);
        }
        if let Some(original) = self.original.take()
            && self.list.as_ref().is_some_and(|list| list.current != original)
        {
            let _ = dispatch("ui.theme.select", serde_json::json!({ "id": original }));
        }
        *self = Self::default();
    }
}

impl SettingsDialog {
    pub(super) fn appearance_page(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        query: Option<&str>,
        dispatch: Option<&mut Dispatch<'_>>,
    ) {
        if let Some(dispatch) = dispatch {
            self.theme_block(ui, p, query, dispatch);
            ui.add_space(12.0);
        }
        let Some(draft) = self.draft.as_mut() else { return };
        let _ = ui_font_row(ui, p, query, &mut draft.ui_font_size);

        ui.add_space(18.0);
        widgets::group_title(ui, p, &t!("settings-accessibility"), query);
        let _ = zoom_row(ui, p, query, &mut draft.ide_zoom);
        ui.label(egui::RichText::new(t!("settings-ide-zoom-note")).color(p.text_secondary));

        ui.add_space(18.0);
        widgets::group_title(ui, p, &t!("settings-presentation"), query);
        let label = t!("settings-presentation-font-size");
        let field =
            widgets::number_field(ui, p, &label, query, &mut draft.presentation_font_size, PRESENTATION_FONT_SIZES);
        if field.changed() {
            draft.presentation_font_size = draft.presentation_font_size.round();
        }

        ui.add_space(18.0);
        let title = widgets::group_title(ui, p, &t!("settings-language"), query);
        let name = |code: &str| {
            zen_i18n::LANGUAGES
                .iter()
                .find(|(known, _)| *known == code)
                .map_or(code.to_owned(), |(_, name)| (*name).to_owned())
        };
        let system = t!("settings-language-system", language = name(&zen_i18n::system_language()));
        let current = draft.language.clone().unwrap_or_else(|| zen_i18n::SOURCE.to_owned());
        let selected = if current == zen_i18n::SYSTEM { system.clone() } else { name(&current) };
        let combo = egui::ComboBox::from_id_salt("settings-language")
            .selected_text(selected)
            .width(widgets::field_width(ui, 120.0, 220.0))
            .truncate()
            .show_ui(ui, |ui| {
                for (code, language) in zen_i18n::LANGUAGES {
                    if ui.selectable_label(current == *code, *language).clicked() {
                        draft.language = Some((*code).to_owned());
                    }
                }
                if ui.selectable_label(current == zen_i18n::SYSTEM, system).clicked() {
                    draft.language = Some(zen_i18n::SYSTEM.to_owned());
                }
            });
        let _ = combo.response.labelled_by(title.id);
        ui.label(egui::RichText::new(t!("settings-language-note")).color(p.text_secondary));
    }

    fn theme_block(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>, dispatch: &mut Dispatch<'_>) {
        let title = widgets::group_title(ui, p, &t!("settings-theme"), query);
        let rows = self.theme.list.as_ref().map(theme_rows).unwrap_or_default();
        let current = rows.iter().find(|row| row.selected).map(|row| row.name.clone()).unwrap_or_default();
        let mut chosen = None;
        let following = self.theme.list.as_ref().is_some_and(|list| list.appearance.follow_system);
        let mut sync = None;
        let load = ui.horizontal_wrapped(|ui| {
            ui.label_inputs_by(title.id, |ui| {
                let width = widgets::field_width(ui, 140.0, 260.0);
                egui::ComboBox::from_id_salt("settings-theme").selected_text(current).width(width).truncate().show_ui(
                    ui,
                    |ui| {
                        let groups = rows.iter().any(|row| row.user);
                        let mut user_group = None;
                        for row in &rows {
                            if groups && user_group != Some(row.user) {
                                if user_group.is_some() {
                                    ui.separator();
                                }
                                let header =
                                    if row.user { t!("settings-theme-user") } else { t!("settings-theme-builtin") };
                                ui.label(
                                    egui::RichText::new(header).font(theme::regular(11.5)).color(p.text_secondary),
                                );
                                user_group = Some(row.user);
                            }
                            let response =
                                ui.add_enabled(row.enabled, egui::Button::selectable(row.selected, &row.name));
                            if let Some(hint) = &row.hint {
                                response.on_disabled_hover_text(hint);
                                ui.label(egui::RichText::new(hint).font(theme::regular(11.5)).color(p.error_text));
                            } else if response.clicked() && !row.selected {
                                chosen = Some(row.id.clone());
                            }
                        }
                    },
                )
            });
            ui.add_space(8.0);
            let mut follow = following;
            if widgets::checkbox(ui, p, &mut follow, &t!("settings-theme-sync"), query).changed() {
                sync = Some(follow);
            }
            ui.add_space(8.0);
            !cfg!(target_arch = "wasm32") && ui.button(t!("settings-theme-load")).clicked()
        });
        if let Some(id) = chosen {
            self.theme.select(&id, dispatch);
        }
        if let Some(follow) = sync {
            self.theme.follow_system(follow, dispatch);
        }
        if let Some(list) = self.theme.list.as_ref().filter(|list| list.appearance.follow_system) {
            let name = |id: &str| rows.iter().find(|row| row.id == id).map_or(id.to_owned(), |row| row.name.clone());
            let note = t!(
                "settings-theme-sync-note",
                dark = name(&list.appearance.dark),
                light = name(&list.appearance.light)
            );
            ui.label(egui::RichText::new(note).color(p.text_secondary));
        }
        if load.inner
            && let Some(path) = pick_theme_file()
        {
            self.theme.import(&path, dispatch);
        }
        match &self.theme.message {
            Some(ThemeMessage::Info(text)) => {
                ui.label(egui::RichText::new(text).color(p.text_secondary));
            }
            Some(ThemeMessage::Error(text)) => {
                ui.label(egui::RichText::new(text).color(p.error_text));
            }
            None => {}
        }
    }
}

fn ui_font_row(
    ui: &mut Ui,
    p: &IdePalette,
    query: Option<&str>,
    ui_font_size: &mut Option<f32>,
) -> (egui::Response, egui::Response) {
    ui.horizontal_wrapped(|ui| {
        let mut custom = ui_font_size.is_some();
        let check = widgets::checkbox(ui, p, &mut custom, &t!("settings-ui-font-custom"), query);
        if check.changed() {
            *ui_font_size = custom.then_some(DEFAULT_UI_FONT_SIZE);
        }
        ui.add_space(12.0);
        let size = ui
            .add_enabled_ui(custom, |ui| {
                let mut size = ui_font_size.unwrap_or(DEFAULT_UI_FONT_SIZE);
                let field = widgets::number_field(ui, p, &t!("settings-ui-font-size"), query, &mut size, UI_FONT_SIZES);
                if field.changed() {
                    *ui_font_size = Some(size.round());
                }
                field
            })
            .inner;
        (check, size)
    })
    .inner
}

fn zoom_row(
    ui: &mut Ui,
    p: &IdePalette,
    query: Option<&str>,
    ide_zoom: &mut u16,
) -> (egui::Response, egui::Response, Option<egui::Response>) {
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(widgets::label_job(ui, p, &t!("settings-ide-zoom"), query));
        let percent = |value: u16| t!("ide-zoom-percent", percent = value);
        let presets = egui::ComboBox::from_id_salt("settings-ide-zoom")
            .selected_text(percent(*ide_zoom))
            .width(90.0)
            .show_ui(ui, |ui| {
                for preset in IDE_ZOOM_PRESETS {
                    ui.selectable_value(ide_zoom, preset, percent(preset));
                }
            })
            .response
            .labelled_by(label.id);
        ui.add_space(12.0);
        let custom_label = ui.label(widgets::label_job(ui, p, &t!("settings-ide-zoom-custom"), query));
        let custom = ui.add(egui::DragValue::new(ide_zoom).range(IDE_ZOOMS).suffix("%")).labelled_by(custom_label.id);
        let reset = (*ide_zoom != DEFAULT_IDE_ZOOM).then(|| {
            let job = widgets::label_job(ui, p, &t!("settings-ide-zoom-reset"), query);
            let width = ui.fonts_mut(|fonts| fonts.layout_job(job.clone())).size().x;
            if ui.available_size_before_wrap().x < width + 12.0 {
                ui.end_row();
            } else {
                ui.add_space(12.0);
            }
            let link = ui
                .scope(|ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.link(job)
                })
                .inner;
            if link.clicked() {
                *ide_zoom = DEFAULT_IDE_ZOOM;
            }
            link
        });
        (presets, custom, reset)
    })
    .inner
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::ide::settings::{IdeSettings, PageId, SettingsContext};
    use egui::vec2;
    use serde_json::{Value, json};

    pub(in crate::ide::settings) fn following_the_system(dialog: &mut SettingsDialog) {
        let mut fake = Fake::new();
        fake.follow = true;
        dialog.theme.list = parse::<ThemeList>(fake.dispatch("ui.theme.list", json!({})).unwrap()).ok();
        assert!(dialog.theme.list.is_some());
    }

    struct Fake {
        current: String,
        selects: Vec<String>,
        reloads: usize,
        follow: bool,
    }

    impl Fake {
        fn new() -> Self {
            Self { current: "zen-dark".to_owned(), selects: Vec::new(), reloads: 0, follow: false }
        }

        fn dispatch(&mut self, id: &str, params: Value) -> Result<Value, String> {
            match id {
                "ui.theme.followSystem" => {
                    self.follow = params["enabled"].as_bool().unwrap_or_default();
                    Ok(json!({}))
                }
                "ui.theme.list" => Ok(json!({
                    "current": self.current,
                    "appearance": { "followSystem": self.follow, "systemDark": true, "dark": "monokai", "light": "zen-light" },
                    "themes": [
                        { "id": "broken", "name": "Broken", "dark": true, "source": "user",
                          "path": "/themes/broken.json", "error": "line 3: expected `,`" },
                        { "id": "zen-dark", "name": "Zen Dark", "dark": true, "source": "builtin" },
                        { "id": "monokai", "name": "Monokai", "dark": true, "source": "user",
                          "path": "/themes/monokai.json" },
                    ],
                })),
                "ui.theme.reload" => {
                    self.reloads += 1;
                    Ok(json!({}))
                }
                "ui.theme.select" => {
                    let id = params["id"].as_str().unwrap_or_default().to_owned();
                    if id == "broken" {
                        return Err("the theme has errors".to_owned());
                    }
                    self.selects.push(id.clone());
                    self.current = id;
                    Ok(json!({}))
                }
                "ui.theme.import" => {
                    assert_eq!(params["select"], json!(true));
                    if params["path"].as_str().is_some_and(|path| path.ends_with(".txt")) {
                        return Err("not a theme file".to_owned());
                    }
                    self.current = "monokai".to_owned();
                    Ok(json!({ "id": "monokai", "name": "Monokai", "mapped": 12, "unmapped": ["a", "b", "c"] }))
                }
                _ => Err(format!("unknown command `{id}`")),
            }
        }
    }

    fn opened(fake: &mut Fake) -> ThemeChoice {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut choice = ThemeChoice::default();
        choice.begin(&mut |id: &str, params| fake.dispatch(id, params));
        choice
    }

    #[test]
    fn cancel_restores_the_theme_the_dialog_opened_with() {
        let mut fake = Fake::new();
        let mut choice = opened(&mut fake);
        assert_eq!(fake.reloads, 1, "opening reads the themes folder again");
        assert_eq!(choice.original.as_deref(), Some("zen-dark"));
        choice.select("monokai", &mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(fake.current, "monokai", "the choice applies at once");
        assert_eq!(choice.list.as_ref().map(|list| list.current.as_str()), Some("monokai"));
        choice.cancel(&mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(fake.current, "zen-dark");
        assert!(choice.list.is_none() && choice.original.is_none(), "the state is cleared");
    }

    #[test]
    fn sync_with_os_is_a_command_and_cancel_undoes_it() {
        let mut fake = Fake::new();
        let mut choice = opened(&mut fake);
        assert_eq!(choice.original_follow, Some(false));
        choice.follow_system(true, &mut |id: &str, params| fake.dispatch(id, params));
        assert!(fake.follow);
        assert!(choice.list.as_ref().is_some_and(|list| list.appearance.follow_system));
        assert_eq!(choice.list.as_ref().map(|list| list.appearance.dark.as_str()), Some("monokai"));
        choice.cancel(&mut |id: &str, params| fake.dispatch(id, params));
        assert!(!fake.follow, "Cancel turns it off again");

        let mut choice = opened(&mut fake);
        choice.follow_system(true, &mut |id: &str, params| fake.dispatch(id, params));
        choice.commit();
        choice.cancel(&mut |id: &str, params| fake.dispatch(id, params));
        assert!(fake.follow, "Cancel after Apply keeps it");
    }

    #[test]
    fn an_import_says_how_many_colours_were_made_readable() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let imported = ThemeImported {
            name: "Day".to_owned(),
            mapped: 40,
            unmapped: Vec::new(),
            adjusted: vec!["ui.text.secondary".to_owned(), "terminal.ansi.3".to_owned()],
        };
        let message = imported_message(&imported);
        let plain = imported_message(&ThemeImported { adjusted: Vec::new(), ..imported });
        assert!(message.starts_with(&plain) && message.len() > plain.len(), "{message}");
    }

    #[test]
    fn ok_and_apply_keep_the_chosen_theme() {
        let mut fake = Fake::new();
        let mut choice = opened(&mut fake);
        choice.select("monokai", &mut |id: &str, params| fake.dispatch(id, params));
        choice.commit();
        choice.cancel(&mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(fake.current, "monokai", "Cancel after Apply keeps what was applied");
        assert_eq!(fake.selects, ["monokai"]);
    }

    #[test]
    fn cancel_without_a_change_selects_nothing() {
        let mut fake = Fake::new();
        let mut choice = opened(&mut fake);
        choice.cancel(&mut |id: &str, params| fake.dispatch(id, params));
        assert!(fake.selects.is_empty());
    }

    #[test]
    fn a_failed_selection_is_shown_as_an_error() {
        let mut fake = Fake::new();
        let mut choice = opened(&mut fake);
        choice.select("broken", &mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(choice.message, Some(ThemeMessage::Error("the theme has errors".to_owned())));
        assert_eq!(fake.current, "zen-dark");
    }

    #[test]
    fn built_in_themes_come_first_and_broken_ones_are_disabled() {
        let mut fake = Fake::new();
        let choice = opened(&mut fake);
        let rows = theme_rows(choice.list.as_ref().unwrap());
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["zen-dark", "broken", "monokai"]);
        assert!(rows[0].selected && !rows[0].user && rows[0].enabled && rows[0].hint.is_none());
        assert!(rows[1].user && !rows[1].enabled);
        assert!(rows[1].hint.as_deref().is_some_and(|hint| hint.contains("line 3: expected `,`")));
        assert!(rows[2].enabled && !rows[2].selected);
    }

    #[test]
    fn an_import_selects_the_theme_and_reports_what_was_carried_over() {
        let mut fake = Fake::new();
        let mut choice = opened(&mut fake);
        choice.import(Path::new("/downloads/Monokai.icls"), &mut |id: &str, params| fake.dispatch(id, params));
        let expected = imported_message(&ThemeImported {
            name: "Monokai".to_owned(),
            mapped: 12,
            unmapped: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
            adjusted: Vec::new(),
        });
        assert_eq!(choice.message, Some(ThemeMessage::Info(expected)));
        assert_eq!(choice.list.as_ref().map(|list| list.current.as_str()), Some("monokai"), "the list is read again");
        choice.import(Path::new("/downloads/notes.txt"), &mut |id: &str, params| fake.dispatch(id, params));
        assert!(
            matches!(&choice.message, Some(ThemeMessage::Error(text)) if text.contains("not a theme file")),
            "{:?}",
            choice.message
        );
    }

    #[test]
    fn the_import_message_counts_with_plurals() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let message = |mapped, unmapped: usize| {
            imported_message(&ThemeImported {
                name: "Monokai".to_owned(),
                mapped,
                unmapped: vec![String::new(); unmapped],
                adjusted: Vec::new(),
            })
        };
        let text = message(12, 3);
        assert!(text.contains("Monokai") && text.contains("12") && text.contains('3'), "{text}");
        assert_ne!(message(1, 0).replace('1', "2"), message(2, 0), "one colour is singular");
        assert_ne!(message(5, 1).replace('1', "2"), message(5, 2), "one entry is singular");
    }

    #[test]
    fn escape_on_the_appearance_page_reverts_the_preview() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut fake = Fake::new();
        let mut dialog = SettingsDialog::default();
        dialog.open();
        dialog.section = PageId::Appearance;
        let settings = IdeSettings::default();
        let context = SettingsContext {
            settings: &settings,
            mcp_url: None,
            product_id: "gogarden",
            executable: None,
            form: None,
            sections: crate::ide::settings::SECTIONS,
            language_servers: None,
            declared: &[],
            folding: &[],
        };
        let mut screen = crate::offscreen::Offscreen::new(vec2(1000.0, 700.0), 1.0);
        let palette = IdePalette::zen_dark();
        let image = screen.render(|ui| {
            dialog.show(ui, &palette, &context, Some(&mut |id: &str, params| fake.dispatch(id, params)));
        });
        assert!(image.width() > 0);
        assert_eq!(dialog.theme.original.as_deref(), Some("zen-dark"));
        dialog.theme.select("monokai", &mut |id: &str, params| fake.dispatch(id, params));
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        screen.frame(vec![escape], |ui| {
            dialog.show(ui, &palette, &context, Some(&mut |id: &str, params| fake.dispatch(id, params)));
        });
        assert!(!dialog.open);
        assert_eq!(fake.current, "zen-dark");
    }

    fn font_row_pass(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        size: &mut Option<f32>,
    ) -> (egui::Response, egui::Response) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 200.0))),
            events,
            ..egui::RawInput::default()
        };
        let mut responses = None;
        let mut output = ctx.run_ui(input, |ui| {
            responses = Some(ui_font_row(ui, &IdePalette::zen_dark(), None, size));
        });
        output.textures_delta.clear();
        responses.unwrap()
    }

    fn click(at: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    }

    #[test]
    fn the_size_field_is_on_only_with_a_custom_font() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut size = None;
        let (check, field) = font_row_pass(&ctx, Vec::new(), &mut size);
        assert!(check.rect.width() > 0.0 && field.rect.width() > 0.0, "both controls are drawn");
        assert!(check.rect.right() <= field.rect.left(), "the size follows the check box on its row");
        assert!(!field.enabled(), "the size waits for the check box");
        let (_, field) = font_row_pass(&ctx, click(check.rect.center()), &mut size);
        assert_eq!(size, Some(13.0), "a custom font starts at 13 points");
        let (check, field_after) = font_row_pass(&ctx, Vec::new(), &mut size);
        assert!(field_after.enabled(), "{field:?}");
        let _ = font_row_pass(&ctx, click(check.rect.center()), &mut size);
        assert_eq!(size, None, "without a custom font the UI takes 13 points again");
    }

    fn zoom_row_pass(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        zoom: &mut u16,
    ) -> (egui::Response, egui::Response, Option<egui::Response>) {
        zoom_row_pass_in(ctx, 800.0, events, zoom)
    }

    fn zoom_row_pass_in(
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
        zoom: &mut u16,
    ) -> (egui::Response, egui::Response, Option<egui::Response>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 200.0))),
            events,
            ..egui::RawInput::default()
        };
        let mut row = None;
        let mut output = ctx.run_ui(input, |ui| {
            row = Some(zoom_row(ui, &IdePalette::zen_dark(), None, zoom));
        });
        output.textures_delta.clear();
        row.unwrap()
    }

    #[test]
    fn reset_to_default_shows_only_while_the_zoom_is_changed() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut zoom = 100;
        let (presets, custom, reset) = zoom_row_pass(&ctx, Vec::new(), &mut zoom);
        assert!(reset.is_none(), "no link at 100%");
        assert!(presets.rect.right() <= custom.rect.left(), "the custom value follows the presets");
        zoom = 125;
        let (_, _, reset) = zoom_row_pass(&ctx, Vec::new(), &mut zoom);
        let link = reset.expect("the link while the zoom is changed");
        let _ = zoom_row_pass(&ctx, click(link.rect.center()), &mut zoom);
        assert_eq!(zoom, 100, "Reset to default goes back to 100%");
        assert!(zoom_row_pass(&ctx, Vec::new(), &mut zoom).2.is_none());

        zoom = 150;
        let narrow = egui::Context::default();
        theme::install(&narrow);
        let (_, custom, reset) = zoom_row_pass_in(&narrow, 330.0, Vec::new(), &mut zoom);
        let link = reset.expect("the link on a narrow page");
        assert!(link.rect.height() < 2.0 * custom.rect.height(), "the link keeps one line: {link:?}");
        assert!(link.rect.right() <= 330.0, "it moves to the next row whole: {link:?}");
    }

    #[test]
    fn the_zoom_presets_run_from_70_to_200_percent() {
        assert_eq!(IDE_ZOOM_PRESETS.first(), Some(&70));
        assert_eq!(IDE_ZOOM_PRESETS.last(), Some(&200));
        assert!(IDE_ZOOM_PRESETS.contains(&DEFAULT_IDE_ZOOM));
        assert!(IDE_ZOOM_PRESETS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(IDE_ZOOM_PRESETS.iter().all(|preset| IDE_ZOOMS.contains(preset)));
    }

    fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.visual_bounding_rect())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_appearance_page_draws_the_ui_font_size_and_ok_applies_it() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut fake = Fake::new();
        let mut dialog = SettingsDialog::default();
        dialog.open();
        dialog.section = PageId::Appearance;
        let settings = IdeSettings::default();
        let context = SettingsContext {
            settings: &settings,
            mcp_url: None,
            product_id: "gogarden",
            executable: None,
            form: None,
            sections: crate::ide::settings::SECTIONS,
            language_servers: None,
            declared: &[],
            folding: &[],
        };
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let palette = IdePalette::zen_dark();
        let mut pass = |dialog: &mut SettingsDialog, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 700.0))),
                events,
                ..egui::RawInput::default()
            };
            let mut applied = None;
            let mut output = ctx.run_ui(input, |ui| {
                applied = dialog.show(ui, &palette, &context, Some(&mut |id: &str, params| fake.dispatch(id, params)));
            });
            output.textures_delta.clear();
            (texts(&output), applied)
        };
        let _ = pass(&mut dialog, Vec::new());
        let (shown, _) = pass(&mut dialog, Vec::new());
        for label in [t!("settings-ui-font-custom"), t!("settings-ui-font-size"), t!("settings-ide-zoom")] {
            assert!(shown.iter().any(|(text, _)| *text == label), "{label} is on the page: {shown:?}");
        }
        dialog.draft.as_mut().unwrap().ui_font_size = Some(15.0);
        dialog.draft.as_mut().unwrap().ide_zoom = 150;
        let (shown, _) = pass(&mut dialog, Vec::new());
        assert!(shown.iter().any(|(text, _)| text == "15"), "the field shows the draft's size: {shown:?}");
        let ok = shown.iter().find(|(text, _)| *text == t!("settings-ok")).map(|(_, rect)| rect.center()).unwrap();
        let (_, applied) = pass(&mut dialog, click(ok));
        assert_eq!(applied.as_ref().and_then(|settings| settings.ui_font_size), Some(15.0), "OK applies the draft");
        assert_eq!(applied.map(|settings| settings.ide_zoom), Some(150), "with the zoom");
        assert!(!dialog.open);
    }
}

use egui::{Rect, Ui, vec2};
use serde_json::{Value, json};
use zen_i18n::t;

use super::super::settings::{DEFAULT_UI_FONT_SIZE, Dispatch, IdeSettings, PageId, UI_FONT_SIZES};
use super::{Overview, Request, WelcomeView};
use crate::theme;

const REFRESH: f64 = 1.0;
const FIELD: f32 = 240.0;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Choice {
    id: String,
    name: String,
    enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Lists {
    theme: String,
    themes: Vec<Choice>,
    follow_system: bool,
    keymap: String,
    keymaps: Vec<Choice>,
}

#[derive(Debug, Default)]
pub(super) struct Customize {
    lists: Option<(f64, Lists)>,
    error: Option<String>,
}

impl Customize {
    pub(super) fn refresh(&mut self) {
        self.lists = None;
    }
}

fn choices(list: &Value) -> Vec<Choice> {
    let text = |row: &Value, key: &str| row[key].as_str().unwrap_or_default().to_owned();
    list.as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| Choice { id: text(row, "id"), name: text(row, "name"), enabled: row["error"].is_null() })
                .collect()
        })
        .unwrap_or_default()
}

fn load(dispatch: &mut Dispatch<'_>) -> Result<Lists, String> {
    let themes = dispatch("ui.theme.list", json!({}))?;
    let keymaps = dispatch("ui.keymap.list", json!({}))?;
    Ok(Lists {
        theme: themes["current"].as_str().unwrap_or_default().to_owned(),
        themes: choices(&themes["themes"]),
        follow_system: themes["appearance"]["followSystem"].as_bool().unwrap_or(false),
        keymap: keymaps["current"].as_str().unwrap_or_default().to_owned(),
        keymaps: choices(&keymaps["keymaps"]),
    })
}

fn heading(ui: &mut Ui, color: egui::Color32, text: String) -> egui::Response {
    ui.add_space(14.0);
    let label = ui.label(egui::RichText::new(text).font(theme::semibold(13.0)).color(color));
    ui.add_space(4.0);
    label
}

fn combo(ui: &mut Ui, id: &str, title: &egui::Response, current: &str, rows: &[Choice]) -> Option<String> {
    let name = rows.iter().find(|row| row.id == current).map_or(current, |row| row.name.as_str());
    let mut chosen = None;
    let combo = egui::ComboBox::from_id_salt(id).selected_text(name).width(FIELD).truncate().show_ui(ui, |ui| {
        for row in rows {
            let selected = row.id == current;
            let item = ui.add_enabled(row.enabled, egui::Button::selectable(selected, &row.name));
            if item.clicked() && !selected {
                chosen = Some(row.id.clone());
            }
        }
    });
    let _ = combo.response.labelled_by(title.id);
    chosen
}

impl WelcomeView {
    pub(super) fn customize(
        &mut self,
        ui: &mut Ui,
        main: Rect,
        overview: &Overview,
        dispatch: &mut Dispatch<'_>,
        requests: &mut Vec<Request>,
    ) {
        let p = self.palette;
        let now = ui.input(|input| input.time);
        let fresh = self.customize.lists.as_ref().is_some_and(|(at, _)| now >= *at && now - at < REFRESH);
        if !fresh {
            match load(dispatch) {
                Ok(lists) => self.customize.lists = Some((now, lists)),
                Err(error) => self.customize.error = Some(error),
            }
        }
        let lists = self.customize.lists.as_ref().map(|(_, lists)| lists.clone()).unwrap_or_default();
        let mut pane = ui.new_child(egui::UiBuilder::new().max_rect(main.shrink2(vec2(20.0, 2.0))));
        let ui = &mut pane;
        let mut change = None;

        let title = heading(ui, p.text, t!("ide-welcome-customize-theme"));
        ui.horizontal(|ui| {
            if let Some(id) = combo(ui, "welcome-theme", &title, &lists.theme, &lists.themes) {
                change = Some(("ui.theme.select", json!({ "id": id })));
            }
            let mut follow = lists.follow_system;
            if ui.checkbox(&mut follow, t!("settings-theme-sync")).changed() {
                change = Some(("ui.theme.followSystem", json!({ "enabled": follow })));
            }
        });

        let title = heading(ui, p.text, t!("ide-welcome-customize-font"));
        let mut size = overview.settings.ui_font_size.unwrap_or(DEFAULT_UI_FONT_SIZE);
        let field = ui.add(egui::DragValue::new(&mut size).range(UI_FONT_SIZES).speed(0.25).fixed_decimals(0));
        let field = field.labelled_by(title.id);
        if field.changed() && Some(size.round()) != overview.settings.ui_font_size {
            requests.push(Request::Settings(Box::new(IdeSettings {
                ui_font_size: Some(size.round()),
                ..overview.settings.clone()
            })));
        }

        let title = heading(ui, p.text, t!("ide-welcome-customize-keymap"));
        ui.horizontal(|ui| {
            if let Some(id) = combo(ui, "welcome-keymap", &title, &lists.keymap, &lists.keymaps) {
                change = Some(("ui.keymap.select", json!({ "id": id })));
            }
            if ui.link(t!("ide-welcome-customize-configure")).clicked() {
                self.wanted_settings = Some(Some(PageId::Keymap));
            }
        });

        heading(ui, p.text, t!("settings-accessibility"));
        let mut always = true;
        ui.add_enabled(false, egui::Checkbox::new(&mut always, t!("ide-welcome-customize-screen-readers")));
        ui.label(egui::RichText::new(t!("ide-welcome-customize-screen-readers-note")).color(p.text_secondary));

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            if ui.link(t!("keymap-view-ideSettings-import")).clicked() {
                self.wanted_import = Some(json!({}));
            }
            ui.add_space(12.0);
            if ui.link(t!("ide-welcome-customize-all-settings")).clicked() {
                self.wanted_settings = Some(None);
            }
        });
        if let Some((id, params)) = change {
            self.customize.error = dispatch(id, params).err();
            self.customize.refresh();
        }
        if let Some(error) = &self.customize.error {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(error).color(p.error_text));
        }
    }
}

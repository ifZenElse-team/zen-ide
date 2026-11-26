use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::{Align2, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde::Deserialize;
use zen_core::keymap::{Binding, ImportWarning, Keymap, KeymapImport, Keystroke, Unmapped, UnmappedReason};
use zen_i18n::t;

use super::IdePalette;
use super::settings::Dispatch;
use crate::{accessibility, theme};

pub(super) use zen_core::keymap::{GROUPS, group_of};

static PRODUCT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

const WHATS_NEW: &str = "view.whatsNew.open";

pub(super) fn set_product(name: &str) {
    let _ = PRODUCT.set(name.to_owned());
}

fn product() -> String {
    PRODUCT.get().cloned().unwrap_or_else(|| "Zen IDE".to_owned())
}

pub(super) fn action_title(action: &str) -> String {
    if action == WHATS_NEW {
        return t!("keymap-view-whatsNew-open", product = product());
    }
    if let Some(name) = super::external_tools::title(action) {
        return name;
    }
    if let Some(name) = action.strip_prefix(zen_core::keymap::MACRO_ACTION) {
        return name.to_owned();
    }
    let (base, extend) = match action.strip_suffix("+extend") {
        Some(base) => (base, true),
        None => (action, false),
    };
    let title = named(base, |key| t!(key));
    if extend { t!("keymap-with-selection", action = title.as_str()) } else { title }
}

fn named(base: &str, text: impl Fn(&str) -> String) -> String {
    if base == "diagnostics.log.show" && !cfg!(target_os = "macos") {
        return text(if cfg!(windows) { "ide-show-log-explorer" } else { "ide-show-log-files" });
    }
    let key = format!("keymap-{}", base.replace(['.', '/'], "-"));
    let title = text(&key);
    if title != key {
        return title;
    }
    if let Some(message) = super::search_everywhere::command_step_message(base) {
        return text(message);
    }
    let key = format!("command-{}", base.replace('.', "-"));
    let title = text(&key);
    if title == key { base.to_owned() } else { title }
}

pub(super) fn action_title_english(action: &str) -> String {
    if action == WHATS_NEW {
        return zen_i18n::source_text("keymap-view-whatsNew-open", &[("product", zen_i18n::Value::from(product()))]);
    }
    if let Some(name) = super::external_tools::title(action) {
        return name;
    }
    if let Some(name) = action.strip_prefix(zen_core::keymap::MACRO_ACTION) {
        return name.to_owned();
    }
    let (base, extend) = match action.strip_suffix("+extend") {
        Some(base) => (base, true),
        None => (action, false),
    };
    let title = named(base, |key| zen_i18n::source_text(key, &[]));
    if extend {
        zen_i18n::source_text("keymap-with-selection", &[("action", zen_i18n::Value::from(title))])
    } else {
        title
    }
}

pub(super) fn action_matches(action: &str, query: &str) -> bool {
    action_title(action).to_lowercase().contains(query) || action_title_english(action).to_lowercase().contains(query)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
struct KeymapList {
    current: String,
    keymaps: Vec<KeymapRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct KeymapRow {
    id: String,
    name: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum KeymapMessage {
    Imported(KeymapImport),
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Prompt {
    Rename { id: String, name: String },
    Delete { id: String, name: String },
}

#[derive(Default)]
pub(super) struct KeymapPage {
    drafts: BTreeMap<String, Keymap>,
    list: Option<KeymapList>,
    original: Option<String>,
    message: Option<KeymapMessage>,
    prompt: Option<Prompt>,
    search: String,
    selected: Option<String>,
    recording: Option<(String, Option<Keystroke>)>,
    abbreviations: BTreeMap<String, Vec<String>>,
    abbreviating: Option<(String, String)>,
}

fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_keymap_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(t!("settings-keymap-import-title"))
        .add_filter(t!("settings-keymap-files"), &["xml"])
        .pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_keymap_file() -> Option<PathBuf> {
    None
}

fn reason_text(reason: UnmappedReason) -> String {
    match reason {
        UnmappedReason::UnknownAction => t!("settings-keymap-reason-unknown-action"),
        UnmappedReason::SecondKeystroke => t!("settings-keymap-reason-second-keystroke"),
        UnmappedReason::MouseShortcut => t!("settings-keymap-reason-mouse"),
        UnmappedReason::UnknownKey => t!("settings-keymap-reason-unknown-key"),
    }
}

fn imported_message(report: &KeymapImport) -> String {
    t!(
        "settings-keymap-imported",
        name = report.name.as_str(),
        mapped = report.mapped,
        unmapped = report.unmapped.len()
    )
}

fn unmapped_line(entry: &Unmapped) -> String {
    let reason = reason_text(entry.reason);
    match &entry.shortcut {
        Some(shortcut) => format!("{} ({shortcut}): {reason}", entry.action_id),
        None => format!("{}: {reason}", entry.action_id),
    }
}

impl KeymapPage {
    pub(super) fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub(super) fn search(&self) -> &str {
        &self.search
    }

    pub(super) fn set_search(&mut self, text: &str) {
        text.clone_into(&mut self.search);
    }

    pub(super) fn reset(&mut self) {
        *self = Self { search: std::mem::take(&mut self.search), ..Self::default() };
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        let _ = dispatch("ui.keymap.reload", serde_json::json!({}));
        self.refresh(dispatch);
        self.original = self.list.as_ref().map(|list| list.current.clone());
    }

    pub(super) fn commit(&mut self) {
        for (id, keymap) in &self.drafts {
            let saved = crate::keymap::keymap(id);
            if saved.as_ref().map(Keymap::user_changes) == Some(keymap.user_changes()) {
                continue;
            }
            if let Err(error) = crate::keymap::replace(id, keymap.user_changes().clone()) {
                self.message = Some(KeymapMessage::Error(error));
            }
        }
        if let Some(list) = &self.list {
            self.original = Some(list.current.clone());
        }
    }

    pub(super) fn cancel(&mut self, dispatch: &mut Dispatch<'_>) {
        if let Some(original) = self.original.take()
            && self.list.as_ref().is_some_and(|list| list.current != original)
        {
            let _ = dispatch("ui.keymap.select", serde_json::json!({ "id": original }));
        }
        self.reset();
    }

    fn refresh(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("ui.keymap.list", serde_json::json!({})).and_then(parse::<KeymapList>) {
            Ok(list) => self.list = Some(list),
            Err(error) => self.message = Some(KeymapMessage::Error(error)),
        }
        self.read_abbreviations(dispatch);
    }

    fn read_abbreviations(&mut self, dispatch: &mut Dispatch<'_>) {
        if let Ok(listed) = dispatch("ui.keymap.abbreviations", serde_json::json!({})) {
            self.abbreviations = serde_json::from_value(listed["abbreviations"].clone()).unwrap_or_default();
        }
    }

    fn change_abbreviation(&mut self, dispatch: &mut Dispatch<'_>, command: &str, action: &str, abbreviation: &str) {
        let params = serde_json::json!({ "action": action, "abbreviation": abbreviation });
        match dispatch(command, params) {
            Ok(changed) => {
                self.abbreviations = serde_json::from_value(changed["abbreviations"].clone()).unwrap_or_default();
            }
            Err(error) => self.message = Some(KeymapMessage::Error(error)),
        }
    }

    fn shown_id(&self) -> String {
        self.list.as_ref().map_or_else(crate::keymap::current_id, |list| list.current.clone())
    }

    fn run(&mut self, dispatch: &mut Dispatch<'_>, id: &str, params: serde_json::Value) -> Option<serde_json::Value> {
        let result = dispatch(id, params);
        self.refresh(dispatch);
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.message = Some(KeymapMessage::Error(error));
                None
            }
        }
    }

    fn select(&mut self, id: &str, dispatch: &mut Dispatch<'_>) {
        self.message = None;
        self.selected = None;
        let _ = self.run(dispatch, "ui.keymap.select", serde_json::json!({ "id": id }));
    }

    fn duplicate(&mut self, dispatch: &mut Dispatch<'_>) {
        let id = self.shown_id();
        let name = self.shown_name();
        let copy = t!("settings-keymap-copy-name", name = name.as_str());
        let created = self.run(dispatch, "ui.keymap.duplicate", serde_json::json!({ "id": id, "name": copy }));
        if let Some(new_id) = created.as_ref().and_then(|created| created["id"].as_str()).map(str::to_owned) {
            if let Some(draft) = self.drafts.get(&id).cloned() {
                self.drafts.insert(new_id.clone(), draft);
            }
            self.select(&new_id, dispatch);
        }
    }

    fn import(&mut self, path: &Path, dispatch: &mut Dispatch<'_>) {
        let result = dispatch("ui.keymap.import", serde_json::json!({ "path": path, "select": true }))
            .and_then(parse::<KeymapImport>);
        self.message = Some(match result {
            Ok(report) => KeymapMessage::Imported(report),
            Err(error) => KeymapMessage::Error(t!("settings-keymap-import-failed", error = error)),
        });
        self.refresh(dispatch);
    }

    fn shown_name(&self) -> String {
        let id = self.shown_id();
        self.list
            .as_ref()
            .and_then(|list| list.keymaps.iter().find(|row| row.id == id))
            .map_or_else(|| t!("workbench-keymap-macos"), |row| row.name.clone())
    }

    fn chooser(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let rows = self.list.as_ref().map(|list| list.keymaps.clone()).unwrap_or_default();
        let shown = self.shown_id();
        let builtin = rows.iter().find(|row| row.id == shown).is_none_or(|row| row.source != "user");
        let mut chosen = None;
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            let title = ui.label(egui::RichText::new(t!("settings-keymap-label")).color(p.text));
            let gear_room = 32.0;
            let combo = egui::ComboBox::from_id_salt("settings-keymap")
                .selected_text(self.shown_name())
                .width(super::settings::widgets::field_width(ui, 140.0 + gear_room, 260.0 + gear_room) - gear_room)
                .truncate()
                .show_ui(ui, |ui| {
                    for row in &rows {
                        let response =
                            ui.add_enabled(row.error.is_none(), egui::Button::selectable(row.id == shown, &row.name));
                        if let Some(error) = &row.error {
                            response.on_disabled_hover_text(t!("settings-keymap-broken", error = error.as_str()));
                        } else if response.clicked() && row.id != shown {
                            chosen = Some(row.id.clone());
                        }
                    }
                });
            let _ = combo.response.labelled_by(title.id);
            let (rect, gear) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            accessibility::button_with(&gear, || t!("settings-keymap-actions"));
            if gear.hovered() {
                ui.painter().rect_filled(rect, 4, p.popup_hover);
            }
            crate::icons::gear(ui.painter(), rect.shrink(4.0), p.text_secondary);
            let gear = gear.on_hover_text(t!("settings-keymap-actions"));
            egui::Popup::menu(&gear).show(|ui| {
                ui.set_min_width(180.0);
                if ui.button(t!("settings-keymap-duplicate")).clicked() {
                    action = Some("duplicate");
                }
                if ui.add_enabled(!builtin, egui::Button::new(t!("settings-keymap-rename"))).clicked() {
                    action = Some("rename");
                }
                if ui.add_enabled(!builtin, egui::Button::new(t!("settings-keymap-delete"))).clicked() {
                    action = Some("delete");
                }
                if !cfg!(target_arch = "wasm32") {
                    ui.separator();
                    if ui.button(t!("settings-keymap-import")).clicked() {
                        action = Some("import");
                    }
                }
            });
        });
        if let Some(id) = chosen {
            self.select(&id, dispatch);
        }
        match action {
            Some("duplicate") => self.duplicate(dispatch),
            Some("rename") => self.prompt = Some(Prompt::Rename { id: shown.clone(), name: self.shown_name() }),
            Some("delete") => self.prompt = Some(Prompt::Delete { id: shown.clone(), name: self.shown_name() }),
            Some("import") => {
                if let Some(path) = pick_keymap_file() {
                    self.import(&path, dispatch);
                }
            }
            _ => {}
        }
        self.report(ui, p);
        self.prompt(ui, p, dispatch);
    }

    fn report(&self, ui: &mut Ui, p: &IdePalette) {
        match &self.message {
            Some(KeymapMessage::Imported(report)) => {
                ui.label(egui::RichText::new(imported_message(report)).color(p.text_secondary));
                for warning in &report.warnings {
                    let ImportWarning::ForeignParent { parent } = warning;
                    let parent = parent.clone().unwrap_or_else(|| t!("settings-keymap-no-parent"));
                    ui.label(
                        egui::RichText::new(t!("settings-keymap-foreign-parent", parent = parent))
                            .color(p.warning_text),
                    );
                }
                if !report.unmapped.is_empty() {
                    egui::ScrollArea::vertical().id_salt("keymap-unmapped").max_height(72.0).show(ui, |ui| {
                        for entry in &report.unmapped {
                            ui.label(
                                egui::RichText::new(unmapped_line(entry))
                                    .font(theme::regular(11.5))
                                    .color(p.text_secondary),
                            );
                        }
                    });
                }
            }
            Some(KeymapMessage::Error(text)) => {
                ui.label(egui::RichText::new(text).color(p.error_text));
            }
            None => {}
        }
    }

    fn prompt(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let Some(prompt) = self.prompt.clone() else { return };
        let mut close = false;
        let mut confirmed = None;
        egui::Modal::new(ui.scope_id().with("keymap-prompt")).show(ui.ctx(), |ui| {
            ui.set_width(360.0);
            match &prompt {
                Prompt::Rename { id, name } => {
                    ui.label(
                        egui::RichText::new(t!("settings-keymap-rename-title"))
                            .font(theme::semibold(14.0))
                            .color(p.text),
                    );
                    ui.add_space(8.0);
                    let mut text = name.clone();
                    let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width()));
                    field.request_focus();
                    if text != *name {
                        self.prompt = Some(Prompt::Rename { id: id.clone(), name: text.clone() });
                    }
                    if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        confirmed = Some(prompt.clone());
                    }
                }
                Prompt::Delete { name, .. } => {
                    ui.label(
                        egui::RichText::new(t!("settings-keymap-delete-title"))
                            .font(theme::semibold(14.0))
                            .color(p.text),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(t!("settings-keymap-delete-question", name = name.as_str())).color(p.text),
                    );
                }
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(t!("settings-cancel")).clicked() {
                    close = true;
                }
                if ui
                    .add(egui::Button::new(egui::RichText::new(t!("settings-ok")).color(p.accent_text)).fill(p.accent))
                    .clicked()
                {
                    confirmed = self.prompt.clone();
                }
            });
        });
        if let Some(prompt) = confirmed {
            self.confirm(prompt, dispatch);
            close = true;
        }
        if close {
            self.prompt = None;
        }
    }

    fn confirm(&mut self, prompt: Prompt, dispatch: &mut Dispatch<'_>) {
        self.message = None;
        match prompt {
            Prompt::Rename { id, name } => {
                let _ = self.run(dispatch, "ui.keymap.rename", serde_json::json!({ "id": id, "name": name }));
            }
            Prompt::Delete { id, .. } => {
                if self.run(dispatch, "ui.keymap.delete", serde_json::json!({ "id": id })).is_some() {
                    self.drafts.remove(&id);
                }
            }
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: Option<&mut Dispatch<'_>>) {
        let mut dispatch = dispatch;
        if let Some(dispatch) = dispatch.as_mut() {
            self.chooser(ui, p, dispatch);
            ui.add_space(8.0);
        }
        let shown = self.shown_id();
        let draft = self
            .drafts
            .entry(shown.clone())
            .or_insert_with(|| crate::keymap::keymap(&shown).unwrap_or_else(crate::keymap::current));
        let bindings = draft.bindings();
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(t!("keymap-search"))
                .desired_width(ui.available_width()),
        );
        accessibility::edit(&search, |node| node.set_label(t!("keymap-search")));
        ui.add_space(8.0);
        let query = self.search.trim().to_lowercase();
        let mut action_to_record = None;
        let mut change: Option<(String, Option<Vec<Keystroke>>)> = None;
        let mut abbreviate = None;
        let mut unabbreviate: Option<(String, String)> = None;
        let abbreviations = &self.abbreviations;
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height()).show(ui, |ui| {
            for (index, group) in GROUPS.iter().enumerate() {
                let rows: Vec<&Binding> = bindings
                    .iter()
                    .filter(|binding| group_of(&binding.action) == index)
                    .filter(|binding| {
                        query.is_empty()
                            || action_matches(&binding.action, &query)
                            || binding.keystrokes.iter().any(|key| key.to_string().to_lowercase().contains(&query))
                    })
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t!(group)).font(theme::semibold(13.0)).color(p.text));
                for binding in rows {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                    let selected = self.selected.as_deref() == Some(binding.action.as_str());
                    if selected {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.popup_hover);
                    }
                    let color = if draft.is_changed(&binding.action) { p.link } else { p.text };
                    let title = action_title(&binding.action);
                    accessibility::item(&response, egui::Role::ListBoxOption, selected, &title);
                    ui.painter().text(
                        pos2(rect.left() + 20.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        title,
                        theme::regular(13.0),
                        color,
                    );
                    let mut keys: Vec<String> = binding.keystrokes.iter().map(ToString::to_string).collect();
                    keys.extend(zen_core::keymap::gesture(&binding.action).map(str::to_owned));
                    let own = abbreviations.get(&binding.action).cloned().unwrap_or_default();
                    keys.extend(
                        own.iter().map(|abbreviation| t!("keymap-abbreviation", abbreviation = abbreviation.as_str())),
                    );
                    let keys = keys.join("   ");
                    accessibility::describe(&response, &keys);
                    ui.painter().text(
                        pos2(rect.right() - 8.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        keys,
                        theme::regular(13.0),
                        p.text_secondary,
                    );
                    if response.clicked() {
                        self.selected = Some(binding.action.clone());
                    }
                    if response.double_clicked() {
                        action_to_record = Some(binding.action.clone());
                    }
                    response.context_menu(|ui| {
                        self.selected = Some(binding.action.clone());
                        if ui.button(t!("keymap-add")).clicked() {
                            action_to_record = Some(binding.action.clone());
                            ui.close();
                        }
                        for key in &binding.keystrokes {
                            if ui.button(t!("keymap-remove", shortcut = key.to_string())).clicked() {
                                let rest = binding.keystrokes.iter().filter(|other| *other != key).cloned().collect();
                                change = Some((binding.action.clone(), Some(rest)));
                                ui.close();
                            }
                        }
                        if draft.is_changed(&binding.action) && ui.button(t!("keymap-reset")).clicked() {
                            change = Some((binding.action.clone(), None));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(t!("keymap-abbreviation-add")).clicked() {
                            abbreviate = Some(binding.action.clone());
                            ui.close();
                        }
                        for abbreviation in &own {
                            let label = t!("keymap-abbreviation-remove", abbreviation = abbreviation.as_str());
                            if ui.button(label).clicked() {
                                unabbreviate = Some((binding.action.clone(), abbreviation.clone()));
                                ui.close();
                            }
                        }
                    });
                }
            }
        });
        if let Some((action, keys)) = change {
            draft.set(&action, keys);
        }
        if let Some(action) = action_to_record {
            self.recording = Some((action, None));
        }
        if let Some(action) = abbreviate {
            self.abbreviating = Some((action, String::new()));
        }
        if let Some(dispatch) = dispatch.as_mut() {
            if let Some((action, abbreviation)) = unabbreviate {
                self.change_abbreviation(dispatch, "ui.keymap.removeAbbreviation", &action, &abbreviation);
            }
            self.abbreviation_prompt(ui, p, dispatch);
        }
        self.recorder(ui, p);
    }

    fn abbreviation_prompt(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let Some((action, mut text)) = self.abbreviating.clone() else { return };
        let mut close = false;
        let mut confirmed = false;
        egui::Modal::new(ui.scope_id().with("keymap-abbreviation")).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.label(egui::RichText::new(t!("keymap-abbreviation-title")).font(theme::semibold(14.0)).color(p.text));
            ui.label(egui::RichText::new(action_title(&action)).color(p.text_secondary));
            ui.add_space(8.0);
            let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width()));
            accessibility::labelled(&field, &t!("keymap-abbreviation-title"));
            field.request_focus();
            if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                confirmed = true;
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(t!("settings-cancel")).clicked() {
                    close = true;
                }
                let ok = egui::Button::new(egui::RichText::new(t!("settings-ok")).color(p.accent_text)).fill(p.accent);
                if ui.add_enabled(!text.trim().is_empty(), ok).clicked() {
                    confirmed = true;
                }
            });
        });
        if confirmed && !text.trim().is_empty() {
            self.change_abbreviation(dispatch, "ui.keymap.addAbbreviation", &action, &text);
            close = true;
        }
        self.abbreviating = if close { None } else { Some((action, text)) };
    }

    fn recorder(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some((action, pressed)) = self.recording.clone() else { return };
        let shown = self.shown_id();
        let Some(draft) = self.drafts.get_mut(&shown) else { return };
        let mut pressed = pressed;
        let cancel = take_keystroke(ui, &mut pressed);
        let outcome = if cancel {
            Recorded::Cancelled
        } else {
            shortcut_dialog(ui, p, &action_title(&action), &action, pressed, draft)
        };
        match outcome {
            Recorded::Done { key, take_from } => {
                for other in take_from {
                    let rest = draft.keystrokes(&other).into_iter().filter(|stroke| *stroke != key).collect();
                    draft.set(&other, Some(rest));
                }
                let mut keys = draft.keystrokes(&action);
                if !keys.contains(&key) {
                    keys.push(key);
                }
                draft.set(&action, Some(keys));
                self.recording = None;
            }
            Recorded::Cancelled => self.recording = None,
            Recorded::Open(pressed) => self.recording = Some((action, pressed)),
        }
    }
}

pub(super) enum Recorded {
    Open(Option<Keystroke>),
    Cancelled,
    Done { key: Keystroke, take_from: Vec<String> },
}

pub(super) fn take_keystroke(ui: &mut Ui, pressed: &mut Option<Keystroke>) -> bool {
    let mut cancel = false;
    ui.input_mut(|input| {
        input.events.retain(|event| match event {
            egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. } if modifiers.is_none() => {
                cancel = true;
                false
            }
            egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                if let Some(stroke) = crate::keymap::recordable(*key, *physical_key, *modifiers) {
                    *pressed = Some(stroke);
                }
                false
            }
            egui::Event::Text(_) => false,
            _ => true,
        });
    });
    cancel
}

pub(super) fn shortcut_dialog(
    ui: &mut Ui,
    p: &IdePalette,
    title: &str,
    action: &str,
    pressed: Option<Keystroke>,
    keymap: &Keymap,
) -> Recorded {
    let mut outcome = Recorded::Open(pressed.clone());
    egui::Modal::new(ui.scope_id().with("keymap-recorder")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.label(egui::RichText::new(t!("keymap-recorder-title")).font(theme::semibold(14.0)).color(p.text));
        ui.label(egui::RichText::new(title).color(p.text_secondary));
        ui.add_space(10.0);
        let (field, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        ui.painter().rect_filled(field, 5, p.frame);
        ui.painter().rect_stroke(field, 5, Stroke::new(1.0, p.accent), StrokeKind::Inside);
        let (text, color) = match &pressed {
            Some(key) => (key.to_string(), p.text),
            None => (t!("keymap-recorder-press"), p.text_secondary),
        };
        ui.painter().text(
            field.left_center() + vec2(10.0, 0.0),
            Align2::LEFT_CENTER,
            text,
            theme::regular(13.0),
            color,
        );
        let others: Vec<String> = pressed
            .as_ref()
            .map(|key| keymap.actions_with(key).into_iter().filter(|other| other != action).collect())
            .unwrap_or_default();
        if !others.is_empty() {
            ui.add_space(8.0);
            let names: Vec<String> = others.iter().map(|other| action_title(other)).collect();
            ui.label(egui::RichText::new(t!("keymap-conflict", actions = names.join(", "))).color(p.warning_text));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(t!("settings-cancel")).clicked() {
                outcome = Recorded::Cancelled;
            }
            let Some(key) = pressed.clone() else { return };
            if !others.is_empty() && ui.button(t!("keymap-conflict-leave")).clicked() {
                outcome = Recorded::Done { key: key.clone(), take_from: Vec::new() };
            }
            let ok = if others.is_empty() { t!("settings-ok") } else { t!("keymap-conflict-remove") };
            if ui.add(egui::Button::new(egui::RichText::new(ok).color(p.accent_text)).fill(p.accent)).clicked() {
                outcome = Recorded::Done { key, take_from: others.clone() };
            }
        });
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    struct Fake {
        current: String,
        keymaps: Vec<(String, String)>,
        calls: Vec<String>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                current: "macos".to_owned(),
                keymaps: vec![("macos".to_owned(), "macOS".to_owned()), ("mine".to_owned(), "Mine".to_owned())],
                calls: Vec::new(),
            }
        }

        fn dispatch(&mut self, id: &str, params: Value) -> Result<Value, String> {
            self.calls.push(id.to_owned());
            let id_param = params["id"].as_str().unwrap_or_default().to_owned();
            match id {
                "ui.keymap.reload" => Ok(json!({})),
                "ui.keymap.list" => {
                    let mut keymaps: Vec<Value> = self
                        .keymaps
                        .iter()
                        .map(|(id, name)| {
                            let source = if id == "macos" { "builtin" } else { "user" };
                            json!({ "id": id, "name": name, "source": source })
                        })
                        .collect();
                    keymaps.push(json!({ "id": "broken", "name": "broken", "source": "user", "error": "line 1" }));
                    Ok(json!({ "current": self.current, "keymaps": keymaps }))
                }
                "ui.keymap.select" => {
                    if !self.keymaps.iter().any(|(known, _)| *known == id_param) {
                        return Err(format!("no keymap `{id_param}`"));
                    }
                    self.current = id_param;
                    Ok(json!({}))
                }
                "ui.keymap.import" => {
                    assert_eq!(params["select"], json!(true));
                    if params["path"].as_str().is_some_and(|path| path.ends_with(".txt")) {
                        return Err("not a keymap".to_owned());
                    }
                    self.keymaps.push(("fixture-keys".to_owned(), "Fixture Keys".to_owned()));
                    self.current = "fixture-keys".to_owned();
                    Ok(json!({
                        "id": "fixture-keys",
                        "name": "Fixture Keys",
                        "parent": "Fixture Base",
                        "shortcuts": { "editor.line.duplicate": ["⇧⌘D"] },
                        "mapped": 7,
                        "unmapped": [
                            { "actionId": "Fixture.NoSuchAction", "reason": "unknownAction" },
                            { "actionId": "Fixture.FormatAction", "reason": "secondKeystroke", "shortcut": "meta K, meta F" },
                        ],
                        "warnings": [{ "kind": "foreignParent", "parent": "Fixture Base" }],
                    }))
                }
                "ui.keymap.duplicate" => {
                    let name = params["name"].as_str().unwrap_or_default().to_owned();
                    self.keymaps.push(("copy".to_owned(), name.clone()));
                    Ok(json!({ "id": "copy", "name": name }))
                }
                "ui.keymap.rename" => {
                    let name = params["name"].as_str().unwrap_or_default().to_owned();
                    for keymap in &mut self.keymaps {
                        if keymap.0 == id_param {
                            keymap.1.clone_from(&name);
                        }
                    }
                    Ok(json!({}))
                }
                "ui.keymap.delete" => {
                    self.keymaps.retain(|(known, _)| *known != id_param);
                    if self.current == id_param {
                        self.current = "macos".to_owned();
                    }
                    Ok(json!({}))
                }
                _ => Err(format!("unknown command `{id}`")),
            }
        }
    }

    fn opened(fake: &mut Fake) -> KeymapPage {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let mut page = KeymapPage::default();
        page.begin(&mut |id: &str, params| fake.dispatch(id, params));
        page
    }

    #[test]
    fn the_combo_selects_at_once_and_cancel_goes_back() {
        let mut fake = Fake::new();
        let mut page = opened(&mut fake);
        assert_eq!(page.original.as_deref(), Some("macos"));
        assert_eq!(page.shown_name(), "macOS");
        page.select("mine", &mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(fake.current, "mine");
        assert_eq!(page.shown_id(), "mine");
        assert_eq!(page.shown_name(), "Mine");
        page.select("nope", &mut |id: &str, params| fake.dispatch(id, params));
        assert!(matches!(&page.message, Some(KeymapMessage::Error(text)) if text.contains("nope")));
        page.cancel(&mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(fake.current, "macos");
        assert!(page.list.is_none() && page.drafts.is_empty());
    }

    #[test]
    fn an_import_selects_the_keymap_and_reports_what_was_left_out() {
        let mut fake = Fake::new();
        let mut page = opened(&mut fake);
        page.import(Path::new("/downloads/Fixture Keys.xml"), &mut |id: &str, params| fake.dispatch(id, params));
        let Some(KeymapMessage::Imported(report)) = &page.message else { panic!("{:?}", page.message) };
        assert_eq!(report.mapped, 7);
        assert_eq!(page.shown_id(), "fixture-keys", "the list is read again");
        let text = imported_message(report);
        assert!(text.contains("Fixture Keys") && text.contains('7') && text.contains('2'), "{text}");
        let lines: Vec<String> = report.unmapped.iter().map(unmapped_line).collect();
        assert!(lines[0].starts_with("Fixture.NoSuchAction: "), "{lines:?}");
        assert!(lines[1].contains("Fixture.FormatAction (meta K, meta F)"), "{lines:?}");
        assert_ne!(reason_text(UnmappedReason::MouseShortcut), reason_text(UnmappedReason::UnknownKey));
        page.import(Path::new("/downloads/notes.txt"), &mut |id: &str, params| fake.dispatch(id, params));
        assert!(matches!(&page.message, Some(KeymapMessage::Error(text)) if text.contains("not a keymap")));
    }

    #[test]
    fn duplicate_rename_and_delete_go_through_commands() {
        let mut fake = Fake::new();
        let mut page = opened(&mut fake);
        page.select("mine", &mut |id: &str, params| fake.dispatch(id, params));
        let mut edited = Keymap::default_macos();
        edited.set("editor.line.duplicate", Some(Vec::new()));
        page.drafts.insert("mine".to_owned(), edited.clone());
        page.duplicate(&mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(fake.current, "copy");
        assert_eq!(page.shown_name(), t!("settings-keymap-copy-name", name = "Mine"));
        assert_eq!(
            page.drafts.get("copy").map(Keymap::user_changes),
            Some(edited.user_changes()),
            "the edits come along"
        );
        let rename = Prompt::Rename { id: "copy".to_owned(), name: "Renamed".to_owned() };
        page.confirm(rename, &mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(page.shown_name(), "Renamed");
        let delete = Prompt::Delete { id: "copy".to_owned(), name: "Renamed".to_owned() };
        page.confirm(delete, &mut |id: &str, params| fake.dispatch(id, params));
        assert_eq!(page.shown_id(), "macos");
        assert!(!page.drafts.contains_key("copy"));
        assert!(
            fake.calls.contains(&"ui.keymap.rename".to_owned()) && fake.calls.contains(&"ui.keymap.delete".to_owned())
        );
    }

    #[test]
    fn the_keymap_page_draws_the_combo_and_the_report() {
        use crate::ide::settings::{IdeSettings, PageId, SettingsContext, SettingsDialog};
        let mut fake = Fake::new();
        let _ = opened(&mut fake);
        let mut dialog = SettingsDialog::default();
        dialog.open();
        dialog.show_page(PageId::Keymap);
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
        assert!(fake.calls.contains(&"ui.keymap.list".to_owned()), "{:?}", fake.calls);
        dialog
            .keymap_page()
            .import(Path::new("/downloads/Fixture Keys.xml"), &mut |id: &str, params| fake.dispatch(id, params));
        let image = screen.render(|ui| {
            dialog.show(ui, &palette, &context, Some(&mut |id: &str, params| fake.dispatch(id, params)));
        });
        assert!(image.width() > 0);
        assert_eq!(dialog.keymap_page().shown_id(), "fixture-keys");
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
        assert_eq!(fake.current, "macos", "Cancel goes back to the keymap the dialog opened with");
    }

    #[test]
    fn abbreviations_are_added_shown_and_removed_and_commands_are_named() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        for language in languages {
            zen_i18n::set_language(Some(language));
            let data = tempfile::tempdir().unwrap();
            let store: zen_workbench::keymaps::SharedKeymaps = std::sync::Arc::new(std::sync::Mutex::new(
                zen_workbench::keymaps::KeymapStore::new(Some(data.path().to_owned()), Vec::new()),
            ));
            let mut registry = zen_core::Registry::new();
            zen_workbench::keymaps::register(&mut registry, &store).unwrap();
            let mut session = zen_core::Session::new();
            let mut dispatch = |id: &str, params: Value| -> Result<Value, String> {
                registry.dispatch(&mut session, id, params).map_err(|error| error.to_string())
            };
            let mut page = KeymapPage::default();
            page.begin(&mut dispatch);
            page.change_abbreviation(&mut dispatch, "ui.keymap.addAbbreviation", "editor.line.duplicate", "dl");
            assert_eq!(page.abbreviations.get("editor.line.duplicate"), Some(&vec!["dl".to_owned()]));
            page.search = action_title("editor.line.duplicate");
            page.abbreviating = Some(("editor.line.duplicate".to_owned(), "dup".to_owned()));
            let palette = IdePalette::zen_dark();
            let mut screen = crate::offscreen::Offscreen::new(vec2(800.0, 500.0), 1.5);
            palette.apply(screen.context());
            screen.set_background(palette.editor_background);
            let image = screen.render(|ui| page.show(ui, &palette, Some(&mut dispatch)));
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("keymap-abbreviation-{language}.png")), &image).unwrap();
            }
            let tree = screen.accessibility_tree(|ui| page.show(ui, &palette, Some(&mut dispatch)));
            let labels: Vec<String> =
                tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect();
            assert!(labels.contains(&t!("keymap-abbreviation-title")), "{labels:?}");
            page.abbreviating = None;
            page.change_abbreviation(&mut dispatch, "ui.keymap.removeAbbreviation", "editor.line.duplicate", "dl");
            assert!(page.abbreviations.is_empty());

            let shortcut = serde_json::json!({ "action": "search.history.clear", "keystrokes": ["⌃⌥⇧H"] });
            dispatch("ui.keymap.setShortcut", shortcut).unwrap();
            let mut page = KeymapPage::default();
            page.begin(&mut dispatch);
            let active = zen_workbench::keymaps::lock(&store).active().clone();
            page.drafts.insert(page.shown_id(), active);
            page.search = action_title("search.history.clear");
            let image = screen.render(|ui| page.show(ui, &palette, Some(&mut dispatch)));
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("keymap-command-shortcut-{language}.png")), &image)
                    .unwrap();
            }
            let tree = screen.accessibility_tree(|ui| page.show(ui, &palette, Some(&mut dispatch)));
            let described: Vec<String> =
                tree.nodes.iter().filter_map(|(_, node)| node.description().map(str::to_owned)).collect();
            assert!(described.iter().any(|keys| keys == "⌃⌥⇧H"), "Settings → Keymap lists it: {described:?}");
        }
        zen_i18n::set_language(Some("en"));
        assert_eq!(action_title("search.history.clear"), t!("command-search-history-clear"));
        assert_eq!(action_title("editor.image.zoom/in"), t!("ide-image-zoom-in"));
        assert_eq!(action_title_english("search.history.clear"), "Clear Search Everywhere History");
    }

    #[test]
    fn every_action_has_a_name() {
        let english = include_str!("../../locales/en/ui.ftl");
        for binding in Keymap::default_macos().bindings() {
            let base = binding.action.trim_end_matches("+extend").replace(['.', '/'], "-");
            assert!(english.contains(&format!("\nkeymap-{base} = ")), "no name for {}", binding.action);
        }
    }

    #[test]
    fn every_action_is_in_a_group() {
        assert_eq!(group_of("view.findInFiles.open"), 3);
        assert_eq!(group_of("editor.selection.selectAll"), 5);
        assert_eq!(group_of("editor.selection.selectAllOccurrences"), 6);
        assert_eq!(group_of("editor.caret.move/left+extend"), 4);
        assert_eq!(group_of("editor.edit.undo"), 0);
    }
}

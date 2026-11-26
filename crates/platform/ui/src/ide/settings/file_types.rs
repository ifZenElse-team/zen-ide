use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::file_types::{FileTypeInfo, FileTypes};

use super::Dispatch;
use crate::ide::IdePalette;
use crate::ide::glyphs::{glyph_minus, glyph_pencil, glyph_plus};
use crate::{accessibility, file_icons, theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ide) enum FileTypesTab {
    #[default]
    Recognized,
    Ignored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListKind {
    Patterns,
    Hashbangs,
    Ignored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Editing {
    list: ListKind,
    index: Option<usize>,
    text: String,
    focus: bool,
}

#[derive(Default)]
pub(in crate::ide) struct FileTypesPage {
    pub(in crate::ide) tab: FileTypesTab,
    original: Option<FileTypes>,
    types: Vec<FileTypeInfo>,
    ignored: Vec<String>,
    selected_type: usize,
    selected: Option<(ListKind, usize)>,
    editing: Option<Editing>,
    message: Option<String>,
}

const ROW: f32 = 22.0;

impl FileTypesPage {
    pub(in crate::ide) fn reset(&mut self) {
        let tab = self.tab;
        *self = Self { tab, ..Self::default() };
    }

    pub(in crate::ide) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("editor.fileTypes.list", json!({}))
            .and_then(|value| serde_json::from_value::<FileTypes>(value).map_err(|error| error.to_string()))
        {
            Ok(mut listed) => {
                listed.types.sort_by_key(|ty| ty.name.to_lowercase());
                self.types.clone_from(&listed.types);
                self.ignored.clone_from(&listed.ignored);
                self.original = Some(listed);
            }
            Err(error) => self.message = Some(error),
        }
        self.selected_type = self.selected_type.min(self.types.len().saturating_sub(1));
    }

    pub(in crate::ide) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        let Some(original) = self.original.clone() else { return };
        let mut failed = None;
        let mut run = |command: &str, params: serde_json::Value| {
            if let Err(error) = dispatch(command, params) {
                failed = Some(error);
            }
        };
        for (kind, remove, add) in [
            (ListKind::Patterns, "editor.fileTypes.removePattern", "editor.fileTypes.addPattern"),
            (ListKind::Hashbangs, "editor.fileTypes.removeHashbang", "editor.fileTypes.addHashbang"),
        ] {
            let list =
                |ty: &FileTypeInfo| if kind == ListKind::Patterns { ty.patterns.clone() } else { ty.hashbangs.clone() };
            for before in &original.types {
                let now = self.types.iter().find(|ty| ty.id == before.id).map(list).unwrap_or_default();
                let had = |id: &str, pattern: &String| {
                    original.types.iter().any(|ty| ty.id == id && list(ty).contains(pattern))
                };
                let moved = |pattern: &String| {
                    self.types
                        .iter()
                        .any(|ty| ty.id != before.id && list(ty).contains(pattern) && !had(&ty.id, pattern))
                };
                for gone in list(before).iter().filter(|pattern| !now.contains(pattern) && !moved(pattern)) {
                    run(remove, json!({ "type": before.id, "pattern": gone }));
                }
            }
            for now in &self.types {
                let before = original.types.iter().find(|ty| ty.id == now.id).map(list).unwrap_or_default();
                for new in list(now).iter().filter(|pattern| !before.contains(pattern)) {
                    run(add, json!({ "type": now.id, "pattern": new }));
                }
            }
        }
        if original.ignored != self.ignored {
            run("editor.fileTypes.setIgnored", json!({ "patterns": self.ignored }));
        }
        let selected = self.types.get(self.selected_type).map(|ty| ty.id.clone());
        self.load(dispatch);
        if let Some(id) = selected {
            self.selected_type = self.types.iter().position(|ty| ty.id == id).unwrap_or(0);
        }
        self.message = failed;
    }

    fn items(&self, list: ListKind) -> &[String] {
        match list {
            ListKind::Patterns => self.types.get(self.selected_type).map_or(&[], |ty| ty.patterns.as_slice()),
            ListKind::Hashbangs => self.types.get(self.selected_type).map_or(&[], |ty| ty.hashbangs.as_slice()),
            ListKind::Ignored => &self.ignored,
        }
    }

    fn items_mut(&mut self, list: ListKind) -> Option<&mut Vec<String>> {
        match list {
            ListKind::Patterns => self.types.get_mut(self.selected_type).map(|ty| &mut ty.patterns),
            ListKind::Hashbangs => self.types.get_mut(self.selected_type).map(|ty| &mut ty.hashbangs),
            ListKind::Ignored => Some(&mut self.ignored),
        }
    }

    fn owner(&self, list: ListKind, text: &str) -> Option<&FileTypeInfo> {
        let current = self.types.get(self.selected_type).map(|ty| ty.id.as_str());
        self.types.iter().filter(|ty| Some(ty.id.as_str()) != current).find(|ty| match list {
            ListKind::Patterns => ty.patterns.iter().any(|own| own == text),
            ListKind::Hashbangs => ty.hashbangs.iter().any(|own| own == text),
            ListKind::Ignored => false,
        })
    }

    fn finish_editing(&mut self) {
        let Some(editing) = self.editing.take() else { return };
        let text = editing.text.trim().to_owned();
        if text.is_empty() || text.contains('/') {
            return;
        }
        if let Some(owner) = self.owner(editing.list, &text).map(|ty| ty.id.clone())
            && let Some(other) = self.types.iter_mut().find(|ty| ty.id == owner)
        {
            match editing.list {
                ListKind::Patterns => other.patterns.retain(|own| *own != text),
                ListKind::Hashbangs => other.hashbangs.retain(|own| *own != text),
                ListKind::Ignored => {}
            }
        }
        let list = editing.list;
        let Some(items) = self.items_mut(list) else { return };
        let index = match editing.index.filter(|index| *index < items.len()) {
            Some(index) => {
                items[index] = text.clone();
                index
            }
            None => {
                items.push(text.clone());
                items.len() - 1
            }
        };
        let mut seen = Vec::new();
        items.retain(|item| {
            let first = !seen.contains(item);
            seen.push(item.clone());
            first
        });
        let index = items.iter().position(|item| *item == text).unwrap_or(index);
        self.selected = Some((list, index));
    }

    #[cfg(test)]
    pub(in crate::ide) fn start_adding(&mut self, ignored: bool, text: &str) {
        let list = if ignored { ListKind::Ignored } else { ListKind::Patterns };
        self.editing = Some(Editing { list, index: None, text: text.to_owned(), focus: true });
    }

    #[cfg(test)]
    pub(in crate::ide) fn select_type(&mut self, name: &str) -> bool {
        match self.types.iter().position(|ty| ty.name == name || ty.id == name) {
            Some(index) => {
                self.selected_type = index;
                self.selected = None;
                self.editing = None;
                true
            }
            None => false,
        }
    }

    pub(in crate::ide) fn show(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.horizontal_wrapped(|ui| {
            for (tab, key) in [
                (FileTypesTab::Recognized, "settings-file-types-recognized"),
                (FileTypesTab::Ignored, "settings-file-types-ignored"),
            ] {
                let chosen = self.tab == tab;
                let text = egui::RichText::new(t!(key)).color(if chosen { p.text } else { p.text_secondary });
                let response = ui.add(egui::Button::selectable(chosen, text));
                accessibility::item(&response, egui::Role::Tab, chosen, &t!(key));
                if response.clicked() && !chosen {
                    self.tab = tab;
                    self.selected = None;
                    self.editing = None;
                }
            }
        });
        ui.add_space(8.0);
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.error_text));
            ui.add_space(6.0);
        }
        match self.tab {
            FileTypesTab::Recognized => self.recognized(ui, p),
            FileTypesTab::Ignored => self.ignored_tab(ui, p),
        }
    }

    fn recognized(&mut self, ui: &mut Ui, p: &IdePalette) {
        let width = ui.available_width();
        let contents = self.types.get(self.selected_type).map(|ty| ty.contents.clone()).unwrap_or_default();
        let height = if contents.is_empty() { 360.0 } else { 360.0 + 24.0 + 18.0 * contents.len() as f32 };
        let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let left = Rect::from_min_size(area.min, vec2((width * 0.38).clamp(160.0, 240.0), height));
        let right = Rect::from_min_max(pos2(left.right() + 16.0, area.top()), area.max);
        let mut left_ui = ui.new_child(egui::UiBuilder::new().max_rect(left));
        self.type_list(&mut left_ui, p, left);
        let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(right));
        right_ui.label(egui::RichText::new(t!("settings-file-types-patterns")).color(p.text));
        right_ui.add_space(4.0);
        self.list_editor(&mut right_ui, p, ListKind::Patterns, 150.0);
        right_ui.add_space(10.0);
        right_ui.label(egui::RichText::new(t!("settings-file-types-hashbangs")).color(p.text));
        right_ui.add_space(4.0);
        self.list_editor(&mut right_ui, p, ListKind::Hashbangs, 110.0);
        if !contents.is_empty() {
            right_ui.add_space(10.0);
            right_ui.label(egui::RichText::new(t!("settings-file-types-contents")).color(p.text));
            for marker in &contents {
                right_ui.label(egui::RichText::new(marker).monospace().color(p.text_secondary));
            }
        }
    }

    fn type_list(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect) {
        ui.painter().rect_stroke(rect, 4, Stroke::new(1.0, p.button_border), StrokeKind::Inside);
        let inner = rect.shrink(2.0);
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        egui::ScrollArea::vertical().id_salt("settings-file-types-list").auto_shrink([false, false]).show(
            &mut list_ui,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for index in 0..self.types.len() {
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let chosen = index == self.selected_type;
                    if chosen {
                        ui.painter().rect_filled(row, 3, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row, 3, p.hover);
                    }
                    let ty = &self.types[index];
                    let sample = ty.patterns.first().map(|pattern| pattern.replace('*', "file").replace('?', "x"));
                    let icon = Rect::from_center_size(pos2(row.left() + 14.0, row.center().y), vec2(14.0, 14.0));
                    file_icons::paint(
                        ui.painter(),
                        icon,
                        sample.as_deref().map_or(file_icons::FileIcon::Generic, file_icons::classify),
                    );
                    ui.painter().text(
                        pos2(row.left() + 28.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &ty.name,
                        theme::regular(13.0),
                        p.text,
                    );
                    accessibility::item(&response, egui::Role::ListItem, chosen, &ty.name);
                    if response.clicked() && !chosen {
                        self.selected_type = index;
                        self.selected = None;
                        self.editing = None;
                    }
                }
            },
        );
    }

    fn ignored_tab(&mut self, ui: &mut Ui, p: &IdePalette) {
        self.list_editor(ui, p, ListKind::Ignored, 300.0);
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t!("settings-file-types-ignored-note")).color(p.text_secondary));
    }

    fn list_editor(&mut self, ui: &mut Ui, p: &IdePalette, list: ListKind, height: f32) {
        let selected = self.selected.filter(|(kind, index)| *kind == list && *index < self.items(list).len());
        let (add, remove, edit) = match list {
            ListKind::Patterns => (
                "settings-file-types-add-pattern",
                "settings-file-types-remove-pattern",
                "settings-file-types-edit-pattern",
            ),
            ListKind::Hashbangs => (
                "settings-file-types-add-hashbang",
                "settings-file-types-remove-hashbang",
                "settings-file-types-edit-hashbang",
            ),
            ListKind::Ignored => (
                "settings-file-types-add-ignored",
                "settings-file-types-remove-ignored",
                "settings-file-types-edit-ignored",
            ),
        };
        let has_type = list == ListKind::Ignored || !self.types.is_empty();
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (which, key, enabled) in
                [(0, add, has_type), (1, remove, selected.is_some()), (2, edit, selected.is_some())]
            {
                let (rect, response) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
                let response = response.on_hover_text(t!(key));
                if enabled && response.hovered() {
                    ui.painter().rect_filled(rect, 4, p.popup_hover);
                }
                let color = if enabled { p.text } else { p.gutter_text };
                let glyph = rect.shrink(6.0);
                match which {
                    0 => glyph_plus(ui.painter(), glyph, color),
                    1 => glyph_minus(ui.painter(), glyph, color),
                    _ => glyph_pencil(ui.painter(), glyph, color),
                }
                accessibility::name(&response, egui::Role::Button, &t!(key));
                if !enabled {
                    accessibility::disabled(&response);
                } else if response.clicked() {
                    action = Some(which);
                }
            }
        });
        match action {
            Some(0) => self.editing = Some(Editing { list, index: None, text: String::new(), focus: true }),
            Some(1) => {
                if let Some((_, index)) = selected
                    && let Some(items) = self.items_mut(list)
                {
                    items.remove(index);
                    self.selected = None;
                }
            }
            Some(_) => {
                if let Some((_, index)) = selected {
                    let text = self.items(list)[index].clone();
                    self.editing = Some(Editing { list, index: Some(index), text, focus: true });
                }
            }
            None => {}
        }
        if self.editing.as_ref().is_some_and(|editing| editing.list == list) {
            self.edit_row(ui, p, list);
        }
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        ui.painter().rect_stroke(rect, 4, Stroke::new(1.0, p.button_border), StrokeKind::Inside);
        let items: Vec<String> = self.items(list).to_vec();
        if items.is_empty() {
            let empty =
                if list == ListKind::Ignored { "settings-file-types-no-ignored" } else { "settings-file-types-none" };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, t!(empty), theme::regular(13.0), p.text_secondary);
            return;
        }
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(2.0)));
        egui::ScrollArea::vertical()
            .id_salt(("settings-file-types-items", list as u8))
            .auto_shrink([false, false])
            .show(&mut list_ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, item) in items.iter().enumerate() {
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let chosen = selected == Some((list, index));
                    if chosen {
                        ui.painter().rect_filled(row, 3, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row, 3, p.hover);
                    }
                    ui.painter().text(
                        pos2(row.left() + 8.0, row.center().y),
                        Align2::LEFT_CENTER,
                        item,
                        theme::regular(13.0),
                        p.text,
                    );
                    accessibility::item(&response, egui::Role::ListItem, chosen, item);
                    if response.clicked() {
                        self.selected = Some((list, index));
                    }
                    if response.double_clicked() {
                        self.editing = Some(Editing { list, index: Some(index), text: item.clone(), focus: true });
                    }
                }
            });
    }

    fn edit_row(&mut self, ui: &mut Ui, p: &IdePalette, list: ListKind) {
        let Some(editing) = self.editing.as_mut() else { return };
        let label = if list == ListKind::Hashbangs {
            t!("settings-file-types-hashbang")
        } else {
            t!("settings-file-types-pattern")
        };
        let mut done = false;
        let mut cancel = false;
        ui.horizontal_wrapped(|ui| {
            let title = ui.label(egui::RichText::new(&label).color(p.text));
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut editing.text)
                        .desired_width((ui.available_width() - 150.0).max(80.0)),
                )
                .labelled_by(title.id);
            if std::mem::take(&mut editing.focus) {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                done = true;
            }
            if field.has_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                cancel = true;
            }
            if ui.button(t!("settings-ok")).clicked() {
                done = true;
            }
            if ui.button(t!("settings-cancel")).clicked() {
                cancel = true;
            }
        });
        let text = editing.text.trim().to_owned();
        if text.contains('/') {
            ui.label(egui::RichText::new(t!("settings-file-types-no-folders")).color(p.error_text));
        } else if let Some(owner) = self.owner(list, &text) {
            let current = self.types.get(self.selected_type).map(|ty| ty.name.clone()).unwrap_or_default();
            let warning = t!(
                "settings-file-types-moves",
                pattern = text.as_str(),
                from = owner.name.as_str(),
                to = current.as_str()
            );
            ui.label(egui::RichText::new(warning).color(p.warning_text));
        }
        ui.add_space(4.0);
        if cancel {
            self.editing = None;
        } else if done {
            self.finish_editing();
        }
    }
}

#[cfg(test)]
pub(in crate::ide) mod testing {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use zen_vfs::file_types::{self, Changes, FileType};

    static GLOBAL: Mutex<()> = Mutex::new(());

    pub(in crate::ide) struct Exclusive {
        #[allow(dead_code, reason = "held for the test")]
        guard: MutexGuard<'static, ()>,
        declared: Vec<FileType>,
    }

    impl Drop for Exclusive {
        fn drop(&mut self) {
            file_types::set_changes(Changes::default());
            file_types::set_overrides(std::collections::BTreeMap::new());
            let added: Vec<String> = file_types::current()
                .declared()
                .iter()
                .filter(|ty| self.declared.iter().all(|before| before.id != ty.id))
                .map(|ty| ty.id.clone())
                .collect();
            for id in added {
                file_types::undeclare(&id);
            }
            file_types::declare(std::mem::take(&mut self.declared));
        }
    }

    fn ty(id: &str, name: &str, patterns: &[&str], hashbangs: &[&str]) -> FileType {
        FileType {
            id: id.to_owned(),
            name: name.to_owned(),
            localized: false,
            patterns: patterns.iter().map(|pattern| (*pattern).to_owned()).collect(),
            hashbangs: hashbangs.iter().map(|hashbang| (*hashbang).to_owned()).collect(),
            contents: Vec::new(),
        }
    }

    pub(in crate::ide) fn exclusive() -> Exclusive {
        let guard = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
        let declared = file_types::current().declared().to_vec();
        file_types::set_changes(Changes::default());
        file_types::set_overrides(std::collections::BTreeMap::new());
        file_types::declare(zen_workbench::file_types::platform_types());
        file_types::declare([
            ty("zt-go", "Go", &["*.go"], &[]),
            ty("zt-gomod", "Go Module", &["go.mod", "go.work"], &[]),
            ty("zt-markdown", "Markdown", &["*.md", "*.markdown"], &[]),
            ty("zt-yaml", "YAML", &["*.yml", "*.yaml", "*.zt-yml"], &[]),
            ty("zt-shell", "Shell Script", &["*.sh", "*.bash", ".bashrc"], &["sh", "bash", "zsh"]),
            ty("zt-json", "JSON", &["*.json", "*.jsonc"], &[]),
            FileType {
                contents: vec![file_types::ContentMarker {
                    starts: "<?zt-xml".to_owned(),
                    unless: vec!["<?zt-xml-not".to_owned()],
                    first_tag: None,
                }],
                ..ty("zt-xml", "ZT XML", &["*.zt-xml"], &[])
            },
        ]);
        Exclusive { guard, declared }
    }
}

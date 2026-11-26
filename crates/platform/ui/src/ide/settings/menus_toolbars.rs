use std::collections::BTreeSet;

use egui::Ui;
use zen_i18n::t;

use super::SettingsDialog;
use super::model::IdeSettings;
use crate::ide::customization::{
    self, Customization, ICONS, MAIN_TOOLBAR, POPUPS, SEPARATOR, SLOT, SUBMENU, group_title, item_title,
};
use crate::ide::main_menu::MENUS;
use crate::ide::{IdePalette, keymap_page};

const MAIN_MENU: &str = "mainMenu";
const ROW_INDENT: f32 = 18.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Selected {
    pub(super) group: String,
    pub(super) index: Option<usize>,
}

#[derive(Default)]
pub(super) struct MenusPage {
    only: Option<&'static str>,
    pub(super) search: String,
    expanded: BTreeSet<String>,
    pub(super) selected: Option<Selected>,
    pub(super) adding: Option<String>,
    picking_icon: bool,
}

fn matches(text: &str, search: &str) -> bool {
    text.to_lowercase().contains(&search.to_lowercase())
}

fn group_matches(custom: &Customization, group: &str, search: &str) -> bool {
    search.is_empty()
        || matches(&group_title(group), search)
        || custom.items(group).iter().any(|item| {
            matches(&item_title(item), search)
                || item.strip_prefix(SUBMENU).is_some_and(|inner| group_matches(custom, inner, search))
        })
}

fn is_popup(group: &str) -> bool {
    POPUPS.iter().any(|popup| popup.id == group)
}

impl MenusPage {
    pub(super) fn for_group(only: Option<&'static str>, add: bool) -> Self {
        let selected = only.map(|group| Selected { group: group.to_owned(), index: None });
        let expanded = only.iter().map(|group| (*group).to_owned()).collect();
        Self { only, selected, expanded, adding: add.then(String::new), ..Self::default() }
    }

    fn toggle(&mut self, group: &str) {
        if !self.expanded.remove(group) {
            self.expanded.insert(group.to_owned());
        }
    }

    fn is_open(&self, group: &str) -> bool {
        self.expanded.contains(group) || !self.search.is_empty()
    }

    fn selected_action(&self, custom: &Customization) -> Option<String> {
        let selected = self.selected.as_ref()?;
        let item = custom.items(&selected.group).get(selected.index?)?.clone();
        let special = item.starts_with(SUBMENU) || item.starts_with(SLOT) || item.starts_with("special:");
        (item != SEPARATOR && !special).then_some(item)
    }

    pub(super) fn add(&mut self, custom: &mut Customization, token: &str) {
        let Some(selected) = self.selected.as_mut() else { return };
        let group = selected.group.clone();
        selected.index = Some(custom.insert(&group, selected.index, token));
        self.adding = None;
    }

    pub(super) fn remove(&mut self, custom: &mut Customization) {
        let Some(Selected { group, index: Some(index) }) = self.selected.clone() else { return };
        custom.remove(&group, index);
        let left = custom.items(&group).len();
        self.selected = Some(Selected { group, index: Some(index.min(left.saturating_sub(1))) }.clamped(left));
    }

    pub(super) fn shift(&mut self, custom: &mut Customization, down: bool) {
        let Some(selected) = self.selected.as_mut() else { return };
        let Some(index) = selected.index else { return };
        selected.index = Some(custom.shift(&selected.group, index, down));
    }

    pub(super) fn restore(&mut self, custom: &mut Customization) {
        let Some(selected) = self.selected.as_mut() else { return };
        custom.restore(&selected.group);
        selected.index = None;
    }

    fn row(&mut self, ui: &mut Ui, p: &IdePalette, depth: usize, title: &str, selected: Selected) -> egui::Response {
        let on = self.selected.as_ref() == Some(&selected);
        let response = ui
            .horizontal(|ui| {
                ui.add_space(ROW_INDENT * depth as f32);
                let color = if on { p.text } else { p.text_secondary };
                ui.add(egui::Button::selectable(on, egui::RichText::new(title).color(color)).truncate())
            })
            .inner;
        if response.clicked() {
            self.selected = Some(selected);
        }
        response
    }

    fn group(&mut self, ui: &mut Ui, p: &IdePalette, custom: &Customization, group: &str, depth: usize) {
        if !group_matches(custom, group, &self.search) {
            return;
        }
        let selected = Selected { group: group.to_owned(), index: None };
        let open = self.is_open(group);
        let arrow = if open { "−" } else { "+" };
        if self.row(ui, p, depth, &format!("{arrow} {}", group_title(group)), selected).clicked() {
            self.toggle(group);
        }
        if !open {
            return;
        }
        if is_popup(group) {
            ui.horizontal(|ui| {
                ui.add_space(ROW_INDENT * (depth + 1) as f32);
                ui.add(
                    egui::Label::new(egui::RichText::new(t!("settings-menus-popup-note")).color(p.text_secondary))
                        .wrap(),
                );
            });
        }
        if group == MAIN_TOOLBAR {
            ui.horizontal(|ui| {
                ui.add_space(ROW_INDENT * (depth + 1) as f32);
                ui.add(
                    egui::Label::new(egui::RichText::new(t!("settings-menus-toolbar-note")).color(p.text_secondary))
                        .wrap(),
                );
            });
        }
        for (index, item) in custom.items(group).into_iter().enumerate() {
            if let Some(inner) = item.strip_prefix(SUBMENU) {
                self.group(ui, p, custom, inner, depth + 1);
                continue;
            }
            let title = item_title(&item);
            if !self.search.is_empty() && !matches(&title, &self.search) && !matches(&group_title(group), &self.search)
            {
                continue;
            }
            let selected = Selected { group: group.to_owned(), index: Some(index) };
            self.row(ui, p, depth + 1, &title, selected);
        }
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, custom: &Customization) {
        if let Some(only) = self.only {
            self.group(ui, p, custom, only, 0);
            return;
        }
        let title = format!("{} {}", if self.is_open(MAIN_MENU) { "−" } else { "+" }, t!("settings-menus-main-menu"));
        let selected = Selected { group: MAIN_MENU.to_owned(), index: None };
        if self.row(ui, p, 0, &title, selected).clicked() {
            self.toggle(MAIN_MENU);
        }
        if self.is_open(MAIN_MENU) {
            for (menu, _) in MENUS {
                self.group(ui, p, custom, menu, 1);
            }
        }
        self.group(ui, p, custom, MAIN_TOOLBAR, 0);
        for popup in POPUPS {
            self.group(ui, p, custom, popup.id, 0);
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, draft: &mut IdeSettings) {
        let custom = &mut draft.customization;
        let in_group = self.selected.as_ref().is_some_and(|selected| selected.group != MAIN_MENU);
        let on_item = in_group && self.selected.as_ref().is_some_and(|selected| selected.index.is_some());
        let action = self.selected_action(custom);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(in_group, egui::Button::new(t!("settings-menus-add"))).clicked() {
                self.adding = Some(String::new());
            }
            if ui.add_enabled(action.is_some(), egui::Button::new(t!("settings-menus-edit-icon"))).clicked() {
                self.picking_icon = !self.picking_icon;
            }
            if ui.add_enabled(on_item, egui::Button::new(t!("settings-menus-move-up"))).clicked() {
                self.shift(custom, false);
            }
            if ui.add_enabled(on_item, egui::Button::new(t!("settings-menus-move-down"))).clicked() {
                self.shift(custom, true);
            }
            if ui.add_enabled(on_item, egui::Button::new(t!("settings-menus-remove"))).clicked() {
                self.remove(custom);
            }
            if ui.add_enabled(in_group, egui::Button::new(t!("settings-menus-restore"))).clicked() {
                self.restore(custom);
            }
            if ui.button(t!("settings-menus-restore-all")).clicked() {
                custom.restore_all();
                self.selected = None;
            }
        });
    }

    fn adder(&mut self, ui: &mut Ui, p: &IdePalette, custom: &mut Customization) {
        let Some(query) = self.adding.as_mut() else { return };
        ui.add(egui::TextEdit::singleline(query).hint_text(t!("settings-menus-add-search")).desired_width(280.0));
        let query = query.clone();
        let mut chosen = None;
        egui::Frame::new().fill(p.popup).inner_margin(6).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("settings-menus-add").max_height(180.0).show(ui, |ui| {
                if ui.button(t!("settings-menus-separator")).clicked() {
                    chosen = Some(SEPARATOR.to_owned());
                }
                let group = self.selected.as_ref().map(|selected| selected.group.clone()).unwrap_or_default();
                for token in customization::missing_defaults(custom, &group) {
                    let title = item_title(&token);
                    if matches(&title, &query) && ui.button(&title).clicked() {
                        chosen = Some(token);
                    }
                }
                for action in customization::all_actions() {
                    let title = keymap_page::action_title(&action);
                    if (matches(&title, &query) || matches(&action, &query)) && ui.button(&title).clicked() {
                        chosen = Some(action);
                    }
                }
            });
        });
        if let Some(chosen) = chosen {
            self.add(custom, &chosen);
        }
        ui.add_space(6.0);
    }

    fn icons(&mut self, ui: &mut Ui, custom: &mut Customization) {
        let Some(action) = self.selected_action(custom).filter(|_| self.picking_icon) else { return };
        ui.horizontal_wrapped(|ui| {
            let mut chosen = None;
            for (id, key, _) in ICONS {
                if ui.button(t!(key)).clicked() {
                    chosen = Some(Some(*id));
                }
            }
            if ui.button(t!("settings-menus-no-icon")).clicked() {
                chosen = Some(None);
            }
            if let Some(icon) = chosen {
                custom.set_icon(&action, icon);
                self.picking_icon = false;
            }
        });
        ui.add_space(6.0);
    }
}

impl Selected {
    fn clamped(mut self, left: usize) -> Self {
        if left == 0 {
            self.index = None;
        }
        self
    }
}

impl SettingsDialog {
    pub(super) fn menus_page(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(draft) = self.draft.as_mut() else { return };
        let page = &mut self.menus;
        page.toolbar(ui, draft);
        ui.add_space(6.0);
        page.adder(ui, p, &mut draft.customization);
        page.icons(ui, &mut draft.customization);
        let name = t!("settings-menus-search");
        let field = ui.add(egui::TextEdit::singleline(&mut page.search).hint_text(&name).desired_width(240.0));
        crate::accessibility::name(&field, egui::Role::TextInput, &name);
        ui.add_space(6.0);
        let custom = draft.customization.clone();
        egui::ScrollArea::vertical().id_salt("settings-menus-tree").auto_shrink([false, false]).show(ui, |ui| {
            page.tree(ui, p, &custom);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(group: &'static str) -> MenusPage {
        MenusPage::for_group(Some(group), false)
    }

    #[test]
    fn the_toolbar_of_the_page_edits_the_selected_group() {
        let mut custom = Customization::default();
        let mut menus = page(MAIN_TOOLBAR);
        menus.add(&mut custom, "editor.code.reformat");
        assert_eq!(custom.items(MAIN_TOOLBAR).last().map(String::as_str), Some("editor.code.reformat"));
        assert_eq!(menus.selected.as_ref().unwrap().index, Some(2));
        menus.add(&mut custom, SEPARATOR);
        menus.shift(&mut custom, false);
        assert_eq!(custom.items(MAIN_TOOLBAR)[2], SEPARATOR);
        menus.remove(&mut custom);
        assert_eq!(custom.items(MAIN_TOOLBAR).len(), 3);
        menus.restore(&mut custom);
        assert_eq!(custom, Customization::default());
    }

    #[test]
    fn opening_for_a_group_shows_only_it_and_can_start_adding() {
        let menus = MenusPage::for_group(Some(MAIN_TOOLBAR), true);
        assert_eq!(menus.only, Some(MAIN_TOOLBAR));
        assert!(menus.adding.is_some());
        assert!(menus.is_open(MAIN_TOOLBAR));
    }

    #[test]
    fn the_search_finds_a_group_by_what_it_holds() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let custom = Customization::default();
        assert!(group_matches(&custom, "menu-file", "Reload"), "an item of the group");
        assert!(group_matches(&custom, "menu-file", "File"), "its name");
        assert!(!group_matches(&custom, "menu-file", "no such thing"));
        assert!(!is_popup(MAIN_TOOLBAR));
        assert!(is_popup(customization::TAB_POPUP));
    }

    #[test]
    fn a_removed_built_in_item_can_be_added_back() {
        let mut custom = Customization::default();
        let mut menus = page(customization::GIT_LOG_POPUP);
        menus.selected.as_mut().unwrap().index = Some(0);
        let first = custom.items(customization::GIT_LOG_POPUP)[0].clone();
        menus.remove(&mut custom);
        assert_eq!(
            customization::missing_defaults(&custom, customization::GIT_LOG_POPUP),
            std::slice::from_ref(&first)
        );
        assert_eq!(menus.selected_action(&custom), None, "a built-in item has no icon");
        menus.add(&mut custom, &first);
        assert_eq!(custom.items(customization::GIT_LOG_POPUP).len(), 3);
    }

    #[test]
    fn the_search_finds_built_in_items_of_a_popup() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        assert!(group_matches(&Customization::default(), customization::TERMINAL_POPUP, "Split"));
    }

    #[test]
    fn only_a_real_action_has_an_icon() {
        let mut custom = Customization::default();
        let mut menus = page(MAIN_TOOLBAR);
        menus.add(&mut custom, "editor.code.reformat");
        assert_eq!(menus.selected_action(&custom).as_deref(), Some("editor.code.reformat"));
        menus.add(&mut custom, SEPARATOR);
        assert_eq!(menus.selected_action(&custom), None);
    }
}

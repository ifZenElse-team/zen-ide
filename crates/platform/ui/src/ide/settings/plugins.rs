use egui::{Color32, Ui, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_modules::{AreaList, ModuleEntry, ModuleList};
use zen_toolbox::markdown::Block;
use zen_workbench::plugin_manager::{CatalogStatus, PluginDetails, PluginList, PluginSummary};

use super::{Dispatch, SettingsDialog, widgets};
use crate::ide::IdePalette;
use crate::theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ide) enum PluginsTab {
    #[default]
    Marketplace,
    Installed,
    BuiltIn,
}

#[derive(Default)]
pub(in crate::ide) struct PluginsPage {
    pub(in crate::ide) tab: PluginsTab,
    pub(in crate::ide) search: String,
    pub(in crate::ide) selected: Option<String>,
    status: Option<String>,
    shown: Option<Shown>,
    pub(in crate::ide) selected_module: Option<String>,
    modules: Option<ModulesShown>,
    pub(in crate::ide) restart: bool,
    uninstalling: Option<(String, String)>,
}

struct ModulesShown {
    key: String,
    at: f64,
    list: Result<ModuleList, String>,
    areas: Option<AreaList>,
}

struct Shown {
    key: (PluginsTab, String),
    at: f64,
    list: Result<PluginList, String>,
    details: Option<(PluginDetails, Vec<Vec<Block>>)>,
}

impl Shown {
    fn busy(&self) -> bool {
        self.list.as_ref().is_ok_and(|list| list.catalog == CatalogStatus::Loading || !list.installing.is_empty())
    }
}

const IDLE_REFRESH: f64 = 1.0;

const LIST_WIDTH: f32 = 340.0;

const NARROW: f32 = 560.0;

type Action = (&'static str, String);

impl SettingsDialog {
    pub(super) fn plugins_page(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: Option<&mut Dispatch<'_>>) {
        let Some(dispatch) = dispatch else { return };
        let page = &mut self.plugins;
        let mut action: Option<Action> = None;
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (PluginsTab::Marketplace, t!("settings-plugins-marketplace")),
                (PluginsTab::Installed, t!("settings-plugins-installed")),
                (PluginsTab::BuiltIn, t!("settings-plugins-built-in")),
            ] {
                let selected = page.tab == tab;
                let text = egui::RichText::new(label).font(theme::semibold(13.0)).color(if selected {
                    p.text
                } else {
                    p.text_secondary
                });
                let response = ui.add(egui::Button::new(text).frame(selected).fill(if selected {
                    p.selection
                } else {
                    Color32::TRANSPARENT
                }));
                if response.clicked() {
                    page.tab = tab;
                    page.status = None;
                }
            }
            if page.tab == PluginsTab::Installed {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.link(t!("settings-plugins-check-updates")).clicked() {
                        action = Some(("plugins.updates.check", String::new()));
                    }
                });
            }
        });
        ui.add_space(8.0);
        if page.tab == PluginsTab::BuiltIn {
            built_in(page, ui, p, dispatch);
            return;
        }
        let now = ui.input(|input| input.time);
        let key = (page.tab, page.search.trim().to_owned());
        let kept = page
            .shown
            .take()
            .filter(|shown| shown.key == key && !shown.busy() && now - shown.at < IDLE_REFRESH && now >= shown.at);
        let mut shown = kept.unwrap_or_else(|| {
            let command = match page.tab {
                PluginsTab::Marketplace => "plugins.catalog.list",
                PluginsTab::Installed | PluginsTab::BuiltIn => "plugins.installed.list",
            };
            let list = dispatch(command, json!({ "query": key.1 }))
                .and_then(|value| serde_json::from_value::<PluginList>(value).map_err(|error| error.to_string()));
            Shown { key, at: now, list, details: None }
        });
        let list = match &shown.list {
            Ok(list) => list,
            Err(error) => {
                ui.label(egui::RichText::new(error).color(p.error_text));
                page.shown = Some(shown);
                return;
            }
        };
        if !list.restart_required.is_empty() {
            egui::Frame::new().fill(p.banner).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let names = list.restart_required.join(", ");
                ui.label(egui::RichText::new(t!("settings-plugins-restart", names = names)).color(p.text));
            });
            ui.add_space(6.0);
        }
        if let Some(status) = &page.status {
            ui.label(egui::RichText::new(status).color(p.text_secondary));
            ui.add_space(4.0);
        }
        if let Some(error) = &list.check_failed {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t!("settings-plugins-check-failed", error = error.as_str()))
                        .color(p.text_secondary),
                )
                .wrap(),
            );
            ui.add_space(4.0);
        }
        if list.catalog == CatalogStatus::Loading || !list.installing.is_empty() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        if !page.selected.as_ref().is_some_and(|id| list.plugins.iter().any(|plugin| &plugin.id == id)) {
            page.selected = list.plugins.first().map(|plugin| plugin.id.clone());
        }
        let mut details = shown.details.take();
        if details.as_ref().map(|(details, _)| &details.plugin.id) != page.selected.as_ref() {
            details = page.selected.as_ref().and_then(|id| {
                let details = dispatch("plugins.plugin.details", json!({ "id": id }))
                    .ok()
                    .and_then(|value| serde_json::from_value::<PluginDetails>(value).ok())?;
                let notes = details.notes.iter().map(|notes| zen_toolbox::markdown::parse(&notes.notes)).collect();
                Some((details, notes))
            });
        }
        let shown_details = &details;

        let height = (ui.clip_rect().bottom() - ui.cursor().top() - 4.0).max(160.0);
        let width = ui.available_width();
        let list_pane = |ui: &mut Ui, page: &mut PluginsPage, action: &mut Option<Action>, size: egui::Vec2| {
            ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
                let search = ui.add(
                    egui::TextEdit::singleline(&mut page.search)
                        .hint_text(t!("settings-plugins-search"))
                        .desired_width(size.x),
                );
                crate::accessibility::name(&search, egui::Role::TextInput, &t!("settings-plugins-search"));
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("settings-plugins-list")
                    .max_height(size.y - 34.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| match page.tab {
                        PluginsTab::Marketplace => marketplace(ui, p, list, &mut page.selected, action),
                        PluginsTab::Installed | PluginsTab::BuiltIn => {
                            installed(ui, p, list, &mut page.selected, action);
                        }
                    });
            });
        };
        let agents: serde_json::Value =
            dispatch("plugins.runAccess.get", json!({})).map(|answer| answer["agents"].clone()).unwrap_or_default();
        let details_pane = |ui: &mut Ui, action: &mut Option<Action>, size: egui::Vec2| {
            ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("settings-plugins-details")
                    .max_height(size.y)
                    .auto_shrink([false, false])
                    .show(ui, |ui| match shown_details {
                        Some((details, notes)) => {
                            let access = agents[details.plugin.id.as_str()].as_str().unwrap_or("none");
                            details_view(ui, p, details, notes, access, action);
                        }
                        None => {
                            ui.label(egui::RichText::new(t!("settings-plugins-none-selected")).color(p.text_secondary));
                        }
                    });
            });
        };
        if width >= NARROW {
            let list_width = (width * 0.45).clamp(220.0, LIST_WIDTH);
            ui.horizontal_top(|ui| {
                list_pane(ui, page, &mut action, vec2(list_width, height));
                ui.add_space(8.0);
                let (separator, _) = ui.allocate_exact_size(vec2(1.0, height), egui::Sense::hover());
                ui.painter().vline(separator.center().x, separator.y_range(), egui::Stroke::new(1.0, p.tab_border));
                ui.add_space(12.0);
                let rest = ui.available_width();
                details_pane(ui, &mut action, vec2(rest, height));
            });
        } else {
            let list_height = (height * 0.5).max(120.0);
            list_pane(ui, page, &mut action, vec2(width, list_height));
            ui.separator();
            details_pane(ui, &mut action, vec2(width, (height - list_height - 8.0).max(80.0)));
        }

        if let Some(("plugins.plugin.uninstall", id)) = &action {
            let name = list.plugins.iter().find(|plugin| &plugin.id == id).map_or(id, |plugin| &plugin.name);
            page.uninstalling = Some((id.clone(), name.clone()));
            action = None;
        }
        let action = action.or_else(|| uninstall_dialog(ui, p, page).map(|id| ("plugins.plugin.uninstall", id)));
        let Some((command, id)) = action else {
            shown.details = details;
            page.shown = Some(shown);
            return;
        };
        {
            let name = list.plugins.iter().find(|plugin| plugin.id == id).map(|plugin| plugin.name.clone());
            let params = match id.split_once(ACCESS_SEPARATOR) {
                Some((plugin, access)) if command == SET_RUN_ACCESS => json!({ "plugin": plugin, "access": access }),
                _ if id.is_empty() => json!({}),
                _ => json!({ "id": id }),
            };
            page.status = match dispatch(command, params) {
                Ok(_) if command == "plugins.plugin.uninstall" => {
                    Some(t!("settings-plugins-uninstalled", name = name.unwrap_or(id).as_str()))
                }
                Ok(_) => None,
                Err(error) => Some(error),
            };
        }
    }
}

fn uninstall_dialog(ui: &mut Ui, p: &IdePalette, page: &mut PluginsPage) -> Option<String> {
    let (id, name) = page.uninstalling.clone()?;
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("plugins-uninstall")).frame(crate::ide::dialog_frame(p)).show(
        ui.ctx(),
        |ui| {
            ui.set_width(380.0);
            let title = egui::RichText::new(t!("settings-plugins-uninstall-title")).font(theme::semibold(15.0));
            ui.label(title.color(p.text));
            ui.add_space(12.0);
            let question = t!("settings-plugins-uninstall-question", name = name.as_str());
            ui.add(egui::Label::new(egui::RichText::new(question).font(theme::regular(13.5)).color(p.text)).wrap());
            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if accent_button(ui, p, &t!("settings-plugins-uninstall-go")).clicked() {
                    choice = Some(true);
                }
                if ui.button(t!("settings-cancel")).clicked() {
                    choice = Some(false);
                }
            });
        },
    );
    if choice.is_none() && modal.should_close() {
        choice = Some(false);
    }
    match choice {
        Some(go) => {
            page.uninstalling = None;
            go.then_some(id)
        }
        None => None,
    }
}

fn built_in(page: &mut PluginsPage, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
    let now = ui.input(|input| input.time);
    let key = page.search.trim().to_owned();
    let kept = page.modules.take().filter(|shown| shown.key == key && now - shown.at < IDLE_REFRESH && now >= shown.at);
    let shown = kept.unwrap_or_else(|| {
        let list = dispatch("plugins.modules.list", json!({ "query": key }))
            .and_then(|value| serde_json::from_value::<ModuleList>(value).map_err(|error| error.to_string()));
        let areas = dispatch("plugins.areas.list", json!({}))
            .ok()
            .and_then(|value| serde_json::from_value::<AreaList>(value).ok())
            .filter(|areas| !areas.areas.is_empty());
        ModulesShown { key, at: now, list, areas }
    });
    let list = match &shown.list {
        Ok(list) => list,
        Err(error) => {
            ui.label(egui::RichText::new(error).color(p.error_text));
            page.modules = Some(shown);
            return;
        }
    };
    let mut action: Option<Action> = None;
    ui.add(
        egui::Label::new(
            egui::RichText::new(t!("settings-plugins-built-in-about", product = list.product.as_str()))
                .color(p.text_secondary),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    if let Some(areas) = &shown.areas
        && areas_block(ui, p, areas)
    {
        page.status = dispatch("plugins.areas.show", json!({})).err();
    }
    if !list.restart_required.is_empty() {
        egui::Frame::new().fill(p.banner).corner_radius(6).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let names = list.restart_required.join(", ");
                let text = t!("settings-plugins-built-in-restart", product = list.product.as_str(), names = names);
                ui.label(egui::RichText::new(text).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if accent_button(ui, p, &t!("settings-plugins-restart-now")).clicked() {
                        page.restart = true;
                    }
                });
            });
        });
        ui.add_space(6.0);
    }
    if let Some(status) = &page.status {
        ui.label(egui::RichText::new(status).color(p.text_secondary));
        ui.add_space(4.0);
    }
    if !page.selected_module.as_ref().is_some_and(|id| list.modules.iter().any(|module| &module.id == id)) {
        page.selected_module = list.modules.first().map(|module| module.id.clone());
    }
    let selected = page.selected_module.clone();
    let details = selected.as_ref().and_then(|id| list.modules.iter().find(|module| &module.id == id));

    let height = (ui.clip_rect().bottom() - ui.cursor().top() - 4.0).max(160.0);
    let width = ui.available_width();
    let list_pane = |ui: &mut Ui, page: &mut PluginsPage, action: &mut Option<Action>, size: egui::Vec2| {
        ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut page.search)
                    .hint_text(t!("settings-plugins-search"))
                    .desired_width(size.x),
            );
            crate::accessibility::name(&search, egui::Role::TextInput, &t!("settings-plugins-search"));
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("settings-plugins-built-in")
                .max_height(size.y - 34.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if list.modules.is_empty() {
                        ui.label(egui::RichText::new(t!("settings-plugins-nothing-found")).color(p.text_secondary));
                        return;
                    }
                    let on = list.modules.iter().filter(|module| module.enabled).count();
                    let title = t!(
                        "settings-plugins-group",
                        title = t!("settings-plugins-built-in-group"),
                        enabled = on,
                        total = list.modules.len()
                    );
                    widgets::group_title(ui, p, &title, None);
                    for module in &list.modules {
                        module_row(ui, p, module, &list.product, &mut page.selected_module, action);
                    }
                });
        });
    };
    let details_pane = |ui: &mut Ui, action: &mut Option<Action>, size: egui::Vec2| {
        ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("settings-plugins-built-in-details")
                .max_height(size.y)
                .auto_shrink([false, false])
                .show(ui, |ui| match details {
                    Some(module) => module_details(ui, p, module, &list.product, action),
                    None => {
                        ui.label(egui::RichText::new(t!("settings-plugins-none-selected")).color(p.text_secondary));
                    }
                });
        });
    };
    if width >= NARROW {
        let list_width = (width * 0.45).clamp(220.0, LIST_WIDTH);
        ui.horizontal_top(|ui| {
            list_pane(ui, page, &mut action, vec2(list_width, height));
            ui.add_space(8.0);
            let (separator, _) = ui.allocate_exact_size(vec2(1.0, height), egui::Sense::hover());
            ui.painter().vline(separator.center().x, separator.y_range(), egui::Stroke::new(1.0, p.tab_border));
            ui.add_space(12.0);
            let rest = ui.available_width();
            details_pane(ui, &mut action, vec2(rest, height));
        });
    } else {
        let list_height = (height * 0.5).max(120.0);
        list_pane(ui, page, &mut action, vec2(width, list_height));
        ui.separator();
        details_pane(ui, &mut action, vec2(width, (height - list_height - 8.0).max(80.0)));
    }
    let Some((command, id)) = action else {
        page.modules = Some(shown);
        return;
    };
    page.status = dispatch(command, json!({ "id": id })).err();
}

fn areas_block(ui: &mut Ui, p: &IdePalette, areas: &AreaList) -> bool {
    let mut choose = false;
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, p.tab_border))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(t!("settings-plugins-areas")).font(theme::semibold(13.0)).color(p.text),
                    );
                    let on: Vec<&str> =
                        areas.areas.iter().filter(|area| area.enabled).map(|area| area.name.as_str()).collect();
                    let text = if on.is_empty() {
                        t!("settings-plugins-areas-none")
                    } else {
                        t!("settings-plugins-areas-on", names = on.join(", "))
                    };
                    ui.label(egui::RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    choose = ui.button(t!("settings-plugins-areas-choose")).clicked();
                });
            });
        });
    ui.add_space(8.0);
    choose
}

fn module_state(module: &ModuleEntry, product: &str) -> (String, Option<String>) {
    let default = if module.on_by_default {
        t!("settings-plugins-on-by-default", product = product)
    } else {
        t!("settings-plugins-off-by-default", product = product)
    };
    let pending = module
        .restart_required
        .then(|| if module.enabled { t!("settings-plugins-pending-on") } else { t!("settings-plugins-pending-off") });
    (default, pending)
}

fn module_row(
    ui: &mut Ui,
    p: &IdePalette,
    module: &ModuleEntry,
    product: &str,
    selected: &mut Option<String>,
    action: &mut Option<Action>,
) {
    let is_selected = selected.as_deref() == Some(module.id.as_str());
    let (default, pending) = module_state(module, product);
    let frame = egui::Frame::new()
        .fill(if is_selected { p.selection } else { Color32::TRANSPARENT })
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let mut enabled = module.enabled;
                if ui.checkbox(&mut enabled, "").on_hover_text(t!("settings-plugins-enabled")).changed() {
                    let command = if enabled { "plugins.module.enable" } else { "plugins.module.disable" };
                    *action = Some((command, module.id.clone()));
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&module.name).font(theme::semibold(13.0)).color(p.text))
                            .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(egui::RichText::new(default).font(theme::regular(11.0)).color(p.link))
                            .selectable(false),
                    );
                    if let Some(pending) = pending {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(pending).font(theme::regular(11.0)).color(p.text_secondary),
                            )
                            .selectable(false),
                        );
                    }
                });
            });
        });
    if ui.rect_contains_pointer(frame.response.rect) && ui.input(|input| input.pointer.primary_clicked()) {
        *selected = Some(module.id.clone());
    }
}

fn module_details(ui: &mut Ui, p: &IdePalette, module: &ModuleEntry, product: &str, action: &mut Option<Action>) {
    let (default, pending) = module_state(module, product);
    ui.label(egui::RichText::new(&module.name).font(theme::semibold(17.0)).color(p.text));
    ui.add_space(2.0);
    ui.label(egui::RichText::new(default).font(theme::regular(12.0)).color(p.link));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let (label, command) = if module.enabled {
            (t!("settings-plugins-disable"), "plugins.module.disable")
        } else {
            (t!("settings-plugins-enable"), "plugins.module.enable")
        };
        if ui.button(label).clicked() {
            *action = Some((command, module.id.clone()));
        }
        if let Some(pending) = &pending {
            ui.label(egui::RichText::new(pending).color(p.text_secondary));
        }
    });
    ui.add_space(10.0);
    ui.add(egui::Label::new(egui::RichText::new(&module.description).color(p.text)).wrap());
    ui.add_space(6.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new(t!("settings-plugins-built-in-note"))
                .font(theme::regular(12.0))
                .color(p.text_secondary),
        )
        .wrap(),
    );
}

fn marketplace(
    ui: &mut Ui,
    p: &IdePalette,
    list: &PluginList,
    selected: &mut Option<String>,
    action: &mut Option<Action>,
) {
    match list.catalog {
        CatalogStatus::Loading if !list.plugins.is_empty() => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(t!("settings-plugins-checking")).color(p.text_secondary));
            });
        }
        CatalogStatus::Loading | CatalogStatus::NotFetched => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(t!("settings-plugins-loading")).color(p.text_secondary));
            });
            return;
        }
        CatalogStatus::Unavailable => {
            ui.label(egui::RichText::new(t!("settings-plugins-unavailable")).color(p.text_secondary));
            if ui.button(t!("settings-plugins-retry")).clicked() {
                *action = Some(("plugins.updates.check", String::new()));
            }
            return;
        }
        CatalogStatus::Ready => {}
    }
    if list.plugins.is_empty() {
        ui.label(egui::RichText::new(t!("settings-plugins-nothing-found")).color(p.text_secondary));
    }
    for plugin in &list.plugins {
        let version = plugin.latest.clone().unwrap_or_default();
        row(ui, p, plugin, &version, selected, |ui| {
            if plugin.installing {
                ui.spinner();
            } else if plugin.update.is_some() {
                if ui.button(t!("settings-plugins-update")).clicked() {
                    *action = Some(("plugins.plugin.update", plugin.id.clone()));
                }
            } else if plugin.installed.is_some() {
                ui.add_enabled(false, egui::Button::new(t!("settings-plugins-is-installed")));
            } else if accent_button(ui, p, &t!("settings-plugins-install")).clicked() {
                *action = Some(("plugins.plugin.install", plugin.id.clone()));
            }
        });
    }
}

fn installed(
    ui: &mut Ui,
    p: &IdePalette,
    list: &PluginList,
    selected: &mut Option<String>,
    action: &mut Option<Action>,
) {
    if list.catalog == CatalogStatus::Loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new(t!("settings-plugins-checking")).color(p.text_secondary));
        });
    }
    if list.plugins.is_empty() {
        ui.label(egui::RichText::new(t!("settings-plugins-none")).color(p.text_secondary));
    }
    let updates = list.plugins.iter().filter(|plugin| plugin.update.is_some()).count();
    if updates > 0 {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(t!("settings-plugins-updates", count = updates))
                    .font(theme::semibold(12.0))
                    .color(p.text),
            );
            if ui.link(t!("settings-plugins-update-all")).clicked() {
                *action = Some(("plugins.updates.all", String::new()));
            }
        });
        ui.add_space(4.0);
    }
    for (title, group) in installed_groups(&list.plugins) {
        widgets::group_title(ui, p, &title, None);
        for plugin in group {
            let version = plugin.installed.clone().unwrap_or_default();
            row(ui, p, plugin, &version, selected, |ui| {
                let mut enabled = plugin.enabled;
                if ui.checkbox(&mut enabled, "").on_hover_text(t!("settings-plugins-enabled")).changed() {
                    let command = if enabled { "plugins.plugin.enable" } else { "plugins.plugin.disable" };
                    *action = Some((command, plugin.id.clone()));
                }
                if plugin.installing {
                    ui.spinner();
                } else if plugin.update.is_some() && ui.button(t!("settings-plugins-update")).clicked() {
                    *action = Some(("plugins.plugin.update", plugin.id.clone()));
                }
            });
        }
        ui.add_space(6.0);
    }
}

fn row(
    ui: &mut Ui,
    p: &IdePalette,
    plugin: &PluginSummary,
    version: &str,
    selected: &mut Option<String>,
    actions: impl FnOnce(&mut Ui),
) {
    let is_selected = selected.as_deref() == Some(plugin.id.as_str());
    let frame = egui::Frame::new()
        .fill(if is_selected { p.selection } else { Color32::TRANSPARENT })
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                actions(ui);
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&plugin.name).font(theme::semibold(13.0)).color(p.text),
                            )
                            .selectable(false),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(version).font(theme::regular(12.0)).color(p.text_secondary),
                            )
                            .selectable(false),
                        );
                    });
                    if plugin.bundled {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(t!("settings-plugins-bundled"))
                                    .font(theme::regular(11.0))
                                    .color(p.link),
                            )
                            .selectable(false),
                        );
                    }
                });
            });
        });
    if ui.rect_contains_pointer(frame.response.rect) && ui.input(|input| input.pointer.primary_clicked()) {
        *selected = Some(plugin.id.clone());
    }
}

const SET_RUN_ACCESS: &str = "plugins.runAccess.set";

const ACCESS_SEPARATOR: char = '\u{1f}';

fn runs_block(ui: &mut Ui, p: &IdePalette, plugin: &PluginSummary, access: &str, action: &mut Option<Action>) {
    if plugin.runs.is_empty() {
        return;
    }
    if !plugin.tool_windows.is_empty() {
        ui.add_space(4.0);
        let windows = plugin.tool_windows.join(", ");
        ui.label(
            egui::RichText::new(t!("settings-plugins-tool-windows", windows = windows.as_str()))
                .font(theme::regular(12.0))
                .color(p.text_secondary),
        );
    }
    ui.add_space(4.0);
    let programs = plugin.runs.join(", ");
    ui.label(
        egui::RichText::new(t!("settings-plugins-runs", programs = programs.as_str()))
            .font(theme::regular(12.0))
            .color(p.text_secondary),
    );
    if !plugin.person_only.is_empty() {
        let commands = plugin.person_only.join("; ");
        ui.label(
            egui::RichText::new(t!("settings-plugins-person-only", commands = commands.as_str()))
                .font(theme::regular(12.0))
                .color(p.text_secondary),
        );
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t!("settings-plugins-agent-runs")).font(theme::regular(12.0)).color(p.text));
        let label = |value: &str| match value {
            "ask" => t!("settings-plugins-agent-runs-ask"),
            "full" => t!("settings-plugins-agent-runs-full"),
            _ => t!("settings-plugins-agent-runs-none"),
        };
        egui::ComboBox::from_id_salt(("settings-plugins-agent-runs", plugin.id.as_str()))
            .selected_text(label(access))
            .show_ui(ui, |ui| {
                for value in ["none", "ask", "full"] {
                    if ui.selectable_label(access == value, label(value)).clicked() && access != value {
                        *action = Some((SET_RUN_ACCESS, format!("{}{ACCESS_SEPARATOR}{value}", plugin.id)));
                    }
                }
            });
    });
}

fn details_view(
    ui: &mut Ui,
    p: &IdePalette,
    details: &PluginDetails,
    blocks: &[Vec<Block>],
    access: &str,
    action: &mut Option<Action>,
) {
    let plugin = &details.plugin;
    ui.label(egui::RichText::new(&plugin.name).font(theme::semibold(17.0)).color(p.text));
    ui.add_space(2.0);
    let version = match (&plugin.installed, &plugin.update, &plugin.latest) {
        (Some(installed), Some(update), _) => {
            t!("settings-plugins-version-update", installed = installed.as_str(), latest = update.as_str())
        }
        (Some(installed), None, _) => t!("settings-plugins-version", version = installed.as_str()),
        (None, _, Some(latest)) => t!("settings-plugins-version", version = latest.as_str()),
        (None, _, None) => String::new(),
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(version).color(p.text_secondary));
        if plugin.bundled {
            ui.label(egui::RichText::new(t!("settings-plugins-bundled")).font(theme::regular(12.0)).color(p.link));
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if plugin.installing {
            ui.spinner();
            ui.label(egui::RichText::new(t!("settings-plugins-installing")).color(p.text_secondary));
        } else if plugin.installed.is_none() {
            if accent_button(ui, p, &t!("settings-plugins-install")).clicked() {
                *action = Some(("plugins.plugin.install", plugin.id.clone()));
            }
        } else {
            if plugin.update.is_some() && accent_button(ui, p, &t!("settings-plugins-update")).clicked() {
                *action = Some(("plugins.plugin.update", plugin.id.clone()));
            }
            let (label, command) = if plugin.enabled {
                (t!("settings-plugins-disable"), "plugins.plugin.disable")
            } else {
                (t!("settings-plugins-enable"), "plugins.plugin.enable")
            };
            if ui.button(label).clicked() {
                *action = Some((command, plugin.id.clone()));
            }
            if !plugin.bundled && ui.button(t!("settings-plugins-uninstall")).clicked() {
                *action = Some(("plugins.plugin.uninstall", plugin.id.clone()));
            }
        }
    });
    ui.add_space(10.0);
    if !plugin.description.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new(&plugin.description).color(p.text)).wrap());
        ui.add_space(6.0);
    }
    let kinds: Vec<String> = plugin
        .extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .chain(plugin.names.iter().cloned())
        .collect();
    if !kinds.is_empty() {
        ui.add(
            egui::Label::new(egui::RichText::new(kinds.join("  ")).font(theme::regular(12.0)).color(p.gutter_text))
                .wrap(),
        );
    }
    if plugin.language_servers {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(t!("settings-plugins-language-servers"))
                .font(theme::regular(12.0))
                .color(p.text_secondary),
        );
    }
    runs_block(ui, p, plugin, access, action);
    ui.add_space(14.0);
    widgets::group_title(ui, p, &t!("settings-plugins-whats-new"), None);
    let palette =
        theme::Palette { text: p.text, text_secondary: p.text_secondary, hover: p.frame, ..theme::Palette::DARK };
    for (notes, blocks) in details.notes.iter().zip(blocks) {
        let heading = if notes.date.is_empty() {
            t!("settings-plugins-notes-version", version = notes.version.as_str())
        } else {
            t!("settings-plugins-notes-version-date", version = notes.version.as_str(), date = notes.date.as_str())
        };
        ui.label(egui::RichText::new(heading).font(theme::semibold(13.0)).color(p.text));
        ui.add_space(4.0);
        if notes.notes.trim().is_empty() {
            ui.label(egui::RichText::new(t!("settings-plugins-no-notes")).color(p.text_secondary));
        } else {
            crate::markdown::show(ui, blocks, palette);
        }
        ui.add_space(10.0);
    }
    if details.notes.is_empty() {
        ui.label(egui::RichText::new(t!("settings-plugins-no-notes")).color(p.text_secondary));
    }
}

fn accent_button(ui: &mut Ui, p: &IdePalette, label: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(label).color(p.accent_text)).fill(p.accent))
}

pub(in crate::ide) fn installed_groups(plugins: &[PluginSummary]) -> Vec<(String, Vec<&PluginSummary>)> {
    [(false, t!("settings-plugins-downloaded")), (true, t!("settings-plugins-bundled-group"))]
        .into_iter()
        .filter_map(|(bundled, title)| {
            let group: Vec<_> = plugins.iter().filter(|plugin| plugin.bundled == bundled).collect();
            let on = group.iter().filter(|plugin| plugin.enabled).count();
            (!group.is_empty())
                .then(|| (t!("settings-plugins-group", title = title, enabled = on, total = group.len()), group))
        })
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Shape};
    use zen_workbench::plugin_manager::{PluginManager, SharedPlugins};

    use super::*;
    use crate::ide::settings::PageId;
    use crate::plugins::fake::{EMPTY_MODULE, FakeSource, plugin};
    use crate::plugins::{PluginConfig, PluginSource, Plugins};

    fn manifest(folder: &std::path::Path, id: &str, name: &str) {
        let target = folder.join(id);
        std::fs::create_dir_all(&target).unwrap();
        let json = serde_json::json!({
            "id": id, "name": name, "version": "0.1.0", "extensions": [id], "notes": format!("- {name} 0.1.0 notes."),
        });
        std::fs::write(target.join("plugin.json"), serde_json::to_vec(&json).unwrap()).unwrap();
        std::fs::write(target.join("plugin.wasm"), EMPTY_MODULE).unwrap();
    }

    pub(in crate::ide) fn installed(root: &std::path::Path, bundled: &[&str], downloaded: &[&str]) -> Arc<Plugins> {
        with_source(root, bundled, downloaded).0
    }

    fn with_source(root: &std::path::Path, bundled: &[&str], downloaded: &[&str]) -> (Arc<Plugins>, Arc<FakeSource>) {
        let name = |id: &str| match id {
            "sql" => "SQL".to_owned(),
            "markdown" => "Markdown".to_owned(),
            other => other.to_owned(),
        };
        std::fs::create_dir_all(root.join("bundle")).unwrap();
        for id in bundled {
            manifest(&root.join("bundle"), id, &name(id));
        }
        for id in downloaded {
            manifest(&root.join("user"), id, &name(id));
        }
        let source = Arc::new(FakeSource::new(vec![
            plugin("markdown", "Markdown", "md", &["0.1.0", "0.3.0"]),
            plugin("sql", "SQL", "sql", &["0.1.0", "0.2.0"]),
            plugin("yaml", "YAML", "yaml", &["0.1.0"]),
        ]));
        let config = PluginConfig {
            folder: root.join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: Some(root.join("bundle")),
            optional: Vec::new(),
        };
        (Plugins::with_source(Some(config), Some(Arc::clone(&source) as Arc<dyn PluginSource>)), source)
    }

    struct Page {
        ctx: egui::Context,
        dialog: SettingsDialog,
        registry: zen_core::Registry,
        session: zen_core::Session,
        plugins: Arc<Plugins>,
        source: Arc<FakeSource>,
        texts: Vec<(String, Rect)>,
        _root: tempfile::TempDir,
    }

    fn texts(shape: &Shape, found: &mut Vec<(String, Rect)>) {
        match shape {
            Shape::Text(text) => found.push((text.galley.text().to_owned(), text.visual_bounding_rect())),
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, found)),
            _ => {}
        }
    }

    impl Page {
        fn new(bundled: &[&str], downloaded: &[&str], tab: PluginsTab) -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let root = tempfile::tempdir().unwrap();
            let (plugins, source) = with_source(root.path(), bundled, downloaded);
            let mut registry = zen_core::Registry::new();
            let shared: SharedPlugins = Arc::clone(&plugins) as _;
            zen_workbench::plugin_manager::register(&mut registry, &shared).unwrap();
            zen_modules::register(&mut registry, &qa_modules(&root.path().join("modules.json"))).unwrap();
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut dialog = SettingsDialog::default();
            dialog.open();
            dialog.section = PageId::Plugins;
            dialog.plugins.tab = tab;
            let mut page = Self {
                ctx,
                dialog,
                registry,
                session: zen_core::Session::new(),
                plugins,
                source,
                texts: Vec::new(),
                _root: root,
            };
            page.settle();
            page
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 900.0))),
                ..Default::default()
            };
            let (dialog, registry, session) = (&mut self.dialog, &self.registry, &mut self.session);
            let mut output = self.ctx.run_ui(input, |ui| {
                let mut dispatch =
                    |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
                dialog.plugins_page(ui, &IdePalette::zen_dark(), Some(&mut dispatch));
            });
            output.textures_delta.clear();
            self.texts.clear();
            for clipped in &output.shapes {
                texts(&clipped.shape, &mut self.texts);
            }
        }

        fn settle(&mut self) {
            let start = Instant::now();
            loop {
                self.frame(Vec::new());
                let list = PluginManager::installed(&*self.plugins, "");
                if list.catalog != CatalogStatus::Loading && list.installing.is_empty() {
                    break;
                }
                assert!(start.elapsed() < Duration::from_secs(60), "the background work did not finish");
                std::thread::sleep(Duration::from_millis(5));
            }
            self.frame(Vec::new());
        }

        fn click(&mut self, label: &str) {
            let at = self
                .texts
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, rect)| rect.center())
                .unwrap_or_else(|| panic!("no {label:?} among {:?}", self.texts));
            self.click_at(at);
        }

        fn click_topmost(&mut self, label: &str) {
            let at = self.texts.iter().rfind(|(text, _)| text == label).map(|(_, rect)| rect.center());
            self.click_at(at.unwrap_or_else(|| panic!("no {label:?} among {:?}", self.texts)));
        }

        fn click_at(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        fn shows(&self, text: &str) -> bool {
            self.texts.iter().any(|(shown, _)| shown == text)
        }

        fn select(&mut self, id: &str) {
            self.dialog.plugins.selected = Some(id.to_owned());
            self.settle();
        }
    }

    const QA_EN: &str = "module-e2e-name = UI Tests\nmodule-e2e-description = Writes and runs end-to-end tests.\n\
        module-api-name = API Testing\nmodule-api-description = Sends requests and checks the answers.\n\
        module-load-name = Load Testing\nmodule-load-description = Runs load scenarios.\n\
        module-go-name = Go\nmodule-go-description = Everything for Go.\n";

    fn nothing(_: &mut zen_modules::ModuleHost<'_>) -> Result<(), zen_core::RegistryError> {
        Ok(())
    }

    const fn module(id: &'static str, name: &'static str, description: &'static str) -> zen_modules::ModuleDescriptor {
        zen_modules::ModuleDescriptor { id, name, description, locales: &[("en", QA_EN)], register: nothing }
    }

    static E2E: zen_modules::ModuleDescriptor = module("e2e", "module-e2e-name", "module-e2e-description");
    static API: zen_modules::ModuleDescriptor = module("api", "module-api-name", "module-api-description");
    static LOAD: zen_modules::ModuleDescriptor = module("load", "module-load-name", "module-load-description");
    static GO: zen_modules::ModuleDescriptor = module("go", "module-go-name", "module-go-description");

    fn qa_modules(file: &std::path::Path) -> zen_modules::SharedModules {
        use zen_modules::BuiltinModule;
        zen_i18n::register(zen_modules::LOCALES);
        let modules = zen_modules::Modules::load(
            "TestTotami",
            vec![BuiltinModule::on(&E2E), BuiltinModule::on(&API), BuiltinModule::off(&LOAD), BuiltinModule::off(&GO)],
            Some(file.to_owned()),
        );
        modules.register_locales();
        Arc::new(std::sync::Mutex::new(modules))
    }

    #[test]
    fn the_built_in_tab_lists_every_module_and_switches_one_for_the_next_start() {
        let mut page = Page::new(&["markdown"], &[], PluginsTab::BuiltIn);
        for name in ["UI Tests", "API Testing", "Load Testing", "Go"] {
            assert!(page.shows(name), "no {name} among {:?}", page.texts);
        }
        assert!(page.shows(&t!("settings-plugins-on-by-default", product = "TestTotami")));
        assert!(page.shows(&t!("settings-plugins-off-by-default", product = "TestTotami")));
        assert!(page.shows("Writes and runs end-to-end tests."), "the first is selected");
        assert!(!page.shows(&t!("settings-plugins-restart-now")), "nothing waits for a restart");

        page.click("Go");
        assert_eq!(page.dialog.plugins.selected_module.as_deref(), Some("go"));
        assert!(page.shows("Everything for Go."));
        page.click(&t!("settings-plugins-enable"));
        assert!(page.shows(&t!("settings-plugins-pending-on")), "{:?}", page.texts);
        assert!(page.shows(&t!("settings-plugins-built-in-restart", product = "TestTotami", names = "Go")));
        let listed = page.registry.dispatch(&mut page.session, "plugins.modules.list", serde_json::json!({})).unwrap();
        let go = listed["modules"].as_array().unwrap().iter().find(|module| module["id"] == "go").unwrap().clone();
        assert_eq!((go["enabled"].clone(), go["active"].clone()), (serde_json::json!(true), serde_json::json!(false)));
        let saved = std::fs::read_to_string(page._root.path().join("modules.json")).unwrap();
        assert!(saved.contains("\"go\""), "{saved}");

        page.dialog.plugins.search = "load".to_owned();
        page.settle();
        assert!(page.shows("Load Testing") && !page.shows("UI Tests"));
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            page.dialog.plugins.search.clear();
            page.dialog.plugins.selected_module = Some("go".to_owned());
            zen_i18n::set_language(Some("en"));
            let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 560.0), 2.0);
            let palette = IdePalette::zen_dark();
            palette.apply(screen.context());
            let (dialog, registry, session) = (&mut page.dialog, &page.registry, &mut page.session);
            let mut image = None;
            for _ in 0..3 {
                image = Some(screen.render(|ui| {
                    ui.painter().rect_filled(ui.max_rect(), 0, palette.popup);
                    egui::Frame::new().inner_margin(16).show(ui, |ui| {
                        let mut dispatch = |id: &str, params| {
                            registry.dispatch(session, id, params).map_err(|error| error.to_string())
                        };
                        dialog.plugins_page(ui, &palette, Some(&mut dispatch));
                    });
                }));
            }
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("plugins-built-in.png"), &image.unwrap())
                .unwrap();
        }
    }

    #[test]
    fn the_built_in_tab_shows_the_areas_and_offers_their_choice() {
        let mut page = Page::new(&["markdown"], &[], PluginsTab::BuiltIn);
        assert!(!page.shows(&t!("settings-plugins-areas")), "a product without areas shows none");
        let modules = qa_modules(&page._root.path().join("modules.json"));
        let areas: zen_modules::SharedAreas = Arc::new(std::sync::Mutex::new(zen_modules::Areas::load(
            "TestTotami",
            vec!["load".to_owned(), "markdown".to_owned()],
            modules,
            Some(Arc::clone(&page.plugins) as Arc<dyn zen_modules::AreaPlugins>),
            None,
        )));
        zen_modules::register_areas(&mut page.registry, &areas).unwrap();
        page.dialog.plugins.search = "go".to_owned();
        page.settle();
        page.dialog.plugins.search.clear();
        page.settle();
        assert!(page.shows(&t!("settings-plugins-areas")), "{:?}", page.texts);
        let markdown = PluginManager::installed(&*page.plugins, "").plugins[0].name.clone();
        assert!(page.shows(&t!("settings-plugins-areas-on", names = markdown.as_str())), "{:?}", page.texts);
        page.click(&t!("settings-plugins-areas-choose"));
        assert!(zen_modules::Areas::lock(&areas).take_show(), "the dialog is asked for");
    }

    #[test]
    fn restart_ide_on_the_built_in_tab_asks_for_the_restart() {
        let mut page = Page::new(&["markdown"], &[], PluginsTab::BuiltIn);
        page.click("Load Testing");
        page.click(&t!("settings-plugins-enable"));
        assert!(!page.dialog.plugins.restart);
        page.click(&t!("settings-plugins-restart-now"));
        assert!(page.dialog.plugins.restart, "the dialog then applies itself, closes and restarts the IDE");
    }

    #[test]
    fn the_marketplace_searches_and_installs() {
        let mut page = Page::new(&["markdown"], &[], PluginsTab::Marketplace);
        assert!(page.shows("Markdown") && page.shows("SQL") && page.shows("YAML"), "{:?}", page.texts);
        assert!(page.shows(&t!("settings-plugins-is-installed")), "the bundled one is installed");
        page.click("SQL");
        assert_eq!(page.dialog.plugins.selected.as_deref(), Some("sql"));
        assert!(page.shows("Supports SQL files.") && page.shows("SQL 0.2.0 notes."));
        page.dialog.plugins.search = "yam".to_owned();
        page.settle();
        assert!(page.shows("YAML") && !page.shows("SQL"));
        page.click(&t!("settings-plugins-install"));
        page.dialog.plugins.tab = PluginsTab::Installed;
        page.dialog.plugins.search.clear();
        page.settle();
        assert!(page.shows("YAML"), "{:?}", page.texts);
        assert!(page.plugins.highlighter(&[], "a.yaml").is_some());
    }

    #[test]
    fn installed_updates_all_after_checking_and_shows_the_notes() {
        let mut page = Page::new(&["markdown"], &["sql"], PluginsTab::Installed);
        assert!(!page.shows(&t!("settings-plugins-update-all")), "no update is known before asking");
        page.click(&t!("settings-plugins-check-updates"));
        assert!(page.shows(&t!("settings-plugins-updates", count = 2)), "{:?}", page.texts);
        page.select("markdown");
        assert!(page.shows(&t!("settings-plugins-whats-new")));
        assert!(page.shows("Markdown 0.3.0 notes.") && page.shows("Markdown 0.1.0 notes."), "{:?}", page.texts);
        assert!(page.shows(&t!("settings-plugins-version-update", installed = "0.1.0", latest = "0.3.0")));

        page.click(&t!("settings-plugins-update-all"));
        assert!(!page.shows(&t!("settings-plugins-update-all")));
        let versions: Vec<_> = PluginManager::installed(&*page.plugins, "")
            .plugins
            .into_iter()
            .map(|plugin| (plugin.id, plugin.installed.unwrap_or_default()))
            .collect();
        assert_eq!(versions, [("markdown".to_owned(), "0.3.0".to_owned()), ("sql".to_owned(), "0.2.0".to_owned())]);
        page.select("markdown");
        assert!(page.shows("Markdown 0.3.0 notes.") && !page.shows("Markdown 0.1.0 notes."));
    }

    #[test]
    fn a_failed_check_keeps_the_updates_found_before() {
        let mut page = Page::new(&["markdown"], &["sql"], PluginsTab::Installed);
        page.click(&t!("settings-plugins-check-updates"));
        assert!(page.shows(&t!("settings-plugins-updates", count = 2)));
        *page.source.offline.lock().unwrap() = true;
        page.click(&t!("settings-plugins-check-updates"));
        assert!(page.shows(&t!("settings-plugins-updates", count = 2)), "{:?}", page.texts);
        assert!(page.shows(&t!("settings-plugins-check-failed", error = "offline")), "{:?}", page.texts);
        page.dialog.plugins.tab = PluginsTab::Marketplace;
        page.settle();
        assert!(page.shows("YAML"), "the Marketplace still lists the catalog");
    }

    #[test]
    fn the_page_asks_for_the_list_only_when_something_changed() {
        let mut page = Page::new(&["markdown"], &[], PluginsTab::Marketplace);
        let fetched_at = page.dialog.plugins.shown.as_ref().unwrap().at;
        page.frame(Vec::new());
        page.frame(Vec::new());
        assert_eq!(page.dialog.plugins.shown.as_ref().unwrap().at, fetched_at, "idle frames reuse the list");
        page.dialog.plugins.search = "sq".to_owned();
        page.frame(Vec::new());
        assert_ne!(page.dialog.plugins.shown.as_ref().unwrap().at, fetched_at, "a new search asks again");
        assert!(page.shows("SQL") && !page.shows("YAML"));
    }

    #[test]
    fn a_bundled_plugin_offers_disable_but_no_uninstall() {
        let mut page = Page::new(&["markdown"], &["sql"], PluginsTab::Installed);
        page.select("markdown");
        assert!(page.shows(&t!("settings-plugins-disable")) && !page.shows(&t!("settings-plugins-uninstall")));
        page.click(&t!("settings-plugins-disable"));
        assert!(page.plugins.highlighter(&[], "a.markdown").is_none());
        assert!(page.shows(&t!("settings-plugins-enable")));
        page.select("sql");
        page.click(&t!("settings-plugins-uninstall"));
        assert!(page.shows(&t!("settings-plugins-uninstall-question", name = "SQL")), "{:?}", page.texts);
        assert_eq!(
            PluginManager::installed(&*page.plugins, "sql").plugins.len(),
            1,
            "nothing is gone before the answer"
        );
        page.click(&t!("settings-cancel"));
        assert_eq!(PluginManager::installed(&*page.plugins, "sql").plugins.len(), 1, "Cancel keeps it");
        page.click(&t!("settings-plugins-uninstall"));
        page.click_topmost(&t!("settings-plugins-uninstall-go"));
        assert!(page.shows(&t!("settings-plugins-uninstalled", name = "SQL")), "{:?}", page.texts);
        assert!(PluginManager::installed(&*page.plugins, "sql").plugins.is_empty());
    }

    #[test]
    fn installed_plugins_are_grouped_into_downloaded_and_bundled() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let root = tempfile::tempdir().unwrap();
        let plugins = installed(root.path(), &["markdown", "yaml"], &["sql"]);
        plugins.set_enabled("yaml", false).unwrap();
        let list = PluginManager::installed(&*plugins, "").plugins;
        let groups: Vec<_> = installed_groups(&list)
            .into_iter()
            .map(|(title, group)| (title, group.iter().map(|plugin| plugin.id.as_str()).collect::<Vec<_>>()))
            .collect();
        let title = |key: &str, enabled: usize, total: usize| {
            t!("settings-plugins-group", title = t!(key), enabled = enabled, total = total)
        };
        assert_eq!(
            groups,
            [
                (title("settings-plugins-downloaded", 1, 1), vec!["sql"]),
                (title("settings-plugins-bundled-group", 1, 2), vec!["markdown", "yaml"]),
            ]
        );
        let only_bundled: Vec<_> = list.iter().filter(|plugin| plugin.bundled).cloned().collect();
        assert_eq!(installed_groups(&only_bundled).len(), 1, "an empty group has no title");
    }
}

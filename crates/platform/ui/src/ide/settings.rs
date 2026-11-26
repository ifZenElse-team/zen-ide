mod actions_on_save;
mod appearance;
mod caffeine;
mod certificates;
pub(super) mod code_style_page;
mod code_style_schemes;
mod editor;
mod editor_tabs;
mod external_tools;
pub(super) mod file_encodings;
mod file_templates;
pub(super) mod file_types;
mod file_watchers;
mod form;
mod inspections;
mod markdown;
mod menus_toolbars;
mod model;
mod network;
mod notifications;
pub(super) mod page;
mod plugins;
mod project_structure;
mod schemas;
mod search;
mod spelling;
mod templates;
mod terminal;
mod todo;
mod trusted_locations;
pub(super) mod widgets;

use std::path::PathBuf;

use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use zen_i18n::t;

use super::IdePalette;
use crate::{accessibility, theme};
use appearance::ThemeChoice;

const REVEAL_ROOM: f32 = 24.0;
pub(crate) use model::{AUTOSAVE_IDLE_SECONDS, DEFAULT_PRESENTATION_FONT_SIZE, PRESENTATION_FONT_SIZES};
pub use model::{
    CodeStyleScheme, DEFAULT_UI_FONT_SIZE, EditorFont, FoldArrows, IdeSettings, NavigationBarPlacement, UI_FONT_SIZES,
    WheelFontSize,
};
pub(crate) use model::{DEFAULT_IDE_ZOOM, IDE_ZOOM_PRESETS, IDE_ZOOMS, OpenProjectIn};
pub(super) use page::{PageId, SECTIONS, SectionDef};
use search::{SearchIndex, SettingsSearch};
use zen_project::generators::GeneratorSetting;
use zen_workbench::spelling::DictionaryLevel;

pub(super) struct SettingsContext<'a> {
    pub(super) settings: &'a IdeSettings,
    pub(super) mcp_url: Option<&'a str>,
    pub(super) product_id: &'a str,
    pub(super) executable: Option<PathBuf>,
    pub(super) form: Option<zen_modules::form::Form>,
    pub(super) sections: &'a [SectionDef],
    pub(super) language_servers: Option<&'a [super::language_servers::ServerInfo]>,
    pub(super) declared: &'a [zen_project::inspect::DeclaredSetting],
    pub(super) folding: &'a [zen_modules::folding::FoldingOptions],
}

pub(super) fn module_texts(folding: &[zen_modules::folding::FoldingOptions]) -> Vec<(PageId, &'static str)> {
    folding
        .iter()
        .flat_map(|group| std::iter::once(group.title).chain(group.options.iter().map(|option| option.label)))
        .map(|key| (PageId::CodeFolding, key))
        .collect()
}

fn section_row_id(index: usize) -> egui::Id {
    egui::Id::unique(("settings-section", index))
}

fn generator_settings(dispatch: &mut Dispatch<'_>) -> Vec<(String, GeneratorSetting)> {
    let list = dispatch("project.new.generators", serde_json::json!({})).unwrap_or_default();
    let generators = list["generators"].as_array().cloned().unwrap_or_default();
    let of_generator = |generator: serde_json::Value| {
        let info: zen_project::generators::GeneratorInfo = serde_json::from_value(generator).ok()?;
        let settings = info.settings.into_iter().map(move |setting| (info.name.clone(), setting));
        Some(settings.collect::<Vec<_>>())
    };
    generators.into_iter().filter_map(of_generator).flatten().collect()
}

#[derive(Default)]
pub(super) struct SettingsDialog {
    open: bool,
    draft: Option<IdeSettings>,
    port: String,
    section: PageId,
    copied: Option<f64>,
    plugins: plugins::PluginsPage,
    agent_form: [String; 3],
    keymap: super::keymap_page::KeymapPage,
    templates: templates::TemplatesPage,
    postfix: templates::PostfixPage,
    external_tools: external_tools::ExternalToolsPage,
    schemas: schemas::SchemasPage,
    code_styles: code_style_page::CodeStylePages,
    pub(super) style_pages: Vec<zen_modules::CodeStylePage>,
    file_watchers: file_watchers::FileWatchersPage,
    trusted_locations: trusted_locations::TrustedLocationsPage,
    todo: todo::TodoPage,
    pub(super) file_types: file_types::FileTypesPage,
    pub(super) file_encodings: file_encodings::FileEncodingsPage,
    pub(super) project_structure: project_structure::ProjectStructurePage,
    spelling: spelling::SpellingPage,
    generator_settings: Vec<(String, GeneratorSetting)>,
    proxy: network::ProxyPage,
    certificates: certificates::CertificatesPage,
    inspections: inspections::InspectionsPage,
    pub(super) open_request: Option<PathBuf>,
    file_templates: file_templates::FileTemplatesPage,
    pub(super) form_request: Option<(String, serde_json::Value)>,
    servers_page: super::language_servers::ServersPage,
    pub(super) server_request: Option<(&'static str, serde_json::Value)>,
    pub(super) restart_request: bool,
    form: form::FormPage,
    editor_config_export: Option<editor::ExportOutcome>,
    scheme: code_style_schemes::SchemePage,
    on_save: actions_on_save::OnSavePage,
    menus: menus_toolbars::MenusPage,
    theme: ThemeChoice,
    search: SettingsSearch,
    reveal_section: bool,
    page_view: Option<(f32, f32, f32)>,
    #[cfg(test)]
    section_list: Option<Rect>,
    #[cfg(test)]
    section_rows: Vec<(usize, Rect, Rect)>,
    #[cfg(test)]
    page_scroll: Option<(Rect, Rect)>,
    #[cfg(test)]
    page_area: Option<Rect>,
    #[cfg(test)]
    buttons_top: Option<f32>,
}

impl SettingsDialog {
    fn keyed_offset(&self, ui: &mut Ui) -> Option<f32> {
        let (offset, view, content) = self.page_view?;
        let search = egui::Id::unique(SEARCH_FIELD);
        if ui.memory(|memory| memory.focused().is_some_and(|focused| focused != search)) {
            return None;
        }
        let bottom = (content - view).max(0.0);
        let step = (view - 40.0).max(view / 2.0);
        let target = ui.input_mut(|input| {
            let none = egui::Modifiers::NONE;
            if input.consume_key(none, egui::Key::PageDown) {
                Some(offset + step)
            } else if input.consume_key(none, egui::Key::PageUp) {
                Some(offset - step)
            } else if input.consume_key(none, egui::Key::End) {
                Some(bottom)
            } else if input.consume_key(none, egui::Key::Home) {
                Some(0.0)
            } else {
                None
            }
        })?;
        Some(target.clamp(0.0, bottom))
    }
}

const SEARCH_FIELD: &str = "settings-search-field";

const PLUGINS_REFRESH: f64 = 1.0;

pub(super) type Dispatch<'a> = dyn FnMut(&str, serde_json::Value) -> Result<serde_json::Value, String> + 'a;

impl SettingsDialog {
    pub(super) fn open(&mut self) {
        self.on_save.for_new_projects = false;
        self.release_keymap_search();
        self.release_plugins_search();
        self.open = true;
        self.draft = None;
        self.certificates = certificates::CertificatesPage::default();
        self.section = PageId::Mcp;
        self.search = SettingsSearch::default();
        self.reveal_section = true;
    }

    pub(super) fn open_at(&mut self, page: PageId) {
        self.open();
        self.section = page;
    }

    pub(super) fn embedded_plugins(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) -> bool {
        egui::ScrollArea::vertical()
            .id_salt("embedded-plugins")
            .auto_shrink([false, false])
            .show(ui, |ui| self.plugins_page(ui, p, Some(&mut *dispatch)));
        std::mem::take(&mut self.plugins.restart)
    }

    pub(super) fn open_for_new_projects(&mut self) {
        self.open_at(PageId::ActionsOnSave);
        self.on_save.for_new_projects = true;
    }

    pub(super) fn open_customize(&mut self, only: Option<&'static str>, add: bool) {
        self.open_at(PageId::MenusAndToolbars);
        self.menus = menus_toolbars::MenusPage::for_group(only, add);
    }

    pub(super) fn show_project_structure_part(&mut self, part: &str) -> bool {
        self.project_structure.show_part(part)
    }

    pub(super) fn show_plugins_tab(&mut self, tab: &str) -> bool {
        self.plugins.tab = match tab {
            "marketplace" => plugins::PluginsTab::Marketplace,
            "installed" => plugins::PluginsTab::Installed,
            "builtIn" => plugins::PluginsTab::BuiltIn,
            _ => return false,
        };
        self.section = PageId::Plugins;
        true
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn open_page(&self) -> Option<PageId> {
        self.open.then_some(self.section)
    }

    #[cfg(test)]
    pub(super) fn search_text(&self) -> &str {
        &self.search.text
    }

    pub(super) fn focus_search(&mut self) {
        self.search.focus = true;
    }

    pub(super) fn search_for(
        &mut self,
        query: &str,
        sections: &[SectionDef],
        more: &[(PageId, &str)],
        now: f64,
        dispatch: Option<&mut Dispatch<'_>>,
    ) -> serde_json::Value {
        query.clone_into(&mut self.search.text);
        let query = self.search.query().map(str::to_owned);
        self.run_search(query.clone(), sections, more, now, dispatch);
        let text = query.as_deref().unwrap_or_default();
        let pages: Vec<String> = search::matches_with(text, sections, more, |key| t!(key), &self.search.found)
            .into_iter()
            .map(PageId::name)
            .collect();
        let tab = match self.plugins.tab {
            plugins::PluginsTab::Marketplace => "marketplace",
            plugins::PluginsTab::Installed => "installed",
            plugins::PluginsTab::BuiltIn => "builtIn",
        };
        serde_json::json!({
            "query": query.unwrap_or_default(),
            "pages": pages,
            "page": self.section.name(),
            "keymapSearch": self.keymap.search(),
            "plugins": { "tab": tab, "search": self.plugins.search },
        })
    }

    fn run_search(
        &mut self,
        query: Option<String>,
        sections: &[SectionDef],
        more: &[(PageId, &str)],
        now: f64,
        dispatch: Option<&mut Dispatch<'_>>,
    ) {
        let found = match query.as_deref() {
            Some(query) => {
                self.refresh_index(now, dispatch);
                self.search.index.as_ref().map(|index| index.found(query)).unwrap_or_default()
            }
            None => {
                self.search.index = None;
                Vec::new()
            }
        };
        match query.as_deref().filter(|_| found.contains(&PageId::Keymap)) {
            Some(query) => {
                let before = match self.search.keymap_query.take() {
                    Some((set, before)) if self.keymap.search() == set => before,
                    _ => self.keymap.search().to_owned(),
                };
                self.keymap.set_search(query);
                self.search.keymap_query = Some((query.to_owned(), before));
            }
            None => self.release_keymap_search(),
        }
        match query.as_deref().filter(|_| found.contains(&PageId::Plugins)) {
            Some(query) => {
                let before = match self.search.plugins_query.take() {
                    Some((set, search, tab))
                        if self.plugins.search == set && self.plugins.tab == plugins::PluginsTab::Installed =>
                    {
                        (search, tab)
                    }
                    _ => (self.plugins.search.clone(), self.plugins.tab),
                };
                self.plugins.tab = plugins::PluginsTab::Installed;
                query.clone_into(&mut self.plugins.search);
                self.search.plugins_query = Some((query.to_owned(), before.0, before.1));
            }
            None => self.release_plugins_search(),
        }
        self.search.found = found;
        let text = query.as_deref().unwrap_or_default();
        let titled = search::titled(text, sections, |key| t!(key));
        let matched = search::matches_with(text, sections, more, |key| t!(key), &self.search.found);
        let own = search::matches_with(text, sections, more, |key| t!(key), &[]);
        let section = search::page_for(self.section, &matched, &titled, &own);
        self.reveal_section |= section != self.section;
        self.section = section;
        self.search.reveal = query.is_some();
        self.search.last = query;
    }

    fn refresh_index(&mut self, now: f64, dispatch: Option<&mut Dispatch<'_>>) {
        let language = zen_i18n::language();
        let mut index = match self.search.index.take().filter(|index| index.language == language) {
            Some(index) => index,
            None => {
                let actions = crate::keymap::current().bindings().into_iter().map(|binding| {
                    (
                        super::keymap_page::action_title(&binding.action),
                        super::keymap_page::action_title_english(&binding.action),
                    )
                });
                SearchIndex::new(language, f64::NEG_INFINITY, actions, None)
            }
        };
        let stale = index.plugins.is_none() || !(index.at..index.at + PLUGINS_REFRESH).contains(&now);
        if stale && let Some(dispatch) = dispatch {
            let list = dispatch("plugins.installed.list", serde_json::json!({})).and_then(|value| {
                serde_json::from_value::<zen_workbench::plugin_manager::PluginList>(value)
                    .map_err(|error| error.to_string())
            });
            index.plugins = Some(list.map(|list| list.plugins).unwrap_or_default());
            index.at = now;
        }
        self.search.index = Some(index);
    }

    fn release_keymap_search(&mut self) {
        if let Some((query, before)) = self.search.keymap_query.take()
            && self.keymap.search() == query
        {
            self.keymap.set_search(&before);
        }
    }

    fn release_plugins_search(&mut self) {
        if let Some((query, before, tab)) = self.search.plugins_query.take()
            && self.plugins.search == query
            && self.plugins.tab == plugins::PluginsTab::Installed
        {
            self.plugins.search = before;
            self.plugins.tab = tab;
        }
    }

    pub(super) fn open_language_servers(&mut self) {
        self.open();
        self.section = PageId::LanguageServers;
    }

    pub(super) fn shows_language_servers(&self) -> bool {
        self.open && self.section == PageId::LanguageServers
    }

    #[cfg(test)]
    pub(super) fn show_page(&mut self, page: PageId) {
        self.section = page;
    }

    #[cfg(test)]
    pub(super) fn keymap_page(&mut self) -> &mut super::keymap_page::KeymapPage {
        &mut self.keymap
    }

    pub(super) fn is_recording(&self) -> bool {
        self.open && self.keymap.is_recording()
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        context: &SettingsContext<'_>,
        mut dispatch: Option<&mut Dispatch<'_>>,
    ) -> Option<IdeSettings> {
        if !self.open {
            return None;
        }
        if self.draft.is_none()
            && let Some(dispatch) = dispatch.as_deref_mut()
        {
            self.theme.begin(dispatch);
            self.keymap.begin(dispatch);
            self.templates.begin(dispatch);
            self.postfix.begin(dispatch);
            self.file_types.begin(dispatch);
            self.file_encodings.begin(dispatch);
            self.project_structure.begin(dispatch);
            self.spelling.begin(dispatch);
            self.inspections.begin(dispatch);
            self.file_templates.begin(dispatch);
            self.external_tools.begin(dispatch);
            self.generator_settings = generator_settings(dispatch);
            self.file_watchers.begin(dispatch);
            self.scheme.begin(dispatch);
            self.trusted_locations.begin(dispatch);
            self.code_styles.begin(dispatch);
            self.on_save.begin(dispatch);
        }
        let draft = self.draft.get_or_insert_with(|| context.settings.clone()).clone();
        if self.port.is_empty() {
            self.port = draft.mcp_port.to_string();
        }
        let mut applied = None;
        let mut save_templates = false;
        let mut close = false;
        let mut cancel = false;
        if !self.keymap.is_recording() {
            let (find, escape) = ui.input_mut(|input| {
                let find = input.consume_key(egui::Modifiers::COMMAND, egui::Key::F);
                let escape =
                    !self.search.text.is_empty() && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                (find, escape)
            });
            if find {
                self.search.focus = true;
            }
            if escape {
                self.search.clear();
            }
        }
        let query = self.search.query().map(str::to_owned);
        let more = module_texts(context.folding);
        if self.search.last != query {
            let now = ui.input(|input| input.time);
            self.run_search(query.clone(), context.sections, &more, now, dispatch.as_deref_mut());
        }
        let matched = search::matches_with(
            query.as_deref().unwrap_or_default(),
            context.sections,
            &more,
            |key| t!(key),
            &self.search.found,
        );
        let modal = egui::Modal::new(ui.scope_id().with("settings")).show(ui.ctx(), |ui| {
            let screen = ui.ctx().content_rect();
            ui.set_width((screen.width() - 80.0).clamp(560.0, 880.0));
            ui.set_height((screen.height() - 120.0).clamp(420.0, 560.0));
            let rect = ui.max_rect();
            let sidebar = Rect::from_min_size(rect.min, vec2(250.0, rect.height() - 48.0));
            ui.painter().rect_filled(sidebar, 6, p.frame);
            let field = Rect::from_min_size(
                pos2(sidebar.left() + 6.0, sidebar.top() + 8.0),
                vec2(sidebar.width() - 12.0, 26.0),
            );
            let response = ui.put(
                field,
                egui::TextEdit::singleline(&mut self.search.text)
                    .id(egui::Id::unique(SEARCH_FIELD))
                    .hint_text(t!("settings-search-placeholder"))
                    .align(Align2::LEFT_CENTER),
            );
            accessibility::edit(&response, |node| node.set_label(t!("settings-search-placeholder")));
            if std::mem::take(&mut self.search.focus) {
                response.request_focus();
            }
            let rows = search::visible_rows(context.sections, &matched);
            let list = Rect::from_min_max(
                pos2(sidebar.left(), field.bottom() + 8.0),
                pos2(sidebar.right(), sidebar.bottom() - 6.0),
            );
            #[cfg(test)]
            {
                self.section_list = Some(list);
                self.section_rows.clear();
            }
            if rows.is_empty() {
                ui.painter().text(
                    pos2(sidebar.left() + 16.0, list.top() + 13.0),
                    Align2::LEFT_CENTER,
                    t!("settings-search-nothing"),
                    theme::regular(13.0),
                    p.text_secondary,
                );
            }
            let reveal = std::mem::take(&mut self.reveal_section);
            let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
            egui::ScrollArea::vertical().id_salt("settings-sections").auto_shrink([false, false]).show(
                &mut list_ui,
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for row in rows {
                        let section = &context.sections[row.index];
                        let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
                        let item = Rect::from_min_size(
                            pos2(sidebar.left() + 6.0, slot.top()),
                            vec2(sidebar.width() - 12.0, 26.0),
                        );
                        let response = ui.interact(item, section_row_id(row.index), Sense::click());
                        #[cfg(test)]
                        self.section_rows.push((row.index, item, item.intersect(ui.clip_rect())));
                        if section.page == self.section {
                            ui.painter().rect_filled(item, 5, p.selection);
                            if reveal {
                                response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                            }
                        } else if response.hovered() {
                            ui.painter().rect_filled(item, 5, p.hover);
                        }
                        let x = item.left() + 10.0 + f32::from(section.level) * 16.0;
                        let color = if row.matched { p.text } else { p.text_secondary };
                        let title = widgets::highlighted(
                            &t!(section.key),
                            query.as_deref(),
                            theme::regular(13.0),
                            color,
                            p.search_highlight,
                        );
                        let galley = ui.painter().layout_job(title);
                        let level = usize::from(section.level) + 1;
                        accessibility::tree_item(&response, level, None, section.page == self.section, galley.text());
                        let clip = item.shrink2(vec2(4.0, 0.0)).intersect(ui.clip_rect());
                        ui.painter().with_clip_rect(clip).galley(
                            pos2(x, item.center().y - galley.size().y / 2.0),
                            galley,
                            color,
                        );
                        if response.clicked() {
                            self.section = section.page;
                        }
                    }
                },
            );

            let page = Rect::from_min_max(
                pos2(sidebar.right() + 20.0, rect.top() + 4.0),
                pos2(rect.right(), sidebar.bottom()),
            );
            let mut page_ui = ui.new_child(egui::UiBuilder::new().max_rect(page));
            page_ui.set_clip_rect(page.intersect(ui.clip_rect()));
            page_ui.label(
                egui::RichText::new(t!(self.section.section_in(context.sections).key))
                    .font(theme::semibold(15.0))
                    .color(p.text),
            );
            page_ui.add_space(12.0);
            widgets::forget_first_match(ui.ctx());
            let mut area =
                egui::ScrollArea::vertical().id_salt(("settings-page", self.section)).auto_shrink([false, false]);
            if let Some(offset) = self.search.scroll_to.take().or_else(|| self.keyed_offset(ui)) {
                area = area.vertical_scroll_offset(offset);
            }
            let scroll = area.show(&mut page_ui, |ui| {
                match self.section {
                    PageId::Mcp => self.mcp_page(ui, p, context),
                    PageId::Appearance => self.appearance_page(ui, p, query.as_deref(), dispatch.as_deref_mut()),
                    PageId::Module(id) => self.module_page(ui, p, context, id, query.as_deref()),
                    PageId::SystemSettings => self.system_page(ui, p, query.as_deref()),
                    PageId::HttpProxy => self.proxy_page(ui, p, query.as_deref()),
                    PageId::ServerCertificates => self.certificates_page(ui, p, query.as_deref()),
                    PageId::Notifications => self.notifications_page(ui, p, query.as_deref()),
                    PageId::Editor => self.editor_page(ui, p, query.as_deref()),
                    PageId::EditorGeneral => self.general_page(ui, p, context.sections),
                    PageId::Todo => self.todo_page(ui, p, query.as_deref()),
                    PageId::EditorTabs => self.editor_tabs_page(ui, p, query.as_deref()),
                    PageId::CodeFolding => self.code_folding_page(ui, p, query.as_deref(), context.folding),
                    PageId::PostfixCompletion => self.postfix.show(ui, p),
                    PageId::EditorFont => self.font_page(ui, p, query.as_deref()),
                    PageId::MenusAndToolbars => self.menus_page(ui, p),
                    PageId::CodeStyleOf(language) => self.code_style_language_page(ui, p, query.as_deref(), language),
                    PageId::CodeStyleOther => self.code_style_other_page(ui, p, query.as_deref()),
                    PageId::InlayHints => self.inlay_hints_page(ui, p, query.as_deref()),
                    PageId::CodeVision => self.code_vision_page(ui, p, query.as_deref()),
                    PageId::Inspections => self.inspections_page(ui, p, query.as_deref(), dispatch.as_deref_mut()),
                    PageId::LiveTemplates => self.templates.show(ui, p),
                    PageId::FileTypes => self.file_types.show(ui, p),
                    PageId::FileEncodings => {
                        if let Some(draft) = self.draft.as_mut() {
                            self.file_encodings.show(ui, p, &mut draft.global_encoding);
                        }
                    }
                    PageId::NaturalLanguages => self.natural_languages_page(ui, p, query.as_deref()),
                    PageId::Spelling => self.spelling_page(ui, p, query.as_deref()),
                    PageId::FileTemplates => self.file_templates.show(ui, p, dispatch.as_deref_mut()),
                    PageId::Keymap => self.keymap.show(ui, p, dispatch.as_deref_mut()),
                    PageId::LanguageServers => {
                        if let Some(request) = self.servers_page.show(ui, p, context.language_servers) {
                            self.server_request = Some(request);
                        }
                    }
                    PageId::Plugins => self.plugins_page(ui, p, dispatch.as_deref_mut()),
                    PageId::Schemas => self.schemas.show(ui, p, query.as_deref(), dispatch.as_deref_mut()),
                    PageId::ActionsOnSave => self.actions_on_save_page(ui, p),
                    PageId::ExternalTools => self.external_tools.show(ui, p, dispatch.as_deref_mut()),
                    PageId::FileWatchers => {
                        self.file_watchers.show(ui, p, dispatch.as_deref_mut());
                        if std::mem::take(&mut self.file_watchers.actions_on_save) {
                            self.section = PageId::ActionsOnSave;
                        }
                    }
                    PageId::Caffeine => self.caffeine_page(ui, p, query.as_deref()),
                    PageId::TrustedLocations => self.trusted_locations.show(ui, p),
                    PageId::VersionControl => self.vcs_page(ui, p, query.as_deref()),
                    PageId::ProjectStructure => self.project_structure.show(ui, p),
                    PageId::CodeStyle => self.code_style_general_page(ui, p, query.as_deref(), dispatch.as_deref_mut()),
                    PageId::Terminal => self.terminal_page(ui, p, query.as_deref()),
                    PageId::Build => self.build_page(ui, p, query.as_deref()),
                    PageId::Debugger => self.debugger_page(ui, p, query.as_deref()),
                    PageId::DataViews => self.data_views_page(ui, p, query.as_deref()),
                    PageId::Markdown => self.markdown_page(ui, p, query.as_deref()),
                    PageId::Languages | PageId::Tools => in_development(ui, p),
                    PageId::Advanced => self.advanced_page(ui, p, query.as_deref()),
                }
                ui.min_rect()
            });
            if std::mem::take(&mut self.search.reveal)
                && let Some(top) = widgets::first_match(ui.ctx())
                && !(scroll.inner_rect.top()..scroll.inner_rect.bottom() - REVEAL_ROOM).contains(&top)
            {
                self.search.scroll_to =
                    Some((top - scroll.inner_rect.top() + scroll.state.clamped_offset().y - 8.0).max(0.0));
                ui.ctx().request_repaint();
            }
            self.page_view = Some((scroll.state.clamped_offset().y, scroll.inner_rect.height(), scroll.content_size.y));
            #[cfg(test)]
            {
                self.page_scroll = Some((scroll.inner_rect, scroll.inner));
                self.page_area = Some(page);
            }

            let mut right = rect.right();
            let bottom = rect.bottom() - 34.0;
            #[cfg(test)]
            {
                self.buttons_top = Some(bottom);
            }
            for (label, primary) in [("settings-apply", false), ("settings-cancel", false), ("settings-ok", true)] {
                let galley = ui.painter().layout_no_wrap(t!(label), theme::regular(13.0), p.text);
                let button = Rect::from_min_size(
                    pos2(right - galley.size().x - 28.0, bottom),
                    vec2(galley.size().x + 28.0, 30.0),
                );
                let response = ui.interact(button, ui.scope_id().with(label), Sense::click());
                accessibility::name(
                    &response,
                    if primary { egui::Role::DefaultButton } else { egui::Role::Button },
                    galley.text(),
                );
                let fill = if primary {
                    p.accent
                } else if response.hovered() {
                    p.popup_hover
                } else {
                    p.island
                };
                ui.painter().rect_filled(button, 6, fill);
                if !primary {
                    ui.painter().rect_stroke(button, 6, Stroke::new(1.0, p.button_border), StrokeKind::Inside);
                }
                ui.painter().galley(
                    button.center() - galley.size() / 2.0,
                    galley,
                    if primary { p.accent_text } else { p.text },
                );
                if response.clicked() {
                    if label != "settings-cancel" {
                        self.keymap.commit();
                        save_templates = true;
                    }
                    match label {
                        "settings-cancel" => {
                            close = true;
                            cancel = true;
                        }
                        "settings-ok" => {
                            applied = self.draft.clone();
                            close = true;
                            self.theme.commit();
                        }
                        _ => {
                            applied = self.draft.clone();
                            self.theme.commit();
                        }
                    }
                }
                right = button.left() - 8.0;
            }
            ui.allocate_rect(rect, Sense::hover());
        });
        if std::mem::take(&mut self.plugins.restart) {
            self.keymap.commit();
            self.theme.commit();
            save_templates = true;
            applied = self.draft.clone();
            close = true;
            self.restart_request = true;
        }
        if let Some(page) = self.project_structure.goto.take()
            && let Some(page) = PageId::from_name(page, context.sections)
        {
            self.section = page;
        }
        if let Some(path) = self.project_structure.open.take().or_else(|| self.spelling.open.take()) {
            self.keymap.commit();
            self.theme.commit();
            save_templates = true;
            applied = self.draft.clone();
            close = true;
            self.open_request = Some(path);
        }
        if applied.is_some() {
            self.keep_proxy_password();
            self.certificates.commit();
        }
        if save_templates && let Some(dispatch) = dispatch.as_deref_mut() {
            self.templates.commit(dispatch);
            self.postfix.commit(dispatch);
            self.file_types.commit(dispatch);
            self.file_encodings.commit(dispatch);
            self.project_structure.commit(dispatch);
            self.scheme.commit(dispatch);
            self.on_save.commit(dispatch);
            let level = self.draft.as_ref().map_or_else(DictionaryLevel::default, |draft| {
                if draft.spelling.use_single_dictionary {
                    draft.spelling.saving_dictionary
                } else {
                    DictionaryLevel::Application
                }
            });
            self.spelling.commit(dispatch, level);
            self.inspections.commit(dispatch);
            self.file_templates.commit(dispatch);
            self.external_tools.commit(dispatch);
            self.file_watchers.commit(dispatch);
            self.trusted_locations.commit(dispatch);
            let scheme = self.draft.as_ref().map_or(CodeStyleScheme::Project, |draft| draft.code_style_scheme);
            self.code_styles.commit(dispatch, scheme);
        }
        let dismissed = modal.should_close();
        if dismissed && !close {
            cancel = true;
        }
        if cancel && let Some(dispatch) = dispatch {
            self.theme.cancel(dispatch);
            self.keymap.cancel(dispatch);
        }
        if close || dismissed {
            self.theme = ThemeChoice::default();
            self.open = false;
            self.draft = None;
            self.port.clear();
            self.keymap.reset();
            self.templates.reset();
            self.postfix.reset();
            self.file_types.reset();
            self.file_encodings.reset();
            self.project_structure.reset();
            self.spelling.reset();
            self.inspections.reset();
            self.file_templates.reset();
            self.external_tools.reset();
            self.file_watchers.reset();
            self.trusted_locations.reset();
            self.code_styles.reset();
        }
        applied.filter(|settings| settings != context.settings)
    }

    pub(super) fn shown_module_page(&self) -> Option<&'static str> {
        match self.section {
            PageId::Module(id) if self.open => Some(id),
            _ => None,
        }
    }

    fn mcp_page(&mut self, ui: &mut Ui, p: &IdePalette, context: &SettingsContext<'_>) {
        let query = self.search.query();
        let Some(draft) = self.draft.as_mut() else { return };
        ui.label(egui::RichText::new(t!("settings-mcp-about")).color(p.text_secondary));
        ui.add_space(10.0);
        widgets::checkbox(ui, p, &mut draft.mcp_enabled, &t!("settings-mcp-enable"), query);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let title = ui.label(widgets::label_job(ui, p, &t!("settings-mcp-port"), query));
            let field = ui.add(egui::TextEdit::singleline(&mut self.port).desired_width(70.0)).labelled_by(title.id);
            if field.changed()
                && let Ok(port) = self.port.trim().parse::<u16>()
                && port >= 1024
            {
                draft.mcp_port = port;
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let title = ui.label(widgets::label_job(ui, p, &t!("settings-mcp-language"), query));
            let name = |code: &str| {
                zen_i18n::LANGUAGES
                    .iter()
                    .find(|(known, _)| *known == code)
                    .map_or_else(|| code.to_owned(), |(_, name)| (*name).to_owned())
            };
            let combo = egui::ComboBox::from_id_salt("settings-mcp-language")
                .selected_text(name(&draft.mcp_language))
                .width(widgets::field_width(ui, 120.0, 220.0))
                .truncate()
                .show_ui(ui, |ui| {
                    for (code, language) in zen_i18n::LANGUAGES {
                        ui.selectable_value(&mut draft.mcp_language, (*code).to_owned(), *language);
                    }
                });
            let _ = combo.response.labelled_by(title.id);
        });
        ui.label(egui::RichText::new(t!("settings-mcp-language-note")).color(p.text_secondary));
        ui.add_space(10.0);
        let title = ui.label(widgets::label_job(ui, p, &t!("settings-mcp-redact"), query));
        let mut patterns = draft.mcp_redact_files.join("\n");
        let field = ui.add(
            egui::TextEdit::multiline(&mut patterns)
                .font(egui::TextStyle::Monospace)
                .desired_rows(4)
                .desired_width(widgets::field_width(ui, 240.0, 420.0)),
        );
        if field.labelled_by(title.id).changed() {
            draft.mcp_redact_files = patterns.split('\n').map(str::to_owned).collect();
        }
        ui.label(egui::RichText::new(t!("settings-mcp-redact-note")).color(p.text_secondary));
        ui.add_space(12.0);
        let status = match context.mcp_url {
            Some(url) => t!("settings-mcp-running", url = url),
            None => t!("settings-mcp-stopped"),
        };
        ui.label(egui::RichText::new(status).color(if context.mcp_url.is_some() { p.run } else { p.text_secondary }));
        ui.add_space(16.0);

        widgets::group_title(ui, p, &t!("settings-mcp-connect"), query);
        let url = context.mcp_url.map_or_else(|| format!("http://127.0.0.1:{}/mcp", draft.mcp_port), str::to_owned);
        let http = format!("claude mcp add --transport http {} {url}", context.product_id);
        let stdio = context.executable.as_ref().map(|exe| {
            format!("claude mcp add {} -- {} mcp", context.product_id, super::launcher_dialog::shell_word(exe))
        });
        let headless = context.executable.as_ref().map(|exe| {
            let program = super::launcher_dialog::shell_word(exe);
            format!("claude mcp add -s project {} -- {program} mcp --headless .", context.product_id)
        });
        let now = ui.input(|input| input.time);
        let mut command_row = |ui: &mut Ui, label: String, command: String| {
            ui.label(egui::RichText::new(label).color(p.text_secondary));
            egui::Frame::new().fill(p.frame).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(&command).monospace().color(p.text)).wrap());
            });
            if ui.button(t!("settings-copy")).clicked() {
                ui.ctx().copy_text(command.clone());
                self.copied = Some(now);
            }
            ui.add_space(8.0);
        };
        if let Some(headless) = headless {
            command_row(ui, t!("settings-mcp-headless"), headless);
        }
        command_row(ui, t!("settings-mcp-http"), http);
        if let Some(stdio) = stdio {
            command_row(ui, t!("settings-mcp-stdio"), stdio);
        }
        if self.copied.is_some_and(|copied| now - copied < 2.0) {
            ui.label(egui::RichText::new(t!("settings-copied")).color(p.run));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
    }
}

fn in_development(ui: &mut Ui, p: &IdePalette) {
    ui.label(egui::RichText::new(t!("ide-in-development")).color(p.text_secondary));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offscreen::Offscreen;
    use egui::Color32;
    use serde_json::json;

    #[derive(Clone, Copy)]
    enum Paint {
        Image,
        Tree,
        No,
    }

    struct Harness {
        dialog: SettingsDialog,
        settings: IdeSettings,
        registry: zen_core::Registry,
        session: zen_core::Session,
        palette: IdePalette,
        screen: Offscreen,
        executable: Option<PathBuf>,
        sections: Vec<SectionDef>,
        form: Option<zen_modules::form::Form>,
        dispatch_everywhere: bool,
        folding: Vec<zen_modules::folding::FoldingOptions>,
    }

    const CODE_STYLE: zen_modules::CodeStylePage =
        zen_modules::CodeStylePage { language: "go", title: "settings-section-code-style-other" };

    impl Harness {
        fn new(page: PageId) -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let mut dialog = SettingsDialog::default();
            dialog.open();
            dialog.section = page;
            let mut harness = Self {
                dialog,
                settings: IdeSettings::default(),
                registry: zen_core::Registry::new(),
                session: zen_core::Session::new(),
                palette: IdePalette::zen_dark(),
                screen: Offscreen::new(vec2(1000.0, 700.0), 1.0),
                executable: None,
                sections: page::sections(&[], &[CODE_STYLE]),
                form: None,
                dispatch_everywhere: false,
                folding: Vec::new(),
            };
            harness.use_plugins(crate::plugins::Plugins::new(None));
            harness
        }

        fn use_plugins(&mut self, plugins: std::sync::Arc<crate::plugins::Plugins>) {
            self.registry = zen_core::Registry::new();
            let plugins: zen_workbench::plugin_manager::SharedPlugins = plugins;
            zen_workbench::plugin_manager::register(&mut self.registry, &plugins).unwrap();
            let keymaps = zen_workbench::keymaps::KeymapStore::new(None, Vec::new());
            zen_workbench::keymaps::register(&mut self.registry, &std::sync::Arc::new(std::sync::Mutex::new(keymaps)))
                .unwrap();
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> crate::offscreen::Frame {
            self.draw(events, true).unwrap()
        }

        fn draw(&mut self, events: Vec<egui::Event>, paint: bool) -> Option<crate::offscreen::Frame> {
            self.paint(events, if paint { Paint::Image } else { Paint::No }).and_then(Result::ok)
        }

        fn paint(
            &mut self,
            events: Vec<egui::Event>,
            paint: Paint,
        ) -> Option<Result<crate::offscreen::Frame, egui::accesskit::TreeUpdate>> {
            let context = SettingsContext {
                settings: &self.settings,
                mcp_url: None,
                product_id: "gogarden",
                executable: self.executable.clone(),
                form: self.form.clone(),
                sections: &self.sections,
                language_servers: None,
                declared: &[],
                folding: &self.folding,
            };
            let (dialog, palette, registry, session) =
                (&mut self.dialog, &self.palette, &self.registry, &mut self.session);
            let everywhere = self.dispatch_everywhere;
            let ui = |ui: &mut Ui| {
                let mut dispatch =
                    |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
                let plugins = everywhere || dialog.section == PageId::Plugins;
                dialog.show(ui, palette, &context, plugins.then_some(&mut dispatch as &mut Dispatch<'_>));
            };
            match paint {
                Paint::Image => Some(Ok(self.screen.frame(events, ui))),
                Paint::Tree => Some(Err(self.screen.accessibility_tree(ui))),
                Paint::No => {
                    self.screen.step(events, ui);
                    None
                }
            }
        }

        fn tree(&mut self) -> egui::accesskit::TreeUpdate {
            match self.paint(Vec::new(), Paint::Tree) {
                Some(Err(tree)) => tree,
                _ => unreachable!(),
            }
        }

        fn step(&mut self, events: Vec<egui::Event>) {
            self.draw(events, false);
        }

        fn search(&mut self, text: &str) {
            self.dialog.search.text = text.to_owned();
            self.frame(Vec::new());
        }
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    struct DeclaresAnalyzers;

    impl zen_project::inspect::Inspector for DeclaresAnalyzers {
        fn id(&self) -> &str {
            "analyzers"
        }

        fn handles(&self, _: &str) -> bool {
            false
        }

        fn inspections(&self) -> Vec<zen_project::inspect::DeclaredInspection> {
            let declared = |id: &str, cleanup: bool| zen_project::inspect::DeclaredInspection {
                id: id.to_owned(),
                title: id.to_owned(),
                group: "gopls".to_owned(),
                description: format!("What {id} reports"),
                cleanup,
                off: false,
            };
            vec![declared("simplifyslice", true), declared("unusedparams", false)]
        }

        fn inspect(&self, _: &zen_project::inspect::InspectContext<'_>) -> Vec<zen_project::inspect::Finding> {
            Vec::new()
        }

        fn apply(
            &self,
            _: &zen_project::inspect::InspectContext<'_>,
            _: &[zen_project::inspect::InspectionFix],
        ) -> Result<Box<dyn zen_project::inspect::FixJob>, String> {
            Err("no fixes".to_owned())
        }
    }

    fn inspections_harness(project: &std::path::Path) -> Harness {
        let mut harness = Harness::new(PageId::Inspections);
        zen_workbench::register(&mut harness.registry).unwrap();
        harness
            .session
            .state_mut::<zen_workbench::inspections::Inspections>()
            .set_inspectors(vec![std::sync::Arc::new(DeclaresAnalyzers)], None);
        harness.registry.dispatch(&mut harness.session, "file.project.open", json!({ "path": project })).unwrap();
        harness.dispatch_everywhere = true;
        for _ in 0..12 {
            harness.step(Vec::new());
        }
        harness
    }

    fn with_page<T>(harness: &mut Harness, f: impl FnOnce(&mut SettingsDialog, &mut Dispatch<'_>) -> T) -> T {
        let (registry, session) = (&harness.registry, &mut harness.session);
        let mut dispatch = |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
        f(&mut harness.dialog, &mut dispatch)
    }

    fn enabled(harness: &mut Harness, id: &str) -> bool {
        let view = harness.registry.dispatch(&mut harness.session, "inspect.profile.get", json!({})).unwrap();
        view["inspections"].as_array().unwrap().iter().find(|row| row["id"] == id).unwrap()["enabled"] == true
    }

    #[test]
    fn the_inspections_page_turns_an_inspection_off_when_applied_and_filters_to_cleanup() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let project = tempfile::tempdir().unwrap();
                let mut harness = inspections_harness(project.path());
                let image = harness.frame(Vec::new());
                if let Some(dir) = &snapshots {
                    crate::offscreen::write_png(&dir.join(format!("inspections-page-{language}.png")), &image).unwrap();
                }
                click_on(&mut harness, egui::accesskit::Role::CheckBox, "unusedparams");
                assert!(enabled(&mut harness, "unusedparams"), "nothing changes until Apply");
                with_page(&mut harness, |dialog, dispatch| dialog.inspections.commit(dispatch));
                assert!(!enabled(&mut harness, "unusedparams"), "in {language}");
                let name = t!("settings-inspections-only-cleanup");
                click_on(&mut harness, egui::accesskit::Role::CheckBox, &name);
                harness.step(Vec::new());
                let tree = harness.tree();
                let shown =
                    |id: &str| accessibility::testing::find(&tree, egui::accesskit::Role::CheckBox, id).is_some();
                assert!(shown("simplifyslice") && !shown("unusedparams"), "in {language}");
            });
        }
    }

    #[test]
    fn the_gear_duplicates_and_the_chosen_profile_becomes_the_projects_when_applied() {
        let project = tempfile::tempdir().unwrap();
        let mut harness = inspections_harness(project.path());
        with_page(&mut harness, |dialog, dispatch| {
            dialog.inspections.gear(dispatch, inspections::Gear::Duplicate, Some("Team".to_owned()), None);
            dialog.inspections.commit(dispatch);
        });
        let list = harness.registry.dispatch(&mut harness.session, "inspect.profile.list", json!({})).unwrap();
        assert_eq!(list["active"], json!({ "name": "Team", "level": "project" }));
        with_page(&mut harness, |dialog, dispatch| {
            dialog.inspections.gear(dispatch, inspections::Gear::Delete, None, None);
            dialog.inspections.commit(dispatch);
        });
        let list = harness.registry.dispatch(&mut harness.session, "inspect.profile.list", json!({})).unwrap();
        assert_eq!(list["active"]["name"], "Project Default");
    }

    #[test]
    fn the_inspections_page_turns_the_spelling_options() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let mut harness = Harness::new(PageId::Inspections);
                for _ in 0..12 {
                    harness.step(Vec::new());
                }
                let image = harness.frame(Vec::new());
                if let Some(dir) = &snapshots {
                    crate::offscreen::write_png(&dir.join(format!("spelling-settings-{language}.png")), &image)
                        .unwrap();
                }
                let tree = harness.tree();
                for key in [
                    "settings-section-inspections",
                    "settings-inspections-proofreading",
                    "settings-inspections-spelling",
                    "settings-inspections-process-code",
                    "settings-inspections-process-comments",
                ] {
                    assert!(
                        tree.nodes.iter().any(|(_, node)| node.label().is_some_and(|label| label.contains(&t!(key)))),
                        "{key} in {language}"
                    );
                }
                let name = t!("settings-inspections-process-literals");
                let node = accessibility::testing::find(&tree, egui::accesskit::Role::CheckBox, &name)
                    .unwrap_or_else(|| panic!("{name}"));
                let bounds = node.bounds().unwrap();
                let at = pos2(bounds.x0 as f32 + 6.0, ((bounds.y0 + bounds.y1) / 2.0) as f32);
                let click = |pressed| egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                harness.frame(vec![egui::Event::PointerMoved(at)]);
                harness.frame(vec![click(true)]);
                harness.frame(vec![click(false)]);
                let draft = &harness.dialog.draft.as_ref().unwrap().spelling;
                assert!(!draft.process_literals, "the click turns Process literals off");
                assert!(draft.enabled && draft.process_code && draft.process_comments);
            });
        }
    }

    #[test]
    fn the_sql_code_style_page_sets_the_options_of_reformat_code() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let mut harness = Harness::new(PageId::CodeStyleOf(page::SQL));
                assert!(harness.sections.iter().any(|section| section.page == PageId::CodeStyleOf(page::SQL)));
                for (theme_name, theme) in
                    [("light", zen_theme::ResolvedTheme::zen_light()), ("dark", zen_theme::ResolvedTheme::zen_dark())]
                {
                    harness.palette = IdePalette::from_theme(&theme);
                    harness.palette.apply(harness.screen.context());
                    for _ in 0..6 {
                        harness.step(Vec::new());
                    }
                    let image = harness.frame(Vec::new());
                    if let Some(dir) = &snapshots {
                        let file = dir.join(format!("sql-code-style-{language}-{theme_name}.png"));
                        crate::offscreen::write_png(&file, &image).unwrap();
                    }
                }
                let own_line = t!("settings-code-style-sql-clause-own-line");
                click_on(&mut harness, egui::accesskit::Role::CheckBox, &own_line);
                let draft = harness.dialog.draft.as_ref().unwrap();
                assert!(draft.sql_format.clause_on_own_line);
                assert!(draft.sql_format.item_per_line, "on by default");
            });
        }
    }

    const GO_STYLE: &str = r#"{
        "language": "go",
        "title": { "en": "Go" },
        "preview": "package main\n\nfunc main() {}\n",
        "options": [
            { "id": "gofumpt", "label": { "en": "Use gofumpt", "ru": "Через gofumpt" }, "tab": "wrappingAndBraces",
              "kind": { "type": "boolean" }, "default": false,
              "serverSetting": { "section": "gopls", "path": "gofumpt" } }
        ],
        "notes": { "spaces": { "en": "No options.", "ru": "Нет настроек." } },
        "arrangement": [ { "id": "const", "label": { "en": "Constants", "ru": "Константы" } },
            { "id": "func", "label": { "en": "Functions", "ru": "Функции" } } ]
    }"#;

    #[test]
    fn a_declared_language_page_has_the_six_tabs_and_keeps_changed_options_and_the_order() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let mut harness = Harness::new(PageId::CodeStyleOf("go"));
                zen_workbench::code_style_options::register(&mut harness.registry).unwrap();
                let declaration = zen_project::code_style::CodeStyleDeclaration::parse(GO_STYLE).unwrap();
                harness
                    .session
                    .set_state(zen_workbench::code_style_options::CodeStyles::new(vec![declaration], Vec::new()));
                harness.dispatch_everywhere = true;
                for _ in 0..4 {
                    harness.step(Vec::new());
                }
                for tab in zen_project::code_style::StyleTab::ALL {
                    let tree = harness.tree();
                    let name = t!(code_style_page::tab_key(tab));
                    assert!(accessibility::testing::find(&tree, egui::accesskit::Role::Tab, &name).is_some(), "{name}");
                }
                click_on(&mut harness, egui::accesskit::Role::Tab, &t!("settings-code-style-tab-wrapping"));
                harness.step(Vec::new());
                if let Some(dir) = &snapshots {
                    let image = harness.frame(Vec::new());
                    crate::offscreen::write_png(&dir.join(format!("go-code-style-{language}.png")), &image).unwrap();
                }
                let gofumpt = if language == "ru" { "Через gofumpt" } else { "Use gofumpt" };
                click_on(&mut harness, egui::accesskit::Role::CheckBox, gofumpt);
                let draft = harness.dialog.draft.as_ref().unwrap();
                assert_eq!(draft.code_style_options["go"]["gofumpt"], serde_json::json!(true));
                click_on(&mut harness, egui::accesskit::Role::Tab, &t!("settings-code-style-tab-arrangement"));
                harness.step(Vec::new());
                click_on(&mut harness, egui::accesskit::Role::Button, &t!("settings-code-style-arrangement-down"));
                let draft = harness.dialog.draft.as_ref().unwrap();
                assert_eq!(draft.code_style_options["go"]["arrangement"], serde_json::json!(["func", "const"]));
                click_on(
                    &mut harness,
                    egui::accesskit::Role::CheckBox,
                    &t!("settings-code-style-rearrange-on-reformat"),
                );
                assert!(harness.dialog.draft.as_ref().unwrap().rearrange_on_reformat);
            });
        }
    }

    fn click_on(harness: &mut Harness, role: egui::accesskit::Role, name: &str) {
        let tree = harness.tree();
        let node = accessibility::testing::find(&tree, role, name).unwrap_or_else(|| panic!("{name}"));
        let bounds = node.bounds().unwrap();
        let at = pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let click = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        harness.frame(vec![egui::Event::PointerMoved(at)]);
        harness.frame(vec![click(true)]);
        harness.frame(vec![click(false)]);
    }

    #[test]
    fn the_spelling_page_keeps_the_accepted_words_and_the_custom_dictionaries() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                zen_i18n::register(zen_workbench::LOCALES);
                let data = tempfile::tempdir().unwrap();
                let application = data.path().join("dictionaries/application.dic");
                std::fs::create_dir_all(application.parent().unwrap()).unwrap();
                std::fs::write(&application, "gopls\n").unwrap();
                let team = data.path().join("team.dic");
                std::fs::write(&team, "mesage\nkubelet\n").unwrap();
                let mut harness = Harness::new(PageId::Spelling);
                harness.registry = zen_core::Registry::new();
                zen_workbench::register(&mut harness.registry).unwrap();
                harness
                    .session
                    .state_mut::<zen_workbench::spelling::Spelling>()
                    .set_application_dictionary(application.clone());
                harness.dispatch_everywhere = true;
                harness.settings.spelling.custom_dictionaries = vec![team.display().to_string()];
                harness.settings.apply_to_session(harness.session.settings_mut());
                for _ in 0..12 {
                    harness.step(Vec::new());
                }
                let image = harness.frame(Vec::new());
                if let Some(dir) = &snapshots {
                    crate::offscreen::write_png(&dir.join(format!("spelling-dictionaries-{language}.png")), &image)
                        .unwrap();
                }
                let tree = harness.tree();
                for key in [
                    "settings-section-natural-languages",
                    "settings-section-spelling",
                    "settings-spelling-single",
                    "settings-spelling-accepted",
                    "settings-spelling-custom",
                    "settings-spelling-configure",
                ] {
                    assert!(
                        tree.nodes.iter().any(|(_, node)| node
                            .label()
                            .or(node.value())
                            .is_some_and(|label| label.contains(t!(key).trim_end_matches(':')))),
                        "{key} in {language}"
                    );
                }
                let application_level = t!("settings-spelling-application-dictionary");
                let project_level = t!("settings-spelling-project-dictionary");
                for row in
                    [application_level.as_str(), project_level.as_str(), "gopls", team.display().to_string().as_str()]
                {
                    assert!(
                        accessibility::testing::find(&tree, egui::accesskit::Role::TreeItem, row).is_some(),
                        "the row {row}"
                    );
                }
                click_on(&mut harness, egui::accesskit::Role::Button, &t!("settings-spelling-add"));
                harness.frame(vec![egui::Event::Text("Kubectl".to_owned())]);
                harness.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
                harness.frame(Vec::new());
                assert_eq!(harness.dialog.spelling.words(), ["gopls", "kubectl"], "{language}");
                let checkbox = t!("settings-spelling-single");
                let single = |harness: &Harness| harness.dialog.draft.as_ref().unwrap().spelling.use_single_dictionary;
                assert!(!single(&harness), "Use single dictionary is off by default");
                click_on(&mut harness, egui::accesskit::Role::CheckBox, &checkbox);
                assert!(single(&harness));
                click_on(&mut harness, egui::accesskit::Role::CheckBox, &checkbox);
                assert!(!single(&harness));
                click_on(&mut harness, egui::accesskit::Role::DefaultButton, &t!("settings-ok"));
                assert_eq!(std::fs::read_to_string(&application).unwrap(), "gopls\nkubectl\n", "OK saves the word");
                assert!(!harness.dialog.is_open());
            });
        }
    }

    fn tinted_pixels(image: &egui::ColorImage, tint: Color32) -> usize {
        let lean =
            |pixel: Color32| (i32::from(pixel.g()) - i32::from(pixel.r()), i32::from(pixel.g()) - i32::from(pixel.b()));
        let (red, blue) = lean(tint);
        image
            .pixels
            .iter()
            .filter(|pixel| {
                let (r, b) = lean(**pixel);
                r.signum() == red.signum() && b.signum() == blue.signum() && r.abs() >= 8 && b.abs() >= 4
            })
            .count()
    }

    #[test]
    fn a_query_moves_to_the_first_matching_page_and_keeps_a_matching_one() {
        let mut harness = Harness::new(PageId::Appearance);
        harness.frame(Vec::new());
        harness.search(&t!("settings-section-font"));
        assert_eq!(harness.dialog.section, PageId::Appearance, "the current page matches, so it stays");
        harness.search("");
        harness.dialog.show_page(PageId::Mcp);
        harness.search(&t!("settings-section-font"));
        assert_eq!(harness.dialog.section, PageId::EditorFont, "a page titled by the query before the first one");
        harness.search(&t!("settings-on-save-reformat"));
        assert_eq!(
            harness.dialog.section,
            PageId::ActionsOnSave,
            "the page with an option of that name before the Keymap action of that name"
        );
        assert!(harness.dialog.search.found.contains(&PageId::Keymap), "Keymap stays in the tree");
        harness.search(&t!("keymap-editor-line-duplicate"));
        assert_eq!(harness.dialog.section, PageId::Keymap, "an action's name alone opens Keymap");
        harness.dialog.section = PageId::Tools;
        harness.frame(Vec::new());
        assert_eq!(harness.dialog.section, PageId::Tools, "a parent picked by hand stays while the query is the same");
        harness.search(&t!("settings-mcp-port"));
        assert_eq!(harness.dialog.section, PageId::Mcp);
        harness.search(&t!("settings-mcp"));
        assert_eq!(harness.dialog.section, PageId::Mcp, "the current page matches, so it stays");
        harness.search("qzxv wkjp");
        assert_eq!(harness.dialog.section, PageId::Mcp, "nothing found keeps the page");
        harness.search("");
        assert_eq!(harness.dialog.section, PageId::Mcp);
    }

    #[test]
    fn escape_clears_the_search_before_it_closes_the_dialog() {
        let mut harness = Harness::new(PageId::Mcp);
        harness.search("port");
        harness.frame(vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
        assert!(harness.dialog.open, "the first Escape only clears the search");
        assert!(harness.dialog.search.text.is_empty());
        harness.frame(Vec::new());
        harness.frame(vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
        assert!(!harness.dialog.open, "Escape with an empty search closes the dialog");
    }

    #[test]
    fn command_f_focuses_the_search_field() {
        let mut harness = Harness::new(PageId::Appearance);
        harness.frame(Vec::new());
        let field = egui::Id::unique(SEARCH_FIELD);
        assert!(!harness.screen.context().memory(|memory| memory.has_focus(field)));
        harness.frame(vec![key(egui::Key::F, egui::Modifiers::COMMAND)]);
        assert!(harness.screen.context().memory(|memory| memory.has_focus(field)));
        harness.dialog.open();
        harness.dialog.focus_search();
        assert!(harness.dialog.search.text.is_empty(), "opening again starts with an empty search");
        harness.frame(Vec::new());
        assert!(harness.screen.context().memory(|memory| memory.has_focus(field)));
    }

    #[test]
    fn the_page_marks_the_labels_that_match() {
        let mut harness = Harness::new(PageId::ActionsOnSave);
        let highlight = harness.palette.search_highlight;
        let plain = harness.frame(Vec::new());
        assert_eq!(tinted_pixels(&plain, highlight), 0);
        harness.search(&t!("settings-on-save-reformat"));
        assert_eq!(harness.dialog.section, PageId::ActionsOnSave);
        assert_eq!(harness.dialog.search.query(), Some(t!("settings-on-save-reformat").as_str()));
        let marked = harness.frame(Vec::new());
        assert!(tinted_pixels(&marked, highlight) > 0, "the matching check box label is marked");
    }

    #[test]
    fn page_keys_scroll_the_page() {
        let mut harness = Harness::new(PageId::Editor);
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        let offset =
            |harness: &Harness| harness.dialog.page_scroll.map(|(view, content)| view.top() - content.top()).unwrap();
        assert_eq!(offset(&harness), 0.0);
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        harness.step(vec![key(egui::Key::PageDown)]);
        harness.frame(Vec::new());
        let down = offset(&harness);
        assert!(down > 100.0, "Page Down scrolls the page: {down}");
        harness.step(vec![key(egui::Key::End)]);
        harness.frame(Vec::new());
        let (viewport, content) = harness.dialog.page_scroll.unwrap();
        assert!((content.bottom() - viewport.bottom()).abs() < 2.0, "End shows the page's end");
        harness.step(vec![key(egui::Key::Home)]);
        harness.frame(Vec::new());
        assert_eq!(offset(&harness), 0.0, "Home its top");
    }

    #[test]
    fn a_search_brings_the_first_match_into_view_once_and_leaves_the_scrolling_to_the_user() {
        let mut harness = Harness::new(PageId::Editor);
        let highlight = harness.palette.search_highlight;
        harness.search(t!("settings-editor-indent-guides").trim_end_matches(':'));
        harness.step(Vec::new());
        let marked = harness.frame(Vec::new());
        assert!(tinted_pixels(&marked, highlight) > 0, "the match is in view without scrolling by hand");
        let offset = |harness: &Harness| harness.dialog.page_scroll.map(|(view, content)| view.top() - content.top());
        let scrolled = offset(&harness).unwrap();
        assert!(scrolled > 0.0, "{scrolled}");
        let (viewport, _) = harness.dialog.page_scroll.unwrap();
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: egui::Modifiers::NONE,
        };
        harness.step(vec![egui::Event::PointerMoved(viewport.center()), wheel]);
        for _ in 0..12 {
            harness.step(Vec::new());
        }
        harness.frame(Vec::new());
        let moved = offset(&harness).unwrap();
        assert!(moved < scrolled, "the user's scroll stays: {moved} after {scrolled}");
    }

    #[test]
    fn every_built_page_marks_its_matching_labels() {
        for (page, key) in [
            (PageId::Appearance, "settings-ui-font-custom"),
            (PageId::CodeFolding, "settings-folding-imports"),
            (PageId::CodeFolding, "settings-folding-show-arrows"),
            (PageId::Editor, "settings-editor-closing-tags"),
            (PageId::Editor, "settings-editor-indent-guides"),
            (PageId::Editor, "settings-editor-hard-wrap-guides"),
            (PageId::Advanced, "settings-advanced-tab-rendering"),
            (PageId::EditorFont, "settings-font-line-height"),
            (PageId::CodeStyleOf("go"), "settings-code-style-use-tabs"),
            (PageId::CodeStyleOther, "settings-code-style-yaml"),
            (PageId::InlayHints, "settings-inlay-hints-variable-types"),
            (PageId::CodeVision, "settings-code-vision-authors"),
            (PageId::Inspections, "settings-inspections-process-literals"),
            (PageId::Spelling, "settings-spelling-accepted"),
            (PageId::SystemSettings, "settings-open-window"),
            (PageId::EditorTabs, "settings-tabs-sort"),
            (PageId::Advanced, "settings-advanced-pinned-left"),
            (PageId::Todo, "settings-todo-multiline"),
            (PageId::Todo, "settings-todo-no-filters"),
            (PageId::Mcp, "settings-mcp-port"),
            (PageId::Markdown, "settings-markdown-preview-layout"),
            (PageId::Markdown, "settings-markdown-sync-scroll"),
            (PageId::Markdown, "settings-markdown-preview-font-size"),
            (PageId::Advanced, "settings-advanced-markdown-hide-toolbar"),
        ] {
            let mut harness = Harness::new(page);
            let highlight = harness.palette.search_highlight;
            assert_eq!(tinted_pixels(&harness.frame(Vec::new()), highlight), 0, "{page:?}");
            harness.search(t!(key).trim_end_matches(':'));
            assert_eq!(harness.dialog.section, page, "{key}");
            let mut marked = harness.frame(Vec::new());
            for _ in 0..2 {
                harness.step(Vec::new());
                marked = harness.frame(Vec::new());
            }
            assert!(tinted_pixels(&marked, highlight) > 0, "{page:?} marks {key}");
        }
    }

    fn write_snapshot(name: &str, image: &egui::ColorImage) {
        if let Ok(folder) = std::env::var("ZEN_UI_SNAPSHOTS") {
            crate::offscreen::write_png(&std::path::Path::new(&folder).join(format!("{name}.png")), image).unwrap();
        }
    }

    fn filtered_tree(harness: &Harness) -> Vec<(PageId, bool)> {
        let query = harness.dialog.search.query().unwrap_or_default().to_owned();
        let matched = search::matches(&query, &harness.sections, |key| t!(key), &harness.dialog.search.found);
        search::visible_rows(&harness.sections, &matched)
            .into_iter()
            .map(|row| (harness.sections[row.index].page, row.matched))
            .collect()
    }

    fn with_plugins(page: PageId) -> (Harness, tempfile::TempDir) {
        let root = tempfile::tempdir().unwrap();
        let mut harness = Harness::new(page);
        harness.use_plugins(plugins::tests::installed(root.path(), &["markdown", "yaml", "json"], &["sql"]));
        harness.dispatch_everywhere = true;
        (harness, root)
    }

    #[test]
    fn an_action_name_keeps_the_keymap_and_opens_it_with_its_own_search() {
        for (language, shown, english) in
            [("en", "Duplicate Line", "duplicate line"), ("ru", "Дублировать строку", "duplicate line")]
        {
            zen_i18n::with_language(language, || {
                let (mut harness, _root) = with_plugins(PageId::Appearance);
                harness.frame(Vec::new());
                assert!(search::matches(shown, &harness.sections, |key| t!(key), &[]).is_empty(), "{shown}");
                harness.search(shown);
                assert_eq!(harness.dialog.section, PageId::Keymap, "{language}: {shown}");
                assert_eq!(harness.dialog.keymap.search(), shown);
                assert_eq!(filtered_tree(&harness), [(PageId::Keymap, true)], "{language}");
                let image = harness.frame(Vec::new());
                write_snapshot(&format!("settings-search-keymap-{language}"), &image);
                harness.search(english);
                assert_eq!(harness.dialog.section, PageId::Keymap, "{language}: the English name too");
                assert_eq!(harness.dialog.keymap.search(), english);
                harness.search("");
                assert_eq!(harness.dialog.keymap.search(), "", "the search it set goes with the query");
                assert_eq!(filtered_tree(&harness).len(), harness.sections.len());
            });
        }
    }

    #[test]
    fn a_word_of_an_action_keeps_the_keymap_shown_while_its_own_search_finds_nothing() {
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let mut harness = Harness::new(PageId::Appearance);
                harness.dialog.show_page(PageId::Keymap);
                harness.frame(Vec::new());
                let word = t!("keymap-editor-line-duplicate").split_whitespace().next().unwrap().to_owned();
                let query = format!("{word} qzxv");
                harness.search(&query);
                assert_eq!(harness.dialog.search.found, [PageId::Keymap], "{language}: {query}");
                assert_eq!(harness.dialog.section, PageId::Keymap, "{language}: the current page stays");
                assert_eq!(harness.dialog.keymap.search(), query);
                assert_eq!(filtered_tree(&harness), [(PageId::Keymap, true)], "{language}");
            });
        }
    }

    #[test]
    fn an_installed_plugin_keeps_plugins_with_installed_filtered() {
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let (mut harness, _root) = with_plugins(PageId::Plugins);
                harness.frame(Vec::new());
                assert_eq!(harness.dialog.plugins.tab, plugins::PluginsTab::Marketplace);
                harness.search("SQL");
                assert_eq!(harness.dialog.section, PageId::Plugins, "{language}");
                assert_eq!(harness.dialog.plugins.tab, plugins::PluginsTab::Installed);
                assert_eq!(harness.dialog.plugins.search, "SQL");
                let sql = PageId::CodeStyleOf(page::SQL);
                let expected = [
                    (PageId::Keymap, true),
                    (PageId::Editor, false),
                    (PageId::CodeStyle, false),
                    (sql, true),
                    (PageId::Plugins, true),
                ];
                assert_eq!(filtered_tree(&harness), expected, "{language}");
                for _ in 0..3 {
                    harness.frame(Vec::new());
                }
                let image = harness.frame(Vec::new());
                write_snapshot(&format!("settings-search-plugins-{language}"), &image);
                assert_eq!(harness.dialog.plugins.selected.as_deref(), Some("sql"), "the only one listed");
                let listed = harness
                    .registry
                    .dispatch(&mut harness.session, "plugins.installed.list", serde_json::json!({ "query": "SQL" }))
                    .unwrap();
                assert_eq!(listed["plugins"].as_array().map(Vec::len), Some(1), "{listed}");

                harness.search("markdown");
                let pages: Vec<PageId> =
                    filtered_tree(&harness).into_iter().filter(|(_, own)| *own).map(|(page, _)| page).collect();
                assert_eq!(
                    pages,
                    [PageId::Keymap, PageId::CodeStyleOther, PageId::Plugins, PageId::Markdown, PageId::Advanced],
                    "{language}: the preview's actions, the language, the plug-in and its settings"
                );
                assert_eq!(harness.dialog.section, PageId::Plugins, "the current page matches, so it stays");
                assert_eq!(harness.dialog.plugins.search, "markdown");
                harness.search("toml");
                assert!(!filtered_tree(&harness).contains(&(PageId::Plugins, true)), "{language}: not installed here");
                assert_eq!(harness.dialog.plugins.search, "", "the search it set goes with the query");
            });
        }
    }

    #[test]
    fn clearing_the_query_puts_back_what_the_pages_own_searches_held_unless_edited() {
        zen_i18n::with_language("en", || {
            let (mut harness, _root) = with_plugins(PageId::Appearance);
            harness.dialog.keymap.set_search("typed by hand");
            harness.dialog.plugins.search = "also by hand".to_owned();
            harness.frame(Vec::new());
            for query in ["dup", "duplicate", "duplicate l"] {
                harness.search(query);
                assert_eq!(harness.dialog.keymap.search(), query);
            }
            harness.search("");
            assert_eq!(harness.dialog.keymap.search(), "typed by hand", "what was there before comes back");
            harness.search("sql");
            assert_eq!(
                (harness.dialog.plugins.tab, harness.dialog.plugins.search.as_str()),
                (plugins::PluginsTab::Installed, "sql")
            );
            harness.dialog.section = PageId::Appearance;
            harness.frame(Vec::new());
            harness.search("");
            assert_eq!(
                (harness.dialog.plugins.tab, harness.dialog.plugins.search.as_str()),
                (plugins::PluginsTab::Marketplace, "also by hand"),
                "another page shown meanwhile changes nothing"
            );

            harness.search("duplicate");
            harness.dialog.keymap.set_search("mine");
            harness.search("duplicate line");
            assert_eq!(harness.dialog.keymap.search(), "duplicate line", "the query goes on to the page");
            harness.search("");
            assert_eq!(harness.dialog.keymap.search(), "mine", "the text typed meanwhile comes back");
            harness.search("duplicate");
            harness.dialog.keymap.set_search("edited");
            harness.search("");
            assert_eq!(harness.dialog.keymap.search(), "edited", "an edit stays");

            harness.search("sql");
            harness.dialog.plugins.tab = plugins::PluginsTab::BuiltIn;
            harness.search("");
            assert_eq!(
                (harness.dialog.plugins.tab, harness.dialog.plugins.search.as_str()),
                (plugins::PluginsTab::BuiltIn, "sql"),
                "another tab chosen stays"
            );

            harness.dialog.plugins.search = "before".to_owned();
            harness.dialog.plugins.tab = plugins::PluginsTab::Marketplace;
            harness.search("duplicate");
            harness.search("dupl sql");
            harness.search("sql");
            harness.dialog.open();
            assert!(harness.dialog.search.text.is_empty());
            assert_eq!(harness.dialog.keymap.search(), "edited", "reopened, Settings puts back the Keymap search");
            assert_eq!(
                (harness.dialog.plugins.tab, harness.dialog.plugins.search.as_str()),
                (plugins::PluginsTab::Marketplace, "before"),
                "and the Plugins one"
            );
        });
    }

    #[test]
    fn a_query_matching_neither_leaves_the_search_as_it_was() {
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let (mut harness, _root) = with_plugins(PageId::Appearance);
                harness.dialog.keymap.set_search("typed by hand");
                harness.dialog.plugins.search = "also by hand".to_owned();
                harness.frame(Vec::new());
                let query = t!("settings-mcp-port");
                harness.search(&query);
                assert!(harness.dialog.search.found.is_empty(), "{language}: {query}");
                assert_eq!(harness.dialog.section, PageId::Mcp);
                assert_eq!(filtered_tree(&harness), [(PageId::Tools, false), (PageId::Mcp, true)], "{language}");
                let highlight = harness.palette.search_highlight;
                let image = harness.frame(Vec::new());
                write_snapshot(&format!("settings-search-neither-{language}"), &image);
                assert!(tinted_pixels(&image, highlight) > 0, "the match is marked as before");
                assert_eq!(harness.dialog.keymap.search(), "typed by hand", "the pages' own searches stay");
                assert_eq!(harness.dialog.plugins.search, "also by hand");
                assert_eq!(harness.dialog.plugins.tab, plugins::PluginsTab::Marketplace);
                harness.search("qzxv wkjp");
                assert!(filtered_tree(&harness).is_empty());
                assert_eq!(harness.dialog.section, PageId::Mcp, "nothing found keeps the page");
            });
        }
    }

    #[test]
    fn the_search_answers_what_it_found_for_agents() {
        zen_i18n::with_language("en", || {
            let (mut harness, _root) = with_plugins(PageId::Mcp);
            let sections = harness.sections.clone();
            let (dialog, registry, session) = (&mut harness.dialog, &harness.registry, &mut harness.session);
            let mut dispatch =
                |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
            let found = dialog.search_for(" markdown ", &sections, &[], 0.0, Some(&mut dispatch));
            assert_eq!(
                found,
                serde_json::json!({
                    "query": "markdown",
                    "pages": ["keymap", "codeStyleOther", "plugins", "markdown", "advanced"],
                    "page": "markdown",
                    "keymapSearch": "markdown",
                    "plugins": { "tab": "installed", "search": "markdown" },
                })
            );
            let found = dialog.search_for("reformat code", &sections, &[], 0.5, Some(&mut dispatch));
            assert_eq!(found["pages"], serde_json::json!(["keymap", "actionsOnSave"]), "{found}");
            assert_eq!(found["page"], "actionsOnSave", "the page with the option, before the Keymap action");
            assert_eq!(found["keymapSearch"], "reformat code");
            assert_eq!(found["plugins"]["search"], "", "{found}");
            let found = dialog.search_for("", &sections, &[], 1.0, Some(&mut dispatch));
            assert_eq!(found["keymapSearch"], "");
            assert_eq!(found["pages"].as_array().map(Vec::len), Some(sections.len()));
        });
    }

    #[test]
    fn the_search_reads_the_plugins_again_after_a_second() {
        let root = tempfile::tempdir().unwrap();
        let (mut harness, _first) = with_plugins(PageId::Mcp);
        let sections = harness.sections.clone();
        let finds_plugins = |harness: &mut Harness, query: &str, now: f64| {
            let (dialog, registry, session) = (&mut harness.dialog, &harness.registry, &mut harness.session);
            let mut dispatch =
                |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
            let found = dialog.search_for(query, &sections, &[], now, Some(&mut dispatch));
            found["pages"].as_array().unwrap().contains(&serde_json::json!("plugins"))
        };
        assert!(!finds_plugins(&mut harness, "toml", 10.0));
        harness.use_plugins(plugins::tests::installed(root.path(), &["toml"], &[]));
        assert!(!finds_plugins(&mut harness, "toml ", 10.5), "read a moment ago");
        assert!(finds_plugins(&mut harness, "toml", 11.5), "read again");
    }

    #[test]
    #[ignore = "a measurement: cargo test --release -p zen-ui --lib settings_search_cost -- --ignored --nocapture"]
    #[allow(clippy::print_stdout, reason = "a measurement, read on the terminal")]
    fn settings_search_cost_per_keystroke() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("bundle");
        let count = crate::plugins::fake::repository_plugins(&bundle);
        let config = crate::plugins::PluginConfig {
            folder: root.path().join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: Some(bundle),
            optional: Vec::new(),
        };
        let actions = crate::keymap::current().bindings().len();
        let milliseconds = |started: std::time::Instant| started.elapsed().as_secs_f64() * 1000.0;
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let fresh = |page: PageId| {
                    let mut harness = Harness::new(page);
                    harness.use_plugins(crate::plugins::Plugins::new(Some(config.clone())));
                    harness.dispatch_everywhere = true;
                    for _ in 0..5 {
                        harness.step(Vec::new());
                    }
                    harness
                };
                let mut harness = fresh(PageId::Mcp);
                let sections = harness.sections.clone();
                let (dialog, registry, session) = (&mut harness.dialog, &harness.registry, &mut harness.session);
                let mut dispatch =
                    |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
                let mut search = |query: &str, now: f64| {
                    let started = std::time::Instant::now();
                    dialog.search_for(query, &sections, &[], now, Some(&mut dispatch));
                    milliseconds(started)
                };
                let first = search("d", 0.0);
                let next: Vec<f64> = (2..=14).map(|end| search(&"duplicate line"[..end], 0.5)).collect();
                let reread = search("sql", 5.0);
                let mean = next.iter().sum::<f64>() / next.len() as f64;
                let worst = next.iter().copied().fold(0.0, f64::max);
                println!(
                    "{language}: {actions} actions, {count} plug-ins. The search alone: the first keystroke {first:.2} ms \
                     (reads the action names and the plug-ins), the next ones {mean:.2} ms on average and {worst:.2} ms \
                     at most, one that reads the plug-ins again {reread:.2} ms"
                );
                for (from, query) in [
                    (PageId::Appearance, "duplicate line"),
                    (PageId::Mcp, "sql"),
                    (PageId::Mcp, "plugins"),
                    (PageId::Appearance, "language for"),
                ] {
                    let mut harness = fresh(from);
                    let idle = {
                        let started = std::time::Instant::now();
                        for _ in 0..20 {
                            harness.step(Vec::new());
                        }
                        milliseconds(started) / 20.0
                    };
                    let mut frames = Vec::new();
                    for (end, _) in query.char_indices().skip(1).chain([(query.len(), ' ')]) {
                        harness.dialog.search.text = query[..end].to_owned();
                        let started = std::time::Instant::now();
                        harness.step(Vec::new());
                        frames.push(format!("{:.1}{}", milliseconds(started), short(harness.dialog.section)));
                    }
                    println!("  typing {query:?} on {from:?} (idle frame {idle:.2} ms): {}", frames.join(" "));
                }
            });
        }
    }

    fn short(page: PageId) -> String {
        format!("[{}]", page.name())
    }

    fn section_rows(harness: &Harness) -> Vec<(usize, Rect, Rect)> {
        harness.dialog.section_rows.clone()
    }

    #[test]
    fn the_section_tree_scrolls_inside_the_dialog_at_its_smallest_size() {
        let mut harness = Harness::new(PageId::Appearance);
        harness.screen = Offscreen::new(vec2(640.0, 480.0), 1.0);
        for _ in 0..3 {
            harness.frame(Vec::new());
        }
        let image = harness.frame(Vec::new());
        if let Ok(path) = std::env::var("ZEN_SETTINGS_MIN_PNG") {
            crate::offscreen::write_png(std::path::Path::new(&path), &image).unwrap();
        }
        let list = harness.dialog.section_list.unwrap();
        assert!(list.bottom() <= 480.0 && list.height() > 100.0, "{list:?}");
        let rows = section_rows(&harness);
        assert_eq!(rows.len(), harness.sections.len(), "every section has a row");
        for (index, _, visible) in &rows {
            assert!(
                visible.is_negative() || visible.height() == 0.0 || list.contains_rect(*visible),
                "section {index} shows only inside the tree: {visible:?} in {list:?}"
            );
        }
        let last = harness.sections.len() - 1;
        assert_eq!(harness.sections[last].page, PageId::Advanced);
        let (_, whole, _) = rows[last];
        assert!(whole.top() >= list.bottom(), "Advanced Settings waits below, to scroll to: {whole:?}");

        harness.dialog.open_at(PageId::Advanced);
        for _ in 0..3 {
            harness.frame(Vec::new());
        }
        let (_, whole, visible) = section_rows(&harness)[last];
        assert!(list.contains_rect(whole) && visible == whole, "{whole:?} in {list:?}");
    }

    #[test]
    fn the_installed_plugins_draw_in_groups_at_the_dialogs_smallest_size() {
        let root = tempfile::tempdir().unwrap();
        let mut harness = Harness::new(PageId::Plugins);
        harness.use_plugins(plugins::tests::installed(
            root.path(),
            &["markdown", "yaml", "json", "toml", "dockerfile", "gitlab-ci"],
            &["sql"],
        ));
        harness.dialog.plugins.tab = plugins::PluginsTab::Installed;
        harness.screen = Offscreen::new(vec2(640.0, 480.0), 1.0);
        for _ in 0..3 {
            harness.frame(Vec::new());
        }
        let image = harness.frame(Vec::new());
        if let Ok(path) = std::env::var("ZEN_SETTINGS_PLUGINS_PNG") {
            crate::offscreen::write_png(std::path::Path::new(&path), &image).unwrap();
        }
        assert_eq!(image.size, [640, 480]);
    }

    #[test]
    fn the_marketplace_draws_its_list_and_the_details() {
        let root = tempfile::tempdir().unwrap();
        let mut harness = Harness::new(PageId::Plugins);
        let plugins = plugins::tests::installed(root.path(), &["markdown"], &["sql"]);
        harness.use_plugins(std::sync::Arc::clone(&plugins));
        let start = std::time::Instant::now();
        while zen_workbench::plugin_manager::PluginManager::catalog(&*plugins, "").catalog
            != zen_workbench::plugin_manager::CatalogStatus::Ready
        {
            assert!(start.elapsed() < std::time::Duration::from_secs(60));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        harness.dialog.plugins.selected = Some("markdown".to_owned());
        for _ in 0..3 {
            harness.frame(Vec::new());
        }
        let image = harness.frame(Vec::new());
        if let Ok(path) = std::env::var("ZEN_SETTINGS_MARKETPLACE_PNG") {
            crate::offscreen::write_png(std::path::Path::new(&path), &image).unwrap();
        }
        assert_eq!(harness.dialog.plugins.selected.as_deref(), Some("markdown"));
    }

    #[test]
    fn every_page_scrolls_above_the_buttons_at_the_smallest_size() {
        let mut harness = Harness::new(PageId::Appearance);
        harness.screen = Offscreen::new(vec2(640.0, 480.0), 1.0);
        harness.executable = Some(PathBuf::from("/Applications/GoGarden.app/Contents/MacOS/gogarden"));
        for &page in PageId::ALL {
            harness.dialog.section = page;
            for _ in 0..3 {
                harness.step(Vec::new());
            }
            let (viewport, content) = harness.dialog.page_scroll.unwrap();
            let buttons = harness.dialog.buttons_top.unwrap();
            assert!(
                viewport.height() > 100.0 && viewport.bottom() <= buttons,
                "{page:?}: {viewport:?} above {buttons}"
            );
            if page == PageId::Mcp {
                assert!(content.bottom() > viewport.bottom(), "the MCP Server page needs to scroll: {content:?}");
            }
            let wheel = egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -100_000.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            };
            harness.step(vec![egui::Event::PointerMoved(viewport.left_top() + vec2(20.0, 20.0)), wheel]);
            for _ in 0..60 {
                harness.step(Vec::new());
            }
            let (viewport, content) = harness.dialog.page_scroll.unwrap();
            assert!(
                content.bottom() <= viewport.bottom() + 0.5 && content.bottom() > viewport.top(),
                "{page:?} scrolls to its last widget: {content:?} in {viewport:?}"
            );
        }
    }

    #[test]
    fn menus_and_toolbars_edits_a_group_through_its_toolbar() {
        use egui::accesskit::Role;
        let mut harness = Harness::new(PageId::MenusAndToolbars);
        for _ in 0..3 {
            harness.frame(Vec::new());
        }
        let toolbar = format!("+ {}", t!("settings-menus-main-toolbar"));
        click_on(&mut harness, Role::Button, &toolbar);
        click_on(&mut harness, Role::Button, &t!("settings-menus-add"));
        harness.dialog.menus.adding = Some("no such action".to_owned());
        harness.frame(Vec::new());
        click_on(&mut harness, Role::Button, &t!("settings-menus-separator"));
        harness.frame(Vec::new());
        let draft = harness.dialog.draft.as_ref().unwrap();
        assert_eq!(draft.customization.items("mainToolbar").last().map(String::as_str), Some("-"));
        let image = harness.frame(Vec::new());
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("menus-and-toolbars-en.png"), &image).unwrap();
        }
        click_on(&mut harness, Role::Button, &t!("settings-menus-restore"));
        harness.frame(Vec::new());
        assert_eq!(harness.dialog.draft.as_ref().unwrap().customization, Default::default());
    }

    fn go_like_form() -> zen_modules::form::Form {
        use zen_modules::form::{Action, Choice, Item, MenuEntry, Tone};
        let root = "/opt/homebrew/Cellar/go/1.27.2/libexec";
        let text = |text: &str, tone| Item::Text { text: text.to_owned(), tone, spinner: false };
        let button = |label: &str| Item::Button {
            label: label.to_owned(),
            enabled: true,
            action: Action::command("demo.tool.install"),
        };
        zen_modules::form::Form {
            items: vec![
                Item::Group { title: "GOROOT".to_owned() },
                Item::Row {
                    items: vec![
                        Item::Choice {
                            label: "GOROOT".to_owned(),
                            options: vec![Choice { value: root.to_owned(), label: format!("go1.27.2   {root}") }],
                            selected: Some(root.to_owned()),
                            action: Action::with_value("demo.sdk.choose", "root"),
                        },
                        Item::Menu {
                            label: "Add SDK".to_owned(),
                            entries: vec![MenuEntry {
                                label: "Local…".to_owned(),
                                action: Action::with_value("demo.sdk.add", "root").picking_folder("Choose"),
                            }],
                        },
                    ],
                },
                Item::Group { title: "gopls".to_owned() },
                text("Not installed", Tone::Secondary),
                Item::Path {
                    label: "Custom path:".to_owned(),
                    value: None,
                    action: Action::with_value("demo.tool.choose", "path"),
                },
                text("Leave it empty to use the gopls found in GOBIN, GOPATH/bin or on the PATH.", Tone::Secondary),
                Item::Row { items: vec![button("Install gopls"), button("Look again")] },
                text(
                    "gopls is the Go language server: completion, navigation and the problems come from it. \
                     It is installed with go install into GOPATH/bin.",
                    Tone::Secondary,
                ),
                Item::Group { title: "Delve".to_owned() },
                text("1.27.2   /Users/somebody/projects/with/a/rather/long/path/gopath/bin/dlv", Tone::Normal),
            ],
            busy: false,
        }
    }

    #[test]
    fn every_page_stays_inside_the_dialog_horizontally_at_the_smallest_size() {
        let mut harness = Harness::new(PageId::Appearance);
        harness.screen = Offscreen::new(vec2(640.0, 480.0), 1.0);
        harness.executable = Some(PathBuf::from("/Applications/GoGarden.app/Contents/MacOS/gogarden"));
        harness.sections = page::sections(&[form::tests::PAGE], &[CODE_STYLE]);
        harness.form = Some(go_like_form());
        appearance::tests::following_the_system(&mut harness.dialog);
        harness.dispatch_everywhere = true;
        let pages = PageId::ALL.iter().copied().chain([PageId::Module("demo")]);
        let mut wide = Vec::new();
        for page in pages {
            harness.dialog.section = page;
            for _ in 0..10 {
                harness.step(Vec::new());
            }
            let image = harness.frame(Vec::new());
            if let Ok(folder) = std::env::var("ZEN_SETTINGS_MIN_DIR") {
                let name = format!("{}.png", page.name().replace(['/', ':'], "-"));
                crate::offscreen::write_png(&std::path::Path::new(&folder).join(name), &image).unwrap();
            }
            let area = harness.dialog.page_area.unwrap();
            let (viewport, content) = harness.dialog.page_scroll.unwrap();
            if content.right() > area.right() + 0.5 || viewport.right() > area.right() + 0.5 {
                wide.push((page, content.right(), viewport.right(), area.right()));
            }
        }
        assert!(wide.is_empty(), "pages wider than their area (page, content, viewport, area): {wide:?}");
    }

    #[test]
    fn pages_with_their_own_lists_keep_them_inside_the_page_area() {
        for size in [vec2(1000.0, 700.0), vec2(640.0, 480.0)] {
            let mut harness = Harness::new(PageId::Keymap);
            harness.screen = Offscreen::new(size, 1.0);
            for page in [PageId::Keymap, PageId::Plugins, PageId::CodeStyleOther, PageId::LanguageServers] {
                harness.dialog.section = page;
                for _ in 0..3 {
                    harness.step(Vec::new());
                }
                let (viewport, content) = harness.dialog.page_scroll.unwrap();
                assert!(
                    content.height() <= viewport.height() + 1.0,
                    "{page:?} at {size:?} scrolls its own list, not the page: {content:?} in {viewport:?}"
                );
            }
        }
    }

    fn file_types_harness() -> Harness {
        let mut harness = Harness::new(PageId::FileTypes);
        harness.registry = zen_core::Registry::new();
        zen_workbench::register(&mut harness.registry).unwrap();
        harness.session.set_state(zen_workbench::file_types::FileTypeStore::load(None));
        harness.dispatch_everywhere = true;
        let scale = if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { 2.0 } else { 1.0 };
        harness.screen = Offscreen::new(vec2(1000.0, 700.0), scale);
        harness
    }

    fn settled(harness: &mut Harness) -> crate::offscreen::Frame {
        let context = SettingsContext {
            settings: &harness.settings,
            mcp_url: None,
            product_id: "gogarden",
            executable: None,
            form: None,
            sections: &harness.sections,
            language_servers: None,
            declared: &[],
            folding: &harness.folding,
        };
        let (dialog, palette, registry, session) =
            (&mut harness.dialog, &harness.palette, &harness.registry, &mut harness.session);
        harness.screen.render(|ui| {
            let mut dispatch =
                |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
            dialog.show(ui, palette, &context, Some(&mut dispatch as &mut Dispatch<'_>));
        })
    }

    fn tree(harness: &mut Harness) -> egui::accesskit::TreeUpdate {
        let context = SettingsContext {
            settings: &harness.settings,
            mcp_url: None,
            product_id: "gogarden",
            executable: None,
            form: None,
            sections: &harness.sections,
            language_servers: None,
            declared: &[],
            folding: &harness.folding,
        };
        let (dialog, palette, registry, session) =
            (&mut harness.dialog, &harness.palette, &harness.registry, &mut harness.session);
        harness.screen.accessibility_tree(|ui| {
            let mut dispatch =
                |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
            dialog.show(ui, palette, &context, Some(&mut dispatch as &mut Dispatch<'_>));
        })
    }

    fn labels(tree: &egui::accesskit::TreeUpdate) -> Vec<String> {
        tree.nodes.iter().filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned)).collect()
    }

    fn click_label(harness: &mut Harness, label: &str) {
        let tree = tree(harness);
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .and_then(|(_, node)| node.bounds())
            .unwrap_or_else(|| panic!("no {label:?} in {:?}", labels(&tree)));
        let at = pos2((bounds.x0 + bounds.x1) as f32 / 2.0, (bounds.y0 + bounds.y1) as f32 / 2.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        harness.step(vec![egui::Event::PointerMoved(at)]);
        harness.step(vec![button(true)]);
        harness.step(vec![button(false)]);
    }

    fn snapshot(harness: &mut Harness, name: &str, language: &str) {
        match std::env::var_os("ZEN_UI_SNAPSHOTS") {
            Some(dir) => {
                let image = settled(harness);
                let file = std::path::Path::new(&dir).join(format!("file-types-{name}-{language}.png"));
                crate::offscreen::write_png(&file, &image).unwrap();
            }
            None => harness.step(Vec::new()),
        }
    }

    #[test]
    fn file_types_list_the_patterns_warn_of_a_move_and_apply_it_and_the_ignored_names() {
        let languages: &[&str] = if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { &["en", "ru"] } else { &["en"] };
        zen_i18n::register(zen_workbench::LOCALES);
        for language in languages {
            let _exclusive = file_types::testing::exclusive();
            zen_i18n::set_language(Some(language));
            let mut harness = file_types_harness();
            harness.step(Vec::new());
            assert!(harness.dialog.file_types.select_type("Go"));
            harness.step(Vec::new());
            let shown = labels(&tree(&mut harness));
            for wanted in [
                t!("settings-file-types-recognized"),
                t!("settings-file-types-ignored"),
                "Go".into(),
                "YAML".into(),
                "*.go".into(),
            ] {
                assert!(shown.contains(&wanted), "{wanted} in {shown:?}");
            }
            assert!(shown.contains(&t!("settings-file-types-add-pattern")), "+ − ✎ are named: {shown:?}");
            assert!(!shown.contains(&t!("settings-file-types-contents")), "Go has no content markers");
            snapshot(&mut harness, "recognized", language);
            assert!(harness.dialog.file_types.select_type("ZT XML"));
            harness.step(Vec::new());
            let shown = labels(&tree(&mut harness));
            assert!(shown.contains(&t!("settings-file-types-contents")), "{shown:?}");
            assert!(shown.contains(&"<?zt-xml (not <?zt-xml-not)".to_owned()), "read-only markers: {shown:?}");
            assert!(harness.dialog.file_types.select_type("Go"));
            harness.step(Vec::new());

            harness.dialog.file_types.start_adding(false, "*.zt-yml");
            snapshot(&mut harness, "move-warning", language);
            let warning = t!("settings-file-types-moves", pattern = "*.zt-yml", from = "YAML", to = "Go");
            assert!(labels(&tree(&mut harness)).contains(&warning), "the move is told");
            harness.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
            harness.step(Vec::new());
            click_label(&mut harness, &t!("settings-apply"));
            let registry = zen_vfs::file_types::current();
            assert_eq!(registry.type_of("page.zt-yml").map(|ty| ty.id).as_deref(), Some("zt-go"), "applied");
            assert!(!registry.get("zt-yaml").unwrap().patterns.contains(&"*.zt-yml".to_owned()), "moved, not copied");
            assert!(registry.changes().removed.is_empty(), "a move is kept as one mapping: {:?}", registry.changes());
            snapshot(&mut harness, "applied", language);

            click_label(&mut harness, &t!("settings-file-types-ignored"));
            harness.step(Vec::new());
            let shown = labels(&tree(&mut harness));
            for wanted in zen_vfs::file_types::DEFAULT_IGNORED {
                assert!(shown.contains(&(*wanted).to_owned()), "{wanted} in {shown:?}");
            }
            assert!(shown.contains(&t!("settings-file-types-ignored-note")), "{shown:?}");
            snapshot(&mut harness, "ignored", language);
            harness.dialog.file_types.start_adding(true, "*.zt-log");
            harness.step(Vec::new());
            harness.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
            click_label(&mut harness, &t!("settings-ok"));
            assert!(zen_vfs::is_ignored("build.zt-log"), "OK applies the ignored names");
            assert!(zen_vfs::is_ignored(".git"), "the others stay");
            harness.dialog.file_types.tab = file_types::FileTypesTab::Recognized;
        }
        zen_i18n::set_language(Some("en"));
    }

    fn locations_harness(env: &zen_workbench::build_tools::BuildEnv) -> Harness {
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut harness = Harness::new(PageId::TrustedLocations);
        harness.registry = zen_core::Registry::new();
        zen_workbench::register(&mut harness.registry).unwrap();
        harness.session.set_state(env.clone());
        harness.dispatch_everywhere = true;
        harness.step(Vec::new());
        harness
    }

    #[test]
    fn trusted_locations_lists_folders_and_says_when_there_are_none() {
        let dir = tempfile::tempdir().unwrap();
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_data_dir(Some(dir.path().join("data")));
        let mut harness = locations_harness(&env);
        let shown = labels(&tree(&mut harness));
        assert!(shown.contains(&t!("settings-trusted-locations-about")), "{shown:?}");
        assert!(shown.contains(&"Nothing to show".to_owned()), "{shown:?}");
        let names = [
            t!("settings-trusted-locations-add"),
            t!("settings-trusted-locations-remove"),
            t!("settings-trusted-locations-up"),
            t!("settings-trusted-locations-down"),
        ];
        assert!(names.iter().all(|name| shown.contains(name)), "{shown:?}");
    }

    #[test]
    fn trusted_locations_change_only_on_apply_and_removing_one_stops_trusting() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().canonicalize().unwrap().join("work");
        let project = folder.join("shop");
        std::fs::create_dir_all(&project).unwrap();
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_data_dir(Some(dir.path().join("data")));
        let mut harness = locations_harness(&env);
        harness.dialog.trusted_locations.add(folder.clone());
        harness.step(Vec::new());
        assert!(!zen_workbench::trust::is_trusted(&env, &project), "nothing before Apply");
        click_label(&mut harness, &t!("settings-apply"));
        assert!(zen_workbench::trust::is_trusted(&env, &project));
        assert_eq!(zen_workbench::trust::locations(&env), vec![folder.clone()]);

        click_label(&mut harness, &folder.display().to_string());
        click_label(&mut harness, &t!("settings-trusted-locations-remove"));
        click_label(&mut harness, &t!("settings-apply"));
        assert!(!zen_workbench::trust::is_trusted(&env, &project), "its projects ask again");
    }

    #[test]
    fn adding_the_home_folder_warns_that_the_check_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_data_dir(Some(dir.path().join("data")));
        let mut harness = locations_harness(&env);
        let warning = t!("settings-trusted-locations-home");
        assert!(!labels(&tree(&mut harness)).contains(&warning));
        let home = std::fs::canonicalize(std::env::var_os("HOME").unwrap()).unwrap();
        harness.dialog.trusted_locations.add(home.parent().unwrap().to_owned());
        harness.step(Vec::new());
        assert!(labels(&tree(&mut harness)).contains(&warning));
    }

    #[test]
    fn the_settings_search_finds_trusted_locations() {
        let section = SECTIONS.iter().find(|section| section.page == PageId::TrustedLocations).unwrap();
        assert_eq!(section.key, "settings-trusted-locations");
        assert_eq!(PageId::from_name("trustedLocations", SECTIONS), Some(PageId::TrustedLocations));
    }

    #[test]
    fn project_structure_shows_the_folders_and_writes_the_marks_and_patterns_on_apply() {
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("shop");
        for folder in ["gen", "static", "logs", "src"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        let mut harness = Harness::new(PageId::ProjectStructure);
        harness.registry = zen_core::Registry::new();
        zen_workbench::register(&mut harness.registry).unwrap();
        harness
            .session
            .state_mut::<zen_workbench::project_settings::ProjectSettingsState>()
            .set_providers(vec![std::sync::Arc::new(zen_jb_import::IdeaProject::with_user_home(None))]);
        harness
            .registry
            .dispatch(&mut harness.session, "file.project.open", serde_json::json!({ "path": root }))
            .unwrap();
        harness.dispatch_everywhere = true;
        harness.step(Vec::new());
        let page = &mut harness.dialog.project_structure;
        page.choose(project_structure::Part::Modules);
        harness.step(Vec::new());
        let page = &mut harness.dialog.project_structure;
        assert_eq!(
            page.shown_folders(),
            [root.clone(), root.join("gen"), root.join("logs"), root.join("src"), root.join("static")],
            "the content root, open, with its folders"
        );
        let shown = labels(&tree(&mut harness));
        for wanted in [
            t!("settings-project-structure-excluded"),
            t!("settings-project-structure-resource-root"),
            t!("settings-project-structure-add-content-root"),
            t!("settings-project-structure-exclude-files"),
            "gen".into(),
            "static".into(),
        ] {
            assert!(shown.contains(&wanted), "{wanted} in {shown:?}");
        }
        let page = &mut harness.dialog.project_structure;
        page.select(&root.join("gen"));
        page.mark_selected(false);
        page.select(&root.join("static"));
        page.mark_selected(true);
        page.set_patterns("*.log; tmp?");
        harness.step(Vec::new());
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = settled(&mut harness);
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("project-structure.png"), &image).unwrap();
        }
        assert!(!root.join(".idea").exists(), "nothing is written before Apply");
        click_label(&mut harness, &t!("settings-apply"));
        let module = std::fs::read_to_string(root.join(".idea/shop.iml")).unwrap();
        for line in [
            "<sourceFolder url=\"file://$MODULE_DIR$/static\" type=\"java-resource\" />",
            "<excludeFolder url=\"file://$MODULE_DIR$/gen\" />",
            "<excludePattern pattern=\"*.log\" />",
            "<excludePattern pattern=\"tmp?\" />",
        ] {
            assert!(module.contains(line), "{line} in {module}");
        }
        let structure =
            harness.registry.dispatch(&mut harness.session, "project.structure.get", serde_json::json!({})).unwrap();
        assert_eq!(structure["excluded"], serde_json::json!([root.join("gen")]));
        assert_eq!(structure["resourceRoots"], serde_json::json!([root.join("static")]));
        assert_eq!(structure["excludePatterns"], serde_json::json!(["*.log", "tmp?"]));

        assert!(
            labels(&tree(&mut harness)).contains(&t!("settings-project-structure-excluded-folders")),
            "the marked folders are listed beside the tree"
        );
        click_label(&mut harness, &t!("settings-project-structure-unmark", name = "gen"));
        harness.dialog.project_structure.set_patterns("");
        harness.step(Vec::new());
        click_label(&mut harness, &t!("settings-ok"));
        let structure =
            harness.registry.dispatch(&mut harness.session, "project.structure.get", serde_json::json!({})).unwrap();
        assert_eq!(structure["excluded"], serde_json::json!([]), "the × of a listed folder takes its mark off");
        assert_eq!(structure["excludePatterns"], serde_json::json!([]));
        assert_eq!(structure["resourceRoots"], serde_json::json!([root.join("static")]));
    }

    #[test]
    fn project_structure_shows_the_build_model_and_sets_the_project_jdk_on_apply() {
        use std::sync::Arc;
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("shop");
        std::fs::create_dir_all(root.join("src/main/java")).unwrap();
        std::fs::write(
            root.join("pom.xml"),
            "<project><groupId>demo</groupId><artifactId>shop</artifactId><version>1</version>\
             <properties><maven.compiler.release>17</maven.compiler.release></properties>\
             <dependencies><dependency><groupId>org.junit</groupId><artifactId>junit</artifactId>\
             <version>5.0</version><scope>test</scope></dependency></dependencies></project>",
        )
        .unwrap();
        let jvm = dir.path().join("jvm");
        for (folder, version) in [("a-17", "17.0.12"), ("b-21", "21.0.5")] {
            let home = jvm.join(folder);
            std::fs::create_dir_all(home.join("bin")).unwrap();
            std::fs::write(home.join("bin/java"), "").unwrap();
            std::fs::write(home.join("release"), format!("JAVA_VERSION=\"{version}\"\n")).unwrap();
        }
        let mut store = zen_workbench::jdk::Store::with_sources(zen_workbench::jdk::Sources {
            system_folders: vec![jvm.clone()],
            ..zen_workbench::jdk::Sources::default()
        });
        store.set_data_dir(dir.path().join("data"));
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_home(Some(dir.path().join("home")));
        env.set_jdk(Some(Arc::new(|_: &std::path::Path| None)));
        let places = zen_workbench::jvm_libraries::Places {
            maven: dir.path().join("home/.m2/repository"),
            gradle: dir.path().join("home/.gradle/files"),
            cache: dir.path().join("cache"),
        };
        let provider = zen_workbench::jvm_libraries::JvmLibraries::with_places(
            env,
            places,
            vec![Box::new(zen_workbench::jvm_libraries::Declared)],
        );
        let mut harness = Harness::new(PageId::ProjectStructure);
        harness.registry = zen_core::Registry::new();
        zen_workbench::register(&mut harness.registry).unwrap();
        harness.session.set_state(zen_workbench::jdk::Jdks::new(store));
        harness
            .session
            .state_mut::<zen_workbench::project_model::ProjectModels>()
            .set_providers(vec![Arc::new(provider)], zen_workbench::model::ModelEnv::default());
        harness
            .registry
            .dispatch(&mut harness.session, "file.project.open", serde_json::json!({ "path": root }))
            .unwrap();
        harness.dispatch_everywhere = true;
        harness.step(Vec::new());
        let shown = labels(&tree(&mut harness));
        for wanted in [
            t!("settings-project-structure-project"),
            t!("settings-project-structure-modules"),
            t!("settings-project-structure-libraries"),
            t!("settings-project-structure-sdks"),
            t!("settings-project-structure-problems-count", count = 1),
            t!("settings-project-structure-sdk"),
            t!("settings-project-structure-language-level"),
            t!("settings-project-structure-build-level", level = "17"),
            t!("settings-project-structure-from-build", file = "pom.xml"),
            "target/classes".into(),
        ] {
            assert!(shown.contains(&wanted), "{wanted} in {shown:?}");
        }
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = settled(&mut harness);
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("project-structure-project.png"), &image)
                .unwrap();
        }

        harness.dialog.project_structure.choose_tab(project_structure::ModuleTab::Dependencies);
        harness.step(Vec::new());
        let shown = labels(&tree(&mut harness));
        for wanted in ["org.junit:junit", "5.0", "test"] {
            assert!(shown.contains(&wanted.to_owned()), "{wanted} in {shown:?}");
        }
        harness.dialog.project_structure.choose_tab(project_structure::ModuleTab::Sources);
        harness.step(Vec::new());
        assert!(
            labels(&tree(&mut harness)).contains(&t!("settings-project-structure-source-folders")),
            "the build's source roots are listed beside the tree"
        );

        let (modules, libraries, problems) = harness.dialog.project_structure.sections();
        assert_eq!(modules.len(), 1);
        assert_eq!(libraries[0].used_by, ["shop"]);
        assert_eq!(problems.len(), 1, "the library is not downloaded: {problems:?}");

        harness.dialog.project_structure.set_jdk(Some(&jvm.join("a-17")), "11");
        harness.step(Vec::new());
        click_label(&mut harness, &t!("settings-apply"));
        let project =
            harness.registry.dispatch(&mut harness.session, "toolchain.jdk.project", serde_json::json!({})).unwrap();
        assert_eq!((project["jdk"]["major"].as_u64(), project["languageLevel"].as_u64()), (Some(17), Some(11)));
        assert_eq!(project["chosen"], true);

        harness.dialog.project_structure.choose(project_structure::Part::Project);
        harness.step(Vec::new());
        click_label(&mut harness, &t!("settings-project-structure-open-build-file"));
        assert_eq!(harness.dialog.open_request.take(), Some(root.join("pom.xml")));
        assert!(!harness.dialog.is_open(), "the build file opens in the editor, so the dialog closes");
    }

    #[test]
    fn project_structure_adds_the_offered_artifacts_and_keeps_them_on_apply() {
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("shop");
        std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
        std::fs::write(
            root.join("pom.xml"),
            "<project><groupId>demo</groupId><artifactId>shop</artifactId><version>1</version></project>",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main/java/shop/App.java"),
            "package shop;\npublic class App { public static void main(String[] a) {} }\n",
        )
        .unwrap();
        let mut harness = Harness::new(PageId::ProjectStructure);
        harness.registry = zen_core::Registry::new();
        zen_workbench::register(&mut harness.registry).unwrap();
        harness
            .registry
            .dispatch(&mut harness.session, "file.project.open", serde_json::json!({ "path": root }))
            .unwrap();
        harness.dispatch_everywhere = true;
        harness.step(Vec::new());
        harness.dialog.project_structure.choose(project_structure::Part::Artifacts);
        harness.step(Vec::new());
        let shown = labels(&tree(&mut harness));
        for wanted in [t!("settings-project-structure-artifacts"), t!("settings-project-structure-artifacts-offered")] {
            assert!(shown.contains(&wanted), "{wanted} in {shown:?}");
        }

        harness.dialog.project_structure.add_artifact(1);
        harness.dialog.project_structure.edit_artifact(|artifact| {
            artifact.libraries = zen_workbench::artifacts::LibraryPlacement::NextTo;
        });
        harness.step(Vec::new());
        let shown = labels(&tree(&mut harness));
        for wanted in [
            t!("settings-project-structure-artifact-main-class"),
            t!("settings-project-structure-artifact-copy"),
            t!("settings-project-structure-artifact-file", path = "out/artifacts/shop/shop.jar"),
        ] {
            assert!(shown.contains(&wanted), "{wanted} in {shown:?}");
        }
        click_label(&mut harness, &t!("settings-apply"));
        let kept = zen_workbench::artifacts::saved(&root);
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].name.as_str(), kept[0].main_class.as_str()), ("shop:jar", "shop.App"));
        assert_eq!(kept[0].libraries, zen_workbench::artifacts::LibraryPlacement::NextTo);

        harness.dialog.project_structure.choose(project_structure::Part::Artifacts);
        harness.dialog.project_structure.add_artifact(1);
        assert_eq!(harness.dialog.project_structure.artifact_names(), ["shop:jar", "shop:jar 2"], "names stay unique");
        harness.step(Vec::new());
        click_label(&mut harness, &t!("settings-project-structure-artifact-remove"));
        click_label(&mut harness, &t!("settings-project-structure-artifact-remove"));
        click_label(&mut harness, &t!("settings-apply"));
        assert!(zen_workbench::artifacts::saved(&root).is_empty(), "removed on apply");
    }

    #[test]
    fn actions_on_save_are_off_by_default_and_read_from_older_files() {
        let settings: IdeSettings = serde_json::from_str(r#"{"mcpEnabled": true, "foldImports": false}"#).unwrap();
        assert!(!settings.reformat_on_save && !settings.optimize_imports_on_save);
        assert!(settings.mcp_enabled && !settings.fold_imports);
        let json = serde_json::to_value(IdeSettings { reformat_on_save: true, ..IdeSettings::default() }).unwrap();
        assert_eq!(json["reformatOnSave"], true);
        assert_eq!(json["optimizeImportsOnSave"], false);
    }

    fn editor_page(
        screen: &mut Offscreen,
        dialog: &mut SettingsDialog,
        events: Vec<egui::Event>,
    ) -> crate::offscreen::Frame {
        let palette = IdePalette::zen_dark();
        screen.frame(events, |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.popup);
            egui::Frame::new().inner_margin(16).show(ui, |ui| dialog.editor_page(ui, &palette, None));
        })
    }

    fn click_on_editor_page(
        screen: &mut Offscreen,
        dialog: &mut SettingsDialog,
        role: egui::accesskit::Role,
        name: &str,
    ) {
        let palette = IdePalette::zen_dark();
        let tree = screen.accessibility_tree(|ui| {
            egui::Frame::new().inner_margin(16).show(ui, |ui| dialog.editor_page(ui, &palette, None));
        });
        let node =
            accessibility::testing::find(&tree, role, name).unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let bounds = node.bounds().expect("the node has bounds");
        let at = pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            editor_page(screen, dialog, events);
        }
    }

    #[test]
    fn the_http_proxy_page_has_its_options_and_asks_what_to_check() {
        use egui::accesskit::Role;
        let mut harness = Harness::new(PageId::HttpProxy);
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        let tree = harness.tree();
        let shown = labels(&tree).join("\n");
        for key in [
            "settings-proxy-none",
            "settings-proxy-auto",
            "settings-proxy-system-link",
            "settings-proxy-pac",
            "settings-proxy-clear-passwords",
            "settings-proxy-manual",
            "settings-proxy-http",
            "settings-proxy-socks",
            "settings-proxy-host",
            "settings-proxy-port",
            "settings-proxy-no-proxy-for",
            "settings-proxy-no-proxy-example",
            "settings-proxy-authentication",
            "settings-proxy-login",
            "settings-proxy-password",
            "settings-proxy-remember",
            "settings-proxy-check",
        ] {
            assert!(shown.contains(&t!(key)), "{key}: {shown}");
        }
        assert_eq!(harness.dialog.draft.as_ref().unwrap().proxy.mode, zen_net::ProxyMode::AutoDetect, "the default");
        click_on(&mut harness, Role::RadioButton, &t!("settings-proxy-manual"));
        assert_eq!(harness.dialog.draft.as_ref().unwrap().proxy.mode, zen_net::ProxyMode::Manual);
        click_on(&mut harness, Role::Button, &t!("settings-proxy-check"));
        assert_eq!(harness.dialog.proxy.asking.as_deref(), Some("https://"));
        harness.frame(Vec::new());
        assert!(labels(&harness.tree()).join("\n").contains(&t!("settings-proxy-check-ask")));
    }

    #[test]
    fn a_proxy_script_is_named_as_not_supported_on_the_page() {
        let mut harness = Harness::new(PageId::HttpProxy);
        harness.frame(Vec::new());
        let proxy = &mut harness.dialog.draft.as_mut().unwrap().proxy;
        proxy.automatic_configuration = true;
        proxy.automatic_configuration_url = "https://example.com/wpad.pac".to_owned();
        harness.frame(Vec::new());
        assert!(labels(&harness.tree()).join("\n").contains(&t!("settings-proxy-pac-unsupported")));
    }

    #[test]
    fn the_server_certificates_page_shows_the_list_warns_and_gives_details() {
        use egui::accesskit::Role;
        let mut harness = Harness::new(PageId::ServerCertificates);
        harness.frame(Vec::new());
        harness.dialog.certificates.set_list(Vec::new());
        harness.frame(Vec::new());
        let shown = labels(&harness.tree()).join("\n");
        for key in ["settings-certificates-auto", "settings-certificates-accepted", "settings-certificates-empty"] {
            assert!(shown.contains(&t!(key)), "{key}: {shown}");
        }
        assert!(!harness.dialog.draft.as_ref().unwrap().accept_untrusted_certificates, "off by default");
        assert!(!shown.contains(&t!("settings-certificates-auto-warning")));
        click_on(&mut harness, Role::CheckBox, &t!("settings-certificates-auto"));
        assert!(harness.dialog.draft.as_ref().unwrap().accept_untrusted_certificates);
        harness.frame(Vec::new());
        assert!(labels(&harness.tree()).join("\n").contains(&t!("settings-certificates-auto-warning")));

        let root = zen_net::trust::parse_certificates(include_bytes!("../../../net/tests/fixtures/ca.pem"));
        harness.dialog.certificates.set_list(root);
        harness.dialog.certificates.select(0);
        harness.frame(Vec::new());
        let shown = labels(&harness.tree()).join("\n");
        assert!(shown.contains("Zen Test Root") && !shown.contains(&t!("settings-certificates-empty")), "{shown}");
        assert!(shown.contains(&t!("settings-certificates-fingerprint")), "{shown}");
        click_on(&mut harness, Role::Button, &t!("settings-certificates-remove"));
        assert!(harness.dialog.certificates.list().is_empty());
    }

    #[test]
    fn the_editor_page_has_the_on_save_options() {
        use egui::accesskit::Role;
        use zen_workbench::on_save::TrailingSpaces;
        zen_i18n::register(crate::toolbox::LOCALES);
        let defaults = IdeSettings::default();
        assert!(defaults.remove_trailing_spaces && defaults.keep_trailing_spaces_on_caret_line);
        assert_eq!(defaults.remove_trailing_spaces_on, TrailingSpaces::Modified);
        assert!(!defaults.remove_trailing_blank_lines && !defaults.ensure_line_break_at_end);
        let mut dialog = SettingsDialog { draft: Some(defaults), ..SettingsDialog::default() };
        let mut screen = Offscreen::new(vec2(900.0, 1800.0), 1.0);
        IdePalette::zen_dark().apply(screen.context());
        editor_page(&mut screen, &mut dialog, Vec::new());
        let mut click = |dialog: &mut SettingsDialog, role, key: &str| {
            click_on_editor_page(&mut screen, dialog, role, &t!(key));
        };
        click(&mut dialog, Role::CheckBox, "settings-editor-on-save-blank-lines");
        click(&mut dialog, Role::CheckBox, "settings-editor-on-save-line-break");
        click(&mut dialog, Role::ComboBox, "settings-editor-on-save-trailing-spaces");
        click(&mut dialog, Role::Button, "settings-editor-on-save-all-lines");
        click(&mut dialog, Role::CheckBox, "settings-editor-on-save-keep-caret-line");
        let draft = dialog.draft.clone().unwrap();
        assert!(draft.remove_trailing_blank_lines && draft.ensure_line_break_at_end);
        assert_eq!(draft.remove_trailing_spaces_on, TrailingSpaces::All);
        assert!(!draft.keep_trailing_spaces_on_caret_line);
        click(&mut dialog, Role::CheckBox, "settings-editor-on-save-trailing-spaces");
        assert!(!dialog.draft.as_ref().unwrap().remove_trailing_spaces);
        click(&mut dialog, Role::CheckBox, "settings-editor-on-save-keep-caret-line");
        assert!(!dialog.draft.as_ref().unwrap().keep_trailing_spaces_on_caret_line, "greyed out with its parent off");
        click(&mut dialog, Role::CheckBox, "settings-editor-on-save-trailing-spaces");
        let mut session = zen_core::Settings::default();
        dialog.draft.clone().unwrap().apply_to_session(&mut session);
        assert_eq!(session.get::<String>(zen_workbench::on_save::REMOVE_TRAILING_SPACES_ON).as_deref(), Some("all"));
        assert_eq!(session.get::<bool>(zen_workbench::on_save::REMOVE_TRAILING_SPACES), Some(true));
        assert_eq!(session.get::<bool>(zen_workbench::on_save::ENSURE_LINE_BREAK), Some(true));
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            let mut dialog = SettingsDialog { draft: Some(IdeSettings::default()), ..SettingsDialog::default() };
            let mut screen = Offscreen::new(vec2(760.0, 1000.0), 2.0);
            IdePalette::zen_dark().apply(screen.context());
            editor_page(&mut screen, &mut dialog, Vec::new());
            let image = editor_page(&mut screen, &mut dialog, Vec::new());
            let path = std::path::Path::new(&dir).join(format!("settings-editor-on-save-{language}.png"));
            crate::offscreen::write_png(&path, &image).unwrap();
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn settings_reach_the_session() {
        let mut session = zen_core::Settings::default();
        IdeSettings::default().apply_to_session(&mut session);
        assert_eq!(session.get::<bool>("editor.onSave.reformat"), Some(false));
        assert_eq!(session.get::<bool>("editor.onSave.optimizeImports"), Some(false));
        assert_eq!(session.get::<bool>(zen_workbench::folding::FOLD_IMPORTS), Some(true));
        assert_eq!(session.get::<bool>(zen_workbench::folding::SHOW_ARROWS), Some(true));
        assert_eq!(session.get::<String>(zen_workbench::folding::ARROWS).as_deref(), Some("onHover"));
        assert_eq!(session.get::<bool>(zen_workbench::folding::BOTTOM_ARROWS), Some(false));
        for off in [
            zen_modules::folding::FILE_HEADER,
            zen_modules::folding::DOC_COMMENTS,
            zen_modules::folding::METHOD_BODIES,
            zen_modules::folding::CUSTOM_REGIONS,
        ] {
            assert_eq!(session.get::<bool>(off), Some(false), "{off}");
        }
        let folding = IdeSettings {
            fold_arrows: FoldArrows::Always,
            fold_bottom_arrows: true,
            show_fold_arrows: false,
            fold_method_bodies: true,
            ..IdeSettings::default()
        };
        folding.apply_to_session(&mut session);
        assert_eq!(session.get::<String>(zen_workbench::folding::ARROWS).as_deref(), Some("always"));
        assert_eq!(session.get::<bool>(zen_workbench::folding::BOTTOM_ARROWS), Some(true));
        assert_eq!(session.get::<bool>(zen_workbench::folding::SHOW_ARROWS), Some(false));
        assert_eq!(session.get::<bool>(zen_modules::folding::METHOD_BODIES), Some(true));
        let json = serde_json::to_value(&folding).unwrap();
        assert_eq!((json["foldArrows"].clone(), json["foldBottomArrows"].clone()), (json!("always"), json!(true)));
        let settings = IdeSettings { reformat_on_save: true, optimize_imports_on_save: true, ..IdeSettings::default() };
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>("editor.onSave.reformat"), Some(true));
        assert_eq!(session.get::<bool>("editor.onSave.optimizeImports"), Some(true));
    }

    #[test]
    fn the_code_folding_page_finds_every_label_and_keeps_a_modules_options() {
        zen_i18n::register(WIDGET_MESSAGES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        let in_combo = ["settings-folding-arrows-always", "settings-folding-arrows-hover"];
        for language in languages {
            if snapshots.is_some() {
                zen_i18n::set_language(Some(language));
            }
            let mut harness = Harness::new(PageId::CodeFolding);
            harness.folding = vec![WIDGETS];
            let highlight = harness.palette.search_highlight;
            harness.frame(Vec::new());
            let plain = harness.frame(Vec::new());
            assert_eq!(tinted_pixels(&plain, highlight), 0);
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("code-folding-page-{language}.png")), &plain).unwrap();
            }
            let labels =
                page::CODE_FOLDING_SEARCH.iter().chain(["test-folding-widgets", "test-folding-widgets-option"].iter());
            for key in labels {
                let label = t!(key);
                harness.dialog.section = PageId::Appearance;
                label.trim_end_matches(':').clone_into(&mut harness.dialog.search.text);
                harness.step(Vec::new());
                let rows = harness
                    .dialog
                    .section_rows
                    .iter()
                    .map(|(index, ..)| harness.sections[*index].page)
                    .collect::<Vec<_>>();
                assert!(rows.contains(&PageId::CodeFolding), "{language}: {label} shows Code Folding in {rows:?}");
                harness.dialog.section = PageId::CodeFolding;
                let marked = harness.frame(Vec::new());
                assert_eq!(harness.dialog.section, PageId::CodeFolding, "{language}: {label}");
                if !in_combo.contains(key) {
                    assert!(tinted_pixels(&marked, highlight) > 0, "{language}: the page marks {label}");
                }
                if let Some(dir) = &snapshots
                    && *key == "test-folding-widgets-option"
                {
                    let file = dir.join(format!("code-folding-search-{language}.png"));
                    crate::offscreen::write_png(&file, &marked).unwrap();
                }
            }
        }
        if snapshots.is_some() {
            zen_i18n::set_language(Some("en"));
        }
        let mut harness = Harness::new(PageId::CodeFolding);
        harness.folding = vec![WIDGETS];
        harness.frame(Vec::new());
        let draft = harness.dialog.draft.as_ref().unwrap();
        assert!(draft.show_fold_arrows && draft.fold_arrows == FoldArrows::OnHover && !draft.fold_bottom_arrows);
        assert!(!draft.fold_file_header && draft.fold_imports);
        assert!(!draft.fold_doc_comments && !draft.fold_method_bodies && !draft.fold_custom_regions);
        assert!(draft.declared_settings.is_empty(), "a module's option keeps its default until it is changed");
    }

    #[test]
    fn the_general_page_links_to_code_folding() {
        let mut harness = Harness::new(PageId::EditorGeneral);
        let draw = |harness: &mut Harness| {
            let (dialog, palette, sections) = (&mut harness.dialog, &harness.palette, harness.sections.clone());
            harness.screen.accessibility_tree(|ui| {
                dialog.general_page(ui, palette, &sections);
            })
        };
        let tree = draw(&mut harness);
        let link = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label().is_some_and(|label| label == t!("settings-section-code-folding")))
            .and_then(|(_, node)| node.bounds())
            .expect("a link to Code Folding");
        let at = pos2(((link.x0 + link.x1) / 2.0) as f32, ((link.y0 + link.y1) / 2.0) as f32);
        let click = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![click(true)], vec![click(false)]] {
            let (dialog, palette, sections) = (&mut harness.dialog, &harness.palette, harness.sections.clone());
            harness.screen.step(events, |ui| dialog.general_page(ui, palette, &sections));
        }
        assert_eq!(harness.dialog.section, PageId::CodeFolding);
    }

    const WIDGETS: zen_modules::folding::FoldingOptions = zen_modules::folding::FoldingOptions {
        title: "test-folding-widgets",
        options: &[zen_modules::folding::FoldOption {
            key: "test.folding.widgets",
            label: "test-folding-widgets-option",
            default: true,
        }],
    };

    const WIDGET_MESSAGES: &[(&str, &str)] = &[
        ("en", "test-folding-widgets = Widgets\ntest-folding-widgets-option = One-line widget bodies\n"),
        ("ru", "test-folding-widgets = Виджеты\ntest-folding-widgets-option = Однострочные тела виджетов\n"),
    ];
}

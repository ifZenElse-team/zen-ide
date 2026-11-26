use std::collections::HashMap;
use std::path::PathBuf;

use egui::Ui;
use zen_i18n::t;
use zen_modules::form::{Action, Browse, Item, Target, Tone};

use super::page::{PageId, SectionDef};
use super::{SettingsContext, SettingsDialog, widgets};
use crate::ide::IdePalette;

#[derive(Default)]
pub(super) struct FormPage {
    error: Option<String>,
    paths: HashMap<String, (String, Option<String>)>,
}

impl SettingsDialog {
    pub(super) fn module_page(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        context: &SettingsContext<'_>,
        page: &str,
        query: Option<&str>,
    ) {
        let Some(form) = &context.form else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(t!("settings-module-loading")).color(p.text_secondary));
            });
            return;
        };
        self.form_items(ui, p, &form.items, context.sections, query);
        if let Some(error) = &self.form.error {
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(error).color(p.error_text)).wrap());
        }
        self.declared_group(ui, p, context.declared, page, query);
        if form.busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    fn form_items(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        items: &[Item],
        sections: &[SectionDef],
        query: Option<&str>,
    ) {
        for (index, item) in items.iter().enumerate() {
            if let Item::Group { title } = item {
                if index > 0 {
                    ui.add_space(12.0);
                }
                widgets::group_title(ui, p, title, query);
                continue;
            }
            self.form_item(ui, p, item, sections, query);
            ui.add_space(6.0);
        }
    }

    fn form_item(&mut self, ui: &mut Ui, p: &IdePalette, item: &Item, sections: &[SectionDef], query: Option<&str>) {
        match item {
            Item::Group { title } => {
                widgets::group_title(ui, p, title, query);
            }
            Item::Text { text, tone, spinner } => {
                let color = match tone {
                    Tone::Normal => p.text,
                    Tone::Secondary => p.text_secondary,
                    Tone::Error => p.error_text,
                };
                let job = widgets::highlighted(
                    text,
                    query,
                    egui::TextStyle::Body.resolve(ui.style()),
                    color,
                    p.search_highlight,
                );
                if *spinner {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.add(egui::Label::new(job).wrap());
                    });
                } else {
                    ui.add(egui::Label::new(job).wrap());
                }
            }
            Item::Link { label, target } => {
                if ui.link(label).clicked() {
                    match target {
                        Target::Url { url } => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
                        Target::Page { page } => {
                            if let Some(page) = PageId::from_name(page, sections) {
                                self.section = page;
                            }
                        }
                    }
                }
            }
            Item::Row { items } => {
                ui.horizontal_wrapped(|ui| {
                    for item in items {
                        self.form_item(ui, p, item, sections, query);
                    }
                });
            }
            Item::Choice { label, options, selected, action } => {
                let shown = options
                    .iter()
                    .find(|option| Some(&option.value) == selected.as_ref())
                    .map(|option| option.label.clone())
                    .unwrap_or_default();
                let combo = egui::ComboBox::from_id_salt(("settings-form-choice", label.as_str()))
                    .selected_text(shown)
                    .width(widgets::field_width(ui, 280.0, 640.0) - 120.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for option in options {
                            let current = Some(&option.value) == selected.as_ref();
                            if ui.selectable_label(current, &option.label).clicked() && !current {
                                self.run(action, Some(serde_json::Value::String(option.value.clone())));
                            }
                        }
                    });
                crate::accessibility::name(&combo.response, egui::Role::ComboBox, label);
            }
            Item::Menu { label, entries } => {
                ui.menu_button(label, |ui| {
                    for entry in entries {
                        if ui.button(&entry.label).clicked() {
                            ui.close();
                            self.run(&entry.action, None);
                        }
                    }
                });
            }
            Item::Button { label, enabled, action } => {
                if ui.add_enabled(*enabled, egui::Button::new(label)).clicked() {
                    self.run(action, None);
                }
            }
            Item::Path { label, value, action } => {
                self.text_item(ui, p, label, query, value.as_deref(), action, Some(false));
            }
            Item::Field { label, value, action, browse } => {
                let browse = match browse {
                    Browse::None => None,
                    Browse::File => Some(false),
                    Browse::Folder => Some(true),
                };
                let value = Some(value.as_str()).filter(|value| !value.is_empty());
                self.text_item(ui, p, label, query, value, action, browse);
            }
            Item::Check { label, value, action } => {
                let mut on = *value;
                if widgets::checkbox(ui, p, &mut on, label, query).changed() {
                    self.run(action, Some(serde_json::Value::Bool(on)));
                }
            }
            Item::Setting { key, label, default } => {
                let Some(draft) = self.draft.as_mut() else { return };
                match key.as_str() {
                    "reformatOnSave" => {
                        let mut actions = self.on_save.actions(draft);
                        if widgets::checkbox(ui, p, &mut actions.reformat, label, query).changed() {
                            self.on_save.set_actions(draft, actions);
                        }
                    }
                    "optimizeImportsOnSave" => {
                        let mut actions = self.on_save.actions(draft);
                        if widgets::checkbox(ui, p, &mut actions.optimize_imports, label, query).changed() {
                            self.on_save.set_actions(draft, actions);
                        }
                    }
                    _ => {
                        let mut on = draft.declared_settings.get(key).copied().unwrap_or(*default);
                        if widgets::checkbox(ui, p, &mut on, label, query).changed() {
                            draft.declared_settings.insert(key.clone(), on);
                        }
                    }
                }
            }
        }
    }

    #[expect(clippy::too_many_arguments, reason = "one field drawn from its item's parts")]
    fn text_item(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        label: &str,
        query: Option<&str>,
        value: Option<&str>,
        action: &Action,
        browse: Option<bool>,
    ) {
        let key = format!("{}{}", action.command, serde_json::Value::Object(action.params.clone()));
        let (text, shown) = self.form.paths.entry(key).or_default();
        if shown.as_deref() != value {
            *text = value.unwrap_or_default().to_owned();
            *shown = value.map(str::to_owned);
        }
        let field = match browse {
            Some(folder) => widgets::path_field(ui, p, label, query, text, folder),
            None => widgets::text_field(ui, p, label, query, text),
        };
        if (field.lost_focus() || (field.changed() && !field.has_focus()))
            && let Some(wanted) = path_change(text, value)
        {
            let action = action.clone();
            self.run(&action, Some(wanted.map_or(serde_json::Value::Null, serde_json::Value::String)));
        }
    }

    fn run(&mut self, action: &Action, value: Option<serde_json::Value>) {
        let value = match &action.pick_folder {
            Some(title) => match pick_folder(title) {
                Some(folder) => Some(serde_json::Value::String(folder.display().to_string())),
                None => return,
            },
            None => value,
        };
        self.form_request = Some((action.command.clone(), action.params_with(value)));
    }

    pub(in crate::ide) fn form_answer(&mut self, answer: Result<serde_json::Value, String>) {
        self.form.error = answer.err();
    }

    pub(super) fn declared_group(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        declared: &[zen_project::inspect::DeclaredSetting],
        page: &str,
        query: Option<&str>,
    ) {
        let Some(draft) = self.draft.as_mut() else { return };
        let mut groups: Vec<&str> = Vec::new();
        for setting in declared.iter().filter(|setting| setting.page == page) {
            if !groups.contains(&setting.group.as_str()) {
                groups.push(&setting.group);
            }
        }
        for group in groups {
            ui.add_space(12.0);
            widgets::group_title(ui, p, group, query);
            for setting in declared.iter().filter(|setting| setting.page == page && setting.group == group) {
                let mut on = draft.declared_settings.get(&setting.key).copied().unwrap_or(setting.default);
                if widgets::checkbox(ui, p, &mut on, &setting.title, query).changed() {
                    draft.declared_settings.insert(setting.key.clone(), on);
                }
                ui.indent(("declared", &setting.key), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let note = |text: String| egui::RichText::new(text).size(12.0).color(p.text_secondary);
                    ui.add(egui::Label::new(note(setting.description.clone())).wrap());
                    if !setting.network.is_empty() {
                        let hosts = setting.network.join(", ");
                        ui.add(egui::Label::new(note(t!("settings-declared-network", hosts = hosts))).wrap());
                    }
                });
                ui.add_space(6.0);
            }
        }
    }
}

fn path_change(text: &str, saved: Option<&str>) -> Option<Option<String>> {
    let wanted = Some(text.trim()).filter(|text| !text.is_empty());
    (wanted != saved).then(|| wanted.map(str::to_owned))
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

#[cfg(target_arch = "wasm32")]
fn pick_folder(_title: &str) -> Option<PathBuf> {
    None
}

#[cfg(test)]
pub(super) mod tests {
    use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Shape, vec2};
    use serde_json::json;
    use zen_modules::form::{Choice, Form, MenuEntry};

    use super::*;
    use crate::ide::settings::{IdeSettings, page};
    use crate::theme;

    pub(in crate::ide::settings) const PAGE: zen_modules::SettingsPage = zen_modules::SettingsPage {
        id: "demo",
        title: "settings-languages",
        search: &[],
        form: "demo.settings",
        commands: &[],
        parent: "",
    };

    struct Page {
        ctx: egui::Context,
        dialog: SettingsDialog,
        form: Form,
        texts: Vec<(String, Rect)>,
        declared: Vec<zen_project::inspect::DeclaredSetting>,
        sections: Vec<SectionDef>,
    }

    fn texts(shape: &Shape, found: &mut Vec<(String, Rect)>) {
        match shape {
            Shape::Text(text) => found.push((text.galley.text().to_owned(), text.visual_bounding_rect())),
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, found)),
            _ => {}
        }
    }

    fn form() -> Form {
        Form {
            items: vec![
                Item::Group { title: "GOROOT".to_owned() },
                Item::Row {
                    items: vec![
                        Item::Choice {
                            label: "GOROOT".to_owned(),
                            options: vec![
                                Choice { value: "/sdk/a".to_owned(), label: "go1.27   /sdk/a".to_owned() },
                                Choice { value: "/sdk/b".to_owned(), label: "go1.26   /sdk/b".to_owned() },
                            ],
                            selected: Some("/sdk/a".to_owned()),
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
                Item::Group { title: "Tool".to_owned() },
                Item::Text { text: "1.0   /bin/tool".to_owned(), tone: Tone::Normal, spinner: false },
                Item::Path {
                    label: "Custom path:".to_owned(),
                    value: None,
                    action: Action::with_value("demo.tool.choose", "path"),
                },
                Item::Row {
                    items: vec![Item::Button {
                        label: "Update tool".to_owned(),
                        enabled: true,
                        action: Action::command("demo.tool.install"),
                    }],
                },
                Item::Group { title: "On Save".to_owned() },
                Item::Setting {
                    key: "reformatOnSave".to_owned(),
                    label: "Reformat code on save".to_owned(),
                    default: false,
                },
                Item::Setting { key: "demo.check".to_owned(), label: "Check something".to_owned(), default: true },
                Item::Check {
                    label: "Work offline".to_owned(),
                    value: true,
                    action: Action::with_value("demo.set", "value").param("key", "offline"),
                },
                Item::Field {
                    label: "VM options:".to_owned(),
                    value: "-Xmx1g".to_owned(),
                    action: Action::with_value("demo.set", "value").param("key", "vmOptions"),
                    browse: Browse::None,
                },
                Item::Link {
                    label: "Configure on Actions on Save".to_owned(),
                    target: Target::Page { page: "actionsOnSave".to_owned() },
                },
            ],
            busy: false,
        }
    }

    fn declared() -> Vec<zen_project::inspect::DeclaredSetting> {
        let setting = |key: &str, page: &str, title: &str| zen_project::inspect::DeclaredSetting {
            key: key.to_owned(),
            page: page.to_owned(),
            group: "Dependencies".to_owned(),
            title: title.to_owned(),
            description: "Asks the module proxy for newer versions.".to_owned(),
            default: true,
            network: vec!["vuln.go.dev".to_owned(), "api.osv.dev".to_owned()],
        };
        vec![
            setting("demo.dependencies", "demo", "Check for newer versions of dependencies"),
            setting("other.key", "other", "Elsewhere"),
        ]
    }

    impl Page {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut dialog = SettingsDialog::default();
            dialog.open();
            dialog.section = PageId::Module("demo");
            dialog.draft = Some(IdeSettings::default());
            let sections = page::sections(&[PAGE], &[]);
            let mut page = Self { ctx, dialog, form: form(), texts: Vec::new(), declared: declared(), sections };
            page.frame(Vec::new());
            page
        }

        fn frame(&mut self, events: Vec<Event>) {
            let settings = IdeSettings::default();
            let context = SettingsContext {
                settings: &settings,
                mcp_url: None,
                product_id: "test",
                executable: None,
                form: Some(self.form.clone()),
                sections: &self.sections,
                language_servers: None,
                declared: &self.declared,
                folding: &[],
            };
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 1400.0))),
                ..Default::default()
            };
            let dialog = &mut self.dialog;
            let mut output = self.ctx.run_ui(input, |ui| match dialog.section {
                PageId::Module(id) => dialog.module_page(ui, &IdePalette::zen_dark(), &context, id, None),
                PageId::ActionsOnSave => dialog.actions_on_save_page(ui, &IdePalette::zen_dark()),
                PageId::SystemSettings => dialog.system_page(ui, &IdePalette::zen_dark(), None),
                _ => {}
            });
            output.textures_delta.clear();
            self.texts.clear();
            for clipped in &output.shapes {
                texts(&clipped.shape, &mut self.texts);
            }
        }

        fn click(&mut self, label: &str) {
            let at = self
                .texts
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, rect)| rect.center())
                .unwrap_or_else(|| panic!("no {label:?} among {:?}", self.texts));
            self.frame(vec![Event::PointerMoved(at)]);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn shows(&self, text: &str) -> bool {
            self.texts.iter().any(|(shown, _)| shown == text)
        }
    }

    #[test]
    fn a_module_page_draws_its_form_and_asks_for_its_commands() {
        let mut page = Page::new();
        for text in ["GOROOT", "go1.27   /sdk/a", "Add SDK", "Tool", "1.0   /bin/tool", "Update tool", "On Save"] {
            assert!(page.shows(text), "no {text:?} among {:?}", page.texts);
        }
        assert!(page.shows("Check for newer versions of dependencies") && !page.shows("Elsewhere"));
        assert!(page.texts.iter().any(|(text, _)| text.contains("vuln.go.dev, api.osv.dev")), "{:?}", page.texts);
        assert!(page.dialog.form_request.is_none(), "nothing runs by looking");
        page.click("Update tool");
        assert_eq!(page.dialog.form_request.take(), Some(("demo.tool.install".to_owned(), json!({}))));
        page.dialog.form_answer(Err("not a tool".to_owned()));
        page.frame(Vec::new());
        assert!(page.shows("not a tool"));
        page.dialog.form_answer(Ok(json!({})));
        page.frame(Vec::new());
        assert!(!page.shows("not a tool"), "an error stays until the next command");
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            zen_i18n::set_language(Some("en"));
            let mut screen = crate::offscreen::Offscreen::new(vec2(760.0, 900.0), 2.0);
            let palette = IdePalette::zen_dark();
            palette.apply(screen.context());
            let settings = IdeSettings::default();
            let context = SettingsContext {
                settings: &settings,
                mcp_url: None,
                product_id: "test",
                executable: None,
                form: Some(page.form.clone()),
                sections: &page.sections,
                language_servers: None,
                declared: &page.declared,
                folding: &[],
            };
            let dialog = &mut page.dialog;
            let image = screen.render(|ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.popup);
                egui::Frame::new()
                    .inner_margin(16)
                    .show(ui, |ui| dialog.module_page(ui, &palette, &context, "demo", None));
            });
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("module-page.png"), &image).unwrap();
        }
    }

    #[test]
    fn settings_of_the_ide_and_declared_ones_change_the_draft_and_a_link_opens_its_page() {
        let mut page = Page::new();
        page.click("Reformat code on save");
        assert!(page.dialog.draft.as_ref().unwrap().reformat_on_save);
        page.click("Check something");
        assert_eq!(page.dialog.draft.as_ref().unwrap().declared_settings.get("demo.check"), Some(&false));
        page.click("Check for newer versions of dependencies");
        assert_eq!(page.dialog.draft.as_ref().unwrap().declared_settings.get("demo.dependencies"), Some(&false));
        page.click("Configure on Actions on Save");
        assert_eq!(page.dialog.section, PageId::ActionsOnSave);
        assert!(page.dialog.form_request.is_none(), "the options run no command");
    }

    #[test]
    fn a_check_box_and_a_text_field_of_a_form_run_their_command_with_the_value() {
        let mut page = Page::new();
        assert!(page.shows("VM options:") && page.shows("-Xmx1g"), "{:?}", page.texts);
        page.click("Work offline");
        assert_eq!(
            page.dialog.form_request.take(),
            Some(("demo.set".to_owned(), json!({ "key": "offline", "value": false })))
        );
    }

    #[test]
    fn generators_network_and_settings_are_in_system_settings() {
        let mut page = Page::new();
        page.dialog.generator_settings = vec![(
            "Spring Boot".to_owned(),
            zen_project::generators::GeneratorSetting {
                key: "spring-boot.initializr.serviceUrl".to_owned(),
                label: "service URL".to_owned(),
                help: Some("Your own instance".to_owned()),
                default: "https://start.spring.io".to_owned(),
            },
        )];
        page.dialog.section = PageId::SystemSettings;
        page.frame(Vec::new());
        page.frame(Vec::new());
        assert!(page.shows("Spring Boot: service URL") && page.shows("Your own instance"), "{:?}", page.texts);
        assert!(page.shows("https://start.spring.io"), "the default is the hint: {:?}", page.texts);
        assert!(page.dialog.draft.as_ref().unwrap().generators_network, "on by default");
        page.click(&t!("settings-generators-network"));
        assert!(!page.dialog.draft.as_ref().unwrap().generators_network);
    }

    #[test]
    fn autosave_options_are_in_system_settings_and_actions_on_save_links_to_them() {
        let mut page = Page::new();
        page.dialog.section = PageId::ActionsOnSave;
        page.frame(Vec::new());
        page.click(&t!("settings-on-save-autosave"));
        assert_eq!(page.dialog.section, PageId::SystemSettings, "the link opens System Settings");
        page.frame(Vec::new());
        for key in [
            "settings-system-files",
            "settings-system-autosave-idle",
            "settings-system-autosave-seconds",
            "settings-system-autosave-deactivation",
            "settings-system-autosave-note",
        ] {
            assert!(page.shows(&t!(key)), "no {key} among {:?}", page.texts);
        }
        assert!(page.shows("15"), "the idle time is 15 seconds: {:?}", page.texts);
        let draft = page.dialog.draft.as_ref().unwrap();
        assert!(draft.autosave_on_idle && draft.autosave_on_frame_deactivation, "both on by default");
        page.click(&t!("settings-system-autosave-idle"));
        page.click(&t!("settings-system-autosave-deactivation"));
        let draft = page.dialog.draft.as_ref().unwrap();
        assert!(!draft.autosave_on_idle && !draft.autosave_on_frame_deactivation, "the check boxes switch them");
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let mut dialog = SettingsDialog::default();
            dialog.open();
            dialog.draft = Some(IdeSettings::default());
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                for (name, section) in
                    [("system-settings", PageId::SystemSettings), ("actions-on-save", PageId::ActionsOnSave)]
                {
                    let mut screen = crate::offscreen::Offscreen::new(vec2(760.0, 300.0), 2.0);
                    let palette = IdePalette::zen_dark();
                    palette.apply(screen.context());
                    let image = screen.render(|ui| {
                        ui.painter().rect_filled(ui.max_rect(), 0, palette.popup);
                        egui::Frame::new().inner_margin(16).show(ui, |ui| match section {
                            PageId::SystemSettings => dialog.system_page(ui, &palette, None),
                            _ => dialog.actions_on_save_page(ui, &palette),
                        });
                    });
                    let file = std::path::Path::new(&dir).join(format!("{name}-{language}.png"));
                    crate::offscreen::write_png(&file, &image).unwrap();
                }
            }
            zen_i18n::set_language(Some("en"));
        }
    }

    #[test]
    fn a_page_waits_for_its_form() {
        let mut page = Page::new();
        page.dialog.form_answer(Ok(json!({})));
        let settings = IdeSettings::default();
        let context = SettingsContext {
            settings: &settings,
            mcp_url: None,
            product_id: "test",
            executable: None,
            form: None,
            sections: &page.sections,
            language_servers: None,
            declared: &[],
            folding: &[],
        };
        let dialog = &mut page.dialog;
        let mut output = page.ctx.run_ui(egui::RawInput::default(), |ui| {
            dialog.module_page(ui, &IdePalette::zen_dark(), &context, "demo", None);
        });
        output.textures_delta.clear();
        let mut found = Vec::new();
        for clipped in &output.shapes {
            texts(&clipped.shape, &mut found);
        }
        assert!(found.iter().any(|(text, _)| *text == t!("settings-module-loading")), "{found:?}");
    }

    #[test]
    fn a_path_field_asks_for_a_change_only() {
        assert_eq!(path_change("/go/bin/dlv", Some("/go/bin/dlv")), None);
        assert_eq!(path_change(" /go/bin/dlv ", Some("/go/bin/dlv")), None);
        assert_eq!(path_change("", None), None);
        assert_eq!(path_change("/opt/dlv", Some("/go/bin/dlv")), Some(Some("/opt/dlv".to_owned())));
        assert_eq!(path_change("  ", Some("/go/bin/dlv")), Some(None));
    }
}

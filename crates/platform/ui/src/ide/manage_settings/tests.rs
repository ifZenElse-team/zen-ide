use std::sync::{Arc, Mutex};

use egui::accesskit::TreeUpdate;
use egui::{Event, vec2};
use serde_json::json;

use super::*;

struct Setup {
    data: PathBuf,
    registry: Registry,
    session: Session,
    view: IdeView,
    dir: tempfile::TempDir,
}

fn product() -> super::super::IdeProduct {
    super::super::IdeProduct {
        name: "GoGarden",
        id: "gogarden",
        version: "0.0.0",
        highlighters: Vec::new(),
        plugins: None,
        mcp_port: 0,
        modules: Vec::new(),
        areas: Vec::new(),
        settings_pages: Vec::new(),
        code_styles: Vec::new(),
        debugger: None,
        project_models: Vec::new(),
        outliners: Vec::new(),
        run_providers: Vec::new(),
        language_servers: Vec::new(),
        templates: Vec::new(),
        folding_options: Vec::new(),
        inspectors: Vec::new(),
    }
}

fn setup() -> Setup {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(data.join("keymaps")).unwrap();
    std::fs::write(data.join("settings.json"), r#"{"ligatures": true}"#).unwrap();
    std::fs::write(data.join("templates.json"), "{}").unwrap();
    std::fs::write(
        data.join("keymaps/mine.json"),
        r#"{"formatVersion": 1, "id": "mine", "name": "Mine", "shortcuts": {}}"#,
    )
    .unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let keymaps = Arc::new(Mutex::new(zen_workbench::keymaps::KeymapStore::new(
        Some(data.clone()),
        zen_jb_import::keymap_importers(),
    )));
    let themes = Arc::new(Mutex::new(zen_theme::ThemeStore::new(
        Some(data.join("themes")),
        Box::new(zen_workbench::themes::DiskFiles),
        zen_jb_import::importers(),
    )));
    let target = zen_workbench::ide_settings::SettingsTarget {
        data: Some(data.clone()),
        product: "GoGarden".to_owned(),
        version: "0.0.0".to_owned(),
        keymaps: Some(keymaps),
        themes: Some(themes),
        plugins: None,
    };
    zen_workbench::ide_settings::register(&mut registry, target).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    let view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
    Setup { dir, data, registry, session, view }
}

fn jetbrains_zip(path: &Path) {
    let mut writer = zen_workbench::ide_settings::zip::ZipWriter::new(1_792_000_000);
    let keymap = r#"<keymap version="1" name="Synthetic Mac" parent="Mac OS X 10.5+">
  <action id="EditorDuplicate"><keyboard-shortcut first-keystroke="meta shift D" /></action>
</keymap>"#;
    let scheme = r#"<scheme name="Synthetic Dusk" parent_scheme="Darcula">
  <colors><option name="CARET_ROW_COLOR" value="2b2b2b" /></colors>
</scheme>"#;
    writer.add("IntelliJ IDEA Global Settings", b"").unwrap();
    writer.add("keymaps/Synthetic Mac.xml", keymap.as_bytes()).unwrap();
    writer.add("colors/Synthetic Dusk.icls", scheme.as_bytes()).unwrap();
    writer.add("options/editor.xml", b"<application />").unwrap();
    std::fs::write(path, writer.finish()).unwrap();
}

fn labels(tree: &TreeUpdate) -> Vec<String> {
    tree.nodes.iter().filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned)).collect()
}

fn enter() -> Event {
    Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

impl Setup {
    fn act(&mut self, ctx: &egui::Context, action: &str, params: Value) -> Value {
        let Setup { registry, session, view, .. } = self;
        match view.perform_action(ctx, registry, session, action, params, true) {
            Ok(value) => value,
            Err(super::super::agent::ActionError::Refused(message)) => panic!("{action}: {message}"),
            Err(super::super::agent::ActionError::Dispatch(error)) => panic!("{action}: {error}"),
        }
    }
}

#[test]
fn the_dialogs_export_import_and_restore() {
    let languages: &[&str] = if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { &["en", "ru"] } else { &["en"] };
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for language in languages {
        let mut setup = setup();
        zen_i18n::set_language(Some(language));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1000.0, 700.0), 1.5);
        let ctx = screen.context().clone();
        let mut shoot = |setup: &mut Setup, name: &str, events: Vec<Event>| {
            let Setup { registry, session, view, .. } = setup;
            screen.step(events, |ui| view.show(ui, registry, session));
            if let Some(dir) = &snapshots {
                let image = screen.render(|ui| view.show(ui, registry, session));
                crate::offscreen::write_png(&dir.join(format!("{name}-{language}.png")), &image).unwrap();
            }
            screen.step(Vec::new(), |ui| view.show(ui, registry, session));
            let tree = screen.accessibility_tree(|ui| view.show(ui, registry, session));
            labels(&tree)
        };

        let zip = setup.dir.path().join("out/settings.zip");
        setup.act(&ctx, "view.ideSettings.export", json!({ "path": zip }));
        assert_eq!(setup.view.manage_settings.open_dialogs(), ["exportSettings"]);
        let shown = shoot(&mut setup, "export-settings", Vec::new());
        for label in
            [t!("ide-settings-select-all"), t!("ide-settings-component-keymaps"), t!("ide-settings-component-plugins")]
        {
            assert!(shown.iter().any(|shown| shown.contains(&label)), "{label} in {shown:?}");
        }
        shoot(&mut setup, "export-settings-done", vec![enter()]);
        assert!(setup.view.manage_settings.open_dialogs().is_empty());
        let archive = zen_workbench::ide_settings::zip::Archive::parse(
            std::fs::read(&zip).unwrap(),
            zen_workbench::ide_settings::zip::Limits::SETTINGS,
        )
        .unwrap();
        let names: Vec<&str> = archive.entries().iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["zen-settings.json", "settings.json", "keymaps/mine.json", "templates.json"]);

        setup.act(&ctx, "view.ideSettings.import", json!({}));
        assert_eq!(setup.view.manage_settings.open_dialogs(), ["importSettings"]);
        shoot(&mut setup, "import-settings", Vec::new());

        let jetbrains = setup.dir.path().join("jetbrains-settings.zip");
        jetbrains_zip(&jetbrains);
        setup.act(&ctx, "view.ideSettings.import", json!({ "path": jetbrains }));
        assert_eq!(setup.view.manage_settings.open_dialogs(), ["selectComponentsToImport"]);
        let shown = shoot(&mut setup, "select-components-jetbrains", Vec::new());
        assert!(shown.iter().any(|shown| shown.contains("options/editor.xml")), "{shown:?}");
        let shown = shoot(&mut setup, "settings-imported-jetbrains", vec![enter()]);
        assert_eq!(setup.view.manage_settings.open_dialogs(), ["settingsImported"]);
        assert!(shown.iter().any(|shown| shown.contains(&t!("ide-settings-component-keymaps"))), "{shown:?}");
        assert!(setup.data.join("keymaps/synthetic-mac.json").is_file());
        assert!(std::fs::read_dir(setup.data.join("themes")).unwrap().count() == 1, "the colour scheme became a theme");
        shoot(&mut setup, "settings-imported-closed", vec![enter()]);
        assert!(setup.view.manage_settings.open_dialogs().is_empty());
        assert!(!setup.view.requests.iter().any(|request| matches!(request, Request::Restart)));

        setup.act(&ctx, "view.ideSettings.import", json!({ "path": zip, "components": ["settings"] }));
        shoot(&mut setup, "select-components-zen", Vec::new());
        shoot(&mut setup, "settings-imported-restart", vec![enter()]);
        assert_eq!(setup.view.manage_settings.open_dialogs(), ["settingsImported"]);
        shoot(&mut setup, "settings-imported-restarting", vec![enter()]);
        assert!(setup.view.requests.iter().any(|request| matches!(request, Request::Restart)));
        setup.view.requests.clear();

        setup.act(&ctx, "view.ideSettings.restoreDefaults", json!({}));
        assert_eq!(setup.view.manage_settings.open_dialogs(), ["restoreDefaultSettings"]);
        let shown = shoot(&mut setup, "restore-default-settings", Vec::new());
        assert!(shown.iter().any(|shown| shown.contains(&t!("ide-settings-restore-ok"))), "{shown:?}");
        shoot(&mut setup, "restore-default-settings-done", vec![enter()]);
        assert!(setup.view.requests.iter().any(|request| matches!(request, Request::Restart)));
        assert!(!setup.data.join("settings.json").exists());
        let backups = setup.data.join(zen_workbench::ide_settings::BACKUPS);
        let backup = std::fs::read_dir(&backups).unwrap().next().unwrap().unwrap().path();
        assert!(backup.join("settings.json").is_file() && backup.join("keymaps/synthetic-mac.json").is_file());
    }
    zen_i18n::set_language(Some("en"));
}

#[test]
fn bad_sources_and_paths_are_told_in_the_dialog() {
    let mut setup = setup();
    zen_i18n::set_language(Some("en"));
    let ctx = egui::Context::default();
    let Setup { registry, session, view, .. } = &mut setup;
    let refused = view
        .perform_action(
            &ctx,
            registry,
            session,
            "view.ideSettings.import",
            json!({ "path": "/nonexistent/x.zip" }),
            true,
        )
        .unwrap_err();
    assert!(
        matches!(refused, super::super::agent::ActionError::Refused(message) if message.contains("/nonexistent/x.zip"))
    );
    let refused = view
        .perform_action(&ctx, registry, session, "view.ideSettings.export", json!({ "components": ["nope"] }), true)
        .unwrap_err();
    assert!(matches!(refused, super::super::agent::ActionError::Refused(_)));
    assert!(view.manage_settings.open_dialogs().is_empty());
    let idea = setup.dir.path().join("project/.idea/settings.zip");
    setup.act(&ctx, "view.ideSettings.export", json!({ "path": idea }));
    let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 600.0), 1.0);
    let Setup { registry, session, view, .. } = &mut setup;
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    screen.step(vec![enter()], |ui| view.show(ui, registry, session));
    let error = view.manage_settings.export.as_ref().and_then(|dialog| dialog.error.clone()).unwrap_or_default();
    assert!(error.contains(".idea"), "{error}");
    assert!(!setup.dir.path().join("project/.idea").exists(), "nothing is written into a project");
}

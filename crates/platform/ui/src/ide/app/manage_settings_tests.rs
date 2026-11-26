use std::sync::mpsc;

use serde_json::{Value, json};

use super::*;

fn product() -> IdeProduct {
    IdeProduct {
        name: "Test",
        id: "test",
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
        inspectors: Vec::new(),
        language_servers: Vec::new(),
        templates: Vec::new(),
        folding_options: Vec::new(),
    }
}

fn frame(ctx: &egui::Context, app: &mut IdeApp) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
}

fn ask(ctx: &egui::Context, app: &mut IdeApp, tool: &str, arguments: Value) -> Value {
    ask_result(ctx, app, tool, arguments.clone()).unwrap_or_else(|error| panic!("{tool} {arguments}: {error}"))
}

fn ask_result(ctx: &egui::Context, app: &mut IdeApp, tool: &str, arguments: Value) -> Result<Value, String> {
    let (sender, answer) = mpsc::channel();
    app.route(
        ctx,
        tool,
        &arguments,
        Box::new(move |result| {
            let _ = sender.send(result);
        }),
    );
    for _ in 0..6 {
        if let Ok(result) = answer.try_recv() {
            return result;
        }
        frame(ctx, app);
    }
    panic!("no answer to {tool} {arguments}")
}

fn popups(ctx: &egui::Context, app: &mut IdeApp) -> Value {
    frame(ctx, app);
    ask(ctx, app, "get_window_state", json!({}))["popups"].clone()
}

fn enter(ctx: &egui::Context, app: &mut IdeApp) {
    ask(ctx, app, "press_keys", json!({ "keys": ["⏎"] }));
    frame(ctx, app);
}

fn person_enter(ctx: &egui::Context, app: &mut IdeApp) {
    let modifiers = egui::Modifiers::NONE;
    let key =
        |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers };
    let input = egui::RawInput { events: vec![key(true), key(false)], ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
    frame(ctx, app);
}

fn restarts() -> usize {
    RESTARTS_ASKED.with(std::cell::Cell::get)
}

fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().filter_map(Result::ok) {
            if entry.file_type().unwrap().is_dir() {
                folders.push(entry.path());
            } else {
                let modified = entry.metadata().unwrap().modified().unwrap();
                let mut bytes = std::fs::read(entry.path()).unwrap();
                bytes.extend_from_slice(format!("{modified:?}").as_bytes());
                found.push((entry.path(), bytes));
            }
        }
    }
    found.sort();
    found
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

#[test]
fn the_debug_log_and_special_files_dialogs_open_and_close_from_their_actions() {
    let dir = tempfile::tempdir().unwrap();
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut app = IdeApp::new(product(), registry, Some(dir.path().join("recent.json")), vec![project]);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    std::fs::write(dir.path().join(zen_workbench::ide_log::CATEGORIES_FILE), "zen_lsp:trace\n").unwrap();

    let open = |action: &str| json!({ "action": action, "params": {} });
    ask(&ctx, &mut app, "run_action", open("view.debugLogSettings.open"));
    assert_eq!(popups(&ctx, &mut app), json!(["debugLogSettings"]));
    let slot = app.slot_for(None).unwrap();
    let text = slot.window.lock().unwrap().view.diagnostics.debug_log.clone();
    assert_eq!(text.as_deref(), Some("zen_lsp:trace"));
    ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] }));
    assert_eq!(popups(&ctx, &mut app), json!([]));

    ask(&ctx, &mut app, "run_action", open("view.specialFiles.open"));
    assert_eq!(popups(&ctx, &mut app), json!(["specialFiles"]));
    let slot = app.slot_for(None).unwrap();
    let rows = slot.window.lock().unwrap().view.diagnostics.special.clone().unwrap();
    assert_eq!(rows.len(), 5, "{rows:?}");
}

#[test]
fn open_source_licences_open_from_their_action_and_close_with_escape() {
    let dir = tempfile::tempdir().unwrap();
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut app = IdeApp::new(product(), registry, Some(dir.path().join("recent.json")), vec![project]);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);

    let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.openSourceLicences.open", "params": {} }));
    assert_eq!(opened["result"], json!({ "folder": null, "files": [] }), "a test build carries no notices: {opened}");
    assert_eq!(popups(&ctx, &mut app), json!(["openSourceLicences"]));
    ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] }));
    assert_eq!(popups(&ctx, &mut app), json!([]));
    assert!(crate::keymap::shortcut("view.openSourceLicences.open").is_none());
    assert_eq!(super::super::keymap_page::action_title("view.openSourceLicences.open"), "Open-source Licences");
}

#[test]
fn manage_ide_settings_exports_imports_and_restores_without_touching_the_project() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_owned();
    std::fs::write(data.join("settings.json"), r#"{"ligatures": true, "confirmExit": false}"#).unwrap();
    std::fs::write(data.join("templates.json"), "{}").unwrap();
    let project = data.join("project");
    std::fs::create_dir_all(project.join(".zen")).unwrap();
    std::fs::create_dir_all(project.join(".idea")).unwrap();
    std::fs::write(project.join(".zen/runConfigurations.json"), "{\"configurations\": []}\n").unwrap();
    std::fs::write(project.join(".idea/workspace.xml"), "<project version=\"4\" />\n").unwrap();
    std::fs::write(project.join(".idea/misc.xml"), "<project version=\"4\" />\n").unwrap();
    std::fs::write(project.join("main.go"), "package main\n").unwrap();
    let settings_before = (files(&project.join(".zen")), files(&project.join(".idea")));

    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut app = IdeApp::new(product(), registry, Some(data.join("recent.json")), vec![project.clone()]);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);

    let listed = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "settings.ide.components" }));
    assert_eq!(listed["data"], json!(data), "{listed}");

    let zip = data.join("exported/settings.zip");
    let opened =
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.ideSettings.export", "params": { "path": zip } }));
    assert_eq!(opened["window"]["popups"], json!(["exportSettings"]), "{opened}");
    enter(&ctx, &mut app);
    assert_eq!(popups(&ctx, &mut app), json!([]));
    assert!(zip.is_file(), "Export Settings wrote the ZIP");

    let jetbrains = data.join("jetbrains-settings.zip");
    jetbrains_zip(&jetbrains);
    let seen = ask(
        &ctx,
        &mut app,
        "dispatch_command",
        json!({ "command": "settings.ide.inspect", "params": { "path": jetbrains } }),
    );
    assert_eq!(seen["kind"], "jetBrains", "{seen}");
    let params = json!({ "action": "view.ideSettings.import", "params": { "path": jetbrains } });
    assert_eq!(ask(&ctx, &mut app, "run_action", params)["window"]["popups"], json!(["selectComponentsToImport"]));
    person_enter(&ctx, &mut app);
    assert_eq!(popups(&ctx, &mut app), json!(["settingsImported"]));
    person_enter(&ctx, &mut app);
    assert_eq!(popups(&ctx, &mut app), json!([]));
    let keymaps = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ui.keymap.list" }));
    assert!(keymaps.to_string().contains("Synthetic Mac"), "{keymaps}");
    let themes = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ui.theme.list" }));
    assert!(themes.to_string().contains("Synthetic Dusk"), "{themes}");

    let before = restarts();
    let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.ideSettings.restoreDefaults" }));
    assert_eq!(opened["window"]["popups"], json!(["restoreDefaultSettings"]));
    person_enter(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert_eq!(restarts(), before + 1, "Restore and Restart restarts");
    assert!(!data.join("settings.json").exists() && !data.join("keymaps").exists());
    let backups: Vec<PathBuf> = std::fs::read_dir(data.join(zen_workbench::ide_settings::BACKUPS))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert!(backups[0].join("settings.json").is_file());

    let params = json!({ "action": "view.ideSettings.import", "params": { "path": backups[0] } });
    ask(&ctx, &mut app, "run_action", params);
    person_enter(&ctx, &mut app);
    assert_eq!(popups(&ctx, &mut app), json!(["settingsImported"]));
    person_enter(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert_eq!(restarts(), before + 2, "Restart after the import");
    let restored = std::fs::read_to_string(data.join("settings.json")).unwrap();
    assert!(restored.contains("ligatures"), "{restored}");
    assert!(data.join("keymaps/synthetic-mac.json").is_file());

    let refused = ask_result(
        &ctx,
        &mut app,
        "dispatch_command",
        json!({ "command": "settings.ide.export", "params": { "path": project.join(".idea/settings.zip") } }),
    )
    .unwrap_err();
    assert!(refused.contains(".idea"), "{refused}");
    assert_eq!((files(&project.join(".zen")), files(&project.join(".idea"))), settings_before);
}

#[test]
fn a_headless_ide_never_restarts_and_a_test_never_reaches_a_real_data_folder() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let not_temporary = PathBuf::from("/Users/Shared/zen-not-temporary/space.ifzenelse.gogarden");
    assert_eq!(settings_data(Some(&not_temporary)), None, "under test only a temporary data folder is changed");
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(settings_data(Some(dir.path())), Some(dir.path().to_owned()));

    std::fs::write(dir.path().join("settings.json"), "{}").unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut headless = IdeApp::build(product(), registry, Some(dir.path().join("recent.json")), vec![project], true);
    let before = restarts();
    let ctx = egui::Context::default();
    let logic = |app: &mut IdeApp| {
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    };
    logic(&mut headless);
    let (sender, answer) = mpsc::channel();
    let params = json!({ "command": "settings.ide.restoreDefaults" });
    headless.route(&ctx, "dispatch_command", &params, Box::new(move |result| drop(sender.send(result))));
    logic(&mut headless);
    let refused = answer.try_recv().unwrap().unwrap_err();
    assert!(refused.contains("agentNotAllowed"), "{refused}");
    assert!(dir.path().join("settings.json").exists(), "an agent's call removes nothing");
    headless.shared.state().requests.push(Request::Restart);
    logic(&mut headless);
    logic(&mut headless);
    assert_eq!(restarts(), before, "a headless IDE never restarts into a window");
}

fn maintenance_app(data: &Path, settings: &str) -> (IdeApp, PathBuf) {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    std::fs::write(data.join("settings.json"), settings).unwrap();
    let project = data.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("main.go"), "package main\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    (IdeApp::new(product(), registry, Some(data.join("recent.json")), vec![project.clone()]), project)
}

fn act(ctx: &egui::Context, app: &mut IdeApp, action: &str, params: Value) -> Value {
    ask(ctx, app, "run_action", json!({ "action": action, "params": params }))
}

#[test]
fn invalidate_and_restart_schedules_the_removal_for_the_next_start_and_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), "{}");
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let caches = dir.path().join("caches/language-servers/jdtls");
    let history = dir.path().join("local-history/kept");
    std::fs::create_dir_all(&caches).unwrap();
    std::fs::create_dir_all(&history).unwrap();

    act(&ctx, &mut app, "view.invalidateCaches.open", json!({}));
    assert_eq!(popups(&ctx, &mut app), json!(["invalidateCaches"]));
    let before = restarts();
    let pressed = ask_result(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] }));
    assert!(pressed.is_err_and(|error| error.contains("Only the person")), "an agent cannot answer it");
    assert_eq!(restarts(), before);
    person_enter(&ctx, &mut app);
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);

    assert_eq!(restarts(), before + 1);
    assert_eq!(popups(&ctx, &mut app), json!([]));
    assert!(caches.exists(), "the IDE still runs on them until it restarts");
    drop(app);

    let (mut restarted, _) = maintenance_app(dir.path(), "{}");
    frame(&ctx, &mut restarted);
    assert!(!caches.exists(), "gone at the next start");
    assert!(history.exists(), "Local History goes only when its checkbox is on");
}

#[test]
fn the_invalidate_dialog_starts_with_local_history_kept_and_the_git_caches_chosen() {
    let chosen = super::super::invalidate_dialog::default_params();
    assert!(!chosen.clear_local_history);
    assert!(chosen.clear_vcs_log);
}

#[test]
fn collect_logs_says_what_the_file_holds_before_it_writes_anything() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), "{}");
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let logs = zen_workbench::ide_log::installed_folder().map_or_else(|| dir.path().join("logs"), Path::to_owned);
    let archives = |logs: &Path| {
        std::fs::read_dir(logs).map_or(0, |entries| {
            entries.flatten().filter(|entry| entry.file_name().to_string_lossy().ends_with(".zip")).count()
        })
    };
    let before = archives(&logs);
    assert!(zen_i18n::t!("ide-collect-logs-text").contains("consol"));
    act(&ctx, &mut app, "view.collectLogs.open", json!({}));
    assert_eq!(popups(&ctx, &mut app), json!(["collectLogs"]));
    assert_eq!(archives(&logs), before, "nothing before the answer");
    ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] }));
    assert_eq!(popups(&ctx, &mut app), json!([]));
    assert_eq!(archives(&logs), before, "cancelled");
}

#[test]
fn a_headless_start_leaves_the_caches_a_running_window_uses_and_the_next_window_removes_them() {
    let dir = tempfile::tempdir().unwrap();
    let caches = dir.path().join("caches/language-servers/jdtls");
    std::fs::create_dir_all(&caches).unwrap();
    let schedule = dir.path().join("invalidate-caches.json");
    std::fs::write(&schedule, r#"{"clearLocalHistory":false,"clearVcsLog":true}"#).unwrap();
    let start = |headless: bool| {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        IdeApp::build(product(), registry, Some(dir.path().join("recent.json")), Vec::new(), headless)
    };
    drop(start(true));
    assert!(caches.exists() && schedule.exists(), "a headless IDE removes nothing");
    drop(start(false));
    assert!(!caches.exists() && !schedule.exists());
}

#[test]
fn an_agent_never_restores_the_default_settings_without_the_person() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), r#"{"confirmExit": false, "ligatures": true}"#);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let before = restarts();

    for params in [json!({}), json!({ "confirmed": true })] {
        act(&ctx, &mut app, "view.ideSettings.restoreDefaults", params);
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(popups(&ctx, &mut app), json!(["restoreDefaultSettings"]));
        assert_eq!(restarts(), before, "nothing happens before the answer");
        assert!(dir.path().join("settings.json").exists(), "the settings stay");
    }
    let pressed = ask_result(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] }));
    assert!(pressed.is_err_and(|error| error.contains("Only the person")), "an agent cannot answer it");
    assert_eq!(restarts(), before);
    assert!(dir.path().join("settings.json").exists());

    person_enter(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert_eq!(restarts(), before + 1);
    assert!(!dir.path().join("settings.json").exists());
}

#[test]
fn an_agent_cannot_answer_the_import_dialogs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), r#"{"confirmExit": false, "ligatures": true}"#);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let zip = dir.path().join("exported.zip");
    let export = json!({ "command": "settings.ide.export", "params": { "path": zip } });
    ask(&ctx, &mut app, "dispatch_command", export);
    let before = restarts();
    let open = json!({ "action": "view.ideSettings.import", "params": { "path": zip } });
    let opened = ask(&ctx, &mut app, "run_action", open);
    assert_eq!(opened["window"]["popups"], json!(["selectComponentsToImport"]));
    let refused = |ctx: &egui::Context, app: &mut IdeApp, keys: Value| {
        let pressed = ask_result(ctx, app, "press_keys", json!({ "keys": keys }));
        assert!(pressed.is_err_and(|error| error.contains("Only the person")), "an agent cannot answer it");
    };
    refused(&ctx, &mut app, json!(["⏎"]));
    assert_eq!(popups(&ctx, &mut app), json!(["selectComponentsToImport"]), "nothing was imported");
    person_enter(&ctx, &mut app);
    assert_eq!(popups(&ctx, &mut app), json!(["settingsImported"]));
    refused(&ctx, &mut app, json!(["⏎"]));
    assert_eq!(restarts(), before, "the agent's key restarts nothing");
    person_enter(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert_eq!(restarts(), before + 1, "the person's does");
}

#[test]
fn a_headless_ide_has_no_person_to_answer_the_import_dialogs() {
    let dir = tempfile::tempdir().unwrap();
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let recent = Some(dir.path().join("recent.json"));
    let mut headless = IdeApp::build(product(), registry, recent, vec![project], true);
    let ctx = egui::Context::default();
    let params = json!({ "action": "view.ideSettings.import", "params": {} });
    let answer = headless.headless_tool(&ctx, "run_action", &params);
    assert!(answer.is_err_and(|error| error.contains("view") || error.contains("run_action")));
}

#[test]
fn a_headless_ide_has_no_person_to_restore_the_default_settings() {
    let dir = tempfile::tempdir().unwrap();
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    std::fs::write(dir.path().join("settings.json"), r#"{"ligatures": true}"#).unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let recent = Some(dir.path().join("recent.json"));
    let mut headless = IdeApp::build(product(), registry, recent, vec![project], true);
    let ctx = egui::Context::default();
    let before = restarts();
    let params = json!({ "action": "view.ideSettings.restoreDefaults", "params": { "confirmed": true } });
    let answer = headless.headless_tool(&ctx, "run_action", &params);
    assert!(answer.is_err(), "{answer:?}");
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| headless.logic(ui.ctx()));
    output.textures_delta.clear();
    assert_eq!(restarts(), before);
    assert!(dir.path().join("settings.json").exists(), "the settings stay");
}

#[test]
fn an_agent_never_restarts_the_ide_without_the_person() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), r#"{"confirmExit": false}"#);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let before = restarts();

    for params in [json!({}), json!({ "confirmed": true })] {
        act(&ctx, &mut app, "view.ide.restart", params);
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(popups(&ctx, &mut app), json!(["confirmRestart"]), "whatever the settings say");
        assert_eq!(restarts(), before, "nothing happens before the answer");
    }
    let pressed = ask_result(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] }));
    assert!(pressed.is_err_and(|error| error.contains("Only the person")), "an agent cannot answer it");
    assert_eq!(restarts(), before);
    person_enter(&ctx, &mut app);
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert_eq!(restarts(), before + 1);
}

#[test]
fn restart_saves_the_open_documents_as_exit_does() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, project) = maintenance_app(dir.path(), r#"{"confirmExit": false}"#);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let file = project.join("main.go");
    let run = json!({ "command": "file.document.open", "params": { "path": file } });
    ask(&ctx, &mut app, "dispatch_command", run);
    let edit =
        json!({ "command": "editor.text.write", "params": { "path": file, "text": "// saved\n", "save": false } });
    ask(&ctx, &mut app, "dispatch_command", edit);
    assert!(!std::fs::read_to_string(&file).unwrap().contains("// saved"), "unsaved until the restart");

    app.shared.state().requests.push(Request::RestartAsking(RestartAsk::IfWorkRuns, None));
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);

    assert!(std::fs::read_to_string(&file).unwrap().contains("// saved"));
}

#[test]
fn repair_ide_runs_the_first_step_and_the_notification_goes_on_from_there() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), "{}");
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);

    let report = act(&ctx, &mut app, "view.repairIde.start", json!({}));
    assert_eq!(report["result"]["step"], "refreshFileSystem", "{report}");
    let listed = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ui.notifications.list" }));
    let notification = &listed["notifications"][0];

    let run = |action: &str| json!({ "action": "ui.notifications.action", "params": { "id": notification["id"], "action": action } });
    let more = ask(&ctx, &mut app, "run_action", run("more"));
    assert_eq!(more["action"], "ui.notifications.action", "{more}");
    assert_eq!(popups(&ctx, &mut app), json!(["invalidateCaches"]), "More ends at the dialog");
}

struct SeenBy(Arc<Mutex<Vec<bool>>>);

impl zen_core::Command for SeenBy {
    type Params = zen_core::NoParams;
    type Output = ();
    const ID: &'static str = "test.seen.by";
    const TITLE: &'static str = "Seen By";
    const DESCRIPTION: &'static str = "Remembers who ran it.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<(), zen_core::CommandError> {
        self.0.lock().unwrap().push(zen_core::macros::by_agent(session));
        Ok(())
    }
}

fn press_bound(ctx: &egui::Context, app: &mut IdeApp, keys: Value) {
    crate::keymap::load(Arc::clone(&app.shared.keymaps));
    frame(ctx, app);
    crate::keymap::load(Arc::clone(&app.shared.keymaps));
    ask(ctx, app, "press_keys", json!({ "keys": keys }));
    frame(ctx, app);
}

fn person_presses(ctx: &egui::Context, app: &mut IdeApp, key: egui::Key) {
    let modifiers = egui::Modifiers { alt: true, ctrl: true, mac_cmd: true, command: true, shift: false };
    let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
    crate::keymap::load(Arc::clone(&app.shared.keymaps));
    frame(ctx, app);
    crate::keymap::load(Arc::clone(&app.shared.keymaps));
    let input = egui::RawInput { events: vec![event(true), event(false)], ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
    frame(ctx, app);
}

#[test]
fn a_command_that_keys_from_press_keys_reach_runs_as_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    registry.register(SeenBy(Arc::clone(&seen))).unwrap();
    let mut app = IdeApp::new(product(), registry, Some(dir.path().join("recent.json")), vec![project]);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    ask(&ctx, &mut app, "set_keymap", json!({ "action": "test.seen.by", "keystrokes": ["⌃⌥⌘F10"] }));
    for _ in 0..40 {
        press_bound(&ctx, &mut app, json!(["⌃⌥⌘F10"]));
        if !seen.lock().unwrap().is_empty() {
            break;
        }
    }
    assert_eq!(seen.lock().unwrap().first(), Some(&true), "keys from press_keys count as the agent");
    for _ in 0..3 {
        frame(&ctx, &mut app);
    }
    seen.lock().unwrap().clear();
    for _ in 0..40 {
        person_presses(&ctx, &mut app, egui::Key::F10);
        if !seen.lock().unwrap().is_empty() {
            break;
        }
    }
    assert_eq!(seen.lock().unwrap().first(), Some(&false), "the person's own key is the person's");
}

#[test]
fn a_person_only_command_bound_to_a_key_stays_person_only_through_press_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), r#"{"mcpRedactFiles": ["keep/**"]}"#);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let keys = json!({ "action": "settings.ide.restoreDefaults", "keystrokes": ["⌃⌥⌘F11"] });
    ask(&ctx, &mut app, "set_keymap", keys);
    let settings = dir.path().join("settings.json");
    for _ in 0..8 {
        press_bound(&ctx, &mut app, json!(["⌃⌥⌘F11"]));
        assert!(settings.exists(), "the agent's key removed the settings");
    }
    for _ in 0..3 {
        frame(&ctx, &mut app);
    }
    for _ in 0..40 {
        person_presses(&ctx, &mut app, egui::Key::F11);
        if !settings.exists() {
            break;
        }
    }
    assert!(!settings.exists(), "the same key is bound and works for the person, so the agent's reached it");
}

#[test]
fn a_setting_changed_in_a_frame_of_an_agent_s_keys_passes_what_set_settings_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = maintenance_app(dir.path(), r#"{"mcpRedactFiles": ["keep/**"], "ligatures": false}"#);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let change = |app: &mut IdeApp, change: fn(&mut IdeSettings)| {
        let mut settings = app.shared.overview().settings;
        change(&mut settings);
        let slot = app.slot_for(None).unwrap();
        slot.window.lock().unwrap().view.requests.push(Request::Settings(Box::new(settings)));
    };
    change(&mut app, |settings| settings.mcp_redact_files = Vec::new());
    ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] }));
    frame(&ctx, &mut app);
    assert_eq!(app.shared.overview().settings.mcp_redact_files, ["keep/**"], "refused as set_settings refuses it");
    change(&mut app, |settings| settings.ligatures = true);
    ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] }));
    frame(&ctx, &mut app);
    assert!(app.shared.overview().settings.ligatures, "what set_settings allows goes through");
    frame(&ctx, &mut app);
    change(&mut app, |settings| settings.mcp_redact_files = Vec::new());
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert!(app.shared.overview().settings.mcp_redact_files.is_empty(), "the person's own change");
}

use std::sync::mpsc;

use serde_json::{Value, json};

use super::*;
use crate::ide::macro_dialogs::step_title;
use crate::ide::main_menu::{Entry, MACROS};

fn frame(ctx: &egui::Context, app: &mut IdeApp) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
}

fn ask(ctx: &egui::Context, app: &mut IdeApp, tool: &str, arguments: Value) -> Result<Value, String> {
    let (sender, answer) = mpsc::channel();
    app.route(
        ctx,
        tool,
        &arguments,
        Box::new(move |result| {
            let _ = sender.send(result);
        }),
    );
    for _ in 0..5 {
        if let Ok(result) = answer.try_recv() {
            return result;
        }
        frame(ctx, app);
    }
    answer.try_recv().unwrap_or_else(|_| Err("no answer".to_owned()))
}

fn command(ctx: &egui::Context, app: &mut IdeApp, id: &str, params: Value) -> Value {
    ask(ctx, app, "dispatch_command", json!({ "command": id, "params": params }))
        .unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn action(ctx: &egui::Context, app: &mut IdeApp, id: &str) -> Value {
    own_keymap(app);
    ask(ctx, app, "run_action", json!({ "action": id })).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn keys(ctx: &egui::Context, app: &mut IdeApp, keys: &[&str], text: Option<&str>) -> Value {
    own_keymap(app);
    ask(ctx, app, "press_keys", json!({ "keys": keys, "text": text })).unwrap_or_else(|e| panic!("{keys:?}: {e}"))
}

fn person_keys(ctx: &egui::Context, app: &mut IdeApp, keys: &[&str], text: Option<&str>) -> Value {
    own_keymap(app);
    let mut events = Vec::new();
    for key in keys {
        let stroke = zen_core::keymap::Keystroke::parse(key).unwrap();
        let stroke = stroke.first();
        let modifiers = egui::Modifiers {
            alt: stroke.alt,
            ctrl: stroke.control,
            shift: stroke.shift,
            mac_cmd: stroke.command,
            command: stroke.command,
        };
        let key = *egui::Key::ALL.iter().find(|key| format!("{key:?}") == stroke.key).unwrap();
        for pressed in [true, false] {
            events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
    }
    events.extend(text.map(|text| egui::Event::Text(text.to_owned())));
    let input = egui::RawInput { events, ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
    frame(ctx, app);
    json!({ "window": ask(ctx, app, "get_window_state", json!({})).unwrap() })
}

fn own_keymap(app: &IdeApp) {
    crate::keymap::load(Arc::clone(&app.shared.keymaps));
}

fn settled<T>(app: &mut IdeApp, mut attempt: impl FnMut(&mut IdeApp) -> Option<T>) -> T {
    for _ in 0..10 {
        own_keymap(app);
        if let Some(done) = attempt(app) {
            return done;
        }
    }
    own_keymap(app);
    attempt(app).expect("the IDE's own keymap answers, once no other test has loaded its own")
}

fn text(ctx: &egui::Context, app: &mut IdeApp) -> String {
    let windows = app.shared.state().windows.clone();
    let window = windows[0].window.lock().unwrap();
    let _ = ctx;
    window.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
}

fn with_view<T>(app: &mut IdeApp, act: impl FnOnce(&mut IdeView, &Registry, &mut Session) -> T) -> T {
    let windows = app.shared.state().windows.clone();
    let mut window = windows[0].window.lock().unwrap();
    let window = &mut *window;
    act(&mut window.view, &app.shared.registry, &mut window.session)
}

fn product() -> IdeProduct {
    IdeProduct {
        folding_options: Vec::new(),
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
    }
}

struct Fixture {
    data: tempfile::TempDir,
    first: PathBuf,
    second: PathBuf,
    project: PathBuf,
}

fn fixture() -> Fixture {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let data = tempfile::tempdir().unwrap();
    let project = data.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let first = project.join("first.txt");
    let second = project.join("second.txt");
    std::fs::write(&first, "alpha\nbeta\n").unwrap();
    std::fs::write(&second, "one\ntwo\n").unwrap();
    Fixture { data, first, second, project }
}

fn app_for(fixture: &Fixture) -> IdeApp {
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    IdeApp::new(product(), registry, Some(fixture.data.path().join("recent.json")), vec![fixture.project.clone()])
}

fn record(ctx: &egui::Context, app: &mut IdeApp, fixture: &Fixture) {
    command(ctx, app, "file.document.open", json!({ "path": fixture.first }));
    let started = action(ctx, app, "view.macro.toggleRecording");
    assert_eq!(started["result"]["recording"], true, "{started}");
    person_keys(ctx, app, &["⌘→"], None);
    person_keys(ctx, app, &[], Some("// ok"));
    person_keys(ctx, app, &["⌘D"], None);
    person_keys(ctx, app, &["↓"], None);
    person_keys(ctx, app, &["⌘S"], None);
    person_keys(ctx, app, &["⇧⌘A"], None);
    person_keys(ctx, app, &["⎋"], None);
    command(ctx, app, "editor.selection.get", json!({}));
}

#[test]
fn a_macro_is_recorded_from_keys_named_played_in_another_file_undone_and_bound_to_a_key() {
    let fixture = fixture();
    let mut app = app_for(&fixture);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    record(&ctx, &mut app, &fixture);

    let state = command(&ctx, &mut app, "editor.macro.list", json!({}));
    let recorded: Vec<(String, Value)> = state["recorded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| (step["command"].as_str().unwrap().to_owned(), step["params"].clone()))
        .collect();
    assert_eq!(
        recorded,
        [
            (
                "editor.caret.move".to_owned(),
                json!({ "movement": { "kind": "lineEnd" }, "extend": false, "visual": true })
            ),
            ("editor.edit.type".to_owned(), json!({ "text": "// ok" })),
            ("editor.line.duplicate".to_owned(), json!({})),
            (
                "editor.caret.move".to_owned(),
                json!({ "movement": { "kind": "down" }, "extend": false, "visual": true })
            ),
            ("file.document.saveAll".to_owned(), json!({})),
        ],
        "keys, typing, a line action and Save are recorded; Find Action and queries are not: {state}"
    );
    assert_eq!(std::fs::read_to_string(&fixture.first).unwrap(), "alpha// ok\nalpha// ok\nbeta\n");

    let window = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
    assert_eq!(window["popups"], json!([]), "{window}");
    let stop = action(&ctx, &mut app, "view.macro.toggleRecording");
    assert_eq!(stop["window"]["popups"], json!(["macroName"]), "Stop asks for the name: {stop}");
    let named = keys(&ctx, &mut app, &[], Some("Comment line"));
    assert_eq!(named["window"]["popups"], json!(["macroName"]), "{named}");
    let saved = keys(&ctx, &mut app, &["⏎"], None);
    assert_eq!(saved["window"]["popups"], json!([]), "{saved}");
    let state = command(&ctx, &mut app, "editor.macro.list", json!({}));
    assert_eq!(state["recording"], false);
    assert_eq!(state["macros"][0]["name"], "Comment line", "{state}");
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.data.path().join("macros.json")).unwrap()).unwrap();
    assert_eq!(file["macros"][0]["steps"].as_array().unwrap().len(), 5, "saved per IDE: {file}");

    command(&ctx, &mut app, "file.document.open", json!({ "path": fixture.second }));
    command(&ctx, &mut app, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let played = settled(&mut app, |app| {
        ask(&ctx, app, "run_action", json!({ "action": "editor.macro.play/Comment line" })).ok()
    });
    assert_eq!(played["result"], json!({ "name": "Comment line", "steps": 5 }), "{played}");
    assert_eq!(text(&ctx, &mut app), "one// ok\none// ok\ntwo\n");
    assert_eq!(std::fs::read_to_string(&fixture.second).unwrap(), "one// ok\none// ok\ntwo\n");
    command(&ctx, &mut app, "editor.edit.undo", json!({}));
    assert_eq!(text(&ctx, &mut app), "one\ntwo\n", "one undo takes the playback back");

    let entry = settled(&mut app, |app| {
        let found = ask(&ctx, app, "find_action", json!({ "query": "comment line", "limit": 20 })).unwrap();
        found["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["action"] == "editor.macro.play/Comment line")
            .cloned()
    });
    assert_eq!(entry["group"], "Macros", "{entry}");
    for (query, id) in [
        ("play back last macro", "editor.macro.playLast"),
        ("start macro recording", "view.macro.toggleRecording"),
        ("edit macros", "view.macro.edit"),
        ("play saved macros", "view.macro.playSaved"),
    ] {
        let found = settled(&mut app, |app| {
            let found = ask(&ctx, app, "find_action", json!({ "query": query, "limit": 5 })).unwrap();
            (found["actions"][0]["action"] == id).then_some(found)
        });
        assert!(found["actions"][0].get("shortcut").is_none_or(Value::is_null), "{query}: {found}");
    }

    let set =
        ask(&ctx, &mut app, "set_keymap", json!({ "action": "editor.macro.play/Comment line", "keystrokes": ["⌃⌥C"] }))
            .unwrap();
    assert_eq!(set["keystrokes"], json!(["⌃⌥C"]), "{set}");
    let keymap = ask(&ctx, &mut app, "get_keymap", json!({ "query": "Comment line" })).unwrap();
    assert!(keymap.to_string().contains("editor.macro.play/Comment line"), "{keymap}");
    settled(&mut app, |app| {
        command(&ctx, app, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
        keys(&ctx, app, &["⌃⌥C"], None);
        let now = text(&ctx, app);
        assert!(now == "one\ntwo\n" || now == "one// ok\none// ok\ntwo\n", "{now:?}");
        (now != "one\ntwo\n").then_some(())
    });
    command(&ctx, &mut app, "editor.edit.undo", json!({}));

    command(&ctx, &mut app, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let last = action(&ctx, &mut app, "editor.macro.playLast");
    assert_eq!(last["result"]["name"], "Comment line", "{last}");
    assert_eq!(text(&ctx, &mut app), "one// ok\none// ok\ntwo\n");
    command(&ctx, &mut app, "editor.edit.undo", json!({}));
    assert_eq!(text(&ctx, &mut app), "one\ntwo\n");
}

#[test]
fn edit_macros_renames_and_removes_and_play_saved_macros_plays_the_chosen_one() {
    let fixture = fixture();
    let mut app = app_for(&fixture);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    record(&ctx, &mut app, &fixture);
    command(&ctx, &mut app, "editor.macro.stopRecording", json!({ "name": "First" }));
    record(&ctx, &mut app, &fixture);
    let taken = ask(
        &ctx,
        &mut app,
        "run_action",
        json!({ "action": "view.macro.toggleRecording", "params": { "name": "First" } }),
    );
    assert!(taken.is_err_and(|error| error.contains("First")), "names are unique");
    command(&ctx, &mut app, "editor.macro.stopRecording", json!({ "name": "Second" }));
    record(&ctx, &mut app, &fixture);
    let stop = action(&ctx, &mut app, "view.macro.toggleRecording");
    assert_eq!(stop["window"]["popups"], json!(["macroName"]));
    let kept = keys(&ctx, &mut app, &["⏎"], None);
    assert_eq!(kept["window"]["popups"], json!([]), "an empty name is accepted: {kept}");
    let state = command(&ctx, &mut app, "editor.macro.list", json!({}));
    assert_eq!(state["last"]["name"], "", "kept as the temporary last macro: {state}");
    assert_eq!(state["macros"].as_array().unwrap().len(), 2);

    let opened = action(&ctx, &mut app, "view.macro.edit");
    assert_eq!(opened["window"]["popups"], json!(["editMacros"]), "{opened}");
    let steps = with_view(&mut app, |view, registry, _| {
        view.macro_dialogs.edit_select(0, Some(0));
        view.macro_dialogs.edit_remove_step();
        assert!(view.macro_dialogs.edit_rename("Second").is_err(), "names stay unique");
        view.macro_dialogs.edit_rename("Comment and copy").unwrap();
        view.macro_dialogs.edit_select(1, None);
        view.macro_dialogs.edit_remove();
        let step = zen_core::macros::MacroStep::new(
            "editor.caret.move",
            json!({ "movement": { "kind": "lineEnd" }, "extend": false }),
        );
        (view.macro_dialogs.edit_names(), step_title(&step, registry))
    });
    assert_eq!(steps.0, ["Comment and copy"]);
    assert_eq!(steps.1, crate::ide::keymap_page::action_title("editor.caret.move/lineEnd"));
    let unchanged = command(&ctx, &mut app, "editor.macro.list", json!({}));
    assert_eq!(unchanged["macros"].as_array().unwrap().len(), 2, "nothing changes before OK");
    with_view(&mut app, |view, registry, session| view.macro_edit_ok(&ctx, registry, session));
    let edited = command(&ctx, &mut app, "editor.macro.list", json!({}));
    assert_eq!(edited["macros"].as_array().unwrap().len(), 1, "{edited}");
    assert_eq!(edited["macros"][0]["name"], "Comment and copy");
    assert_eq!(edited["macros"][0]["steps"].as_array().unwrap().len(), 4);

    command(&ctx, &mut app, "file.document.open", json!({ "path": fixture.second }));
    command(&ctx, &mut app, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let popup = action(&ctx, &mut app, "view.macro.playSaved");
    assert_eq!(popup["window"]["popups"], json!(["playSavedMacros"]), "{popup}");
    let played = keys(&ctx, &mut app, &["⏎"], None);
    assert_eq!(played["window"]["popups"], json!([]), "{played}");
    assert_eq!(text(&ctx, &mut app), "// okone\n// okone\ntwo\n", "without its first step the caret stays at 0");
}

#[test]
fn a_recording_keeps_what_its_starter_does_and_not_what_others_dispatch() {
    let fixture = fixture();
    let mut app = app_for(&fixture);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    command(&ctx, &mut app, "file.document.open", json!({ "path": fixture.first }));
    command(&ctx, &mut app, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let started = action(&ctx, &mut app, "view.macro.toggleRecording");
    assert_eq!(started["result"]["recording"], true, "{started}");
    person_keys(&ctx, &mut app, &[], Some("ab"));
    command(&ctx, &mut app, "editor.edit.type", json!({ "text": "[agent]" }));
    command(&ctx, &mut app, "editor.line.duplicate", json!({}));
    person_keys(&ctx, &mut app, &[], Some("c"));
    assert_eq!(text(&ctx, &mut app), "ab[agent]alpha\nab[agent]calpha\nbeta\n");
    let state = command(&ctx, &mut app, "editor.macro.list", json!({}));
    assert_eq!(
        state["recorded"],
        json!([{ "command": "editor.edit.type", "params": { "text": "abc" } }]),
        "an agent's commands stay out of a recording started in the window: {state}"
    );
    action_with(&ctx, &mut app, "view.macro.toggleRecording", json!({ "name": "Person" }));

    command(&ctx, &mut app, "editor.macro.startRecording", json!({}));
    person_keys(&ctx, &mut app, &[], Some("xy"));
    command(&ctx, &mut app, "editor.edit.type", json!({ "text": "(" }));
    person_keys(&ctx, &mut app, &["⌘D"], None);
    let stopped = command(&ctx, &mut app, "editor.macro.stopRecording", json!({ "name": "Agent" }));
    assert_eq!(
        stopped["macros"][1]["steps"],
        json!([{ "command": "editor.edit.type", "params": { "text": "(" } }]),
        "the window's keys stay out of a recording an agent started: {stopped}"
    );
}

#[test]
fn a_damaged_macros_file_is_kept_aside_and_the_window_says_so() {
    let fixture = fixture();
    let file = fixture.data.path().join("macros.json");
    std::fs::write(&file, "{ \"formatVersion\": 1, \"macros\": [").unwrap();
    let app = app_for(&fixture);
    let notice = app.pending_notice.clone().unwrap_or_default();
    assert!(notice.contains("macros.json.corrupt"), "{notice}");
    assert!(fixture.data.path().join("macros.json.corrupt").exists());
    assert!(app.shared.macros.list().is_empty());
}

fn action_with(ctx: &egui::Context, app: &mut IdeApp, id: &str, params: Value) -> Value {
    own_keymap(app);
    ask(ctx, app, "run_action", json!({ "action": id, "params": params })).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn menu_preview(ui: &mut Ui, view: &IdeView, session: &Session, registry: &Registry) {
    let p = view.palette;
    let keymap = crate::keymap::current();
    egui::Area::new(ui.scope_id().with("macros-menu-preview")).fixed_pos(egui::pos2(24.0, 24.0)).show(ui.ctx(), |ui| {
        p.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_min_width(260.0);
            ui.label(
                egui::RichText::new(format!("{} › {}", zen_i18n::t!("menu-edit"), zen_i18n::t!("menu-edit-macros")))
                    .font(crate::theme::semibold(12.0))
                    .color(p.text_secondary),
            );
            ui.separator();
            for entry in MACROS {
                let Entry::Action(id) = entry else { continue };
                let binding = keymap.bindings().into_iter().find(|binding| binding.action == *id).unwrap();
                let enabled = binding.command.starts_with("view.") || registry.is_available(session, &binding.command);
                let title = view.macro_title(id, session).unwrap_or_else(|| crate::ide::keymap_page::action_title(id));
                let color = if enabled { p.text } else { p.text_secondary };
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    ui.label(egui::RichText::new(title).font(crate::theme::regular(13.0)).color(color));
                    let keys = binding.keystrokes.first().map(ToString::to_string).unwrap_or_default();
                    ui.label(egui::RichText::new(keys).color(p.text_secondary));
                });
            }
        });
    });
}

#[test]
fn snapshots_of_the_macros_menu_the_status_mark_and_the_dialogs() {
    let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) else { return };
    let fixture = fixture();
    let mut app = app_for(&fixture);
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0);
    let ctx = screen.context().clone();
    screen.step(Vec::new(), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    command(&ctx, &mut app, "file.document.open", json!({ "path": fixture.first }));
    command(&ctx, &mut app, "editor.edit.type", json!({ "text": "x" }));
    command(&ctx, &mut app, "editor.edit.undo", json!({}));
    let mut draw = |app: &mut IdeApp, name: &str, preview: bool| {
        let image = screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
            if preview {
                let windows = app.shared.state().windows.clone();
                let window = windows[0].window.lock().unwrap();
                menu_preview(ui, &window.view, &window.session, &app.shared.registry);
            }
        });
        crate::offscreen::write_png(&dir.join(format!("{name}.png")), &image).unwrap();
    };
    for language in ["en", "ru"] {
        zen_i18n::set_language(Some(language));
        draw(&mut app, &format!("macros-menu-idle-{language}"), true);
        action(&ctx, &mut app, "view.macro.toggleRecording");
        with_view(&mut app, |_, registry, session| {
            registry.dispatch(session, "editor.edit.type", json!({ "text": "ok" })).unwrap();
            registry.dispatch(session, "editor.line.duplicate", json!({})).unwrap();
        });
        draw(&mut app, &format!("macros-menu-recording-{language}"), true);
        draw(&mut app, &format!("macros-status-recording-{language}"), false);
        action(&ctx, &mut app, "view.macro.toggleRecording");
        draw(&mut app, &format!("macros-enter-name-{language}"), false);
        keys(&ctx, &mut app, &[], Some(&format!("Wrap {language}")));
        keys(&ctx, &mut app, &["⏎"], None);
        action(&ctx, &mut app, "view.macro.edit");
        with_view(&mut app, |view, _, _| view.macro_dialogs.edit_select(0, Some(1)));
        draw(&mut app, &format!("macros-edit-{language}"), false);
        with_view(&mut app, |view, _, _| view.macro_dialogs.edit_open_rename());
        draw(&mut app, &format!("macros-edit-rename-{language}"), false);
        keys(&ctx, &mut app, &["⎋"], None);
        keys(&ctx, &mut app, &["⎋"], None);
        action(&ctx, &mut app, "view.macro.playSaved");
        draw(&mut app, &format!("macros-play-saved-{language}"), false);
        keys(&ctx, &mut app, &["⎋"], None);
        draw(&mut app, &format!("macros-menu-saved-{language}"), true);
        command(&ctx, &mut app, "editor.macro.remove", json!({ "name": format!("Wrap {language}") }));
    }
    zen_i18n::set_language(Some("en"));
}

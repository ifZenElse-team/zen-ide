use std::collections::BTreeMap;

use serde_json::{Value, json};
use zen_core::keymap::Keystroke;

use super::*;
use crate::keymaps::KeymapStore;

struct Harness {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        session.set_state(Macros::load(Some(dir.path().join("data/macros.json"))));
        Self { registry, session, dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn open(&mut self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        self.run("file.document.open", json!({ "path": path }));
        path
    }

    fn text(&self) -> String {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn record_wrap(&mut self) {
        self.run("editor.macro.startRecording", json!({}));
        self.run("editor.caret.move", json!({ "movement": { "kind": "lineEnd" }, "extend": false }));
        for text in ["/", "/", " ", "o", "k"] {
            self.run("editor.edit.type", json!({ "text": text }));
        }
        self.run("editor.line.duplicate", json!({}));
        self.run("editor.caret.move", json!({ "movement": { "kind": "down" }, "extend": false }));
        self.run("file.document.saveAll", json!({}));
    }
}

#[test]
fn a_recording_is_saved_by_name_and_played_back_in_another_file_as_one_undo_step() {
    let mut h = Harness::new();
    let first = h.open("first.txt", "one\ntwo\n");
    h.record_wrap();
    h.run("editor.selection.get", json!({}));
    h.run("editor.fold.list", json!({}));
    let state = h.run("editor.macro.list", json!({}));
    assert_eq!(state["recording"], true);
    let commands: Vec<&str> =
        state["recorded"].as_array().unwrap().iter().map(|step| step["command"].as_str().unwrap()).collect();
    assert_eq!(
        commands,
        [
            "editor.caret.move",
            "editor.edit.type",
            "editor.edit.type",
            "editor.edit.type",
            "editor.line.duplicate",
            "editor.caret.move",
            "file.document.saveAll"
        ],
        "queries are not recorded and plain typing is one step: {state}"
    );
    assert_eq!(state["recorded"][3]["params"], json!({ "text": " ok" }));
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "one// ok\none// ok\ntwo\n", "Save ran while recording");

    let stopped = h.run("editor.macro.stopRecording", json!({ "name": "Comment and copy" }));
    assert_eq!(stopped["recording"], false);
    assert_eq!(stopped["macros"][0]["name"], "Comment and copy");
    assert_eq!(stopped["last"]["name"], "Comment and copy");

    let second = h.open("second.txt", "alpha\nbeta\n");
    h.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let played = h.run("editor.macro.play", json!({ "name": "Comment and copy" }));
    assert_eq!(played, json!({ "name": "Comment and copy", "steps": 7 }));
    assert_eq!(h.text(), "alpha// ok\nalpha// ok\nbeta\n");
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "alpha// ok\nalpha// ok\nbeta\n", "the macro saved");
    assert!(h.run("editor.macro.list", json!({}))["recording"] == false, "playback records nothing");

    h.run("editor.edit.undo", json!({}));
    assert_eq!(h.text(), "alpha\nbeta\n", "one undo takes the whole playback back");
    h.run("editor.edit.redo", json!({}));
    assert_eq!(h.text(), "alpha// ok\nalpha// ok\nbeta\n");

    h.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    h.run("editor.macro.playLast", json!({}));
    assert_eq!(h.text(), "alpha// ok// ok\nalpha// ok// ok\nalpha// ok\nbeta\n");
    h.run("editor.edit.undo", json!({}));
    assert_eq!(h.text(), "alpha// ok\nalpha// ok\nbeta\n");
}

#[test]
fn an_empty_name_keeps_the_temporary_last_macro_and_names_are_unique() {
    let mut h = Harness::new();
    h.open("a.txt", "x\n");
    h.record_wrap();
    h.run("editor.macro.stopRecording", json!({ "name": "Wrap" }));
    h.record_wrap();
    let taken = h.registry.dispatch(&mut h.session, "editor.macro.stopRecording", json!({ "name": " Wrap " }));
    let Err(zen_core::DispatchError::Failed { error, .. }) = taken else { panic!("{taken:?}") };
    assert!(error.message.contains("workbench-macro-exists") || error.message.contains("Wrap"), "{error:?}");
    assert_eq!(h.run("editor.macro.list", json!({}))["recording"], true, "the recording goes on");

    let kept = h.run("editor.macro.stopRecording", json!({ "name": "" }));
    assert_eq!(kept["last"]["name"], "");
    assert_eq!(kept["macros"].as_array().unwrap().len(), 1, "the temporary macro is not saved");
    assert_eq!(kept["last"]["steps"].as_array().unwrap().len(), 7);

    h.run("editor.macro.startRecording", json!({}));
    let empty = h.run("editor.macro.stopRecording", json!({}));
    assert_eq!(empty["last"]["name"], "", "a recording without steps changes nothing");
    assert_eq!(empty["last"]["steps"].as_array().unwrap().len(), 7);

    assert!(!h.registry.is_available(&h.session, "editor.macro.stopRecording"));
    h.run("editor.macro.startRecording", json!({}));
    assert!(!h.registry.is_available(&h.session, "editor.macro.startRecording"));
    assert!(!h.registry.is_available(&h.session, "editor.macro.playLast"), "no playback while recording");
    assert!(!h.registry.is_available(&h.session, "editor.macro.play"));
}

#[test]
fn edit_macros_renames_removes_and_drops_steps_and_the_file_keeps_them() {
    let mut h = Harness::new();
    h.open("a.txt", "x\n");
    h.record_wrap();
    h.run("editor.macro.stopRecording", json!({ "name": "One" }));
    h.record_wrap();
    h.run("editor.macro.stopRecording", json!({ "name": "Two" }));

    let renamed = h.run("editor.macro.rename", json!({ "name": "One", "newName": "First" }));
    let names: Vec<&str> = renamed["macros"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["First", "Two"]);
    assert!(
        h.registry
            .dispatch(&mut h.session, "editor.macro.rename", json!({ "name": "First", "newName": "Two" }))
            .is_err()
    );
    assert!(
        h.registry.dispatch(&mut h.session, "editor.macro.rename", json!({ "name": "First", "newName": " " })).is_err()
    );

    let removed = h.run("editor.macro.removeStep", json!({ "name": "Two", "index": 0 }));
    assert_eq!(removed["macros"][1]["steps"].as_array().unwrap().len(), 6);
    assert_eq!(removed["last"]["steps"].as_array().unwrap().len(), 6, "the last macro is the one changed");
    assert!(
        h.registry.dispatch(&mut h.session, "editor.macro.removeStep", json!({ "name": "Two", "index": 6 })).is_err()
    );

    h.run("editor.macro.remove", json!({ "name": "First" }));
    assert!(h.registry.dispatch(&mut h.session, "editor.macro.play", json!({ "name": "First" })).is_err());

    let file = h.dir.path().join("data/macros.json");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["formatVersion"], 1);
    assert_eq!(saved["macros"][0]["name"], "Two");
    assert_eq!(saved["macros"][0]["steps"][0]["command"], "editor.edit.type");
    let again = Macros::load(Some(file));
    assert_eq!(again.list().len(), 1);
    assert_eq!(again.list()[0].steps.len(), 6);
    assert!(again.last().is_none(), "the last macro lives as long as the IDE runs");
}

#[test]
fn saved_macros_are_keymap_actions_whose_shortcuts_follow_a_rename() {
    let mut h = Harness::new();
    let keymaps: SharedKeymaps = Arc::new(Mutex::new(KeymapStore::new(Some(h.dir.path().join("data")), Vec::new())));
    let store = h.session.state::<Macros>().unwrap().clone();
    store.set_keymaps(Arc::clone(&keymaps));
    h.open("a.txt", "x\n");
    h.record_wrap();
    h.run("editor.macro.stopRecording", json!({ "name": "Wrap" }));
    let action = |keymaps: &SharedKeymaps, name: &str| {
        keymaps::lock(keymaps).active().bindings().into_iter().find(|binding| binding.action == macro_action(name))
    };
    let binding = action(&keymaps, "Wrap").expect("the saved macro is an action of the keymap");
    assert_eq!((binding.command.as_str(), &binding.params), ("editor.macro.play", &json!({ "name": "Wrap" })));
    assert!(binding.keystrokes.is_empty());

    let key = Keystroke::parse("⌃⌥W").unwrap();
    keymaps::lock(&keymaps).set_changes(BTreeMap::from([(macro_action("Wrap"), vec![key.clone()])])).unwrap();
    h.run("editor.macro.rename", json!({ "name": "Wrap", "newName": "Wrap line" }));
    assert!(action(&keymaps, "Wrap").is_none());
    assert_eq!(action(&keymaps, "Wrap line").unwrap().keystrokes, vec![key.clone()], "the shortcut goes with it");
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(h.dir.path().join("data/keymap.json")).unwrap()).unwrap();
    assert_eq!(saved, json!({ "editor.macro.play/Wrap line": ["⌃⌥W"] }));

    h.run("editor.macro.remove", json!({ "name": "Wrap line" }));
    assert!(action(&keymaps, "Wrap line").is_none());
    assert!(keymaps::lock(&keymaps).active().user_changes().is_empty(), "and goes with a removed macro");
}

#[test]
fn a_failing_step_stops_the_playback_and_says_which() {
    let mut h = Harness::new();
    h.open("a.txt", "x\n");
    let store = h.session.state::<Macros>().unwrap().clone();
    let steps = vec![
        MacroStep::new("editor.edit.type", json!({ "text": "a" })),
        MacroStep::new("editor.edit.vanished", json!({})),
        MacroStep::new("editor.edit.type", json!({ "text": "c" })),
    ];
    store.keep("Breaks", steps).unwrap();
    let failed = h.registry.dispatch(&mut h.session, "editor.macro.play", json!({ "name": "Breaks" }));
    let Err(zen_core::DispatchError::Failed { error, .. }) = failed else { panic!("{failed:?}") };
    assert_eq!(error.code.as_deref(), Some(zen_core::macros::STEP_FAILED));
    let data = error.data.unwrap();
    assert_eq!((&data["step"], &data["command"]), (&json!(1), &json!("editor.edit.vanished")));
    assert_eq!(h.text(), "ax\n", "the steps before it ran, the ones after it did not");
}

#[test]
fn a_macro_that_names_what_it_cannot_play_is_refused_before_anything_runs() {
    let mut h = Harness::new();
    h.open("a.txt", "x\n");
    let store = h.session.state::<Macros>().unwrap().clone();
    for (name, command) in
        [("Itself", "editor.macro.play"), ("Window", "view.settings.open"), ("Open", "file.document.open")]
    {
        let steps = vec![
            MacroStep::new("editor.edit.type", json!({ "text": "a" })),
            MacroStep::new(command, json!({ "name": name })),
        ];
        store.keep(name, steps).unwrap();
        let refused = h.registry.dispatch(&mut h.session, "editor.macro.play", json!({ "name": name }));
        let Err(zen_core::DispatchError::Failed { error, .. }) = refused else { panic!("{refused:?}") };
        assert_eq!(error.code.as_deref(), Some(zen_core::macros::STEP_FAILED));
        assert_eq!(error.data.unwrap()["step"], json!(1));
        assert_eq!(h.text(), "x\n", "{name}: nothing ran");
        assert!(h.session.state::<Recorder>().is_none_or(|recorder| !recorder.is_playing()));
    }
}

#[test]
fn a_playback_after_typing_is_an_undo_step_of_its_own() {
    let mut h = Harness::new();
    h.open("a.txt", "\n");
    h.run("editor.macro.startRecording", json!({}));
    h.run("editor.edit.type", json!({ "text": "!" }));
    h.run("editor.macro.stopRecording", json!({ "name": "Bang" }));
    h.run("editor.edit.undo", json!({}));
    h.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    h.run("editor.edit.type", json!({ "text": "a" }));
    h.run("editor.macro.play", json!({ "name": "Bang" }));
    assert_eq!(h.text(), "a!\n");
    h.run("editor.edit.type", json!({ "text": "b" }));
    h.run("editor.edit.undo", json!({}));
    assert_eq!(h.text(), "a!\n", "typing after the playback is a step of its own");
    h.run("editor.edit.undo", json!({}));
    assert_eq!(h.text(), "a\n", "one undo takes the playback back, not the typing before it");
    h.run("editor.edit.undo", json!({}));
    assert_eq!(h.text(), "\n");
}

#[test]
fn a_playback_in_a_read_only_file_changes_nothing_and_ends() {
    let mut h = Harness::new();
    h.open("a.txt", "x\n");
    let store = h.session.state::<Macros>().unwrap().clone();
    store.keep("Edit", vec![MacroStep::new("editor.edit.type", json!({ "text": "a" }))]).unwrap();
    let path = h.dir.path().join("latin1.txt");
    std::fs::write(&path, b"caf\xe9\n").unwrap();
    h.run("file.document.open", json!({ "path": path }));
    let document = h.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(document.read_only().is_some(), "the IDE opens it read-only");
    let before = h.text();
    let _ = h.registry.dispatch(&mut h.session, "editor.macro.play", json!({ "name": "Edit" }));
    assert_eq!(h.text(), before);
    assert_eq!(std::fs::read(&path).unwrap(), b"caf\xe9\n");
    assert!(!h.session.state::<Recorder>().unwrap().is_playing());
    assert!(h.registry.is_available(&h.session, "editor.macro.play"), "the next playback can start");
}

#[test]
fn a_long_macro_plays_as_one_undo_step() {
    let mut h = Harness::new();
    h.open("a.txt", "\n");
    let store = h.session.state::<Macros>().unwrap().clone();
    let steps: Vec<MacroStep> = (0..2000)
        .flat_map(|index| {
            [
                MacroStep::new("editor.edit.insert", json!({ "text": format!("line {index}") })),
                MacroStep::new("editor.edit.newline", json!({})),
            ]
        })
        .collect();
    store.keep("Long", steps).unwrap();
    h.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let played = h.run("editor.macro.play", json!({ "name": "Long" }));
    assert_eq!(played["steps"], 4000);
    assert_eq!(h.text().lines().count(), 2001);
    assert!(h.text().starts_with("line 0\nline 1\n"));
    h.run("editor.edit.undo", json!({}));
    assert_eq!(h.text(), "\n");
}

#[test]
fn a_damaged_file_is_kept_aside_and_the_next_save_starts_afresh() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("macros.json");
    std::fs::write(&file, "{ \"formatVersion\": 1, \"macros\": [ { \"name\": ").unwrap();
    let store = Macros::load(Some(file.clone()));
    assert!(store.list().is_empty());
    assert!(store.take_notice().is_some(), "a notice says so");
    assert!(store.take_notice().is_none(), "once");
    let kept = dir.path().join("macros.json.corrupt");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "{ \"formatVersion\": 1, \"macros\": [ { \"name\": ");
    assert!(!file.exists());
    store.keep("Fresh", vec![MacroStep::new("editor.edit.type", json!({ "text": "a" }))]).unwrap();
    assert_eq!(Macros::load(Some(file)).list().len(), 1);
    assert!(kept.exists(), "the damaged copy stays");
}

#[test]
fn names_with_any_characters_are_actions_of_their_own() {
    let mut h = Harness::new();
    let keymaps: SharedKeymaps = Arc::new(Mutex::new(KeymapStore::new(Some(h.dir.path().join("data")), Vec::new())));
    let store = h.session.state::<Macros>().unwrap().clone();
    store.set_keymaps(Arc::clone(&keymaps));
    h.open("a.txt", "x\n");
    let names = ["a/b", "Ünïcødé ✓", "quote \" and \\", "dup+extend", "editor.macro.play/x", "  spaced  "];
    for name in names {
        h.record_wrap();
        h.run("editor.macro.stopRecording", json!({ "name": name }));
    }
    let actions: Vec<String> = keymaps::lock(&keymaps)
        .active()
        .bindings()
        .into_iter()
        .filter(|binding| binding.command == "editor.macro.play")
        .map(|binding| binding.action)
        .collect();
    let expected: Vec<String> = names.iter().map(|name| macro_action(name.trim())).collect();
    assert_eq!(actions, expected);
    let key = Keystroke::parse("⌃⌥Q").unwrap();
    keymaps::lock(&keymaps)
        .set_changes(BTreeMap::from([(macro_action("quote \" and \\"), vec![key.clone()])]))
        .unwrap();
    h.run("editor.macro.rename", json!({ "name": "quote \" and \\", "newName": "a/b/c" }));
    let found = keymaps::lock(&keymaps).active().lookup(&key, zen_core::keymap::Context::Editor).unwrap();
    assert_eq!((found.action, found.params), (macro_action("a/b/c"), json!({ "name": "a/b/c" })));
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(h.dir.path().join("data/keymap.json")).unwrap()).unwrap();
    assert_eq!(saved, json!({ "editor.macro.play/a/b/c": ["⌃⌥Q"] }));
    h.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    h.run("editor.macro.play", json!({ "name": "editor.macro.play/x" }));
    assert!(h.text().contains("// ok"));
}

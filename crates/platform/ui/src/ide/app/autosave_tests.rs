use std::time::{Duration, Instant};

use egui::Event;
use serde_json::{Value, json};
use zen_workbench::Document;

use super::tests::{ask, frame, themed_app, window_project};
#[cfg(unix)]
use super::tests::{test_shell, with_terminals};
use super::*;

struct Fixture {
    ctx: egui::Context,
    app: IdeApp,
    file: PathBuf,
    before: String,
    _data: tempfile::TempDir,
}

impl Fixture {
    fn new(seconds: u32) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let before = std::fs::read_to_string(&file).unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let mut fixture = Self { ctx, app, file, before, _data: data };
        fixture.settings(|settings| settings.autosave_idle_seconds = seconds);
        fixture
    }

    fn settings(&mut self, change: impl FnOnce(&mut IdeSettings)) {
        change(&mut self.app.shared.settings.lock().unwrap());
    }

    fn edit(&mut self, text: &str) {
        let open = json!({ "command": "file.document.open", "params": { "path": self.file } });
        ask(&self.ctx, &mut self.app, "dispatch_command", open).unwrap();
        let insert = json!({ "command": "editor.edit.insert", "params": { "text": text } });
        ask(&self.ctx, &mut self.app, "dispatch_command", insert).unwrap();
    }

    fn frame_with(&mut self, events: Vec<Event>, focused: bool) {
        let input = egui::RawInput { events, focused, ..egui::RawInput::default() };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
    }

    fn frame(&mut self) {
        frame(&self.ctx, &mut self.app);
    }

    fn later_frame(&mut self, focused: bool) {
        let time = self.ctx.input(|input| input.time) + 1.0;
        let input = egui::RawInput { time: Some(time), focused, ..egui::RawInput::default() };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
    }

    fn leave_the_ide(&mut self) {
        self.frame_with(vec![Event::WindowFocused(false)], false);
        self.later_frame(false);
        self.later_frame(false);
    }

    fn pending(&mut self) -> String {
        self.app.pending_notice.clone().unwrap_or_default()
    }

    fn disk(&self) -> String {
        std::fs::read_to_string(&self.file).unwrap()
    }

    fn dirty(&self) -> bool {
        let slot = self.app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        window.session.state::<Workbench>().unwrap().documents().iter().any(Document::is_dirty)
    }

    fn kinds(&mut self) -> Vec<String> {
        let list = json!({ "command": "localHistory.versions.list", "params": { "path": self.file } });
        let answer = ask(&self.ctx, &mut self.app, "dispatch_command", list).unwrap();
        answer["versions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|version| version["kind"].as_str().unwrap().to_owned())
            .collect()
    }
}

#[test]
fn the_idle_autosave_waits_for_quiet_and_saves_once() {
    let mut fixture = Fixture::new(1);
    fixture.edit("// idle\n");
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1600) {
        fixture.frame_with(vec![Event::PointerMoved(egui::pos2(300.0, 300.0))], true);
        std::thread::sleep(Duration::from_millis(150));
    }
    assert_eq!(fixture.disk(), fixture.before, "input keeps putting the save off");
    assert!(fixture.dirty());

    let saved = format!("// idle\n{}", fixture.before);
    let deadline = Instant::now() + Duration::from_secs(60);
    std::thread::sleep(Duration::from_millis(1000));
    fixture.frame();
    while fixture.disk() != saved && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        fixture.frame();
    }
    assert_eq!(fixture.disk(), saved, "a quiet second saves the file");
    assert!(!fixture.dirty());
    assert_eq!(fixture.kinds().iter().filter(|kind| *kind == "save").count(), 1, "one Local History version");

    std::thread::sleep(Duration::from_millis(1200));
    fixture.frame();
    assert_eq!(
        fixture.kinds().iter().filter(|kind| *kind == "save").count(),
        1,
        "no new version while nothing changes"
    );
}

#[cfg(unix)]
#[test]
fn a_file_the_idle_autosave_could_not_write_is_tried_again_after_new_input() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new(1);
    fixture.edit("// later\n");
    let folder = fixture.file.parent().unwrap().to_owned();
    let mode = |mode: u32| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode)).unwrap();
    fixture.frame();
    mode(0o555);
    std::thread::sleep(Duration::from_millis(1200));
    fixture.frame();
    mode(0o755);
    assert_eq!(fixture.disk(), fixture.before, "the folder is read-only");
    assert!(fixture.dirty());

    std::thread::sleep(Duration::from_millis(1200));
    fixture.frame();
    assert_eq!(fixture.disk(), fixture.before, "no new input and no change: not tried again yet");

    fixture.frame_with(vec![Event::PointerMoved(egui::pos2(300.0, 300.0))], true);
    std::thread::sleep(Duration::from_millis(1200));
    fixture.frame();
    assert_eq!(fixture.disk(), format!("// later\n{}", fixture.before), "the next quiet second saves it");
    assert!(!fixture.dirty());
}

#[test]
fn the_idle_autosave_follows_its_setting() {
    let mut fixture = Fixture::new(1);
    fixture.settings(|settings| settings.autosave_on_idle = false);
    fixture.edit("// kept in the editor\n");
    std::thread::sleep(Duration::from_millis(1300));
    fixture.frame();
    fixture.frame();
    assert_eq!(fixture.disk(), fixture.before, "off: nothing is saved by itself");
    assert!(fixture.dirty());
}

#[test]
fn leaving_the_window_for_another_application_saves_the_modified_files() {
    let mut fixture = Fixture::new(3600);
    fixture.edit("// left\n");
    fixture.leave_the_ide();
    assert_eq!(fixture.disk(), format!("// left\n{}", fixture.before));
    assert!(!fixture.dirty());
    assert_eq!(fixture.kinds().iter().filter(|kind| *kind == "save").count(), 1);

    fixture.frame_with(vec![Event::WindowFocused(true)], true);
    fixture.leave_the_ide();
    assert_eq!(fixture.kinds().iter().filter(|kind| *kind == "save").count(), 1, "nothing modified, no version");

    fixture.settings(|settings| settings.autosave_on_frame_deactivation = false);
    fixture.frame_with(vec![Event::WindowFocused(true)], true);
    fixture.edit("// stays\n");
    fixture.leave_the_ide();
    assert!(fixture.dirty(), "off: leaving the window saves nothing");
    assert_eq!(fixture.disk(), format!("// left\n{}", fixture.before));
}

#[test]
fn the_focus_coming_back_at_once_saves_nothing() {
    let mut fixture = Fixture::new(3600);
    fixture.edit("// typing\n");
    fixture.frame_with(vec![Event::WindowFocused(false)], false);
    fixture.frame_with(vec![Event::WindowFocused(true)], true);
    fixture.later_frame(true);
    fixture.later_frame(true);
    assert!(fixture.dirty());
}

#[test]
fn an_installed_update_restarts_after_saving_the_files() {
    let mut fixture = Fixture::new(3600);
    fixture.edit("// saved first\n");
    let before = RESTARTS_ASKED.with(std::cell::Cell::get);
    fixture.app.shared.state().requests.push(Request::UpdateInstalled("9.9.9".to_owned()));
    fixture.frame();
    assert_eq!(RESTARTS_ASKED.with(std::cell::Cell::get), before + 1);
    assert_eq!(fixture.disk(), format!("// saved first\n{}", fixture.before));
}

#[test]
fn an_update_never_restarts_over_files_it_could_not_save() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new(3600);
    fixture.edit("// unsaved\n");
    let folder = fixture.file.parent().unwrap().to_owned();
    let mode = |mode: u32| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode)).unwrap();
    let before = RESTARTS_ASKED.with(std::cell::Cell::get);
    fixture.frame();
    mode(0o555);
    fixture.app.shared.state().requests.push(Request::UpdateInstalled("9.9.9".to_owned()));
    fixture.app.logic(&fixture.ctx.clone());
    mode(0o755);
    assert_eq!(RESTARTS_ASKED.with(std::cell::Cell::get), before, "the edits are still only in the IDE");
    assert!(fixture.dirty());
    assert!(fixture.pending().contains("9.9.9"), "{}", fixture.pending());
}

#[test]
fn a_restart_the_shell_refused_tells_the_person_and_keeps_the_ide_open() {
    let mut fixture = Fixture::new(3600);
    RESTART_REFUSED.with(|refused| refused.set(true));
    fixture.app.shared.state().requests.push(Request::UpdateInstalled("9.9.9".to_owned()));
    fixture.app.logic(&fixture.ctx.clone());
    RESTART_REFUSED.with(|refused| refused.set(false));
    assert!(fixture.pending().contains("the shell cannot start"), "{}", fixture.pending());
}

#[test]
fn an_update_is_acted_on_once_for_each_version() {
    let mut asked = None;
    assert!(!newly_installed("1.0", "1.0", &mut asked), "the running version");
    assert!(newly_installed("1.1", "1.0", &mut asked));
    assert!(!newly_installed("1.1", "1.0", &mut asked), "a failed restart does not loop");
    assert!(newly_installed("1.2", "1.0", &mut asked), "a newer one is tried");
}

#[cfg(unix)]
#[test]
fn the_terminal_getting_the_keyboard_saves_the_modified_files() {
    use zen_workbench::terminal::Terminals;
    let mut fixture = Fixture::new(3600);
    let home = fixture._data.path().to_owned();
    with_terminals(&fixture.app, |window| window.session.state_mut::<Terminals>().set_launcher(test_shell(&home)));
    fixture.settings(|settings| settings.autosave_on_frame_deactivation = false);
    fixture.edit("// before the terminal\n");
    for _ in 0..3 {
        fixture.frame();
    }
    ask(&fixture.ctx, &mut fixture.app, "terminal_open", json!({})).unwrap();
    for _ in 0..3 {
        fixture.frame();
    }
    let has_keyboard =
        |fixture: &Fixture| with_terminals(&fixture.app, |window| window.view.terminal_has_keyboard(&fixture.ctx));
    assert!(has_keyboard(&fixture), "the new session has the keyboard");
    assert!(fixture.dirty(), "off: switching to the Terminal saves nothing");

    with_terminals(&fixture.app, |window| window.view.editor_view.focus());
    for _ in 0..2 {
        fixture.frame();
    }
    assert!(!has_keyboard(&fixture));
    fixture.settings(|settings| settings.autosave_on_frame_deactivation = true);
    ask(&fixture.ctx, &mut fixture.app, "terminal_open", json!({})).unwrap();
    for _ in 0..3 {
        fixture.frame();
    }
    assert!(has_keyboard(&fixture));
    assert!(!fixture.dirty(), "on: the Terminal getting the keyboard saves");
    assert_eq!(fixture.disk(), format!("// before the terminal\n{}", fixture.before));

    fixture.edit("// typed while in the terminal\n");
    for _ in 0..3 {
        fixture.frame();
    }
    assert!(fixture.dirty(), "only getting the keyboard saves, not keeping it");
}

#[test]
fn agents_read_and_change_the_autosave_settings_and_run_an_autosave() {
    let mut fixture = Fixture::new(3600);
    let settings = ask(&fixture.ctx, &mut fixture.app, "get_settings", json!({})).unwrap();
    assert_eq!(
        (&settings["autosaveOnIdle"], &settings["autosaveIdleSeconds"], &settings["autosaveOnFrameDeactivation"]),
        (&json!(true), &json!(3600), &json!(true))
    );
    let changed = json!({ "autosaveOnIdle": false, "autosaveIdleSeconds": 30, "autosaveOnFrameDeactivation": false });
    let applied = ask(&fixture.ctx, &mut fixture.app, "set_settings", changed).unwrap();
    assert_eq!(applied["autosaveIdleSeconds"], 30, "{applied}");
    fixture.frame();
    let settings = fixture.app.shared.settings.lock().unwrap().clone();
    assert!(!settings.autosave_on_idle && !settings.autosave_on_frame_deactivation);
    let refused = ask(&fixture.ctx, &mut fixture.app, "set_settings", json!({ "autosaveIdleSeconds": 0 }));
    assert!(refused.is_err(), "{refused:?}");

    fixture.edit("// by an agent\n");
    let done = ask(&fixture.ctx, &mut fixture.app, "dispatch_command", json!({ "command": "file.document.autosave" }))
        .unwrap();
    let saved: Vec<Value> = done["saved"].as_array().unwrap().clone();
    assert_eq!(saved.len(), 1, "{done}");
    assert_eq!(fixture.disk(), format!("// by an agent\n{}", fixture.before));
}

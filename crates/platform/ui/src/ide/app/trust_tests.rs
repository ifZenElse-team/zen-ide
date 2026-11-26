use egui::accesskit::Role;
use serde_json::json;
use zen_workbench::build_tools::BuildEnv;

use super::tests::{ask_on, themed_app, window_project};
use super::*;

const SIZE: egui::Vec2 = egui::vec2(1100.0, 700.0);

fn root_frame(app: &mut IdeApp, ui: &mut Ui) {
    app.logic(ui.ctx());
    if app.open_projects().is_empty() {
        app.trust_dialog(ui);
    } else {
        app.ui(ui);
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    project: PathBuf,
    app: IdeApp,
    screen: crate::offscreen::Offscreen,
    env: BuildEnv,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::Builder::new().prefix("trust").tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        window_project(&root);
        let project = root.join("project");
        let mut app = themed_app(&root, Vec::new());
        let env = BuildEnv::default();
        env.set_data_dir(Some(root.join("trust-data")));
        app.trust_env = env.clone();
        app.asks_trust = true;
        Self { _dir: dir, root, project, app, screen: crate::offscreen::Offscreen::new(SIZE, 1.0), env }
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        let app = &mut self.app;
        self.screen.step(events, |ui| root_frame(app, ui));
    }

    fn click(&mut self, role: Role, name: &str) {
        let app = &mut self.app;
        let tree = self.screen.accessibility_tree(|ui| root_frame(app, ui));
        let node = crate::accessibility::testing::find(&tree, role, name)
            .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let bounds = node.bounds().expect("the node has bounds");
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            self.step(events);
        }
    }

    fn shows(&mut self, role: Role, name: &str) -> bool {
        let app = &mut self.app;
        let tree = self.screen.accessibility_tree(|ui| root_frame(app, ui));
        crate::accessibility::testing::find(&tree, role, name).is_some()
    }

    fn open(&mut self) {
        let project = self.project.clone();
        self.app.open(None, &project);
        self.step(Vec::new());
    }

    fn opened(&self) -> Vec<PathBuf> {
        self.app.open_projects()
    }
}

fn ask_err(f: &mut Fixture, tool: &str, arguments: serde_json::Value) -> String {
    let (sender, answer) = std::sync::mpsc::channel();
    let ctx = f.screen.context().clone();
    f.app.route(
        &ctx,
        tool,
        &arguments,
        Box::new(move |result| {
            let _ = sender.send(result);
        }),
    );
    for _ in 0..5 {
        if let Ok(result) = answer.try_recv() {
            return result.expect_err("refused");
        }
        f.step(Vec::new());
    }
    panic!("no answer to {tool}")
}

#[test]
fn an_unknown_project_is_asked_about_before_it_opens_and_dont_open_leaves_nothing() {
    let mut f = Fixture::new();
    f.open();
    assert!(f.opened().is_empty(), "the question comes first");
    assert!(f.shows(Role::Button, "Trust Project"));
    assert!(f.shows(Role::Button, "Preview in Safe Mode"));
    assert!(f.shows(Role::Button, "Don't Open"));
    assert!(f.app.shared.overview().trust_question);
    f.click(Role::Button, "Don't Open");
    assert!(f.opened().is_empty());
    assert!(!f.app.shared.overview().trust_question);
    assert!(!zen_workbench::trust::is_trusted(&f.env, &f.project));
}

#[test]
fn trust_project_keeps_the_answer_and_the_next_open_does_not_ask() {
    let mut f = Fixture::new();
    f.open();
    f.click(Role::Button, "Trust Project");
    assert_eq!(f.opened(), vec![f.project.clone()]);
    assert!(zen_workbench::trust::is_trusted(&f.env, &f.project));
    assert!(std::fs::read_to_string(f.root.join("trust-data/build-tools.json")).unwrap().contains("projectTrust"));
    let close = f.project.clone();
    f.app.shared.state().requests.push(Request::Close(close));
    f.step(Vec::new());
    f.step(Vec::new());
    f.open();
    assert!(f.app.trust_questions.is_empty());
}

#[test]
fn preview_opens_in_safe_mode_for_this_time_only_without_a_second_prompt() {
    let mut f = Fixture::new();
    std::fs::write(f.project.join("pom.xml"), "<project/>").unwrap();
    f.open();
    f.click(Role::Button, "Preview in Safe Mode");
    assert_eq!(f.opened(), vec![f.project.clone()]);
    assert!(!zen_workbench::trust::is_trusted(&f.env, &f.project));
    let notices = f.app.with_session(|session| {
        session.state::<zen_workbench::notifications::Notifications>().map_or(0, |store| store.newest_first().count())
    });
    assert_eq!(notices, Some(0), "the dialog has asked already");
    assert!(f.app.trust_answered.is_empty(), "the next open asks again");
}

#[test]
fn trust_all_projects_in_the_parent_adds_a_trusted_location() {
    let mut f = Fixture::new();
    f.open();
    let parent = f.project.parent().unwrap().file_name().unwrap().to_string_lossy().into_owned();
    f.click(Role::CheckBox, &format!("Trust all projects in '{parent}'"));
    f.click(Role::Button, "Trust Project");
    assert_eq!(f.opened(), vec![f.project.clone()]);
    assert_eq!(zen_workbench::trust::locations(&f.env), vec![f.project.parent().unwrap().to_owned()]);
}

#[test]
fn an_agent_opens_in_safe_mode_without_asking_and_never_presses_the_dialog() {
    let mut f = Fixture::new();
    f.open();
    f.click(Role::Button, "Preview in Safe Mode");
    let other = f.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    ask_on(&mut f.screen, &mut f.app, "open_project", json!({ "path": other }));
    assert!(f.app.trust_questions.is_empty());
    assert_eq!(f.opened(), vec![f.project.clone(), other.clone()]);

    let third = f.root.join("third");
    std::fs::create_dir_all(&third).unwrap();
    f.app.open(None, &third);
    f.step(Vec::new());
    assert_eq!(f.app.trust_questions.len(), 1);
    let refused = ask_err(&mut f, "press_keys", json!({ "keys": ["⏎"] }));
    assert!(refused.contains("person"), "{refused}");
    let state = ask_on(&mut f.screen, &mut f.app, "get_window_state", json!({ "project": other }));
    assert!(state["popups"].as_array().unwrap().contains(&json!("trustProject")), "{state}");
    assert_eq!(state["safeMode"], true, "{state}");
}

#[test]
fn enter_does_not_answer_the_dialog() {
    let mut f = Fixture::new();
    f.open();
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    f.step(vec![enter]);
    f.step(Vec::new());
    assert!(f.opened().is_empty());
    assert!(!zen_workbench::trust::is_trusted(&f.env, &f.project));
    assert_eq!(f.app.trust_questions.len(), 1);
}

#[test]
fn the_banner_trusts_an_open_project_without_reopening_it_and_agents_cannot_ask() {
    let mut f = Fixture::new();
    f.open();
    f.click(Role::Button, "Preview in Safe Mode");
    let main = f.project.join("main.go");
    let params = json!({ "command": "file.document.open", "params": { "path": main } });
    ask_on(&mut f.screen, &mut f.app, "dispatch_command", params);
    f.step(Vec::new());
    f.step(Vec::new());
    assert!(f.shows(Role::Link, "Trust project…"));
    let refused = ask_err(&mut f, "run_action", json!({ "action": "view.project.trust" }));
    assert!(refused.contains("person"), "{refused}");
    assert!(f.app.trust_questions.is_empty());

    f.click(Role::Link, "Trust project…");
    assert!(f.shows(Role::Button, "Trust Project"));
    assert!(!f.shows(Role::Button, "Don't Open"), "the project is open");
    f.click(Role::Button, "Trust Project");
    f.step(Vec::new());
    assert!(zen_workbench::trust::is_trusted(&f.env, &f.project));
    assert_eq!(f.opened(), vec![f.project.clone()], "the same window");
    assert!(!f.shows(Role::Link, "Trust project…"));
}

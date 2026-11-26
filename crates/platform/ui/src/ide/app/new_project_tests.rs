use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use egui::accesskit::{Role, TreeUpdate};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_project::generators::{
    FormServer, GeneratedFile, GeneratorForm, GeneratorInfo, GeneratorRequest, GeneratorSection, ProjectGenerator,
};

use super::super::settings::Dispatch;
use super::tests::{test_product, window_project};
use super::*;
use crate::accessibility::testing::find;
use zen_workbench::build_tools::BuildEnv;
use zen_workbench::jdk::{Jdks, Sources, Store};

const SIZE: egui::Vec2 = egui::vec2(1280.0, 800.0);

#[derive(Clone)]
struct Fake {
    id: &'static str,
    section: GeneratorSection,
    paged: bool,
    fail_form: Arc<AtomicBool>,
    hold_form: Arc<AtomicBool>,
    fail_files: bool,
    jvm: bool,
    network: bool,
    saved_form: bool,
    server: bool,
    lists: bool,
    service: Option<&'static str>,
    asked: Arc<AtomicBool>,
}

impl Fake {
    fn new(id: &'static str) -> Self {
        Self {
            id,
            section: GeneratorSection::Types,
            paged: false,
            fail_form: Arc::default(),
            hold_form: Arc::default(),
            fail_files: false,
            jvm: false,
            network: false,
            saved_form: false,
            server: false,
            lists: false,
            service: None,
            asked: Arc::default(),
        }
    }

    fn jvm(id: &'static str) -> Self {
        Self { jvm: true, ..Self::new(id) }
    }
}

impl ProjectGenerator for Fake {
    fn info(&self) -> GeneratorInfo {
        GeneratorInfo {
            id: self.id.to_owned(),
            name: format!("Type {}", self.id),
            icon: "main.go".to_owned(),
            section: self.section,
            network: self.network,
            service: self.service.map(str::to_owned),
            form_depends_on: if self.lists { vec!["catalog".to_owned()] } else { Vec::new() },
            ..GeneratorInfo::default()
        }
    }

    fn form_with(&self, values: &zen_project::generators::Values) -> Result<GeneratorForm, String> {
        if !self.lists {
            return self.form();
        }
        let added = values.get("lists").and_then(|lists| lists["things"].as_array()).map_or(0, Vec::len);
        let options: Vec<Value> = std::iter::once(json!({ "value": "", "label": "" }))
            .chain((0..added).map(|_| json!({ "value": "g:a", "label": "g:a" })))
            .collect();
        let catalog = values.get("catalog").and_then(Value::as_str).unwrap_or("first");
        serde_json::from_value(json!({ "fields": [
            { "id": "about", "label": "", "type": "note", "before": "Go to the ",
              "link": { "label": "Java page", "generator": "java" }, "after": "." },
            { "id": "catalog", "label": "Catalog", "type": "choice", "default": "first", "options": [
                { "value": "first", "label": "First" }, { "value": "second", "label": "Second" }] },
            { "id": "shown", "label": format!("From {catalog}"), "type": "text" },
            { "id": "thing", "label": "Thing", "type": "choice",
              "options": options,
              "manage": { "id": "things", "label": "Add thing", "title": "Add", "addOne": true,
                          "value": "${groupId}:${artifactId}",
                          "columns": [{ "id": "groupId", "label": "Group" }, { "id": "artifactId", "label": "Artifact" }] } },
            { "id": "properties", "label": "Properties", "type": "table", "empty": "No properties",
              "columns": [{ "id": "name", "label": "Name" }, { "id": "value", "label": "Value" }] },
        ] }))
        .map_err(|error| error.to_string())
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        self.asked.store(true, Ordering::Relaxed);
        while self.hold_form.load(Ordering::Relaxed) {
            std::thread::yield_now();
        }
        if self.fail_form.swap(false, Ordering::Relaxed) {
            return Err("the service does not answer".to_owned());
        }
        if self.jvm {
            return serde_json::from_value(json!({ "fields": [
                { "id": "jdk", "label": "JDK:", "type": "jdk", "required": true },
                { "id": "group", "label": "GroupId:", "type": "text", "default": "org.example", "advanced": true },
            ] }))
            .map_err(|error| error.to_string());
        }
        let notice = self.saved_form.then(|| "From saved data".to_owned());
        let page = u8::from(self.paged);
        let fields: GeneratorForm = serde_json::from_value(json!({ "fields": [
            { "id": "kind", "label": "Kind", "type": "choice", "default": "a", "options": [
                { "value": "a", "label": "Kind A" }, { "value": "b", "label": "Kind B" }] },
            { "id": "extra", "label": "Extra", "type": "text",
              "visibleWhen": { "field": "kind", "anyOf": ["b"] } },
            { "id": "build", "label": "Build:", "type": "choice", "segmented": true, "default": "m", "options": [
                { "value": "i", "label": "IDE" }, { "value": "m", "label": "Maven" }] },
            { "id": "starters", "label": "Starters", "type": "select", "page": page, "groups": [
                { "name": "Web", "options": [
                    { "value": "web", "label": "Spring Web", "description": "Build web" },
                    { "value": "old", "label": "Old Starter", "disabledNote": "Needs kind B",
                      "enabledWhen": { "field": "kind", "anyOf": ["b"] } }] },
                { "name": "Data", "options": [{ "value": "jdbc", "label": "JDBC" }] }] },
        ] }))
        .map_err(|error| error.to_string())?;
        let server = self
            .server
            .then(|| FormServer { label: "Server URL:".to_owned(), url: "https://start.example.com/".to_owned() });
        Ok(GeneratorForm { notice, server, ..fields })
    }

    fn files(&self, request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        if self.fail_files {
            return Err("the generator broke".to_owned());
        }
        Ok(vec![GeneratedFile::text("README.md", request.name.clone()), GeneratedFile::text("src/a.txt", "text a")])
    }
}

struct Window {
    app: IdeApp,
    screen: crate::offscreen::Offscreen,
    root: PathBuf,
    _data: tempfile::TempDir,
}

impl Window {
    fn new(project: bool, generators: Vec<Fake>) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(data.path()).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let paths = if project {
            window_project(&root);
            vec![root.join("project")]
        } else {
            Vec::new()
        };
        let app = IdeApp::new(test_product(), registry, Some(root.join("recent.json")), paths);
        {
            let mut settings = app.shared.settings.lock().unwrap();
            settings.autosave_on_idle = false;
            settings.default_project_directory = root.join("projects").to_string_lossy().into_owned();
        }
        for generator in generators {
            app.shared.new_projects.declare(Arc::new(generator));
        }
        let screen = crate::offscreen::Offscreen::new(SIZE, 1.0);
        let mut window = Self { app, screen, root, _data: data };
        let current = window.app.shared.overview().settings;
        let ctx = window.screen.context().clone();
        window.app.handle(&ctx, Request::Settings(Box::new(current)));
        window.steps(if project { 150 } else { 3 });
        window
    }

    fn steps(&mut self, count: usize) {
        for _ in 0..count {
            self.step(Vec::new());
        }
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        let Self { app, screen, .. } = self;
        screen.step(events, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
    }

    fn tree(&mut self) -> TreeUpdate {
        let Self { app, screen, .. } = self;
        screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
    }

    fn has(&mut self, role: Role, name: &str) -> bool {
        let tree = self.tree();
        find(&tree, role, name).is_some()
    }

    fn click(&mut self, role: Role, name: &str) {
        self.steps(2);
        let tree = self.tree();
        let node = find(&tree, role, name).unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let bounds = node.bounds().expect("bounds");
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
        self.steps(3);
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        let (sender, answer) = mpsc::channel();
        let ctx = self.screen.context().clone();
        self.app.route(
            &ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..20 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            self.steps(1);
        }
        answer.recv_timeout(std::time::Duration::from_secs(10)).map_err(|error| error.to_string())?
    }

    fn state(&mut self) -> Value {
        self.call("get_window_state", json!({})).unwrap()
    }

    fn open(&mut self, action: &str, params: Value) -> Value {
        self.call("run_action", json!({ "action": action, "params": params })).unwrap()
    }

    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let started = std::time::Instant::now();
        while !done(self) {
            assert!(started.elapsed() < std::time::Duration::from_secs(30), "{what}");
            self.steps(1);
        }
    }

    fn set_location(&mut self, location: &Path) {
        let slot = self.app.slot_for(None).ok();
        let location = location.to_string_lossy().into_owned();
        let registry = Arc::clone(&self.app.shared.registry);
        match slot {
            Some(slot) => {
                let mut window = slot.window.lock().unwrap();
                let ProjectWindow { session, view } = &mut *window;
                let mut dispatch =
                    |id: &str, params| crate::ide::dialog_dispatch(&registry, session, false, id, params);
                view.new_project.as_mut().unwrap().set_location(&mut dispatch, location);
            }
            None => {
                let session = &mut self.app.welcome_session;
                let mut dispatch =
                    |id: &str, params| crate::ide::dialog_dispatch(&registry, session, false, id, params);
                self.app.welcome.new_project.as_mut().unwrap().set_location(&mut dispatch, location);
            }
        }
    }

    fn with_project_dialog<T>(&mut self, work: impl FnOnce(&mut IdeView, &mut Dispatch<'_>) -> T) -> T {
        let slot = self.app.slot_for(None).unwrap();
        let registry = Arc::clone(&self.app.shared.registry);
        let mut window = slot.window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        let mut dispatch = |id: &str, params| crate::ide::dialog_dispatch(&registry, session, false, id, params);
        work(view, &mut dispatch)
    }

    fn set_clone_url(&mut self, url: &str) {
        self.with_project_dialog(|view, dispatch| {
            view.clone.as_mut().unwrap().set_url(dispatch, url.to_owned());
        });
    }

    fn own_trust_store(&mut self) -> BuildEnv {
        let env = BuildEnv::default();
        self.app.welcome_session.set_state(env.clone());
        if let Ok(slot) = self.app.slot_for(None) {
            slot.window.lock().unwrap().session.set_state(env.clone());
        }
        env
    }

    fn own_jdks(&mut self, versions: &[&str]) {
        let jvm = self.root.join("jvm");
        std::fs::create_dir_all(&jvm).unwrap();
        for version in versions {
            let home = jvm.join(format!("{version}.jdk/Contents/Home"));
            std::fs::create_dir_all(home.join("bin")).unwrap();
            std::fs::write(home.join("bin/java"), "").unwrap();
            std::fs::write(home.join("bin/javac"), "").unwrap();
            std::fs::write(home.join("release"), format!("JAVA_VERSION=\"{version}\"\n")).unwrap();
        }
        let sources = Sources { system_folders: vec![jvm], ..Sources::default() };
        let mut store = Store::with_sources(sources);
        store.set_data_dir(self.root.join("data"));
        let jdks = Jdks::new(store);
        self.app.welcome_session.set_state(jdks.clone());
        if let Ok(slot) = self.app.slot_for(None) {
            slot.window.lock().unwrap().session.set_state(jdks);
        }
    }

    fn person_at_the_clone(&mut self) {
        self.with_project_dialog(|view, _| view.clone.as_mut().unwrap().by_agent = false);
    }

    fn person_at_the_wizard(&mut self) {
        let slot = self.app.slot_for(None).unwrap();
        slot.window.lock().unwrap().view.new_project.as_mut().unwrap().by_agent = false;
    }

    fn snapshot(&mut self, name: &str) {
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) else { return };
        let Self { app, screen, .. } = self;
        let image = screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        crate::offscreen::write_png(&dir.join(format!("{name}.png")), &image).unwrap();
    }
}

fn wizard(window: &mut Window) -> Value {
    window.state()["newProject"].clone()
}

#[test]
fn the_action_opens_the_wizard_with_the_types_and_create_cancel() {
    let mut window = Window::new(true, vec![Fake::new("go")]);
    window.open("view.newProject.open", json!({}));
    window.steps(3);
    assert_eq!(window.state()["popups"], json!(["newProject"]));
    assert!(window.has(Role::ListBoxOption, "Type go"));
    assert!(window.has(Role::ListBoxOption, &t!("workbench-new-project-empty")));
    assert!(window.has(Role::Button, &t!("ide-new-project-create")));
    assert!(window.has(Role::Button, &t!("settings-cancel")));
    assert_eq!(wizard(&mut window)["generator"], "go");
    window.snapshot("new-project");
    window.click(Role::Button, &t!("settings-cancel"));
    assert_eq!(window.state()["popups"], json!([]));
}

#[test]
fn view_new_project_empty_opens_on_empty_project_with_its_description_and_the_default_location() {
    let mut window = Window::new(true, vec![Fake::new("go")]);
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    let state = wizard(&mut window);
    assert_eq!(state["generator"], "empty");
    assert_eq!(state["name"], "untitled");
    assert_eq!(state["location"], json!(window.root.join("projects/untitled")));
    assert!(window.has(Role::Label, &t!("workbench-new-project-empty-description")));
    let parent = crate::ide::home_relative(&window.root.join("projects/untitled"));
    assert!(window.has(Role::Label, &t!("ide-new-project-created-in", path = parent.as_str())));
}

#[test]
fn the_name_is_the_last_part_of_the_location_and_changes_it() {
    let mut window = Window::new(true, Vec::new());
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    let other = window.root.join("projects/shop");
    window.set_location(&other);
    window.steps(2);
    assert_eq!(wizard(&mut window)["name"], "shop");
    let registry = Arc::clone(&window.app.shared.registry);
    let slot = window.app.slot_for(None).unwrap();
    {
        let mut shown = slot.window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *shown;
        let mut dispatch = |id: &str, params| crate::ide::dialog_dispatch(&registry, session, false, id, params);
        view.new_project.as_mut().unwrap().set_name(&mut dispatch, "store");
    }
    assert_eq!(wizard(&mut window)["location"], json!(window.root.join("projects/store")));
}

#[test]
fn an_invalid_location_disables_create_with_the_reason() {
    let mut window = Window::new(true, Vec::new());
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    let file = window.root.join("file.txt");
    std::fs::write(&file, "x").unwrap();
    window.set_location(&file);
    window.steps(3);
    assert_eq!(wizard(&mut window)["problem"], t!("workbench-new-project-is-file"));
    assert!(window.has(Role::Label, &t!("workbench-new-project-is-file")));
    window.click(Role::Button, &t!("ide-new-project-create"));
    assert_eq!(wizard(&mut window)["creating"], false, "Create does nothing");
    assert_eq!(window.state()["popups"], json!(["newProject"]));
}

#[test]
fn a_person_creates_a_git_project_which_opens_trusted() {
    let mut window = Window::new(true, Vec::new());
    let trust = window.own_trust_store();
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    let target = window.root.join("projects/created");
    window.set_location(&target);
    window.person_at_the_wizard();
    window.click(Role::CheckBox, &t!("ide-new-project-git"));
    window.click(Role::Button, &t!("ide-new-project-create"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(target.join(".git").is_dir(), "the Git repository");
    assert!(zen_workbench::trust::is_trusted(&trust, &target), "created projects are trusted");
}

#[test]
fn an_agent_in_the_wizard_never_trusts_the_project() {
    let mut window = Window::new(true, Vec::new());
    let trust = window.own_trust_store();
    window.app.trust_env = trust.clone();
    window.app.asks_trust = true;
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    let target = window.root.join("projects/created");
    window.set_location(&target);
    window.click(Role::Button, &t!("ide-new-project-create"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(window.app.trust_questions.is_empty(), "only a person is asked");
    assert!(!zen_workbench::trust::is_trusted(&trust, &target));
}

#[test]
fn a_folder_that_holds_something_asks_before_it_is_used() {
    let mut window = Window::new(true, Vec::new());
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    let target = window.root.join("full");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("mine.txt"), "mine").unwrap();
    window.set_location(&target);
    window.click(Role::Button, &t!("ide-new-project-create"));
    assert!(window.has(Role::Button, &t!("ide-new-project-reuse-yes")), "it asks");
    assert!(!window.app.open_projects().contains(&target));
    window.click(Role::Button, &t!("ide-new-project-reuse-yes"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert_eq!(std::fs::read_to_string(target.join("mine.txt")).unwrap(), "mine");
}

#[test]
fn a_type_that_asks_a_service_shows_the_address_and_asks_only_after_connect() {
    let service = "https://start.example";
    let fake = Fake { service: Some(service), network: true, ..Fake::new("spring") };
    let asked = Arc::clone(&fake.asked);
    let mut window = Window::new(true, vec![Fake::new("plain"), fake]);
    window.open("view.newProject.open", json!({ "generator": "plain" }));
    window.person_at_the_wizard();
    window.steps(2);
    window.click(Role::ListBoxOption, "Type spring");
    window.steps(3);
    assert!(window.has(Role::Label, &t!("ide-new-project-connects", service = service)), "the address comes first");
    assert!(!asked.load(Ordering::Relaxed), "nothing was asked yet");
    window.click(Role::Button, &t!("ide-new-project-connect"));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    assert!(asked.load(Ordering::Relaxed));
}

#[test]
fn a_form_being_loaded_shows_a_spinner_line_and_a_failed_one_retries() {
    let fake = Fake::new("spring");
    fake.hold_form.store(true, Ordering::Relaxed);
    fake.fail_form.store(true, Ordering::Relaxed);
    let (hold, fail) = (Arc::clone(&fake.hold_form), Arc::clone(&fake.fail_form));
    let mut window = Window::new(true, vec![fake]);
    window.open("view.newProject.open", json!({ "generator": "spring" }));
    window.steps(3);
    let loading = t!("ide-new-project-loading", name = "Type spring");
    assert!(window.has(Role::Label, &loading), "the line while it loads");
    assert_ne!(wizard(&mut window)["creating"], true);
    hold.store(false, Ordering::Relaxed);
    window.until("the failure shows", |window| window.has(Role::Label, "the service does not answer"));
    assert!(!fail.load(Ordering::Relaxed));
    window.click(Role::Button, &t!("ide-new-project-retry"));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    assert!(!window.has(Role::Label, "the service does not answer"));
    window.snapshot("new-project-form");
}

#[test]
fn a_field_shows_while_the_field_it_depends_on_has_the_value_and_a_select_searches() {
    let mut window = Window::new(true, vec![Fake::new("spring")]);
    window.open("view.newProject.open", json!({ "generator": "spring" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    assert!(!window.has(Role::TextInput, "Extra"));
    assert!(window.has(Role::CheckBox, "Spring Web"));
    assert!(!window.has(Role::CheckBox, "JDBC"), "only the first group starts open");
    let slot = window.app.slot_for(None).unwrap();
    {
        let mut shown = slot.window.lock().unwrap();
        let dialog = shown.view.new_project.as_mut().unwrap();
        dialog.set_value("kind", json!("b"));
        dialog.set_search("starters", "jd");
    }
    window.steps(3);
    assert!(window.has(Role::TextInput, "Extra"), "the dependent field");
    assert!(window.has(Role::CheckBox, "JDBC"));
    assert!(!window.has(Role::CheckBox, "Spring Web"), "the search filters");
}

#[test]
fn a_form_with_a_second_page_has_next_previous_and_create_and_no_file_preview() {
    let fake = Fake { paged: true, section: GeneratorSection::Generators, ..Fake::new("spring") };
    let mut window = Window::new(true, vec![fake]);
    window.open("view.newProject.open", json!({ "generator": "spring" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    let target = window.root.join("projects/shop");
    window.set_location(&target);
    assert!(!window.has(Role::CheckBox, "Spring Web"), "the second page's field waits");
    assert!(!window.has(Role::Button, &t!("ide-new-project-create")), "Next comes first");
    window.click(Role::Button, &t!("ide-new-project-next"));
    assert_eq!(wizard(&mut window)["page"], 1);
    assert!(window.has(Role::CheckBox, "Spring Web"));
    assert!(window.has(Role::Label, &t!("ide-new-project-none-added")));
    assert!(!window.has(Role::ListBoxOption, "Type spring"), "the list of types stands only on the first page");
    window.snapshot("new-project-second-page");
    window.click(Role::CheckBox, "Spring Web");
    assert!(!window.has(Role::Label, &t!("ide-new-project-none-added")));
    assert!(window.has(Role::Label, "Build web"), "the ticked option's description");
    window.click(Role::Button, &t!("ide-new-project-previous"));
    assert_eq!(wizard(&mut window)["page"], 0);
    window.click(Role::Button, &t!("ide-new-project-next"));
    assert!(window.has(Role::Label, "Spring Web"), "the added list keeps it");
    window.click(Role::Button, &t!("ide-new-project-create"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(target.join("src/a.txt").is_file());
}

#[test]
fn a_failing_generator_leaves_nothing_open() {
    let broken = Fake { fail_files: true, ..Fake::new("broken") };
    let mut window = Window::new(true, vec![broken]);
    window.open("view.newProject.open", json!({ "generator": "broken" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    let target = window.root.join("projects/shop");
    window.set_location(&target);
    window.click(Role::Button, &t!("ide-new-project-create"));
    window.until("the error shows", |window| window.has(Role::Label, "the generator broke"));
    assert_eq!(window.app.open_projects().len(), 1, "no half-created project opens");
    assert!(!target.exists());
    assert_eq!(wizard(&mut window)["error"], "the generator broke");
}

#[test]
fn arrow_keys_move_through_the_types_and_the_search_narrows_them() {
    let mut window = Window::new(true, vec![Fake::new("go"), Fake::new("java")]);
    window.open("view.newProject.open", json!({ "generator": "go" }));
    window.steps(3);
    let press = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    window.step(vec![press(egui::Key::ArrowDown)]);
    window.steps(2);
    assert_eq!(wizard(&mut window)["generator"], "java");
    window.step(vec![press(egui::Key::ArrowUp)]);
    window.steps(2);
    assert_eq!(wizard(&mut window)["generator"], "go");
    let slot = window.app.slot_for(None).unwrap();
    slot.window.lock().unwrap().view.new_project.as_mut().unwrap().set_type_search("jav");
    window.steps(3);
    assert!(window.has(Role::ListBoxOption, "Type java"));
    assert!(!window.has(Role::ListBoxOption, "Type go"));
    assert!(!window.has(Role::ListBoxOption, &t!("workbench-new-project-empty")));
}

#[test]
fn the_list_has_both_headings_and_more_via_plugins_opens_the_marketplace_and_returns() {
    let plugin = Fake { section: GeneratorSection::Generators, ..Fake::new("vite") };
    let mut window = Window::new(true, vec![Fake::new("go"), plugin]);
    window.open("view.newProject.open", json!({}));
    window.steps(3);
    assert!(window.has(Role::Label, &t!("ide-new-project-title")));
    assert!(window.has(Role::Label, &t!("ide-new-project-generators")));
    window.click(Role::Button, &t!("ide-new-project-more-plugins"));
    let popups = window.state()["popups"].clone();
    assert!(popups.as_array().unwrap().contains(&json!("settings:plugins")), "{popups}");
    assert!(window.state()["newProject"].is_object(), "the wizard stays behind Settings");
}

#[test]
fn a_segmented_choice_is_a_row_of_buttons_that_sets_the_value() {
    let mut window = Window::new(true, vec![Fake::new("spring")]);
    window.open("view.newProject.open", json!({ "generator": "spring" }));
    window.until("the form shows", |window| window.has(Role::RadioButton, "Maven"));
    assert!(!window.has(Role::ComboBox, "Build:"));
    window.click(Role::RadioButton, "IDE");
    let slot = window.app.slot_for(None).unwrap();
    let given = slot.window.lock().unwrap().view.new_project.as_ref().unwrap().given_value("build");
    assert_eq!(given, Some(json!("i")));
}

#[test]
fn a_form_s_server_stands_above_name_and_its_gear_opens_system_settings() {
    let fake = Fake { server: true, ..Fake::new("spring") };
    let mut window = Window::new(true, vec![fake]);
    window.open("view.newProject.open", json!({ "generator": "spring" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    assert!(window.has(Role::Button, "start.example.com"));
    window.click(Role::Button, &t!("ide-new-project-server-settings"));
    let popups = window.state()["popups"].clone();
    assert!(popups.as_array().unwrap().iter().any(|popup| popup == "settings:systemSettings"), "{popups}");
}

#[test]
fn close_other_projects_keeps_the_one_in_front_and_close_project_leaves_the_welcome_screen() {
    let mut window = Window::new(true, Vec::new());
    let other = window.root.join("other");
    std::fs::create_dir(&other).unwrap();
    opening(&mut window, super::super::settings::OpenProjectIn::CurrentWindow);
    let ctx = window.screen.context().clone();
    window.app.handle(&ctx, Request::Open(other.clone()));
    window.steps(3);
    assert_eq!(window.app.open_projects().len(), 2);
    let active = window.app.shared.overview().active_tab.unwrap();
    let front = json!({ "action": "view.project.closeOthers", "params": {}, "project": active });
    window.call("run_action", front).unwrap();
    window.steps(3);
    assert_eq!(window.app.open_projects(), [active]);
    window.open("view.project.close", json!({}));
    window.steps(3);
    assert!(window.app.open_projects().is_empty());

    let mut window = Window::new(true, Vec::new());
    let other = window.root.join("other");
    std::fs::create_dir(&other).unwrap();
    opening(&mut window, super::super::settings::OpenProjectIn::CurrentWindow);
    let ctx = window.screen.context().clone();
    window.app.handle(&ctx, Request::Open(other));
    window.steps(3);
    let active = window.app.shared.overview().active_tab.unwrap();
    let front = json!({ "action": "view.project.closeAll", "params": {}, "project": active });
    window.call("run_action", front).unwrap();
    window.steps(3);
    assert!(window.app.open_projects().is_empty());
}

#[test]
fn a_form_from_saved_data_says_so_and_the_wizard_says_when_the_network_is_off() {
    let fake = Fake { network: true, saved_form: true, ..Fake::new("spring") };
    let mut window = Window::new(true, vec![fake]);
    window.open("view.newProject.open", json!({ "generator": "spring" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    assert!(window.has(Role::Label, "From saved data"));
    assert!(window.has(Role::Label, &t!("ide-new-project-network")));
    window.open("view.newProject.close", json!({}));
    let mut settings = window.app.shared.overview().settings;
    settings.generators_network = false;
    let ctx = window.screen.context().clone();
    window.app.handle(&ctx, Request::Settings(Box::new(settings)));
    window.open("view.newProject.open", json!({ "generator": "spring" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "Kind"));
    assert!(window.has(Role::Label, &t!("ide-new-project-no-network")));
    assert!(!window.has(Role::Label, &t!("ide-new-project-network")));
}

#[test]
fn generators_stand_under_their_heading_after_empty_project() {
    let plugin = Fake { section: GeneratorSection::Generators, ..Fake::new("vite") };
    let mut window = Window::new(true, vec![plugin]);
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    assert!(window.has(Role::ListBoxOption, "Type vite"));
    assert!(window.has(Role::Label, &t!("ide-new-project-generators")));
}

#[test]
fn the_jdk_field_lists_the_jdks_and_advanced_fields_stay_folded_until_opened() {
    let mut window = Window::new(true, vec![Fake::jvm("java")]);
    window.own_jdks(&["21.0.5"]);
    window.open("view.newProject.open", json!({ "generator": "java" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "JDK:"));
    assert_eq!(wizard(&mut window)["problem"], Value::Null);
    window.snapshot("new-project-jdk");
    assert!(!window.has(Role::TextInput, "GroupId:"), "folded");
    window.click(Role::DisclosureTriangle, &t!("ide-new-project-advanced"));
    assert!(window.has(Role::TextInput, "GroupId:"), "opened");
    window.click(Role::ComboBox, "JDK:");
    assert!(window.has(Role::Button, &t!("jdk-download-version", version = 21)));
}

#[test]
fn no_jdk_disables_create_with_the_reason() {
    let mut window = Window::new(true, vec![Fake::jvm("java")]);
    window.own_jdks(&[]);
    window.open("view.newProject.open", json!({ "generator": "java" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "JDK:"));
    assert_eq!(wizard(&mut window)["problem"], t!("workbench-new-project-no-jdk"));
    window.snapshot("new-project-no-jdk");
    window.click(Role::Button, &t!("ide-new-project-create"));
    assert_eq!(wizard(&mut window)["creating"], false);
}

#[test]
fn download_in_the_jdk_field_asks_for_that_release_and_a_finished_download_reloads_the_form() {
    let mut window = Window::new(true, vec![Fake::jvm("java")]);
    window.own_jdks(&[]);
    window.open("view.newProject.open", json!({ "generator": "java" }));
    window.until("the form shows", |window| window.has(Role::ComboBox, "JDK:"));
    let slot = window.app.slot_for(None).unwrap();
    let mut calls: Vec<(String, Value)> = Vec::new();
    {
        let mut shown = slot.window.lock().unwrap();
        let dialog = shown.view.new_project.as_mut().unwrap();
        let mut dispatch = |id: &str, params: Value| {
            calls.push((id.to_owned(), params));
            Ok(json!({}))
        };
        dialog.download_jdk(&mut dispatch, 21);
        assert!(dialog.state()["downloadingJdk"] == 21);
    }
    assert_eq!(calls, [("toolchain.jdk.download".to_owned(), json!({ "version": 21 }))]);
    window.until("the download's end is noticed", |window| wizard(window)["downloadingJdk"].is_null());
}

#[test]
fn the_welcome_screen_s_new_project_button_opens_the_wizard_and_creates() {
    let mut window = Window::new(false, vec![Fake::new("go")]);
    let trust = window.own_trust_store();
    window.click(Role::Button, &t!("ide-welcome-new"));
    assert!(window.app.welcome.new_project.is_some(), "the wizard opens");
    window.click(Role::ListBoxOption, &t!("workbench-new-project-empty"));
    let target = window.root.join("projects/first");
    window.set_location(&target);
    window.click(Role::Button, &t!("ide-new-project-create"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(window.app.welcome.new_project.is_none());
    assert!(zen_workbench::trust::is_trusted(&trust, &target));
}

#[test]
fn agents_reach_the_wizard_and_its_commands_with_no_project_open() {
    let mut window = Window::new(false, vec![Fake::new("go")]);
    window.own_trust_store();
    let list = window.call("dispatch_command", json!({ "command": "project.new.generators" })).unwrap();
    assert_eq!(list["generators"][0]["id"], "go");
    let target = window.root.join("projects/agent");
    let params = json!({ "generator": "empty", "location": target, "waitMs": 10000 });
    let created =
        window.call("dispatch_command", json!({ "command": "project.new.create", "params": params })).unwrap();
    assert_eq!(created["state"], "ready", "{created}");
    assert_eq!(created["trusted"], false, "an agent's project is not trusted");
    window.open("view.newProject.open", json!({}));
    window.steps(3);
    assert!(window.app.welcome.new_project.is_some());
    window.open("view.newProject.close", json!({}));
    assert!(window.app.welcome.new_project.is_none());
}

fn opening(window: &mut Window, opening: super::super::settings::OpenProjectIn) {
    let settings = IdeSettings { open_project_in: Some(opening), ..window.app.shared.overview().settings };
    let ctx = window.screen.context().clone();
    window.app.handle(&ctx, Request::Settings(Box::new(settings)));
}

#[test]
fn open_project_in_new_window_gives_the_project_its_own_window() {
    let mut window = Window::new(true, Vec::new());
    opening(&mut window, super::super::settings::OpenProjectIn::NewWindow);
    let target = window.root.join("other");
    std::fs::create_dir(&target).unwrap();
    let ctx = window.screen.context().clone();
    window.app.handle(&ctx, Request::Open(target.clone()));
    let slot = window.app.slot_for(Some(&target)).unwrap();
    assert!(!slot.tabbed);
}

#[test]
fn open_project_in_current_window_makes_it_a_tab() {
    let mut window = Window::new(true, Vec::new());
    opening(&mut window, super::super::settings::OpenProjectIn::CurrentWindow);
    let target = window.root.join("other");
    std::fs::create_dir(&target).unwrap();
    let ctx = window.screen.context().clone();
    window.app.handle(&ctx, Request::Open(target.clone()));
    assert!(window.app.slot_for(Some(&target)).unwrap().tabbed);
}

#[test]
fn open_project_in_ask_asks_and_the_answer_decides() {
    let mut window = Window::new(true, Vec::new());
    opening(&mut window, super::super::settings::OpenProjectIn::Ask);
    let target = window.root.join("other");
    std::fs::create_dir(&target).unwrap();
    let ctx = window.screen.context().clone();
    window.app.handle(&ctx, Request::Open(target.clone()));
    window.steps(3);
    assert!(!window.app.open_projects().contains(&target), "nothing opens before the answer");
    window.click(Role::Button, &t!("ide-open-project-new-window"));
    assert!(!window.app.slot_for(Some(&target)).unwrap().tabbed);
}

#[test]
fn a_saved_new_window_choice_reads_as_new_window() {
    let saved: IdeSettings = serde_json::from_value(json!({ "openInNewWindow": true })).unwrap();
    assert_eq!(saved.project_opening(), super::super::settings::OpenProjectIn::NewWindow);
    assert_eq!(IdeSettings::default().project_opening(), super::super::settings::OpenProjectIn::CurrentWindow);
}

fn origin(folder: &Path) -> String {
    std::fs::create_dir_all(folder).unwrap();
    let git = |args: &[&str]| zen_vcs::git::run(folder, args, None).unwrap();
    git(&["init", "-q"]);
    git(&["config", "user.email", "a@b.c"]);
    git(&["config", "user.name", "A"]);
    std::fs::write(folder.join("pom.xml"), "<project/>").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);
    format!("file://{}", folder.display())
}

fn clone_state(window: &mut Window) -> Value {
    window.state()["clone"].clone()
}

#[test]
fn clone_repository_has_only_the_url_page_and_clone_waits_for_a_url() {
    let mut window = Window::new(true, Vec::new());
    window.open("view.project.fromVcs", json!({}));
    window.steps(3);
    assert!(window.has(Role::ListBoxOption, &t!("ide-clone-repository-url")));
    assert!(window.has(Role::ComboBox, &t!("ide-clone-version-control")));
    assert!(window.has(Role::CheckBox, &t!("ide-clone-shallow")));
    assert_eq!(clone_state(&mut window)["problem"], t!("workbench-clone-no-url"));
    window.set_clone_url("https://host/team/shop.git");
    let state = clone_state(&mut window);
    assert_eq!(state["problem"], Value::Null, "{state}");
    assert!(state["directory"].as_str().unwrap().ends_with("projects/shop"), "{state}");
    window.with_project_dialog(|view, _| view.clone.as_mut().unwrap().set_shallow(Some("0")));
    assert_eq!(clone_state(&mut window)["problem"], t!("ide-clone-bad-depth"));
    window.with_project_dialog(|view, _| view.clone.as_mut().unwrap().set_shallow(None));
    window.snapshot("clone-repository");
    window.open("view.project.fromVcs.close", json!({}));
    assert_eq!(clone_state(&mut window), Value::Null);
}

#[test]
fn a_person_clones_is_asked_to_open_and_the_clone_opens_untrusted() {
    let mut window = Window::new(true, Vec::new());
    let trust = window.own_trust_store();
    let url = origin(&window.root.join("origin"));
    window.open("view.project.fromVcs", json!({}));
    window.steps(3);
    window.set_clone_url(&url);
    let target = window.root.join("projects/origin");
    assert_eq!(clone_state(&mut window)["directory"], target.to_string_lossy().as_ref());
    window.person_at_the_clone();
    window.click(Role::Button, &t!("ide-clone-clone"));
    window.until("the question", |window| !clone_state(window)["cloned"].is_null());
    assert!(!window.app.open_projects().contains(&target), "it asks first");
    window.click(Role::Button, &t!("ide-clone-open-yes"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(!zen_workbench::trust::is_trusted(&trust, &target), "a clone is never trusted by itself");
}

#[test]
fn a_clone_a_person_opens_comes_through_the_trust_dialog() {
    let mut window = Window::new(true, Vec::new());
    let trust = window.own_trust_store();
    window.app.trust_env = trust.clone();
    window.app.asks_trust = true;
    let url = origin(&window.root.join("origin"));
    window.open("view.project.fromVcs", json!({}));
    window.steps(3);
    window.set_clone_url(&url);
    let target = window.root.join("projects/origin");
    window.person_at_the_clone();
    window.click(Role::Button, &t!("ide-clone-clone"));
    window.until("the question", |window| !clone_state(window)["cloned"].is_null());
    window.click(Role::Button, &t!("ide-clone-open-yes"));
    window.until("the trust dialog", |window| !window.app.trust_questions.is_empty());
    assert!(!window.app.open_projects().contains(&target), "the answer comes first");
    window.click(Role::Button, &t!("ide-trust-preview"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(!zen_workbench::trust::is_trusted(&trust, &target), "safe mode trusts nothing");
}

#[test]
fn a_clone_an_agent_starts_opens_in_safe_mode_without_asking_the_person() {
    let mut window = Window::new(true, Vec::new());
    let trust = window.own_trust_store();
    window.app.trust_env = trust.clone();
    window.app.asks_trust = true;
    let url = origin(&window.root.join("origin"));
    window.open("view.project.fromVcs", json!({}));
    window.steps(3);
    window.set_clone_url(&url);
    let target = window.root.join("projects/origin");
    window.click(Role::Button, &t!("ide-clone-clone"));
    window.until("the question", |window| !clone_state(window)["cloned"].is_null());
    window.click(Role::Button, &t!("ide-clone-open-yes"));
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(window.app.trust_questions.is_empty(), "only a person is asked");
    assert!(!zen_workbench::trust::is_trusted(&trust, &target));
}

#[test]
fn not_opening_a_clone_keeps_it_on_disk() {
    let mut window = Window::new(true, Vec::new());
    let url = origin(&window.root.join("origin"));
    window.open("view.project.fromVcs", json!({}));
    window.steps(3);
    window.set_clone_url(&url);
    window.person_at_the_clone();
    window.click(Role::Button, &t!("ide-clone-clone"));
    window.until("the question", |window| !clone_state(window)["cloned"].is_null());
    window.click(Role::Button, &t!("ide-clone-open-no"));
    window.until("the dialog closes", |window| clone_state(window).is_null());
    let target = window.root.join("projects/origin");
    assert!(target.join("pom.xml").is_file());
    assert!(!window.app.open_projects().contains(&target));
}

#[test]
fn a_clone_that_fails_shows_why_and_stays_open() {
    let mut window = Window::new(true, Vec::new());
    window.open("view.project.fromVcs", json!({}));
    window.steps(3);
    let url = format!("file://{}", window.root.join("nothing").display());
    window.set_clone_url(&url);
    window.person_at_the_clone();
    window.click(Role::Button, &t!("ide-clone-clone"));
    window.until("the error", |window| !clone_state(window)["error"].is_null());
    assert!(!window.root.join("projects/nothing").exists());
}

#[test]
fn the_welcome_screen_clones_too() {
    let mut window = Window::new(false, Vec::new());
    window.click(Role::Button, &t!("ide-welcome-clone"));
    window.steps(3);
    assert!(window.app.welcome.clone.is_some());
    window.open("view.project.fromVcs.close", json!({}));
    assert!(window.app.welcome.clone.is_none());
    let found = window.call("dispatch_command", json!({ "command": "project.fromVcs.defaults" })).unwrap();
    assert_eq!(found["urls"], json!([]));
}

#[test]
fn project_from_existing_sources_opens_the_folder_of_a_chosen_build_file_untrusted() {
    let mut window = Window::new(true, Vec::new());
    let trust = window.own_trust_store();
    let target = window.root.join("sources");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("build.gradle"), "").unwrap();
    let answer = window.open("view.project.fromSources", json!({ "path": target.join("build.gradle") }));
    assert_eq!(answer["result"]["build"], "gradle");
    window.until("the project opens", |window| window.app.open_projects().contains(&target));
    assert!(!zen_workbench::trust::is_trusted(&trust, &target));
}

#[test]
fn the_choosers_are_for_people_agents_give_a_path() {
    let mut window = Window::new(true, Vec::new());
    for action in ["view.project.fromSources", "view.module.fromSources"] {
        let refused = window.call("run_action", json!({ "action": action })).unwrap_err();
        assert!(refused.contains("`path`"), "{refused}");
    }
}

fn module_window() -> Window {
    let mut window = Window::new(true, vec![Fake::jvm("java")]);
    window.own_jdks(&["21"]);
    window.steps(3);
    window
}

#[test]
fn new_module_is_the_wizard_with_its_own_heading_and_location_and_no_empty_project_or_git() {
    let mut window = module_window();
    window.open("view.module.new", json!({}));
    window.steps(3);
    let state = window.state()["newProject"].clone();
    assert_eq!(state["module"], true);
    let project = window.root.join("project");
    assert_eq!(state["location"], project.to_string_lossy().as_ref());
    assert!(window.has(Role::ListBoxOption, "Type java"));
    assert!(!window.has(Role::ListBoxOption, &t!("workbench-new-project-empty")));
    assert!(!window.has(Role::CheckBox, &t!("ide-new-project-git")));
    window.snapshot("new-module");
}

#[test]
fn new_module_has_the_wizard_s_groups_search_and_segmented_build_system() {
    let mut window = Window::new(true, vec![Fake::new("java")]);
    window.open("view.module.new", json!({}));
    window.until("the form shows", |window| window.has(Role::RadioButton, "Maven"));
    assert!(window.has(Role::Label, &t!("ide-new-module-title")));
    assert!(!window.has(Role::Label, &t!("ide-new-project-title")));
    assert!(window.has(Role::TextInput, &t!("ide-new-project-search")));
    assert!(window.has(Role::Button, &t!("ide-new-project-more-plugins")));
    let line = t!("ide-new-module-where", path = "");
    let tree = window.tree();
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.value().or(node.label()).is_some_and(|text| text.starts_with(line.trim()))),
        "the line says where the module goes"
    );
}

#[test]
fn a_module_s_location_stays_the_folder_it_goes_in() {
    let mut window = module_window();
    window.open("view.module.new", json!({}));
    window.steps(3);
    let folder = window.root.join("project/services");
    window.with_project_dialog(|view, dispatch| {
        view.new_project.as_mut().unwrap().set_location(dispatch, folder.to_string_lossy().into_owned());
    });
    window.steps(3);
    let state = window.state()["newProject"].clone();
    assert_eq!(state["location"], folder.to_string_lossy().as_ref());
    assert_eq!(state["name"], "untitled");
}

#[test]
fn a_person_creates_a_module_inside_the_project() {
    let mut window = module_window();
    window.open("view.module.new", json!({}));
    window.steps(3);
    window.person_at_the_wizard();
    window.click(Role::Button, &t!("ide-new-project-create"));
    let folder = window.root.join("project/untitled");
    window.until("the module", |window| window.state()["newProject"].is_null());
    assert!(folder.join("README.md").is_file());
    assert_eq!(window.app.open_projects().len(), 1, "a module opens nothing");
}

fn archetype_window() -> Window {
    let generator = Fake { lists: true, section: GeneratorSection::Generators, ..Fake::new("archetype") };
    let mut window = Window::new(true, vec![Fake::jvm("java"), generator]);
    window.open("view.newProject.open", json!({ "generator": "archetype" }));
    window.steps(10);
    window
}

#[test]
fn a_note_s_link_selects_the_other_type() {
    let mut window = archetype_window();
    window.person_at_the_wizard();
    window.snapshot("maven-archetype");
    assert!(window.has(Role::CheckBox, &t!("ide-new-project-git")));
    assert!(window.has(Role::TextInput, &t!("ide-new-project-search")));
    window.click(Role::Link, "Java page");
    assert_eq!(wizard(&mut window)["generator"], "java");
}

#[test]
fn a_field_a_form_depends_on_loads_it_again() {
    let mut window = archetype_window();
    assert!(window.has(Role::TextInput, "From first"));
    window.with_project_dialog(|view, _| view.new_project.as_mut().unwrap().set_value("catalog", json!("second")));
    window.with_project_dialog(|view, _| view.new_project.as_mut().unwrap().form_changed());
    window.until("the new form", |window| window.has(Role::TextInput, "From second"));
}

#[test]
fn add_keeps_a_row_for_the_type_and_chooses_it() {
    let mut window = archetype_window();
    window.with_project_dialog(|view, dispatch| {
        let dialog = view.new_project.as_mut().unwrap();
        dialog.open_managed_list(dispatch, "thing");
        let row = [("groupId", "g"), ("artifactId", "a")].map(|(key, value)| (key.to_owned(), value.to_owned()));
        dialog.set_managed_row(row.into_iter().collect());
        dialog.save_list(dispatch);
    });
    window.until("the form with the row", |window| window.has(Role::ComboBox, "Thing"));
    window.steps(5);
    assert_eq!(wizard(&mut window)["managing"], Value::Null);
    let rows = window
        .call(
            "dispatch_command",
            json!({ "command": "project.newLists.get", "params": { "generator": "archetype", "list": "things" } }),
        )
        .unwrap();
    assert_eq!(rows["rows"][0]["artifactId"], "a");
    let values = window.with_project_dialog(|view, _| view.new_project.as_ref().unwrap().value("thing"));
    assert_eq!(values, Some(json!("g:a")), "the added row is chosen");
}

#[test]
fn a_table_starts_empty_and_plus_adds_a_row() {
    let mut window = archetype_window();
    window.person_at_the_wizard();
    window.click(Role::Button, "+");
    let rows = window.with_project_dialog(|view, _| view.new_project.as_ref().unwrap().value("properties"));
    assert_eq!(rows, Some(json!([{}])));
    assert!(window.has(Role::TextInput, "Value"));
}

#[test]
fn the_file_new_menu_has_the_specified_entries_in_order() {
    use super::super::main_menu::Entry::{Action, NewItems, Separator, Submenu};
    let (_, file) = super::super::main_menu::MENUS[0];
    let Submenu(_, new) = file[0] else { panic!("File starts with New") };
    let shown: Vec<&str> = new
        .iter()
        .map(|entry| match entry {
            Action(action) => *action,
            Separator => "-",
            NewItems => "file items",
            _ => panic!("New has no other kind of entry"),
        })
        .collect();
    assert_eq!(
        shown,
        [
            "view.newProject.open",
            "view.project.fromSources",
            "view.project.fromVcs",
            "-",
            "view.module.new",
            "view.module.fromSources",
            "-",
            "file items",
        ]
    );
}

#[test]
fn an_agent_in_the_welcome_screen_s_wizard_never_trusts_the_project() {
    let mut window = Window::new(false, Vec::new());
    let trust = window.own_trust_store();
    window.open("view.newProject.empty", json!({}));
    window.steps(3);
    let target = window.root.join("projects/by-agent");
    window.set_location(&target);
    window.click(Role::Button, &t!("ide-new-project-create"));
    window.until("the project", |_| target.is_dir());
    assert!(!zen_workbench::trust::is_trusted(&trust, &target));
}

use egui::accesskit::{Role, TreeUpdate};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use serde_json::{Value, json};

use super::*;
use crate::accessibility::testing::find;
use crate::offscreen::Offscreen;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 820.0);

const POM: &str = "<project><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>shop</artifactId>\
    <version>1</version></project>";

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

fn draw(app: &mut IdeApp) -> impl FnMut(&mut Ui) + '_ {
    |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    }
}

fn center(tree: &TreeUpdate, role: Role, name: &str) -> Pos2 {
    let bounds = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == role && node.label() == Some(name))
        .filter_map(|(_, node)| node.bounds())
        .max_by(|one, other| one.x0.total_cmp(&other.x0))
        .unwrap_or_else(|| panic!("{name}"));
    egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32)
}

fn click(screen: &mut Offscreen, app: &mut IdeApp, at: Pos2, button: PointerButton) {
    let press = |pressed| Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE };
    for events in [vec![Event::PointerMoved(at)], vec![press(true)], vec![press(false)], Vec::new(), Vec::new()] {
        screen.step(events, draw(app));
    }
}

fn tool_state(app: &IdeApp, tool: &str) -> Value {
    let windows = app.shared.state().windows.clone();
    let window = windows[0].window.lock().unwrap();
    let ProjectWindow { session, view } = &*window;
    view.window_state(session)["buildTools"][tool].clone()
}

fn state(app: &IdeApp) -> Value {
    tool_state(app, "maven")
}

#[test]
fn the_maven_window_is_used_by_mouse_and_keyboard_when_drawn() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    crate::keymap::load(Arc::new(Mutex::new(KeymapStore::new(None, Vec::new()))));
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("pom.xml"), POM).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut app = IdeApp::new(product(), registry, Some(data.path().join("recent.json")), vec![project]);
    app.splash = super::super::splash::Splash::finished();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/maven/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let plugin = zen_workbench::plugin_projects::PluginProject {
        id: "maven".to_owned(),
        name: "Maven".to_owned(),
        support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
    };
    let mut screen = Offscreen::new(SIZE, 1.0);
    let windows = app.shared.state().windows.clone();
    windows[0]
        .window
        .lock()
        .unwrap()
        .session
        .state_mut::<zen_workbench::plugin_projects::PluginProjects>()
        .set(vec![plugin]);
    for _ in 0..3 {
        screen.step(Vec::new(), draw(&mut app));
    }
    {
        let mut window = windows[0].window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        let action = "view.toolWindow.maven";
        view.perform_action(screen.context(), &app.shared.registry, session, action, json!({}), false).unwrap();
    }
    for _ in 0..3 {
        screen.step(Vec::new(), draw(&mut app));
    }

    let tree = screen.accessibility_tree(draw(&mut app));
    let window = find(&tree, Role::Heading, "Maven").and_then(|heading| heading.bounds()).unwrap();
    let hide = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Button && node.label() == Some("Hide"))
        .filter_map(|(_, node)| node.bounds())
        .max_by(|one, other| one.x0.total_cmp(&other.x0))
        .unwrap();
    let last = center(&tree, Role::Button, "View Options");
    assert!(
        f64::from(last.x) + 12.0 <= hide.x0 && hide.x1 <= f64::from(SIZE.x),
        "the toolbar fits: {window:?} {hide:?}"
    );
    let head = center(&tree, Role::TreeItem, "shop");
    click(&mut screen, &mut app, head, PointerButton::Secondary);
    let tree = screen.accessibility_tree(draw(&mut app));
    assert!(find(&tree, Role::Button, "Show Effective POM").is_some(), "the project's menu is drawn");
    assert!(find(&tree, Role::Button, "Unlink Maven Projects").is_some());
    assert!(find(&tree, Role::Button, "Analyze Dependencies").is_some());
    screen.step(
        [true, false]
            .map(|pressed| Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            })
            .into(),
        draw(&mut app),
    );

    let tree = screen.accessibility_tree(draw(&mut app));
    click(&mut screen, &mut app, center(&tree, Role::TreeItem, "shop"), PointerButton::Primary);
    screen.step(vec![Event::Text("life".to_owned())], draw(&mut app));
    screen.step(Vec::new(), draw(&mut app));
    let maven = state(&app);
    assert_eq!(maven["search"], "life", "{maven}");
    assert_eq!(maven["selected"].as_str().map(|id| id.ends_with("/lifecycle")), Some(true), "{maven}");

    let tree = screen.accessibility_tree(draw(&mut app));
    click(&mut screen, &mut app, center(&tree, Role::Button, "Execute Maven Goal"), PointerButton::Primary);
    screen.step(vec![Event::Text("dependency:tr".to_owned())], draw(&mut app));
    screen.step(Vec::new(), draw(&mut app));
    let tree = screen.accessibility_tree(draw(&mut app));
    assert!(find(&tree, Role::Button, "dependency:tree").is_some(), "the goal is completed");
}

#[cfg(unix)]
fn fake_gradle(folder: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = r#"#!/bin/sh
for arg in "$@"; do
  case "$arg" in -Dzen.model.out=*) out="${arg#-Dzen.model.out=}" ;; esac
done
if [ -n "$out" ]; then
  mkdir -p "$out"
  cat > "$out/root.json" <<JSON
{"build":":","path":":","name":"shop","folder":"$PWD","buildFile":"$PWD/build.gradle","tasks":[{"name":"assemble","group":"build"}],"sourceSets":[],"configurations":[{"name":"compileClasspath","dependencies":[{"groupId":"org.a","artifactId":"a","version":"1"}]}]}
JSON
  cat > "$out/tools.json" <<JSON
{"build":":tools","path":":tools","name":"tools","folder":"$PWD/tools","tasks":[],"sourceSets":[],"configurations":[]}
JSON
fi
exit 0
"#;
    let gradle = folder.join("gradlew");
    std::fs::write(&gradle, script).unwrap();
    std::fs::set_permissions(&gradle, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn the_gradle_window_fills_after_import_and_is_used_by_mouse_and_keyboard_when_drawn() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    crate::keymap::load(Arc::new(Mutex::new(KeymapStore::new(None, Vec::new()))));
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("settings.gradle"), "rootProject.name = 'shop'\n").unwrap();
    std::fs::write(project.join("build.gradle"), "").unwrap();
    fake_gradle(&project);
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut app = IdeApp::new(product(), registry, Some(data.path().join("recent.json")), vec![project.clone()]);
    app.splash = super::super::splash::Splash::finished();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/gradle/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let plugin = zen_workbench::plugin_projects::PluginProject {
        id: "gradle".to_owned(),
        name: "Gradle".to_owned(),
        support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
    };
    let mut screen = Offscreen::new(SIZE, 1.0);
    let windows = app.shared.state().windows.clone();
    {
        let mut window = windows[0].window.lock().unwrap();
        window.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![plugin]);
        let grant = json!({ "path": project });
        app.shared.registry.dispatch(&mut window.session, "project.trust.grant", grant).unwrap();
    }
    for _ in 0..3 {
        screen.step(Vec::new(), draw(&mut app));
    }
    {
        let mut window = windows[0].window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        let action = "view.toolWindow.gradle";
        view.perform_action(screen.context(), &app.shared.registry, session, action, json!({}), false).unwrap();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let labels = |gradle: &Value| gradle["rows"].to_string();
    loop {
        screen.step(Vec::new(), draw(&mut app));
        let gradle = tool_state(&app, "gradle");
        if labels(&gradle).contains("\"Tasks\"") {
            assert!(labels(&gradle).contains("tools"), "{gradle}");
            assert!(labels(&gradle).contains("Dependencies"), "{gradle}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the tasks never came: {gradle}");
        {
            let mut window = windows[0].window.lock().unwrap();
            let ProjectWindow { session: _, view } = &mut *window;
            view.build_tools.stale(super::super::build_tool_window::Tool::Gradle);
        }
    }

    let tree = screen.accessibility_tree(draw(&mut app));
    let hide = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Button && node.label() == Some("Hide"))
        .filter_map(|(_, node)| node.bounds())
        .max_by(|one, other| one.x0.total_cmp(&other.x0))
        .unwrap();
    let last = center(&tree, Role::Button, "Collapse All");
    assert!(f64::from(last.x) + 12.0 <= hide.x0, "the toolbar fits: {hide:?}");
    for button in ["Reload All Gradle Projects", "Add Gradle Project", "Toggle Offline Mode", "Build Tool Settings"] {
        assert!(find(&tree, Role::Button, button).is_some(), "{button}");
    }

    click(&mut screen, &mut app, center(&tree, Role::TreeItem, "shop"), PointerButton::Secondary);
    let tree = screen.accessibility_tree(draw(&mut app));
    assert!(find(&tree, Role::Button, "Build Tool Settings").is_some(), "the project's menu is drawn");
    screen.step(
        [true, false]
            .map(|pressed| Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            })
            .into(),
        draw(&mut app),
    );

    let tree = screen.accessibility_tree(draw(&mut app));
    click(&mut screen, &mut app, center(&tree, Role::TreeItem, "shop"), PointerButton::Primary);
    screen.step(vec![Event::Text("depen".to_owned())], draw(&mut app));
    screen.step(Vec::new(), draw(&mut app));
    let gradle = tool_state(&app, "gradle");
    assert_eq!(gradle["search"], "depen", "{gradle}");
    assert_eq!(gradle["selected"], "@:/dependencies", "{gradle}");

    let tree = screen.accessibility_tree(draw(&mut app));
    click(&mut screen, &mut app, center(&tree, Role::Button, "Execute Gradle Task"), PointerButton::Primary);
    screen.step(vec![Event::Text("ass".to_owned())], draw(&mut app));
    screen.step(Vec::new(), draw(&mut app));
    let tree = screen.accessibility_tree(draw(&mut app));
    assert!(find(&tree, Role::Button, "assemble").is_some(), "the task is completed");
}

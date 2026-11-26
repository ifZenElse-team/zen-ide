use std::sync::mpsc;

use egui::{Pos2, Rect, ViewportEvent, ViewportInfo, WindowLevel, pos2, vec2};
use serde_json::{Value, json};

use super::*;

fn app(data: &Path, projects: Vec<PathBuf>) -> IdeApp {
    zen_i18n::register(crate::toolbox::LOCALES);
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let product = IdeProduct { name: "Test", id: "test", version: "0.0.0", ..IdeProduct::default() };
    IdeApp::new(product, registry, Some(data.join("recent.json")), projects)
}

fn project(data: &Path, name: &str) -> PathBuf {
    let project = data.join(name);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("main.go"), "package main\n").unwrap();
    project
}

fn frame_focused(ctx: &egui::Context, app: &mut IdeApp, focused: bool) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 820.0))),
        ..Default::default()
    };
    if !focused {
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { focused: Some(false), ..Default::default() });
    }
    let mut output = ctx.run_ui(input, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
    output
}

fn frame(ctx: &egui::Context, app: &mut IdeApp) -> egui::FullOutput {
    frame_focused(ctx, app, true)
}

fn run(ctx: &egui::Context, app: &mut IdeApp, command: &str, params: Value) -> Result<Value, String> {
    let (sender, answer) = mpsc::channel();
    app.route(
        ctx,
        "dispatch_command",
        &json!({ "command": command, "params": params }),
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
    answer.try_recv().unwrap_or_else(|_| panic!("no answer to {command}"))
}

fn state(ctx: &egui::Context, app: &mut IdeApp) -> Value {
    let (sender, answer) = mpsc::channel();
    app.route(
        ctx,
        "get_window_state",
        &json!({}),
        Box::new(move |result| {
            let _ = sender.send(result);
        }),
    );
    for _ in 0..5 {
        if let Ok(result) = answer.try_recv() {
            return result.unwrap();
        }
        frame(ctx, app);
    }
    panic!("no window state")
}

fn tool(state: &Value, id: &str) -> Value {
    state["toolWindows"]["windows"].as_array().unwrap().iter().find(|window| window["id"] == id).unwrap().clone()
}

fn draw_viewport(
    ctx: &egui::Context,
    output: &egui::FullOutput,
    viewport: ViewportId,
    outer: Rect,
    focused: bool,
    events: Vec<ViewportEvent>,
) {
    let callback = output.viewport_output[&viewport].viewport_ui_cb.clone().expect("a deferred viewport");
    let inner = Rect::from_min_max(outer.min + vec2(0.0, 28.0), outer.max);
    let mut input = egui::RawInput {
        viewport_id: viewport,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, inner.size())),
        ..Default::default()
    };
    input.viewports.insert(
        viewport,
        ViewportInfo {
            outer_rect: Some(outer),
            inner_rect: Some(inner),
            focused: Some(focused),
            events,
            ..Default::default()
        },
    );
    let mut drawn = ctx.run_ui(input, |ui| callback(ui));
    drawn.textures_delta.clear();
}

#[test]
fn moving_from_the_main_window_to_a_floating_tool_window_saves_nothing() {
    zen_i18n::register(zen_workbench::LOCALES);
    let data = tempfile::tempdir().unwrap();
    let project = project(data.path(), "project");
    let file = project.join("main.go");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut ide = app(data.path(), vec![project]);
    frame(&ctx, &mut ide);
    let project = ide.shared.state().windows[0].path.clone();
    run(&ctx, &mut ide, "view.toolWindow.viewMode", json!({ "id": "project", "mode": "float" })).unwrap();
    run(&ctx, &mut ide, "file.document.open", json!({ "path": file })).unwrap();
    run(&ctx, &mut ide, "editor.edit.insert", json!({ "text": "// typing\n" })).unwrap();
    frame(&ctx, &mut ide);
    let float = tool_viewport(&project, "project", ViewMode::Float);

    let later = |ctx: &egui::Context, ide: &mut IdeApp, events: Vec<egui::Event>| {
        let mut input = egui::RawInput { events, ..Default::default() };
        input.time = Some(ctx.input(|input| input.time) + 1.0);
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { focused: Some(false), ..Default::default() });
        let mut output = ctx.run_ui(input, |ui| {
            ide.logic(ui.ctx());
            ide.ui(ui);
        });
        output.textures_delta.clear();
    };
    later(&ctx, &mut ide, vec![egui::Event::WindowFocused(false)]);
    let mut in_float = egui::RawInput { viewport_id: float, ..Default::default() };
    in_float.viewports.insert(float, ViewportInfo { focused: Some(true), ..Default::default() });
    let mut output = ctx.run_ui(in_float, |_| {});
    output.textures_delta.clear();
    later(&ctx, &mut ide, Vec::new());
    later(&ctx, &mut ide, Vec::new());
    later(&ctx, &mut ide, Vec::new());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n", "the focus stayed in the IDE");
}

#[test]
fn float_and_window_open_viewports_of_their_own_that_keep_their_screen() {
    let data = tempfile::tempdir().unwrap();
    let project = project(data.path(), "project");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut ide = app(data.path(), vec![project.clone()]);
    frame(&ctx, &mut ide);
    let project = ide.shared.state().windows[0].path.clone();

    let done = run(&ctx, &mut ide, "view.toolWindow.viewMode", json!({ "id": "project", "mode": "float" })).unwrap();
    assert_eq!(done["result"], json!({ "id": "project", "mode": "float", "visible": true }));
    assert_eq!(done["window"]["left"], Value::Null, "Project left the left side");
    let float = tool_viewport(&project, "project", ViewMode::Float);
    let output = frame(&ctx, &mut ide);
    let viewport = &output.viewport_output[&float];
    assert!(matches!(viewport.class, egui::ViewportClass::Deferred), "a deferred viewport: an OS window of its own");
    assert_eq!(viewport.parent, ViewportId::ROOT);
    assert_eq!(viewport.builder.title.as_deref(), Some("Project"));
    assert_eq!(viewport.builder.window_level, Some(WindowLevel::AlwaysOnTop), "above the main window");
    assert_eq!(viewport.builder.taskbar, Some(false), "no Dock or taskbar entry of its own");
    assert_eq!(viewport.builder.minimize_button, Some(false));
    assert_eq!(viewport.builder.inner_size, Some(vec2(330.0, 520.0)));
    assert_eq!(viewport.builder.position, None, "where the system puts a new window");

    let second = Rect::from_min_size(pos2(2600.0, 140.0), vec2(420.0, 640.0));
    draw_viewport(&ctx, &output, float, second, true, Vec::new());
    let shown = state(&ctx, &mut ide);
    let bounds = json!({ "x": 2600.0, "y": 140.0, "width": 420.0, "height": 612.0 });
    assert_eq!(tool(&shown, "project")["bounds"], bounds, "{shown}");
    assert_eq!(tool(&shown, "project")["mode"], "float");
    let output = frame(&ctx, &mut ide);
    assert_eq!(
        output.viewport_output[&float].builder.position, None,
        "an open window keeps the place it opened with: it is never moved under the pointer"
    );

    draw_viewport(&ctx, &output, float, second, false, Vec::new());
    let output = frame_focused(&ctx, &mut ide, false);
    assert_eq!(output.viewport_output[&float].builder.window_level, Some(WindowLevel::Normal));
    let output = frame_focused(&ctx, &mut ide, true);
    assert_eq!(output.viewport_output[&float].builder.window_level, Some(WindowLevel::AlwaysOnTop));

    run(&ctx, &mut ide, "view.toolWindow.viewMode", json!({ "id": "project", "mode": "window" })).unwrap();
    let window = tool_viewport(&project, "project", ViewMode::Window);
    let output = frame(&ctx, &mut ide);
    assert!(!output.viewport_output.contains_key(&float), "the floating window closed");
    let viewport = &output.viewport_output[&window];
    assert!(matches!(viewport.class, egui::ViewportClass::Deferred));
    assert_eq!(viewport.builder.window_level, Some(WindowLevel::Normal), "a window like any other");
    assert_eq!(viewport.builder.taskbar, Some(true));
    assert_eq!(viewport.builder.position, Some(pos2(2600.0, 140.0)), "on the second screen");
    assert_eq!(viewport.builder.inner_size, Some(vec2(420.0, 612.0)));

    let left = Rect::from_min_size(pos2(-1500.0, 60.0), vec2(500.0, 700.0));
    draw_viewport(&ctx, &output, window, left, true, Vec::new());
    draw_viewport(&ctx, &output, window, left, true, vec![ViewportEvent::Close]);
    let shown = state(&ctx, &mut ide);
    assert_eq!(tool(&shown, "project")["visible"], false, "its close button hides the tool window");
    assert_eq!(tool(&shown, "project")["mode"], "window");
    let output = frame(&ctx, &mut ide);
    assert!(!output.viewport_output.contains_key(&window), "and its window closes");

    run(&ctx, &mut ide, "view.toolWindow.project", json!({})).unwrap();
    ide.remember_workspace(&ctx, true);
    let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
    assert!(saved.contains("\"mode\": \"window\"") && saved.contains("-1500"), "{saved}");
    drop(ide);
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut ide = app(data.path(), vec![project.clone()]);
    frame(&ctx, &mut ide);
    let output = frame(&ctx, &mut ide);
    let viewport = &output.viewport_output[&window];
    assert_eq!(viewport.builder.position, Some(pos2(-1500.0, 60.0)), "back on the left screen");
    assert_eq!(viewport.builder.inner_size, Some(vec2(500.0, 672.0)));
    assert_eq!(tool(&state(&ctx, &mut ide), "project")["mode"], "window");
}

#[test]
fn a_backend_with_one_window_draws_a_floating_tool_window_inside_the_main_one() {
    let data = tempfile::tempdir().unwrap();
    let project = project(data.path(), "project");
    let ctx = egui::Context::default();
    let mut ide = app(data.path(), vec![project.clone()]);
    frame(&ctx, &mut ide);
    let project = ide.shared.state().windows[0].path.clone();
    run(&ctx, &mut ide, "view.toolWindow.viewMode", json!({ "id": "project", "mode": "float" })).unwrap();
    let output = frame(&ctx, &mut ide);
    let float = tool_viewport(&project, "project", ViewMode::Float);
    assert!(!output.viewport_output.contains_key(&float), "no viewport: embedded");
    let shown = state(&ctx, &mut ide);
    assert_eq!(tool(&shown, "project")["visible"], true);
    assert_eq!(tool(&shown, "project")["bounds"], Value::Null, "an embedded window has no place on a screen");
}

#[test]
fn modes_are_kept_per_project_and_saved_layouts_per_ide() {
    let data = tempfile::tempdir().unwrap();
    let first = project(data.path(), "first");
    let second = project(data.path(), "second");
    let ctx = egui::Context::default();
    let mut ide = app(data.path(), vec![first.clone()]);
    frame(&ctx, &mut ide);
    run(&ctx, &mut ide, "view.toolWindow.viewMode", json!({ "id": "terminal", "mode": "undock" })).unwrap();
    run(&ctx, &mut ide, "view.toolWindow.viewMode", json!({ "id": "problems", "mode": "dockUnpinned" })).unwrap();
    let saved = run(&ctx, &mut ide, "view.layout.saveAsNew", json!({ "name": "Review" })).unwrap();
    assert_eq!(saved["result"], json!({ "name": "Review" }));
    frame(&ctx, &mut ide);
    frame(&ctx, &mut ide);
    let file = std::fs::read_to_string(data.path().join("window-layouts.json")).unwrap();
    assert!(file.contains("\"Review\"") && file.contains("\"undock\""), "{file}");
    run(&ctx, &mut ide, "view.layout.restoreDefault", json!({})).unwrap();
    run(&ctx, &mut ide, "view.toolWindow.viewMode", json!({ "id": "git", "mode": "window" })).unwrap();
    frame(&ctx, &mut ide);
    frame(&ctx, &mut ide);
    ide.remember_workspace(&ctx, true);
    drop(ide);

    let mut ide = app(data.path(), vec![second.clone()]);
    frame(&ctx, &mut ide);
    let shown = state(&ctx, &mut ide);
    assert_eq!(tool(&shown, "git")["mode"], "dockPinned", "the other project's modes are its own");
    assert_eq!(shown["toolWindows"]["layouts"], json!({ "saved": ["Review"], "current": "default" }));
    let applied = run(&ctx, &mut ide, "view.layout.apply", json!({ "name": "Review" })).unwrap();
    assert_eq!(applied["result"]["current"], "Review");
    let shown = state(&ctx, &mut ide);
    assert_eq!(tool(&shown, "terminal")["mode"], "undock", "the saved layout keeps the modes");
    assert_eq!(tool(&shown, "problems")["mode"], "dockUnpinned");
    run(&ctx, &mut ide, "view.layout.apply", json!({ "name": "default" })).unwrap();
    let shown = state(&ctx, &mut ide);
    assert_eq!(tool(&shown, "terminal")["mode"], "dockPinned", "Window → Layouts → Default resets them");
    assert_eq!(shown["toolWindows"]["layouts"]["current"], "default");
    drop(ide);

    let mut ide = app(data.path(), vec![first.clone()]);
    frame(&ctx, &mut ide);
    let shown = state(&ctx, &mut ide);
    assert_eq!(tool(&shown, "git")["mode"], "window", "kept per project with the layout");
    assert_eq!(tool(&shown, "terminal")["mode"], "dockPinned");
    run(&ctx, &mut ide, "view.layout.delete", json!({ "name": "Review" })).unwrap();
    frame(&ctx, &mut ide);
    frame(&ctx, &mut ide);
    let file = std::fs::read_to_string(data.path().join("window-layouts.json")).unwrap();
    assert!(!file.contains("Review"), "{file}");
}

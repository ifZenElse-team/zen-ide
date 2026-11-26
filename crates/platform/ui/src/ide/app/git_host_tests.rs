use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::tests::{ask, frame, themed_app, window_project};
use super::*;

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn repository(data: &Path) -> (PathBuf, PathBuf) {
    zen_vcs::git::set_environment(&[
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "Ada"),
        ("GIT_AUTHOR_EMAIL", "ada@example.com"),
        ("GIT_COMMITTER_NAME", "Ada"),
        ("GIT_COMMITTER_EMAIL", "ada@example.com"),
    ]);
    let (project, file) = window_project(data);
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "First"]);
    std::fs::write(&file, "package main\n\nfunc main() {}\n").unwrap();
    (project, file)
}

struct Fixture {
    ctx: egui::Context,
    app: IdeApp,
}

impl Fixture {
    fn new(data: &Path, project: &Path) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut app = themed_app(data, vec![project.to_owned()]);
        app.shared.settings.lock().unwrap().autosave_on_idle = false;
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        Self { ctx, app }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        ask(&self.ctx, &mut self.app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
    }

    fn frames(&mut self, count: usize) {
        for _ in 0..count {
            frame(&self.ctx, &mut self.app);
        }
    }

    fn git(&mut self) -> Value {
        self.run("workspace.host.state", json!({ "id": "git" }))
    }

    fn editor(&mut self) -> Value {
        ask(&self.ctx, &mut self.app, "get_window_state", json!({})).unwrap()["editor"].clone()
    }

    fn view<T>(&self, read: impl FnOnce(&IdeView) -> T) -> T {
        let windows = self.app.shared.state().windows.clone();
        let window = windows[0].window.lock().unwrap();
        read(&window.view)
    }
}

#[test]
fn the_git_workspace_expands_on_the_host_with_its_graph_pinned_and_its_tabs_inside() {
    let data = tempfile::tempdir().unwrap();
    let data = std::fs::canonicalize(data.path()).unwrap();
    let (project, file) = repository(&data);
    let mut fixture = Fixture::new(&data, &project);

    let opened = fixture.run("vcs.workspace.open", json!({}));
    assert_eq!(opened["open"], true, "vcs.workspace.open answers as before: {opened}");
    fixture.frames(2);
    let state = fixture.git();
    assert_eq!((state["expanded"].clone(), state["open"].clone()), (json!(true), json!(true)), "{state}");
    assert_eq!(state["active"], "graph", "{state}");
    assert_eq!(state["tabs"][0]["closable"], false, "the graph is pinned: {state}");
    assert_eq!(fixture.editor(), "gitWorkspace");

    fixture.run("vcs.workspace.select", json!({ "wip": true }));
    fixture.run("vcs.workspace.openDiff", json!({ "path": file, "part": "unstaged" }));
    fixture.frames(2);
    let state = fixture.git();
    assert_eq!(state["active"], "diff", "a diff opened from the workspace is its inner tab: {state}");
    assert_eq!(fixture.editor(), "diff");
    assert!(fixture.view(|view| view.diff_from_workspace), "not a tab of the main editor");

    fixture.run("workspace.tab.close", json!({ "workspace": "git", "tab": "diff" }));
    fixture.frames(2);
    assert!(fixture.view(|view| view.diff.is_none()), "closing the inner tab closes the diff");
    assert_eq!(fixture.git()["active"], "graph");

    fixture.run("workspace.host.collapse", json!({}));
    fixture.frames(1);
    let state = fixture.git();
    assert_eq!((state["expanded"].clone(), state["open"].clone()), (json!(false), json!(true)), "{state}");
    assert_ne!(fixture.editor(), "gitWorkspace", "the code is back");
    fixture.run("workspace.host.expand", json!({ "id": "git" }));
    fixture.frames(1);
    assert_eq!(fixture.editor(), "gitWorkspace", "expanding brings the graph back");

    fixture.app.remember_workspace(&fixture.ctx, true);
    drop(fixture);
    let mut fixture = Fixture::new(&data, &project);
    let deadline = Instant::now() + Duration::from_secs(30);
    while fixture.editor() != "gitWorkspace" && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        fixture.frames(1);
    }
    assert_eq!(fixture.editor(), "gitWorkspace", "a restart brings the workspace back expanded on its graph");
    let state = fixture.run("vcs.workspace.state", json!({}));
    assert_eq!(state["open"], true, "{state}");

    fixture.run("vcs.workspace.close", json!({}));
    fixture.frames(1);
    let state = fixture.git();
    assert_eq!((state["expanded"].clone(), state["open"].clone()), (json!(false), json!(false)), "{state}");
    assert_eq!(fixture.run("vcs.workspace.state", json!({}))["open"], false);
}

#[test]
fn snapshots_of_the_git_workspace_on_the_host_with_its_tab_in_the_editor_strip() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for theme in ["zen-dark", "zen-light"] {
        let data = tempfile::tempdir().unwrap();
        let data = std::fs::canonicalize(data.path()).unwrap();
        std::fs::write(data.join("settings.json"), format!(r#"{{"theme": "{theme}"}}"#)).unwrap();
        let (project, file) = repository(&data);
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 600.0), 1.5);
        let app = themed_app(&data, vec![project.clone()]);
        let mut fixture = Fixture { ctx: screen.context().clone(), app };
        fixture.frames(1);
        fixture.run("file.document.open", json!({ "path": file }));
        fixture.run("vcs.workspace.open", json!({}));
        let mut draw = |fixture: &mut Fixture| {
            screen.render(|ui| {
                fixture.app.logic(ui.ctx());
                fixture.app.ui(ui);
            })
        };
        let mut image = draw(&mut fixture);
        let deadline = Instant::now() + Duration::from_secs(30);
        while fixture.view(|view| view.git_workspace.panels.is_none()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            image = draw(&mut fixture);
        }
        assert!(fixture.view(|view| view.git_workspace.panels.is_some()), "the graph is drawn under the strip");
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(20));
            image = draw(&mut fixture);
        }
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("git-workspace-host-{theme}.png")), &image).unwrap();
        }
        fixture.run("workspace.host.collapse", json!({}));
        let mut image = draw(&mut fixture);
        for _ in 0..3 {
            image = draw(&mut fixture);
        }
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("git-workspace-host-collapsed-{theme}.png")), &image)
                .unwrap();
        }
    }
}

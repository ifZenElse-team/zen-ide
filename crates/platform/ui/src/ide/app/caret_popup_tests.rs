use std::collections::HashSet;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use egui::{Order, Rect, vec2};
use serde_json::{Value, json};

use super::*;

const TARGET_LINE: usize = 20;
const LAST_LINE: usize = 90;
const COLUMN: usize = "\tfmt.Println(".len();
const LONG_LINE: usize = 4;
const SIZE: egui::Vec2 = vec2(1100.0, 860.0);

fn long_line() -> String {
    format!("var _ = fmt.Sprint(\"{}\", fmt.Println)", "the caret goes to the end of this long line ".repeat(6))
}

fn source() -> String {
    let mut text = format!("package main\n\nimport \"fmt\"\n{}\nfunc main() {{\n", long_line());
    for line in 6..=LAST_LINE {
        text.push_str(&format!("\tfmt.Println({line})\n"));
    }
    text.push_str("}\n");
    text
}

fn offset_of(text: &str, line: usize, column: usize) -> usize {
    text.split_inclusive('\n').take(line - 1).map(|line| line.chars().count()).sum::<usize>() + column
}

struct Finding {
    start: usize,
}

impl zen_project::inspect::Inspector for Finding {
    fn id(&self) -> &str {
        "test"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name == "main.go"
    }

    fn inspect(&self, _context: &zen_project::inspect::InspectContext<'_>) -> Vec<zen_project::inspect::Finding> {
        let fix = |id: &str, title: &str| zen_project::inspect::InspectionFix {
            id: id.to_owned(),
            title: title.to_owned(),
            kind: zen_project::inspect::FixKind::Apply,
            batch: None,
            label: None,
            inline: false,
            args: json!({}),
        };
        vec![zen_project::inspect::Finding {
            start: self.start,
            end: self.start + 2,
            severity: zen_project::inspect::FindingSeverity::Warning,
            message: "A magic number".to_owned(),
            note: None,
            rows: Vec::new(),
            details: None,
            fixes: vec![fix("name", "Name the number"), fix("inline", "Inline the number")],
            source: "test".to_owned(),
            code: "magicNumber".to_owned(),
            deprecated: false,
            unused: false,
        }]
    }

    fn apply(
        &self,
        _context: &zen_project::inspect::InspectContext<'_>,
        _fixes: &[zen_project::inspect::InspectionFix],
    ) -> Result<Box<dyn zen_project::inspect::FixJob>, String> {
        Err("not in tests".to_owned())
    }
}

struct Window {
    app: IdeApp,
    screen: crate::offscreen::Offscreen,
    _server: zen_lsp::fake::FakeServer,
    text: String,
    _data: tempfile::TempDir,
}

impl Window {
    fn new(animated: bool, fixes: bool) -> Self {
        let mut window = Self::bare(animated, fixes);
        let file = window.file();
        window.call("dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }));
        for _ in 0..10 {
            window.step();
        }
        let end = offset_of(&window.text, LAST_LINE, COLUMN);
        window.command("editor.selection.set", json!({ "selections": [{ "anchor": end, "head": end }] }));
        for _ in 0..40 {
            window.step();
        }
        window
    }

    fn file(&self) -> PathBuf {
        self._data.path().join("project").join("main.go")
    }

    fn bare(animated: bool, fixes: bool) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let text = source();
        let file = project.join("main.go");
        std::fs::write(&file, &text).unwrap();
        let other = project.join("other.go");
        std::fs::write(&other, "package main\n\nfunc Println(a ...any) {}\n\nfunc Print(a ...any) {}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct {
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
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        };
        let app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let screen = crate::offscreen::Offscreen::new(SIZE, 2.0);
        if !animated {
            screen.context().all_styles_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
        }
        let server = zen_lsp::fake::FakeServer::new();
        server.set_capabilities(json!({
            "codeActionProvider": true,
            "completionProvider": { "triggerCharacters": ["."] },
            "signatureHelpProvider": { "triggerCharacters": ["(", ","] },
            "hoverProvider": true,
            "definitionProvider": true,
            "referencesProvider": true
        }));
        server.on(
            "textDocument/codeAction",
            json!([
                { "title": "Inline call to Println", "kind": "refactor.inline" },
                { "title": "Extract to a variable", "kind": "refactor.extract" }
            ]),
        );
        server.on(
            "textDocument/completion",
            Value::Array(
                [
                    "Println", "Print", "Printf", "Sprint", "Sprintf", "Sprintln", "Fprint", "Fprintf", "Fprintln",
                    "Errorf",
                ]
                .map(|label| json!({ "label": label, "kind": 3, "detail": "func(a ...any)" }))
                .to_vec(),
            ),
        );
        server.on(
            "textDocument/signatureHelp",
            json!({
                "signatures": [{ "label": "Println(a ...any)", "parameters": [{ "label": "a ...any" }] }],
                "activeSignature": 0,
                "activeParameter": 0
            }),
        );
        server.on("textDocument/hover", json!({
            "contents": { "kind": "markdown", "value": "```go\nfunc Println(a ...any)\n```\n\nPrintln prints its operands." }
        }));
        let uri = zen_lsp::position::file_uri(&other);
        let location = |line: u32| {
            json!({ "uri": uri, "range": {
                "start": { "line": line, "character": 5 }, "end": { "line": line, "character": 10 } } })
        };
        server.on("textDocument/definition", json!([location(2), location(4)]));
        server.on("textDocument/references", json!([location(2), location(4)]));
        let launched = server.clone();
        let start = offset_of(&text, TARGET_LINE, COLUMN);
        {
            let windows = app.shared.state().windows.clone();
            let mut window = windows[0].window.lock().unwrap();
            let session = &mut window.session;
            session.state_mut::<zen_workbench::language::Languages>().set_specs(vec![
                zen_workbench::language::ServerSpec {
                    language_id: "go".to_owned(),
                    extensions: vec!["go".to_owned()],
                    launch: Arc::new(move |_| Ok(launched.launcher())),
                    initialization_options: json!({}),
                },
            ]);
            if fixes {
                session
                    .state_mut::<zen_workbench::inspections::Inspections>()
                    .set_inspectors(vec![Arc::new(Finding { start })], None);
            }
        }
        let mut window = Self { app, screen, _server: server, text, _data: data };
        window.step();
        window
    }

    fn step(&mut self) {
        self.step_with(Vec::new());
    }

    fn step_with(&mut self, events: Vec<egui::Event>) {
        let Self { app, screen, .. } = self;
        screen.step(events, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
    }

    fn render(&mut self) -> crate::offscreen::Frame {
        let Self { app, screen, .. } = self;
        screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
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
                return result.unwrap_or_else(|error| panic!("{tool} {arguments}: {error}"));
            }
            self.step();
        }
        panic!("no answer to {tool} {arguments}");
    }

    fn command(&mut self, command: &str, params: Value) -> Value {
        self.call("dispatch_command", json!({ "command": command, "params": params }))
    }

    fn caret(&self) -> Option<Rect> {
        let windows = self.app.shared.state().windows.clone();
        let window = windows[0].window.lock().unwrap();
        window.view.editor_view.caret_place().rect()
    }

    fn foreground(&self) -> HashSet<(egui::Id, [u32; 4])> {
        self.screen.context().memory(|memory| {
            memory
                .layer_ids()
                .filter(|layer| layer.order == Order::Foreground && memory.areas().is_visible(layer))
                .filter_map(|layer| {
                    let rect = memory.area_rect(layer.id)?;
                    Some((layer.id, [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(f32::to_bits)))
                })
                .collect()
        })
    }

    fn popups(&self, before: &HashSet<(egui::Id, [u32; 4])>) -> Vec<(egui::Id, Rect)> {
        let ids: HashSet<egui::Id> = before.iter().map(|(id, _)| *id).collect();
        self.foreground()
            .into_iter()
            .filter(|(id, _)| !ids.contains(id))
            .map(|(id, rect)| {
                let [left, top, right, bottom] = rect.map(f32::from_bits);
                (id, Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom)))
            })
            .collect()
    }

    fn wait_for_fixes(&mut self) {
        let start = offset_of(&self.text, TARGET_LINE, COLUMN);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let fixes = self.command("problems.fixes.list", json!({ "offset": start }));
            if fixes.as_array().is_some_and(|fixes| !fixes.is_empty()) {
                return;
            }
            assert!(Instant::now() < deadline, "no fixes at the target: {fixes}");
            std::thread::sleep(Duration::from_millis(10));
            self.step();
        }
    }
}

fn next_to(popup: Rect, caret: Rect) -> bool {
    let below = (caret.bottom() - 1.0..=caret.bottom() + 12.0).contains(&popup.top());
    let above = (caret.top() - 12.0..=caret.top() + 1.0).contains(&popup.bottom());
    let beside = popup.left() <= caret.left() + 4.0 && popup.right() >= caret.left();
    (below || above) && beside
}

const POPUPS: [(&str, &str, &str); 7] = [
    ("intentions", "run_action", "view.intentions.open"),
    ("quick-fixes", "run_action", "view.intentions.open"),
    ("completion", "dispatch_command", "editor.completion.start"),
    ("parameter-info", "dispatch_command", "editor.parameterInfo.show"),
    ("quick-documentation", "dispatch_command", "editor.documentation.show"),
    ("declaration-targets", "dispatch_command", "navigate.declaration.goTo"),
    ("show-usages", "dispatch_command", "navigate.usages.show"),
];

fn open_after_jump(window: &mut Window, name: &str, tool: &str, action: &str) {
    let jump = offset_of(&window.text, TARGET_LINE, COLUMN);
    let arguments = match tool {
        "run_action" => json!({ "action": action, "params": {} }),
        _ => json!({ "command": action, "params": {} }),
    };
    let (sender, answer) = mpsc::channel();
    let ctx = window.screen.context().clone();
    window.app.route(
        &ctx,
        "dispatch_command",
        &json!({ "command": "editor.selection.set", "params": { "selections": [{ "anchor": jump, "head": jump }] } }),
        Box::new(|_| {}),
    );
    window.app.route(
        &ctx,
        tool,
        &arguments,
        Box::new(move |result| {
            let _ = sender.send(result);
        }),
    );
    if let Ok(result) = answer.try_recv() {
        result.unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn popups_at_the_caret_open_where_the_caret_scrolled_to() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for animated in [true, false] {
        for (name, tool, action) in POPUPS {
            let mut window = Window::new(animated, name == "quick-fixes");
            let start = window.caret().expect("the caret is drawn at the end of the file");
            if name == "quick-fixes" {
                window.wait_for_fixes();
            }
            let before = window.foreground();
            open_after_jump(&mut window, name, tool, action);
            let mut drawn = 0;
            let mut sized: HashSet<egui::Id> = HashSet::new();
            let deadline = Instant::now() + Duration::from_secs(10);
            for frame in 1.. {
                window.step();
                let popups = window.popups(&before);
                for (id, popup) in &popups {
                    if sized.insert(*id) {
                        continue;
                    }
                    let caret = window.caret();
                    assert!(
                        caret.is_some_and(|caret| caret != start && next_to(*popup, caret)),
                        "{name}, animated {animated}: frame {frame} drew the popup at {popup:?}, the caret at {caret:?} (before the jump {start:?})"
                    );
                }
                drawn += usize::from(!popups.is_empty());
                if drawn >= 20 || (frame >= 40 && drawn == 0 && Instant::now() > deadline) {
                    break;
                }
                if popups.is_empty() {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            assert!(drawn > 0, "{name}, animated {animated}: no popup opened");
            if let Some(dir) = &snapshots
                && animated
            {
                let image = window.render();
                crate::offscreen::write_png(&dir.join(format!("caret-popup-{name}.png")), &image).unwrap();
            }
            let caret = window.caret().expect("the caret is drawn after the jump");
            let popups = window.popups(&before);
            assert!(
                popups.len() == 1 && next_to(popups[0].1, caret),
                "{name}, animated {animated}: the popups {popups:?} are not at the caret {caret:?}"
            );
        }
    }
}

fn route(window: &mut Window, tool: &str, arguments: Value) {
    let ctx = window.screen.context().clone();
    window.app.route(&ctx, tool, &arguments, Box::new(|result| result.map(drop).unwrap()));
}

fn select(window: &mut Window, line: usize, column: usize) {
    let at = offset_of(&window.text, line, column);
    let selections = json!({ "selections": [{ "anchor": at, "head": at }] });
    route(window, "dispatch_command", json!({ "command": "editor.selection.set", "params": selections }));
}

fn open(window: &mut Window, tool: &str, action: &str) {
    match tool {
        "run_action" => route(window, tool, json!({ "action": action, "params": {} })),
        _ => route(window, tool, json!({ "command": action, "params": {} })),
    }
}

fn inside(popup: Rect, screen: egui::Vec2) -> bool {
    Rect::from_min_size(egui::Pos2::ZERO, screen).expand(0.5).contains_rect(popup)
}

fn snapshot(window: &mut Window, name: &str) {
    if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) {
        let image = window.render();
        crate::offscreen::write_png(&dir.join(format!("caret-popup-{name}.png")), &image).unwrap();
    }
}

fn settle(window: &mut Window, before: &HashSet<(egui::Id, [u32; 4])>, screen: egui::Vec2, what: &str) -> (Rect, Rect) {
    let mut sized: HashSet<egui::Id> = HashSet::new();
    let mut drawn = 0;
    for frame in 0..400 {
        window.step();
        let popups = window.popups(before);
        for (id, popup) in &popups {
            if sized.insert(*id) {
                continue;
            }
            let caret = window.caret();
            assert!(
                caret.is_some_and(|caret| next_to(*popup, caret)) && inside(*popup, screen),
                "{what}: frame {frame} drew the popup at {popup:?}, the caret at {caret:?}"
            );
        }
        drawn += usize::from(!popups.is_empty());
        if drawn >= 10 {
            break;
        }
        if popups.is_empty() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let popups = window.popups(before);
    assert_eq!(popups.len(), 1, "{what}: {popups:?}");
    (popups[0].1, window.caret().unwrap())
}

const EDGE_POPUPS: [(&str, &str, &str); 4] = [
    ("intentions", "run_action", "view.intentions.open"),
    ("completion", "dispatch_command", "editor.completion.start"),
    ("declaration-targets", "dispatch_command", "navigate.declaration.goTo"),
    ("show-usages", "dispatch_command", "navigate.usages.show"),
];

#[test]
fn popups_at_the_caret_flip_above_it_at_the_bottom_and_move_left_at_the_right() {
    let mut flipped = Vec::new();
    for (name, tool, action) in EDGE_POPUPS {
        let mut window = Window::new(false, false);
        let caret = window.caret().unwrap();
        assert!(caret.bottom() > SIZE.y - 200.0, "the caret is at the bottom of the window: {caret:?}");
        let before = window.foreground();
        select(&mut window, LAST_LINE, "\tfmt.Print".len());
        open(&mut window, tool, action);
        let (popup, caret) = settle(&mut window, &before, SIZE, &format!("{name} at the bottom"));
        let above = popup.bottom() <= caret.top() + 1.0;
        assert!(above || popup.top() >= caret.bottom() - 1.0, "{name}: {popup:?} covers the caret {caret:?}");
        assert!(above || popup.height() < SIZE.y - caret.bottom(), "{name}: {popup:?} has room below {caret:?}");
        flipped.push((name, above));
        snapshot(&mut window, &format!("{name}-bottom"));

        let mut window = Window::new(false, false);
        let before = window.foreground();
        select(&mut window, LONG_LINE, long_line().len() - 1);
        open(&mut window, tool, action);
        let (popup, caret) = settle(&mut window, &before, SIZE, &format!("{name} at the right"));
        assert!(caret.left() > popup.left() + 40.0, "{name}: {popup:?} moved left of the caret {caret:?}");
        snapshot(&mut window, &format!("{name}-right"));
    }
    assert!(flipped.iter().any(|(_, above)| *above), "a tall popup goes above the caret: {flipped:?}");
}

#[test]
fn a_popup_opens_in_the_group_of_its_editor() {
    let mut window = Window::new(false, false);
    window.command("editor.split.right", json!({}));
    for _ in 0..10 {
        window.step();
    }
    let mut carets = Vec::new();
    for (side, group) in [("right", 2), ("left", 1)] {
        assert_eq!(window.command("editor.split.list", json!({}))["focused"], group);
        let before = window.foreground();
        select(&mut window, TARGET_LINE, COLUMN);
        open(&mut window, "run_action", "view.intentions.open");
        let (_, caret) = settle(&mut window, &before, SIZE, &format!("the {side} group"));
        carets.push(caret);
        snapshot(&mut window, &format!("split-{side}"));
        window.step_with(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        window.command("editor.split.previous", json!({}));
        for _ in 0..10 {
            window.step();
        }
    }
    assert!(carets[0].left() - carets[1].left() > 200.0, "each popup at the caret of its own group: {carets:?}");
}

#[test]
fn a_popup_follows_the_caret_into_wrapped_and_folded_text() {
    let mut window = Window::new(false, false);
    window.command("editor.softWrap.toggle", json!({}));
    for _ in 0..10 {
        window.step();
    }
    let before = window.foreground();
    select(&mut window, LONG_LINE, long_line().len());
    open(&mut window, "dispatch_command", "editor.completion.start");
    let (_, caret) = settle(&mut window, &before, SIZE, "the end of a wrapped line");
    assert!(caret.right() < SIZE.x - 100.0, "the line wraps: {caret:?}");
    snapshot(&mut window, "soft-wrap");

    let mut window = Window::new(false, false);
    window.command(
        "editor.selection.set",
        json!({ "selections": [{ "head": offset_of(&window.text, TARGET_LINE, COLUMN) }] }),
    );
    for _ in 0..10 {
        window.step();
    }
    let before = window.foreground();
    route(&mut window, "dispatch_command", json!({ "command": "editor.fold.collapse", "params": {} }));
    open(&mut window, "run_action", "view.intentions.open");
    let (_, caret) = settle(&mut window, &before, SIZE, "a caret moved out of a fold");
    snapshot(&mut window, "fold-collapsed");
    window.step_with(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    for _ in 0..5 {
        window.step();
    }
    let before = window.foreground();
    select(&mut window, 60, COLUMN);
    open(&mut window, "run_action", "view.intentions.open");
    let (_, inside_fold) = settle(&mut window, &before, SIZE, "a caret moved into a fold");
    assert_ne!(inside_fold, caret, "the fold opened and the caret moved");
    snapshot(&mut window, "fold-revealed");
}

#[test]
fn a_popup_follows_the_caret_when_the_font_the_zoom_or_the_window_change() {
    let mut window = Window::new(false, false);
    let before = window.foreground();
    select(&mut window, TARGET_LINE, COLUMN);
    open(&mut window, "run_action", "view.intentions.open");
    let (_, first) = settle(&mut window, &before, SIZE, "opened");
    window.call("set_settings", json!({ "editorFont": { "size": 22 } }));
    let (_, bigger) = settle(&mut window, &before, SIZE, "a larger editor font");
    assert_ne!(first, bigger, "the caret moved with the font");
    snapshot(&mut window, "font-size");
    window.call("set_settings", json!({ "uiFontSize": 18 }));
    let (_, zoomed) = settle(&mut window, &before, SIZE, "a larger interface");
    assert_ne!(bigger, zoomed, "the caret moved with the zoom");
    snapshot(&mut window, "zoom");
    let small = vec2(700.0, 520.0);
    window.screen.set_size(small);
    let (popup, _) = settle(&mut window, &before, small, "a smaller window");
    assert!(inside(popup, small));
    snapshot(&mut window, "window-resized");
}

#[test]
fn a_popup_never_stays_hidden_when_its_caret_is_off_the_screen() {
    let wheel = |dy: f32| {
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: egui::Modifiers::NONE,
        }]
    };
    let mut window = Window::new(false, false);
    for _ in 0..200 {
        window.step();
    }
    let before = window.foreground();
    select(&mut window, TARGET_LINE, COLUMN);
    open(&mut window, "run_action", "view.intentions.open");
    let (popup, caret) = settle(&mut window, &before, SIZE, "opened");
    window.step_with(vec![egui::Event::PointerMoved(egui::pos2(700.0, 200.0))]);
    window.step_with(wheel(-3000.0));
    for _ in 0..10 {
        window.step();
    }
    let away = window.caret().unwrap();
    assert_eq!(away.size(), egui::Vec2::ZERO, "the caret left the screen: {away:?}");
    let popups = window.popups(&before);
    assert!(popups.len() == 1 && next_to(popups[0].1, away), "the popup waits in the middle of the text: {popups:?}");
    snapshot(&mut window, "scrolled-away");
    window.step_with(wheel(3000.0));
    let (back, again) = settle(&mut window, &before, SIZE, "scrolled back");
    assert_eq!((back, again), (popup, caret), "the popup is back at the caret");
    window.step_with(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    window.step_with(wheel(-3000.0));
    for _ in 0..10 {
        window.step();
    }
    let before = window.foreground();
    open(&mut window, "run_action", "view.intentions.open");
    let (_, away) = settle(&mut window, &before, SIZE, "opened with the caret off the screen");
    assert_eq!(away.size(), egui::Vec2::ZERO, "the editor did not scroll: {away:?}");
    snapshot(&mut window, "opened-away");
}

#[test]
fn a_selection_set_before_the_first_frame_scrolls_to_the_caret() {
    let mut window = Window::bare(false, false);
    let file = window.file();
    let before = window.foreground();
    route(&mut window, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }));
    select(&mut window, 70, COLUMN);
    open(&mut window, "run_action", "view.intentions.open");
    let (_, caret) = settle(&mut window, &before, SIZE, "opened with the file");
    assert!(
        caret.height() > 0.0 && caret.top() > 0.0 && caret.bottom() < SIZE.y,
        "the caret is on the screen: {caret:?}"
    );
    snapshot(&mut window, "first-frame");
}

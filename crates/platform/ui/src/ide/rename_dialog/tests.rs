use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use zen_lsp::fake::FakeServer;
use zen_workbench::language::{Languages, ServerSpec};

use super::*;
use crate::ide::find_window::{FindAction, FindWindow};
use crate::ide::refactoring_preview::{PreviewChange, PreviewFile, PreviewOperation, PreviewRow};

const MAIN: &str = "package main\n\nfunc main() { greet() }\n";
const LIB: &str = "package main\n\nfunc greet() {}\n";
const CALL: usize = 30;

struct Harness {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    server: FakeServer,
    dialog: RenameDialog,
    find: FindWindow,
    outcomes: Vec<RenameOutcome>,
    actions: Vec<FindAction>,
    dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.go"), MAIN).unwrap();
        std::fs::write(dir.path().join("lib.go"), LIB).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let server = FakeServer::new();
        server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
            "renameProvider": { "prepareProvider": true }
        }));
        let launched = server.clone();
        session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(move |_| Ok(launched.launcher())),
            initialization_options: json!({}),
        }]);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut harness = Self {
            ctx,
            registry,
            session,
            server,
            dialog: RenameDialog::default(),
            find: FindWindow::default(),
            outcomes: Vec::new(),
            actions: Vec::new(),
            dir,
        };
        let main = harness.path("main.go");
        harness.registry.dispatch(&mut harness.session, "file.document.open", json!({ "path": main })).unwrap();
        harness.wait(|harness| {
            let _ = harness.registry.dispatch(&mut harness.session, "language.servers.sync", json!({}));
            harness.session.state::<Languages>().is_some_and(|languages| languages.synced_text(&main).is_some())
        });
        harness
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn uri(&self, name: &str) -> String {
        zen_lsp::position::file_uri(&self.path(name))
    }

    fn answer_rename(&self) {
        let range = |character: u64| json!({ "start": { "line": 2, "character": character }, "end": { "line": 2, "character": character + 5 } });
        self.server.on("textDocument/prepareRename", json!({ "range": range(14), "placeholder": "greet" }));
        let (main, lib) = (self.uri("main.go"), self.uri("lib.go"));
        self.server.on_with("textDocument/rename", move |params| {
            let name = params["newName"].clone();
            json!({ "changes": {
                main.clone(): [{ "range": range(14), "newText": name }],
                lib.clone(): [{ "range": range(5), "newText": name }]
            } })
        });
    }

    fn frame(&mut self, events: Vec<Event>) {
        let input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
            ..Default::default()
        };
        let Self { ctx, registry, session, dialog, find, outcomes, actions, .. } = self;
        let mut output = ctx.run_ui(input, |ui| {
            outcomes.extend(dialog.update(ui, registry, session, &IdePalette::zen_dark()));
            actions.extend(find.show(ui, &IdePalette::zen_dark()));
        });
        output.textures_delta.clear();
    }

    fn wait(&mut self, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !done(self) {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn open_dialog(&mut self) {
        self.dialog.open(self.path("main.go"), CALL);
        self.wait(|harness| {
            harness.frame(Vec::new());
            matches!(harness.dialog.state, Some(State::Open(_)))
        });
        self.frame(Vec::new());
    }

    fn form(&self) -> &Form {
        let Some(State::Open(form)) = &self.dialog.state else { panic!("the dialog is not open") };
        form
    }

    fn key(&mut self, key: Key) {
        let event =
            |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![event(true)]);
        self.frame(vec![event(false)]);
    }

    fn click(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerMoved(at)]);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
    }

    fn settle(&mut self) {
        self.wait(|harness| {
            harness.frame(Vec::new());
            !matches!(&harness.dialog.state, Some(State::Open(form)) if form.waiting.is_some())
        });
    }

    fn document_text(&self, name: &str) -> String {
        let path = self.path(name);
        let workbench = self.session.state::<Workbench>().unwrap();
        workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
    }
}

#[test]
fn enter_renames_with_the_typed_name_across_the_project() {
    let mut harness = Harness::new();
    harness.answer_rename();
    harness.open_dialog();
    assert_eq!((harness.form().name.as_str(), harness.form().text.as_str()), ("greet", "greet"));
    harness.frame(vec![Event::Text("hello".to_owned())]);
    assert_eq!(harness.form().text, "hello", "the placeholder is selected, so typing replaces it");
    harness.key(Key::Enter);
    harness.settle();
    assert!(harness.dialog.state.is_none(), "the dialog closes");
    assert!(matches!(harness.outcomes.as_slice(), [RenameOutcome::Closed]), "{:?}", harness.outcomes);
    let sent = harness.server.requests("textDocument/rename");
    assert_eq!(sent.last().unwrap()["newName"], "hello");
    assert_eq!(harness.document_text("main.go"), MAIN.replace("greet", "hello"));
    assert_eq!(std::fs::read_to_string(harness.path("lib.go")).unwrap(), LIB.replace("greet", "hello"));
}

#[test]
fn an_invalid_name_is_shown_in_the_dialog_and_escape_cancels() {
    let mut harness = Harness::new();
    harness.answer_rename();
    harness.open_dialog();
    harness.frame(vec![Event::Text("1st".to_owned())]);
    harness.key(Key::Enter);
    harness.settle();
    assert!(harness.form().error.is_some(), "the preview's validation error is shown in the dialog");
    assert!(harness.outcomes.is_empty());
    harness.key(Key::Escape);
    assert!(harness.dialog.state.is_none(), "Escape closes the dialog");
    assert!(matches!(harness.outcomes.as_slice(), [RenameOutcome::Closed]));
    assert!(harness.server.requests("textDocument/rename").is_empty(), "nothing was renamed");
    assert_eq!(harness.document_text("main.go"), MAIN);
}

#[test]
fn a_prepare_error_is_a_notice_instead_of_the_dialog() {
    let mut harness = Harness::new();
    harness.server.on("textDocument/prepareRename", Value::Null);
    harness.dialog.open(harness.path("main.go"), CALL);
    harness.wait(|harness| {
        harness.frame(Vec::new());
        harness.dialog.state.is_none()
    });
    assert!(
        matches!(harness.outcomes.as_slice(), [RenameOutcome::Notice(message)] if *message == t!("workbench-rename-not-here"))
    );
}

#[test]
fn preview_opens_the_refactoring_preview_and_do_refactor_applies_it() {
    let mut harness = Harness::new();
    harness.answer_rename();
    harness.open_dialog();
    harness.frame(vec![Event::Text("hello".to_owned())]);
    let preview = harness.form().buttons[2];
    harness.click(preview.center());
    harness.settle();
    let Some(RenameOutcome::Preview(tab)) = harness.outcomes.pop() else { panic!("{:?}", harness.outcomes) };
    assert!(harness.dialog.state.is_none(), "the dialog closes for the preview");
    harness.find.show_refactoring(*tab);
    harness.frame(Vec::new());
    harness.frame(Vec::new());
    let tab = harness.find.refactoring().unwrap();
    let rows: Vec<PreviewRow> = tab.drawn().1.iter().map(|(row, _)| *row).collect();
    assert_eq!(
        rows,
        [
            PreviewRow::Usages,
            PreviewRow::File(0),
            PreviewRow::Change(0, 0),
            PreviewRow::File(1),
            PreviewRow::Change(1, 0)
        ]
    );
    let relative: Vec<&str> = tab.preview.files.iter().map(|file| file.relative.as_str()).collect();
    assert_eq!(relative, ["lib.go", "main.go"]);
    assert_eq!(harness.document_text("main.go"), MAIN, "a preview changes nothing");

    let refactor = tab.drawn().2[0];
    harness.click(refactor.center());
    let action = harness.actions.pop().unwrap();
    let (command, params) = action.command().unwrap();
    assert_eq!((command, &params["newName"]), ("refactor.rename.apply", &json!("hello")));
    harness.registry.dispatch(&mut harness.session, command, params).unwrap();
    assert_eq!(harness.document_text("main.go"), MAIN.replace("greet", "hello"));
    assert_eq!(harness.server.requests("textDocument/rename").len(), 1, "the previewed rename is applied");
}

fn preview_tab(path: &std::path::Path) -> PreviewTab {
    let change = |start, line| PreviewChange {
        path: path.to_owned(),
        start,
        line,
        preview: Some("greet()".to_owned()),
        highlight: Some([0, 5]),
        new_text: "hello".to_owned(),
    };
    let file = PreviewFile {
        path: path.to_owned(),
        relative: "main.go".to_owned(),
        changes: vec![change(14, 3), change(40, 5)],
    };
    let preview = RenamePreview { pending: false, files: vec![file], usage_count: 2, ..RenamePreview::default() };
    PreviewTab::new("hello".to_owned(), path.to_owned(), 14, preview)
}

#[test]
fn the_files_a_rename_creates_renames_or_deletes_are_listed_after_the_usages() {
    let mut harness = Harness::new();
    let main = harness.path("main.go");
    let mut tab = preview_tab(&main);
    tab.preview.operations = vec![
        PreviewOperation {
            kind: "rename".to_owned(),
            path: harness.path("lib.go"),
            relative: "lib.go".to_owned(),
            new_relative: Some("hello.go".to_owned()),
        },
        PreviewOperation {
            kind: "delete".to_owned(),
            path: harness.path("old.go"),
            relative: "old.go".to_owned(),
            new_relative: None,
        },
    ];
    assert_eq!(tab.preview.operations[0].label(), t!("ide-refactoring-file-rename", from = "lib.go", to = "hello.go"));
    harness.find.show_refactoring(tab);
    harness.frame(Vec::new());
    harness.frame(Vec::new());
    let tab = harness.find.refactoring().unwrap();
    let rows: Vec<PreviewRow> = tab.drawn().1.iter().map(|(row, _)| *row).collect();
    assert_eq!(
        rows,
        [
            PreviewRow::Usages,
            PreviewRow::File(0),
            PreviewRow::Change(0, 0),
            PreviewRow::Change(0, 1),
            PreviewRow::Operations,
            PreviewRow::Operation(0),
            PreviewRow::Operation(1)
        ]
    );
    let row = tab.drawn().1.iter().find(|(row, _)| *row == PreviewRow::Operation(0)).unwrap().1;
    harness.click(row.center());
    assert!(harness.actions.is_empty(), "a file operation has no place to go to");
}

#[test]
fn a_row_click_goes_to_the_change_and_cancel_forgets_the_rename() {
    let mut harness = Harness::new();
    let main = harness.path("main.go");
    harness.find.show_refactoring(preview_tab(&main));
    harness.frame(Vec::new());
    harness.frame(Vec::new());
    let tab = harness.find.refactoring().unwrap();
    assert_eq!(tab.drawn().0, Some(PreviewRow::Change(0, 0)), "the first change is selected");
    let row = tab.drawn().1.iter().find(|(row, _)| *row == PreviewRow::Change(0, 1)).unwrap().1;
    let cancel = tab.drawn().2[1];
    harness.click(row.center());
    let go = FindAction::GoTo { path: main.clone(), offset: 40, focus_editor: false };
    assert_eq!(harness.actions, std::slice::from_ref(&go));
    let (command, params) = go.command().unwrap();
    assert_eq!((command, params), ("navigate.place.goTo", json!({ "path": main, "offset": 40 })));

    harness.click(cancel.center());
    let cancelled = harness.actions.last().unwrap();
    assert!(matches!(cancelled, FindAction::CancelRefactoring { .. }), "{cancelled:?}");
    assert_eq!(cancelled.command().unwrap().0, "refactor.rename.cancel");
    assert!(harness.find.close_refactoring(), "no tab is left");
    assert!(harness.find.refactoring().is_none());
}

#[test]
fn the_actions_name_their_commands() {
    let path = PathBuf::from("/p/main.go");
    let (command, params) = RenameAction::Refactor("hello".to_owned()).command(&path, 3);
    assert_eq!((command, params), ("refactor.rename.apply", json!({ "newName": "hello", "path": path, "offset": 3 })));
    assert_eq!(RenameAction::Preview("x".to_owned()).command(&path, 3).0, "refactor.rename.preview");
    assert_eq!(RenameAction::Cancel.command(&path, 3).0, "refactor.rename.cancel");
}

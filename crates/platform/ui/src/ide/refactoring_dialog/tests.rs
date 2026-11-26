use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2};
use zen_core::CommandError;
use zen_lsp::fake::FakeServer;
use zen_workbench::Workbench;
use zen_workbench::language::refactoring_dialog::{
    RefactoringApplied, RefactoringForm, RefactoringPlan, RefactoringProvider, RefactoringProviders, TableColumn,
    Values,
};
use zen_workbench::language::{Languages, ServerSpec};

use super::*;
use crate::ide::find_window::{FindAction, FindWindow};

const MAIN: &str = "package main\n\nfunc main() { greet() }\n";
const LIB: &str = "package main\n\n// greet says hello.\nfunc greet() {}\n\nfunc other() {}\n";
const LIB_WITHOUT_GREET: &str = "package main\n\nfunc other() {}\n";

struct Harness {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    _server: FakeServer,
    dialog: RefactoringDialog,
    find: FindWindow,
    outcomes: Vec<RefactoringOutcome>,
    actions: Vec<FindAction>,
    dir: tempfile::TempDir,
}

fn range(line: u64, start: u64, end: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

impl Harness {
    fn new(used: bool) -> Self {
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
            "documentSymbolProvider": true,
            "referencesProvider": true
        }));
        server.on(
            "textDocument/documentSymbol",
            json!([{ "name": "greet", "kind": 12, "range": range(3, 0, 15), "selectionRange": range(3, 5, 10) }]),
        );
        let main = zen_lsp::position::file_uri(&dir.path().join("main.go"));
        let usages = if used { json!([{ "uri": main, "range": range(2, 14, 19) }]) } else { json!([]) };
        server.on("textDocument/references", usages);
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
            _server: server,
            dialog: RefactoringDialog::default(),
            find: FindWindow::default(),
            outcomes: Vec::new(),
            actions: Vec::new(),
            dir,
        };
        let lib = harness.path("lib.go");
        harness.registry.dispatch(&mut harness.session, "file.document.open", json!({ "path": lib })).unwrap();
        harness.wait(|harness| {
            let _ = harness.registry.dispatch(&mut harness.session, "language.servers.sync", json!({}));
            harness.session.state::<Languages>().is_some_and(|languages| languages.synced_text(&lib).is_some())
        });
        harness
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
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

    fn safe_delete(&mut self) {
        let offset = LIB.find("greet()").unwrap() + 2;
        self.dialog.open("safeDelete".to_owned(), json!({ "path": self.path("lib.go"), "offset": offset }));
    }

    fn settle(&mut self) {
        self.wait(|harness| {
            harness.frame(Vec::new());
            match &harness.dialog.state {
                None | Some(State::Conflicts(..)) => true,
                Some(State::Open(form)) => form.shown && form.waiting.is_none(),
                Some(State::Loading { .. }) => false,
            }
        });
        self.frame(Vec::new());
        self.frame(Vec::new());
    }

    fn button(&self, key: &str) -> Rect {
        match &self.dialog.state {
            Some(State::Open(form) | State::Conflicts(form, _)) => form.buttons[key],
            None | Some(State::Loading { .. }) => panic!("no dialog"),
        }
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

    fn lib_text(&self) -> String {
        let path = self.path("lib.go");
        let workbench = self.session.state::<Workbench>().unwrap();
        workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
    }
}

#[test]
fn safe_delete_of_an_unused_declaration_deletes_it_without_a_dialog() {
    let mut harness = Harness::new(false);
    harness.safe_delete();
    harness.settle();
    assert!(harness.dialog.state.is_none());
    assert!(matches!(harness.outcomes.as_slice(), [RefactoringOutcome::Closed]), "{:?}", harness.outcomes);
    assert_eq!(harness.lib_text(), LIB_WITHOUT_GREET);
}

#[test]
fn usages_are_listed_as_conflicts_and_delete_anyway_deletes() {
    let mut harness = Harness::new(true);
    harness.safe_delete();
    harness.settle();
    let Some(State::Conflicts(form, plan)) = &harness.dialog.state else { panic!("no conflicts") };
    assert_eq!(form.spec.conflicts_title, t!("workbench-safe-delete-usages-detected"));
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].relative.as_deref(), Some("main.go"));
    assert_eq!(harness.lib_text(), LIB, "nothing is deleted while the usages are shown");
    harness.click(harness.button("anyway").center());
    assert!(harness.dialog.state.is_none());
    assert_eq!(harness.lib_text(), LIB_WITHOUT_GREET);
}

#[test]
fn cancel_keeps_the_declaration() {
    let mut harness = Harness::new(true);
    harness.safe_delete();
    harness.settle();
    harness.click(harness.button("cancel").center());
    assert!(harness.dialog.state.is_none());
    assert!(matches!(harness.outcomes.as_slice(), [RefactoringOutcome::Closed]));
    assert_eq!(harness.lib_text(), LIB);
}

#[test]
fn view_usages_lists_them_in_the_find_tool_window_where_do_refactor_deletes_anyway() {
    let mut harness = Harness::new(true);
    harness.safe_delete();
    harness.settle();
    harness.click(harness.button("view").center());
    let Some(RefactoringOutcome::Preview(tab)) = harness.outcomes.pop() else { panic!("{:?}", harness.outcomes) };
    assert!(tab.conflicts);
    assert_eq!(tab.preview.files[0].relative, "main.go");
    assert_eq!(tab.preview.files[0].changes[0].line, 3);
    harness.find.show_refactoring(*tab);
    harness.frame(Vec::new());
    harness.frame(Vec::new());
    let refactor = harness.find.refactoring().unwrap().drawn().2[0];
    harness.click(refactor.center());
    let Some(FindAction::Refactor { command, params }) = harness.actions.pop() else { panic!("{:?}", harness.actions) };
    assert_eq!((command.as_str(), &params["force"]), ("refactor.dialog.apply", &json!(true)));
    harness.registry.dispatch(&mut harness.session, &command, params).unwrap();
    assert_eq!(harness.lib_text(), LIB_WITHOUT_GREET);
}

struct Moving;

impl RefactoringProvider for Moving {
    fn id(&self) -> &str {
        "test.move"
    }

    fn title(&self) -> String {
        "Move".to_owned()
    }

    fn form(&self, _: &mut Session, _: &Value, _: Duration) -> Result<RefactoringForm, CommandError> {
        let field = |id: &str, kind, value| RefactoringField {
            id: id.to_owned(),
            label: id.to_owned(),
            kind,
            value,
            options: Vec::new(),
            columns: Vec::new(),
            new_row: Value::Null,
            fixed: false,
        };
        Ok(RefactoringForm {
            title: self.title(),
            fields: vec![field("to", FieldKind::Text, json!("pkg")), field("all", FieldKind::Checkbox, json!(false))],
            ..RefactoringForm::default()
        })
    }

    fn plan(&self, _: &mut Session, _: &Value, values: &Values, _: Duration) -> Result<RefactoringPlan, CommandError> {
        let error = (values.get("to") == Some(&json!("bad"))).then(|| "bad target".to_owned());
        Ok(RefactoringPlan { usage_count: 2, error, ..RefactoringPlan::default() })
    }

    fn apply(&self, _: &mut Session, _: &Value, _: &Values, _: Duration) -> Result<RefactoringApplied, CommandError> {
        Ok(RefactoringApplied::default())
    }

    fn cancel(&self, _: &mut Session) {}
}

#[test]
fn a_plugin_refactoring_shows_its_fields_an_error_and_its_preview() {
    let mut harness = Harness::new(false);
    harness.session.state_mut::<RefactoringProviders>().add(Arc::new(Moving));
    harness.dialog.open("test.move".to_owned(), Value::Null);
    harness.settle();
    let Some(State::Open(form)) = &harness.dialog.state else { panic!("no form") };
    assert_eq!(form.values["to"], "pkg");
    harness.frame(vec![Event::Text("bad".to_owned())]);
    harness.click(harness.button("refactor").center());
    harness.settle();
    let Some(State::Open(form)) = &harness.dialog.state else { panic!("no form") };
    assert_eq!(form.error.as_deref(), Some("bad target"), "the plan's error is shown in the dialog");
    harness.frame(vec![Event::Text("ok".to_owned())]);
    harness.frame(Vec::new());
    harness.frame(Vec::new());
    harness.click(harness.button("preview").center());
    harness.settle();
    let Some(RefactoringOutcome::Preview(tab)) = harness.outcomes.pop() else { panic!("{:?}", harness.outcomes) };
    assert_eq!(tab.heading.as_deref(), Some("Move"));
    assert_eq!(tab.apply.0, "refactor.dialog.apply");
    assert_eq!(tab.apply.1["values"]["to"], "ok", "the field has the keyboard again with its text selected");
}

struct Signing;

impl RefactoringProvider for Signing {
    fn id(&self) -> &str {
        "test.signature"
    }

    fn title(&self) -> String {
        "Change Signature".to_owned()
    }

    fn form(&self, _: &mut Session, _: &Value, _: Duration) -> Result<RefactoringForm, CommandError> {
        let column = |id: &str| TableColumn::new(id, id.to_owned(), ColumnKind::Text);
        let rows = json!([{ "name": "a", "originalIndex": 0 }, { "name": "b", "originalIndex": 1 }]);
        Ok(RefactoringForm {
            title: self.title(),
            fields: vec![RefactoringField {
                id: "parameters".to_owned(),
                label: "Parameters".to_owned(),
                kind: FieldKind::Table,
                value: rows,
                options: Vec::new(),
                columns: vec![column("name"), column("defaultValue")],
                new_row: json!({ "name": "", "originalIndex": -1 }),
                fixed: false,
            }],
            ..RefactoringForm::default()
        })
    }

    fn plan(&self, _: &mut Session, _: &Value, _: &Values, _: Duration) -> Result<RefactoringPlan, CommandError> {
        Ok(RefactoringPlan::default())
    }

    fn apply(&self, _: &mut Session, _: &Value, _: &Values, _: Duration) -> Result<RefactoringApplied, CommandError> {
        Ok(RefactoringApplied::default())
    }

    fn cancel(&self, _: &mut Session) {}
}

#[test]
fn a_table_field_adds_moves_and_removes_rows_keeping_what_the_provider_put_in_them() {
    let mut harness = Harness::new(false);
    harness.session.state_mut::<RefactoringProviders>().add(Arc::new(Signing));
    harness.dialog.open("test.signature".to_owned(), Value::Null);
    harness.settle();
    let press = |harness: &mut Harness, key: &str| {
        harness.click(harness.button(key).center());
        harness.frame(Vec::new());
        harness.frame(Vec::new());
    };
    let names = |harness: &Harness| -> Vec<(String, i64)> {
        let Some(State::Open(form)) = &harness.dialog.state else { panic!("no form") };
        form.values["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| (row["name"].as_str().unwrap_or_default().to_owned(), row["originalIndex"].as_i64().unwrap()))
            .collect()
    };
    press(&mut harness, "tableAdd");
    assert_eq!(names(&harness), [("a".to_owned(), 0), ("b".to_owned(), 1), (String::new(), -1)], "the new row last");
    press(&mut harness, "tableUp");
    press(&mut harness, "tableUp");
    assert_eq!(names(&harness)[0], (String::new(), -1), "moved up twice");
    press(&mut harness, "tableDown");
    assert_eq!(names(&harness)[1], (String::new(), -1));
    press(&mut harness, "tableRemove");
    assert_eq!(names(&harness), [("a".to_owned(), 0), ("b".to_owned(), 1)]);
    harness.frame(vec![Event::Text("x".to_owned())]);
    assert_eq!(names(&harness)[0].0, "a", "no cell has the keyboard after a button");
}

struct Members;

impl RefactoringProvider for Members {
    fn id(&self) -> &str {
        "test.members"
    }

    fn title(&self) -> String {
        "Pull Members Up".to_owned()
    }

    fn form(&self, _: &mut Session, _: &Value, _: Duration) -> Result<RefactoringForm, CommandError> {
        let rows = json!([
            { "selected": false, "member": "age: int", "signature": "age" },
            { "selected": true, "member": "bark(): String", "signature": "bark()", "abstract": false }
        ]);
        Ok(RefactoringForm {
            title: self.title(),
            fields: vec![RefactoringField {
                id: "members".to_owned(),
                label: "Members".to_owned(),
                kind: FieldKind::Table,
                value: rows,
                options: Vec::new(),
                columns: vec![
                    TableColumn::new("selected", String::new(), ColumnKind::Checkbox),
                    TableColumn::new("member", "Member".to_owned(), ColumnKind::Label),
                    TableColumn::new("abstract", "Make abstract".to_owned(), ColumnKind::Checkbox),
                ],
                new_row: Value::Null,
                fixed: true,
            }],
            ..RefactoringForm::default()
        })
    }

    fn plan(&self, _: &mut Session, _: &Value, _: &Values, _: Duration) -> Result<RefactoringPlan, CommandError> {
        Ok(RefactoringPlan::default())
    }

    fn apply(&self, _: &mut Session, _: &Value, _: &Values, _: Duration) -> Result<RefactoringApplied, CommandError> {
        Ok(RefactoringApplied::default())
    }

    fn cancel(&self, _: &mut Session) {}
}

#[test]
fn a_fixed_table_ticks_its_check_boxes_and_has_no_buttons_to_add_or_move_rows() {
    let mut harness = Harness::new(false);
    harness.session.state_mut::<RefactoringProviders>().add(Arc::new(Members));
    harness.dialog.open("test.members".to_owned(), Value::Null);
    harness.settle();
    let check = |harness: &Harness, row: usize, column: &str| -> Option<Rect> {
        let Some(State::Open(form)) = &harness.dialog.state else { panic!("no form") };
        form.checks.get(&("members".to_owned(), row, column.to_owned())).copied()
    };
    {
        let Some(State::Open(form)) = &harness.dialog.state else { panic!("no form") };
        assert!(!form.buttons.contains_key("tableAdd"), "{:?}", form.buttons);
    }
    assert!(check(&harness, 0, "abstract").is_none(), "a row without a boolean has no check box");
    let tick = check(&harness, 0, "selected").expect("a check box in the first row");
    harness.click(tick.center());
    harness.frame(Vec::new());
    let untick = check(&harness, 1, "abstract").expect("the method can be made abstract");
    harness.click(untick.center());
    harness.frame(Vec::new());
    let Some(State::Open(form)) = &harness.dialog.state else { panic!("no form") };
    let rows = form.values["members"].as_array().unwrap();
    assert_eq!(rows[0]["selected"], true, "{rows:?}");
    assert_eq!(rows[1]["abstract"], true);
    assert_eq!(rows[1]["signature"], "bark()", "what the provider put in a row stays");
}

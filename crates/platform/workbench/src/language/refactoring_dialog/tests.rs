use std::sync::Mutex;

use serde_json::json;
use zen_core::DispatchError;

use super::*;
use crate::language::safe_delete::tests::{LIB, answer_usages, greet_offset, lib_text, project};

fn failure(result: Result<Value, DispatchError>) -> String {
    match result {
        Err(DispatchError::Failed { error, .. }) => error.message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn safe_delete_through_the_dialog_lists_usages_as_conflicts_and_deletes_anyway_with_force() {
    let mut fixture = project();
    answer_usages(&fixture, true);
    let listed = fixture.dispatch("refactor.dialog.list", json!({})).unwrap();
    let ids: Vec<&str> = listed.as_array().unwrap().iter().filter_map(|info| info["id"].as_str()).collect();
    assert_eq!(
        ids,
        [
            "safeDelete",
            "rename",
            "javaMove",
            "javaChangeSignature",
            "javaPullUp",
            "javaPushDown",
            "javaExtractSuperclass",
            "javaExtractInterface",
            "javaEncapsulateFields",
            "javaIntroduceParameterObject"
        ]
    );

    let context = json!({ "path": "lib.go", "offset": greet_offset() });
    let form =
        fixture.dispatch("refactor.dialog.form", json!({ "provider": "safeDelete", "context": context })).unwrap();
    assert_eq!(form["immediate"], true);
    assert_eq!(form["fields"], json!([]));
    assert_eq!(form["anywayLabel"], t!("workbench-safe-delete-anyway"));

    let ask = json!({ "provider": "safeDelete", "context": context, "waitMs": 5000 });
    let plan = fixture.dispatch_once_running("refactor.dialog.preview", ask.clone()).unwrap();
    assert_eq!(plan["pending"], false);
    assert_eq!(plan["conflicts"].as_array().unwrap().len(), 1);
    assert_eq!(plan["conflicts"][0]["relative"], "main.go");
    assert_eq!(plan["conflicts"][0]["place"]["line"], 3);
    assert_eq!(plan["conflictsMessage"], t!("workbench-safe-delete-conflicts", name = "greet", count = 1));
    assert_eq!(plan["files"][0]["relative"], "lib.go", "the deletion is the preview's change");

    let refused = failure(fixture.dispatch("refactor.dialog.apply", ask.clone()));
    assert!(refused.contains(&t!("workbench-safe-delete-conflicts", name = "greet", count = 1)), "{refused}");
    assert!(lib_text(&mut fixture).contains("func greet"));
    let mut anyway = ask;
    anyway["force"] = json!(true);
    let applied = fixture.dispatch("refactor.dialog.apply", anyway).unwrap();
    assert_eq!(applied["files"], json!([fixture.path("lib.go")]));
    assert!(!lib_text(&mut fixture).contains("func greet"));
}

#[test]
fn rename_through_the_dialog_asks_for_the_new_name_and_previews_and_applies_it() {
    let mut fixture = crate::language::testing::fixture(&[("lib.go", LIB)]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "renameProvider": true
    }));
    fixture.open("lib.go");
    let lib = zen_lsp::position::file_uri(&fixture.path("lib.go"));
    fixture.server.on_with("textDocument/rename", move |params| {
        let range = json!({ "start": { "line": 3, "character": 5 }, "end": { "line": 3, "character": 10 } });
        json!({ "changes": { lib.clone(): [{ "range": range, "newText": params["newName"] }] } })
    });
    let context = json!({ "path": "lib.go", "offset": greet_offset() });
    let form = fixture
        .dispatch_once_running("refactor.dialog.form", json!({ "provider": "rename", "context": context }))
        .unwrap();
    assert_eq!(form["fields"][0]["id"], "newName");
    assert_eq!(form["fields"][0]["value"], "greet");
    assert_eq!(form["immediate"], false);

    let ask = json!({ "provider": "rename", "context": context, "values": { "newName": "hello" }, "waitMs": 5000 });
    let plan = fixture.dispatch("refactor.dialog.preview", ask.clone()).unwrap();
    assert_eq!((plan["usageCount"].clone(), plan["conflicts"].clone()), (json!(1), json!([])));
    fixture.dispatch("refactor.dialog.apply", ask).unwrap();
    assert!(lib_text(&mut fixture).contains("func hello()"));
}

struct Moving {
    applied: Mutex<Vec<Values>>,
}

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
        let conflicts = (values.get("to") == Some(&json!("taken")))
            .then(|| RefactoringConflict { message: "taken".to_owned(), place: None, relative: None })
            .into_iter()
            .collect();
        Ok(RefactoringPlan { conflicts, ..RefactoringPlan::default() })
    }

    fn apply(
        &self,
        _: &mut Session,
        _: &Value,
        values: &Values,
        _: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        self.applied.lock().unwrap().push(values.clone());
        Ok(RefactoringApplied::default())
    }

    fn cancel(&self, _: &mut Session) {}
}

#[test]
fn a_plugin_refactoring_declares_its_fields_and_is_refused_on_conflicts_until_forced() {
    let mut fixture = project();
    let moving = Arc::new(Moving { applied: Mutex::new(Vec::new()) });
    fixture.session.state_mut::<RefactoringProviders>().add(moving.clone());
    let form = fixture.dispatch("refactor.dialog.form", json!({ "provider": "test.move" })).unwrap();
    assert_eq!(form["fields"][1], json!({ "id": "all", "label": "all", "kind": "checkbox", "value": false }));
    let taken = json!({ "provider": "test.move", "values": { "to": "taken" } });
    assert!(fixture.dispatch("refactor.dialog.apply", taken.clone()).is_err());
    let mut anyway = taken;
    anyway["force"] = json!(true);
    fixture.dispatch("refactor.dialog.apply", anyway).unwrap();
    assert_eq!(moving.applied.lock().unwrap().len(), 1);
    let unknown = failure(fixture.dispatch("refactor.dialog.form", json!({ "provider": "nope" })));
    assert_eq!(unknown, t!("workbench-refactoring-unknown", id = "nope"));
}

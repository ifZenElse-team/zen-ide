use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::language::refactoring_dialog::RefactoringProviders;
use crate::language::testing::{Fixture, fixture_for};

const WAIT: Duration = Duration::from_secs(5);
const MAIN: &str = "package com.acme.shop;\n\npublic class Main {\n    Helper helper;\n    Shape shape;\n}\n";
const SHAPE: &str = "package com.acme.shop;\n\npublic class Shape {\n}\n\nclass Helper {\n}\n";
const POINT: &str = "package com.acme.geo;\n\npublic class Point {\n}\n";
const SHAPE_MOVED: &str = "package com.acme.geo;\n\npublic class Shape {\n}\n\nclass Helper {\n}\n";
const CALC: &str = "package com.acme.shop;\n\npublic class Calc {\n    static double twice(double value, int times) {\n        return value * 2 * times;\n    }\n\n    void use() {\n        twice(1.0, 2);\n    }\n}\n";

fn range(line: u64, start: u64, end_line: u64, end: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": end_line, "character": end } })
}

fn uri(fixture: &Fixture, name: &str) -> String {
    zen_lsp::position::file_uri(&fixture.path(name))
}

fn capabilities(fixture: &Fixture) {
    let folders = json!({ "filters": [{ "pattern": { "glob": "**", "matches": "folder" } }] });
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "referencesProvider": true,
        "definitionProvider": true,
        "workspace": { "fileOperations": { "willRename": folders, "didRename": folders } }
    }));
}

fn shop() -> Fixture {
    let mut fixture = fixture_for(
        "java",
        &[
            ("src/com/acme/shop/Main.java", MAIN),
            ("src/com/acme/shop/Shape.java", SHAPE),
            ("src/com/acme/geo/Point.java", POINT),
            ("src/com/acme/shop/Calc.java", CALC),
        ],
    );
    capabilities(&fixture);
    let destination = |fixture: &Fixture, folder: &str, name: &str| {
        json!({
            "displayName": name,
            "uri": zen_lsp::position::file_uri(&fixture.path(folder)),
            "project": "shop",
            "isDefaultPackage": false
        })
    };
    let list = json!({
        "destinations": [
            destination(&fixture, "src/com/acme", "com.acme"),
            destination(&fixture, "src/com/acme/geo", "com.acme.geo"),
            destination(&fixture, "src/com/acme/shop", "com.acme.shop")
        ]
    });
    fixture.server.on("java/getMoveDestinations", list);
    fixture.open("src/com/acme/shop/Shape.java");
    fixture
}

fn dialog(fixture: &mut Fixture, command: &str, provider: &str, context: Value, values: Value) -> Value {
    let params = json!({ "provider": provider, "context": context, "values": values, "waitMs": 5000 });
    fixture.dispatch_once_running(command, params).unwrap()
}

fn text(fixture: &mut Fixture, name: &str) -> String {
    let path = fixture.path(name);
    fixture.with(|_, workbench| match workbench.documents().iter().find(|document| document.path() == path) {
        Some(document) => document.text().to_owned(),
        None => std::fs::read_to_string(&path).unwrap_or_default(),
    })
}

fn answer_move(fixture: &Fixture) {
    let (shape, main) = (uri(fixture, "src/com/acme/shop/Shape.java"), uri(fixture, "src/com/acme/shop/Main.java"));
    let moved = uri(fixture, "src/com/acme/geo/Shape.java");
    fixture.server.on_with("java/move", move |params| {
        assert_eq!(params["moveKind"], "moveResource");
        assert_eq!(params["updateReferences"], true);
        assert_eq!(params["destination"]["displayName"], "com.acme.geo", "the server's own destination comes back");
        json!({ "edit": { "documentChanges": [
            { "textDocument": { "uri": main, "version": null }, "edits": [
                { "range": range(0, 22, 2, 0), "newText": ";\n\nimport com.acme.geo.Shape;\n\n" }
            ] },
            { "textDocument": { "uri": shape, "version": null }, "edits": [
                { "range": range(0, 8, 0, 21), "newText": "com.acme.geo" }
            ] },
            { "kind": "rename", "oldUri": shape, "newUri": moved }
        ] } })
    });
    let main = uri(fixture, "src/com/acme/shop/Main.java");
    fixture.server.on_with("textDocument/references", move |params| {
        assert_eq!(params["position"], json!({ "line": 5, "character": 6 }), "asked at Helper, the hidden class");
        json!([{ "uri": main, "range": range(3, 4, 3, 10) }])
    });
}

#[test]
fn moving_a_class_lists_the_servers_packages_starting_from_its_own() {
    let mut fixture = shop();
    let context = json!({ "paths": [fixture.path("src/com/acme/shop/Shape.java")] });
    let form = dialog(&mut fixture, "refactor.dialog.form", MOVE, context.clone(), json!({}));
    assert_eq!(form["title"], t!("workbench-java-move-class-title", name = "Shape"));
    let field = &form["fields"][0];
    assert_eq!((field["id"].clone(), field["kind"].clone()), (json!("destination"), json!("choice")));
    let labels: Vec<&str> =
        field["options"].as_array().unwrap().iter().filter_map(|option| option["label"].as_str()).collect();
    assert_eq!(labels, ["com.acme", "com.acme.geo", "com.acme.shop"]);
    assert_eq!(field["value"], json!(uri(&fixture, "src/com/acme/shop")), "its own package first");
    let sent = &fixture.server.wait_for("java/getMoveDestinations", 1, WAIT)[0];
    assert_eq!(sent["sourceUris"], json!([uri(&fixture, "src/com/acme/shop/Shape.java")]));

    let same = dialog(&mut fixture, "refactor.dialog.preview", MOVE, context, json!({}));
    assert_eq!(same["error"], json!(t!("workbench-java-move-same-package", package = "com.acme.shop")));
}

#[test]
fn moving_a_class_previews_the_servers_edit_lists_package_private_uses_and_one_undo_brings_it_back() {
    let mut fixture = shop();
    answer_move(&fixture);
    let context = json!({ "path": "src/com/acme/shop/Shape.java" });
    let values = json!({ "destination": uri(&fixture, "src/com/acme/geo") });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", MOVE, context.clone(), values.clone());
    assert_eq!(plan["pending"], false, "{plan}");
    assert_eq!(plan["operations"][0]["kind"], "rename");
    assert_eq!(plan["operations"][0]["newRelative"], "src/com/acme/geo/Shape.java");
    assert_eq!(plan["usageCount"], 2);
    let conflicts = plan["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1, "{plan}");
    let file = "src/com/acme/shop/Main.java";
    assert_eq!(conflicts[0]["message"], json!(t!("workbench-java-move-package-private", name = "Helper", file = file)));
    assert_eq!(conflicts[0]["place"]["line"], 4);

    let params = json!({ "provider": MOVE, "context": context, "values": values, "waitMs": 5000 });
    assert!(fixture.dispatch("refactor.dialog.apply", params.clone()).is_err(), "refused with conflicts");
    let mut forced = params;
    forced["force"] = json!(true);
    fixture.dispatch("refactor.dialog.apply", forced).unwrap();
    assert!(!fixture.path("src/com/acme/shop/Shape.java").exists());
    assert_eq!(text(&mut fixture, "src/com/acme/geo/Shape.java"), SHAPE_MOVED);
    assert!(text(&mut fixture, "src/com/acme/shop/Main.java").contains("import com.acme.geo.Shape;"));

    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert!(fixture.path("src/com/acme/shop/Shape.java").exists());
    assert!(!fixture.path("src/com/acme/geo/Shape.java").exists());
    assert_eq!(text(&mut fixture, "src/com/acme/shop/Main.java"), MAIN);
}

#[test]
fn a_class_is_not_moved_over_a_file_of_the_same_name() {
    let mut fixture = shop();
    std::fs::write(fixture.path("src/com/acme/geo/Shape.java"), POINT).unwrap();
    let context = json!({ "path": "src/com/acme/shop/Shape.java" });
    let values = json!({ "destination": uri(&fixture, "src/com/acme/geo") });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", MOVE, context, values);
    let expected = t!("workbench-java-move-file-exists", name = "Shape.java", package = "com.acme.geo");
    assert_eq!(plan["error"], json!(expected));
}

#[test]
fn moving_a_package_renames_its_folder_with_the_servers_edits_for_the_new_name() {
    let mut fixture = shop();
    let main = uri(&fixture, "src/com/acme/shop/Main.java");
    fixture.server.on_with("workspace/willRenameFiles", move |_| {
        json!({ "documentChanges": [
            { "textDocument": { "uri": main, "version": null }, "edits": [
                { "range": range(0, 8, 0, 21), "newText": "com.acme.geo.shop" }
            ] }
        ] })
    });
    let context = json!({ "paths": [fixture.path("src/com/acme/shop")] });
    let form = dialog(&mut fixture, "refactor.dialog.form", MOVE, context.clone(), json!({}));
    assert_eq!(form["title"], t!("workbench-java-move-package-title", name = "shop"));
    let labels: Vec<&str> =
        form["fields"][0]["options"].as_array().unwrap().iter().filter_map(|option| option["label"].as_str()).collect();
    assert_eq!(labels, ["com.acme", "com.acme.geo"], "not into itself");
    assert_eq!(form["fields"][0]["value"], json!(uri(&fixture, "src/com/acme")), "its parent first");

    let values = json!({ "destination": uri(&fixture, "src/com/acme/geo") });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", MOVE, context.clone(), values.clone());
    assert_eq!(plan["operations"][0]["newRelative"], "src/com/acme/geo/shop", "{plan}");
    assert_eq!(plan["files"][0]["relative"], "src/com/acme/shop/Main.java");
    let asked = &fixture.server.wait_for("workspace/willRenameFiles", 1, WAIT)[0];
    assert_eq!(asked["files"][0]["newUri"], json!(uri(&fixture, "src/com/acme/geo/shop")));

    dialog(&mut fixture, "refactor.dialog.apply", MOVE, context, values);
    assert!(fixture.path("src/com/acme/geo/shop/Calc.java").is_file());
    assert!(!fixture.path("src/com/acme/shop").exists());
    assert!(text(&mut fixture, "src/com/acme/geo/shop/Main.java").starts_with("package com.acme.geo.shop;"));
    let told = &fixture.server.wait_for("workspace/didRenameFiles", 1, WAIT)[0];
    assert_eq!(told["files"][0]["oldUri"], json!(uri(&fixture, "src/com/acme/shop")));
}

fn signature_info(fixture: &Fixture) {
    fixture.server.on_with("java/getChangeSignatureInfo", |params| {
        if params["range"]["start"]["line"] != 3 {
            return Value::Null;
        }
        json!({
            "methodIdentifier": "=shop/src<com.acme.shop{Calc.java[Calc~twice~D~I",
            "modifier": "",
            "returnType": "double",
            "methodName": "twice",
            "parameters": [
                { "type": "double", "name": "value", "defaultValue": "", "originalIndex": 0 },
                { "type": "int", "name": "times", "defaultValue": "", "originalIndex": 1 }
            ],
            "exceptions": []
        })
    });
}

fn calc_offset(needle: &str) -> usize {
    CALC[..CALC.find(needle).unwrap()].chars().count() + 1
}

#[test]
fn change_signature_asks_for_visibility_return_type_name_and_parameters() {
    let mut fixture = shop();
    signature_info(&fixture);
    fixture.open("src/com/acme/shop/Calc.java");
    let context = json!({ "path": "src/com/acme/shop/Calc.java", "offset": calc_offset("twice(double") });
    let form = dialog(&mut fixture, "refactor.dialog.form", CHANGE_SIGNATURE, context, json!({}));
    let ids: Vec<&str> = form["fields"].as_array().unwrap().iter().filter_map(|field| field["id"].as_str()).collect();
    assert_eq!(ids, ["visibility", "returnType", "name", "parameters", "delegate"]);
    assert_eq!(form["fields"][0]["value"], "");
    assert_eq!(form["fields"][0]["options"][2]["label"], json!(t!("workbench-java-signature-package-private")));
    assert_eq!(form["fields"][1]["value"], "double");
    assert_eq!(form["fields"][2]["value"], "twice");
    let table = &form["fields"][3];
    assert_eq!(table["kind"], "table");
    let columns: Vec<&str> =
        table["columns"].as_array().unwrap().iter().filter_map(|column| column["id"].as_str()).collect();
    assert_eq!(columns, ["type", "name", "defaultValue"]);
    assert_eq!(table["value"][1], json!({ "type": "int", "name": "times", "defaultValue": "", "originalIndex": 1 }));
    assert_eq!(table["newRow"]["originalIndex"], -1);
}

#[test]
fn change_signature_on_a_call_finds_the_method_through_its_definition() {
    let mut fixture = shop();
    signature_info(&fixture);
    let calc = uri(&fixture, "src/com/acme/shop/Calc.java");
    fixture.server.on("textDocument/definition", json!([{ "uri": calc, "range": range(3, 18, 3, 23) }]));
    fixture.open("src/com/acme/shop/Calc.java");
    let context = json!({ "path": "src/com/acme/shop/Calc.java", "offset": calc_offset("twice(1.0") });
    let form = dialog(&mut fixture, "refactor.dialog.form", CHANGE_SIGNATURE, context, json!({}));
    assert_eq!(form["fields"][2]["value"], "twice", "{form}");
    assert_eq!(fixture.server.requests("java/getChangeSignatureInfo").len(), 2);
}

#[test]
fn change_signature_sends_the_new_signature_and_previews_every_call_as_its_own_change() {
    let mut fixture = shop();
    signature_info(&fixture);
    let calc = uri(&fixture, "src/com/acme/shop/Calc.java");
    fixture.server.on_with("java/getRefactorEdit", move |params| {
        assert_eq!(params["command"], "changeSignature");
        let new_text = "public static double twice(int times, double value, String label) {\n        return value * 2 * times;\n    }\n\n    void use() {\n        twice(2, 1.0, \"x\"";
        json!({ "edit": { "documentChanges": [
            { "textDocument": { "uri": calc, "version": null }, "edits": [{ "range": range(3, 4, 8, 20), "newText": new_text }] }
        ] } })
    });
    fixture.open("src/com/acme/shop/Calc.java");
    let context = json!({ "path": "src/com/acme/shop/Calc.java", "offset": calc_offset("twice(double") });
    let values = json!({
        "visibility": "public",
        "parameters": [
            { "type": "int", "name": "times", "defaultValue": "", "originalIndex": 1 },
            { "type": "double", "name": "value", "defaultValue": "", "originalIndex": 0 },
            { "type": "String", "name": "label", "defaultValue": "\"x\"", "originalIndex": -1 }
        ]
    });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", CHANGE_SIGNATURE, context.clone(), values.clone());
    assert_eq!(plan["conflicts"], json!([]), "{plan}");
    let lines: Vec<u64> =
        plan["files"][0]["changes"].as_array().unwrap().iter().filter_map(|change| change["line"].as_u64()).collect();
    assert_eq!(lines, [4, 9], "the declaration and the call, not one change over both");
    let sent = &fixture.server.wait_for("java/getRefactorEdit", 1, WAIT)[0];
    let arguments = &sent["commandArguments"];
    assert_eq!(arguments[0], "=shop/src<com.acme.shop{Calc.java[Calc~twice~D~I");
    assert_eq!(
        (arguments[1].clone(), arguments[2].clone(), arguments[3].clone()),
        (json!(false), json!("twice"), json!("public"))
    );
    assert_eq!(
        arguments[5][2],
        json!({ "type": "String", "name": "label", "defaultValue": "\"x\"", "originalIndex": -1 })
    );
    assert_eq!(sent["context"]["range"]["start"], json!({ "line": 3, "character": 19 }), "at the caret");

    dialog(&mut fixture, "refactor.dialog.apply", CHANGE_SIGNATURE, context, values);
    let changed = text(&mut fixture, "src/com/acme/shop/Calc.java");
    assert!(changed.contains("public static double twice(int times, double value, String label) {"), "{changed}");
    assert!(changed.contains("twice(2, 1.0, \"x\");"), "{changed}");
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&mut fixture, "src/com/acme/shop/Calc.java"), CALC);
}

#[test]
fn change_signature_refuses_what_the_server_cannot_do_and_lists_a_removed_parameter_still_used() {
    let mut fixture = shop();
    signature_info(&fixture);
    let calc = uri(&fixture, "src/com/acme/shop/Calc.java");
    fixture.server.on_with("java/getRefactorEdit", move |_| {
        json!({ "edit": { "changes": { calc.clone(): [{ "range": range(3, 31, 3, 47), "newText": "value" }] } } })
    });
    fixture.open("src/com/acme/shop/Calc.java");
    let context = json!({ "path": "src/com/acme/shop/Calc.java", "offset": calc_offset("twice(double") });
    let new_without_default =
        json!({ "parameters": [{ "type": "int", "name": "extra", "defaultValue": "", "originalIndex": -1 }] });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", CHANGE_SIGNATURE, context.clone(), new_without_default);
    assert_eq!(plan["error"], json!(t!("workbench-java-signature-default-needed", name = "extra")));
    let twice = json!({ "parameters": [
        { "type": "int", "name": "a", "originalIndex": 0 },
        { "type": "int", "name": "a", "originalIndex": 1 }
    ] });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", CHANGE_SIGNATURE, context.clone(), twice);
    assert_eq!(plan["error"], json!(t!("workbench-java-signature-duplicate", name = "a")));

    let removed =
        json!({ "parameters": [{ "type": "double", "name": "value", "defaultValue": "", "originalIndex": 0 }] });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", CHANGE_SIGNATURE, context, removed);
    let conflicts = plan["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1, "{plan}");
    assert_eq!(conflicts[0]["message"], json!(t!("workbench-java-signature-removed-used", name = "times")));
    assert_eq!(conflicts[0]["place"]["line"], 5);
}

#[test]
fn the_window_finds_move_and_change_signature_for_java_files_only() {
    let mut fixture = shop();
    let providers = fixture.session.state_mut::<RefactoringProviders>().clone();
    let java = json!({ "paths": [fixture.path("src/com/acme/shop/Shape.java")] });
    let found = providers.for_action(&mut fixture.session, "move", &java).map(|provider| provider.id().to_owned());
    assert_eq!(found.as_deref(), Some(MOVE));
    let package = json!({ "paths": [fixture.path("src/com/acme")] });
    assert!(providers.for_action(&mut fixture.session, "move", &package).is_some());
    std::fs::write(fixture.path("notes.txt"), "notes\n").unwrap();
    let text = json!({ "paths": [fixture.path("notes.txt")] });
    assert!(providers.for_action(&mut fixture.session, "move", &text).is_none());
    let caret = json!({ "path": fixture.path("src/com/acme/shop/Calc.java"), "offset": 0 });
    let found =
        providers.for_action(&mut fixture.session, "changeSignature", &caret).map(|provider| provider.id().to_owned());
    assert_eq!(found.as_deref(), Some(CHANGE_SIGNATURE));
    let listed = fixture.dispatch("refactor.dialog.list", json!({})).unwrap();
    let actions: Vec<(&str, &str)> = listed
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|info| Some((info["id"].as_str()?, info["action"].as_str()?)))
        .collect();
    assert_eq!(
        actions,
        [
            (MOVE, "move"),
            (CHANGE_SIGNATURE, "changeSignature"),
            (PULL_UP, "pullMembersUp"),
            (PUSH_DOWN, "pushMembersDown"),
            (EXTRACT_SUPERCLASS, "extractSuperclass"),
            (EXTRACT_INTERFACE, "extractInterface"),
            (ENCAPSULATE_FIELDS, "encapsulateFields"),
            (INTRODUCE_PARAMETER_OBJECT, "introduceParameterObject")
        ]
    );
    let ending = json!({ "path": fixture.path("src/com/acme/shop/Calc.java"), "offset": 30 });
    let found =
        providers.for_action(&mut fixture.session, "pullMembersUp", &ending).map(|provider| provider.id().to_owned());
    assert_eq!(found.as_deref(), Some(PULL_UP), "in a class");
    assert!(providers.for_action(&mut fixture.session, "encapsulateFields", &text).is_none());
}

#[test]
fn the_types_a_move_can_hide_are_the_top_level_ones_without_public() {
    let found = hidden_types(
        "package a;\n\npublic class A {\n    class Inner {}\n}\n\nclass B {}\nfinal record C() {}\n@interface D {}\n",
    );
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["B", "C", "D"]);
    assert!(hidden_types("public final class A {}\npublic enum E { X }\n").is_empty());
}

#[test]
fn a_method_body_uses_a_parameter_by_its_name_but_not_after_a_dot() {
    let text =
        "void f(int a, int b) {\n    this.a = b; // a\n    String s = \"a\";\n    g(a);\n}\nvoid g(int a) { a++; }\n";
    let used = body_uses(text, 5, &["a".to_owned(), "c".to_owned()]);
    assert_eq!(used.len(), 1, "{used:?}");
    assert_eq!(used[0].1, text.find("g(a)").unwrap() + 2);
    assert!(body_uses("abstract void f(int a);\nvoid g() { a(); }\n", 14, &["a".to_owned()]).is_empty());
}

#[test]
fn an_edit_over_several_lines_is_split_into_its_changed_lines() {
    let text = "a\nb\nc\nd\n";
    let edit = json!({ "changes": { "file:///x.java": [{ "range": range(0, 0, 3, 1), "newText": "A\nb\nc\nD" }] } });
    let refined = refine(&edit, |_| Some(text.to_owned()));
    assert_eq!(
        refined["changes"]["file:///x.java"],
        json!([
            { "range": range(0, 0, 1, 0), "newText": "A\n" },
            { "range": range(3, 0, 4, 0), "newText": "D\n" }
        ])
    );
}

use serde_json::{Value, json};

use super::*;
use crate::language::testing::{Fixture, fixture_for};

pub(in crate::java::refactoring) const ANIMAL: &str = "package com.acme;\n\npublic class Animal {\n    protected String name;\n\n    public String describe() {\n        return name;\n    }\n}\n";
pub(in crate::java::refactoring) const DOG: &str = "package com.acme;\n\nimport java.util.List;\n\npublic class Dog extends Animal {\n    public int age;\n    private String owner;\n\n    /** Barks. */\n    public String bark(int times) {\n        return name + \" barks \" + times;\n    }\n\n    public String whose() {\n        return owner;\n    }\n\n    List<String> tricks() {\n        return List.of();\n    }\n}\n";
pub(in crate::java::refactoring) const SHOP: &str = "package com.acme;\n\npublic class Shop {\n    public static int total(int price, int count, String label) {\n        return price * count + label.length();\n    }\n\n    public static void main(String[] args) {\n        Dog dog = new Dog();\n        dog.age = 3;\n        dog.age += 2;\n        System.out.println(dog.age + total(1, 2, \"x\") + dog.describe());\n    }\n}\n";

const WAIT_MS: u64 = 5000;

pub(in crate::java::refactoring) fn uri(fixture: &Fixture, name: &str) -> String {
    zen_lsp::position::file_uri(&fixture.path(name))
}

pub(in crate::java::refactoring) fn range(line: u64, start: u64, end_line: u64, end: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": end_line, "character": end } })
}

pub(in crate::java::refactoring) fn zoo() -> Fixture {
    let mut fixture = fixture_for(
        "java",
        &[("src/com/acme/Animal.java", ANIMAL), ("src/com/acme/Dog.java", DOG), ("src/com/acme/Shop.java", SHOP)],
    );
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "referencesProvider": true,
        "implementationProvider": true
    }));
    fixture.open("src/com/acme/Dog.java");
    fixture
}

fn item(fixture: &Fixture, name: &str, file: &str, line: u64) -> Value {
    json!({
        "name": name,
        "kind": 5,
        "uri": uri(fixture, file),
        "range": range(line, 0, line + 3, 1),
        "selectionRange": range(line, 13, line, 13 + name.len() as u64),
        "data": { "element": name }
    })
}

fn hierarchy_of(fixture: &Fixture) {
    let animal = item(fixture, "Animal", "src/com/acme/Animal.java", 2);
    let dog = item(fixture, "Dog", "src/com/acme/Dog.java", 4);
    let object = json!({
        "name": "Object",
        "kind": 5,
        "uri": "jdt://contents/java.base/java.lang/Object.java",
        "range": range(0, 0, 0, 0),
        "selectionRange": range(0, 0, 0, 0)
    });
    let (prepared_animal, prepared_dog) = (animal.clone(), dog.clone());
    fixture.server.on_with("textDocument/prepareTypeHierarchy", move |params| {
        if params["textDocument"]["uri"].as_str().unwrap_or_default().ends_with("Dog.java") {
            json!([prepared_dog])
        } else {
            json!([prepared_animal])
        }
    });
    let super_of_dog = json!([animal, object]);
    fixture.server.on_with("typeHierarchy/supertypes", move |params| {
        assert_eq!(params["item"]["data"]["element"], "Dog", "the prepared item comes back");
        super_of_dog.clone()
    });
    fixture.server.on("typeHierarchy/subtypes", json!([dog]));
}

pub(in crate::java::refactoring) fn dialog(
    fixture: &mut Fixture,
    command: &str,
    provider: &str,
    context: Value,
    values: Value,
) -> Value {
    let params = json!({ "provider": provider, "context": context, "values": values, "waitMs": WAIT_MS });
    fixture.dispatch_once_running(command, params).unwrap()
}

pub(in crate::java::refactoring) fn forced(fixture: &mut Fixture, provider: &str, context: Value, values: Value) {
    let params =
        json!({ "provider": provider, "context": context, "values": values, "waitMs": WAIT_MS, "force": true });
    fixture.dispatch("refactor.dialog.apply", params).unwrap();
}

pub(in crate::java::refactoring) fn text(fixture: &mut Fixture, name: &str) -> String {
    let path = fixture.path(name);
    fixture.with(|_, workbench| match workbench.documents().iter().find(|document| document.path() == path) {
        Some(document) => document.text().to_owned(),
        None => std::fs::read_to_string(&path).unwrap_or_default(),
    })
}

pub(in crate::java::refactoring) fn at(text: &str, needle: &str) -> usize {
    text[..text.find(needle).unwrap()].chars().count()
}

pub(in crate::java::refactoring) fn rows(form: &Value, field: &str) -> Vec<Value> {
    let found = form["fields"].as_array().unwrap().iter().find(|known| known["id"] == field).unwrap();
    found["value"].as_array().unwrap().clone()
}

pub(in crate::java::refactoring) fn messages(plan: &Value) -> Vec<String> {
    plan["conflicts"].as_array().unwrap().iter().map(|c| c["message"].as_str().unwrap().to_owned()).collect()
}

fn select(rows: &[Value], signatures: &[&str], abstract_: &[&str]) -> Value {
    Value::Array(
        rows.iter()
            .map(|row| {
                let mut row = row.clone();
                let signature = row["signature"].as_str().unwrap_or_default().to_owned();
                row["selected"] = json!(signatures.contains(&signature.as_str()));
                if row.get("abstract").is_some() {
                    row["abstract"] = json!(abstract_.contains(&signature.as_str()));
                }
                row
            })
            .collect(),
    )
}

#[test]
fn pull_members_up_offers_the_superclasses_in_the_project_and_the_classs_members() {
    let mut fixture = zoo();
    hierarchy_of(&fixture);
    let context = json!({ "path": "src/com/acme/Dog.java", "offset": at(DOG, "bark(int") });
    let form = dialog(&mut fixture, "refactor.dialog.form", PULL_UP, context, json!({}));
    assert_eq!(form["title"], json!(t!("workbench-java-pull-up-title-of", name = "Dog")), "{form}");
    let destination = &form["fields"][0];
    assert_eq!(destination["options"].as_array().unwrap().len(), 1, "the JDK's Object is not offered: {destination}");
    assert_eq!(destination["options"][0]["label"], "com.acme.Animal");
    let table = &form["fields"][1];
    assert_eq!(table["fixed"], true);
    let columns: Vec<(&str, &str)> = table["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| (column["id"].as_str().unwrap(), column["kind"].as_str().unwrap_or("text")))
        .collect();
    assert_eq!(columns, [("selected", "checkbox"), ("member", "label"), ("abstract", "checkbox")]);
    let members = rows(&form, "members");
    let labels: Vec<&str> = members.iter().map(|row| row["member"].as_str().unwrap()).collect();
    assert_eq!(labels, ["age: int", "owner: String", "bark(int): String", "whose(): String", "tricks(): List<String>"]);
    assert_eq!(members[2]["selected"], true, "the member at the caret");
    assert_eq!(members[0]["selected"], false);
    assert!(members[0].get("abstract").is_none(), "a field cannot be made abstract");
}

#[test]
fn pulling_a_method_up_moves_it_with_its_doc_widens_what_stays_and_one_undo_brings_it_back() {
    let mut fixture = zoo();
    hierarchy_of(&fixture);
    let context = json!({ "path": "src/com/acme/Dog.java", "offset": at(DOG, "bark(int") });
    let form = dialog(&mut fixture, "refactor.dialog.form", PULL_UP, context.clone(), json!({}));
    let values = json!({
        "destination": uri(&fixture, "src/com/acme/Animal.java"),
        "members": select(&rows(&form, "members"), &["bark(int)", "owner", "tricks()"], &[])
    });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", PULL_UP, context.clone(), values.clone());
    assert_eq!(plan["error"], Value::Null, "{plan}");
    assert!(messages(&plan).is_empty(), "whose() stays and uses owner, which becomes protected: {plan}");
    dialog(&mut fixture, "refactor.dialog.apply", PULL_UP, context, values);
    assert_eq!(
        text(&mut fixture, "src/com/acme/Animal.java"),
        "package com.acme;\n\nimport java.util.List;\n\npublic class Animal {\n    protected String name;\n\n    public String describe() {\n        return name;\n    }\n\n    protected String owner;\n\n    /** Barks. */\n    public String bark(int times) {\n        return name + \" barks \" + times;\n    }\n\n    List<String> tricks() {\n        return List.of();\n    }\n}\n"
    );
    assert_eq!(
        text(&mut fixture, "src/com/acme/Dog.java"),
        "package com.acme;\n\nimport java.util.List;\n\npublic class Dog extends Animal {\n    public int age;\n\n    public String whose() {\n        return owner;\n    }\n}\n"
    );
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&mut fixture, "src/com/acme/Animal.java"), ANIMAL, "one undo for both files");
    assert_eq!(text(&mut fixture, "src/com/acme/Dog.java"), DOG);
}

#[test]
fn pulling_up_a_member_that_uses_one_left_behind_is_a_conflict_and_abstract_makes_the_superclass_abstract() {
    let mut fixture = zoo();
    hierarchy_of(&fixture);
    let context = json!({ "path": "src/com/acme/Dog.java", "offset": at(DOG, "whose") });
    let form = dialog(&mut fixture, "refactor.dialog.form", PULL_UP, context.clone(), json!({}));
    let destination = uri(&fixture, "src/com/acme/Animal.java");
    let values = json!({ "destination": destination, "members": select(&rows(&form, "members"), &["whose()"], &[]) });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", PULL_UP, context.clone(), values);
    assert_eq!(messages(&plan), [t!("workbench-java-hierarchy-uses-staying", name = "owner", class = "Dog")]);
    assert_eq!(plan["conflicts"][0]["place"]["line"], 15);

    let members = select(&rows(&form, "members"), &["whose()"], &["whose()"]);
    let values = json!({ "destination": destination, "members": members });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", PULL_UP, context.clone(), values.clone());
    assert_eq!(messages(&plan), [t!("workbench-java-hierarchy-made-abstract", class = "Animal")], "the body stays");
    let params = json!({ "provider": PULL_UP, "context": context, "values": values, "waitMs": WAIT_MS });
    assert!(fixture.dispatch("refactor.dialog.apply", params).is_err(), "refused with conflicts");
    forced(&mut fixture, PULL_UP, context, values);
    let animal = text(&mut fixture, "src/com/acme/Animal.java");
    assert!(animal.contains("public abstract class Animal {"), "{animal}");
    assert!(animal.contains("\n    public abstract String whose();\n}\n"), "{animal}");
    assert_eq!(text(&mut fixture, "src/com/acme/Dog.java"), DOG, "the method stays in Dog");
}

#[test]
fn pushing_a_member_down_moves_it_to_the_subclasses_and_reports_uses_they_do_not_cover() {
    let mut fixture = zoo();
    hierarchy_of(&fixture);
    let shop = uri(&fixture, "src/com/acme/Shop.java");
    fixture.server.on_with("textDocument/references", move |params| {
        assert_eq!(params["position"]["line"], 5, "asked at describe");
        json!([{ "uri": shop, "range": range(11, 60, 11, 68) }])
    });
    fixture.open("src/com/acme/Animal.java");
    let context = json!({ "path": "src/com/acme/Animal.java", "offset": at(ANIMAL, "describe") });
    let form = dialog(&mut fixture, "refactor.dialog.form", PUSH_DOWN, context.clone(), json!({}));
    assert_eq!(form["message"], json!(t!("workbench-java-push-down-to", classes = "Dog")), "{form}");
    let values = json!({ "members": select(&rows(&form, "members"), &["describe()"], &[]) });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", PUSH_DOWN, context.clone(), values);
    let expected = t!("workbench-java-push-down-used", name = "describe", file = "Shop.java", line = 12);
    assert_eq!(messages(&plan), [expected], "{plan}");

    let values = json!({ "members": select(&rows(&form, "members"), &["describe()"], &["describe()"]) });
    forced(&mut fixture, PUSH_DOWN, context, values);
    let animal = text(&mut fixture, "src/com/acme/Animal.java");
    assert!(animal.contains("public abstract class Animal"), "{animal}");
    assert!(animal.contains("    public abstract String describe();\n}"), "{animal}");
    let dog = text(&mut fixture, "src/com/acme/Dog.java");
    assert!(dog.ends_with("    public String describe() {\n        return name;\n    }\n}\n"), "{dog}");
}

#[test]
fn extracting_a_superclass_creates_its_file_between_the_class_and_its_old_superclass() {
    let mut fixture = zoo();
    let context = json!({ "path": "src/com/acme/Dog.java", "offset": at(DOG, "Dog extends") });
    let form = dialog(&mut fixture, "refactor.dialog.form", EXTRACT_SUPERCLASS, context.clone(), json!({}));
    assert_eq!(form["fields"][0]["value"], "BaseDog");
    assert!(rows(&form, "members").iter().all(|row| row["selected"] == false));
    let members = select(&rows(&form, "members"), &["tricks()", "bark(int)"], &["bark(int)"]);
    let values = json!({ "name": "Pet", "members": members });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", EXTRACT_SUPERCLASS, context.clone(), values.clone());
    assert_eq!(plan["operations"][0]["kind"], "create", "{plan}");
    assert_eq!(plan["operations"][0]["relative"], "src/com/acme/Pet.java");
    dialog(&mut fixture, "refactor.dialog.apply", EXTRACT_SUPERCLASS, context.clone(), values);
    assert_eq!(
        std::fs::read_to_string(fixture.path("src/com/acme/Pet.java")).unwrap(),
        "package com.acme;\n\nimport java.util.List;\n\npublic abstract class Pet extends Animal {\n    public abstract String bark(int times);\n\n    List<String> tricks() {\n        return List.of();\n    }\n}\n"
    );
    let dog = text(&mut fixture, "src/com/acme/Dog.java");
    assert!(dog.contains("public class Dog extends Pet {"), "{dog}");
    assert!(!dog.contains("tricks()") && dog.contains("bark(int times)"), "{dog}");
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert!(!fixture.path("src/com/acme/Pet.java").exists(), "one undo removes the new file");
    assert_eq!(text(&mut fixture, "src/com/acme/Dog.java"), DOG);
    dialog(&mut fixture, "refactor.dialog.apply", EXTRACT_SUPERCLASS, context.clone(), json!({ "name": "Pet" }));

    let taken = dialog(&mut fixture, "refactor.dialog.preview", EXTRACT_SUPERCLASS, context, json!({ "name": "Pet" }));
    assert_eq!(taken["error"], json!(t!("workbench-java-hierarchy-file-exists", name = "Pet")));
}

#[test]
fn extract_interface_asks_the_server_and_drops_its_generated_comments() {
    let mut fixture = zoo();
    let dog = uri(&fixture, "src/com/acme/Dog.java");
    let folder = uri(&fixture, "src/com/acme");
    let destination =
        json!({ "displayName": "com.acme", "uri": folder, "isDefaultPackage": false, "isParentOfSelectedFile": true });
    let other = json!({
        "displayName": "(default package)",
        "uri": uri(&fixture, "src"),
        "isDefaultPackage": true,
        "isParentOfSelectedFile": false
    });
    fixture.server.on(
        "java/checkExtractInterfaceStatus",
        json!({
            "subTypeName": "Dog",
            "members": [
                { "name": "bark", "typeName": "String", "parameters": ["int"], "handleIdentifier": "=Dog~bark~I" },
                { "name": "whose", "typeName": "String", "parameters": [], "handleIdentifier": "=Dog~whose" }
            ],
            "destinationResponse": { "destinations": [other, destination] }
        }),
    );
    let created = uri(&fixture, "src/com/acme/Barker.java");
    fixture.server.on_with("java/getRefactorEdit", move |params| {
        assert_eq!(params["command"], "extractInterface");
        assert!(params["context"]["textDocument"]["uri"].as_str().unwrap().ends_with("Dog.java"), "{params}");
        assert_eq!(params["commandArguments"][0], json!(["=Dog~bark~I"]));
        assert_eq!(params["commandArguments"][1], "Barker");
        assert_eq!(params["commandArguments"][2]["displayName"], "com.acme");
        json!({ "edit": { "documentChanges": [
            { "textDocument": { "uri": dog, "version": null }, "edits": [{
                "range": range(4, 31, 9, 4),
                "newText": " implements Barker {\n    public int age;\n    private String owner;\n\n    /** Barks. */\n    /** (non-Javadoc)\n     * @see com.acme.Barker#bark(int)\n     */\n    @Override\n    "
            }] },
            { "kind": "create", "uri": created, "options": { "overwrite": false, "ignoreIfExists": true } },
            { "textDocument": { "uri": created, "version": null }, "edits": [{
                "range": range(0, 0, 0, 0),
                "newText": "package com.acme;\n\npublic interface Barker {\n\n    String bark(int times);\n\n}"
            }] }
        ] } })
    });
    let context = json!({ "path": "src/com/acme/Dog.java", "offset": at(DOG, "bark(int") });
    let form = dialog(&mut fixture, "refactor.dialog.form", EXTRACT_INTERFACE, context.clone(), json!({}));
    assert_eq!(form["fields"][0]["value"], "IDog", "{form}");
    assert_eq!(form["fields"][1]["value"], json!(folder), "the class's own package");
    let members = rows(&form, "members");
    assert_eq!(members[0]["member"], "bark(int): String");
    assert_eq!((members[0]["selected"].clone(), members[1]["selected"].clone()), (json!(true), json!(false)));
    let values = json!({ "name": "Barker", "destination": folder, "members": members });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", EXTRACT_INTERFACE, context.clone(), values.clone());
    assert_eq!(plan["error"], Value::Null, "{plan}");
    dialog(&mut fixture, "refactor.dialog.apply", EXTRACT_INTERFACE, context, values);
    let dog = text(&mut fixture, "src/com/acme/Dog.java");
    assert!(dog.contains("public class Dog extends Animal implements Barker {"), "{dog}");
    assert!(dog.contains("    /** Barks. */\n    @Override\n    public String bark(int times) {"), "{dog}");
    assert!(!dog.contains("non-Javadoc"), "{dog}");
    assert!(fixture.path("src/com/acme/Barker.java").is_file());
}

#[test]
fn a_generated_comment_goes_with_its_line_and_nothing_else_changes() {
    let edit = json!({ "documentChanges": [{ "textDocument": { "uri": "file:///a" }, "edits": [
        { "newText": "x {\n    /** (non-Javadoc)\n     * @see A#b()\n     */\n    @Override\n    /** Mine. */\n" }
    ] }] });
    let clean = without_generated_comments(&edit);
    assert_eq!(clean["documentChanges"][0]["edits"][0]["newText"], "x {\n    @Override\n    /** Mine. */\n");
}

#[test]
fn an_abstract_declaration_keeps_the_methods_throws_clause_and_type_parameters() {
    let text =
        "class A {\n    protected final <T> T read(T a) throws java.io.IOException {\n        return a;\n    }\n}\n";
    let class = class_at(text, 12).unwrap();
    assert_eq!(
        declaration(text, &class.members[0], false),
        "protected abstract <T> T read(T a) throws java.io.IOException;"
    );
    assert_eq!(declaration(text, &class.members[0], true), "<T> T read(T a) throws java.io.IOException;");
}

#[test]
fn a_constructor_passing_arguments_to_the_old_superclass_is_a_conflict_of_extract_superclass() {
    let text = "class Dog extends Animal {\n    int age;\n\n    Dog(String name) {\n        super(name);\n    }\n}\n";
    let class = class_at(text, 8).unwrap();
    let chosen = [Chosen { signature: "age".to_owned(), abstract_: false }];
    let extract =
        NewSuperclass { source: Path::new("/none/Dog.java"), text, class: &class, name: "Pet", chosen: &chosen };
    let mut edits = Edits::default();
    extract_superclass(&extract, &mut edits).unwrap();
    let messages: Vec<&str> = edits.conflicts.iter().map(|(_, _, message)| message.as_str()).collect();
    assert_eq!(messages, [t!("workbench-java-superclass-constructor", class = "Pet")]);
}

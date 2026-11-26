use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::DispatchError;
use zen_modules::generate::{
    Chooser, ChooserOption, Chosen, CodeGenerator, CodeGenerators, GenerateContext, Insertion, Member, Offer,
};

use super::*;
use crate::language::testing::{Fixture, fixture};

const MAIN: &str = "package main\n\ntype Point struct {\n\tx int\n\ty int\n}\n";

fn range(line: u64, start: u64, end: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

fn uri(fixture: &Fixture) -> String {
    zen_lsp::position::file_uri(&fixture.path("main.go"))
}

fn project() -> Fixture {
    let mut fixture = fixture(&[("main.go", MAIN)]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "codeActionProvider": { "resolveProvider": false }
    }));
    fixture.open("main.go");
    fixture
}

fn list(fixture: &mut Fixture) -> GeneratorList {
    serde_json::from_value(fixture.dispatch("editor.generate.list", json!({ "offset": 0, "waitMs": 5000 })).unwrap())
        .unwrap()
}

fn titles(listed: &GeneratorList) -> Vec<&str> {
    listed.generators.iter().map(|generator| generator.title.as_str()).collect()
}

fn text(fixture: &Fixture) -> String {
    fixture.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
}

fn failure(result: Result<Value, DispatchError>) -> String {
    match result {
        Err(DispatchError::Failed { error, .. }) => error.message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn insert_line(fixture: &Fixture, line: &str) -> Value {
    json!({ "changes": { uri(fixture): [{ "range": range(6, 0, 0), "newText": line }] } })
}

#[test]
fn generate_lists_the_servers_source_actions_but_not_tidying_or_fixes() {
    let mut fixture = project();
    fixture.server.on(
        "textDocument/codeAction",
        json!([
            { "title": "Organize Imports", "kind": "source.organizeImports", "edit": {} },
            { "title": "Fix all", "kind": "source.fixAll", "edit": {} },
            { "title": "Extract function", "kind": "refactor.extract", "edit": {} },
            { "title": "Create variable", "kind": "quickfix", "edit": {} },
            { "title": "Run go mod tidy", "command": "tidy", "arguments": [] },
            { "title": "Add test for Point", "kind": "source.addTest", "edit": {} },
            { "title": "Disabled", "kind": "source.generate", "disabled": { "reason": "no" } }
        ]),
    );
    let listed = list(&mut fixture);
    assert_eq!(titles(&listed), ["Add test for Point"]);
    assert_eq!(listed.generators[0].kind.as_deref(), Some("source.addTest"));
    assert!(!listed.generators[0].chooser);
    let sent = &fixture.server.requests("textDocument/codeAction")[0];
    assert_eq!(sent["context"]["only"], json!(["source"]));

    fixture.with(|languages, _| {
        languages.declare_generate_for_test(&zen_lsp::GenerateDeclaration {
            kinds: vec!["source.doc".to_owned()],
            prompts: Vec::new(),
        });
    });
    fixture
        .server
        .on("textDocument/codeAction", json!([{ "title": "Add test", "kind": "source.addTest", "edit": {} }]));
    fixture.with(|_, workbench| {
        workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "\n".to_owned())]);
    });
    let listed = list(&mut fixture);
    assert!(listed.generators.is_empty(), "a server that names its kinds offers only those");
    assert_eq!(listed.error.as_deref(), Some(t!("workbench-generate-none").as_str()));
}

#[test]
fn a_servers_generator_is_applied_with_its_edit_as_one_undo_step() {
    let mut fixture = project();
    let edit = insert_line(&fixture, "\nfunc TestPoint() {}\n");
    fixture
        .server
        .on("textDocument/codeAction", json!([{ "title": "Add test", "kind": "source.addTest", "edit": edit }]));
    let listed = list(&mut fixture);
    let applied = fixture.dispatch("editor.generate.apply", json!({ "id": listed.generators[0].id })).unwrap();
    assert_eq!(applied, json!({ "title": "Add test", "files": [fixture.path("main.go")] }));
    assert_eq!(text(&fixture), format!("{MAIN}\nfunc TestPoint() {{}}\n"));
    let again = fixture.dispatch("editor.generate.apply", json!({ "id": listed.generators[0].id }));
    assert_eq!(failure(again), t!("workbench-generate-unknown", id = listed.generators[0].id.as_str()));
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);
}

fn to_string_prompt() -> zen_lsp::GenerateDeclaration {
    serde_json::from_value(json!({
        "prompts": [{
            "command": "lang.generateToStringPrompt",
            "title": { "en": "Generate toString()", "ru": "Создать toString()" },
            "members": "lang/checkToStringStatus",
            "lists": [{ "id": "fields", "items": "/fields/*", "label": "{/name}", "detail": "{/type}", "selected": "/isSelected" }],
            "options": [
                { "id": "regenerate", "label": { "en": "Replace the existing one" }, "when": "/exists" },
                { "id": "never", "label": { "en": "Never shown" }, "when": "/missing" }
            ],
            "generate": "lang/generateToString",
            "params": { "context": "${context}", "fields": "${members:fields}", "regenerate": "${option:regenerate}" }
        }]
    }))
    .unwrap()
}

#[test]
fn a_prompt_generator_asks_the_server_for_members_and_sends_the_choice_back() {
    let mut fixture = project();
    fixture.with(|languages, _| languages.declare_generate_for_test(&to_string_prompt()));
    fixture.server.on(
        "textDocument/codeAction",
        json!([{
            "title": "Generate toString()...",
            "kind": "source.generate.toString",
            "command": { "title": "Generate", "command": "lang.generateToStringPrompt", "arguments": [{ "at": 3 }] }
        }]),
    );
    fixture.server.on(
        "lang/checkToStringStatus",
        json!({ "type": "Point", "exists": true, "fields": [
            { "name": "x", "type": "int", "isSelected": true },
            { "name": "y", "type": "int", "isSelected": false }
        ] }),
    );
    let edit = insert_line(&fixture, "\nfunc (p Point) String() string { return \"\" }\n");
    fixture.server.on("lang/generateToString", edit);
    let listed = list(&mut fixture);
    assert!(listed.generators[0].chooser);
    let id = listed.generators[0].id.clone();
    let members = fixture.dispatch("editor.generate.members", json!({ "id": id, "waitMs": 5000 })).unwrap();
    let chooser: Chooser = serde_json::from_value(members["chooser"].clone()).unwrap();
    assert_eq!(fixture.server.requests("lang/checkToStringStatus")[0], json!({ "at": 3 }));
    assert_eq!(chooser.title, "Generate toString()");
    let names: Vec<(&str, bool)> = chooser.members.iter().map(|member| (member.id.as_str(), member.selected)).collect();
    assert_eq!(names, [("fields.0", true), ("fields.1", false)]);
    assert_eq!(chooser.members[1].detail.as_deref(), Some("int"));
    assert_eq!(chooser.options.iter().map(|option| option.id.as_str()).collect::<Vec<_>>(), ["regenerate"]);

    let refused = fixture.dispatch("editor.generate.apply", json!({ "id": id, "members": ["fields.7"] }));
    assert_eq!(failure(refused), t!("workbench-generate-unknown-member", id = "fields.7"));
    let listed = list(&mut fixture);
    let id = listed.generators[0].id.clone();
    let chosen = json!({ "id": id, "members": ["fields.1"], "options": { "regenerate": true } });
    let applied = fixture.dispatch("editor.generate.apply", chosen).unwrap();
    assert_eq!(applied["title"], "Generate toString()...");
    let sent = &fixture.server.requests("lang/generateToString")[0];
    assert_eq!(
        sent,
        &json!({ "context": { "at": 3 }, "fields": [{ "name": "y", "type": "int", "isSelected": false }], "regenerate": true })
    );
    assert!(text(&fixture).ends_with("func (p Point) String() string { return \"\" }\n"));
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);
}

struct Fields;

impl CodeGenerator for Fields {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn offers(&self, context: &GenerateContext<'_>) -> Vec<Offer> {
        let mut offers =
            vec![Offer { id: "fields".to_owned(), title: "Field List".to_owned(), chooser: true, action: None }];
        if context.offset == 0 {
            offers.push(Offer { id: "mark".to_owned(), title: "Mark".to_owned(), chooser: false, action: None });
        }
        offers
    }

    fn chooser(&self, _: &GenerateContext<'_>, id: &str) -> Option<Chooser> {
        (id == "fields").then(|| Chooser {
            title: "Choose Fields".to_owned(),
            members: ["x", "y"]
                .iter()
                .map(|name| Member {
                    id: (*name).to_owned(),
                    name: (*name).to_owned(),
                    selected: true,
                    ..Member::default()
                })
                .collect(),
            options: vec![ChooserOption { id: "upper".to_owned(), label: "Upper case".to_owned(), value: false }],
            allow_empty: false,
            search: false,
        })
    }

    fn generate(&self, context: &GenerateContext<'_>, id: &str, chosen: &Chosen) -> Result<Insertion, String> {
        let at = context.text.chars().count();
        if id == "mark" {
            return Ok(Insertion { at, end: None, text: "\n// mark\n".to_owned(), also: Vec::new() });
        }
        let names = chosen.members.join(", ");
        let names = if chosen.options.get("upper") == Some(&true) { names.to_uppercase() } else { names };
        Ok(Insertion { at, end: None, text: format!("\n{}// {names}\n", context.indent), also: Vec::new() })
    }
}

struct Reading(std::sync::atomic::AtomicBool);

impl CodeGenerator for Reading {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn offers(&self, _: &GenerateContext<'_>) -> Vec<Offer> {
        vec![Offer {
            id: "stub".to_owned(),
            title: "Stub".to_owned(),
            chooser: true,
            action: Some("implement".to_owned()),
        }]
    }

    fn pending(&self, _: &GenerateContext<'_>, _: &str) -> bool {
        self.0.swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    fn chooser(&self, _: &GenerateContext<'_>, _: &str) -> Option<Chooser> {
        Some(Chooser {
            title: "Stub".to_owned(),
            members: vec![Member { id: "a".to_owned(), name: "a".to_owned(), ..Member::default() }],
            search: true,
            ..Chooser::default()
        })
    }

    fn generate(&self, context: &GenerateContext<'_>, _: &str, _: &Chosen) -> Result<Insertion, String> {
        let at = context.text.chars().count();
        Ok(Insertion { at, end: None, text: "\n// stub\n".to_owned(), also: vec![(0..0, "// head\n".to_owned())] })
    }
}

#[test]
fn a_generator_still_reading_answers_pending_and_its_two_edits_are_one_undo_step() {
    let mut fixture = project();
    let generator = Arc::new(Reading(std::sync::atomic::AtomicBool::new(true)));
    fixture.session.state_mut::<CodeGenerators>().add(generator);
    fixture.server.on("textDocument/codeAction", json!([]));
    let listed = list(&mut fixture);
    assert_eq!(listed.generators[0].action.as_deref(), Some("implement"));
    let id = listed.generators[0].id.clone();
    let members = fixture.dispatch("editor.generate.members", json!({ "id": id })).unwrap();
    assert_eq!((&members["pending"], &members["chooser"]), (&json!(true), &json!(null)));
    let members = fixture.dispatch("editor.generate.members", json!({ "id": id })).unwrap();
    assert_eq!(members["chooser"]["search"], true);

    fixture.dispatch("editor.generate.apply", json!({ "id": id, "members": ["a"] })).unwrap();
    assert_eq!(text(&fixture), format!("// head\n{MAIN}\n// stub\n"));
    let caret = fixture.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
    assert_eq!(caret, "// head\n".len() + MAIN.chars().count() + 1, "the caret follows the code past the other edit");
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);
}

#[test]
fn a_languages_own_generator_comes_first_and_writes_the_chosen_members() {
    let mut fixture = project();
    fixture.session.state_mut::<CodeGenerators>().add(Arc::new(Fields));
    fixture
        .server
        .on("textDocument/codeAction", json!([{ "title": "Add test", "kind": "source.addTest", "edit": {} }]));
    let listed = list(&mut fixture);
    assert_eq!(titles(&listed), ["Field List", "Mark", "Add test"]);
    assert_eq!((listed.generators[0].chooser, listed.generators[1].chooser), (true, false));
    let id = listed.generators[0].id.clone();
    let members = fixture.dispatch("editor.generate.members", json!({ "id": id })).unwrap();
    assert_eq!(members["pending"], false);
    assert_eq!(members["chooser"]["members"].as_array().unwrap().len(), 2);
    let refused = fixture.dispatch("editor.generate.members", json!({ "id": listed.generators[1].id }));
    assert_eq!(failure(refused), t!("workbench-generate-no-chooser"));

    let nothing = fixture.dispatch("editor.generate.apply", json!({ "id": id, "members": [] }));
    assert_eq!(failure(nothing), t!("workbench-generate-nothing-chosen"));
    let listed = list(&mut fixture);
    let id = listed.generators[0].id.clone();
    let applied = fixture.dispatch("editor.generate.apply", json!({ "id": id, "options": { "upper": true } })).unwrap();
    assert_eq!(applied["title"], "Field List");
    assert_eq!(text(&fixture), format!("{MAIN}\n\t// X, Y\n"));
    let caret = fixture.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
    assert_eq!(caret, MAIN.chars().count() + 2, "the caret goes to the generated code");
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);

    let listed = list(&mut fixture);
    fixture.with(|_, workbench| {
        workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "\n".to_owned())]);
    });
    let stale = fixture.dispatch("editor.generate.apply", json!({ "id": listed.generators[1].id }));
    assert_eq!(failure(stale), t!("workbench-generate-stale"));
}

#[test]
fn member_pointers_walk_nested_lists_and_keep_the_parent() {
    let answer = json!({ "delegateFields": [
        { "field": { "name": "list" }, "delegateMethods": [{ "name": "size", "parameters": [] }, { "name": "get", "parameters": ["int"] }] },
        { "field": { "name": "map" }, "delegateMethods": [{ "name": "clear", "parameters": [] }] }
    ] });
    let found = items(&answer, "/delegateFields/*/delegateMethods/*");
    let shown: Vec<(String, String)> = found
        .iter()
        .map(|item| (text_template("{/name}({/parameters})", item), text_template("{parent/field/name}", item)))
        .collect();
    assert_eq!(
        shown,
        [("size()", "list"), ("get(int)", "list"), ("clear()", "map")].map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
    let value = json!({ "field": "${parent:/field}", "delegateMethod": "${item}" });
    let no_members = BTreeMap::new();
    let no_options = BTreeMap::new();
    let scope = Scope { context: &Value::Null, members: &no_members, options: &no_options, item: Some(&found[2]) };
    assert_eq!(
        fill(&value, &scope),
        json!({ "field": { "name": "map" }, "delegateMethod": { "name": "clear", "parameters": [] } })
    );

    let root = json!([{ "fieldName": "a" }, { "fieldName": "b" }]);
    assert_eq!(items(&root, "").len(), 2, "an array answer is the list itself");
    assert_eq!(items(&root, "/*").len(), 2);
    assert_eq!(items(&json!({ "fields": [1, 2, 3] }), "/fields").len(), 3);
    assert!(items(&json!({}), "/fields/*").is_empty());
}

#[test]
fn agents_generate_nothing_in_a_file_hidden_from_them() {
    let mut fixture = project();
    let listed = list(&mut fixture);
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["main.go".to_owned()]);
    let id = listed.generators.first().map_or_else(String::new, |generator| generator.id.clone());
    for (command, params) in [
        ("editor.generate.list", json!({ "offset": 0 })),
        ("editor.generate.members", json!({ "id": id })),
        ("editor.generate.apply", json!({ "id": id })),
    ] {
        let Fixture { registry, session, .. } = &mut fixture;
        let refused = zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params));
        match refused {
            Err(DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::hidden_files::REDACTED_CODE), "{command}");
            }
            other => panic!("{command}: {other:?}"),
        }
    }
    assert_eq!(text(&fixture), MAIN);
}

fn java_generate() -> zen_lsp::GenerateDeclaration {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/java/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    serde_json::from_value(manifest["languageServers"][0]["generate"].clone()).unwrap()
}

fn jdt_action(title: &str, kind: &str, command: &str, argument: Value) -> Value {
    json!({ "title": title, "kind": kind, "command": { "title": title, "command": command, "arguments": [argument] } })
}

fn field(name: &str, kind: &str, selected: bool) -> Value {
    json!({ "bindingKey": name, "name": name, "type": kind, "isField": true, "isSelected": selected })
}

fn jdt_project() -> Fixture {
    let mut fixture = project();
    fixture.with(|languages, _| languages.declare_generate_for_test(&java_generate()));
    let at = json!({ "range": { "start": { "line": 8, "character": 0 } } });
    let with_kind = |kind: u64| {
        let mut argument = at.clone();
        argument["kind"] = json!(kind);
        argument
    };
    fixture.server.on(
        "textDocument/codeAction",
        json!([
            jdt_action(
                "Generate Getters and Setters...",
                "source.generate.accessors",
                "java.action.generateAccessorsPrompt",
                with_kind(2)
            ),
            jdt_action(
                "Generate Getters...",
                "source.generate.accessors",
                "java.action.generateAccessorsPrompt",
                with_kind(0)
            ),
            jdt_action(
                "Generate Setters...",
                "source.generate.accessors",
                "java.action.generateAccessorsPrompt",
                with_kind(1)
            ),
            jdt_action(
                "Generate Constructors...",
                "source.generate.constructors",
                "java.action.generateConstructorsPrompt",
                at.clone()
            ),
            jdt_action(
                "Generate hashCode() and equals()...",
                "source.generate.hashCodeEquals",
                "java.action.hashCodeEqualsPrompt",
                at.clone()
            ),
            jdt_action(
                "Generate toString()...",
                "source.generate.toString",
                "java.action.generateToStringPrompt",
                at.clone()
            ),
            jdt_action(
                "Override/Implement Methods...",
                "source.overrideMethods",
                "java.action.overrideMethodsPrompt",
                at.clone()
            ),
            jdt_action(
                "Generate Delegate Methods...",
                "source.generate.delegateMethods",
                "java.action.generateDelegateMethodsPrompt",
                at.clone()
            )
        ]),
    );
    let fields = json!([field("id", "String", true), field("count", "int", false)]);
    let mut to_string_fields = fields.clone();
    to_string_fields
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "hashCode", "type": "int", "isField": false, "isSelected": false, "parameters": [] }));
    fixture.server.on(
        "java/resolveUnimplementedAccessors",
        json!([
            { "fieldName": "id", "isStatic": false, "generateGetter": true, "generateSetter": true, "typeName": "String" },
            { "fieldName": "count", "isStatic": false, "generateGetter": true, "generateSetter": true, "typeName": "int" }
        ]),
    );
    fixture.server.on(
        "java/checkConstructorsStatus",
        json!({ "constructors": [{ "name": "Base", "parameters": [] }, { "name": "Base", "parameters": ["String"] }], "fields": fields }),
    );
    fixture.server.on(
        "java/checkHashCodeEqualsStatus",
        json!({ "type": "Order", "fields": fields, "existingMethods": ["equals", "hashCode"] }),
    );
    fixture
        .server
        .on("java/checkToStringStatus", json!({ "type": "Order", "fields": to_string_fields, "exists": false }));
    fixture.server.on(
        "java/listOverridableMethods",
        json!({ "type": "Order", "methods": [
            { "name": "toString", "parameters": [], "unimplemented": false, "declaringClass": "java.lang.Object" },
            { "name": "area", "parameters": [], "unimplemented": true, "declaringClass": "com.acme.shop.Shape" },
            { "name": "scale", "parameters": ["double"], "unimplemented": true, "declaringClass": "com.acme.shop.Shape" }
        ] }),
    );
    fixture.server.on(
        "java/checkDelegateMethodsStatus",
        json!({ "delegateFields": [
            { "field": { "name": "id", "type": "String" }, "delegateMethods": [{ "name": "length", "parameters": [] }] },
            { "field": { "name": "items", "type": "List<String>" }, "delegateMethods": [
                { "name": "size", "parameters": [] }, { "name": "get", "parameters": ["int"] }
            ] }
        ] }),
    );
    let edit = insert_line(&fixture, "// generated\n");
    for request in [
        "java/generateAccessors",
        "java/generateConstructors",
        "java/generateHashCodeEquals",
        "java/generateToString",
        "java/addOverridableMethods",
        "java/generateDelegateMethods",
    ] {
        fixture.server.on(request, edit.clone());
    }
    fixture
}

fn chooser_of(fixture: &mut Fixture, title: &str) -> (String, Chooser) {
    let listed = list(fixture);
    let generator = listed.generators.iter().find(|generator| generator.title == title).unwrap();
    assert!(generator.chooser, "{title}");
    let id = generator.id.clone();
    let members = fixture.dispatch("editor.generate.members", json!({ "id": id, "waitMs": 5000 })).unwrap();
    (id, serde_json::from_value(members["chooser"].clone()).unwrap())
}

fn generated(fixture: &mut Fixture, title: &str, members: Option<&[&str]>, request: &str) -> Value {
    let (id, _) = chooser_of(fixture, title);
    let mut params = json!({ "id": id });
    if let Some(members) = members {
        params["members"] = json!(members);
    }
    let applied = fixture.dispatch("editor.generate.apply", params).unwrap();
    assert_eq!(applied["title"], title);
    assert!(text(fixture).ends_with("// generated\n"), "{title}");
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    fixture.server.requests(request).last().cloned().unwrap()
}

fn rows(chooser: &Chooser) -> Vec<String> {
    chooser
        .members
        .iter()
        .map(|member| {
            let group = member.group.as_deref().map(|group| format!("{group} / ")).unwrap_or_default();
            let mark = if member.selected { "[x]" } else { "[ ]" };
            format!("{mark} {group}{}", member.name)
        })
        .collect()
}

#[test]
fn the_java_plugin_offers_every_jdt_prompt_in_the_order_of_the_generate_popup() {
    zen_i18n::set_language(Some("en"));
    let mut fixture = jdt_project();
    let listed = list(&mut fixture);
    assert_eq!(
        titles(&listed),
        [
            "Constructor",
            "Getter",
            "Setter",
            "Getter and Setter",
            "equals() and hashCode()",
            "toString()",
            "Override Methods",
            "Implement Methods",
            "Delegate Methods"
        ]
    );
    let actions: Vec<Option<&str>> = listed.generators.iter().map(|generator| generator.action.as_deref()).collect();
    assert_eq!(&actions[6..8], [Some("override"), Some("implement")]);
    assert!(listed.generators.iter().all(|generator| generator.chooser));
}

#[test]
fn java_constructor_and_accessors_send_the_chosen_fields() {
    let mut fixture = jdt_project();
    let (_, chooser) = chooser_of(&mut fixture, "Constructor");
    assert_eq!(chooser.title, "Choose Fields to Initialize by Constructor");
    assert_eq!(rows(&chooser), ["[x] Base / Base()", "[x] Base / Base(String)", "[x] id", "[ ] count"]);
    let sent =
        generated(&mut fixture, "Constructor", Some(&["constructors.1", "fields.1"]), "java/generateConstructors");
    assert_eq!(sent["context"]["range"]["start"]["line"], 8);
    assert_eq!(sent["constructors"], json!([{ "name": "Base", "parameters": ["String"] }]));
    assert_eq!(sent["fields"], json!([field("count", "int", false)]));

    for (title, kind) in [("Getter", 0), ("Setter", 1), ("Getter and Setter", 2)] {
        let (_, chooser) = chooser_of(&mut fixture, title);
        assert_eq!(rows(&chooser), ["[x] id", "[x] count"], "{title}");
        let sent = generated(&mut fixture, title, Some(&["fields.0"]), "java/generateAccessors");
        let asked = fixture.server.requests("java/resolveUnimplementedAccessors").last().cloned().unwrap();
        assert_eq!((asked["kind"].clone(), sent["context"]["kind"].clone()), (json!(kind), json!(kind)), "{title}");
        assert_eq!(sent["accessors"][0]["fieldName"], "id");
        assert_eq!(sent["accessors"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn java_equals_hash_code_and_to_string_send_the_chosen_fields_and_options() {
    zen_i18n::set_language(Some("en"));
    let mut fixture = jdt_project();
    let (_, chooser) = chooser_of(&mut fixture, "equals() and hashCode()");
    assert_eq!(rows(&chooser), ["[x] Order / id", "[ ] Order / count"]);
    let options: Vec<(&str, &str, bool)> =
        chooser.options.iter().map(|option| (option.id.as_str(), option.label.as_str(), option.value)).collect();
    assert_eq!(options, [("regenerate", "Replace the existing equals() and hashCode()", true)]);
    let sent = generated(&mut fixture, "equals() and hashCode()", None, "java/generateHashCodeEquals");
    assert_eq!(
        (sent["fields"].clone(), sent["regenerate"].clone()),
        (json!([field("id", "String", true)]), json!(true))
    );

    let (_, chooser) = chooser_of(&mut fixture, "toString()");
    assert_eq!(rows(&chooser), ["[x] Order / id", "[ ] Order / count"], "fields only, not methods");
    assert!(chooser.allow_empty);
    let sent = generated(&mut fixture, "toString()", Some(&["fields.1"]), "java/generateToString");
    assert_eq!(sent["fields"], json!([field("count", "int", false)]));
    let sent = generated(&mut fixture, "toString()", Some(&[]), "java/generateToString");
    assert_eq!(sent["fields"], json!([]));
}

#[test]
fn java_override_implement_and_delegate_methods_send_the_chosen_methods() {
    let mut fixture = jdt_project();
    let (_, chooser) = chooser_of(&mut fixture, "Override Methods");
    assert_eq!(chooser.title, "Select Methods to Override");
    assert_eq!(rows(&chooser), ["[ ] java.lang.Object / toString()"]);
    let (_, chooser) = chooser_of(&mut fixture, "Implement Methods");
    assert_eq!(rows(&chooser), ["[x] com.acme.shop.Shape / area()", "[x] com.acme.shop.Shape / scale(double)"]);
    let sent = generated(&mut fixture, "Implement Methods", Some(&["methods.1"]), "java/addOverridableMethods");
    assert_eq!(sent["overridableMethods"][0]["name"], "scale");
    assert_eq!(sent["overridableMethods"].as_array().unwrap().len(), 1);
    let sent = generated(&mut fixture, "Override Methods", Some(&["methods.0"]), "java/addOverridableMethods");
    assert_eq!(sent["overridableMethods"][0]["name"], "toString");

    let (_, chooser) = chooser_of(&mut fixture, "Delegate Methods");
    assert_eq!(
        rows(&chooser),
        ["[ ] id: String / length()", "[ ] items: List<String> / size()", "[ ] items: List<String> / get(int)"]
    );
    let sent = generated(&mut fixture, "Delegate Methods", Some(&["methods.2"]), "java/generateDelegateMethods");
    assert_eq!(
        sent["delegateEntries"],
        json!([{ "field": { "name": "items", "type": "List<String>" }, "delegateMethod": { "name": "get", "parameters": ["int"] } }])
    );
}

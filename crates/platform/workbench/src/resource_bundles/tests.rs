use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_syntax::{
    Expansion, Feature, KeyReferences, LineFold, OutlineItem, Outliner, Suggestions, SymbolKind, TextFeatures,
};

use super::*;
use crate::plugins::PluginFeatures;

struct Properties;

impl Outliner for Properties {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".properties")
    }

    fn outline(&self, text: &str) -> Vec<OutlineItem> {
        let chars = |byte: usize| text[..byte].chars().count();
        zen_plugin_properties::entries(text)
            .into_iter()
            .map(|entry| OutlineItem {
                name: entry.key,
                detail: None,
                kind: SymbolKind::Property,
                start: chars(entry.key_start),
                end: chars(entry.end),
                selection_start: chars(entry.key_start),
                children: Vec::new(),
            })
            .collect()
    }
}

impl TextFeatures for Properties {
    fn supports(&self, feature: Feature) -> bool {
        matches!(feature, Feature::Outline | Feature::Inlays)
    }

    fn inlays(&self, text: &str) -> Vec<zen_syntax::TextInlay> {
        zen_plugin_properties::inlays(text)
            .into_iter()
            .map(|inlay| zen_syntax::TextInlay { offset: inlay.offset, text: inlay.text })
            .collect()
    }

    fn folds(&self, _: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, _: &str, _: usize) -> Option<Suggestions> {
        None
    }

    fn expand(&self, _: &str, _: usize) -> Option<Expansion> {
        None
    }

    fn key_references(&self) -> Vec<KeyReferences> {
        serde_json::from_str(zen_plugin_properties::KEY_REFERENCES).unwrap()
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let resources = root.join("src/main/resources/i18n");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("messages.properties"), "# Greetings\nhello=Hello\nbye = Bye\n").unwrap();
        std::fs::write(resources.join("messages_ru.properties"), "hello=\\u041f\\u0440\\u0438\\u0432\\u0435\\u0442\n")
            .unwrap();
        std::fs::write(resources.join("messages_de_DE.properties"), "hello=Hallo\nbye=\n").unwrap();
        std::fs::write(resources.join("alone.properties"), "x=1\n").unwrap();
        let java = root.join("src/main/java/a");
        std::fs::create_dir_all(&java).unwrap();
        std::fs::write(
            java.join("App.java"),
            "class App {\n  ResourceBundle b = ResourceBundle.getBundle(\"i18n.messages\");\n  String s = b.getString(\"he\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            java.join("Go.java"),
            "class Go {\n  String s = b.getString(\"bye\");\n  Object o = getBundle(\"i18n/messages\");\n  String n = b.getString(\"nope\");\n}\n",
        )
        .unwrap();
        let mut session = Session::new();
        session.state_mut::<PluginFeatures>().set(vec![Arc::new(Properties)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["ready"] != true {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Self { registry, session, _dir: dir, root }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.root.join("src/main/resources/i18n").join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap()
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.file(name)).unwrap()
    }

    fn caret_after(&mut self, path: &Path, needle: &str) -> usize {
        let text = std::fs::read_to_string(path).unwrap();
        let at = text[..text.find(needle).unwrap() + needle.len()].chars().count();
        self.run("navigate.place.goTo", json!({ "path": path, "offset": at }));
        at
    }

    fn labels(&mut self, path: &Path, needle: &str) -> (Vec<String>, Value) {
        self.caret_after(path, needle);
        let list = self.run("editor.completion.start", json!({}));
        let labels = list["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["label"].as_str())
            .map(str::to_owned)
            .collect();
        (labels, list)
    }
}

#[test]
fn file_names_split_into_a_base_name_and_a_locale() {
    assert_eq!(split_name("messages.properties"), Some(("messages", "")));
    assert_eq!(split_name("messages_ru.properties"), Some(("messages", "ru")));
    assert_eq!(split_name("messages_de_DE.properties"), Some(("messages", "de_DE")));
    assert_eq!(split_name("my_app_en_US_WIN.properties"), Some(("my_app", "en_US_WIN")));
    assert_eq!(split_name("es_419.properties"), Some(("es_419", "")), "a locale needs a base name");
    assert_eq!(split_name("my_app.properties"), Some(("my_app", "")), "app is no language");
    assert_eq!(split_name("app.json"), None);
}

#[test]
fn a_bundle_is_two_files_or_more_of_one_base_name_in_one_folder() {
    let fixture = Fixture::new();
    let bundles = in_folder(&fixture.file(""));
    assert_eq!(bundles.len(), 1);
    let locales: Vec<&str> = bundles[0].files.iter().map(|file| file.locale.as_str()).collect();
    assert_eq!(locales, ["", "de_DE", "ru"]);
    assert_eq!(bundles[0].path(), fixture.file("messages.properties"));
    assert!(bundle_of(&fixture.file("alone.properties")).is_none());
}

#[test]
fn the_table_has_a_column_per_locale_and_marks_missing_translations() {
    let mut fixture = Fixture::new();
    let path = fixture.file("messages_ru.properties");
    let table = fixture.run("properties.bundle.get", json!({ "path": path }));
    assert_eq!(table["name"], "messages");
    assert_eq!(table["keys"][0]["key"], "hello");
    assert_eq!(table["keys"][0]["values"], json!(["Hello", "Hallo", "Привет"]));
    assert_eq!(table["keys"][1]["values"], json!(["Bye", "", null]));
    assert_eq!(table["keys"][1]["missing"], json!(["de_DE", "ru"]));
    assert_eq!(table["incomplete"], 1);
    let listed = fixture.run("properties.bundle.list", json!({}));
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[test]
fn adding_renaming_and_removing_a_key_changes_every_file_in_one_undo_step() {
    let mut fixture = Fixture::new();
    let path = fixture.file("messages.properties");
    let table = fixture.run("properties.bundle.addKey", json!({ "path": path, "key": "new key", "value": "Value" }));
    assert_eq!(table["keys"][2]["values"], json!(["Value", "", ""]));
    assert_eq!(fixture.read("messages.properties"), "# Greetings\nhello=Hello\nbye = Bye\nnew\\ key=Value\n");
    assert_eq!(fixture.read("messages_ru.properties").lines().last(), Some("new\\ key="));
    fixture.run("properties.bundle.renameKey", json!({ "path": path, "key": "hello", "newKey": "greeting" }));
    assert!(fixture.read("messages_ru.properties").starts_with("greeting=\\u041f"));
    assert!(fixture.read("messages_de_DE.properties").starts_with("greeting=Hallo"));
    let refused = fixture.registry.dispatch(
        &mut fixture.session,
        "properties.bundle.renameKey",
        json!({ "path": path, "key": "greeting", "newKey": "bye" }),
    );
    assert!(refused.is_err(), "the new name is taken");
    fixture.run("properties.bundle.removeKey", json!({ "path": path, "key": "bye" }));
    assert_eq!(fixture.read("messages_de_DE.properties"), "greeting=Hallo\nnew\\ key=\n");
    let table = fixture.run("properties.bundle.undo", json!({ "path": path }));
    assert_eq!(table["keys"][1]["key"], "bye");
    assert_eq!(fixture.read("messages_de_DE.properties"), "greeting=Hallo\nbye=\nnew\\ key=\n");
    fixture.run("properties.bundle.undo", json!({ "path": path }));
    assert!(fixture.read("messages_ru.properties").starts_with("hello="));
    assert!(fixture.read("messages.properties").contains("hello=Hello"));
}

#[test]
fn a_translation_is_written_escaped_into_its_own_file() {
    let mut fixture = Fixture::new();
    let path = fixture.file("messages.properties");
    let table = fixture.run(
        "properties.bundle.setValue",
        json!({ "path": path, "key": "bye", "locale": "ru", "value": " Пока\nвсем" }),
    );
    assert_eq!(table["keys"][1]["values"][2], " Пока\nвсем");
    assert_eq!(fixture.read("messages_ru.properties").lines().last(), Some("bye=\\ Пока\\nвсем"));
    fixture.run(
        "properties.bundle.setValue",
        json!({ "path": path, "key": "bye", "locale": "de_DE", "value": "Tschüss" }),
    );
    assert_eq!(fixture.read("messages_de_DE.properties"), "hello=Hallo\nbye=Tschüss\n");
    assert_eq!(fixture.read("messages.properties"), "# Greetings\nhello=Hello\nbye = Bye\n", "untouched");
}

#[test]
fn an_open_file_of_the_bundle_changes_in_its_editor() {
    let mut fixture = Fixture::new();
    let ru = fixture.file("messages_ru.properties");
    fixture.run("file.document.open", json!({ "path": ru }));
    fixture.run(
        "properties.bundle.setValue",
        json!({ "path": ru, "key": "hello", "locale": "ru", "value": "Здравствуй" }),
    );
    let text = fixture.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text, "hello=Здравствуй\n");
    assert!(fixture.read("messages_ru.properties").starts_with("hello=\\u041f"), "not saved yet");
}

#[test]
fn literals_name_the_call_they_are_the_first_argument_of() {
    let text = "x = b.getString( \"a.b\" ) + f(1, \"no\") + \"plain\" // getString(\"c\")";
    let at = text.find("a.b").unwrap();
    assert_eq!(
        references::literal_at(text, at + 1),
        Some(references::Literal { call: "getString".into(), start: at, end: at + 3 })
    );
    assert_eq!(references::literal_at(text, text.find("no").unwrap()), None);
    assert_eq!(references::literal_at(text, text.find("plain").unwrap()), None);
    assert_eq!(references::literal_at(text, text.find("\"c").unwrap() + 1), None, "in a comment");
}

#[test]
fn bundle_names_are_paths_under_the_resources_folder() {
    let root = Path::new("/p");
    assert_eq!(
        references::bundle_name(Path::new("/p/src/main/resources/i18n/messages_ru.properties"), Some(root)),
        Some("i18n.messages".into())
    );
    assert_eq!(references::bundle_name(Path::new("/p/conf/app.properties"), Some(root)), Some("conf.app".into()));
}

#[test]
fn keys_of_the_bundles_a_java_file_names_complete_in_its_string_literals() {
    let mut fixture = Fixture::new();
    let java = fixture.root.join("src/main/java/a/App.java");
    let (labels, list) = fixture.labels(&java, "getString(\"he");
    assert_eq!(labels, ["hello"], "{list}");
    assert_eq!(list["prefix"], "he");
    let (labels, list) = fixture.labels(&java, "getBundle(\"i18n.");
    assert_eq!(labels, ["i18n.messages"], "{list}");
}

#[test]
fn a_bundle_file_completes_the_keys_its_siblings_have() {
    let mut fixture = Fixture::new();
    let ru = fixture.file("messages_ru.properties");
    std::fs::write(&ru, "hello=x\nb").unwrap();
    let (labels, list) = fixture.labels(&ru, "\nb");
    assert_eq!(labels, ["bye"], "{list}");
}

#[test]
fn go_to_declaration_on_a_key_opens_it_in_the_default_file() {
    let mut fixture = Fixture::new();
    let java = fixture.root.join("src/main/java/a/Go.java");
    let at = fixture.caret_after(&java, "getString(\"b");
    let state = fixture.run("navigate.declaration.goTo", json!({ "path": java, "offset": at }));
    assert_eq!(state["navigated"], true, "{state}");
    let workbench = fixture.session.state::<Workbench>().unwrap();
    let document = workbench.active_document().unwrap();
    assert_eq!(document.path(), fixture.file("messages.properties"));
    assert_eq!(document.selections().primary().range(), 24..27);
    let at = fixture.caret_after(&java, "getBundle(\"i18n");
    let state = fixture.run("navigate.declaration.goTo", json!({ "path": java, "offset": at }));
    assert_eq!(state["navigated"], true, "{state}");
    let at = fixture.caret_after(&java, "getString(\"no");
    let state = fixture.run("navigate.declaration.goTo", json!({ "path": java, "offset": at }));
    assert!(state["error"].is_string() && state["navigated"] == false, "{state}");
}

#[test]
fn a_plug_in_s_inlays_show_as_inlay_hints_of_its_file() {
    let mut fixture = Fixture::new();
    let ru = fixture.file("messages_ru.properties");
    fixture.run("file.document.open", json!({ "path": ru }));
    let answer = fixture.run("editor.inlayHints.list", json!({ "path": ru }));
    assert_eq!(answer["hints"][0]["label"], "= Привет", "{answer}");
    assert_eq!((answer["hints"][0]["line"].clone(), answer["hints"][0]["column"].clone()), (json!(1), json!(43)));
    let again = fixture.run("editor.inlayHints.list", json!({ "path": ru, "since": answer["generation"] }));
    assert!(again["hints"].is_null(), "unchanged: {again}");
    fixture.session.settings_mut().set(crate::language::inlay_hints::ENABLED, false).unwrap();
    let off = fixture.run("editor.inlayHints.list", json!({ "path": ru }));
    assert!(off["hints"].as_array().is_none_or(Vec::is_empty), "{off}");
}

#[test]
fn agents_neither_read_nor_change_a_bundle_with_a_file_hidden_from_them() {
    let mut fixture = Fixture::new();
    let path = fixture.file("messages.properties");
    fixture
        .session
        .state_mut::<crate::hidden_files::HiddenFromAgents>()
        .set(&["src/main/resources/i18n/messages_ru.properties".to_owned()]);
    for (command, params) in [
        ("properties.bundle.get", json!({ "path": path })),
        ("properties.bundle.addKey", json!({ "path": path, "key": "k", "value": "v" })),
        ("properties.bundle.removeKey", json!({ "path": path, "key": "bye" })),
    ] {
        let Fixture { registry, session, .. } = &mut fixture;
        let refused = zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params));
        let code = match refused {
            Err(zen_core::DispatchError::Failed { error, .. }) => error.code,
            other => panic!("{command}: {other:?}"),
        };
        assert_eq!(code.as_deref(), Some(crate::hidden_files::REDACTED_CODE), "{command}");
    }
    assert!(fixture.read("messages.properties").contains("bye = Bye"), "nothing changed");
    let table = fixture.run("properties.bundle.get", json!({ "path": path }));
    assert_eq!(table["locales"].as_array().unwrap().len(), 3, "the person sees the whole bundle");
}

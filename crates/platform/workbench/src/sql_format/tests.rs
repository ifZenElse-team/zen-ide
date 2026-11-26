use serde_json::{Value, json};
use zen_core::Registry;

use super::*;

struct Window {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Window {
    fn new(files: &[(&str, &str)]) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        crate::database::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        Self { registry, session, dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn open(&mut self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        self.run("file.document.open", json!({ "path": path }));
        path
    }

    fn text(&self) -> String {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn reformat(&mut self, params: Value) -> Value {
        let mut params = params;
        params["waitMs"] = json!(10_000);
        self.run("editor.code.reformat", params)
    }
}

const SCRIPT: &str = "select a, b from t where x = 1;\n-- keep me\nselect 'é  x' from u;\n";

#[test]
fn reformat_code_formats_a_whole_sql_file_as_one_undo_step() {
    let mut window = Window::new(&[("query.sql", SCRIPT)]);
    window.open("query.sql");
    let options = FormatOptions { keyword_case: zen_db::format::KeywordCase::Upper, ..FormatOptions::default() };
    window.session.settings_mut().set(OPTIONS, options).unwrap();
    let done = window.reformat(json!({}));
    assert_eq!(done["changed"], true, "{done}");
    assert_eq!(done["pending"], false);
    assert_eq!(window.text(), "SELECT a,\n    b\nFROM t\nWHERE x = 1;\n-- keep me\nSELECT 'é  x'\nFROM u;\n");
    let again = window.reformat(json!({}));
    assert_eq!(again["changed"], false, "idempotent");
    window.run("editor.edit.undo", json!({}));
    assert_eq!(window.text(), SCRIPT, "one undo step");
}

#[test]
fn a_selection_reformats_only_the_statements_it_touches() {
    let mut window = Window::new(&[("query.sql", SCRIPT)]);
    window.open("query.sql");
    let start = SCRIPT.find("select 'é").unwrap();
    let chars = SCRIPT[..start].chars().count();
    window.run("editor.selection.set", json!({ "selections": [{ "anchor": chars + 2, "head": chars + 6 }] }));
    window.reformat(json!({}));
    assert_eq!(window.text(), "select a, b from t where x = 1;\n-- keep me\nselect 'é  x'\nfrom u;\n");
    window.reformat(json!({ "wholeFile": true }));
    assert!(window.text().starts_with("select a,\n    b\nfrom t\nwhere x = 1;"), "{}", window.text());
}

#[test]
fn the_sql_code_style_gives_the_indentation_and_the_extension_the_dialect() {
    let mut window = Window::new(&[("q.mysql", "select `a  b`, c from t # note\n")]);
    window.session.settings_mut().set(&crate::code_style::key("sql", "useTabs"), true).unwrap();
    let path = window.open("q.mysql");
    assert!(is_sql(&path));
    window.reformat(json!({}));
    assert_eq!(window.text(), "select `a  b`,\n\tc\nfrom t # note\n");
}

#[test]
fn a_pending_reformat_is_waited_on() {
    let mut window = Window::new(&[("query.sql", SCRIPT)]);
    let path = window.open("query.sql");
    let mut answer = window.run("editor.code.reformat", json!({ "path": path, "waitMs": 0 }));
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while answer["pending"] == true && std::time::Instant::now() < deadline {
        answer = window.run("editor.code.reformat", json!({ "path": path, "waitMs": 50 }));
    }
    assert_eq!(answer["changed"], true, "{answer}");
}

#[test]
fn unformattable_statements_are_reported_and_left_alone() {
    let mut window = Window::new(&[("broken.sql", "select 'open from t\n")]);
    window.open("broken.sql");
    let done = window.reformat(json!({}));
    assert_eq!(done["changed"], false);
    assert!(done["error"].as_str().unwrap().contains("1 statement was left"), "{done}");
    assert_eq!(window.text(), "select 'open from t\n");
}

#[test]
fn format_sql_answers_text_for_agents() {
    let mut window = Window::new(&[]);
    let done = window.run(
        "db.sql.format",
        json!({ "text": "select $x$ a  b $x$, c from t", "dialect": "postgres", "options": { "keywordCase": "upper" }, "indent": 2 }),
    );
    assert_eq!(done["text"], "SELECT $x$ a  b $x$,\n  c\nFROM t");
    assert_eq!(
        (done["changed"].as_bool(), done["statements"].as_u64(), done["truncated"].as_bool()),
        (Some(true), Some(1), Some(false))
    );
    let cut = window.run("db.sql.format", json!({ "text": "select a, b from t", "limit": 5 }));
    assert_eq!((cut["text"].as_str(), cut["truncated"].as_bool()), (Some("selec"), Some(true)));
    let long = "x".repeat(MAX_TEXT + 1);
    assert!(window.registry.dispatch(&mut window.session, "db.sql.format", json!({ "text": long })).is_err());
    let unknown = window.registry.dispatch(
        &mut window.session,
        "db.sql.format",
        json!({ "text": "select 1", "connection": "nope" }),
    );
    assert!(unknown.unwrap_err().to_string().contains("nope"));
}

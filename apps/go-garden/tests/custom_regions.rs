use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::Workbench;

fn open(name: &str, text: &str) -> (Registry, Session, tempfile::TempDir) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join(name);
    std::fs::write(&path, text).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.state_mut::<zen_workbench::brackets::Highlighters>().set_source(Arc::new(|name: &str| {
        if name.ends_with(".go") {
            Some(Arc::new(zen_lang_go::GoHighlighter) as _)
        } else if name.ends_with(".rs") {
            Some(Arc::new(zen_lang_rust::RustHighlighter) as _)
        } else {
            None
        }
    }));
    registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
    (registry, session, folder)
}

fn regions(registry: &Registry, session: &mut Session, text: &str) -> Vec<(String, String)> {
    let listed: Value = registry.dispatch(session, "editor.fold.regions", json!({})).unwrap();
    let folds: Value = registry.dispatch(session, "editor.fold.list", json!({})).unwrap();
    let listed: Vec<(String, String)> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|region| {
            let (start, end) = (region["start"].as_u64().unwrap() as usize, region["end"].as_u64().unwrap() as usize);
            let held: String = text.chars().skip(start).take(end - start).collect();
            assert!(
                folds.as_array().unwrap().iter().any(|fold| fold["kind"] == "region" && fold["start"] == start),
                "{folds}"
            );
            (region["description"].as_str().unwrap().to_owned(), held)
        })
        .collect();
    listed
}

fn upto_last_endregion(text: &str, from: &str) -> String {
    let end = text.rfind("endregion").unwrap() + "endregion".len();
    text[text.find(from).unwrap()..end].to_owned()
}

const GO: &str = "package main\n\n// region Real\nvar raw = `\n// endregion\n`\n\n/*\n// endregion\n\t// region Not one\n*/\n\nfunc f() {\n\t// region Inner\n\tx := \"a\"\n\t// endregion\n}\n// endregion\n";

#[test]
fn go_region_comments_in_raw_strings_and_block_comments_do_not_count() {
    let (registry, mut session, _folder) = open("main.go", GO);
    assert_eq!(
        regions(&registry, &mut session, GO),
        [
            ("Real".to_owned(), upto_last_endregion(GO, "// region Real")),
            ("Inner".to_owned(), "// region Inner\n\tx := \"a\"\n\t// endregion".to_owned()),
        ]
    );
    let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
    let open = GO.find("var raw = `\n").unwrap();
    let close = GO.find("`\n\n/*").unwrap();
    document
        .replace_ranges(vec![(open..open + "var raw = `\n".len(), String::new()), (close..close + 2, String::new())]);
    let edited = document.text().to_owned();
    assert_eq!(
        regions(&registry, &mut session, &edited),
        [
            ("Real".to_owned(), "// region Real\n// endregion".to_owned()),
            ("Inner".to_owned(), "// region Inner\n\tx := \"a\"\n\t// endregion".to_owned()),
        ],
        "the highlighting follows the edit: out of the string, the comment closes the region"
    );
}

const RUST: &str = "// region Real\nconst RAW: &str = r#\"\n// endregion\n\"#;\n/*\n// endregion\n*/\nconst S: &str = \"\n// endregion\n\";\nfn f() {\n    // region Inner\n    let x = 1;\n    // endregion\n}\n// endregion\n";

#[test]
fn rust_region_comments_in_strings_and_block_comments_do_not_count() {
    let (registry, mut session, _folder) = open("lib.rs", RUST);
    assert_eq!(
        regions(&registry, &mut session, RUST),
        [
            ("Real".to_owned(), upto_last_endregion(RUST, "// region Real")),
            ("Inner".to_owned(), "// region Inner\n    let x = 1;\n    // endregion".to_owned()),
        ]
    );
}

const PYTHON: &str = "# region Real\ndoc = \"\"\"\n# endregion\n\"\"\"\nquote = '# endregion' # it's\nraw = '''\n# endregion\n'''\n# endregion\n";

#[test]
fn python_region_comments_in_strings_do_not_count_without_a_highlighter() {
    let (registry, mut session, _folder) = open("tool.py", PYTHON);
    assert_eq!(
        regions(&registry, &mut session, PYTHON),
        [("Real".to_owned(), upto_last_endregion(PYTHON, "# region"))]
    );
}

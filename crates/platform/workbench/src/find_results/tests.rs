use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_syntax::{OutlineItem, Outliner, SymbolKind};

use super::*;

struct Dirs {
    project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Dirs {
    fn path(&self) -> &Path {
        self.project.path()
    }
}

fn project(files: &[(&str, &[u8])]) -> (Dirs, Registry, Session) {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Workbench::default());
    let data = tempfile::tempdir().unwrap();
    session.set_state(crate::local_history::LocalHistory::open(
        data.path().join("local-history"),
        crate::local_history::Limits::default(),
    ));
    crate::find::tests::literals(&mut session);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["ready"] != true {
        std::thread::sleep(Duration::from_millis(5));
    }
    (Dirs { project: dir, _data: data }, registry, session)
}

fn run(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Value {
    registry.dispatch(session, id, params.clone()).unwrap_or_else(|error| panic!("{id} {params}: {error}"))
}

fn open(registry: &Registry, session: &mut Session, params: Value) -> Value {
    let mut params = params;
    params["waitMs"] = json!(5000);
    run(registry, session, "search.text.occurrences", params)
}

fn labels(node: &Value, depth: usize, out: &mut Vec<String>) {
    let mut text = format!("{}{}", "  ".repeat(depth), node["label"].as_str().unwrap());
    if let Some(detail) = node["detail"].as_str() {
        text.push_str(&format!(" [{detail}]"));
    }
    if node["kind"] != "usage" && node["kind"] != "target" && node["kind"] != "targets" {
        text.push_str(&format!(" ({})", node["count"]));
    }
    if node["excluded"] == true {
        text.push_str(" ~excluded~");
    }
    out.push(text);
    for child in node["children"].as_array().into_iter().flatten() {
        labels(child, depth + 1, out);
    }
}

fn shape(registry: &Registry, session: &mut Session) -> Vec<String> {
    let tree = run(registry, session, "search.results.tree", json!({ "waitMs": 5000 }));
    let mut out = Vec::new();
    for node in tree["nodes"].as_array().unwrap() {
        labels(node, 0, &mut out);
    }
    out
}

fn node_id(registry: &Registry, session: &mut Session, label: &str) -> String {
    fn find(node: &Value, label: &str) -> Option<String> {
        if node["label"] == label {
            return node["id"].as_str().map(str::to_owned);
        }
        node["children"].as_array().into_iter().flatten().find_map(|child| find(child, label))
    }
    let tree = run(registry, session, "search.results.tree", json!({}));
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|node| find(node, label))
        .unwrap_or_else(|| panic!("{label}: {tree}"))
}

const KUCOIN: &str = "package kucoin\n\n// Version of the client.\nvar Version = \"Version 1\" // Version\n";
const BOOT: &str = "package bootstrap\n\nfunc Run() string {\n\treturn Version\n}\n";

#[test]
fn the_occurrences_are_a_tree_of_usage_types_modules_compacted_folders_files_and_lines() {
    let (dir, registry, mut session) = project(&[
        ("kucoin/client.go", KUCOIN.as_bytes()),
        ("internal/bootstrap/run.go", BOOT.as_bytes()),
        ("internal/bootstrap/run_test.go", b"package bootstrap\n\nvar _ = Version\n"),
    ]);
    let module = dir.path().file_name().unwrap().to_string_lossy().into_owned();
    let found = open(&registry, &mut session, json!({ "query": { "text": "Version", "matchCase": true } }));
    assert_eq!(
        (&found["title"], &found["total"], &found["files"], &found["done"], &found["more"]),
        (&json!("Occurrences of 'Version'"), &json!(6), &json!(3), &json!(true), &json!(false)),
        "{found}"
    );
    assert_eq!(
        shape(&registry, &mut session),
        [
            "Targets".to_owned(),
            "  Occurrences of 'Version'".to_owned(),
            "Found occurrences in Project (6)".to_owned(),
            "  Unclassified (3)".to_owned(),
            format!("    {module} (3)"),
            "      internal/bootstrap (2)".to_owned(),
            "        run.go (1)".to_owned(),
            "          return Version".to_owned(),
            "        run_test.go (1)".to_owned(),
            "          var _ = Version".to_owned(),
            "      kucoin (1)".to_owned(),
            "        client.go (1)".to_owned(),
            "          var Version = \"Version 1\" // Version".to_owned(),
            "  Usage in comments (2)".to_owned(),
            format!("    {module} (2)"),
            "      kucoin (2)".to_owned(),
            "        client.go (2)".to_owned(),
            "          // Version of the client.".to_owned(),
            "          var Version = \"Version 1\" // Version".to_owned(),
            "  Usage in string constants (1)".to_owned(),
            format!("    {module} (1)"),
            "      kucoin (1)".to_owned(),
            "        client.go (1)".to_owned(),
            "          var Version = \"Version 1\" // Version".to_owned(),
        ],
        "a line's matches of one usage type are one row (Merge Usages from the Same Line)"
    );
    let tree = run(&registry, &mut session, "search.results.tree", json!({}));
    let strings = &tree["nodes"][1]["children"][2]["children"][0]["children"][0]["children"][0]["children"][0];
    assert_eq!((&strings["highlights"], &strings["line"]), (&json!([[15, 22]]), &json!(4)), "{strings}");

    let set = |session: &mut Session, option: &str, on: bool| {
        run(&registry, session, "search.results.setOption", json!({ "option": option, "on": on }))
    };
    set(&mut session, "usageType", false);
    set(&mut session, "module", false);
    set(&mut session, "mergeSameLine", false);
    set(&mut session, "showInComments", false);
    let options = set(&mut session, "directory", true);
    assert_eq!(options["directoryStructure"], false, "Group by Directory turns Directory Structure off");
    assert_eq!(
        shape(&registry, &mut session),
        [
            "Targets",
            "  Occurrences of 'Version'",
            "Found occurrences in Project (4)",
            "  internal/bootstrap (2)",
            "    run.go (1)",
            "      return Version",
            "    run_test.go (1)",
            "      var _ = Version",
            "  kucoin (2)",
            "    client.go (2)",
            "      var Version = \"Version 1\" // Version",
            "      var Version = \"Version 1\" // Version",
        ],
        "one row per match, the comments hidden"
    );
    set(&mut session, "directory", false);
    set(&mut session, "productionTest", true);
    set(&mut session, "showInComments", true);
    assert_eq!(
        shape(&registry, &mut session),
        [
            "Targets",
            "  Occurrences of 'Version'",
            "Found occurrences in Project (6)",
            "  Production (5)",
            "    client.go [kucoin] (4)",
            "      // Version of the client.",
            "      var Version = \"Version 1\" // Version",
            "      var Version = \"Version 1\" // Version",
            "      var Version = \"Version 1\" // Version",
            "    run.go [internal/bootstrap] (1)",
            "      return Version",
            "  Test (1)",
            "    run_test.go [internal/bootstrap] (1)",
            "      var _ = Version",
        ],
        "without folders a file shows its folder"
    );
    let test = run(&registry, &mut session, "search.results.tree", json!({ "depth": 2 }));
    assert_eq!(test["nodes"][1]["children"][1]["children"][0]["test"], true, "{test}");
    assert!(test["nodes"][1]["children"][1]["children"][0].get("children").is_none(), "depth limits the levels");
    session.settings_mut().reset(OPTIONS);
}

struct Funcs;

impl Outliner for Funcs {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn outline(&self, text: &str) -> Vec<OutlineItem> {
        let mut items = Vec::new();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if let Some(name) = line.strip_prefix("func ").and_then(|rest| rest.split('(').next()) {
                let end = text[offset..].find("\n}").map_or(text.chars().count(), |at| offset + at + 2);
                let chars = |byte: usize| text[..byte].chars().count();
                items.push(OutlineItem {
                    name: name.to_owned(),
                    detail: None,
                    kind: SymbolKind::Function,
                    start: chars(offset),
                    end: chars(end),
                    selection_start: chars(offset) + 5,
                    children: Vec::new(),
                });
            }
            offset += line.len();
        }
        items
    }
}

#[test]
fn file_structure_puts_the_usages_under_their_declarations_and_modules_nest_unless_flattened() {
    let text = "package a\n\nfunc One() {\n\tuse(x)\n}\n\nfunc Two() {\n\tuse(x)\n\tuse(x)\n}\n\nvar _ = use\n";
    let (_dir, registry, mut session) = project(&[("a.go", text.as_bytes())]);
    session.state_mut::<Outliners>().set(vec![Arc::new(Funcs)]);
    run(&registry, &mut session, "search.results.setOption", json!({ "option": "fileStructure", "on": true }));
    run(&registry, &mut session, "search.results.setOption", json!({ "option": "module", "on": false }));
    open(&registry, &mut session, json!({ "query": { "text": "use", "wholeWords": true } }));
    assert_eq!(
        shape(&registry, &mut session),
        [
            "Targets",
            "  Occurrences of 'use'",
            "Found occurrences in Project (4)",
            "  Unclassified (4)",
            "    a.go (4)",
            "      One (1)",
            "        use(x)",
            "      Two (2)",
            "        use(x)",
            "        use(x)",
            "      var _ = use",
        ]
    );
    session.settings_mut().reset(OPTIONS);

    let module = |name: &str, folder: &str| ModuleScope { name: name.to_owned(), folder: folder.to_owned() };
    let shape = Shape {
        options: ViewOptions::default(),
        modules: vec![module("tools", "app/tools"), module("app", "app"), module("root", "")],
        project: "p".to_owned(),
    };
    let names =
        |relative: &str| shape.modules_of(relative).iter().map(|module| module.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("app/tools/x.go"), ["root", "app", "tools"]);
    assert_eq!(names("application.go"), ["root"]);
    let found = Found {
        found: TextMatch {
            path: PathBuf::from("/p/app/tools/cmd/x.go"),
            relative: "app/tools/cmd/x.go".to_owned(),
            line: 0,
            column: 0,
            range: 0..1,
            line_text: "x".to_owned(),
        },
        usage_type: UsageType::Unclassified,
        members: Vec::new(),
        access: None,
        holder: None,
    };
    let ids =
        |shape: &Shape| group_path(&found, shape, false).into_iter().map(|segment| segment.id).collect::<Vec<_>>();
    assert_eq!(
        ids(&shape),
        ["type:unclassified", "module:root", "module:app", "module:tools", "dir:cmd", "file:app/tools/cmd/x.go"]
    );
    let mut flat = Shape { options: ViewOptions::default(), modules: shape.modules.clone(), project: "p".to_owned() };
    flat.options.flatten_modules = true;
    assert_eq!(ids(&flat), ["type:unclassified", "module:tools", "dir:cmd", "file:app/tools/cmd/x.go"]);
}

#[test]
fn the_search_runs_as_a_task_pauses_at_too_many_results_and_continues_or_aborts() {
    let files: Vec<(String, Vec<u8>)> =
        (0..100).map(|index| (format!("f{index}.txt"), b"hit hit\nhit\n".to_vec())).collect();
    let files: Vec<(&str, &[u8])> = files.iter().map(|(name, text)| (name.as_str(), text.as_slice())).collect();
    let (_dir, registry, mut session) = project(&files);
    let found = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 4 }));
    assert_eq!((&found["more"], &found["done"]), (&json!(true), &json!(false)), "{found}");
    assert!(found["total"].as_u64().unwrap() >= 4, "{found}");
    let tasks = run(&registry, &mut session, "tasks.background.list", json!({}));
    let task = tasks["tasks"].as_array().unwrap().iter().find(|task| task["source"] == TASK_SOURCE).unwrap();
    assert_eq!(
        (&task["title"], &task["state"], &task["cancellable"]),
        (&json!("Searching for 'hit'"), &json!("running"), &json!(true))
    );
    let list = run(&registry, &mut session, "search.results.list", json!({}));
    assert_eq!(list["tabs"][0]["more"], true, "{list}");
    let going = run(&registry, &mut session, "search.results.tooMany", json!({ "continue": true }));
    assert_eq!(going["more"], false);
    let tree = run(&registry, &mut session, "search.results.tree", json!({ "waitMs": 5000 }));
    assert_eq!((&tree["done"], &tree["total"]), (&json!(true), &json!(300)), "Continue lists every match");
    assert!(
        registry.dispatch(&mut session, "search.results.tooMany", json!({ "continue": true })).is_err(),
        "not asking"
    );
    let tasks = run(&registry, &mut session, "tasks.background.list", json!({}));
    let task = tasks["tasks"].as_array().unwrap().iter().rev().find(|task| task["source"] == TASK_SOURCE).unwrap();
    assert_eq!((&task["state"], &task["message"]), (&json!("succeeded"), &json!("Found 300 occurrences")), "{task}");

    let found = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 4 }));
    assert_eq!(found["more"], true);
    let first = found["total"].as_u64().unwrap();
    run(&registry, &mut session, "search.results.tooMany", json!({ "continue": false }));
    let tree = run(&registry, &mut session, "search.results.tree", json!({ "waitMs": 5000 }));
    assert_eq!(tree["done"], true);
    assert!(
        tree["total"].as_u64().unwrap() < 300 && tree["total"].as_u64().unwrap() >= first,
        "Abort keeps those found: {tree}"
    );
    let list = run(&registry, &mut session, "search.results.list", json!({}));
    assert_eq!(list["tabs"].as_array().unwrap().len(), 1, "the results took the place of the tab that is not pinned");

    run(&registry, &mut session, "search.results.pin", json!({}));
    let found = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 4 }));
    assert_eq!(found["more"], true);
    let cancelled = run(&registry, &mut session, "tasks.background.cancel", json!({}));
    assert!(cancelled.as_u64().is_some());
    let tree = run(&registry, &mut session, "search.results.tree", json!({ "waitMs": 5000 }));
    assert_eq!(tree["done"], true, "the × of the status bar stops it, Too Many Results or not");
    let list = run(&registry, &mut session, "search.results.list", json!({}));
    assert_eq!(list["tabs"].as_array().unwrap().len(), 2, "a pinned tab keeps its results");
    let tasks = run(&registry, &mut session, "tasks.background.list", json!({}));
    let task = tasks["tasks"].as_array().unwrap().iter().rev().find(|task| task["source"] == TASK_SOURCE).unwrap();
    assert!(task["message"].as_str().unwrap().starts_with("Search stopped"), "{task}");

    let found = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 0, "newTab": true }));
    assert_eq!((&found["total"], &found["more"]), (&json!(300), &json!(false)), "limit 0 never asks");
    assert_eq!(run(&registry, &mut session, "search.results.list", json!({}))["tabs"].as_array().unwrap().len(), 3);
    run(&registry, &mut session, "search.results.close", json!({}));
    assert_eq!(run(&registry, &mut session, "search.results.list", json!({}))["tabs"].as_array().unwrap().len(), 2);
}

#[test]
fn excluded_nodes_are_left_out_of_replace_selected_and_do_replace_all() {
    let (dir, registry, mut session) = project(&[
        ("a.go", b"package a\r\n\r\nvar Total = Total\r\n"),
        ("b/b.go", b"package b\n\nvar Total = 1\n"),
        ("c/c.go", b"package c\n\nvar Total = 2\n"),
    ]);
    let (a, b, c) = (dir.path().join("a.go"), dir.path().join("b/b.go"), dir.path().join("c/c.go"));
    run(&registry, &mut session, "file.document.open", json!({ "path": a }));
    run(&registry, &mut session, "search.results.setOption", json!({ "option": "mergeSameLine", "on": false }));
    let found = open(
        &registry,
        &mut session,
        json!({ "query": { "text": "Total", "matchCase": true }, "replacement": "Sum\nAll" }),
    );
    assert_eq!(found["total"], 4);
    let c_file = node_id(&registry, &mut session, "c.go");
    let excluded = run(&registry, &mut session, "search.results.exclude", json!({ "nodes": [c_file] }));
    assert_eq!(excluded["usages"], 1);
    let tree = shape(&registry, &mut session);
    assert!(tree.iter().any(|row| row.trim() == "c.go (1) ~excluded~"), "{tree:?}");
    assert!(
        tree.iter().any(|row| row.trim() == "c (1) ~excluded~"),
        "a folder whose usages are all excluded is: {tree:?}"
    );

    let b_file = node_id(&registry, &mut session, "b.go");
    run(&registry, &mut session, "search.results.select", json!({ "node": b_file }));
    let replaced = run(&registry, &mut session, "search.results.replace", json!({}));
    assert_eq!((&replaced["files"], &replaced["replacements"]), (&json!(1), &json!(1)), "Replace Selected");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "package b\n\nvar Sum\nAll = 1\n");
    let versions = run(&registry, &mut session, "localHistory.versions.list", json!({ "path": b }));
    assert!(
        !versions["versions"].as_array().unwrap().is_empty(),
        "a closed file gets a Local History version: {versions}"
    );
    assert!(!shape(&registry, &mut session).iter().any(|row| row.trim().starts_with("b.go")), "replaced usages leave");

    let replaced = run(&registry, &mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert_eq!((&replaced["files"], &replaced["replacements"]), (&json!(1), &json!(2)), "Do Replace All skips c.go");
    let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text, "package a\r\n\r\nvar Sum\r\nAll = Sum\r\nAll\r\n", "CRLF kept in the editor");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "package a\r\n\r\nvar Total = Total\r\n", "not saved by itself");
    run(&registry, &mut session, "editor.edit.undo", json!({}));
    let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text, "package a\r\n\r\nvar Total = Total\r\n", "one undo step takes the file's replacements back");
    assert_eq!(std::fs::read_to_string(&c).unwrap(), "package c\n\nvar Total = 2\n");

    let c_file = node_id(&registry, &mut session, "c.go");
    run(&registry, &mut session, "search.results.include", json!({ "nodes": [c_file] }));
    assert!(!shape(&registry, &mut session).iter().any(|row| row.contains("~excluded~")), "Include takes them back");
    let rest = run(&registry, &mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert_eq!((&rest["files"], &rest["replacements"]), (&json!(1), &json!(1)), "only c.go is left to replace");
    assert_eq!(std::fs::read_to_string(&c).unwrap(), "package c\n\nvar Sum\nAll = 2\n");
    let find_only = open(&registry, &mut session, json!({ "query": { "text": "Total" }, "newTab": true }));
    assert!(find_only["replacement"].is_null());
    assert!(registry.dispatch(&mut session, "search.results.replace", json!({ "nodes": ["found"] })).is_err());
    session.settings_mut().reset(OPTIONS);
}

#[test]
fn replacing_refuses_read_only_files_files_not_in_utf8_and_files_changed_on_disk() {
    let (dir, registry, mut session) =
        project(&[("a.go", b"package a\n\nvar Total = 1\n"), ("b.go", b"package b\n\nvar Total = 2\n")]);
    let (a, b) = (dir.path().join("a.go"), dir.path().join("b.go"));
    let params = json!({ "query": { "text": "Total", "matchCase": true }, "replacement": "Sum" });
    open(&registry, &mut session, params.clone());
    std::fs::write(&b, "package b\n\n\nvar Total = 2\n").unwrap();
    let changed = registry.dispatch(&mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert!(changed.unwrap_err().to_string().contains("changed since the search"));
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "package a\n\nvar Total = 1\n", "nothing replaced");

    std::fs::write(&b, b"package b\n\nvar Total = \xff\n").unwrap();
    let invalid = registry.dispatch(&mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert!(invalid.unwrap_err().to_string().contains("is not UTF-8 text"));

    std::fs::write(&b, "package b\n\nvar Total = 2\n").unwrap();
    let mut permissions = std::fs::metadata(&b).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&b, permissions.clone()).unwrap();
    let read_only = registry.dispatch(&mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert!(read_only.unwrap_err().to_string().contains("read-only"));
    let in_files = registry.dispatch(&mut session, "search.text.replaceAll", params);
    assert!(in_files.unwrap_err().to_string().contains("read-only"), "Replace in Files refuses it too");
    run(&registry, &mut session, "file.document.open", json!({ "path": b }));
    let open_read_only = registry.dispatch(&mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert!(open_read_only.unwrap_err().to_string().contains("read-only"), "open in the editor as well");
    let document = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.text(), "package b\n\nvar Total = 2\n");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "package a\n\nvar Total = 1\n");
    #[allow(clippy::permissions_set_readonly_false, reason = "the temporary folder is removed afterwards")]
    permissions.set_readonly(false);
    std::fs::set_permissions(&b, permissions).unwrap();
    let replaced = run(&registry, &mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert_eq!(replaced["replacements"], 2);
}

#[test]
fn next_and_previous_go_round_the_usages_and_rerun_searches_again() {
    let (dir, registry, mut session) =
        project(&[("a.go", b"package a\n\nvar Total = 1\n"), ("b.go", b"package b\n\n// Total\n")]);
    open(&registry, &mut session, json!({ "query": { "text": "Total", "matchCase": true } }));
    let first = run(&registry, &mut session, "search.results.next", json!({}));
    assert_eq!((&first["line"], first["path"].as_str().unwrap().ends_with("a.go")), (&json!(3), true));
    let active = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(active.selections().primary().head, first["offset"].as_u64().unwrap() as usize, "it went there");
    let second = run(&registry, &mut session, "search.results.next", json!({}));
    assert!(second["path"].as_str().unwrap().ends_with("b.go"));
    let round = run(&registry, &mut session, "search.results.next", json!({}));
    assert_eq!(round["node"], first["node"], "round to the first");
    let back = run(&registry, &mut session, "search.results.previous", json!({}));
    assert_eq!(back["node"], second["node"]);

    run(&registry, &mut session, "search.results.expand", json!({ "expanded": false }));
    let tree = run(&registry, &mut session, "search.results.tree", json!({}));
    assert_eq!(tree["nodes"][1]["children"][0]["collapsed"], true, "Collapse All: {tree}");
    assert!(tree["nodes"][1].get("collapsed").is_none(), "the top nodes stay open");
    run(&registry, &mut session, "search.results.expand", json!({ "expanded": true }));
    let tree = run(&registry, &mut session, "search.results.tree", json!({}));
    assert!(tree["nodes"][1]["children"][0].get("collapsed").is_none(), "Expand All");

    std::fs::write(dir.path().join("c.go"), "package c\n\nvar _ = Total\n").unwrap();
    let index = session.state::<Workbench>().unwrap().index.clone().unwrap();
    index.refresh(&[dir.path().join("c.go")]);
    let query = json!({ "query": { "text": "Total", "matchCase": true } });
    for _ in 0..500 {
        let found = run(&registry, &mut session, "search.text.find", query.clone());
        if found["matches"].as_array().unwrap().len() == 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let rerun = run(&registry, &mut session, "search.results.rerun", json!({ "waitMs": 5000 }));
    assert_eq!((&rerun["total"], &rerun["tab"]), (&json!(3), &first_tab(&registry, &mut session)), "{rerun}");
}

fn first_tab(registry: &Registry, session: &mut Session) -> Value {
    run(registry, session, "search.results.list", json!({}))["tabs"][0]["tab"].clone()
}

#[test]
fn replacing_usage_by_usage_in_one_file_keeps_the_others_in_place() {
    let (dir, registry, mut session) = project(&[("a.go", b"package a\n\nvar Total = Total\nvar _ = Total\n")]);
    let a = dir.path().join("a.go");
    run(&registry, &mut session, "search.results.setOption", json!({ "option": "mergeSameLine", "on": false }));
    let found = open(
        &registry,
        &mut session,
        json!({ "query": { "text": "Total", "matchCase": true }, "replacement": "Sum\nOf" }),
    );
    assert_eq!(found["total"], 3);
    let tree = run(&registry, &mut session, "search.results.tree", json!({}));
    let mut usages = Vec::new();
    fn walk(node: &Value, out: &mut Vec<String>) {
        if node["kind"] == "usage" {
            out.push(node["id"].as_str().unwrap().to_owned());
        }
        for child in node["children"].as_array().into_iter().flatten() {
            walk(child, out);
        }
    }
    walk(&tree["nodes"][1], &mut usages);
    run(&registry, &mut session, "search.results.replace", json!({ "nodes": [usages[0]] }));
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "package a\n\nvar Sum\nOf = Total\nvar _ = Total\n");
    let rest = run(&registry, &mut session, "search.results.tree", json!({}));
    let mut left = Vec::new();
    walk(&rest["nodes"][1], &mut left);
    assert_eq!(left.len(), 2, "{rest}");
    run(&registry, &mut session, "search.results.replace", json!({ "nodes": [left[1]] }));
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "package a\n\nvar Sum\nOf = Total\nvar _ = Sum\nOf\n");
    run(&registry, &mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "package a\n\nvar Sum\nOf = Sum\nOf\nvar _ = Sum\nOf\n");
    session.settings_mut().reset(OPTIONS);
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_written_puts_back_the_files_written_before_it() {
    use std::os::unix::fs::PermissionsExt;
    let names = ["a/1.go", "b/2.go", "c/3.go", "d/4.go", "e/5.go", "o.go"];
    let texts: Vec<String> = names.iter().map(|name| format!("package x\n\n// {name}\nvar Total = Total\n")).collect();
    let files: Vec<(&str, &[u8])> = names.iter().zip(&texts).map(|(name, text)| (*name, text.as_bytes())).collect();
    let (dir, registry, mut session) = project(&files);
    let open_file = dir.path().join("o.go");
    run(&registry, &mut session, "file.document.open", json!({ "path": open_file }));
    run(&registry, &mut session, "editor.edit.type", json!({ "text": "// unsaved Total\n" }));
    let unsaved = format!("// unsaved Total\n{}", texts[5]);
    open(&registry, &mut session, json!({ "query": { "text": "Total", "matchCase": true }, "replacement": "Sum" }));
    let locked = dir.path().join("c");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let failed = registry.dispatch(&mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(failed.is_err(), "the third file cannot be written");
    for (name, text) in names.iter().zip(&texts) {
        assert_eq!(&std::fs::read_to_string(dir.path().join(name)).unwrap(), text, "{name} is as it was");
    }
    let document = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.text(), unsaved, "the open file keeps its unsaved text, untouched");
    let versions =
        run(&registry, &mut session, "localHistory.versions.list", json!({ "path": dir.path().join("a/1.go") }));
    assert!(versions["versions"].as_array().is_none_or(Vec::is_empty), "nothing to remember: {versions}");
    let tree = run(&registry, &mut session, "search.results.tree", json!({}));
    assert_eq!(tree["total"], 13, "every usage is still listed: {tree}");

    let replaced = run(&registry, &mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert_eq!((&replaced["files"], &replaced["replacements"]), (&json!(6), &json!(13)));
    for name in &names[..5] {
        assert!(std::fs::read_to_string(dir.path().join(name)).unwrap().ends_with("var Sum = Sum\n"), "{name}");
    }
    let document = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.text(), unsaved.replace("Total", "Sum"), "the unsaved text is what changes");
    assert_eq!(std::fs::read_to_string(&open_file).unwrap(), texts[5], "an open file is not saved by itself");
    run(&registry, &mut session, "editor.edit.undo", json!({}));
    let document = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.text(), unsaved, "one undo step takes the replacement back");
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % below.max(1)
    }
}

fn usages_by_file(tree: &Value) -> BTreeMap<String, Vec<(String, bool)>> {
    fn walk(node: &Value, out: &mut BTreeMap<String, Vec<(String, bool)>>) {
        if node["kind"] == "usage" {
            let path = node["path"].as_str().unwrap().to_owned();
            out.entry(path).or_default().push((node["id"].as_str().unwrap().to_owned(), node["excluded"] == true));
        }
        for child in node["children"].as_array().into_iter().flatten() {
            walk(child, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(&tree["nodes"][1], &mut out);
    out
}

#[test]
fn random_excludes_includes_and_replacements_change_exactly_the_usages_left_in() {
    for seed in 1..=24_u64 {
        let mut random = Lcg(seed);
        let words = ["hit", "miss", "hit hit", "x hit y hit", "none"];
        let texts: Vec<String> = (0..5)
            .map(|_| {
                let lines = 2 + random.next(5);
                (0..lines).map(|_| words[random.next(words.len())]).collect::<Vec<_>>().join(if random.next(2) == 0 {
                    "\n"
                } else {
                    "\r\n"
                }) + "\n"
            })
            .collect();
        let names: Vec<String> = (0..5).map(|index| format!("d{}/f{index}.txt", index % 2)).collect();
        let files: Vec<(&str, &[u8])> =
            names.iter().zip(&texts).map(|(name, text)| (name.as_str(), text.as_bytes())).collect();
        let (dir, registry, mut session) = project(&files);
        let opened = random.next(6);
        if opened < 5 {
            run(&registry, &mut session, "file.document.open", json!({ "path": dir.path().join(&names[opened]) }));
        }
        run(&registry, &mut session, "search.results.setOption", json!({ "option": "mergeSameLine", "on": false }));
        open(&registry, &mut session, json!({ "query": { "text": "hit" }, "replacement": "a\nb" }));
        let mut left: BTreeMap<String, Vec<(usize, bool)>> = BTreeMap::new();
        for (name, text) in names.iter().zip(&texts) {
            let path = dir.path().join(name).display().to_string();
            left.insert(path, (0..text.matches("hit").count()).map(|ordinal| (ordinal, false)).collect());
        }
        let mut replaced: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for _ in 0..6 {
            let tree = run(&registry, &mut session, "search.results.tree", json!({}));
            let shown = usages_by_file(&tree);
            for (path, usages) in &shown {
                let ours = &left[path];
                assert_eq!(usages.len(), ours.len(), "seed {seed}: {path}: {tree}");
                for ((_, excluded), (_, expected)) in usages.iter().zip(ours) {
                    assert_eq!(excluded, expected, "seed {seed}: {tree}");
                }
            }
            let Some((path, usages)) = shown.iter().nth(random.next(shown.len())) else { break };
            let pick = random.next(usages.len());
            let (node, excluded) = &usages[pick];
            match random.next(3) {
                0 => {
                    let command = if *excluded { "search.results.include" } else { "search.results.exclude" };
                    run(&registry, &mut session, command, json!({ "nodes": [node] }));
                    let slot = &mut left.get_mut(path).unwrap()[pick].1;
                    *slot = !*slot;
                }
                _ if !excluded => {
                    run(&registry, &mut session, "search.results.replace", json!({ "nodes": [node] }));
                    let (ordinal, _) = left.get_mut(path).unwrap().remove(pick);
                    replaced.entry(path.clone()).or_default().push(ordinal);
                }
                _ => {}
            }
        }
        if left.values().flatten().any(|(_, excluded)| !excluded) {
            run(&registry, &mut session, "search.results.replace", json!({ "nodes": ["found"] }));
        }
        for (path, usages) in &left {
            for (ordinal, excluded) in usages {
                if !excluded {
                    replaced.entry(path.clone()).or_default().push(*ordinal);
                }
            }
        }
        for (index, (name, text)) in names.iter().zip(&texts).enumerate() {
            let path = dir.path().join(name);
            let chosen = replaced.get(&path.display().to_string()).cloned().unwrap_or_default();
            let crlf = text.contains("\r\n");
            let mut expected = String::new();
            for (ordinal, part) in text.split("hit").enumerate() {
                if ordinal > 0 {
                    let new = if crlf { "a\r\nb" } else { "a\nb" };
                    expected.push_str(if chosen.contains(&(ordinal - 1)) { new } else { "hit" });
                }
                expected.push_str(part);
            }
            let actual = if index == opened {
                let workbench = session.state::<Workbench>().unwrap();
                workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
            } else {
                std::fs::read_to_string(&path).unwrap()
            };
            assert_eq!(actual, expected, "seed {seed}: {name}");
        }
        session.settings_mut().reset(OPTIONS);
    }
}

#[test]
fn closing_or_replacing_a_tab_paused_at_too_many_results_ends_its_search() {
    let files: Vec<(String, Vec<u8>)> =
        (0..50).map(|index| (format!("f{index}.txt"), b"hit\nhit\n".to_vec())).collect();
    let files: Vec<(&str, &[u8])> = files.iter().map(|(name, text)| (name.as_str(), text.as_slice())).collect();
    let (_dir, registry, mut session) = project(&files);
    let running = |registry: &Registry, session: &mut Session| {
        let tasks = run(registry, session, "tasks.background.list", json!({}));
        tasks["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|task| task["source"] == TASK_SOURCE && task["state"] == "running")
            .count()
    };
    let ended = |registry: &Registry, session: &mut Session| {
        for _ in 0..500 {
            if running(registry, session) == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("a search is still running");
    };
    let found = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 3, "replacement": "miss" }));
    assert_eq!(found["more"], true);
    let early = registry.dispatch(&mut session, "search.results.replace", json!({ "nodes": ["found"] }));
    assert!(early.unwrap_err().to_string().contains("Wait for the search to end"), "no replacing while it runs");
    run(&registry, &mut session, "search.results.close", json!({}));
    ended(&registry, &mut session);

    let found = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 3 }));
    assert_eq!(found["more"], true);
    let next = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 0 }));
    assert_eq!(next["done"], true);
    ended(&registry, &mut session);

    let found = open(&registry, &mut session, json!({ "query": { "text": "hit" }, "limit": 3 }));
    assert_eq!(found["more"], true);
    let rerun = run(&registry, &mut session, "search.results.rerun", json!({ "waitMs": 5000 }));
    assert_eq!(rerun["more"], true, "{rerun}");
    run(&registry, &mut session, "search.results.stop", json!({}));
    ended(&registry, &mut session);
}

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::json;
use zen_core::{Registry, Session};
use zen_vfs::file_types::{self, Changes, DEFAULT_IGNORED, FileType};

use super::*;

static GLOBAL: Mutex<()> = Mutex::new(());

struct Restore(#[allow(dead_code, reason = "held for the test")] MutexGuard<'static, ()>);

impl Drop for Restore {
    fn drop(&mut self) {
        file_types::set_changes(Changes::default());
        file_types::set_overrides(BTreeMap::new());
    }
}

fn exclusive() -> Restore {
    let guard = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
    file_types::set_changes(Changes::default());
    file_types::set_overrides(BTreeMap::new());
    file_types::declare(platform_types());
    file_types::declare([
        FileType {
            id: "zt-go".to_owned(),
            name: "ZT Go".to_owned(),
            localized: false,
            patterns: vec!["*.go".to_owned()],
            hashbangs: Vec::new(),
            contents: Vec::new(),
        },
        FileType {
            id: "zt-yaml".to_owned(),
            name: "ZT YAML".to_owned(),
            localized: false,
            patterns: vec!["*.zt-yml".to_owned()],
            hashbangs: Vec::new(),
            contents: Vec::new(),
        },
        FileType {
            id: "zt-shell".to_owned(),
            name: "ZT Shell".to_owned(),
            localized: false,
            patterns: vec!["*.zt-sh".to_owned()],
            hashbangs: vec!["ztsh".to_owned()],
            contents: Vec::new(),
        },
        FileType {
            id: "zt-xml".to_owned(),
            name: "ZT XML".to_owned(),
            localized: false,
            patterns: vec!["*.zt-xml".to_owned()],
            hashbangs: Vec::new(),
            contents: vec![file_types::ContentMarker { starts: "<?zt-xml".to_owned(), ..Default::default() }],
        },
    ]);
    Restore(guard)
}

fn setup(data: &Path) -> (Registry, Session) {
    zen_i18n::register(crate::LOCALES);
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(FileTypeStore::load(Some(data.join(FILE_NAME))));
    (registry, session)
}

fn wait_for_index(registry: &Registry, session: &mut Session) {
    while registry.dispatch(session, "search.index.status", json!({})).unwrap()["ready"] != true {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn ignored_with(extra: &[&str]) -> Vec<String> {
    DEFAULT_IGNORED.iter().chain(extra).map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_list_names_every_type_with_its_patterns_and_the_default_ignored_names() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session) = setup(data.path());
    let listed = registry.dispatch(&mut session, "editor.fileTypes.list", json!({})).unwrap();
    let text = listed["types"].as_array().unwrap().iter().find(|ty| ty["id"] == TEXT).unwrap();
    assert_eq!(text["name"], "Text");
    assert_eq!(text["patterns"], json!(["*.txt", "*.text"]));
    assert_eq!(listed["ignored"], json!(DEFAULT_IGNORED));
    assert_eq!(listed["ignoredDefault"], true);
}

#[test]
fn a_moved_pattern_changes_the_type_code_style_and_is_kept_per_ide() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session) = setup(data.path());
    assert_eq!(crate::code_style::language_of(Path::new("/p/page.zt-yml")), "zt-yml");
    let added = registry
        .dispatch(&mut session, "editor.fileTypes.addPattern", json!({ "type": "zt-go", "pattern": "*.zt-yml" }))
        .unwrap();
    assert_eq!(added, json!({ "movedFrom": "zt-yaml", "movedFromName": "ZT YAML" }));
    assert_eq!(crate::code_style::language_of(Path::new("/p/page.zt-yml")), "go", "known as a Go file");
    let style = crate::code_style::for_path(&zen_core::Settings::default(), Path::new("/p/page.zt-yml"));
    assert!(style.use_tabs, "Go's code style");
    let typed =
        registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": "/p/page.zt-yml" })).unwrap();
    assert_eq!((&typed["id"], &typed["name"], &typed["byHashbang"]), (&json!("zt-go"), &json!("ZT Go"), &json!(false)));

    let saved = std::fs::read_to_string(data.path().join(FILE_NAME)).unwrap();
    assert!(saved.contains("\"pattern\": \"*.zt-yml\"") && saved.contains("\"type\": \"zt-go\""), "{saved}");
    file_types::set_changes(Changes::default());
    assert_eq!(crate::code_style::language_of(Path::new("/p/page.zt-yml")), "zt-yml");
    let _again = FileTypeStore::load(Some(data.path().join(FILE_NAME)));
    assert_eq!(crate::code_style::language_of(Path::new("/p/page.zt-yml")), "go", "read back at start");

    registry
        .dispatch(&mut session, "editor.fileTypes.removePattern", json!({ "type": "zt-go", "pattern": "*.zt-yml" }))
        .unwrap();
    assert_eq!(crate::code_style::language_of(Path::new("/p/page.zt-yml")), DEFAULT_LANGUAGE_ID);
    registry.dispatch(&mut session, "editor.fileTypes.reset", json!({})).unwrap();
    assert!(!data.path().join(FILE_NAME).exists(), "no changes, no file");
    assert_eq!(crate::code_style::language_of(Path::new("/p/page.zt-yml")), "zt-yml");
}

#[test]
fn content_markers_name_the_type_of_a_file_no_pattern_names() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session) = setup(data.path());
    let profile = data.path().join("cloud.zt-profile");
    std::fs::write(&profile, "\u{feff}\n  <?zt-xml version=\"1.0\"?>\n<root/>\n").unwrap();
    let typed = registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": profile })).unwrap();
    assert_eq!(
        (&typed["id"], &typed["byContent"], &typed["byHashbang"]),
        (&json!("zt-xml"), &json!(true), &json!(false))
    );
    let listed = registry.dispatch(&mut session, "editor.fileTypes.list", json!({})).unwrap();
    let xml = listed["types"].as_array().unwrap().iter().find(|ty| ty["id"] == "zt-xml").cloned().unwrap();
    assert_eq!(xml["contents"], json!(["<?zt-xml"]));
    assert!(
        listed["types"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|ty| ty["id"] != "zt-xml")
            .all(|ty| ty.get("contents").is_none())
    );
    registry
        .dispatch(&mut session, "editor.fileTypes.addPattern", json!({ "type": "zt-go", "pattern": "*.zt-profile" }))
        .unwrap();
    let typed = registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": profile })).unwrap();
    assert_eq!((&typed["id"], &typed["byContent"]), (&json!("zt-go"), &json!(false)));
}

const DEFAULT_LANGUAGE_ID: &str = crate::code_style::DEFAULT_LANGUAGE;

#[test]
fn a_hashbang_names_the_type_of_a_file_without_an_extension() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session) = setup(data.path());
    let script = data.path().join("deploy");
    std::fs::write(&script, "#!/usr/bin/env ztsh\necho\n").unwrap();
    let typed = registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": script })).unwrap();
    assert_eq!((&typed["id"], &typed["byHashbang"]), (&json!("zt-shell"), &json!(true)));
    registry
        .dispatch(&mut session, "editor.fileTypes.removeHashbang", json!({ "type": "zt-shell", "pattern": "ztsh" }))
        .unwrap();
    let typed = registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": script })).unwrap();
    assert_eq!(typed["id"], json!(null));
    let added = registry
        .dispatch(&mut session, "editor.fileTypes.addHashbang", json!({ "type": "zt-yaml", "pattern": "ztsh" }))
        .unwrap();
    assert_eq!(added["movedFrom"], json!(null), "zt-shell lost it already");
    let typed = registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": script })).unwrap();
    assert_eq!(typed["id"], json!("zt-yaml"));
}

#[test]
fn wrong_types_and_patterns_are_refused() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session) = setup(data.path());
    let unknown = registry
        .dispatch(&mut session, "editor.fileTypes.addPattern", json!({ "type": "nope", "pattern": "*.x" }))
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("nope"), "{unknown}");
    for pattern in ["", "  ", "src/*.go"] {
        let refused = registry.dispatch(
            &mut session,
            "editor.fileTypes.addPattern",
            json!({ "type": "zt-go", "pattern": pattern }),
        );
        assert!(refused.is_err(), "{pattern:?}");
    }
    assert!(registry.dispatch(&mut session, "editor.fileTypes.setIgnored", json!({ "patterns": ["a/b"] })).is_err());
}

#[test]
fn ignored_names_leave_the_index_find_in_files_and_go_to_file_at_once() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("ztcache/deep")).unwrap();
    std::fs::write(root.join("main.go"), "package main // needle\n").unwrap();
    std::fs::write(root.join("notes.zt-secret"), "needle\n").unwrap();
    std::fs::write(root.join("ztcache/deep/a.go"), "package deep // needle\n").unwrap();
    let (registry, mut session) = setup(data.path());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    wait_for_index(&registry, &mut session);
    let find = |registry: &Registry, session: &mut Session| {
        let found = registry.dispatch(session, "search.text.find", json!({ "query": { "text": "needle" } })).unwrap();
        let mut files: Vec<String> = found["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|found| found["path"].as_str().unwrap().to_owned())
            .collect();
        files.sort();
        files
    };
    assert_eq!(find(&registry, &mut session), ["main.go", "notes.zt-secret", "ztcache/deep/a.go"]);

    let changed = registry
        .dispatch(
            &mut session,
            "editor.fileTypes.setIgnored",
            json!({ "patterns": ignored_with(&["*.zt-secret", "ztcache"]) }),
        )
        .unwrap();
    assert_eq!(changed["ignoredDefault"], false);
    wait_for_index(&registry, &mut session);
    assert_eq!(find(&registry, &mut session), ["main.go"], "the project was indexed again");
    let files = registry.dispatch(&mut session, "navigate.file.goTo", json!({ "query": "a.go" })).unwrap();
    assert!(files["files"].as_array().unwrap().is_empty(), "{files}");
    let typed =
        registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": "ztcache/deep/a.go" })).unwrap();
    assert_eq!(typed["ignored"], true);
    let names: Vec<String> = zen_vfs::list(&root).unwrap().into_iter().map(|entry| entry.name).collect();
    assert_eq!(names, ["main.go"], "the tree's listing leaves them out too");

    registry.dispatch(&mut session, "editor.fileTypes.setIgnored", json!({ "patterns": DEFAULT_IGNORED })).unwrap();
    wait_for_index(&registry, &mut session);
    assert_eq!(find(&registry, &mut session).len(), 3);
    assert!(!data.path().join(FILE_NAME).exists());
}

#[test]
fn another_window_follows_a_change_by_indexing_again() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("a.zt-other"), "x\n").unwrap();
    let (registry, mut first) = setup(data.path());
    let (_, mut second) = setup(data.path());
    registry.dispatch(&mut second, "file.project.open", json!({ "path": project.path() })).unwrap();
    wait_for_index(&registry, &mut second);
    let before = second.state::<Workbench>().unwrap().index.clone().unwrap();
    registry
        .dispatch(&mut first, "editor.fileTypes.setIgnored", json!({ "patterns": ignored_with(&["*.zt-other"]) }))
        .unwrap();
    follow(&mut second);
    let after = second.state::<Workbench>().unwrap().index.clone().unwrap();
    assert!(!std::ptr::eq(before.exclusions(), after.exclusions()), "a new index");
    assert!(after.exclusions().ignored.contains(&"*.zt-other".to_owned()));
    follow(&mut second);
    let again = second.state::<Workbench>().unwrap().index.clone().unwrap();
    assert!(std::ptr::eq(after.exclusions(), again.exclusions()), "nothing changed since");
}

#[test]
fn a_damaged_file_keeps_what_it_can_and_unknown_types_take_nothing() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let file = data.path().join(FILE_NAME);
    std::fs::write(&file, "{ not json").unwrap();
    let _store = FileTypeStore::load(Some(file.clone()));
    assert!(file_types::current().changes().is_empty(), "a file that is not JSON changes nothing");

    let damaged = json!({
        "patterns": [
            { "type": "zt-go", "pattern": "*.zt-tmpl" },
            { "type": "zt-gone", "pattern": "*.zt-yml" },
            { "type": "zt-go" },
            42,
            { "type": "zt-go", "pattern": "a/b" }
        ],
        "removedPatterns": "nonsense",
        "ignored": [".git", 7, "", "zt-cache"]
    });
    std::fs::write(&file, damaged.to_string()).unwrap();
    let _store = FileTypeStore::load(Some(file));
    let registry = file_types::current();
    assert_eq!(registry.changes().added.len(), 2, "{:?}", registry.changes());
    assert_eq!(registry.type_of("page.zt-tmpl").map(|ty| ty.id).as_deref(), Some("zt-go"));
    assert_eq!(
        registry.type_of("page.zt-yml").map(|ty| ty.id).as_deref(),
        Some("zt-yaml"),
        "a type not declared takes nothing"
    );
    assert_eq!(registry.ignored(), [".git", "zt-cache"]);
}

fn opener(session: &mut Session) -> Arc<Mutex<Vec<PathBuf>>> {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&opened);
    session.set_state(SystemOpener::new(move |path| {
        seen.lock().unwrap().push(path.to_owned());
        Ok(())
    }));
    opened
}

fn open_paths(session: &Session) -> Vec<PathBuf> {
    session.state::<Workbench>().unwrap().documents().iter().map(|document| document.path().to_owned()).collect()
}

#[test]
fn a_file_of_no_type_that_is_not_text_asks_how_to_open_it() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let (registry, mut session) = setup(data.path());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let blob = root.join("dump.zt-bin");
    std::fs::write(&blob, [0x7f, b'Z', b'T', 0, 1, 2, b'x', 0xff]).unwrap();
    let notes = root.join("notes.zt-notes");
    std::fs::write(&notes, "just text\n").unwrap();

    let refused = registry.dispatch(&mut session, "file.document.open", json!({ "path": blob })).unwrap_err();
    let zen_core::DispatchError::Failed { error, .. } = refused else { panic!("{refused}") };
    assert_eq!(error.code.as_deref(), Some(UNKNOWN_FILE_TYPE));
    let facts = error.data.unwrap();
    assert_eq!(facts["patterns"], json!(["*.zt-bin", "dump.zt-bin"]));
    assert_eq!(facts["path"], json!(blob));
    assert!(error.message.contains("dump.zt-bin"), "{}", error.message);
    assert!(open_paths(&session).is_empty());

    registry.dispatch(&mut session, "file.document.open", json!({ "path": notes })).unwrap();
    assert_eq!(
        open_paths(&session),
        std::slice::from_ref(&notes),
        "a file of no type whose content is text opens as text"
    );

    let associated = registry
        .dispatch(
            &mut session,
            "editor.fileTypes.associate",
            json!({ "pattern": "*.zt-bin", "type": "detected", "path": blob }),
        )
        .unwrap();
    assert_eq!(associated, json!({ "movedFrom": null, "movedFromName": null, "opened": "editor" }));
    let workbench = session.state::<Workbench>().unwrap();
    let document = workbench.active_document().unwrap();
    assert_eq!(document.path(), blob);
    assert!(document.read_only().is_some(), "not text: shown read-only");
    let listed = registry.dispatch(&mut session, "editor.fileTypes.list", json!({})).unwrap();
    let detected = listed["types"].as_array().unwrap().iter().find(|ty| ty["id"] == "detected").unwrap().clone();
    assert_eq!(detected["name"], "File type auto-detected by file content");
    assert_eq!(detected["patterns"], json!(["*.zt-bin"]));
    let saved = std::fs::read_to_string(data.path().join(FILE_NAME)).unwrap();
    assert!(saved.contains("\"type\": \"detected\""), "kept per IDE: {saved}");
}

#[test]
fn associating_a_pattern_with_a_type_or_the_application_opens_matching_files_so() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let (registry, mut session) = setup(data.path());
    let opened = opener(&mut session);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let page = root.join("page.zt-page");
    std::fs::write(&page, "package main\n").unwrap();
    let report = root.join("report.zt-doc");
    std::fs::write(&report, [b'%', b'Z', 0, 0, 9]).unwrap();
    let other = root.join("other.zt-doc");
    std::fs::write(&other, [0, 1]).unwrap();

    let associated = registry
        .dispatch(
            &mut session,
            "editor.fileTypes.associate",
            json!({ "pattern": "*.zt-page", "type": "zt-go", "path": page }),
        )
        .unwrap();
    assert_eq!(associated["opened"], "editor");
    let typed = registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": page })).unwrap();
    assert_eq!(typed["id"], "zt-go");
    assert_eq!(crate::code_style::language_of(&page), "go");

    let associated = registry
        .dispatch(
            &mut session,
            "editor.fileTypes.associate",
            json!({ "pattern": "*.zt-doc", "type": "native", "path": "report.zt-doc" }),
        )
        .unwrap();
    assert_eq!(associated["opened"], "application");
    registry.dispatch(&mut session, "file.document.open", json!({ "path": other })).unwrap();
    assert_eq!(*opened.lock().unwrap(), [report.clone(), other.clone()], "opened by the system's application");
    assert_eq!(open_paths(&session), std::slice::from_ref(&page), "not in the editor");
    let messages = &session.state::<Workbench>().unwrap().messages;
    assert!(messages.iter().any(|message| message.contains("report.zt-doc")), "{messages:?}");

    for (pattern, ty) in [("", "zt-go"), ("a/*.x", "zt-go"), ("*.x", "zt-nope")] {
        let refused =
            registry.dispatch(&mut session, "editor.fileTypes.associate", json!({ "pattern": pattern, "type": ty }));
        assert!(refused.is_err(), "{pattern} {ty}");
    }
}

#[test]
fn an_override_changes_only_the_chosen_files_is_kept_per_ide_and_reverts() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let (registry, mut session) = setup(data.path());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    for name in ["a.go", "b.go", "c.go"] {
        std::fs::write(root.join(name), "package main\n").unwrap();
    }
    let (a, b, c) = (root.join("a.go"), root.join("b.go"), root.join("c.go"));
    let own = registry
        .dispatch(&mut session, "editor.fileTypes.override", json!({ "paths": ["a.go", b], "type": TEXT }))
        .unwrap();
    assert_eq!(
        own,
        json!([
            { "path": a, "type": "text", "name": "Text" },
            { "path": b, "type": "text", "name": "Text" }
        ])
    );
    let id = |registry: &Registry, session: &mut Session, path: &Path| {
        registry.dispatch(session, "editor.fileTypes.typeOf", json!({ "path": path })).unwrap()
    };
    let typed = id(&registry, &mut session, &a);
    assert_eq!((&typed["id"], &typed["overridden"]), (&json!("text"), &json!(true)));
    assert_eq!(id(&registry, &mut session, &c)["id"], "zt-go", "the same pattern's other files keep their type");
    assert_eq!(crate::code_style::language_of(&a), crate::code_style::PLAIN_TEXT);
    assert_eq!(crate::code_style::language_of(&c), "go");

    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(data.path().join(OVERRIDES_FILE_NAME)).unwrap()).unwrap();
    assert_eq!(saved, json!({ "files": [{ "path": a, "type": "text" }, { "path": b, "type": "text" }] }));
    file_types::set_overrides(BTreeMap::new());
    assert_eq!(crate::code_style::language_of(&a), "go");
    let _restarted = FileTypeStore::load(Some(data.path().join(FILE_NAME)));
    assert_eq!(crate::code_style::language_of(&a), crate::code_style::PLAIN_TEXT, "read back at the next start");

    let left = registry.dispatch(&mut session, "editor.fileTypes.revertOverride", json!({ "paths": [a] })).unwrap();
    assert_eq!(left, json!([{ "path": b, "type": "text", "name": "Text" }]));
    assert_eq!(id(&registry, &mut session, &a)["overridden"], false);
    assert_eq!(registry.dispatch(&mut session, "editor.fileTypes.overrides", json!({})).unwrap(), left);
    registry.dispatch(&mut session, "editor.fileTypes.revertOverride", json!({ "paths": [b] })).unwrap();
    assert!(!data.path().join(OVERRIDES_FILE_NAME).exists(), "no overrides, no file");

    for (paths, ty) in
        [(json!([a]), "detected"), (json!([a]), "native"), (json!([a]), "zt-nope"), (json!([root]), TEXT)]
    {
        let refused =
            registry.dispatch(&mut session, "editor.fileTypes.override", json!({ "paths": paths, "type": ty }));
        assert!(refused.is_err(), "{paths} {ty}");
    }
}

#[test]
fn a_file_given_the_text_type_has_no_language_server() {
    let _restore = exclusive();
    let mut fixture =
        crate::language::testing::fixture(&[("main.go", "package main\n"), ("util.go", "package main\n")]);
    let (main, util) = (fixture.path("main.go"), fixture.path("util.go"));
    let typed = fixture.dispatch("editor.fileTypes.typeOf", json!({ "path": main })).unwrap();
    assert_eq!(typed["languageServer"], "go");
    let diagnostic = json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 7 } },
        "severity": 1,
        "message": "zt problem"
    });
    let languages = fixture.session.state_mut::<crate::language::Languages>();
    languages.publish_for_test(&main, "package main\n", &[diagnostic]);
    assert_eq!(languages.problems(&main).len(), 1);
    fixture.dispatch("file.document.open", json!({ "path": main })).unwrap();
    fixture.dispatch("editor.fileTypes.override", json!({ "paths": [main], "type": TEXT })).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let languages = fixture.session.state::<crate::language::Languages>().unwrap();
    assert!(languages.problems(&main).is_empty(), "the server's problems go with it");
    let typed = fixture.dispatch("editor.fileTypes.typeOf", json!({ "path": main })).unwrap();
    assert_eq!(
        (&typed["id"], &typed["languageServer"], &typed["highlighted"]),
        (&json!("text"), &json!(null), &json!(false))
    );
    let languages = fixture.session.state::<crate::language::Languages>().unwrap();
    assert!(!languages.serves(&main) && languages.serves(&util));
    assert_eq!(fixture.dispatch("editor.fileTypes.typeOf", json!({ "path": util })).unwrap()["languageServer"], "go");
}

#[test]
fn a_file_overridden_away_from_its_language_is_closed_on_its_server_and_opened_again_when_reverted() {
    let _restore = exclusive();
    let mut fixture = crate::language::testing::fixture(&[("main.go", "package main\n")]);
    let main = fixture.path("main.go");
    fixture.open("main.go");
    let uri = zen_lsp::position::file_uri(&main);
    let wait = std::time::Duration::from_secs(10);
    assert_eq!(fixture.server.wait_for("textDocument/didOpen", 1, wait).len(), 1);
    fixture.dispatch("editor.fileTypes.override", json!({ "paths": [main], "type": TEXT })).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let closed = fixture.server.wait_for("textDocument/didClose", 1, wait);
    assert_eq!(closed.len(), 1, "the server is told the file is gone");
    assert_eq!(closed[0]["textDocument"]["uri"], json!(uri));
    assert_eq!(fixture.open_files(), std::slice::from_ref(&main), "the tab stays open");

    fixture.dispatch("editor.fileTypes.revertOverride", json!({ "paths": [main] })).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let opened = fixture.server.wait_for("textDocument/didOpen", 2, wait);
    assert_eq!(opened.len(), 2, "the server gets the file again");
    assert_eq!(opened[1]["textDocument"]["uri"], json!(uri));
    assert_eq!(opened[1]["textDocument"]["languageId"], "go");
}

#[test]
fn utf16_text_is_not_binary_and_opens_decoded_and_editable() {
    let encode = |text: &str, little: bool, bom: bool| -> Vec<u8> {
        let mut bytes = if bom { if little { vec![0xff, 0xfe] } else { vec![0xfe, 0xff] } } else { Vec::new() };
        for unit in text.encode_utf16() {
            bytes.extend(if little { unit.to_le_bytes() } else { unit.to_be_bytes() });
        }
        bytes
    };
    let latin = "package main\nfunc main() {}\n";
    assert_eq!(utf16_of(&encode(latin, true, true)), Some(Utf16::Little));
    assert_eq!(utf16_of(&encode(latin, false, true)), Some(Utf16::Big));
    assert_eq!(utf16_of(&encode(latin, true, false)), Some(Utf16::Little), "no byte order mark");
    assert_eq!(utf16_of(&encode(latin, false, false)), Some(Utf16::Big));
    assert_eq!(utf16_of(&encode("Привет, мир", true, true)), Some(Utf16::Little));
    for text in [encode(latin, true, true), encode(latin, false, false), encode("Привет, мир", false, true)] {
        assert!(!is_binary(&text), "{text:?}");
    }
    assert_eq!(Utf16::Big.decode(&encode("Привет, мир", false, true)), "Привет, мир");
    assert_eq!(Utf16::Little.decode(&encode(latin, true, false)), latin);
    for binary in [&[0x7f, b'Z', b'T', 0, 1, 2, b'x', 0xff][..], &[0, 0, 1, 2], &[b'P', b'K', 3, 4, 0, 0, 0, 0], &[0]] {
        assert!(is_binary(binary), "{binary:?}");
    }
    assert!(!is_binary(b"plain text"));

    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let (registry, mut session) = setup(data.path());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let cases = [
        (root.join("notes.zt-utf16"), encode("Привет, мир\n", true, true)),
        (root.join("plain.txt"), encode(latin, true, true)),
        (root.join("bare.zt-utf16"), encode(latin, true, false)),
        (root.join("big.go"), encode(latin, false, true)),
    ];
    for (path, bytes) in &cases {
        std::fs::write(path, bytes).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let workbench = session.state::<Workbench>().unwrap();
        let document = workbench.active_document().unwrap();
        assert_eq!(document.path(), path);
        let expected = if path.ends_with("notes.zt-utf16") { "Привет, мир\n" } else { latin };
        assert_eq!(document.text(), expected, "{}", path.display());
        assert!(document.charset().contains("UTF-16"), "{}", document.charset());
        assert_eq!(document.read_only(), None, "it can be edited");
    }
}

#[cfg(unix)]
#[test]
fn pipes_and_devices_are_refused_without_blocking() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let pipe = root.join("pipe.zt-bin");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status().unwrap();
    assert!(made.success());
    let (sender, receiver) = std::sync::mpsc::channel();
    let paths = [pipe.clone(), PathBuf::from("/dev/zero"), root.clone()];
    std::thread::spawn(move || {
        let (registry, mut session) = setup(data.path());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        let answers: Vec<String> = paths
            .iter()
            .map(|path| {
                let refused = registry.dispatch(&mut session, "file.document.open", json!({ "path": path }));
                refused.map_or_else(|error| error.to_string(), |_| "opened".to_owned())
            })
            .collect();
        let _ = sender.send(answers);
    });
    let answers = receiver.recv_timeout(std::time::Duration::from_secs(20)).expect("opening a pipe never blocks");
    for answer in answers {
        assert!(answer.contains("not a regular file"), "{answer}");
    }
}

#[test]
fn files_in_their_application_open_only_when_asked_to_open_them() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let (registry, mut session) = setup(data.path());
    let opened = opener(&mut session);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let report = root.join("report.zt-doc");
    std::fs::write(&report, "text\n").unwrap();
    registry
        .dispatch(&mut session, "editor.fileTypes.associate", json!({ "pattern": "*.zt-doc", "type": "native" }))
        .unwrap();
    assert!(opened.lock().unwrap().is_empty(), "associating without a file starts nothing");

    let refused = registry
        .dispatch(&mut session, "file.document.open", json!({ "path": report, "editorOnly": true }))
        .unwrap_err();
    assert!(refused.to_string().contains("report.zt-doc"), "{refused}");
    let back = registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": report, "offset": 0 }));
    assert!(back.is_err(), "going to a place does not start an application");
    assert!(opened.lock().unwrap().is_empty(), "reopening tabs or navigating starts nothing");
    assert!(open_paths(&session).is_empty());

    registry.dispatch(&mut session, "file.document.open", json!({ "path": "report.zt-doc" })).unwrap();
    assert_eq!(*opened.lock().unwrap(), std::slice::from_ref(&report), "the absolute path, as one argument");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let program = root.join("tool.zt-doc");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let linked = root.join("linked.zt-doc");
        std::os::unix::fs::symlink(&program, &linked).unwrap();
        for path in [&program, &linked] {
            let refused = registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap_err();
            assert!(refused.to_string().contains("executable"), "{refused}");
        }
        assert_eq!(opened.lock().unwrap().len(), 1, "a program, or a link to one, is never started");
    }
}

#[test]
fn an_override_follows_its_file_when_renamed_moved_or_deleted_in_the_ide() {
    let _restore = exclusive();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().canonicalize().unwrap();
    let (registry, mut session) = setup(data.path());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    std::fs::create_dir_all(root.join("pkg")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    for name in ["a.go", "pkg/b.go", "c.go"] {
        std::fs::write(root.join(name), "package main\n").unwrap();
    }
    registry
        .dispatch(
            &mut session,
            "editor.fileTypes.override",
            json!({ "paths": ["a.go", "pkg/b.go", "c.go"], "type": TEXT }),
        )
        .unwrap();
    let own = |registry: &Registry, session: &mut Session| -> Vec<String> {
        let listed = registry.dispatch(session, "editor.fileTypes.overrides", json!({})).unwrap();
        listed.as_array().unwrap().iter().map(|item| item["path"].as_str().unwrap().to_owned()).collect()
    };

    registry.dispatch(&mut session, "file.fs.rename", json!({ "path": "a.go", "name": "renamed.go" })).unwrap();
    registry.dispatch(&mut session, "file.fs.rename", json!({ "path": "pkg", "name": "moved" })).unwrap();
    let path = |name: &str| root.join(name).display().to_string();
    assert_eq!(own(&registry, &mut session), [path("c.go"), path("moved/b.go"), path("renamed.go")]);
    assert_eq!(crate::code_style::language_of(&root.join("renamed.go")), crate::code_style::PLAIN_TEXT);

    registry
        .dispatch(&mut session, "project.file.cut", json!({ "paths": [root.join("renamed.go")], "clipboard": false }))
        .unwrap();
    registry
        .dispatch(&mut session, "project.file.paste", json!({ "directory": root.join("lib"), "clipboard": false }))
        .unwrap();
    assert_eq!(own(&registry, &mut session), [path("c.go"), path("lib/renamed.go"), path("moved/b.go")]);
    let saved = std::fs::read_to_string(data.path().join(OVERRIDES_FILE_NAME)).unwrap();
    assert!(saved.contains("lib/renamed.go") && !saved.contains("\"pkg/"), "kept: {saved}");

    relocate_overrides(&mut session, &root.join("c.go"), None);
    assert_eq!(own(&registry, &mut session), [path("lib/renamed.go"), path("moved/b.go")], "a deleted file loses it");
}

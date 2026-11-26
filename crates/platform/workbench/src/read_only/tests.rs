use serde_json::{Value, json};

use super::*;

fn setup(text: &str) -> (tempfile::TempDir, PathBuf, Registry, Session) {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.go");
    std::fs::write(&file, text).unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Workbench::default());
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    (dir, file, registry, session)
}

fn text(session: &Session) -> String {
    session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
}

fn offers(session: &Session) -> Vec<Value> {
    session
        .state::<Notifications>()
        .map(|store| store.newest_first().map(|item| serde_json::to_value(item).unwrap()).collect())
        .unwrap_or_default()
}

#[test]
fn the_lock_turns_the_write_permission_off_and_on_and_blocks_edits() {
    let (_dir, file, registry, mut session) = setup("package main\n");
    let state = registry.dispatch(&mut session, "file.document.readOnlyState", json!({})).unwrap();
    assert_eq!(state, json!({ "path": file, "readOnly": false }));

    let locked = registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    assert_eq!(locked["readOnly"], true);
    assert!(locked["reason"].as_str().unwrap().contains("main.go"));
    assert!(!zen_vfs::is_writable(&file));
    let document = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(document.is_locked() && document.read_only().is_some());

    registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
    assert_eq!(text(&session), "package main\n", "a read-only file is not edited");
    assert!(registry.dispatch(&mut session, "file.document.saveAll", json!({})).is_ok(), "nothing to save");
    assert!(registry.dispatch(&mut session, "file.document.save", json!({})).is_err(), "never written");

    let unlocked = registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    assert_eq!(unlocked, json!({ "path": file, "readOnly": false }));
    assert!(zen_vfs::is_writable(&file));
    registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    assert_eq!(text(&session), "// x\npackage main\n");

    let explicit = json!({ "path": "main.go", "readOnly": true });
    session.state_mut::<Workbench>().project = Some(file.parent().unwrap().to_owned());
    registry.dispatch(&mut session, "file.document.toggleReadOnly", explicit.clone()).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "// x\npackage main\n", "saved before it is locked");
    assert!(!session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());
    let again = registry.dispatch(&mut session, "file.document.toggleReadOnly", explicit).unwrap();
    assert_eq!(again["readOnly"], true, "an explicit state is kept, not turned");
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({ "readOnly": false })).unwrap();
}

#[test]
fn a_file_locked_by_another_program_is_noticed_on_sync() {
    let (_dir, file, registry, mut session) = setup("package main\n");
    zen_vfs::set_writable(&file, false).unwrap();
    registry.dispatch(&mut session, "file.document.syncWithDisk", json!({ "paths": [file] })).unwrap();
    assert!(session.state::<Workbench>().unwrap().active_document().unwrap().is_locked());
    zen_vfs::set_writable(&file, true).unwrap();
    registry.dispatch(&mut session, "file.document.syncWithDisk", json!({ "paths": [file] })).unwrap();
    assert!(!session.state::<Workbench>().unwrap().active_document().unwrap().is_locked());
}

#[test]
fn a_read_only_file_opens_locked() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.go");
    std::fs::write(&file, "package main\n").unwrap();
    zen_vfs::set_writable(&file, false).unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Workbench::default());
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    let state = registry.dispatch(&mut session, "file.document.readOnlyState", json!({})).unwrap();
    assert_eq!(state["readOnly"], true);
    assert!(state["reason"].is_string());
    zen_vfs::set_writable(&file, true).unwrap();
}

#[test]
fn typing_into_a_read_only_file_offers_to_make_it_writable_once() {
    let (_dir, file, registry, mut session) = setup("package main\n");
    assert!(offer_writable(&mut session).is_empty(), "nothing was refused");
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    assert!(offer_writable(&mut session).is_empty(), "locking is not editing");

    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x" })).unwrap();
    assert_eq!(offer_writable(&mut session), std::slice::from_ref(&file));
    let posted = offers(&session);
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0]["type"], "warning");
    assert!(posted[0]["body"].as_str().unwrap().contains("main.go"));
    assert_eq!(posted[0]["actions"][0]["id"], MAKE_WRITABLE);
    assert_eq!(posted[0]["actions"][0]["command"], "file.document.toggleReadOnly");
    assert_eq!(posted[0]["actions"][0]["params"], json!({ "path": file, "readOnly": false }));

    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "y" })).unwrap();
    assert!(offer_writable(&mut session).is_empty(), "one offer while it is unread");
    assert_eq!(offers(&session).len(), 1);

    let id = posted[0]["id"].clone();
    registry.dispatch(&mut session, "ui.notifications.action", json!({ "id": id, "action": MAKE_WRITABLE })).unwrap();
    for (command, params) in session.state_mut::<Notifications>().take_queued() {
        registry.dispatch(&mut session, &command, params).unwrap();
    }
    assert!(zen_vfs::is_writable(&file));
    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x" })).unwrap();
    assert_eq!(text(&session), "xpackage main\n");
    assert!(offer_writable(&mut session).is_empty());

    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    registry.dispatch(&mut session, "editor.edit.deleteBackward", json!({})).unwrap();
    assert_eq!(
        offer_writable(&mut session),
        std::slice::from_ref(&file),
        "offered again once the last offer was taken"
    );
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    assert!(offers(&session).iter().all(|item| item["read"] == true), "making the file writable settles the offers");
}

#[test]
fn a_file_read_only_for_its_encoding_is_not_offered() {
    let (_dir, file, registry, mut session) = setup("package main\n");
    std::fs::write(&file, b"caf\xe9\n").unwrap();
    registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    let state = registry.dispatch(&mut session, "file.document.readOnlyState", json!({})).unwrap();
    assert_eq!(state["readOnly"], false, "the file itself is writable");
    assert!(state["reason"].is_string(), "the document is not editable");
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({ "readOnly": true })).unwrap();
    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x" })).unwrap();
    assert!(offer_writable(&mut session).is_empty(), "making it writable would not let it be edited");
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({ "readOnly": false })).unwrap();
}

#[test]
fn a_missing_file_or_no_document_is_an_error() {
    let (dir, _file, registry, mut session) = setup("package main\n");
    let missing = dir.path().join("gone.go");
    let error = registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({ "path": missing }));
    assert!(error.is_err());
    registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
    assert!(!registry.is_available(&session, "file.document.toggleReadOnly"));
    assert!(registry.dispatch(&mut session, "file.document.readOnlyState", json!({})).is_err());
}

#[cfg(unix)]
#[test]
fn a_failed_save_leaves_the_file_writable_and_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, file, registry, mut session) = setup("package main\n");
    registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let error = registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap_err().to_string();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(error.contains("main.go"), "{error}");
    assert!(zen_vfs::is_writable(&file), "not made read-only when its changes could not be saved");
    let document = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(document.is_dirty() && !document.is_locked());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n");
}

#[cfg(unix)]
#[test]
fn making_a_file_writable_again_gives_back_its_group_write_permission() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, file, registry, mut session) = setup("package main\n");
    let mode = || std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o664)).unwrap();
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    assert_eq!(mode(), 0o444);
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    assert_eq!(mode(), 0o664);
}

#[test]
fn a_file_made_writable_by_another_program_settles_the_offer() {
    let (_dir, file, registry, mut session) = setup("package main\n");
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({})).unwrap();
    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x" })).unwrap();
    assert_eq!(offer_writable(&mut session).len(), 1);
    zen_vfs::set_writable(&file, true).unwrap();
    registry.dispatch(&mut session, "file.document.syncWithDisk", json!({ "paths": [] })).unwrap();
    assert!(offers(&session).iter().all(|item| item["read"] == true), "{:?}", offers(&session));
    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x" })).unwrap();
    assert_eq!(text(&session), "xpackage main\n");
}

#[test]
fn no_edit_reaches_a_read_only_file() {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.go"), dir.path().join("b.go"));
    std::fs::write(&a, "package a\n\nfunc Total() {}\n").unwrap();
    std::fs::write(&b, "package b\n\nvar _ = Total\n").unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let index = session.state::<Workbench>().unwrap().index.clone().unwrap();
    while !index.is_ready() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    registry.dispatch(&mut session, "file.document.open", json!({ "path": a })).unwrap();
    registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    registry.dispatch(&mut session, "file.document.toggleReadOnly", json!({ "readOnly": true })).unwrap();
    zen_vfs::set_writable(&b, false).unwrap();
    let locked = "// x\npackage a\n\nfunc Total() {}\n";
    assert_eq!(std::fs::read_to_string(&a).unwrap(), locked, "saved before it was locked");

    let commands = [
        ("editor.edit.type", json!({ "text": "x" })),
        ("editor.edit.insert", json!({ "text": "x" })),
        ("editor.edit.paste", json!({ "text": "a\nb\n" })),
        ("editor.edit.tab", json!({})),
        ("editor.caret.add", json!({ "head": 3 })),
        ("editor.edit.type", json!({ "text": "y" })),
        ("editor.line.duplicate", json!({})),
        ("editor.line.delete", json!({})),
        ("editor.line.moveDown", json!({})),
        ("editor.line.indent", json!({})),
        ("editor.comment.line", json!({})),
        ("editor.edit.undo", json!({})),
        ("editor.edit.redo", json!({})),
        ("editor.text.replace", json!({ "path": a, "oldText": "Total", "newText": "Sum" })),
        ("editor.text.replace", json!({ "path": b, "oldText": "Total", "newText": "Sum", "save": true })),
        ("search.text.replaceAll", json!({ "query": { "text": "Total", "matchCase": true }, "replacement": "Sum" })),
        ("file.document.save", json!({})),
        ("file.document.saveAll", json!({})),
    ];
    for (command, params) in commands {
        let _ = registry.dispatch(&mut session, command, params.clone());
        let document = session.state::<Workbench>().unwrap().documents.iter().find(|document| document.path() == a);
        assert_eq!(document.unwrap().text(), locked, "{command} {params}");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), locked, "{command} {params}");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "package b\n\nvar _ = Total\n", "{command} {params}");
    }
    let refused = registry
        .dispatch(&mut session, "search.text.replaceAll", json!({ "query": { "text": "Total" }, "replacement": "Sum" }))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("a.go") || refused.contains("b.go"), "{refused}");

    registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
    for path in [&a, &b] {
        let uri = zen_lsp::position::file_uri(path);
        let rename = json!({ "changes": { uri: [{
            "range": { "start": { "line": 0, "character": 8 }, "end": { "line": 0, "character": 9 } },
            "newText": "z" }] } });
        let mut languages = crate::language::Languages::default();
        let files = crate::language::edits::parse(&rename, &mut languages).unwrap();
        let workbench = session.state_mut::<Workbench>();
        let error = crate::language::edits::apply(&files, &mut languages, workbench).unwrap_err();
        assert!(matches!(error, crate::language::edits::EditError::ReadOnly { .. }), "{error:?}");
    }
    assert_eq!(std::fs::read_to_string(&a).unwrap(), locked);
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "package b\n\nvar _ = Total\n");
    zen_vfs::set_writable(&a, true).unwrap();
    zen_vfs::set_writable(&b, true).unwrap();
}

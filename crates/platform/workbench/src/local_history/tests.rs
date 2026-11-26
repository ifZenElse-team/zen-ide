use serde_json::{Value, json};

use super::*;

struct Fixture {
    registry: Registry,
    session: Session,
    project: tempfile::TempDir,
    data: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let project = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("main.go"), "package main\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(LocalHistory::open(data.path().join("local-history"), Limits::default()));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        Self { registry, session, project, data }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.project.path().join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fails(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }

    fn open(&mut self, name: &str) {
        let path = self.path(name);
        self.run("file.document.open", json!({ "path": path }));
    }

    fn type_and_save(&mut self, text: &str) {
        let workbench = self.session.state_mut::<Workbench>();
        let document = workbench.active_document_mut().unwrap();
        let length = document.text().chars().count();
        document.replace_ranges(vec![(0..length, text.to_owned())]);
        self.run("file.document.save", json!({}));
    }

    fn versions(&mut self, name: &str) -> Vec<Value> {
        let path = self.path(name);
        let list = self.run("localHistory.versions.list", json!({ "path": path }));
        list["versions"].as_array().unwrap().clone()
    }

    fn titles(&mut self, name: &str) -> Vec<String> {
        self.versions(name).iter().map(|version| version["title"].as_str().unwrap().to_owned()).collect()
    }

    fn text(&mut self, name: &str, id: &Value) -> String {
        let path = self.path(name);
        let version = self.run("localHistory.version.get", json!({ "path": path, "id": id }));
        version["content"].as_str().unwrap().to_owned()
    }

    fn reopen_history(&mut self) {
        self.session.set_state(LocalHistory::default());
        self.session.set_state(LocalHistory::open(self.data.path().join("local-history"), Limits::default()));
    }
}

#[test]
fn every_save_keeps_a_version_that_survives_a_restart() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.type_and_save("package main\n\nfunc main() {}\n");
    fixture.type_and_save("package main\n\nfunc main() { run() }\n");
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.titles("main.go"), ["Save", "Save", "External change"], "the text before the first save too");
    fixture.reopen_history();
    let versions = fixture.versions("main.go");
    assert_eq!(versions.len(), 3);
    assert_eq!(fixture.text("main.go", &versions[2]["id"]), "package main\n");
    assert_eq!(fixture.text("main.go", &versions[1]["id"]), "package main\n\nfunc main() {}\n");
    assert_eq!(versions[0]["kind"], "save");
    assert!(versions[0]["time"].as_u64().unwrap() >= versions[1]["time"].as_u64().unwrap());
    let path = fixture.path("main.go");
    let cut = fixture.run("localHistory.version.get", json!({ "path": path, "id": versions[0]["id"], "maxBytes": 7 }));
    assert_eq!((cut["content"].as_str(), cut["truncated"].as_bool()), (Some("package"), Some(true)));
}

#[test]
fn every_autosave_keeps_one_version_and_an_autosave_with_nothing_to_save_none() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    let edit = |fixture: &mut Fixture, text: &str| {
        let workbench = fixture.session.state_mut::<Workbench>();
        workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, text.to_owned())]);
    };
    edit(&mut fixture, "// first\n");
    fixture.run("file.document.autosave", json!({}));
    assert_eq!(fixture.titles("main.go"), ["Save", "External change"]);
    fixture.run("file.document.autosave", json!({}));
    assert_eq!(fixture.titles("main.go").len(), 2, "an autosave with nothing modified adds no version");
    edit(&mut fixture, "// second\n");
    fixture.run("file.document.autosave", json!({}));
    assert_eq!(fixture.titles("main.go"), ["Save", "Save", "External change"]);
    let versions = fixture.versions("main.go");
    assert_eq!(fixture.text("main.go", &versions[0]["id"]), "// second\n// first\npackage main\n");
    assert_eq!(fixture.text("main.go", &versions[1]["id"]), "// first\npackage main\n");
}

#[test]
fn closing_a_tab_with_unsaved_changes_keeps_a_version_as_save_all_does() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    let workbench = fixture.session.state_mut::<Workbench>();
    workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "// closed\n".to_owned())]);
    fixture.run("file.document.close", json!({}));
    assert_eq!(fixture.titles("main.go"), ["Save", "External change"]);
    let versions = fixture.versions("main.go");
    assert_eq!(fixture.text("main.go", &versions[0]["id"]), "// closed\npackage main\n");
}

#[test]
fn diff_and_revert_restore_a_version_as_an_undoable_edit() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.type_and_save("package main\n\nfunc a() {}\n");
    fixture.type_and_save("package main\n\nfunc b() {}\n");
    let versions = fixture.versions("main.go");
    let first = versions[1]["id"].clone();
    let path = fixture.path("main.go");
    let diff = fixture.run("localHistory.version.diff", json!({ "path": path, "id": first }));
    assert_eq!(diff["diff"]["before"], "package main\n\nfunc a() {}\n");
    assert_eq!(diff["diff"]["after"], "package main\n\nfunc b() {}\n");
    assert_eq!(diff["diff"]["hunks"].as_array().unwrap().len(), 1);
    assert_eq!(diff["diff"]["afterTitle"], "Current version");

    let reverted = fixture.run("localHistory.version.revert", json!({ "path": path, "id": first }));
    assert_eq!(reverted["files"], json!([path]));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "package main\n\nfunc a() {}\n");
    let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.text(), "package main\n\nfunc a() {}\n");
    assert!(!document.is_dirty());
    fixture.run("editor.edit.undo", json!({}));
    let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.text(), "package main\n\nfunc b() {}\n", "Undo takes the revert back");
    assert_eq!(fixture.titles("main.go")[..2], ["Revert", "Before Revert"]);
}

#[test]
fn external_changes_and_deletions_are_recorded_and_deleted_files_keep_their_history() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    let path = fixture.path("main.go");
    std::fs::write(&path, "package main // changed by a tool\n").unwrap();
    let queued = fixture.run("localHistory.changes.record", json!({ "paths": [path] }));
    assert_eq!(queued["queued"], 1);
    fixture.run("file.document.syncWithDisk", json!({ "paths": [] }));
    assert_eq!(
        fixture.titles("main.go"),
        ["External change", "External change"],
        "the open file's text before, then the new one"
    );
    let versions = fixture.versions("main.go");
    assert_eq!(fixture.text("main.go", &versions[1]["id"]), "package main\n");

    std::fs::remove_file(&path).unwrap();
    fixture.run("localHistory.changes.record", json!({ "paths": [path] }));
    fixture.run("file.document.close", json!({}));
    let versions = fixture.versions("main.go");
    assert_eq!(versions[0]["kind"], "deleted");
    assert_eq!(versions[0]["hasContent"], false);
    let error = fixture.fails("localHistory.version.revert", json!({ "path": path, "id": versions[0]["id"] }));
    assert!(error.contains("did not exist"), "{error}");
    fixture.run("localHistory.version.revert", json!({ "path": path, "id": versions[1]["id"] }));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "package main // changed by a tool\n",
        "a deleted file comes back"
    );
}

#[test]
fn ignored_places_are_not_recorded() {
    let mut fixture = Fixture::new();
    zen_vfs::excluded_folders::declare(zen_vfs::excluded_folders::ExcludedFolder::new("target", "Cargo.toml"));
    std::fs::write(fixture.path("Cargo.toml"), "").unwrap();
    std::fs::write(fixture.path("package.json"), "{}").unwrap();
    std::fs::write(fixture.path(".gitignore"), "/bin\n").unwrap();
    for name in
        [".git/config", "target/out.txt", "node_modules/x/index.js", ".idea/workspace.xml", "app.iml", "bin/app"]
    {
        let path = fixture.path(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "x").unwrap();
        fixture.run("localHistory.changes.record", json!({ "paths": [path] }));
    }
    let outside = fixture.data.path().join("elsewhere.txt");
    std::fs::write(&outside, "x").unwrap();
    fixture.run("localHistory.changes.record", json!({ "paths": [outside] }));
    let root = fixture.project.path().to_owned();
    let list = fixture.run("localHistory.versions.list", json!({ "path": root }));
    assert_eq!(list["folder"], true);
    assert_eq!(list["versions"], json!([]));
    let error = fixture.fails("localHistory.versions.list", json!({ "path": outside }));
    assert!(error.contains("not in the project"), "{error}");
}

#[test]
fn folder_history_lists_changed_files_and_reverts_them() {
    let mut fixture = Fixture::new();
    std::fs::create_dir_all(fixture.path("pkg")).unwrap();
    for (name, text) in [("pkg/a.go", "a1"), ("pkg/b.go", "b1")] {
        std::fs::write(fixture.path(name), text).unwrap();
        let path = fixture.path(name);
        fixture.run("localHistory.changes.record", json!({ "paths": [path] }));
    }
    let folder = fixture.path("pkg");
    let label = fixture.run("localHistory.label.put", json!({ "label": "Working state" }));
    std::fs::write(fixture.path("pkg/a.go"), "a2").unwrap();
    std::fs::remove_file(fixture.path("pkg/b.go")).unwrap();
    std::fs::write(fixture.path("pkg/c.go"), "c1").unwrap();
    let paths = [fixture.path("pkg/a.go"), fixture.path("pkg/b.go"), fixture.path("pkg/c.go")];
    fixture.run("localHistory.changes.record", json!({ "paths": paths }));

    let list = fixture.run("localHistory.versions.list", json!({ "path": folder }));
    assert_eq!(list["folder"], true);
    let titles: Vec<&str> = list["versions"].as_array().unwrap().iter().map(|v| v["title"].as_str().unwrap()).collect();
    assert_eq!(
        titles,
        ["External change", "Deleted", "External change", "Working state", "External change", "External change"]
    );
    let diff = fixture.run("localHistory.version.diff", json!({ "path": folder, "id": label["id"] }));
    assert_eq!(diff["diff"], Value::Null);
    let changes: Vec<(String, String)> = diff["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| (file["relative"].as_str().unwrap().to_owned(), file["status"].as_str().unwrap().to_owned()))
        .collect();
    let expected = [("pkg/a.go", "modified"), ("pkg/b.go", "deleted"), ("pkg/c.go", "added")];
    assert_eq!(changes, expected.map(|(path, status)| (path.to_owned(), status.to_owned())));
    let a = fixture.path("pkg/a.go");
    let file_diff = fixture.run("localHistory.version.diff", json!({ "path": a, "id": label["id"] }));
    assert_eq!((file_diff["diff"]["before"].as_str(), file_diff["diff"]["after"].as_str()), (Some("a1"), Some("a2")));

    let reverted = fixture.run("localHistory.version.revert", json!({ "path": folder, "id": label["id"] }));
    assert_eq!(reverted["files"].as_array().unwrap().len(), 2);
    assert_eq!(reverted["kept"], json!([fixture.path("pkg/c.go")]));
    assert_eq!(std::fs::read_to_string(fixture.path("pkg/a.go")).unwrap(), "a1");
    assert_eq!(std::fs::read_to_string(fixture.path("pkg/b.go")).unwrap(), "b1");
    assert_eq!(std::fs::read_to_string(fixture.path("pkg/c.go")).unwrap(), "c1");
}

#[test]
fn labels_show_in_the_files_they_apply_to() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.type_and_save("package main\n// 1\n");
    let main = fixture.path("main.go");
    fixture.run("localHistory.label.put", json!({ "label": "  Whole project  " }));
    fixture.run("localHistory.label.put", json!({ "label": "Only main", "path": main }));
    let other = fixture.path("other.go");
    fixture.run("localHistory.label.put", json!({ "label": "Only other", "path": other }));
    assert_eq!(fixture.titles("main.go")[..2], ["Only main", "Whole project"]);
    let label = fixture.versions("main.go")[0]["id"].clone();
    assert_eq!(fixture.text("main.go", &label), "package main\n// 1\n", "a label shows the file as it was then");
    assert!(fixture.fails("localHistory.label.put", json!({ "label": " " })).contains("Enter a label"));
}

#[test]
fn replace_in_files_and_binary_files() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path("notes.txt"), "old name\n").unwrap();
    fixture.run("file.index.refresh", json!({ "paths": [fixture.path("notes.txt")] }));
    for _ in 0..400 {
        let replaced =
            fixture.run("search.text.replaceAll", json!({ "query": { "text": "old" }, "replacement": "new" }));
        if replaced["files"] == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(std::fs::read_to_string(fixture.path("notes.txt")).unwrap(), "new name\n");
    assert_eq!(fixture.titles("notes.txt"), ["Replace in Files", "Before Replace in Files"]);
    let versions = fixture.versions("notes.txt");
    assert_eq!(fixture.text("notes.txt", &versions[1]["id"]), "old name\n");

    let image = fixture.path("logo.png");
    std::fs::write(&image, [0x89, b'P', b'N', b'G', 0, 0, 1]).unwrap();
    fixture.run("localHistory.changes.record", json!({ "paths": [image] }));
    let versions = fixture.versions("logo.png");
    assert_eq!(versions[0]["binary"], true);
    let content = fixture.run("localHistory.version.get", json!({ "path": image, "id": versions[0]["id"] }));
    assert_eq!((content["content"].clone(), content["binary"].clone()), (Value::Null, json!(true)));
    let diff = fixture.run("localHistory.version.diff", json!({ "path": image, "id": versions[0]["id"] }));
    assert_eq!(diff["diff"]["binary"], true);
}

#[test]
fn without_a_store_nothing_is_recorded() {
    zen_i18n::register(crate::LOCALES);
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("a.txt"), "a").unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": project.path().join("a.txt") })).unwrap();
    registry.dispatch(&mut session, "file.document.save", json!({})).unwrap();
    let error = registry
        .dispatch(&mut session, "localHistory.versions.list", json!({ "path": project.path().join("a.txt") }))
        .unwrap_err();
    assert!(error.to_string().contains("not available"), "{error}");
}

#[test]
fn the_worker_prunes_on_request() {
    let data = tempfile::tempdir().unwrap();
    let history = LocalHistory::open(data.path().to_owned(), Limits { max_age: Duration::ZERO, ..Limits::default() });
    history.record(Record {
        root: PathBuf::from("/p"),
        relative: "a".to_owned(),
        kind: VersionKind::Save,
        label: None,
        content: Content::Bytes(b"a".to_vec()),
        time: 1,
        dedupe: true,
    });
    assert_eq!(history.prune(), Some(Pruned { entries: 1, contents: 1 }));
    assert_eq!(LocalHistory::default().prune(), None);
}

#[test]
fn git_rollback_keeps_the_text_it_discards() {
    let dir = crate::vcs::testing::test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(LocalHistory::open(data.path().to_owned(), Limits::default()));
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    std::fs::write(root.join("a.txt"), "my work\n").unwrap();
    registry.dispatch(&mut session, "vcs.changes.rollback", json!({ "paths": ["a.txt"] })).unwrap();
    let list =
        registry.dispatch(&mut session, "localHistory.versions.list", json!({ "path": root.join("a.txt") })).unwrap();
    let titles: Vec<&str> = list["versions"].as_array().unwrap().iter().map(|v| v["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Rollback", "Before Rollback"]);
    let before = list["versions"][1]["id"].clone();
    let kept = registry
        .dispatch(&mut session, "localHistory.version.get", json!({ "path": root.join("a.txt"), "id": before }))
        .unwrap();
    assert_eq!(kept["content"], "my work\n");
    let rolled_back = list["versions"][0]["id"].clone();
    let restored = registry
        .dispatch(&mut session, "localHistory.version.get", json!({ "path": root.join("a.txt"), "id": rolled_back }))
        .unwrap();
    assert_eq!(restored["content"], "one\ntwo\nthree\n");
}

#[test]
fn applying_a_patch_keeps_the_text_before_it_and_undoes_in_one_step_per_file() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = crate::vcs::testing::test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(LocalHistory::open(data.path().to_owned(), Limits::default()));
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("a.txt") })).unwrap();
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
    registry.dispatch(&mut session, "vcs.patch.apply", json!({ "text": patch })).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\n");
    let list =
        registry.dispatch(&mut session, "localHistory.versions.list", json!({ "path": root.join("a.txt") })).unwrap();
    let titles: Vec<&str> = list["versions"].as_array().unwrap().iter().map(|v| v["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Apply Patch", "Before Apply Patch"]);
    let before = list["versions"][1]["id"].clone();
    let kept = registry
        .dispatch(&mut session, "localHistory.version.get", json!({ "path": root.join("a.txt"), "id": before }))
        .unwrap();
    assert_eq!(kept["content"], "one\ntwo\nthree\n");
    let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
    assert_eq!(document.text().to_string(), "one\nTWO\nthree\n");
    assert!(document.undo());
    assert_eq!(document.text().to_string(), "one\ntwo\nthree\n", "one undo step takes the patch back");
}

#[test]
fn committing_records_the_unsaved_text_it_saves() {
    let dir = crate::vcs::testing::test_repository();
    let root = dir.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(LocalHistory::open(data.path().to_owned(), Limits::default()));
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("a.txt") })).unwrap();
    let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
    document.replace_ranges(vec![(0..3, "uno".to_owned())]);
    registry
        .dispatch(&mut session, "vcs.commit.create", json!({ "message": "Second", "paths": [root.join("a.txt")] }))
        .unwrap();
    let list =
        registry.dispatch(&mut session, "localHistory.versions.list", json!({ "path": root.join("a.txt") })).unwrap();
    let titles: Vec<&str> = list["versions"].as_array().unwrap().iter().map(|v| v["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Save", "External change"]);
}

#[test]
fn committing_refuses_a_file_changed_outside_instead_of_writing_over_it() {
    let dir = crate::vcs::testing::test_repository();
    let root = dir.path().canonicalize().unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("a.txt") })).unwrap();
    let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
    document.replace_ranges(vec![(0..3, "uno".to_owned())]);
    std::fs::write(root.join("a.txt"), "theirs\n").unwrap();
    let commit = json!({ "message": "Second", "paths": [root.join("a.txt")] });
    assert!(registry.dispatch(&mut session, "vcs.commit.create", commit).is_err());
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "theirs\n");
    assert!(session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());
}

#[test]
fn a_change_the_ide_saves_right_after_is_not_taken_for_an_external_one() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    let path = fixture.path("main.go");
    let history = fixture.session.state::<LocalHistory>().unwrap().clone();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    history.submit(Job::Query(Box::new(move |_| {
        let _ = gate.recv();
    })));
    std::fs::write(&path, "package main // by a tool\n").unwrap();
    fixture.run("localHistory.changes.record", json!({ "paths": [path] }));
    fixture.run("file.document.syncWithDisk", json!({ "paths": [path] }));
    fixture.type_and_save("package main // saved\n");
    release.send(()).unwrap();
    let titles = fixture.titles("main.go");
    assert_eq!(titles, ["Save", "External change", "External change"], "the first text, the tool's, and the save");
}

#[cfg(unix)]
#[test]
fn files_reached_through_links_out_of_the_project_are_not_recorded() {
    let mut fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "token").unwrap();
    let link = fixture.path("secret.txt");
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), &link).unwrap();
    std::os::unix::fs::symlink(outside.path(), fixture.path("elsewhere")).unwrap();
    let inner = fixture.path("elsewhere/secret.txt");
    let queued = fixture.run("localHistory.changes.record", json!({ "paths": [link, inner] }));
    assert_eq!(queued["queued"], 0);
    assert!(fixture.versions("secret.txt").is_empty());
}

#[test]
fn quitting_waits_a_bounded_time_for_the_queue() {
    let data = tempfile::tempdir().unwrap();
    let history = LocalHistory::open(data.path().to_owned(), Limits::default());
    let (release, gate) = std::sync::mpsc::channel::<()>();
    history.submit(Job::Query(Box::new(move |_| {
        let _ = gate.recv();
    })));
    assert!(!history.flush_within(Duration::from_millis(50)), "a stuck worker does not hold the quit");
    release.send(()).unwrap();
    assert!(history.flush_within(QUIT_WAIT));
    assert!(!LocalHistory::default().flush_within(QUIT_WAIT));
}

#[cfg(unix)]
#[test]
fn revert_does_not_write_through_a_link_out_of_the_project() {
    let mut fixture = Fixture::new();
    let folder = fixture.path("conf");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("app.toml"), "kept = 1\n").unwrap();
    fixture.run("localHistory.changes.record", json!({ "paths": [folder.join("app.toml")] }));
    let id = fixture.versions("conf/app.toml")[0]["id"].clone();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("app.toml"), "theirs\n").unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    std::os::unix::fs::symlink(outside.path(), &folder).unwrap();
    let error = fixture.fails("localHistory.version.revert", json!({ "path": folder.join("app.toml"), "id": id }));
    assert!(error.contains("not in the project"), "{error}");
    assert_eq!(std::fs::read_to_string(outside.path().join("app.toml")).unwrap(), "theirs\n");
}

#[test]
fn an_agent_sees_no_version_of_a_hidden_file_and_may_name_files_relative_to_the_project() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path(".env"), "TOKEN=abc123\n").unwrap();
    for name in ["main.go", ".env"] {
        fixture.open(name);
        fixture.type_and_save(&format!("{name} changed\n"));
    }
    let root = fixture.project.path().to_owned();
    let relative = fixture.run("localHistory.versions.list", json!({ "path": "main.go" }));
    assert!(!relative["versions"].as_array().unwrap().is_empty(), "relative to the project: {relative}");
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
    let Fixture { registry, session, .. } = &mut fixture;
    let (listed, diff, reverted) = zen_core::macros::as_agent(session, |session| {
        let listed = registry.dispatch(session, "localHistory.versions.list", json!({ "path": root })).unwrap();
        let first = listed["versions"].as_array().unwrap().last().unwrap()["id"].clone();
        let diff =
            registry.dispatch(session, "localHistory.version.diff", json!({ "path": root, "id": first })).unwrap();
        let reverted = registry.dispatch(session, "localHistory.version.revert", json!({ "path": root, "id": first }));
        (listed, diff, reverted)
    });
    assert!(!listed.to_string().contains(".env"), "{listed}");
    let Fixture { registry, session, .. } = &mut fixture;
    let sub = zen_core::macros::as_agent(session, |session| {
        registry.dispatch(session, "localHistory.versions.list", json!({ "path": "main.go" })).unwrap()
    });
    assert_eq!(listed.get("hiddenFiles"), sub.get("hiddenFiles"), "{listed} {sub}");
    assert!(listed["versions"].as_array().unwrap().iter().any(|version| version["relative"] == "main.go"));
    assert!(!diff.to_string().contains(".env"), "{diff}");
    assert!(reverted.is_err_and(|error| error.to_string().contains("hidden from agents")));
}

#[test]
fn consoles_are_recorded_under_their_own_root_and_their_versions_are_listed() {
    let mut fixture = Fixture::new();
    let data = fixture.data.path().join("ide");
    fixture.session.state_mut::<crate::workspaces::Workspaces>().set_data_dir(data.clone());
    let folder = data.join("consoles/project/local");
    std::fs::create_dir_all(&folder).unwrap();
    let console = folder.join("console.sql");
    std::fs::write(&console, "select 1;\n").unwrap();
    fixture.run("file.document.open", json!({ "path": console }));
    fixture.type_and_save("select 2;\n");
    fixture.type_and_save("select 3;\n");
    let list = fixture.run("localHistory.versions.list", json!({ "path": console }));
    let versions = list["versions"].as_array().unwrap().clone();
    assert!(versions.len() >= 2, "{list}");
    let text = fixture.run("localHistory.version.get", json!({ "path": console, "id": versions[0]["id"] }));
    assert_eq!(text["content"], "select 3;\n");

    let outside = fixture.data.path().join("elsewhere.sql");
    std::fs::write(&outside, "select 0;\n").unwrap();
    fixture.run("file.document.open", json!({ "path": outside }));
    fixture.type_and_save("select 9;\n");
    assert!(
        fixture
            .registry
            .dispatch(&mut fixture.session, "localHistory.versions.list", json!({ "path": outside }))
            .is_err()
    );
}

#[test]
fn a_store_shared_with_another_process_shows_its_versions_in_the_window() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.type_and_save("package main // one\n");
    let folder = fixture.data.path().join("local-history");
    let root = fixture.project.path().to_owned();
    let mut headless = Store::open(&folder, Limits::default());
    headless.record(&root, "main.go", VersionKind::Save, None, Some(b"package main // two\n"), now(), true).unwrap();
    assert_eq!(fixture.titles("main.go").len(), 3, "the other process's save is listed");
}

#[test]
fn a_busy_history_queue_does_not_lose_the_version_from_before_an_edit() {
    let fixture = Fixture::new();
    let history = fixture.session.state::<LocalHistory>().unwrap().clone();
    let (release, released) = std::sync::mpsc::channel::<()>();
    history.submit(Job::Query(Box::new(move |_| {
        let _ = released.recv();
    })));
    let before = read_before(&fixture.session, std::iter::once(fixture.path("main.go")));
    release.send(()).unwrap();
    assert!(matches!(before.first(), Some((_, Some(Content::Bytes(_))))), "read without waiting for the queue");
}

#[test]
fn the_last_handle_ends_the_worker_so_nothing_is_written_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("local-history");
    let history = LocalHistory::open(folder.clone(), Limits::default());
    let other = history.clone();
    drop(history);
    other.flush();
    drop(other);
    std::fs::remove_dir_all(&folder).unwrap();
    assert!(!folder.exists(), "the worker has ended and makes nothing again");
}

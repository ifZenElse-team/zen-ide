use std::path::Path;

use serde_json::{Value, json};

use super::*;

const MAIN: &str = "package main\n\nfunc main() {}\n";
const UTIL: &str = "package main\n\nvar answer = 42\n";

struct Fixture {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [("main.go", MAIN), ("util.go", UTIL)] {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        Self { registry, session, dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn autosave(&mut self) -> Value {
        self.run("file.document.autosave", json!({}))
    }

    fn edit(&mut self, name: &str, text: &str) {
        let path = self.path(name);
        self.run("file.document.open", json!({ "path": path }));
        self.run("editor.caret.goTo", json!({ "line": 1, "column": 1 }));
        self.run("editor.edit.insert", json!({ "text": text }));
    }

    fn disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
    }

    fn document(&self, name: &str) -> &Document {
        let path = self.path(name);
        let workbench = self.session.state::<Workbench>().unwrap();
        workbench.documents().iter().find(|document| document.path() == path).unwrap()
    }

    fn names(&self, value: &Value) -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|path| Path::new(path.as_str().unwrap()).file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    fn skipped(&self, value: &Value) -> Vec<(String, String)> {
        value["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .map(|skip| {
                let name = Path::new(skip["path"].as_str().unwrap()).file_name().unwrap().to_string_lossy();
                (name.into_owned(), skip["reason"].as_str().unwrap().to_owned())
            })
            .collect()
    }

    fn messages(&mut self) -> Vec<String> {
        self.session.state_mut::<Workbench>().take_messages()
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn autosave_saves_every_modified_file() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// one\n");
    fixture.edit("util.go", "// two\n");
    let done = fixture.autosave();
    assert_eq!(fixture.names(&done["saved"]), ["main.go", "util.go"]);
    assert_eq!(done["skipped"], json!([]));
    assert_eq!(done["failed"], json!([]));
    assert_eq!(fixture.disk("main.go"), format!("// one\n{MAIN}"));
    assert_eq!(fixture.disk("util.go"), format!("// two\n{UTIL}"));
    assert!(!fixture.document("main.go").is_dirty() && !fixture.document("util.go").is_dirty());
    assert_eq!(fixture.autosave(), json!({ "saved": [], "skipped": [], "failed": [] }), "nothing left to save");
}

#[test]
fn a_file_changed_on_disk_is_never_overwritten_and_becomes_a_conflict() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    fixture.edit("util.go", "// saved\n");
    std::fs::write(fixture.path("main.go"), "package main // theirs\n").unwrap();
    let done = fixture.autosave();
    assert_eq!(fixture.names(&done["saved"]), ["util.go"], "the other file is saved");
    assert_eq!(fixture.skipped(&done), [("main.go".to_owned(), "conflict".to_owned())]);
    assert_eq!(fixture.disk("main.go"), "package main // theirs\n", "the change on disk stays");
    assert!(fixture.document("main.go").has_conflict(), "the editor shows the conflict");
    assert!(fixture.document("main.go").is_dirty(), "the edit stays in the editor");
    assert!(fixture.messages().is_empty(), "a conflict is no failure");

    let again = fixture.autosave();
    assert_eq!(fixture.skipped(&again), [("main.go".to_owned(), "conflict".to_owned())]);

    let path = fixture.path("main.go");
    fixture.run("file.document.keepMine", json!({ "path": path }));
    let kept = fixture.autosave();
    assert_eq!(fixture.names(&kept["saved"]), ["main.go"], "Keep My Edits lets autosave write");
    assert_eq!(fixture.disk("main.go"), format!("// mine\n{MAIN}"));
}

#[test]
fn a_late_disk_check_after_keep_memory_changes_raises_no_new_conflict() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), "package main // theirs\n").unwrap();
    fixture.autosave();
    let path = fixture.path("main.go");
    fixture.run("file.document.keepMine", json!({ "path": path }));
    fixture.run("file.document.syncWithDisk", json!({}));
    assert!(!fixture.document("main.go").has_conflict(), "the change on disk was already settled");
    let done = fixture.autosave();
    assert_eq!(fixture.names(&done["saved"]), ["main.go"]);
    std::fs::write(fixture.path("main.go"), "package main // theirs again\n").unwrap();
    fixture.edit("main.go", "// more\n");
    fixture.run("file.document.syncWithDisk", json!({}));
    assert!(fixture.document("main.go").has_conflict(), "a newer change on disk is a conflict again");
}

#[test]
fn a_file_reloaded_from_disk_is_saved_again_after_new_edits() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), "package theirs\n").unwrap();
    fixture.autosave();
    let path = fixture.path("main.go");
    fixture.run("file.document.reloadFromDisk", json!({ "path": path }));
    fixture.edit("main.go", "// next\n");
    let done = fixture.autosave();
    assert_eq!(fixture.names(&done["saved"]), ["main.go"]);
    assert_eq!(fixture.disk("main.go"), "// next\npackage theirs\n");
}

#[test]
fn a_file_deleted_on_disk_is_not_written_back() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// gone\n");
    std::fs::remove_file(fixture.path("main.go")).unwrap();
    let done = fixture.autosave();
    assert_eq!(fixture.skipped(&done), [("main.go".to_owned(), "missing".to_owned())]);
    assert!(!fixture.path("main.go").exists(), "autosave creates no file");
    assert!(fixture.document("main.go").is_dirty());
}

#[cfg(unix)]
#[test]
fn a_read_only_file_is_skipped_without_a_message() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// locked\n");
    set_mode(&fixture.path("main.go"), 0o444);
    let done = fixture.autosave();
    set_mode(&fixture.path("main.go"), 0o644);
    assert_eq!(fixture.skipped(&done), [("main.go".to_owned(), "readOnly".to_owned())]);
    assert_eq!(fixture.disk("main.go"), MAIN);
    assert!(fixture.document("main.go").is_dirty());
    assert!(fixture.messages().is_empty());
}

#[test]
fn a_file_the_ide_opened_read_only_is_skipped() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// encoded\n");
    let path = fixture.path("main.go");
    let workbench = fixture.session.state_mut::<Workbench>();
    let document = workbench.documents.iter_mut().find(|document| document.path == path).unwrap();
    document.read_only = Some("windows-1251".to_owned());
    let done = fixture.autosave();
    assert_eq!(fixture.skipped(&done), [("main.go".to_owned(), "readOnly".to_owned())]);
    assert_eq!(fixture.disk("main.go"), MAIN);
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_written_is_reported_and_the_others_are_saved() {
    let mut fixture = Fixture::new();
    std::fs::create_dir(fixture.path("locked")).unwrap();
    std::fs::write(fixture.path("locked/inner.go"), MAIN).unwrap();
    fixture.edit("locked/inner.go", "// inner\n");
    fixture.edit("util.go", "// util\n");
    set_mode(&fixture.path("locked"), 0o555);
    let done = fixture.autosave();
    set_mode(&fixture.path("locked"), 0o755);
    assert_eq!(fixture.names(&done["saved"]), ["util.go"]);
    let failed = done["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert!(failed[0]["error"].as_str().unwrap().contains("inner.go"));
    let messages = fixture.messages();
    assert_eq!(messages.len(), 1, "one notification: {messages:?}");
    assert!(messages[0].contains("inner.go"));
    assert!(fixture.document("locked/inner.go").is_dirty(), "the edit stays");
}

#[cfg(unix)]
#[test]
fn a_failure_is_notified_once_and_the_file_is_saved_once_it_can_be() {
    let mut fixture = Fixture::new();
    std::fs::create_dir(fixture.path("locked")).unwrap();
    std::fs::write(fixture.path("locked/inner.go"), MAIN).unwrap();
    fixture.edit("locked/inner.go", "// inner\n");
    set_mode(&fixture.path("locked"), 0o555);
    let first = fixture.autosave();
    let second = fixture.autosave();
    set_mode(&fixture.path("locked"), 0o755);
    assert_eq!(first["failed"].as_array().unwrap().len(), 1);
    assert_eq!(second["failed"].as_array().unwrap().len(), 1, "the answer still names the failure");
    assert_eq!(fixture.messages().len(), 1, "the notification is not repeated");
    let done = fixture.autosave();
    assert_eq!(fixture.names(&done["saved"]), ["inner.go"], "the folder is writable again");
    assert_eq!(fixture.disk("locked/inner.go"), format!("// inner\n{MAIN}"));
}

#[cfg(unix)]
#[test]
fn autosave_writes_through_a_symbolic_link_and_keeps_it() {
    let mut fixture = Fixture::new();
    std::fs::create_dir(fixture.path("shared")).unwrap();
    std::fs::write(fixture.path("shared/real.go"), MAIN).unwrap();
    std::os::unix::fs::symlink("shared/real.go", fixture.path("link.go")).unwrap();
    fixture.edit("link.go", "// linked\n");
    let done = fixture.autosave();
    assert_eq!(fixture.names(&done["saved"]), ["link.go"]);
    assert!(std::fs::symlink_metadata(fixture.path("link.go")).unwrap().file_type().is_symlink());
    assert_eq!(fixture.disk("shared/real.go"), format!("// linked\n{MAIN}"));
}

#[test]
fn a_file_no_longer_utf8_on_disk_is_a_conflict_and_stays() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), [0xff, 0xfe, b'x']).unwrap();
    let done = fixture.autosave();
    assert_eq!(fixture.skipped(&done), [("main.go".to_owned(), "conflict".to_owned())]);
    assert_eq!(std::fs::read(fixture.path("main.go")).unwrap(), [0xff, 0xfe, b'x']);
}

#[test]
fn autosave_keeps_the_files_line_separators() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path("crlf.txt"), "one\r\ntwo\r\n").unwrap();
    fixture.edit("crlf.txt", "zero ");
    fixture.autosave();
    assert_eq!(std::fs::read(fixture.path("crlf.txt")).unwrap(), b"zero one\r\ntwo\r\n");
}

#[test]
fn autosave_without_a_project_does_nothing() {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    let done = registry.dispatch(&mut session, "file.document.autosave", json!({})).unwrap();
    assert_eq!(done, json!({ "saved": [], "skipped": [], "failed": [] }));
}

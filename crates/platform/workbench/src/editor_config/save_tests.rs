use serde_json::{Value, json};

use super::*;
use crate::Document;

struct Fixture {
    registry: Registry,
    session: Session,
    project: tempfile::TempDir,
    _history: Option<tempfile::TempDir>,
}

impl Fixture {
    fn new(files: &[(&str, &[u8])]) -> Self {
        Self::create(files, false)
    }

    fn with_history(files: &[(&str, &[u8])]) -> Self {
        Self::create(files, true)
    }

    fn create(files: &[(&str, &[u8])], history: bool) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let project = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            let path = project.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let history = history.then(|| tempfile::tempdir().unwrap());
        if let Some(data) = &history {
            let store = crate::local_history::LocalHistory::open(data.path().to_owned(), Default::default());
            session.set_state(store);
        }
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        Self { registry, session, project, _history: history }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.project.path().join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> zen_core::DispatchError {
        self.registry.dispatch(&mut self.session, id, params).expect_err(id)
    }

    fn open(&mut self, name: &str) {
        let path = self.path(name);
        self.run("file.document.open", json!({ "path": path }));
    }

    fn document(&self) -> &Document {
        self.session.state::<Workbench>().unwrap().active_document().unwrap()
    }

    fn type_at_end(&mut self, text: &str) {
        let end = self.document().text().chars().count();
        self.run("editor.selection.set", json!({ "selections": [{ "anchor": end, "head": end }] }));
        self.run("editor.edit.type", json!({ "text": text }));
    }

    fn save(&mut self) {
        self.run("file.document.save", json!({}));
    }

    fn disk(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.path(name)).unwrap()
    }

    fn versions(&mut self, name: &str) -> Vec<Value> {
        let path = self.path(name);
        let list = self.run("localHistory.versions.list", json!({ "path": path }));
        list["versions"].as_array().unwrap().clone()
    }

    fn replace_all(&mut self, from: &str, to: &str, expected_files: usize) -> Value {
        let paths: Vec<PathBuf> = std::fs::read_dir(self.project.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .collect();
        self.run("file.index.refresh", json!({ "paths": paths }));
        let mut replaced = Value::Null;
        for _ in 0..400 {
            replaced = self.run("search.text.replaceAll", json!({ "query": { "text": from }, "replacement": to }));
            if replaced["files"] == expected_files {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        replaced
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

#[test]
fn end_of_line_converts_a_modified_file_on_save_as_one_step() {
    let config = "[*]\nend_of_line = crlf\ntrim_trailing_whitespace = true\n";
    let mut fixture = Fixture::new(&[(".editorconfig", config.as_bytes()), ("a.txt", b"one \ntwo")]);
    fixture.open("a.txt");
    fixture.type_at_end("!");
    fixture.save();
    assert_eq!(text(fixture.disk("a.txt")), "one\r\ntwo!", "On Save trims every line first (ADR-0106)");
    assert_eq!(fixture.document().line_separator(), zen_text::LineSeparator::Crlf);
    fixture.run("editor.edit.undo", json!({}));
    assert_eq!(fixture.document().text(), "one\ntwo!", "the conversion is one undo step");
}

#[test]
fn new_files_take_end_of_line_and_unchanged_files_are_not_rewritten() {
    let config = b"[*]\nend_of_line = crlf\ntrim_trailing_whitespace = true\n";
    let mut fixture = Fixture::new(&[(".editorconfig", config), ("old.txt", b"a \nb\n")]);
    let created = fixture.run("file.fs.newFile", json!({ "directory": fixture.path(""), "name": "new.txt" }));
    assert_eq!(created, json!(fixture.path("new.txt")));
    assert_eq!(fixture.document().line_separator(), zen_text::LineSeparator::Crlf);
    fixture.run("editor.edit.type", json!({ "text": "x\ny" }));
    fixture.save();
    assert_eq!(text(fixture.disk("new.txt")), "x\r\ny");
    fixture.open("old.txt");
    fixture.save();
    assert_eq!(text(fixture.disk("old.txt")), "a \nb\n", "a save without changes leaves the file as it is");
}

#[test]
fn read_only_documents_and_files_are_left_alone() {
    let mut fixture = Fixture::new(&[(".editorconfig", b"[*]\nend_of_line = crlf\n"), ("ro.txt", b"a  \nb")]);
    fixture.open("ro.txt");
    fixture.type_at_end(" ");
    let path = fixture.path("ro.txt");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();
    assert!(!before_write(&mut fixture.session, &path));
    assert_eq!(fixture.document().text(), "a  \nb ", "nothing converted in a file that cannot be written");
    #[allow(clippy::permissions_set_readonly_false, reason = "the temporary folder must be removable")]
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
}

#[test]
fn charsets_open_and_save_the_file_in_their_encoding() {
    let config = "[*.latin]\ncharset = latin1\n[*.u16]\ncharset = utf-16le\n[*.bom]\ncharset = utf-8-bom\n";
    let mut fixture = Fixture::new(&[
        (".editorconfig", config.as_bytes()),
        ("a.latin", b"caf\xe9\n"),
        ("b.u16", b"\xff\xfea\x00\n\x00"),
        ("c.bom", b"plain\n"),
    ]);
    fixture.open("a.latin");
    assert_eq!(fixture.document().text(), "caf\u{e9}\n");
    assert_eq!(fixture.document().charset(), "ISO-8859-1");
    assert_eq!(fixture.document().read_only(), None);
    fixture.type_at_end("\u{fc}");
    fixture.save();
    assert_eq!(fixture.disk("a.latin"), b"caf\xe9\n\xfc");
    fixture.open("b.u16");
    assert_eq!((fixture.document().text(), fixture.document().charset()), ("a\n", "UTF-16LE"));
    fixture.type_at_end("\u{44f}");
    fixture.save();
    assert_eq!(fixture.disk("b.u16"), b"\xff\xfea\x00\n\x00\x4f\x04", "the byte order mark is kept");
    fixture.open("c.bom");
    fixture.type_at_end("x");
    fixture.save();
    assert_eq!(fixture.disk("c.bom"), b"\xef\xbb\xbfplain\nx");
    std::fs::write(fixture.path("b.u16"), b"\xff\xfeb\x00").unwrap();
    fixture.open("b.u16");
    fixture.run("file.document.syncWithDisk", json!({}));
    assert_eq!(fixture.document().text(), "b", "a change on disk is read in the charset");
}

#[test]
fn a_character_the_charset_cannot_hold_refuses_the_save() {
    let mut fixture = Fixture::new(&[(".editorconfig", b"[*]\ncharset = latin1\n"), ("a.txt", b"caf\xe9\n")]);
    fixture.open("a.txt");
    fixture.type_at_end("\u{44f}");
    let error = fixture.fail("file.document.save", json!({})).to_string();
    assert!(error.contains("ISO-8859-1") && error.contains("U+044F") && error.contains("line 2, column 1"), "{error}");
    assert_eq!(fixture.disk("a.txt"), b"caf\xe9\n", "nothing written");
    assert!(fixture.document().is_dirty());
}

#[test]
fn bytes_that_are_not_the_charset_open_read_only() {
    let mut fixture = Fixture::new(&[(".editorconfig", b"[*]\ncharset = utf-16be\n"), ("odd.txt", b"abc")]);
    fixture.open("odd.txt");
    assert!(fixture.document().read_only().is_some_and(|reason| reason.contains("UTF-16BE")));
}

#[test]
fn the_answer_of_project_editorconfig_get_has_the_save_properties() {
    let config = "[*]\ntrim_trailing_whitespace = true\ninsert_final_newline = false\n";
    let mut fixture = Fixture::new(&[(".editorconfig", config.as_bytes()), ("a.txt", b"")]);
    let answer = fixture.run("project.editorConfig.get", json!({ "path": "a.txt" }));
    assert_eq!(
        answer["save"],
        json!({ "trimTrailingWhitespace": true, "insertFinalNewline": false, "endOfLine": null, "charset": null })
    );
}

#[test]
fn open_list_create_and_export_editorconfig_files() {
    let mut fixture = Fixture::new(&[
        (".editorconfig", b"root = true\n[*]\nindent_size = 2\n"),
        ("sub/.editorconfig", b"[*.go]\nindent_style = tab\nbad line\n"),
        ("sub/main.go", b"package main\n"),
        ("empty/.keep", b""),
    ]);
    fixture.open("sub/main.go");
    let opened = fixture.run("project.editorConfig.open", json!({}));
    assert_eq!(opened, json!(fixture.path("sub/.editorconfig")));
    assert_eq!(fixture.document().path(), fixture.path("sub/.editorconfig"));
    let listed = fixture.run("project.editorConfig.list", json!({}));
    assert_eq!(listed[0]["relative"], ".editorconfig");
    assert_eq!(listed[0]["root"], true);
    assert_eq!(listed[1]["relative"], "sub/.editorconfig");
    assert_eq!(listed[1]["problems"], 1);

    let created = fixture.run("project.editorConfig.create", json!({ "folder": "empty" }));
    assert_eq!(created, json!(fixture.path("empty/.editorconfig")));
    let stub = text(fixture.disk("empty/.editorconfig"));
    assert!(stub.starts_with("root = true\n\n[*]\ncharset = utf-8\nend_of_line = lf\nindent_size = 2\n"), "{stub}");
    assert!(stub.contains("max_line_length = 120\ntab_width = 2\ntrim_trailing_whitespace = false\n"), "{stub}");
    assert!(!stub.contains("ij_"));
    assert_eq!(fixture.document().path(), fixture.path("empty/.editorconfig"), "opened");
    let error = fixture.fail("project.editorConfig.create", json!({ "folder": "empty" }));
    assert!(matches!(&error, zen_core::DispatchError::Failed { error, .. } if error.code.as_deref() == Some(EXISTS)));
    std::fs::remove_file(fixture.path("empty/.editorconfig")).unwrap();
    let stubs = json!({ "root": false, "standard": false, "intellij": true });
    fixture.run("project.editorConfig.create", json!({ "folder": "empty", "stubs": stubs }));
    assert_eq!(
        text(fixture.disk("empty/.editorconfig")),
        "[*]\nij_continuation_indent_size = 4\nij_visual_guides = none\n"
    );

    let refused = fixture.fail("project.editorConfig.export", json!({}));
    assert!(matches!(&refused, zen_core::DispatchError::Failed { error, .. } if error.code.as_deref() == Some(EXISTS)));
    let exported = fixture.run("project.editorConfig.export", json!({ "overwrite": true }));
    let scheme = text(fixture.disk(".editorconfig"));
    assert_eq!(exported["text"], json!(scheme));
    assert!(
        scheme.starts_with("root = true\n\n[*]\nindent_size = 4\nindent_style = space\nmax_line_length = 120\n"),
        "{scheme}"
    );
    assert!(scheme.contains("\n[*.go]\nindent_size = 4\nindent_style = tab\ntab_width = 4\n"), "{scheme}");
    assert!(scheme.contains("\n[{*.json,*.jsonc,*.json5}]\nindent_size = 2\n"), "{scheme}");
    fixture.session.settings_mut().set(crate::line_separators::NEW_FILES, "crlf").unwrap();
    let crlf = fixture.run("project.editorConfig.export", json!({ "path": "other/.editorconfig" }));
    assert!(crlf["text"].as_str().unwrap().contains("end_of_line = crlf\r\n"), "{crlf}");
}

#[test]
fn editorconfig_files_get_completion_without_any_plugin() {
    let mut fixture = Fixture::new(&[(".editorconfig", b"[*]\nind")]);
    fixture.open(".editorconfig");
    fixture.run("editor.selection.set", json!({ "selections": [{ "anchor": 7, "head": 7 }] }));
    let list = fixture.run("editor.completion.start", json!({}));
    let labels: Vec<&str> =
        list["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
    assert!(labels.starts_with(&["indent_size", "indent_style"]), "{labels:?}");
}

#[test]
fn every_charset_saves_a_file_byte_for_byte_as_it_was_read() {
    let config = "[*.l1]\ncharset = latin1\n[*.u8]\ncharset = utf-8\n[*.bom]\ncharset = utf-8-bom\n\
                  [*.be]\ncharset = utf-16be\n[*.le]\ncharset = utf-16le\n";
    let files: [(&str, &[u8], &[u8]); 7] = [
        ("a.l1", b"caf\xe9\r\n\xff\x00z", b"x"),
        ("a.u8", b"\xef\xbb\xbfcaf\xc3\xa9\r\n", b"x"),
        ("a.bom", b"\xef\xbb\xbf\xef\xbb\xbfx\r\n", b"x"),
        ("a.be", b"\xfe\xff\xfe\xff\x00a\x00\r\x00\n\xd8\x3d\xde\x00", b"\x00x"),
        ("b.be", b"\x00a\x00\r\x00\n\x04\x4f", b"\x00x"),
        ("a.le", b"\xff\xfea\x00\r\x00\n\x00\x3d\xd8\x00\xde", b"x\x00"),
        ("b.le", b"a\x00\r\x00\n\x00", b"x\x00"),
    ];
    let mut all = vec![(".editorconfig", config.as_bytes())];
    all.extend(files.iter().map(|(name, bytes, _)| (*name, *bytes)));
    let mut fixture = Fixture::new(&all);
    for (name, bytes, typed) in files {
        fixture.open(name);
        assert_eq!(fixture.document().read_only(), None, "{name}");
        fixture.type_at_end("x");
        fixture.save();
        assert_eq!(fixture.disk(name), [bytes, typed].concat(), "{name}: the bytes read are written back unchanged");
    }
    fixture.open("a.be");
    assert_eq!(fixture.document().text(), "\u{feff}a\r\n\u{1f600}x", "one mark is the file's, the second is text");
    assert_eq!(fixture.document().line_separator(), zen_text::LineSeparator::Crlf);
    fixture.type_at_end("\r\n\u{1f601}");
    fixture.save();
    assert_eq!(
        fixture.disk("a.be"),
        b"\xfe\xff\xfe\xff\x00a\x00\r\x00\n\xd8\x3d\xde\x00\x00x\x00\r\x00\n\xd8\x3d\xde\x01",
        "a surrogate pair and CRLF in UTF-16"
    );
}

#[test]
fn a_file_no_charset_covers_keeps_its_encoding() {
    let config = b"[*.l1]\ncharset = latin1\n";
    let mut fixture = Fixture::new(&[
        (".editorconfig", config),
        ("bom.txt", b"\xef\xbb\xbfcaf\xc3\xa9\n"),
        ("legacy.txt", b"caf\xe9\n"),
        ("wide.txt", b"\xff\xfea\x00"),
    ]);
    fixture.open("bom.txt");
    assert_eq!(fixture.document().charset(), "UTF-8 BOM");
    fixture.type_at_end("x");
    fixture.save();
    assert_eq!(fixture.disk("bom.txt"), b"\xef\xbb\xbfcaf\xc3\xa9\nx", "the mark and the UTF-8 bytes stay");
    fixture.open("legacy.txt");
    assert!(fixture.document().read_only().is_some(), "not UTF-8 and no charset: read-only, as before");
    fixture.fail("file.document.save", json!({}));
    assert_eq!(fixture.disk("legacy.txt"), b"caf\xe9\n");
    let path = fixture.path("wide.txt");
    let opened = fixture.registry.dispatch(&mut fixture.session, "file.document.open", json!({ "path": path }));
    assert!(opened.is_ok());
    assert_eq!(fixture.document().charset(), "UTF-16LE", "its byte order mark says so");
    let _ = fixture.registry.dispatch(&mut fixture.session, "file.document.saveAll", json!({}));
    assert_eq!(fixture.disk("wide.txt"), b"\xff\xfea\x00");
}

#[test]
fn bytes_that_are_not_the_charset_are_never_repaired() {
    let config = b"[*.be]\ncharset = utf-16be\n[*.u8]\ncharset = utf-8-bom\n";
    let mut fixture = Fixture::new(&[
        (".editorconfig", config),
        ("lone.be", b"\x00a\xd8\x00\x00b"),
        ("bad.u8", b"\xef\xbb\xbfa\xff"),
    ]);
    for name in ["lone.be", "bad.u8"] {
        let before = fixture.disk(name);
        fixture.open(name);
        assert!(fixture.document().read_only().is_some(), "{name}");
        let _ = fixture.registry.dispatch(&mut fixture.session, "editor.edit.type", json!({ "text": "x" }));
        let _ = fixture.registry.dispatch(&mut fixture.session, "file.document.save", json!({}));
        let _ = fixture.registry.dispatch(&mut fixture.session, "file.document.saveAll", json!({}));
        assert_eq!(fixture.disk(name), before, "{name}");
    }
}

#[test]
fn save_all_refuses_an_unmappable_character_and_leaves_the_file_as_it_was() {
    let mut fixture =
        Fixture::new(&[(".editorconfig", b"[*]\ncharset = latin1\nend_of_line = crlf\n"), ("a.txt", b"caf\xe9\n")]);
    fixture.open("a.txt");
    fixture.type_at_end("\u{20ac}");
    let error = fixture.fail("file.document.saveAll", json!({})).to_string();
    assert!(error.contains("U+20AC") && error.contains("ISO-8859-1"), "{error}");
    assert_eq!(fixture.disk("a.txt"), b"caf\xe9\n", "byte-identical");
    assert!(fixture.document().is_dirty(), "the change is kept in the editor");
    fixture.run("editor.edit.deleteBackward", json!({}));
    fixture.save();
    assert_eq!(fixture.disk("a.txt"), b"caf\xe9\r\n");
}

#[test]
fn replace_in_files_and_line_separators_leave_closed_files_of_other_charsets_alone() {
    let config = b"[*.l1]\ncharset = latin1\n[*.le]\ncharset = utf-16le\n";
    let mut fixture = Fixture::new(&[
        (".editorconfig", config),
        ("a.txt", b"cart\n"),
        ("b.l1", b"cart\n"),
        ("c.le", b"c\x00a\x00r\x00t\x00\n\x00"),
    ]);
    let replaced = fixture.replace_all("cart", "caf\u{e9}", 1);
    assert_eq!(replaced["files"], 1, "{replaced}");
    assert_eq!(fixture.disk("a.txt"), "caf\u{e9}\n".as_bytes());
    assert_eq!(fixture.disk("b.l1"), b"cart\n", "never written as UTF-8");
    assert_eq!(fixture.disk("c.le"), b"c\x00a\x00r\x00t\x00\n\x00");
    let changed = fixture.run("file.lineSeparator.set", json!({ "separator": "crlf", "paths": [fixture.path("")] }));
    assert_eq!(changed["changed"], json!([fixture.path(".editorconfig"), fixture.path("a.txt")]), "{changed}");
    assert_eq!(fixture.disk("b.l1"), b"cart\n");
    assert_eq!(fixture.disk("c.le"), b"c\x00a\x00r\x00t\x00\n\x00");

    fixture.open("b.l1");
    fixture.open("c.le");
    let replaced = fixture.replace_all("cart", "caf\u{e9}", 2);
    assert_eq!(replaced["files"], 2, "open files change in the editor: {replaced}");
    fixture.run("file.document.saveAll", json!({}));
    assert_eq!(fixture.disk("b.l1"), b"caf\xe9\n");
    assert_eq!(fixture.disk("c.le"), b"c\x00a\x00f\x00\xe9\x00\n\x00");
    let changed = fixture.run("file.lineSeparator.set", json!({ "separator": "lf", "paths": [fixture.path("c.le")] }));
    assert_eq!(changed["changed"], json!([]), "already LF");
    let changed =
        fixture.run("file.lineSeparator.set", json!({ "separator": "crlf", "paths": [fixture.path("c.le")] }));
    assert_eq!(changed["changed"], json!([fixture.path("c.le")]));
    assert_eq!(fixture.disk("c.le"), b"c\x00a\x00f\x00\xe9\x00\r\x00\n\x00", "an open file is written in its charset");
}

#[test]
fn local_history_keeps_and_reverts_files_in_their_charset() {
    let config = b"[*.l1]\ncharset = latin1\n[*.be]\ncharset = utf-16be\n";
    let mut fixture =
        Fixture::with_history(&[(".editorconfig", config), ("a.l1", b"caf\xe9\n"), ("a.be", b"\xfe\xff\x00a\x00\n")]);
    for (name, typed, saved) in [
        ("a.l1", "\u{fc}", b"caf\xe9\n\xfc".as_slice()),
        ("a.be", "\u{1f600}", b"\xfe\xff\x00a\x00\n\xd8\x3d\xde\x00".as_slice()),
    ] {
        let original = fixture.disk(name);
        fixture.open(name);
        fixture.type_at_end(typed);
        fixture.save();
        assert_eq!(fixture.disk(name), saved);
        let versions = fixture.versions(name);
        let first = versions.last().unwrap()["id"].clone();
        let path = fixture.path(name);
        fixture.run("localHistory.version.revert", json!({ "path": path, "id": first }));
        assert_eq!(fixture.disk(name), original, "{name}: the bytes of then, in the file's charset");
        let expected = if name == "a.l1" { "caf\u{e9}\n" } else { "a\n" };
        assert_eq!(fixture.document().text(), expected, "{name}: the open file reads them in its charset");
    }
}

#[test]
fn end_of_line_on_save_is_in_local_history() {
    let mut fixture = Fixture::with_history(&[(".editorconfig", b"[*]\nend_of_line = crlf\n"), ("a.txt", b"one\ntwo")]);
    fixture.open("a.txt");
    fixture.type_at_end("!");
    fixture.save();
    assert_eq!(fixture.disk("a.txt"), b"one\r\ntwo!");
    let versions = fixture.versions("a.txt");
    let path = fixture.path("a.txt");
    let content = |fixture: &mut Fixture, version: &Value| {
        fixture.run("localHistory.version.get", json!({ "path": path, "id": version["id"] }))["content"].clone()
    };
    assert_eq!(versions[0]["kind"], "save");
    assert_eq!(content(&mut fixture, &versions[0]), json!("one\r\ntwo!"), "the save as written");
    assert_eq!(content(&mut fixture, &versions[1]), json!("one\ntwo"), "the text before it");
}

#[test]
fn export_over_an_editorconfig_keeps_the_old_one_in_local_history() {
    let mut fixture = Fixture::with_history(&[(".editorconfig", b"[*]\nindent_size = 3\n")]);
    fixture.run("project.editorConfig.export", json!({ "overwrite": true }));
    let versions = fixture.versions(".editorconfig");
    let titles: Vec<&str> = versions.iter().map(|version| version["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Export to EditorConfig File", "Before Export to EditorConfig File"]);
    let path = fixture.path(".editorconfig");
    let before = fixture.run("localHistory.version.get", json!({ "path": path, "id": versions[1]["id"] }));
    assert_eq!(before["content"], "[*]\nindent_size = 3\n");
}

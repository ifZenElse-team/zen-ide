use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use zen_project::settings::{ProjectSettings, ReadFiles, SettingsProvider};

use super::*;
use crate::local_history::{Limits, LocalHistory};

#[derive(Default)]
struct Kept {
    encodings: Mutex<Encodings>,
    writes: Mutex<usize>,
}

impl SettingsProvider for Kept {
    fn id(&self) -> &str {
        "kept"
    }

    fn watches(&self, _: &Path) -> bool {
        false
    }

    fn load(&self, _: &Path, _: &dyn ReadFiles) -> Option<ProjectSettings> {
        let encodings = self.encodings.lock().unwrap().clone();
        Some(ProjectSettings { encodings, ..Default::default() })
    }

    fn set_encodings(&self, _: &Path, encodings: &Encodings) -> Option<Result<(), String>> {
        *self.encodings.lock().unwrap() = encodings.clone();
        *self.writes.lock().unwrap() += 1;
        Some(Ok(()))
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: PathBuf,
    kept: Arc<Kept>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

impl Fixture {
    fn new(files: &[(&str, &[u8])]) -> Self {
        Self::with(Encodings::default(), files)
    }

    fn with(encodings: Encodings, files: &[(&str, &[u8])]) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let project = dir.path().canonicalize().unwrap();
        for (name, bytes) in files {
            let path = project.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(LocalHistory::open(data.path().join("local-history"), Limits::default()));
        let mut encodings = encodings;
        for file in &mut encodings.files {
            file.path = project.join(&file.path);
        }
        let kept = Arc::new(Kept { encodings: Mutex::new(encodings), writes: Mutex::new(0) });
        session.state_mut::<ProjectSettingsState>().set_providers(vec![kept.clone()]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        Self { registry, session, project, kept, _dirs: (dir, data) }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.project.join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }

    fn open(&mut self, name: &str) {
        let path = self.path(name);
        self.run("file.document.open", json!({ "path": path }));
    }

    fn document(&self) -> &Document {
        self.session.state::<Workbench>().unwrap().active_document().unwrap()
    }

    fn disk(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.path(name)).unwrap()
    }

    fn type_text(&mut self, text: &str) {
        let at = self.document().text().chars().count();
        let document = self.session.state_mut::<Workbench>().active_document_mut().unwrap();
        assert!(document.replace_ranges(vec![(at..at, text.to_owned())]));
    }

    fn table(&self) -> Vec<(PathBuf, String)> {
        let encodings = self.kept.encodings.lock().unwrap();
        encodings.files.iter().map(|file| (file.path.clone(), file.charset.clone())).collect()
    }
}

const WINDOWS_1251: &[u8] = &[0xcf, 0xf0, 0xe8, b'\n'];
const CYRILLIC: &str = "\u{41f}\u{440}\u{438}\n";

fn encodings(default: Option<&str>, files: &[(&str, &str)]) -> Encodings {
    Encodings {
        default: default.map(str::to_owned),
        files: files
            .iter()
            .map(|(path, charset)| FileEncoding { path: PathBuf::from(path), charset: (*charset).to_owned() })
            .collect(),
        bom_for_new_files: None,
    }
}

#[test]
fn a_bom_goes_first_then_the_nearest_table_entry_then_project_then_global() {
    let table = encodings(Some("KOI8-R"), &[("old", "windows-1251"), ("old/inner", "ISO-8859-5")]);
    let mut fixture = Fixture::with(
        table,
        &[
            ("old/a.txt", WINDOWS_1251),
            ("old/inner/b.txt", &[0xbf, b'\n']),
            ("c.txt", &[0xf0, b'\n']),
            ("old/marked.txt", b"\xef\xbb\xbfhi\n"),
        ],
    );
    let charsets = [
        ("old/a.txt", "windows-1251", CYRILLIC),
        ("old/inner/b.txt", "ISO-8859-5", "\u{41f}\n"),
        ("c.txt", "KOI8-R", "\u{41f}\n"),
        ("old/marked.txt", "UTF-8 BOM", "hi\n"),
    ];
    for (name, charset, text) in charsets {
        fixture.open(name);
        assert_eq!(fixture.document().charset(), charset, "{name}");
        assert_eq!(fixture.document().text(), text, "{name}");
    }
}

#[test]
fn global_encoding_applies_when_the_project_says_nothing() {
    let mut fixture = Fixture::new(&[("a.txt", &[0xcf, b'\n']), ("b.dat", b"x\0y\n")]);
    fixture.session.settings_mut().set(project_settings::GLOBAL_ENCODING, "windows-1251").unwrap();
    fixture.open("a.txt");
    assert_eq!((fixture.document().charset(), fixture.document().text()), ("windows-1251", "\u{41f}\n"));
    assert_eq!(fixture.run("project.encodings.get", json!({}))["global"], "windows-1251");
}

#[test]
fn a_bom_is_hidden_kept_on_save_and_shown_on_the_widget() {
    let mut fixture = Fixture::new(&[("a.txt", b"\xef\xbb\xbfab\n")]);
    fixture.open("a.txt");
    assert_eq!(fixture.document().text(), "ab\n", "no U+FEFF in the text");
    assert_eq!(fixture.document().charset(), "UTF-8 BOM");
    assert_eq!(fixture.run("file.encoding.get", json!({}))["bom"], true);
    fixture.type_text("c");
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("a.txt"), b"\xef\xbb\xbfab\nc");
}

#[test]
fn a_utf_16_file_with_a_bom_is_editable_now() {
    let mut fixture = Fixture::new(&[("a.txt", b"\xff\xfea\x00\n\x00")]);
    fixture.open("a.txt");
    assert_eq!(fixture.document().read_only(), None);
    fixture.type_text("\u{44f}");
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("a.txt"), b"\xff\xfea\x00\n\x00\x4f\x04");
}

#[test]
fn add_and_remove_bom_change_the_file_not_its_text_and_keep_unsaved_edits() {
    let mut fixture = Fixture::new(&[("a.txt", b"ab\n")]);
    fixture.open("a.txt");
    fixture.type_text("unsaved");
    let added = fixture.run("file.bom.add", json!({}));
    assert_eq!(added["changed"].as_array().unwrap().len(), 1, "{added}");
    assert_eq!(fixture.disk("a.txt"), b"\xef\xbb\xbfab\n");
    assert_eq!(fixture.document().charset(), "UTF-8 BOM");
    assert_eq!(fixture.document().text(), "ab\nunsaved");
    assert!(fixture.document().is_dirty());
    let again = fixture.run("file.bom.add", json!({}));
    assert_eq!(again["skipped"].as_array().unwrap().len(), 1, "already has one: {again}");
    fixture.run("file.bom.remove", json!({}));
    assert_eq!(fixture.disk("a.txt"), b"ab\n");
    assert_eq!(fixture.document().charset(), "UTF-8");
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("a.txt"), b"ab\nunsaved", "the mark stays off");
}

#[test]
fn bom_commands_work_recursively_on_folders_and_skip_what_has_no_mark() {
    let table = encodings(None, &[("src/old.txt", "windows-1251")]);
    let mut fixture = Fixture::with(
        table,
        &[
            ("src/a.txt", b"a"),
            ("src/deep/b.txt", b"\xef\xbb\xbfb"),
            ("src/old.txt", WINDOWS_1251),
            ("src/c.bin", b"a\0b"),
        ],
    );
    let folder = fixture.path("src");
    let added = fixture.run("file.bom.add", json!({ "paths": [folder] }));
    assert_eq!(added["changed"], json!([fixture.path("src/a.txt")]), "{added}");
    assert_eq!(added["skipped"].as_array().unwrap().len(), 3, "{added}");
    assert_eq!(fixture.disk("src/a.txt"), b"\xef\xbb\xbfa");
    assert_eq!(fixture.disk("src/c.bin"), b"a\0b");
    assert_eq!(fixture.disk("src/old.txt"), WINDOWS_1251);
    let removed = fixture.run("file.bom.remove", json!({ "paths": [folder] }));
    assert_eq!(removed["changed"].as_array().unwrap().len(), 2, "{removed}");
    assert_eq!(fixture.disk("src/deep/b.txt"), b"b");
}

#[test]
fn removing_marks_from_a_folder_leaves_a_binary_that_starts_like_one() {
    let looks_utf8: &[u8] = b"\xef\xbb\xbf\xff\x00\x01";
    let looks_utf16: &[u8] = b"\xff\xfe\x00";
    let mut fixture = Fixture::new(&[
        ("src/a.dat", looks_utf8),
        ("src/b.dat", looks_utf16),
        ("src/c.txt", b"\xef\xbb\xbftext"),
        ("src/d.txt", b"\xff\xfea\x00"),
    ]);
    let folder = fixture.path("src");
    let removed = fixture.run("file.bom.remove", json!({ "paths": [folder] }));
    assert_eq!(removed["changed"].as_array().unwrap().len(), 2, "{removed}");
    assert_eq!(fixture.disk("src/a.dat"), looks_utf8);
    assert_eq!(fixture.disk("src/b.dat"), looks_utf16);
    assert_eq!(fixture.disk("src/c.txt"), b"text");
    assert_eq!(fixture.disk("src/d.txt"), b"a\x00");
}

#[test]
fn a_change_that_loses_nothing_is_made_and_remembered_at_once() {
    let mut fixture = Fixture::new(&[("a.txt", b"plain\n")]);
    fixture.open("a.txt");
    let change = fixture.run("file.encoding.set", json!({ "charset": "windows-1251" }));
    assert_eq!(change["applied"], true, "{change}");
    assert_eq!(change["remembered"], true, "{change}");
    assert_eq!(fixture.document().charset(), "windows-1251");
    assert_eq!(fixture.table(), [(fixture.path("a.txt"), "windows-1251".to_owned())]);
    assert_eq!(fixture.disk("a.txt"), b"plain\n");
}

#[test]
fn a_lossy_change_asks_and_changes_nothing_until_told_how() {
    let mut fixture = Fixture::new(&[("a.txt", CYRILLIC.as_bytes())]);
    fixture.open("a.txt");
    let change = fixture.run("file.encoding.set", json!({ "charset": "windows-1251" }));
    assert_eq!(change["applied"], false, "{change}");
    assert_eq!(change["canReload"], true, "{change}");
    assert_eq!(change["canConvert"], true, "{change}");
    assert_eq!(fixture.document().charset(), "UTF-8");
    assert!(fixture.table().is_empty());
}

#[test]
fn reload_reads_the_bytes_again_and_leaves_the_file_alone() {
    let mut fixture = Fixture::new(&[("a.txt", WINDOWS_1251)]);
    fixture.open("a.txt");
    assert!(fixture.document().read_only().is_some(), "not UTF-8");
    let change = fixture.run("file.encoding.set", json!({ "charset": "windows-1251", "action": "reload" }));
    assert_eq!(change["applied"], true, "{change}");
    assert_eq!(fixture.document().text(), CYRILLIC);
    assert_eq!(fixture.document().read_only(), None, "it can be edited now");
    assert_eq!(fixture.disk("a.txt"), WINDOWS_1251);
    assert!(!fixture.document().is_dirty());
}

#[test]
fn reload_is_refused_over_unsaved_edits() {
    let mut fixture = Fixture::new(&[("a.txt", b"plain\n")]);
    fixture.open("a.txt");
    fixture.type_text("x");
    let error = fixture.fail("file.encoding.set", json!({ "charset": "KOI8-R", "action": "reload" }));
    assert!(error.contains("KOI8-R"), "{error}");
    assert_eq!(fixture.document().text(), "plain\nx");
}

#[test]
fn convert_rewrites_the_file_and_a_foreign_character_stops_it() {
    let mut fixture = Fixture::new(&[("a.txt", CYRILLIC.as_bytes()), ("b.txt", "\u{4e2d}\n".as_bytes())]);
    fixture.open("a.txt");
    fixture.run("file.encoding.set", json!({ "charset": "windows-1251", "action": "convert" }));
    assert_eq!(fixture.disk("a.txt"), WINDOWS_1251);
    assert_eq!(fixture.document().charset(), "windows-1251");
    fixture.open("b.txt");
    let change = fixture.run("file.encoding.set", json!({ "charset": "windows-1251" }));
    assert_eq!(change["canConvert"], false, "{change}");
    assert!(change["unmappable"].as_str().unwrap().contains("U+4E2D"), "{change}");
    let error = fixture.fail("file.encoding.set", json!({ "charset": "windows-1251", "action": "convert" }));
    assert!(error.contains("U+4E2D"), "{error}");
    assert_eq!(fixture.disk("b.txt"), "\u{4e2d}\n".as_bytes(), "nothing lost");
    assert_eq!(fixture.document().charset(), "UTF-8");
}

#[test]
fn convert_writes_unsaved_edits_and_stops_at_a_change_made_outside() {
    let mut fixture = Fixture::new(&[("a.txt", CYRILLIC.as_bytes())]);
    fixture.open("a.txt");
    fixture.run("editor.edit.insert", json!({ "text": "\u{42f}" }));
    fixture.run("file.encoding.set", json!({ "charset": "windows-1251", "action": "convert" }));
    assert_eq!(fixture.document().text(), format!("\u{42f}{CYRILLIC}"), "the text does not change");
    assert!(!fixture.document().is_dirty());
    assert_eq!(fixture.disk("a.txt")[0], 0xdf, "the edit, in the new encoding");
    fixture.run("file.document.syncWithDisk", json!({}));
    assert_eq!(fixture.document().text(), format!("\u{42f}{CYRILLIC}"), "its own write is no change on disk");

    let path = fixture.path("a.txt");
    std::fs::write(&path, "theirs\n").unwrap();
    let error = fixture.fail("file.encoding.set", json!({ "charset": "KOI8-R", "action": "convert" }));
    assert!(error.contains("a.txt"), "{error}");
    assert_eq!(fixture.disk("a.txt"), b"theirs\n", "a change made outside is not written over");
    assert_eq!(fixture.document().charset(), "windows-1251");
}

#[test]
fn an_agent_never_converts_a_file_hidden_from_it() {
    let mut fixture = Fixture::new(&[("secret.txt", CYRILLIC.as_bytes())]);
    fixture.open("secret.txt");
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret.txt".to_owned()]);
    let params = json!({ "charset": "windows-1251", "action": "convert" });
    let Fixture { registry, session, .. } = &mut fixture;
    let refused =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "file.encoding.set", params));
    match refused {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some(crate::hidden_files::REDACTED_CODE), "{error:?}")
        }
        other => panic!("not refused: {other:?}"),
    }
    assert_eq!(fixture.disk("secret.txt"), CYRILLIC.as_bytes());
}

#[test]
fn convert_keeps_the_bytes_from_before_in_local_history() {
    let mut fixture = Fixture::new(&[("a.txt", CYRILLIC.as_bytes())]);
    let data = tempfile::tempdir().unwrap();
    let history = LocalHistory::open(data.path().join("history"), Default::default());
    fixture.session.set_state(history);
    fixture.open("a.txt");
    fixture.run("file.encoding.set", json!({ "charset": "windows-1251", "action": "convert" }));
    let path = fixture.path("a.txt");
    let history = fixture.run("localHistory.versions.list", json!({ "path": path }));
    assert!(
        history.to_string().contains("fileEncoding") || history.to_string().contains("Change File Encoding"),
        "{history}"
    );
}

#[test]
fn a_character_the_encoding_cannot_hold_refuses_the_save_and_loses_nothing() {
    let mut fixture = Fixture::with(encodings(None, &[("a.txt", "windows-1251")]), &[("a.txt", WINDOWS_1251)]);
    fixture.open("a.txt");
    fixture.type_text("\u{4e2d}");
    let error = fixture.fail("file.document.save", json!({}));
    assert!(error.contains("U+4E2D") && error.contains("windows-1251"), "{error}");
    assert_eq!(fixture.disk("a.txt"), WINDOWS_1251);
    assert!(fixture.document().is_dirty());
}

#[test]
fn going_back_to_the_inherited_encoding_drops_the_table_entry() {
    let mut fixture = Fixture::with(encodings(Some("KOI8-R"), &[("a.txt", "windows-1251")]), &[("a.txt", b"plain\n")]);
    fixture.open("a.txt");
    fixture.run("file.encoding.set", json!({ "charset": "KOI8-R" }));
    assert!(fixture.table().is_empty(), "{:?}", fixture.table());
}

#[test]
fn an_unknown_encoding_is_refused_everywhere() {
    let mut fixture = Fixture::new(&[("a.txt", b"a")]);
    fixture.open("a.txt");
    assert!(fixture.fail("file.encoding.set", json!({ "charset": "UTF-32" })).contains("UTF-32"));
    assert!(fixture.fail("project.encodings.set", json!({ "project": "nope" })).contains("nope"));
}

#[test]
fn the_settings_commands_write_through_the_provider() {
    let mut fixture = Fixture::new(&[("a.txt", b"a")]);
    let params = json!({
        "project": "windows-1251",
        "files": [{ "path": "old", "charset": "KOI8-R" }],
        "bomForNewFiles": "with",
    });
    let answer = fixture.run("project.encodings.set", params);
    assert_eq!(answer["project"], "windows-1251", "{answer}");
    assert_eq!(answer["bomForNewFiles"], "with", "{answer}");
    assert_eq!(fixture.table(), [(fixture.path("old"), "KOI8-R".to_owned())]);
    assert_eq!(*fixture.kept.writes.lock().unwrap(), 1);
}

#[test]
fn new_files_take_the_bom_setting_and_their_folders_encoding() {
    let mut table = encodings(None, &[("old", "windows-1251")]);
    table.bom_for_new_files = Some(NewFileBom::With);
    let mut fixture = Fixture::with(table, &[]);
    let session = &fixture.session;
    assert_eq!(new_file_bytes(session, &fixture.path("a.txt"), "a"), b"\xef\xbb\xbfa");
    assert_eq!(new_file_bytes(session, &fixture.path("old/a.txt"), CYRILLIC), WINDOWS_1251);
    fixture.run("project.encodings.set", json!({ "bomForNewFiles": "without" }));
    assert_eq!(new_file_bytes(&fixture.session, &fixture.path("a.txt"), "a"), b"a");
}

#[test]
fn every_change_of_bytes_leaves_a_local_history_entry() {
    let mut fixture = Fixture::new(&[("a.txt", b"a")]);
    fixture.open("a.txt");
    fixture.run("file.bom.add", json!({}));
    let path = fixture.path("a.txt");
    let versions = fixture.run("localHistory.versions.list", json!({ "path": path }));
    assert!(versions.to_string().contains("Byte Order Mark"), "{versions}");
}

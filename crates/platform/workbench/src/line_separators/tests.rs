use serde_json::{Value, json};

use super::*;
use crate::local_history::{Limits, LocalHistory};

struct Fixture {
    registry: Registry,
    session: Session,
    project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        zen_i18n::register(crate::LOCALES);
        let project = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = project.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(LocalHistory::open(data.path().join("local-history"), Limits::default()));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        Self { registry, session, project, _data: data }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.project.path().join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn open(&mut self, name: &str) {
        let path = self.path(name);
        self.run("file.document.open", json!({ "path": path }));
    }

    fn document(&self) -> &Document {
        self.session.state::<Workbench>().unwrap().active_document().unwrap()
    }

    fn caret(&mut self, offset: usize) {
        let document = self.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(offset)));
    }

    fn disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
    }
}

#[test]
fn the_separator_is_the_files_first_one() {
    let mut fixture = Fixture::new(&[("crlf.txt", "a\r\nb\r\n"), ("cr.txt", "a\rb"), ("mixed.txt", "a\r\nb\nc\n")]);
    for (name, label, mixed) in [("crlf.txt", "CRLF", false), ("cr.txt", "CR", false), ("mixed.txt", "CRLF", true)] {
        fixture.open(name);
        let info = fixture.run("file.lineSeparator.get", json!({}));
        assert_eq!(info["label"], label, "{name}");
        assert_eq!(info["mixed"], mixed, "{name}");
        assert_eq!(fixture.document().line_separator().label(), label);
    }
    let closed = fixture.path("cr.txt");
    fixture.run("file.document.close", json!({}));
    assert_eq!(fixture.run("file.lineSeparator.get", json!({ "path": closed }))["separator"], "cr");
}

#[test]
fn typed_and_pasted_lines_take_the_files_separator() {
    let mut fixture = Fixture::new(&[("win.txt", "one\r\ntwo\r\n")]);
    fixture.open("win.txt");
    fixture.caret(3);
    fixture.run("editor.edit.newline", json!({}));
    fixture.run("editor.edit.paste", json!({ "text": "x\ny" }));
    fixture.run("editor.edit.type", json!({ "text": "\n" }));
    assert_eq!(fixture.document().text(), "one\r\nx\r\ny\r\n\r\ntwo\r\n");
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("win.txt"), "one\r\nx\r\ny\r\n\r\ntwo\r\n", "saving keeps CRLF");
}

#[test]
fn changing_the_separator_is_one_undo_step_saved_with_a_local_history_entry() {
    let mut fixture = Fixture::new(&[("main.go", "package main\n\nfunc main() {}\n")]);
    fixture.open("main.go");
    fixture.caret(14);
    let changed = fixture.run("file.lineSeparator.set", json!({ "separator": "crlf" }));
    assert_eq!(changed["changed"], json!([fixture.path("main.go")]));
    let crlf = "package main\r\n\r\nfunc main() {}\r\n";
    assert_eq!(fixture.document().text(), crlf);
    assert!(!fixture.document().is_dirty(), "the file is written at once");
    assert_eq!(fixture.disk("main.go"), crlf);
    assert_eq!(fixture.document().line_separator(), LineSeparator::Crlf);
    assert_eq!(fixture.document().selections().primary().head, 16, "the caret stays on its line");
    let path = fixture.path("main.go");
    let versions = fixture.run("localHistory.versions.list", json!({ "path": path }));
    let titles: Vec<&str> =
        versions["versions"].as_array().unwrap().iter().map(|v| v["title"].as_str().unwrap()).collect();
    let action = t!("local-history-action-line-separators");
    let before = t!("local-history-before", action = action.as_str());
    assert_eq!(titles[..2], [action.as_str(), before.as_str()]);
    assert_eq!(fixture.run("editor.edit.undo", json!({})), json!(true));
    assert_eq!(fixture.document().text(), "package main\n\nfunc main() {}\n", "one step brings LF back");
    assert_eq!(fixture.document().line_separator(), LineSeparator::Lf);
    assert!(fixture.document().is_dirty(), "the file still holds CRLF until it is saved");
    assert_eq!(fixture.run("file.lineSeparator.set", json!({ "separator": "lf" }))["changed"], json!([]));
}

#[test]
fn a_folder_changes_all_its_text_files() {
    let mut fixture = Fixture::new(&[
        ("src/a.txt", "a\nb\n"),
        ("src/deep/b.go", "package b\r\n"),
        ("src/deep/c.txt", "one line"),
        ("src/bin.dat", "a\0\n"),
        ("src/target/d.txt", "x\n"),
        ("src/.git/config", "[core]\n"),
        ("other.txt", "o\n"),
    ]);
    fixture.open("src/a.txt");
    let src = fixture.path("src");
    let changed = fixture.run("file.lineSeparator.set", json!({ "separator": "cr", "paths": [src] }));
    let changed_files = [fixture.path("src/a.txt"), fixture.path("src/deep/b.go"), fixture.path("src/target/d.txt")];
    assert_eq!(changed["changed"], json!(changed_files));
    assert_eq!(changed["skipped"], json!([fixture.path("src/bin.dat")]));
    assert_eq!(fixture.disk("src/a.txt"), "a\rb\r");
    assert_eq!(fixture.document().text(), "a\rb\r", "the open file follows");
    assert_eq!(fixture.disk("src/deep/b.go"), "package b\r");
    assert_eq!(
        fixture.disk("src/target/d.txt"),
        "x\r",
        "a folder named target is ordinary until marked excluded (ADR-0123)"
    );
    assert_eq!(fixture.disk("src/.git/config"), "[core]\n");
    assert_eq!(fixture.disk("other.txt"), "o\n");
    let outside = tempfile::tempdir().unwrap();
    let error = fixture.registry.dispatch(
        &mut fixture.session,
        "file.lineSeparator.set",
        json!({ "separator": "lf", "paths": [outside.path()] }),
    );
    assert!(error.is_err(), "only the project's files");
}

#[test]
fn new_files_take_the_code_style_or_editorconfig_separator() {
    let mut fixture = Fixture::new(&[(".editorconfig", "root = true\n[*.bat]\nend_of_line = crlf\n")]);
    let project = fixture.project.path().to_owned();
    fixture.run("file.fs.newFile", json!({ "directory": project, "name": "run.bat" }));
    assert_eq!(fixture.document().line_separator(), LineSeparator::Crlf, "EditorConfig");
    fixture.run("editor.edit.newline", json!({}));
    assert_eq!(fixture.document().text(), "\r\n");
    fixture.session.settings_mut().set(NEW_FILES, "cr").unwrap();
    fixture.run("file.fs.newFile", json!({ "directory": project, "name": "notes.txt" }));
    assert_eq!(fixture.document().line_separator(), LineSeparator::Cr, "Code Style");
    fixture.session.settings_mut().set(NEW_FILES, SYSTEM).unwrap();
    fixture.run("file.fs.newFile", json!({ "directory": project, "name": "plain.txt" }));
    assert_eq!(fixture.document().line_separator(), LineSeparator::system());
}

#[test]
fn edits_from_agents_and_replace_in_files_keep_the_files_separator() {
    let mut fixture = Fixture::new(&[("win.txt", "one\r\ntwo\r\n"), ("closed.txt", "alpha\r\nbeta\r\n")]);
    let win = fixture.path("win.txt");
    let changed =
        fixture.run("editor.text.replace", json!({ "path": win, "oldText": "one\ntwo", "newText": "1\n2\n3" }));
    assert_eq!(changed["line"], 1);
    assert_eq!(fixture.document().text(), "1\r\n2\r\n3\r\n", "LF text stands for the file's CRLF");
    let index = fixture.session.state::<Workbench>().unwrap().index.clone().unwrap();
    while !index.is_ready() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let replaced = fixture.run(
        "search.text.replaceAll",
        json!({ "query": { "text": "beta" }, "replacement": "b\ne\nt\na", "mask": "closed.txt" }),
    );
    assert_eq!(replaced["replacements"], 1);
    assert_eq!(fixture.disk("closed.txt"), "alpha\r\nb\r\ne\r\nt\r\na\r\n", "a closed file keeps CRLF");
}

#[test]
fn a_change_of_separators_never_writes_over_a_change_made_outside() {
    let mut fixture = Fixture::new(&[("a.txt", "one\ntwo\n")]);
    fixture.open("a.txt");
    std::fs::write(fixture.path("a.txt"), "theirs\n").unwrap();
    let path = fixture.path("a.txt");
    let params = json!({ "path": path, "separator": "crlf" });
    assert!(fixture.registry.dispatch(&mut fixture.session, "file.lineSeparator.set", params).is_err());
    assert_eq!(fixture.disk("a.txt"), "theirs\n");
    assert_eq!(fixture.document().line_separator(), LineSeparator::Lf, "the document stays as it was");
}

#[test]
fn a_read_only_document_is_not_written_by_a_change_of_separators() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "one\ntwo\n").unwrap();
    let mut document = Document::read_only_text(&path, b"one\ntwo\n", "read-only".to_owned());
    assert!(document.convert_and_write(LineSeparator::Crlf).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
}

#[test]
fn a_change_that_cannot_be_written_leaves_the_document_and_the_file_alone() {
    let mut fixture = Fixture::new(&[("locked/win.txt", "one\r\ntwo\r\n")]);
    fixture.open("locked/win.txt");
    fixture.caret(3);
    fixture.run("editor.edit.type", json!({ "text": "!" }));
    let locked = fixture.path("locked");
    let mut permissions = std::fs::metadata(&locked).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o555);
    std::fs::set_permissions(&locked, permissions.clone()).unwrap();
    let result =
        fixture.registry.dispatch(&mut fixture.session, "file.lineSeparator.set", json!({ "separator": "lf" }));
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&locked, permissions).unwrap();
    let error = result.expect_err("the folder cannot be written");
    assert!(error.to_string().contains("win.txt"), "{error}");
    assert_eq!(fixture.document().text(), "one!\r\ntwo\r\n", "the unsaved edit is kept, unconverted");
    assert!(fixture.document().is_dirty());
    assert_eq!(fixture.document().line_separator(), LineSeparator::Crlf);
    assert_eq!(fixture.disk("locked/win.txt"), "one\r\ntwo\r\n");
    fixture.run("file.lineSeparator.set", json!({ "separator": "lf" }));
    assert_eq!(fixture.disk("locked/win.txt"), "one!\ntwo\n", "once writable, the unsaved edit is saved converted");
    assert!(!fixture.document().is_dirty());
}

struct ExcludingLogs;

impl zen_project::settings::SettingsProvider for ExcludingLogs {
    fn id(&self) -> &str {
        "excluding-logs"
    }

    fn watches(&self, _relative: &Path) -> bool {
        false
    }

    fn load(
        &self,
        _root: &Path,
        _files: &dyn zen_project::settings::ReadFiles,
    ) -> Option<zen_project::settings::ProjectSettings> {
        Some(zen_project::settings::ProjectSettings {
            exclude_patterns: vec!["*.log".to_owned()],
            ..Default::default()
        })
    }
}

#[test]
fn a_folder_skips_links_read_only_huge_and_excluded_files() {
    let mut fixture = Fixture::new(&[
        ("src/a.txt", "a\nb\n"),
        ("src/ro.txt", "r\n"),
        ("src/huge.txt", ""),
        ("src/logs/x.log", "l\n"),
        ("outside.txt", "o\n"),
    ]);
    std::os::unix::fs::symlink(fixture.path("outside.txt"), fixture.path("src/link.txt")).unwrap();
    std::fs::write(fixture.path("src/huge.txt"), "x\n".repeat(9 << 20)).unwrap();
    let ro = fixture.path("src/ro.txt");
    let mut permissions = std::fs::metadata(&ro).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&ro, permissions).unwrap();
    fixture
        .session
        .state_mut::<crate::project_settings::ProjectSettingsState>()
        .set_providers(vec![std::sync::Arc::new(ExcludingLogs)]);
    fixture.run("project.settings.refresh", json!({}));
    let src = fixture.path("src");
    let changed = fixture.run("file.lineSeparator.set", json!({ "separator": "crlf", "paths": [src] }));
    assert_eq!(changed["changed"], json!([fixture.path("src/a.txt")]));
    assert_eq!(changed["skipped"], json!([fixture.path("src/huge.txt"), fixture.path("src/ro.txt")]));
    assert_eq!(fixture.disk("outside.txt"), "o\n", "a link is not followed");
    assert_eq!(fixture.disk("src/ro.txt"), "r\n");
    assert_eq!(fixture.disk("src/logs/x.log"), "l\n", "excluded by the project");
    let path = fixture.path("src/a.txt");
    let versions = fixture.run("localHistory.versions.list", json!({ "path": path }));
    assert_eq!(versions["versions"].as_array().unwrap().len(), 2, "one change: the text before and after");
}

mod properties {
    use proptest::prelude::*;
    use zen_editor::{EditOp, Selection, Selections};
    use zen_text::Edit;

    use super::*;

    #[derive(Clone, Debug)]
    enum Step {
        Type(String),
        Paste(String),
        Back,
        Forward,
        Carets(Vec<usize>),
        Replace(usize, usize, String),
        Action(usize, usize, String),
        Undo,
        Redo,
    }

    fn step() -> impl Strategy<Value = Step> {
        let text = "[ab\\r\\n]{0,4}";
        prop_oneof![
            text.prop_map(Step::Type),
            text.prop_map(Step::Paste),
            Just(Step::Back),
            Just(Step::Forward),
            prop::collection::vec(0usize..40, 1..4).prop_map(Step::Carets),
            (0usize..40, 0usize..4, text).prop_map(|(at, len, new)| Step::Replace(at, len, new)),
            (0usize..40, 0usize..4, text).prop_map(|(at, len, new)| Step::Action(at, len, new)),
            Just(Step::Undo),
            Just(Step::Redo),
        ]
    }

    fn separator() -> impl Strategy<Value = LineSeparator> {
        prop_oneof![Just(LineSeparator::Lf), Just(LineSeparator::Crlf), Just(LineSeparator::Cr)]
    }

    fn caret_places(text: &zen_text::Text) -> Vec<usize> {
        (0..text.len_lines())
            .flat_map(|line| {
                let start = text.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or(0);
                let len = text.line(line).map(|line| line.chars().count()).unwrap_or(0);
                start..=start + len
            })
            .collect()
    }

    proptest! {
        #[test]
        fn editing_keeps_the_files_separator(
            separator in separator(),
            lines in prop::collection::vec("[a-c ]{0,4}", 1..6),
            steps in prop::collection::vec(step(), 1..12),
        ) {
            let text = lines.join(separator.as_str());
            let mut document = Document::with_text(Path::new("/p/a.txt"), text.clone());
            document.default_line_separator(separator);
            for step in steps {
                match step {
                    Step::Type(text) => { document.apply_op(&EditOp::Insert { text }); }
                    Step::Paste(text) => { document.apply_op(&EditOp::Paste { text }); }
                    Step::Back => { document.apply_op(&EditOp::DeleteBackward); }
                    Step::Forward => { document.apply_op(&EditOp::DeleteForward); }
                    Step::Carets(picks) => {
                        let places = caret_places(document.snapshot());
                        let carets = picks.iter().map(|pick| Selection::caret(places[pick % places.len()])).collect();
                        document.set_selections(Selections::from_vec(carets, 0));
                    }
                    Step::Replace(at, len, new) => {
                        let places = caret_places(document.snapshot());
                        let start = places[at % places.len()];
                        let end = places.iter().copied().filter(|place| *place >= start).nth(len).unwrap_or(start);
                        document.replace_ranges(vec![(start..end, new)]);
                    }
                    Step::Action(at, len, new) => {
                        let places = caret_places(document.snapshot());
                        let start = places[at % places.len()];
                        let end = places.iter().copied().filter(|place| *place >= start).nth(len).unwrap_or(start);
                        let selections = document.selections().clone();
                        document.apply_action(vec![Edit::replace(start..end, new)], selections);
                    }
                    Step::Undo => { document.undo(); }
                    Step::Redo => { document.redo(); }
                }
                let now = document.text();
                let snapshot = document.snapshot().to_string();
                prop_assert_eq!(now, snapshot.as_str());
                prop_assert!(
                    zen_text::line_separator::detect(now).is_none_or(|found| found == separator),
                    "{:?} became {:?}", text, now
                );
                prop_assert!(!zen_text::line_separator::is_mixed(now), "{:?} became {:?}", text, now);
                prop_assert_eq!(document.line_separator(), separator);
                let len = document.snapshot().len_chars();
                prop_assert!(document.selections().iter().all(|selection| selection.head <= len));
            }
        }
    }
}

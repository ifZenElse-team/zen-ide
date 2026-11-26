use serde_json::{Value, json};
use zen_text::LineSeparator;

use super::*;
use crate::Document;

const CONFIG: &str = "root = true\n\n[*]\nindent_style = space\nindent_size = 2\n\n[*.go]\nindent_style = tab\nindent_size = tab\n\
                      tab_width = 8\nmax_line_length = 100\nij_go_continuation_indent_size = 4\n\n\
                      [{sub/**.txt,legacy/*.txt}]\nindent_size = 3\nmax_line_length = off\nij_visual_guides = 80,120\n";

struct Fixture {
    registry: Registry,
    session: Session,
    project: tempfile::TempDir,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let project = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = project.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        Self { registry, session, project }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.project.path().join(name)
    }

    fn write_quietly(&self, name: &str, text: &str) {
        let path = self.path(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn write(&mut self, name: &str, text: &str) {
        self.write_quietly(name, text);
        self.watcher_saw(name);
    }

    fn watcher_saw(&mut self, name: &str) {
        let path = self.path(name);
        self.run("project.settings.refresh", json!({ "paths": [path] }));
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

    fn caret(&mut self, anchor: usize, head: usize) {
        self.run("editor.selection.set", json!({ "selections": [{ "anchor": anchor, "head": head }] }));
    }

    fn unit(&self) -> String {
        project_settings::indent_unit_of(&self.session, self.document())
    }

    fn style(&self, name: &str) -> FileCodeStyle {
        project_settings::file_code_style(&self.session, &self.path(name))
    }

    fn set_enabled(&mut self, on: bool) {
        self.session.settings_mut().set(ENABLED, on).unwrap();
    }

    fn wait_for_index(&self) {
        let index = self.session.state::<Workbench>().unwrap().index.clone().unwrap();
        while !index.is_ready() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn the_nearest_files_set_the_style_of_each_file() {
    let fixture = Fixture::new(&[
        (".editorconfig", CONFIG),
        ("main.go", ""),
        ("notes.txt", ""),
        ("sub/deep/a.txt", ""),
        ("sub/deep/.editorconfig", "[a.txt]\nindent_size = unset\ntab_width = 6\n"),
        ("legacy/b.txt", ""),
        ("legacy/deeper/c.txt", ""),
    ]);
    let style = |use_tabs, tab_size, indent| (use_tabs, tab_size, indent);
    let of = |name: &str| {
        let file = fixture.style(name);
        (file.use_tabs, file.tab_size, file.indent)
    };
    assert_eq!(of("main.go"), style(true, 8, 8), "tabs as wide as tab_width");
    assert_eq!(fixture.style("main.go").right_margin, Some(100));
    assert_eq!(fixture.style("main.go").continuation_indent, Some(4));
    assert_eq!(of("notes.txt"), style(false, 2, 2));
    assert_eq!(of("legacy/b.txt"), style(false, 3, 3), "{{a,b}} and * of a later section");
    assert_eq!(of("legacy/deeper/c.txt"), style(false, 2, 2), "* stops at a slash");
    assert_eq!(fixture.style("legacy/b.txt").visual_guides, vec![80, 120]);
    assert_eq!(fixture.style("legacy/b.txt").right_margin, None, "max_line_length = off");
    assert_eq!(
        of("sub/deep/a.txt"),
        style(false, 6, 4),
        "the nearest file unsets indent_size: the IDE's indent with the nearest tab_width"
    );
    assert_eq!(of("other.rs"), style(false, 2, 2));
}

#[test]
fn unset_and_missing_properties_fall_back_to_the_project_then_the_ide() {
    struct GoStyle;

    impl zen_project::settings::SettingsProvider for GoStyle {
        fn id(&self) -> &str {
            "go-style"
        }

        fn watches(&self, _: &Path) -> bool {
            false
        }

        fn load(
            &self,
            _: &Path,
            _: &dyn zen_project::settings::ReadFiles,
        ) -> Option<zen_project::settings::ProjectSettings> {
            let go = zen_project::settings::CodeStyleOverride {
                use_tabs: Some(false),
                indent: Some(3),
                right_margin: Some(90),
                ..Default::default()
            };
            let mut settings = zen_project::settings::ProjectSettings::default();
            settings.code_style.insert("go".to_owned(), go);
            Some(settings)
        }
    }

    let mut fixture = Fixture::new(&[(".editorconfig", "[*.go]\ntab_width = 6\n"), ("main.go", "")]);
    let root = fixture.path("");
    let state = fixture.session.state_mut::<project_settings::ProjectSettingsState>();
    state.set_providers(vec![Arc::new(GoStyle)]);
    state.refresh(Some(&root));
    let style = fixture.style("main.go");
    assert_eq!((style.use_tabs, style.tab_size, style.indent, style.right_margin), (false, 6, 3, Some(90)));
    fixture.write(".editorconfig", "[*.go]\ntab_width = 6\nindent_style = tab\nmax_line_length = off\n");
    let style = fixture.style("main.go");
    assert_eq!((style.use_tabs, style.tab_size, style.indent, style.right_margin), (true, 6, 6, None));
    fixture.write(".editorconfig", "[*]\nindent_style = tab\n[*.go]\nindent_style = unset\nindent_size = unset\n");
    let style = fixture.style("main.go");
    assert_eq!((style.use_tabs, style.tab_size, style.indent), (false, 4, 3), "the project's, then the IDE's tab size");
}

#[test]
fn enter_tab_shift_tab_and_the_status_bar_follow_editorconfig() {
    let mut fixture = Fixture::new(&[(".editorconfig", CONFIG), ("main.go", "func f() {}\n"), ("notes.txt", "a\nb\n")]);
    fixture.open("main.go");
    assert_eq!(fixture.unit(), "\t");
    fixture.caret(10, 10);
    fixture.run("editor.edit.newline", json!({}));
    assert_eq!(fixture.document().text(), "func f() {\n\t\n}\n");
    let tab_size = project_settings::code_style_for_path(&fixture.session, &fixture.path("main.go")).tab_size;
    assert_eq!(tab_size, 8, "tabs are drawn as wide as tab_width");

    fixture.open("notes.txt");
    assert_eq!(fixture.unit(), "  ");
    fixture.caret(0, 3);
    fixture.run("editor.line.indent", json!({}));
    assert_eq!(fixture.document().text(), "  a\n  b\n");
    fixture.run("editor.line.outdent", json!({}));
    assert_eq!(fixture.document().text(), "a\nb\n");
    fixture.caret(1, 1);
    fixture.run("editor.edit.tab", json!({}));
    assert_eq!(fixture.document().text(), "a  \nb\n");
}

#[test]
fn turning_support_off_ignores_every_file() {
    let mut fixture = Fixture::new(&[
        (".editorconfig", "root = true\n[*]\nindent_style = tab\nend_of_line = crlf\nfoo\n"),
        ("notes.txt", "a\n"),
    ]);
    fixture.open("notes.txt");
    assert_eq!(fixture.unit(), "\t");
    assert_eq!(crate::line_separators::for_new_file(&fixture.session, &fixture.path("new.txt")), LineSeparator::Crlf);
    fixture.wait_for_index();
    let settings = fixture.run("project.settings.get", json!({}));
    assert_eq!(settings["settings"]["problems"].as_array().unwrap().len(), 1);
    fixture.set_enabled(false);
    assert_eq!(fixture.unit(), "    ", "the IDE's style");
    assert_eq!(
        crate::line_separators::for_new_file(&fixture.session, &fixture.path("new.txt")),
        LineSeparator::system(),
        "end_of_line is ignored too"
    );
    let settings = fixture.run("project.settings.get", json!({}));
    assert_eq!(settings["settings"]["problems"], json!([]));
    let answer = fixture.run("project.editorConfig.get", json!({}));
    assert_eq!(answer["enabled"], false);
    assert_eq!(answer["files"], json!([]));
    fixture.set_enabled(true);
    assert_eq!(fixture.unit(), "\t");
}

#[test]
fn editing_or_adding_a_file_applies_to_open_files_at_once() {
    let mut fixture = Fixture::new(&[("notes.txt", "a\n"), ("sub/a.txt", "b\n")]);
    fixture.open("sub/a.txt");
    assert_eq!(fixture.unit(), "    ");
    fixture.write(".editorconfig", "[*.txt]\nindent_size = 2\n");
    assert_eq!(fixture.unit(), "  ", "a file added above");
    fixture.write(".editorconfig", "[*.txt]\nindent_size = 3\n");
    assert_eq!(fixture.unit(), "   ", "the file edited");
    fixture.write("sub/.editorconfig", "[*]\nindent_style = tab\n");
    assert_eq!(fixture.unit(), "\t", "a nearer file added");
    std::fs::remove_file(fixture.path("sub/.editorconfig")).unwrap();
    fixture.watcher_saw("sub/.editorconfig");
    assert_eq!(fixture.unit(), "   ", "the nearer file removed");
}

#[test]
fn without_the_watcher_the_disk_is_looked_at_again_after_the_recheck_time() {
    let mut fixture = Fixture::new(&[("notes.txt", "a\n")]);
    fixture.session.set_state(EditorConfigs::with_recheck(Duration::from_millis(200)));
    fixture.open("notes.txt");
    assert_eq!(fixture.unit(), "    ");
    fixture.write_quietly(".editorconfig", "[*.txt]\nindent_size = 2\n");
    let deadline = Instant::now() + Duration::from_secs(60);
    while fixture.unit() != "  " && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fixture.unit(), "  ");
}

#[test]
fn every_keystroke_is_answered_from_the_cache_in_a_project_with_hundreds_of_files() {
    let mut files: Vec<(String, String)> =
        vec![(".editorconfig".to_owned(), "root = true\n[*]\nindent_size = 2\n".to_owned())];
    let mut deep = String::new();
    for level in 0..40 {
        deep.push_str(&format!("d{level}/"));
        files.push((
            format!("{deep}.editorconfig"),
            format!("[*.go]\nindent_style = tab\ntab_width = {}\n", level % 8 + 1),
        ));
        for side in 0..10 {
            files.push((format!("{deep}s{side}/.editorconfig"), "[{a,b}/**/*.{go,txt}]\nindent_size = 3\n".to_owned()));
        }
    }
    let leaf = format!("{deep}main.go");
    files.push((leaf.clone(), "func f() {\n}\n".to_owned()));
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
    let mut fixture = Fixture::new(&borrowed);
    assert_eq!(files.len(), 442);
    fixture.session.set_state(EditorConfigs::with_recheck(Duration::from_secs(3600)));
    fixture.open(&leaf);
    let started = Instant::now();
    assert_eq!(fixture.unit(), "\t");
    let first = started.elapsed();
    for (name, _) in &files {
        if name.ends_with(FILE_NAME) {
            std::fs::remove_file(fixture.path(name)).unwrap();
        }
    }
    for _ in 0..10_000 {
        assert_eq!(fixture.unit(), "\t", "answered without reading the disk: the files are gone");
        assert_eq!(project_settings::code_style_for_path(&fixture.session, &fixture.path(&leaf)).tab_size, 8);
    }
    let _ = first;
    fixture.watcher_saw(".editorconfig");
    let answer = fixture.run("project.editorConfig.get", json!({ "path": leaf }));
    assert_eq!(answer["files"], json!([]), "the watcher's change makes it read the disk again");
}

#[test]
fn parse_errors_are_settings_problems() {
    let mut fixture = Fixture::new(&[
        (".editorconfig", "root = true\n[*]\nindent_size = big\n"),
        ("deep/er/.editorconfig", "[*\nindent_style = tab\n"),
        ("notes.txt", ""),
    ]);
    fixture.wait_for_index();
    let answer = fixture.run("project.settings.get", json!({}));
    let problems = answer["settings"]["problems"].clone();
    assert_eq!(
        problems,
        json!([
            { "file": fixture.path(".editorconfig"), "message": "Line 3: indent_size cannot be “big”." },
            { "file": fixture.path("deep/er/.editorconfig"), "message": "Line 1: The section “[*” has no closing “]”." },
        ])
    );
    let sources = answer["settings"]["sources"].as_array().unwrap();
    assert!(sources.contains(&json!(fixture.path("deep/er/.editorconfig"))));
    fixture.write_quietly("deep/er/.editorconfig", "[*]\nindent_style = tab\n");
    let answer = fixture.run("project.settings.get", json!({}));
    assert_eq!(answer["settings"]["problems"].as_array().unwrap().len(), 1, "fixed on disk, gone at once");
}

#[test]
fn agents_read_what_editorconfig_gives_a_file() {
    let mut fixture = Fixture::new(&[(".editorconfig", CONFIG), ("main.go", "package main\n")]);
    let answer = fixture.run("project.editorConfig.get", json!({ "path": "main.go" }));
    assert_eq!(answer["enabled"], true);
    assert_eq!(answer["files"], json!([fixture.path(".editorconfig")]));
    assert_eq!(answer["properties"]["indent_style"], "tab");
    assert_eq!(answer["properties"]["ij_go_continuation_indent_size"], "4");
    assert_eq!(answer["style"]["useTabs"], true);
    assert_eq!(answer["style"]["indent"], "tabSize");
    assert_eq!(answer["style"]["rightMargin"], json!({ "column": 100 }));
    assert_eq!(
        answer["codeStyle"],
        json!({ "useTabs": true, "tabSize": 8, "indent": 8, "continuationIndent": 4, "rightMargin": 100, "visualGuides": [] })
    );
    let outside = fixture.registry.dispatch(
        &mut fixture.session,
        "project.editorConfig.get",
        json!({ "path": "/elsewhere/a.go" }),
    );
    assert!(outside.is_err());
    fixture.open("main.go");
    let active = fixture.run("project.editorConfig.get", json!({}));
    assert_eq!(active["path"], json!(fixture.path("main.go")));
}

#[test]
fn the_drawn_margins_take_max_line_length_and_ij_visual_guides() {
    let mut fixture = Fixture::new(&[
        (
            ".editorconfig",
            "[*.go]\nmax_line_length = 100\nij_go_visual_guides = 72, 80\n[*.txt]\nmax_line_length = off\n",
        ),
        ("main.go", ""),
        ("notes.txt", ""),
        ("other.rs", ""),
    ]);
    let margins = |fixture: &Fixture, name: &str| margins_over(&fixture.session, &fixture.path(name), (110, vec![60]));
    assert_eq!(margins(&fixture, "main.go"), (100, vec![72, 80]));
    assert_eq!(margins(&fixture, "notes.txt"), (110, vec![60]), "off leaves the margin to the project and the IDE");
    assert_eq!(margins(&fixture, "other.rs"), (110, vec![60]));
    fixture.set_enabled(false);
    assert_eq!(margins(&fixture, "main.go"), (110, vec![60]));
}

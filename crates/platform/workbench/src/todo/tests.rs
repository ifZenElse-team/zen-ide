use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use zen_project::settings::{ProjectSettings, ReadFiles, Scope, SettingsProvider};
use zen_syntax::{HighlightKind, Highlighter};

use super::*;
use crate::project_settings::ProjectSettingsState;
use crate::vcs::changelists::LocalChanges;
use crate::vcs::testing::{git, isolate};

fn span(range: Range<usize>, kind: HighlightKind) -> Span {
    Span { range, kind }
}

fn keywords(text: &str) -> Vec<Range<usize>> {
    zen_text::search::find_all(text, &TodoPatterns::default().query()).unwrap()
}

fn texts(text: &str, spans: &[Span], end: Option<&str>) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    todo_ranges(text, &keywords(text), spans, end).into_iter().map(|range| chars[range].iter().collect()).collect()
}

#[test]
fn only_comments_count_one_item_a_line_to_the_comment_end() {
    let text = "s := \"TODO no\" // TODO: yes todo\nx /* FIXME later */ y\n// todos are not todo\n";
    let first = text.find("//").unwrap();
    let line_one = text.find('\n').unwrap();
    let block = text.find("/*").unwrap();
    let block_end = text.find("*/").unwrap() + 2;
    let last = text.rfind("//").unwrap();
    let spans = [
        span(5..14, HighlightKind::String),
        span(first..line_one, HighlightKind::Comment),
        span(block..block_end, HighlightKind::Comment),
        span(last..text.len() - 1, HighlightKind::DocComment),
    ];
    assert_eq!(texts(text, &spans, Some("*/")), ["TODO: yes todo", "FIXME later", "todo"]);
}

#[test]
fn a_block_comment_over_lines_gives_each_line_its_item() {
    let text = "/*\n * TODO one\n * fixme two */\n";
    let spans = [span(0..text.len() - 1, HighlightKind::Comment)];
    assert_eq!(texts(text, &spans, Some("*/")), ["TODO one", "fixme two"]);
    assert!(texts(text, &[], Some("*/")).is_empty(), "no comments, no items");
}

#[test]
fn non_ascii_text_keeps_character_offsets() {
    let text = "// привет TODO: проверить\n";
    let spans = [span(0..text.len() - 1, HighlightKind::Comment)];
    let ranges = todo_ranges(text, &keywords(text), &spans, None);
    assert_eq!((ranges.len(), ranges[0].clone()), (1, 10..25));
}

struct Scopes;

impl SettingsProvider for Scopes {
    fn id(&self) -> &str {
        "test"
    }

    fn watches(&self, _: &Path) -> bool {
        false
    }

    fn load(&self, _: &Path, _: &dyn ReadFiles) -> Option<ProjectSettings> {
        let scope = Scope { name: "Store".to_owned(), pattern: "file:store//*".to_owned() };
        Some(ProjectSettings { scopes: vec![scope], ..ProjectSettings::default() })
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for (file, text) in files {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        crate::find::tests::literals(&mut session);
        session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Scopes)]);
        Fixture { registry, session, root, _dir: dir }
    }

    fn open(&mut self) {
        let root = self.root.clone();
        self.run("file.project.open", json!({ "path": root }));
        while !self.session.state::<Workbench>().unwrap().index.as_ref().unwrap().is_ready() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap()
    }

    fn items(&mut self, params: Value) -> Vec<(String, u64, String)> {
        let list = self.run("todo.items.list", params);
        rows(&list)
    }
}

fn rows(list: &Value) -> Vec<(String, u64, String)> {
    list["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| {
            file["items"].as_array().unwrap().iter().map(move |item| {
                (
                    file["relative"].as_str().unwrap().to_owned(),
                    item["line"].as_u64().unwrap(),
                    item["text"].as_str().unwrap().to_owned(),
                )
            })
        })
        .collect()
}

fn row(file: &str, line: u64, text: &str) -> (String, u64, String) {
    (file.to_owned(), line, text.to_owned())
}

const FILES: &[(&str, &str)] = &[
    ("main.go", "package main\n\n// TODO: wire the store\nvar s = \"TODO not this\"\n"),
    ("store/store.go", "package store\n\nfunc Get() {} // FIXME slow\n"),
    ("notes.txt", "TODO plain text has no comments\n"),
    (".idea/lib.go", "package lib // TODO not indexed\n"),
];

#[test]
fn the_project_tab_lists_comments_of_indexed_files_only() {
    let mut fixture = Fixture::new(FILES);
    fixture.open();
    let list = fixture.run("todo.items.list", json!({}));
    assert_eq!(rows(&list), [row("main.go", 2, "TODO: wire the store"), row("store/store.go", 2, "FIXME slow")]);
    assert_eq!(
        (list["total"].as_u64(), list["done"].as_bool(), &list["tab"]),
        (Some(2), Some(true), &json!("project"))
    );
    let item = &list["files"][0]["items"][0];
    assert_eq!((item["column"].as_u64(), item["length"].as_u64()), (Some(3), Some(20)));
    let text = std::fs::read_to_string(fixture.root.join("main.go")).unwrap();
    let offset = item["offset"].as_u64().unwrap() as usize;
    assert!(text.chars().skip(offset).collect::<String>().starts_with("TODO: wire"));
    let tabs = fixture.run("todo.tabs.list", json!({}));
    assert_eq!(tabs["tabs"], json!(["project", "currentFile", "scopeBased"]), "no Git, no changelist");
    assert_eq!(tabs["patterns"], json!(crate::todo_patterns::DEFAULT_PATTERNS));
}

#[test]
fn the_current_file_follows_unsaved_edits_and_a_scope_its_files() {
    let mut fixture = Fixture::new(FILES);
    fixture.open();
    let none = fixture.run("todo.items.list", json!({ "tab": "currentFile" }));
    assert!(rows(&none).is_empty());
    assert_eq!(none["error"], json!(t!("workbench-todo-no-file")));
    let main = fixture.root.join("main.go");
    fixture.run("file.document.open", json!({ "path": main }));
    fixture.run("editor.edit.type", json!({ "text": "// fixme: first\n" }));
    let current = fixture.items(json!({ "tab": "currentFile" }));
    assert_eq!(current, [row("main.go", 0, "fixme: first"), row("main.go", 3, "TODO: wire the store")]);
    let project = fixture.items(json!({}));
    assert_eq!(project.len(), 3, "the project reads the open file's unsaved text too: {project:?}");
    let store = fixture.items(json!({ "tab": "scopeBased", "scope": "named:Store" }));
    assert_eq!(store, [row("store/store.go", 2, "FIXME slow")]);
    let error = fixture.registry.dispatch(
        &mut fixture.session,
        "todo.items.list",
        json!({ "tab": "scopeBased", "scope": "named:Nope" }),
    );
    assert!(error.is_err());
}

#[test]
fn a_scan_runs_in_the_background_and_follows_changes_on_disk() {
    let mut fixture = Fixture::new(FILES);
    fixture.open();
    let first = fixture.run("todo.scan.start", json!({}))["scan"].as_u64().unwrap();
    let results = wait(&mut fixture, first);
    assert_eq!(rows(&results).len(), 2);
    std::fs::write(fixture.root.join("store/more.go"), "package store\n// TODO more\n").unwrap();
    let more = fixture.root.join("store/more.go");
    fixture.run("file.index.refresh", json!({ "paths": [more] }));
    let second = fixture.run("todo.scan.start", json!({ "tab": "scopeBased", "scope": "named:Store" }))["scan"]
        .as_u64()
        .unwrap();
    assert_eq!(second, first + 1);
    let results = wait(&mut fixture, second);
    assert_eq!(rows(&results), [row("store/more.go", 1, "TODO more"), row("store/store.go", 2, "FIXME slow")]);
    assert_eq!((&results["tab"], &results["scope"]), (&json!("scopeBased"), &json!("named:Store")));
}

fn wait(fixture: &mut Fixture, scan: u64) -> Value {
    for _ in 0..400 {
        let results = fixture.run("todo.scan.results", json!({}));
        if results["scan"] == scan && results["done"] == true {
            return results;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the scan did not finish");
}

#[test]
fn the_current_changelist_lists_the_active_lists_files() {
    isolate();
    let mut fixture = Fixture::new(FILES);
    let root = fixture.root.clone();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let data = tempfile::tempdir().unwrap();
    fixture.session.state_mut::<LocalChanges>().set_folder(data.path().to_owned());
    fixture.open();
    std::fs::write(root.join("store/store.go"), "package store\n\n// TODO changed\nfunc Get() {} // FIXME slow\n")
        .unwrap();
    fixture.run("vcs.status.refresh", json!({ "wait": true }));
    let tabs = fixture.run("todo.tabs.list", json!({}));
    assert_eq!(tabs["tabs"].as_array().unwrap().last(), Some(&json!("currentChangelist")));
    let changed = fixture.items(json!({ "tab": "currentChangelist" }));
    assert_eq!(changed, [row("store/store.go", 2, "TODO changed"), row("store/store.go", 3, "FIXME slow")]);
}

struct Counting(Arc<std::sync::atomic::AtomicUsize>);

impl Highlighter for Counting {
    fn handles(&self, file_name: &str) -> bool {
        crate::find::tests::Literals.handles(file_name)
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        self.0.fetch_add(1, Ordering::Relaxed);
        crate::find::tests::Literals.highlight(text)
    }
}

#[test]
fn a_rescan_reads_only_the_files_changed_since() {
    let mut fixture = Fixture::new(FILES);
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counting: Arc<dyn Highlighter> = Arc::new(Counting(Arc::clone(&count)));
    fixture
        .session
        .state_mut::<Highlighters>()
        .set_source(Arc::new(move |name| crate::find::tests::Literals.handles(name).then(|| Arc::clone(&counting))));
    fixture.open();
    let read = |fixture: &mut Fixture| {
        let before = count.load(Ordering::Relaxed);
        let rows = fixture.items(json!({}));
        (count.load(Ordering::Relaxed) - before, rows.len())
    };
    assert_eq!(read(&mut fixture), (2, 2), "main.go and store/store.go");
    assert_eq!(read(&mut fixture), (0, 2), "nothing changed");
    let store = fixture.root.join("store/store.go");
    std::fs::write(&store, "package store\n\n// TODO changed\nfunc Get() {} // FIXME slow\n").unwrap();
    fixture.run("file.index.refresh", json!({ "paths": [store] }));
    assert_eq!(read(&mut fixture), (1, 3), "only the file changed on disk");
    let main = fixture.root.join("main.go");
    fixture.run("file.document.open", json!({ "path": main }));
    fixture.run("editor.edit.type", json!({ "text": "// fixme: typed\n" }));
    assert_eq!(read(&mut fixture), (1, 4), "only the file edited");
}

#[test]
fn indented_lines_below_a_todo_belong_to_it() {
    let text = "// TODO: one\n//   and more\n//   and again\n// plain\n//   not this\n";
    let spans: Vec<Span> = text
        .split_inclusive('\n')
        .scan(0, |at, line| {
            let start = *at;
            *at += line.len();
            Some(span(start..*at - 1, HighlightKind::Comment))
        })
        .collect();
    let chars: Vec<char> = text.chars().collect();
    let kept = todo_ranges(text, &keywords(text), &spans, None);
    let compiled = TodoPatterns::default().compiled();
    let more = continuation_ranges(text, &kept, &spans, None, |line| compiled.has_match(line));
    let shown: Vec<String> = more.into_iter().map(|range| chars[range].iter().collect()).collect();
    assert_eq!(shown, ["and more", "and again"], "a line that is not indented ends the item");
}

fn set_todo(fixture: &mut Fixture, settings: &crate::todo_patterns::TodoSettings) {
    fixture.session.settings_mut().set(crate::todo_patterns::SETTINGS, settings).unwrap();
}

fn pattern(text: &str) -> crate::todo_patterns::TodoPattern {
    crate::todo_patterns::TodoPattern { pattern: text.to_owned(), ..Default::default() }
}

#[test]
fn the_tool_window_and_the_completion_words_use_the_patterns_of_the_settings() {
    let file = ("a.go", "package a\n\n// TODO one\n// HACK two\n");
    let mut fixture = Fixture::new(&[file]);
    fixture.open();
    assert_eq!(fixture.items(json!({})), [row("a.go", 2, "TODO one")]);
    let settings = crate::todo_patterns::TodoSettings { patterns: vec![pattern(r"\bhack\b.*")], ..Default::default() };
    set_todo(&mut fixture, &settings);
    assert_eq!(fixture.items(json!({})), [row("a.go", 3, "HACK two")], "the same scan, not remembered results");
    assert_eq!(fixture.run("todo.tabs.list", json!({}))["patterns"], json!([r"\bhack\b.*"]));
    assert_eq!(fixture.run("todo.patterns.list", json!({}))["words"], json!(["HACK"]));
}

#[test]
fn a_filter_lists_the_items_of_its_patterns_only() {
    let file = ("a.go", "package a\n\n// TODO one\n// FIXME two\n");
    let mut fixture = Fixture::new(&[file]);
    fixture.open();
    let mut settings = crate::todo_patterns::TodoSettings::default();
    let bugs = crate::todo_patterns::TodoFilter { name: "bugs".to_owned(), patterns: vec![r"\bfixme\b.*".to_owned()] };
    settings.filters.push(bugs);
    set_todo(&mut fixture, &settings);
    assert_eq!(fixture.items(json!({ "filter": "bugs" })), [row("a.go", 3, "FIXME two")]);
    assert_eq!(fixture.items(json!({})).len(), 2);
}

#[test]
fn continuation_lines_join_their_item_unless_the_setting_is_off() {
    let file = ("a.go", "package a\n\n// TODO one\n//   more\n// plain\n");
    let mut fixture = Fixture::new(&[file]);
    fixture.open();
    let list = fixture.run("todo.items.list", json!({}));
    assert_eq!(list["total"], 1);
    assert_eq!(list["files"][0]["items"][0]["continued"], json!(["more"]));
    let settings = crate::todo_patterns::TodoSettings { multiline: false, ..Default::default() };
    set_todo(&mut fixture, &settings);
    let list = fixture.run("todo.items.list", json!({}));
    assert!(list["files"][0]["items"][0].get("continued").is_none());
}

#[test]
fn the_ranges_of_a_document_carry_the_pattern_s_own_colour() {
    let file = ("a.go", "package a\n\n// TODO one\n//   more\n// hack two\n");
    let mut fixture = Fixture::new(&[file]);
    fixture.open();
    let path = fixture.root.join("a.go");
    fixture.run("file.document.open", json!({ "path": path }));
    let mut settings = crate::todo_patterns::TodoSettings::default();
    let mut hack = pattern(r"\bhack\b.*");
    hack.color = Some("#112233".to_owned());
    settings.patterns.push(hack);
    set_todo(&mut fixture, &settings);
    let ranges = fixture.run("todo.document.ranges", json!({}));
    let text = "package a\n\n// TODO one\n//   more\n// hack two\n";
    let at = |word: &str| text.find(word).unwrap() as u64;
    assert_eq!(ranges[0]["start"].as_u64(), Some(at("TODO")));
    assert_eq!(ranges[1]["start"].as_u64(), Some(at("more")), "the continuation is marked too");
    assert!(ranges[0].get("color").is_none());
    assert_eq!(ranges[2]["color"], "#112233");
}

#[test]
fn check_todo_finds_items_in_the_changed_lines_only() {
    isolate();
    let mut fixture = Fixture::new(&[("a.go", "package a\n\n// TODO old\n"), ("b.go", "package b\n")]);
    let root = fixture.root.clone();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "a.go"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    fixture.open();
    std::fs::write(root.join("a.go"), "package a\n\n// TODO old\n// FIXME new\n").unwrap();
    std::fs::write(root.join("b.go"), "package b\n\n// TODO fresh\n").unwrap();
    let found = fixture.run("todo.commit.check", json!({ "paths": [root.join("a.go"), root.join("b.go")] }));
    let found: Vec<(&str, u64)> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["text"].as_str().unwrap(), item["line"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        found,
        [("FIXME new", 3), ("TODO fresh", 2)],
        "an unchanged TODO does not count, a new file counts whole"
    );
}

#[test]
fn agents_read_no_todo_of_a_file_hidden_from_them() {
    let mut fixture = Fixture::new(&[("secret.go", "package a\n\n// TODO the password\n")]);
    let path = fixture.root.join("secret.go");
    fixture.open();
    fixture.run("file.document.open", json!({ "path": path }));
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret.go".to_owned()]);
    for (command, params) in
        [("todo.commit.check", json!({ "paths": [path] })), ("todo.document.ranges", json!({ "path": path }))]
    {
        let Fixture { registry, session, .. } = &mut fixture;
        let refused = zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params));
        match refused {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::hidden_files::REDACTED_CODE), "{command}");
            }
            other => panic!("{command}: {other:?}"),
        }
    }
    let found = fixture.run("todo.commit.check", json!({ "paths": [path] }));
    assert_eq!(found.as_array().unwrap().len(), 1, "the person's check is not limited");
}

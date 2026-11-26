use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::{ScratchLanguage, Scratches};
use crate::Workbench;

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
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let project = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("main.go"), "package main\n\nfunc main() {}\n").unwrap();
        std::fs::write(project.path().join("empty.txt"), "").unwrap();
        let scratches = session.state_mut::<Scratches>();
        scratches.set_data_dir(data.path().to_owned());
        scratches.set_languages(Arc::new(|| {
            vec![
                ScratchLanguage { id: "json".into(), name: "JSON".into(), extension: "json".into() },
                ScratchLanguage { id: "yaml".into(), name: "YAML".into(), extension: "yml".into() },
                ScratchLanguage { id: "gitlab".into(), name: "GitLab CI".into(), extension: String::new() },
            ]
        }));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        Self { registry, session, project, data }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn fails(&mut self, command: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err().to_string()
    }

    fn scratches(&self) -> PathBuf {
        self.data.path().join("scratches")
    }

    fn buffers(&self) -> PathBuf {
        self.data.path().join("buffers")
    }

    fn active(&self) -> PathBuf {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned()
    }

    fn is_open(&self, path: &Path) -> bool {
        self.session.state::<Workbench>().unwrap().documents().iter().any(|document| document.path() == path)
    }

    fn type_text(&mut self, text: &str) {
        self.run("editor.edit.type", json!({ "text": text }));
    }
}

fn ids(languages: &Value) -> Vec<String> {
    languages.as_array().unwrap().iter().map(|language| language["id"].as_str().unwrap().to_owned()).collect()
}

fn path(value: &Value) -> PathBuf {
    PathBuf::from(value.as_str().unwrap())
}

#[test]
fn the_languages_are_built_in_and_from_plug_ins_by_name_and_the_recent_ones_first() {
    let mut f = Fixture::new();
    let languages = f.run("file.scratch.languages", json!({}));
    assert_eq!(
        ids(&languages),
        ["go", "json", "text", "rust", "yaml"],
        "by name; a language without extension is left out"
    );
    assert_eq!(languages[3]["name"], "Rust");
    assert_eq!(languages[3]["extension"], "rs");
    f.run("file.scratch.new", json!({ "language": "yml" }));
    f.run("file.scratch.new", json!({ "language": "Rust" }));
    let languages = f.run("file.scratch.languages", json!({}));
    assert_eq!(ids(&languages), ["rust", "yaml", "go", "json", "text"]);
    assert_eq!(ids(&f.run("file.scratch.languages", json!({ "query": "JS" }))), ["json"]);
    assert_eq!(ids(&f.run("file.scratch.languages", json!({ "query": "rs" }))), ["rust"]);
    let memo: Value = serde_json::from_slice(&std::fs::read(f.data.path().join("scratches.json")).unwrap()).unwrap();
    assert_eq!(memo["recentLanguages"], json!(["rust", "yaml"]));
}

#[test]
fn a_new_scratch_starts_with_what_its_language_gives_and_the_caret_there() {
    let mut f = Fixture::new();
    let fake = Arc::new(zen_run::fake::FakeProvider::new());
    f.session.state_mut::<crate::run::Runs>().set_providers(vec![fake], zen_run::RunEnv::default());
    let started = path(&f.run("file.scratch.new", json!({ "language": "yaml" })));
    assert_eq!(std::fs::read_to_string(&started).unwrap(), "key: \n");
    let document = f.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.selections().primary().head, 5);
    let given = path(&f.run("file.scratch.new", json!({ "language": "yaml", "text": "x" })));
    assert_eq!(std::fs::read_to_string(&given).unwrap(), "x", "text given wins");
    let plain = path(&f.run("file.scratch.new", json!({ "language": "json" })));
    assert_eq!(std::fs::read_to_string(&plain).unwrap(), "", "a language without a starter starts empty");
}

#[test]
fn scratch_files_are_numbered_per_extension_in_the_data_folder_and_opened() {
    let mut f = Fixture::new();
    let first = path(&f.run("file.scratch.new", json!({ "language": "go" })));
    assert_eq!(first, f.scratches().join("scratch.go"));
    assert_eq!(f.active(), first);
    let second = path(&f.run("file.scratch.new", json!({ "language": "Go", "text": "package main\n" })));
    assert_eq!(second, f.scratches().join("scratch_1.go"));
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "package main\n");
    let json = path(&f.run("file.scratch.new", json!({ "language": ".json" })));
    assert_eq!(json, f.scratches().join("scratch.json"));
    let third = path(&f.run("file.scratch.new", json!({ "language": "go" })));
    assert_eq!(third, f.scratches().join("scratch_2.go"));
    let kotlin = path(&f.run("file.scratch.new", json!({ "language": "kt" })));
    assert_eq!(kotlin, f.scratches().join("scratch.kt"), "a plain extension is taken as it is");
    assert!(f.fails("file.scratch.new", json!({ "language": "no such/language" })).contains("no such/language"));
    let listed = f.run("file.scratch.list", json!({}));
    let names: Vec<&str> =
        listed["scratches"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["scratch.go", "scratch.json", "scratch.kt", "scratch_1.go", "scratch_2.go"]);
    assert_eq!(listed["scratches"][0]["language"], "go");
    assert_eq!(listed["folder"], json!(f.scratches()));
}

#[test]
fn a_scratch_file_is_made_from_the_selection_in_its_language() {
    let mut f = Fixture::new();
    let main = f.project.path().join("main.go");
    f.run("file.document.open", json!({ "path": main }));
    assert!(!f.registry.is_available(&f.session, "file.scratch.fromSelection"));
    assert!(
        f.registry.dispatch(&mut f.session, "editor.intentions.list", json!({})).is_err(),
        "no server, no selection"
    );
    f.run("editor.selection.set", json!({ "selections": [{ "anchor": 14, "head": 28 }] }));
    assert!(f.registry.is_available(&f.session, "file.scratch.fromSelection"));
    let listed = f.run("editor.intentions.list", json!({}));
    let action = listed["actions"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(action["id"], "scratch.fromSelection");
    assert_eq!(action["title"], zen_i18n::t!("workbench-scratch-from-selection"));
    let applied = f.run("editor.intentions.apply", json!({ "id": "scratch.fromSelection" }));
    let scratch = f.scratches().join("scratch.go");
    assert_eq!(applied["files"], json!([scratch]));
    assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "func main() {}");
    assert_eq!(f.active(), scratch);

    std::fs::write(f.project.path().join("notes"), "one two").unwrap();
    f.run("file.document.open", json!({ "path": f.project.path().join("notes") }));
    f.run("editor.selection.set", json!({ "selections": [{ "anchor": 0, "head": 3 }, { "anchor": 4, "head": 7 }] }));
    let text = path(&f.run("file.scratch.fromSelection", json!({})));
    assert_eq!(text, f.scratches().join("scratch.txt"), "a file without extension makes plain text");
    assert_eq!(std::fs::read_to_string(text).unwrap(), "one\ntwo");
}

#[test]
fn buffers_cycle_through_five_ask_before_clearing_and_skip_renamed_ones() {
    let mut f = Fixture::new();
    for slot in 1..=5 {
        let opened = f.run("file.scratch.newBuffer", json!({}));
        assert_eq!(path(&opened["path"]), f.buffers().join(format!("buffer{slot}.txt")));
        assert_eq!(opened["reused"], false);
        if slot == 1 {
            f.type_text("kept");
            f.run("file.document.save", json!({}));
        }
    }
    let asked = f.run("file.scratch.newBuffer", json!({}));
    assert_eq!(asked["path"], json!(f.buffers().join("buffer1.txt")));
    assert_eq!(asked["needsAnswer"], true);
    let kept = f.run("file.scratch.newBuffer", json!({ "clear": false }));
    assert_eq!((kept["reused"].clone(), kept["cleared"].clone()), (json!(true), json!(false)));
    assert_eq!(std::fs::read_to_string(f.buffers().join("buffer1.txt")).unwrap(), "kept");
    let next = f.run("file.scratch.newBuffer", json!({}));
    assert_eq!(next["path"], json!(f.buffers().join("buffer2.txt")));
    assert_eq!(next["needsAnswer"], false, "an empty buffer is reused without asking");

    f.run("file.document.open", json!({ "path": f.buffers().join("buffer3.txt") }));
    f.type_text("three");
    let asked = f.run("file.scratch.newBuffer", json!({}));
    assert_eq!(asked["needsAnswer"], true);
    let cleared = f.run("file.scratch.newBuffer", json!({ "clear": true }));
    assert_eq!(cleared["cleared"], true);
    assert_eq!(std::fs::read_to_string(f.buffers().join("buffer3.txt")).unwrap(), "");
    assert_eq!(f.session.state::<Workbench>().unwrap().active_document().unwrap().text(), "", "the open tab too");

    let buffer4 = f.buffers().join("buffer4.txt");
    let renamed = path(&f.run("file.fs.rename", json!({ "path": buffer4, "name": "notes.txt" })));
    assert_eq!(renamed, f.buffers().join("notes.txt"));
    let again = f.run("file.scratch.newBuffer", json!({}));
    assert_eq!(again["path"], json!(buffer4), "the free slot is made again");
    assert_eq!(again["reused"], false);
    assert!(renamed.exists());
    let after = f.run("file.scratch.newBuffer", json!({ "clear": false }));
    assert_eq!(after["path"], json!(f.buffers().join("buffer5.txt")), "the renamed buffer is never reused");
}

#[test]
fn closing_an_empty_scratch_file_deletes_it() {
    let mut f = Fixture::new();
    let empty = path(&f.run("file.scratch.new", json!({ "language": "go" })));
    f.run("file.document.close", json!({}));
    assert!(!empty.exists());
    assert!(!f.is_open(&empty));

    let typed = path(&f.run("file.scratch.new", json!({ "language": "go" })));
    f.type_text("package main");
    f.run("file.document.save", json!({}));
    f.run("file.document.close", json!({}));
    assert!(typed.exists(), "a scratch file with text is kept");

    let blank = path(&f.run("file.scratch.newBuffer", json!({}))["path"]);
    f.type_text("  \n");
    f.run("file.document.close", json!({}));
    assert!(!blank.exists(), "only white space is empty too");

    let project_file = f.project.path().join("empty.txt");
    f.run("file.document.open", json!({ "path": project_file }));
    f.run("file.document.close", json!({}));
    assert!(project_file.exists(), "files of the project are never deleted");
}

#[test]
fn closing_keeps_a_scratch_file_with_saved_text_or_open_in_another_window() {
    let mut f = Fixture::new();
    let saved = path(&f.run("file.scratch.new", json!({ "language": "txt", "text": "keep me\n" })));
    f.run("editor.selection.set", json!({ "selections": [{ "anchor": 0, "head": 7 }] }));
    f.type_text("kept");
    f.run("file.document.close", json!({}));
    assert_eq!(
        std::fs::read_to_string(&saved).unwrap(),
        "kept\n",
        "closing saves the edit, and a file with text stays"
    );

    f.session.state_mut::<Scratches>().set_open_elsewhere(Arc::new(|_: &Path| true));
    let shared = path(&f.run("file.scratch.new", json!({ "language": "txt" })));
    f.run("file.document.close", json!({}));
    assert!(shared.exists(), "another window still shows it");
    f.session.state_mut::<Scratches>().set_open_elsewhere(Arc::new(|_: &Path| false));
    f.run("file.document.open", json!({ "path": shared }));
    f.run("file.document.close", json!({}));
    assert!(!shared.exists());
}

#[test]
fn scratch_files_made_at_once_never_share_a_name() {
    let folder = tempfile::tempdir().unwrap();
    let made: Vec<PathBuf> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let folder = folder.path();
                scope.spawn(move || super::create_new(folder, "scratch", "json", &n.to_string()).unwrap())
            })
            .collect();
        workers.into_iter().map(|worker| worker.join().unwrap()).collect()
    });
    let mut texts: Vec<String> = made.iter().map(|path| std::fs::read_to_string(path).unwrap()).collect();
    texts.sort();
    assert_eq!(texts, (0..8).map(|n| n.to_string()).collect::<Vec<_>>());
}

#[cfg(unix)]
#[test]
fn a_link_in_the_scratches_folder_does_not_reach_files_outside_it() {
    let mut f = Fixture::new();
    f.run("file.scratch.new", json!({ "language": "go", "text": "package main\n" }));
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), f.scratches().join("link")).unwrap();
    let secret = f.scratches().join("link").join("secret.txt");
    assert!(f.fails("file.fs.rename", json!({ "path": secret, "name": "stolen.txt" })).contains("not a folder"));
    assert!(f.fails("file.fs.delete", json!({ "paths": [secret] })).contains("not a folder"));
    assert!(f.fails("file.scratch.move", json!({ "path": secret, "directory": "." })).contains("not a scratch file"));
    assert!(outside.path().join("secret.txt").exists());
    let renamed = path(&f.run("file.fs.rename", json!({ "path": f.scratches().join("link"), "name": "folder" })));
    assert!(renamed.is_symlink(), "the link itself is a scratch entry");
}

#[test]
fn scratch_files_are_renamed_deleted_moved_and_change_language() {
    let mut f = Fixture::new();
    let scratch = path(&f.run("file.scratch.new", json!({ "language": "json", "text": "{}" })));
    let renamed = path(&f.run("file.fs.rename", json!({ "path": scratch, "name": "config.json" })));
    assert_eq!(f.active(), renamed);
    let yaml = path(&f.run("file.scratch.changeLanguage", json!({ "path": renamed, "language": "YAML" })));
    assert_eq!(yaml, f.scratches().join("config.yml"));
    assert_eq!(f.active(), yaml, "the tab follows");
    std::fs::write(f.scratches().join("config.go"), "").unwrap();
    let go = path(&f.run("file.scratch.changeLanguage", json!({ "path": yaml, "language": "go" })));
    assert_eq!(go, f.scratches().join("config_1.go"), "a taken name gets a number");

    std::fs::create_dir(f.project.path().join("pkg")).unwrap();
    let moved = path(&f.run("file.scratch.move", json!({ "path": go, "directory": "pkg" })));
    assert_eq!(moved, f.project.path().join("pkg").join("config_1.go"));
    assert_eq!(f.active(), moved);
    assert!(f.fails("file.scratch.move", json!({ "path": moved, "directory": "." })).contains("not a scratch file"));
    let other = path(&f.run("file.scratch.new", json!({ "language": "go" })));
    let outside = f.data.path().join("elsewhere");
    std::fs::create_dir(&outside).unwrap();
    assert!(f.fails("file.scratch.move", json!({ "path": other, "directory": outside })).contains("not a folder"));

    let deleted = f.run("file.fs.delete", json!({ "paths": [f.scratches().join("config.go")] }));
    assert_eq!(deleted, json!([f.scratches().join("config.go")]));
    assert!(!f.scratches().join("config.go").exists());
    let data_file = f.data.path().join("scratches.json");
    assert!(f.fails("file.fs.delete", json!({ "paths": [data_file] })).contains("not a folder of the open project"));
    assert!(f.fails("file.fs.rename", json!({ "path": f.scratches(), "name": "x" })).contains("not a folder"));
}

#[test]
fn the_scratches_scope_also_holds_the_buffers() {
    let mut f = Fixture::new();
    f.run("file.scratch.new", json!({ "language": "go", "text": "package main // needle\n" }));
    f.run("file.scratch.newBuffer", json!({}));
    f.type_text("needle");
    f.run("file.document.save", json!({}));
    let folder = f.scratches();
    f.session.state_mut::<crate::search_scopes::SearchScopes>().set_scratches(folder);
    let files = f.run("search.scope.files", json!({ "scope": "scratches" }));
    let mut names: Vec<String> =
        files["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_owned()).collect();
    names.sort();
    assert_eq!(names, ["Scratches and Consoles/Buffers/buffer1.txt", "Scratches and Consoles/scratch.go"]);
}

struct GoModule;

impl zen_project::model::ModelProvider for GoModule {
    fn id(&self) -> &str {
        "go-module"
    }

    fn watches(&self, _: &str) -> bool {
        false
    }

    fn load(&self, root: &Path, _: &zen_project::model::ModelEnv) -> Option<zen_project::model::ProjectModel> {
        let module = zen_project::model::Module {
            name: "example.com/shop".to_owned(),
            root: root.to_owned(),
            ..Default::default()
        };
        root.join("go.mod")
            .exists()
            .then(|| zen_project::model::ProjectModel { modules: vec![module], ..Default::default() })
    }
}

const PLAIN: &str = "package main\n\nfunc main() {}\n";
const IMPORT: &str = "import \"example.com/shop/model\"\n\n";

struct Homed {
    fixture: crate::language::testing::Fixture,
    scratch: PathBuf,
    home: PathBuf,
    _data: tempfile::TempDir,
}

fn homed(with_module: bool) -> Homed {
    let files: &[(&str, &str)] = if with_module { &[("go.mod", "module example.com/shop\n")] } else { &[] };
    let mut fixture = crate::language::testing::fixture(files);
    let data = tempfile::tempdir().unwrap();
    fixture.session.state_mut::<Scratches>().set_data_dir(data.path().to_owned());
    let scratch = data.path().join("scratches/scratch.go");
    std::fs::create_dir_all(scratch.parent().unwrap()).unwrap();
    std::fs::write(&scratch, PLAIN).unwrap();
    let home = fixture.path("zen_scratch/scratch/scratch.go");
    let provider = zen_run::fake::FakeProvider::new();
    provider.set_scratch_home("example.com/shop/model", home.clone());
    fixture.session.state_mut::<crate::run::Runs>().set_providers(vec![Arc::new(provider)], zen_run::RunEnv::default());
    let models = fixture.session.state_mut::<crate::project_model::ProjectModels>();
    models.set_providers(vec![Arc::new(GoModule)], zen_project::model::ModelEnv::default());
    Homed { fixture, scratch, home, _data: data }
}

impl Homed {
    fn open(&mut self) {
        let scratch = self.scratch.to_str().unwrap().to_owned();
        self.fixture.open(&scratch);
    }

    fn set_text(&mut self, text: &str) {
        let document = self.fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        let whole = 0..document.text().chars().count();
        assert!(document.replace_ranges(vec![(whole, text.to_owned())]));
    }

    fn sync_until_opened(&mut self, count: usize) -> Vec<Value> {
        self.fixture.dispatch("language.servers.sync", json!({})).unwrap();
        self.fixture.server.wait_for("textDocument/didOpen", count, std::time::Duration::from_secs(10))
    }
}

#[test]
fn a_scratch_moves_to_its_home_in_the_module_when_it_imports_from_it_and_back_when_it_stops() {
    let mut f = homed(true);
    f.open();
    let plain = zen_lsp::position::file_uri(&f.scratch);
    let at_home = zen_lsp::position::file_uri(&f.home);
    assert_ne!(plain, at_home);
    assert_eq!(f.sync_until_opened(1)[0]["textDocument"]["uri"], plain.as_str());

    let imports = format!("package main\n\n{IMPORT}func main() {{}}\n");
    f.set_text(&imports);
    let opened = f.sync_until_opened(2);
    assert_eq!(opened[1]["textDocument"]["uri"], at_home.as_str());
    assert_eq!(opened[1]["textDocument"]["text"], imports.as_str(), "the server gets the text being edited");
    let closed = f.fixture.server.wait_for("textDocument/didClose", 1, std::time::Duration::from_secs(10));
    assert_eq!(closed[0]["textDocument"]["uri"], plain.as_str());
    assert_eq!(zen_lsp::position::uri_path(&at_home).as_deref(), Some(f.scratch.as_path()));

    f.set_text(PLAIN);
    assert_eq!(f.sync_until_opened(3)[2]["textDocument"]["uri"], plain.as_str());
    assert_eq!(zen_lsp::position::file_uri(&f.scratch), plain);
    assert!(!f.home.exists(), "nothing is written to the project");
}

#[test]
fn problems_reported_at_the_home_show_on_the_scratch() {
    let mut f = homed(true);
    std::fs::write(&f.scratch, format!("package main\n\n{IMPORT}func main() {{}}\n")).unwrap();
    f.fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", |opened| {
        let range = json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 7 } });
        let uri = opened["textDocument"]["uri"].clone();
        json!({ "uri": uri, "diagnostics": [{ "range": range, "message": "at home", "severity": 1 }] })
    });
    f.open();
    let at_home = zen_lsp::position::file_uri(&f.home);
    assert_eq!(f.sync_until_opened(1)[0]["textDocument"]["uri"], at_home.as_str());
    let scratch = f.scratch.clone();
    let problems =
        |f: &Homed| f.fixture.session.state::<crate::language::Languages>().unwrap().problems(&scratch).len();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while problems(&f) == 0 {
        assert!(std::time::Instant::now() < deadline, "the problem never reached the scratch");
        f.fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }
}

#[test]
fn a_scratch_gets_its_home_when_the_module_appears_and_loses_it_when_closed() {
    let mut f = homed(false);
    std::fs::write(&f.scratch, format!("package main\n\n{IMPORT}func main() {{}}\n")).unwrap();
    f.open();
    let plain = zen_lsp::position::file_uri(&f.scratch);
    assert_eq!(f.sync_until_opened(1)[0]["textDocument"]["uri"], plain.as_str());

    std::fs::write(f.fixture.path("go.mod"), "module example.com/shop\n").unwrap();
    let root = f.fixture.path("go.mod").parent().unwrap().to_owned();
    f.fixture.session.state_mut::<crate::project_model::ProjectModels>().refresh(Some(&root));
    let at_home = zen_lsp::position::file_uri(&f.home);
    assert_eq!(f.sync_until_opened(2)[1]["textDocument"]["uri"], at_home.as_str());

    f.fixture.dispatch("file.document.close", json!({})).unwrap();
    f.fixture.dispatch("language.servers.sync", json!({})).unwrap();
    assert_eq!(zen_lsp::position::file_uri(&f.scratch), plain);
    let closed = f.fixture.server.wait_for("textDocument/didClose", 2, std::time::Duration::from_secs(10));
    assert_eq!(closed[1]["textDocument"]["uri"], at_home.as_str());
}

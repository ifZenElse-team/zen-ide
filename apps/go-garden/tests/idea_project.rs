use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_lsp::fake::FakeServer;
use zen_workbench::guard::{refusal, refuses_foreign_settings};
use zen_workbench::language::format::REFORMAT_ON_SAVE;
use zen_workbench::language::{Languages, ServerSpec};
use zen_workbench::project_settings::ProjectSettingsState;

const WAIT: Duration = Duration::from_secs(10);

type Snapshot = BTreeMap<PathBuf, Option<(Vec<u8>, SystemTime)>>;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/platform/jb-import/tests/fixtures/idea-project")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn snapshot(root: &Path) -> Snapshot {
    fn walk(root: &Path, dir: &Path, into: &mut Snapshot) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                if refuses_foreign_settings(&relative) {
                    into.insert(relative.clone(), None);
                }
                walk(root, &path, into);
            } else if refuses_foreign_settings(&relative) {
                let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
                into.insert(relative, Some((std::fs::read(&path).unwrap(), modified)));
            }
        }
    }
    let mut snapshot = Snapshot::new();
    walk(root, root, &mut snapshot);
    snapshot
}

struct Window {
    registry: Registry,
    session: Session,
    server: FakeServer,
    root: PathBuf,
}

impl Window {
    fn open(root: &Path) -> Self {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        session
            .state_mut::<ProjectSettingsState>()
            .set_providers(vec![Arc::new(zen_jb_import::IdeaProject::with_user_home(None))]);
        let server = FakeServer::new();
        server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
            "renameProvider": true,
            "documentFormattingProvider": true
        }));
        server.on("textDocument/formatting", json!([{ "range": range(0, 0, 0), "newText": "// formatted\n" }]));
        let launched = server.clone();
        session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(move |_| Ok(launched.launcher())),
            initialization_options: json!({}),
        }]);
        session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        session
            .state_mut::<zen_workbench::run::Runs>()
            .set_providers(vec![Arc::new(zen_lang_go::GoRun)], zen_run::RunEnv::default());
        let mut window = Self { registry, session, server, root: root.to_owned() };
        window.run("file.project.open", json!({ "path": root }));
        window
    }

    fn dispatch(&mut self, id: &str, params: Value) -> Result<Value, DispatchError> {
        self.registry.dispatch(&mut self.session, id, params)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.dispatch(id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn uri(&self, name: &str) -> String {
        zen_lsp::position::file_uri(&self.path(name))
    }

    fn open_synced(&mut self, name: &str) {
        let path = self.path(name);
        self.run("file.document.open", json!({ "path": path }));
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            self.run("language.servers.sync", json!({}));
            if self.session.state::<Languages>().is_some_and(|languages| languages.synced_text(&path).is_some()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the server did not get {name}");
    }

    fn rename(&mut self, also: Option<&str>) -> Result<Value, DispatchError> {
        let mut changes = vec![
            json!({ "textDocument": { "uri": self.uri("main.go"), "version": null }, "edits": [{ "range": range(2, 14, 5), "newText": "hello" }] }),
            json!({ "textDocument": { "uri": self.uri("lib.go"), "version": null }, "edits": [{ "range": range(2, 5, 5), "newText": "hello" }] }),
        ];
        if let Some(name) = also {
            changes.push(json!({ "textDocument": { "uri": self.uri(name), "version": null }, "edits": [{ "range": range(0, 0, 0), "newText": "<!-- hello -->" }] }));
        }
        self.server.on("textDocument/rename", json!({ "documentChanges": changes }));
        let offset = "package main\n\nfunc main() { ".chars().count();
        let path = self.path("main.go");
        self.dispatch(
            "refactor.rename.apply",
            json!({ "path": path, "offset": offset, "newName": "hello", "waitMs": 5000 }),
        )
    }
}

fn range(line: u64, start: u64, length: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": start + length } })
}

fn failure(result: Result<Value, DispatchError>) -> String {
    match result {
        Err(DispatchError::Failed { error, .. }) => error.message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn opening_editing_formatting_and_renaming_leave_the_idea_files_alone() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orchard");
    copy(&fixture(), &root);
    let before = snapshot(&root);
    assert_eq!(before.values().filter(|entry| entry.is_some()).count(), 13, "{:?}", before.keys());

    let mut window = Window::open(&root);
    let got = window.run("project.settings.get", json!({}));
    assert_eq!(got["revision"], 1);
    assert_eq!(got["settings"]["sources"].as_array().unwrap().len(), 12);
    assert_eq!(got["settings"]["problems"], json!([]));
    assert_eq!(window.run("project.settings.refresh", json!({}))["changed"], false);
    let watched = json!({ "paths": [root.join(".idea/scopes/Generated.xml")] });
    assert_eq!(window.run("project.settings.refresh", watched), json!({ "revision": 1, "changed": false }));

    window.open_synced("main.go");
    window.run("file.document.open", json!({ "path": root.join("orchard.iml") }));
    window.run("file.document.open", json!({ "path": root.join(".idea/codeStyles/Project.xml") }));
    window.open_synced("main.go");

    let refused = failure(window.rename(Some("orchard.iml")));
    assert_eq!(refused, refusal(&root.join("orchard.iml")));
    let refused = failure(window.rename(Some(".run/Unit_tests.run.xml")));
    assert_eq!(refused, refusal(&root.join(".run/Unit_tests.run.xml")));
    assert_eq!(std::fs::read_to_string(root.join("lib.go")).unwrap(), "package main\n\nfunc greet(name string) {}\n");
    let renamed = window.rename(None).unwrap();
    assert_eq!(renamed["edits"], 2);
    assert_eq!(std::fs::read_to_string(root.join("lib.go")).unwrap(), "package main\n\nfunc hello(name string) {}\n");

    window.run("file.document.saveAll", json!({}));
    let main = std::fs::read_to_string(root.join("main.go")).unwrap();
    assert!(main.starts_with("// formatted\n") && main.contains("hello(\"orchard\")"), "{main}");
    window.run("editor.code.reformat", json!({ "path": root.join("main.go"), "waitMs": 5000 }));
    window.run("file.document.saveAll", json!({}));
    window.run("file.document.save", json!({}));

    let replaced = window.dispatch(
        "search.text.replaceAll",
        json!({ "query": { "text": "ORCHARD_MODE", "matchCase": true }, "replacement": "MODE" }),
    );
    assert_eq!(failure(replaced), refusal(&root.join(".run/Unit_tests.run.xml")));
    window.run("project.settings.refresh", json!({ "paths": [root.join("orchard.iml")] }));
    edit_the_run_configurations(&mut window);

    assert_eq!(snapshot(&root), before, "the .idea files are byte-identical and were not rewritten");
}

fn edit_the_run_configurations(window: &mut Window) {
    let serve = "jb:.idea/runConfigurations/Serve.xml#Serve";
    let tests = "jb:.run/Unit_tests.run.xml#Unit tests";
    let listed = window.run("run.configuration.list", json!({}));
    let ids: Vec<&str> =
        listed["configurations"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [serve, tests]);
    let adopted = &listed["configurations"][0];
    assert_eq!((adopted["kind"].as_str(), adopted["name"].as_str()), (Some("go.build"), Some("Serve")));
    assert_eq!(adopted["options"]["package"], "example.org/orchard");
    assert_eq!(adopted["options"]["files"], "main.go");
    assert_eq!(adopted["common"]["programArguments"], "--listen :7311");
    assert_eq!(adopted["common"]["env"], json!([["ORCHARD_MODE", "dev"], ["ORCHARD_SEED", "17"]]));
    assert_eq!(listed["configurations"][1]["options"]["testKind"], "directory");
    window.run("run.configuration.select", json!({ "id": tests }));
    let mut changed = listed["configurations"][0].clone();
    changed["name"] = json!("Serve again");
    let refused = failure(window.dispatch("run.configuration.update", json!({ "configuration": changed })));
    assert!(refused.contains("Serve"), "{refused}");
    failure(window.dispatch("run.configuration.delete", json!({ "id": serve })));
    let saved = window.run("run.configuration.saveAdopted", json!({ "id": serve }));
    let mut copy = saved.clone();
    copy["common"]["programArguments"] = json!("--listen :7312");
    window.run("run.configuration.update", json!({ "configuration": copy }));
    let created = window.run("run.configuration.create", json!({ "configuration": listed["configurations"][1] }));
    window.run("run.configuration.delete", json!({ "id": created["id"] }));
    let listed = window.run("run.configuration.list", json!({}));
    let ids: Vec<&str> =
        listed["configurations"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["go.build-1", tests]);
    assert_eq!(listed["selected"], tests);
    assert!(window.root.join(".zen/runConfigurations.json").exists());
}

#[test]
fn the_user_can_still_edit_an_idea_file_explicitly() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orchard");
    copy(&fixture(), &root);
    let mut window = Window::open(&root);
    let file = root.join("tools/tools.iml");
    window.run("file.document.open", json!({ "path": file }));
    let workbench = window.session.state_mut::<zen_workbench::Workbench>();
    let document = workbench.active_document_mut().unwrap();
    let end = document.text().chars().count();
    document.replace_ranges(vec![(end..end, "<!-- mine -->\n".to_owned())]);
    window.run("file.document.save", json!({}));
    assert!(std::fs::read_to_string(&file).unwrap().ends_with("<!-- mine -->\n"));
    let refreshed = window.run("project.settings.refresh", json!({ "paths": [file] }));
    assert_eq!(refreshed["revision"], 1, "the sources did not change");
}

#[test]
fn a_project_without_idea_files_gets_none() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("plain");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("go.mod"), "module example.org/plain\n\ngo 1.23\n").unwrap();
    std::fs::write(root.join("main.go"), "package main\n\nfunc main() { greet(\"orchard\") }\n").unwrap();
    std::fs::write(root.join("lib.go"), "package main\n\nfunc greet(name string) {}\n").unwrap();

    let mut window = Window::open(&root);
    let got = window.run("project.settings.get", json!({}));
    assert_eq!(got["revision"], 0);
    assert_eq!(got["settings"]["sources"], json!([]));
    window.open_synced("main.go");
    window.rename(None).unwrap();
    window.run("file.document.saveAll", json!({}));
    window.run("editor.code.reformat", json!({ "waitMs": 5000 }));
    window.run("file.document.saveAll", json!({}));
    window.run("project.settings.refresh", json!({}));

    assert!(snapshot(&root).is_empty(), "{:?}", snapshot(&root).keys());
    assert!(!root.join(".idea").exists() && !root.join(".run").exists());
}

#[test]
fn the_shared_run_configurations_are_exposed_to_agents() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orchard");
    copy(&fixture(), &root);
    let mut window = Window::open(&root);
    let got = window.run("project.settings.get", json!({}));
    let configurations = got["settings"]["runConfigurations"].as_array().unwrap();
    let names: Vec<&str> = configurations.iter().map(|configuration| configuration["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Serve", "Unit tests"]);
    assert_eq!(configurations[0]["typeId"], "GoApplicationRunConfiguration");
    assert_eq!(configurations[0]["options"]["package"], "example.org/orchard");
    assert_eq!(configurations[0]["env"], json!([["ORCHARD_MODE", "dev"], ["ORCHARD_SEED", "17"]]));
    assert_eq!(configurations[1]["shared"], true);
}

#[test]
fn the_project_code_style_and_encodings_are_obeyed() {
    zen_i18n::register(zen_workbench::LOCALES);
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orchard");
    copy(&fixture(), &root);
    let before = snapshot(&root);
    let mut window = Window::open(&root);

    window.open_synced("main.go");
    window.run("editor.code.reformat", json!({ "path": root.join("main.go"), "waitMs": 5000 }));
    let sent = window.server.requests("textDocument/formatting");
    assert_eq!(sent[0]["options"], json!({ "tabSize": 6, "insertSpaces": false }), "Project.xml sets go");

    let legacy = root.join("legacy/greeting.txt");
    let bytes = std::fs::read(&legacy).unwrap();
    window.run("file.document.open", json!({ "path": legacy }));
    let workbench = window.session.state_mut::<zen_workbench::Workbench>();
    let document = workbench.active_document_mut().unwrap();
    assert_eq!(document.read_only(), None, "a windows-1251 file opens as text");
    assert_eq!(document.charset(), "windows-1251");
    window.run("file.document.saveAll", json!({}));
    assert_eq!(std::fs::read(&legacy).unwrap(), bytes, "the file keeps its bytes");
    assert_eq!(snapshot(&root), before);
}

fn go_project(root: &Path, idea_indent: Option<u8>) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("go.mod"), "module example.org/styled\n\ngo 1.23\n").unwrap();
    std::fs::write(root.join("main.go"), "package main\n\nfunc main() {}\n").unwrap();
    let Some(indent) = idea_indent else { return };
    let styles = root.join(".idea/codeStyles");
    std::fs::create_dir_all(&styles).unwrap();
    std::fs::write(
        styles.join("codeStyleConfig.xml"),
        "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\"><state>\
         <option name=\"USE_PER_PROJECT_SETTINGS\" value=\"true\" /></state></component>\n",
    )
    .unwrap();
    std::fs::write(
        styles.join("Project.xml"),
        format!(
            "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\">\
             <code_scheme name=\"Project\" version=\"173\"><codeStyleSettings language=\"go\"><indentOptions>\
             <option name=\"USE_TAB_CHARACTER\" value=\"false\" /><option name=\"INDENT_SIZE\" value=\"{indent}\" />\
             <option name=\"TAB_SIZE\" value=\"3\" /></indentOptions></codeStyleSettings></code_scheme></component>\n"
        ),
    )
    .unwrap();
}

fn type_enter_and_tab(window: &mut Window) -> String {
    let path = window.path("main.go");
    let brace = "package main\n\nfunc main() {".chars().count();
    window.run("editor.selection.set", json!({ "selections": [{ "anchor": brace, "head": brace }] }));
    window.run("editor.edit.newline", json!({}));
    window.run("editor.edit.tab", json!({}));
    let workbench = window.session.state::<zen_workbench::Workbench>().unwrap();
    let document = workbench.documents().iter().find(|document| document.path() == path).unwrap();
    document.text().to_owned()
}

#[test]
fn the_project_code_style_goes_over_the_users_for_enter_tab_and_reformat() {
    let temp = tempfile::tempdir().unwrap();
    let mut styles = BTreeMap::new();
    styles.insert("go".to_owned(), zen_workbench::code_style::CodeStyle { use_tabs: false, tab_size: 8, indent: 2 });

    let shared = temp.path().join("shared");
    go_project(&shared, Some(6));
    let mut window = Window::open(&shared);
    zen_workbench::code_style::apply(window.session.settings_mut(), &styles);
    window.open_synced("main.go");
    let six = " ".repeat(6);
    assert_eq!(type_enter_and_tab(&mut window), format!("package main\n\nfunc main() {{\n{six}{six}\n}}\n"));
    let style = zen_workbench::project_settings::code_style_for_path(&window.session, &window.path("main.go"));
    assert_eq!((style.tab_size, style.indent, style.use_tabs), (3, 6, false), "the editor draws tabs 3 wide");
    window.run("editor.code.reformat", json!({ "waitMs": 5000 }));
    let sent = window.server.requests("textDocument/formatting");
    assert_eq!(sent[0]["options"], json!({ "tabSize": 6, "insertSpaces": true }));

    let plain = temp.path().join("plain");
    go_project(&plain, None);
    let mut window = Window::open(&plain);
    zen_workbench::code_style::apply(window.session.settings_mut(), &styles);
    window.open_synced("main.go");
    assert_eq!(type_enter_and_tab(&mut window), "package main\n\nfunc main() {\n    \n}\n");
    let style = zen_workbench::project_settings::code_style_for_path(&window.session, &window.path("main.go"));
    assert_eq!(style.tab_size, 8);
    window.run("editor.code.reformat", json!({ "waitMs": 5000 }));
    let sent = window.server.requests("textDocument/formatting");
    assert_eq!(sent[0]["options"], json!({ "tabSize": 2, "insertSpaces": true }), "the user's code style");
}

fn styled_files_project(root: &Path) {
    std::fs::create_dir_all(root.join(".idea/codeStyles")).unwrap();
    std::fs::write(root.join("notes.txt"), "first\n").unwrap();
    std::fs::write(root.join("data.zzz"), "first\n").unwrap();
    std::fs::write(root.join("main.go"), "package main\n\nfunc main() {}\n").unwrap();
    std::fs::write(
        root.join(".idea/codeStyles/codeStyleConfig.xml"),
        "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\"><state>\
         <option name=\"USE_PER_PROJECT_SETTINGS\" value=\"true\" /></state></component>\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".idea/codeStyles/Project.xml"),
        "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\">\
         <code_scheme name=\"Project\" version=\"173\">\
         <option name=\"OTHER_INDENT_OPTIONS\"><value><option name=\"INDENT_SIZE\" value=\"3\" />\
         <option name=\"USE_TAB_CHARACTER\" value=\"false\" /></value></option>\
         <codeStyleSettings language=\"TEXT\"><indentOptions>\
         <option name=\"USE_TAB_CHARACTER\" value=\"true\" /></indentOptions></codeStyleSettings>\
         </code_scheme></component>\n",
    )
    .unwrap();
}

fn tab_enter_tab(window: &mut Window, name: &str) -> String {
    let path = window.path(name);
    window.run("file.document.open", json!({ "path": path }));
    window.run("editor.selection.set", json!({ "selections": [{ "anchor": 0, "head": 0 }] }));
    window.run("editor.edit.tab", json!({}));
    let end = window
        .session
        .state::<zen_workbench::Workbench>()
        .unwrap()
        .active_document()
        .unwrap()
        .text()
        .find('\n')
        .unwrap();
    window.run("editor.selection.set", json!({ "selections": [{ "anchor": end, "head": end }] }));
    window.run("editor.edit.newline", json!({}));
    window.run("editor.edit.tab", json!({}));
    let workbench = window.session.state::<zen_workbench::Workbench>().unwrap();
    workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
}

#[test]
fn plain_text_takes_the_text_style_and_other_files_the_other_indent_options() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("styled");
    styled_files_project(&root);
    let mut window = Window::open(&root);
    let mut styles = BTreeMap::new();
    styles.insert(
        zen_workbench::code_style::DEFAULT_LANGUAGE.to_owned(),
        zen_workbench::code_style::CodeStyle { use_tabs: false, tab_size: 4, indent: 4 },
    );
    zen_workbench::code_style::apply(window.session.settings_mut(), &styles);

    assert_eq!(tab_enter_tab(&mut window, "notes.txt"), "\tfirst\n\t\t\n", "TEXT asks for tabs");
    assert_eq!(tab_enter_tab(&mut window, "data.zzz"), "   first\n      \n", "OTHER_INDENT_OPTIONS: 3 spaces");

    window.open_synced("main.go");
    assert_eq!(type_enter_and_tab(&mut window), "package main\n\nfunc main() {\n\t\t\n}\n", "Go keeps its tabs");
}

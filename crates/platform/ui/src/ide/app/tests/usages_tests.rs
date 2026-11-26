use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;

const MAIN: &str = "package main\n\nimport \"app/store\"\n\nvar Version = \"1\"\n\nfunc main() {\n\tVersion = \"2\"\n\tprintln(Version, Version)\n\tstore.Save(Version)\n}\n";
const STORE: &str =
    "package store\n\n// Save keeps the version.\nfunc Save(v string) { last = v }\n\nvar last string\n";
const TEST: &str = "package main\n\nimport \"testing\"\n\nfunc TestVersion(t *testing.T) { _ = Version }\n";

fn at(text: &str, nth: usize) -> usize {
    text[..text.match_indices("Version").nth(nth).unwrap().0].chars().count()
}

fn range(text: &str, nth: usize) -> Value {
    let start = text.match_indices("Version").nth(nth).unwrap().0;
    json!({
        "start": zen_lsp::position::position(text, start),
        "end": zen_lsp::position::position(text, start + "Version".len()),
    })
}

fn gopls() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let mut candidates = vec![home.join("go/bin/gopls")];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|folder| folder.join("gopls")));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn fake(project: &Path) -> zen_lsp::fake::FakeServer {
    let server = zen_lsp::fake::FakeServer::new();
    server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "referencesProvider": true,
        "documentSymbolProvider": true,
        "documentHighlightProvider": true,
    }));
    let main = zen_lsp::position::file_uri(&project.join("main.go"));
    let test = zen_lsp::position::file_uri(&project.join("main_test.go"));
    let location = |uri: &str, text: &str, nth: usize| json!({ "uri": uri, "range": range(text, nth) });
    server.on(
        "textDocument/references",
        json!([
            location(&main, MAIN, 1),
            location(&main, MAIN, 2),
            location(&main, MAIN, 3),
            location(&main, MAIN, 4),
            location(&test, TEST, 1),
        ]),
    );
    server.on(
        "textDocument/documentSymbol",
        json!([{ "name": "Version", "kind": 13, "range": range(MAIN, 0), "selectionRange": range(MAIN, 0) }]),
    );
    let main_uri = main.clone();
    server.on_with("textDocument/documentHighlight", move |params| {
        if params["textDocument"]["uri"] == main_uri.as_str() {
            let kinds = [(0, 3), (1, 3), (2, 2), (3, 2), (4, 2)];
            json!(
                kinds.iter().map(|(nth, kind)| json!({ "range": range(MAIN, *nth), "kind": kind })).collect::<Vec<_>>()
            )
        } else {
            json!([{ "range": range(TEST, 1), "kind": 1 }])
        }
    });
    server
}

struct Tour {
    screen: crate::offscreen::Offscreen,
    app: IdeApp,
    project: PathBuf,
    snapshots: Option<PathBuf>,
    prefix: &'static str,
    _data: tempfile::TempDir,
}

impl Tour {
    fn new(real: Option<PathBuf>, prefix: &'static str) -> (Self, Option<zen_lsp::fake::FakeServer>) {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        for (name, text) in [
            ("go.mod", "module app\n\ngo 1.22\n"),
            ("main.go", MAIN),
            ("store/store.go", STORE),
            ("main_test.go", TEST),
        ] {
            let path = project.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let app = IdeApp::new(test_product(), registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let fake = real.is_none().then(|| fake(&project));
        let launch: zen_workbench::language::Launch = match (&real, &fake) {
            (Some(gopls), _) => {
                let (gopls, folder) = (gopls.clone(), project.clone());
                Arc::new(move |_| Ok(zen_lsp::process_launcher(gopls.clone(), Vec::new(), folder.clone(), Vec::new())))
            }
            (None, Some(server)) => {
                let server = server.clone();
                Arc::new(move |_| Ok(server.launcher()))
            }
            (None, None) => unreachable!(),
        };
        app.slot_for(None)
            .unwrap()
            .window
            .lock()
            .unwrap()
            .session
            .state_mut::<zen_workbench::language::Languages>()
            .set_specs(vec![zen_workbench::language::ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch,
                initialization_options: json!({ "semanticTokens": true }),
            }]);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let screen = crate::offscreen::Offscreen::new(egui::vec2(1280.0, 1000.0), 2.0);
        let mut tour = Self { screen, app, project, snapshots, prefix, _data: data };
        for _ in 0..60 {
            tour.step();
        }
        (tour, fake)
    }

    fn step(&mut self) {
        screen_step(&mut self.screen, &mut self.app, Vec::new());
    }

    fn command(&mut self, command: &str, params: Value) -> Value {
        screen_ask(&mut self.screen, &mut self.app, "dispatch_command", json!({ "command": command, "params": params }))
            .unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn state(&mut self) -> Value {
        screen_ask(&mut self.screen, &mut self.app, "get_window_state", json!({})).unwrap()
    }

    fn keys(&mut self, keys: &[&str]) {
        screen_ask(&mut self.screen, &mut self.app, "press_keys", json!({ "keys": keys })).unwrap();
        self.step();
    }

    fn until(&mut self, what: &str, done: &dyn Fn(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !done(self) {
            assert!(Instant::now() < deadline, "{what} never happened: {}", self.state());
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        self.step();
    }

    fn click(&mut self, label: &str) {
        screen_wait(&mut self.screen, &mut self.app, Duration::from_millis(60));
        let rect = node_when(&mut self.screen, &mut self.app, label).unwrap_or_else(|| panic!("no {label} on screen"));
        let at = rect.center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in
            [vec![egui::Event::PointerMoved(at)], Vec::new(), vec![button(true)], vec![button(false)], Vec::new()]
        {
            screen_step(&mut self.screen, &mut self.app, events);
        }
    }

    fn tool(&mut self, label: &str) {
        for _ in 0..3 {
            self.step();
        }
        if node_at(&mut self.screen, &mut self.app, label).is_none() {
            self.click(&zen_i18n::t!("ide-find-more"));
        }
        self.click(label);
    }

    fn option(&mut self, name: &str) -> Value {
        self.command("search.results.list", json!({}))["options"][name].clone()
    }

    fn toggle(&mut self, label: &str, option: &str) {
        let before = self.option(option);
        for _ in 0..2 {
            self.tool(label);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if self.option(option) != before {
                    return;
                }
                self.step();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        panic!("{label} did not turn {option} over");
    }

    fn snapshot(&mut self, name: &str) {
        let Some(dir) = self.snapshots.clone() else { return };
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            for _ in 0..3 {
                self.step();
            }
            let image = self.screen.render(|ui| {
                self.app.logic(ui.ctx());
                self.app.ui(ui);
            });
            let path = dir.join(format!("{}-{name}-{language}.png", self.prefix));
            crate::offscreen::write_png(&path, &image).unwrap();
        }
        zen_i18n::set_language(Some("en"));
        self.step();
    }

    fn tree(&mut self) -> Value {
        self.command("search.results.tree", json!({ "waitMs": 5000 }))
    }

    fn groups(&mut self) -> Vec<String> {
        let tree = self.tree();
        tree["nodes"][1]["children"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|node| node["label"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    fn caret(&mut self, nth: usize) {
        self.keys(&["⎋"]);
        self.command("file.document.open", json!({ "path": self.project.join("main.go") }));
        self.command("editor.selection.set", json!({ "selections": [{ "head": at(MAIN, nth) }] }));
        for _ in 0..3 {
            self.step();
        }
    }

    fn find_usages(&mut self, key: bool) -> Value {
        let before = self.command("search.results.list", json!({}));
        if key {
            self.keys(&["⌥F7"]);
        } else {
            let action = json!({ "action": "navigate.usages.find" });
            screen_ask(&mut self.screen, &mut self.app, "run_action", action).unwrap();
        }
        let what = format!("the usages tab after {before}");
        self.until(&what, &|tour| {
            let usages = tour.command("navigate.usages.list", json!({}));
            let list = tour.command("search.results.list", json!({}));
            !usages["pending"].as_bool().unwrap_or(true) && list != before && list["active"].is_u64()
        });
        self.until("the Find tool window", &|tour| tour.state()["bottom"] == "find");
        self.tree()
    }
}

fn tour(real: Option<PathBuf>, prefix: &'static str) {
    let (mut tour, fake) = Tour::new(real.clone(), prefix);
    tour.caret(0);
    if real.is_some() {
        tour.until("gopls answers", &|tour| {
            let found = tour.command("navigate.usages.find", json!({ "waitMs": 3000 }));
            found["total"].as_u64().unwrap_or(0) >= 5
        });
        tour.caret(0);
    }

    let tree = tour.find_usages(true);
    let taller =
        json!({ "action": "view.toolWindow.resize/up", "params": { "id": "find", "direction": "up", "by": 260 } });
    screen_ask(&mut tour.screen, &mut tour.app, "run_action", taller).unwrap();
    screen_wait(&mut tour.screen, &mut tour.app, Duration::from_millis(500));
    assert_eq!(
        (&tree["title"], &tree["kind"], &tree["total"]),
        (&json!("Usages of Version"), &json!("usages"), &json!(5)),
        "{tree}"
    );
    assert_eq!(tree["accessKnown"], true, "the server tells reads from writes: {tree}");
    assert_eq!(tree["nodes"][0]["children"][0]["label"], "Variable Version");
    assert_eq!(tree["nodes"][1]["label"], "Usages in Project Files");
    let groups = tour.groups();
    assert!(groups.contains(&"Value read".to_owned()) && groups.contains(&"Value write".to_owned()), "{groups:?}");
    tour.snapshot("usages");

    tour.click("Group By");
    tour.snapshot("usages-group-by");
    tour.keys(&["⎋"]);
    tour.step();
    tour.command("search.results.setOption", json!({ "option": "mergeSameLine", "on": false }));
    let unmerged = tour.tree()["nodes"][1]["count"].clone();
    tour.command("search.results.setOption", json!({ "option": "mergeSameLine", "on": true }));
    assert_eq!(unmerged, 5, "every usage counts, merged or not");

    tour.toggle("Show Write Access", "showWrite");
    let options = tour.command("search.results.list", json!({}))["options"].clone();
    assert_eq!(options["showWrite"], false, "{options}");
    let groups = tour.groups();
    assert!(!groups.contains(&"Value write".to_owned()), "the write is filtered out: {groups:?}");
    tour.snapshot("usages-write-filtered");
    tour.toggle("Show Write Access", "showWrite");
    tour.toggle("Show Read Access", "showRead");
    let groups = tour.groups();
    assert!(!groups.contains(&"Value read".to_owned()), "the reads are filtered out: {groups:?}");
    tour.toggle("Show Read Access", "showRead");

    let row = tour.tree()["nodes"][1].to_string();
    assert!(row.contains("println(Version, Version)"), "{row}");
    tour.click("9: println(Version, Version)");
    for _ in 0..5 {
        tour.step();
    }
    tour.snapshot("usages-preview");

    tour.tool("Pin Tab");
    let list = tour.command("search.results.list", json!({}));
    assert_eq!(list["tabs"][0]["pinned"], true, "{list}");

    let asked = fake.as_ref().map(|server| server.requests("textDocument/references").len());
    tour.click("9: println(Version, Version)");
    tour.keys(&["⌘R"]);
    tour.until("the rerun", &|tour| tour.tree()["done"] == true);
    if let (Some(server), Some(asked)) = (&fake, asked) {
        assert_eq!(server.requests("textDocument/references").len(), asked + 1, "Rerun asks the server again");
    }
    assert_eq!(tour.tree()["total"], 5);

    tour.caret(1);
    tour.find_usages(false);
    let list = tour.command("search.results.list", json!({}));
    assert_eq!(list["tabs"].as_array().unwrap().len(), 2, "the pinned tab stays: {list}");
    tour.tool("Open results in new tab");
    let options = tour.command("navigate.usagesSettings.get", json!({}))["options"].clone();
    assert_eq!(options["newTab"], true, "{options}");
    tour.caret(1);
    tour.find_usages(false);
    let list = tour.command("search.results.list", json!({}));
    assert_eq!(list["tabs"].as_array().unwrap().len(), 3, "Open in new tab: {list}");
    tour.command("navigate.usagesSettings.set", json!({ "newTab": false }));

    tour.caret(2);
    tour.keys(&["⌥⇧⌘F7"]);
    tour.until("the settings dialog", &|tour| tour.state()["popups"] == json!(["findUsagesSettings"]));
    tour.snapshot("usages-settings");
    tour.click("Project Files");
    tour.click("Current File");
    tour.click("Skip results tab with one usage");
    let dialog = tour.command("navigate.usagesSettings.get", json!({}));
    assert_eq!(
        (&dialog["open"], &dialog["options"]["scope"], &dialog["options"]["skipSingle"]),
        (&json!(true), &json!("currentFile"), &json!(true)),
        "{dialog}"
    );
    tour.click("Skip results tab with one usage");
    tour.keys(&["⏎"]);
    tour.until("the settings found", &|tour| tour.state()["popups"] == json!([]));
    tour.until("the usages in the current file", &|tour| {
        tour.command("search.results.tree", json!({ "depth": 0 }))["nodes"][1]["label"] == "Usages in Current File"
    });
    assert_eq!(tour.tree()["total"], 4, "main_test.go is out of the scope");
    tour.command("navigate.usagesSettings.set", json!({ "scope": "projectFiles" }));

    let opened =
        screen_ask(&mut tour.screen, &mut tour.app, "run_action", json!({ "action": "view.recentFindUsages.open" }))
            .unwrap();
    let searches = &opened["result"]["searches"];
    assert!(searches.as_array().map_or(0, Vec::len) >= 3, "{opened}");
    assert_eq!(searches[0]["scope"], "currentFile");
    tour.until("Recent Find Usages", &|tour| tour.state()["popups"] == json!(["recentFindUsages"]));
    tour.snapshot("usages-recent");
    tour.keys(&["↓"]);
    tour.keys(&["⏎"]);
    tour.until("the recent search", &|tour| {
        tour.command("search.results.tree", json!({ "depth": 0 }))["nodes"][1]["label"] == "Usages in Project Files"
            && tour.state()["popups"] == json!([])
    });

    tour.caret(3);
    tour.keys(&["⌥⌘F7"]);
    tour.until("the Show Usages popup", &|tour| tour.state()["popups"] == json!(["showUsages"]));
    tour.until("the popup's usages", &|tour| tour.command("navigate.showUsages.get", json!({}))["pending"] == false);
    let tabs = tour.command("search.results.list", json!({}))["tabs"].as_array().unwrap().len();
    tour.command("navigate.showUsages.openInFindWindow", json!({}));
    tour.until("the tab of Open in Find Tool Window", &|tour| {
        tour.command("navigate.usages.list", json!({}))["pending"] == false
            && tour.command("search.results.list", json!({}))["tabs"].as_array().unwrap().len() >= tabs
    });
    let tree = tour.tree();
    assert_eq!((&tree["kind"], &tree["total"]), (&json!("usages"), &json!(5)), "{tree}");
    tour.snapshot("usages-from-show-usages");
}

#[test]
fn find_usages_groups_filters_previews_pins_reruns_and_reopens_in_a_running_ide() {
    tour(None, "find-usages");
}

#[test]
#[ignore = "needs gopls"]
fn find_usages_with_gopls_in_a_running_ide() {
    let Some(gopls) = gopls() else {
        assert!(std::env::var_os("ZEN_LSP_REQUIRE_GOPLS").is_none(), "gopls is not installed");
        return;
    };
    tour(Some(gopls), "find-usages-gopls");
}

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_project::model::{Library, ModelEnv, ModelProvider, Module, ProjectModel};
use zen_project::settings::{ProjectSettings, ReadFiles, Scope, SettingsProvider};

use super::*;

struct Models {
    library: PathBuf,
}

impl ModelProvider for Models {
    fn id(&self) -> &str {
        "test"
    }

    fn watches(&self, _: &str) -> bool {
        false
    }

    fn load(&self, root: &Path, _: &ModelEnv) -> Option<ProjectModel> {
        Some(ProjectModel {
            modules: vec![
                Module { name: "example.com/shop".to_owned(), root: root.to_owned(), ..Module::default() },
                Module { name: "example.com/tools".to_owned(), root: root.join("tools"), ..Module::default() },
            ],
            libraries: vec![Library {
                name: "Go SDK".to_owned(),
                root: self.library.clone(),
                exists: true,
                ..Library::default()
            }],
            ..ProjectModel::default()
        })
    }
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
        let scope = |name: &str, pattern: &str| Scope { name: name.to_owned(), pattern: pattern.to_owned() };
        Some(ProjectSettings {
            scopes: vec![
                scope("Tests", "file:*_test.go"),
                scope("Cart", "file:internal/cart//*&&!$Tests"),
                scope("Tools", "file[example.com/tools]:*.go"),
                scope("Broken", "src:*..*"),
            ],
            ..ProjectSettings::default()
        })
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    root: PathBuf,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

impl Fixture {
    fn dispatch(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap()
    }

    fn files(&mut self, params: Value) -> Vec<String> {
        let found = self.dispatch("search.scope.files", params);
        found["files"].as_array().unwrap().iter().map(|file| file.as_str().unwrap().to_owned()).collect()
    }

    fn found(&mut self, params: Value) -> Vec<String> {
        let found = self.dispatch("search.text.find", params);
        let mut files: Vec<String> =
            found["matches"].as_array().unwrap().iter().map(|m| m["path"].as_str().unwrap().to_owned()).collect();
        files.dedup();
        files
    }
}

fn fixture() -> Fixture {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for (file, text) in [
        ("main.go", "package main // needle\n"),
        ("internal/cart/cart.go", "package cart // needle\n"),
        ("internal/cart/cart_test.go", "package cart // needle\n"),
        ("internal/cart/deep/deep.go", "package deep // needle\n"),
        ("tools/gen.go", "package tools // needle\n"),
        ("README.md", "needle\n"),
    ] {
        std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
        std::fs::write(root.join(file), text).unwrap();
    }
    let library = tempfile::tempdir().unwrap();
    std::fs::write(library.path().join("fmt.go"), "package fmt // needle\n").unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Workbench::default());
    let models = Models { library: library.path().to_owned() };
    session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(models)], ModelEnv::default());
    session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Scopes)]);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
    while !session.state::<Workbench>().unwrap().index.as_ref().unwrap().is_ready() {
        std::thread::sleep(Duration::from_millis(5));
    }
    Fixture { registry, session, root, _dirs: (dir, library) }
}

fn needle(scope: &str) -> Value {
    json!({ "query": { "text": "needle" }, "scope": scope })
}

#[test]
fn the_list_has_the_predefined_scopes_the_modules_and_the_named_scopes_in_order() {
    let mut fixture = fixture();
    let list = fixture.dispatch("search.scope.list", json!({}));
    let ids: Vec<&str> = list["scopes"].as_array().unwrap().iter().map(|scope| scope["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "allPlaces",
            "projectFiles",
            "projectAndLibraries",
            "projectNotIgnored",
            "gitTracked",
            "scratches",
            "recentlyViewed",
            "recentlyChanged",
            "module:example.com/shop",
            "module:example.com/tools",
            "currentFile",
            "allChanged",
            "production",
            "tests",
            "named:Tests",
            "named:Cart",
            "named:Tools",
            "named:Broken",
        ]
    );
    let names: Vec<&str> =
        list["scopes"].as_array().unwrap().iter().map(|scope| scope["name"].as_str().unwrap()).collect();
    assert_eq!(&names[..2], ["All Places", "Project Files"]);
    assert_eq!(names[8], "Module 'example.com/shop'");
    assert_eq!(names[13], "Project Test Files");
    assert_eq!(list["scopes"][15]["pattern"], "file:internal/cart//*&&!$Tests");
    assert_eq!(list["scopes"][15]["group"], "named");
    assert!(list["scopes"][17]["error"].as_str().unwrap().contains("src"));
    assert_eq!(list["modules"][1], json!({ "name": "example.com/tools", "folder": "tools" }));
    let everywhere = fixture.dispatch("search.scope.list", json!({ "everywhere": true }));
    assert!(everywhere["scopes"].as_array().unwrap().iter().all(|scope| scope["id"] != "currentFile"));
}

#[test]
fn find_in_files_searches_in_the_scope_it_is_given() {
    let mut fixture = fixture();
    assert_eq!(fixture.found(needle("projectFiles")).len(), 6);
    assert_eq!(fixture.found(json!({ "query": { "text": "needle" } })).len(), 6, "Project Files when absent");
    assert_eq!(fixture.found(needle("tests")), ["internal/cart/cart_test.go"]);
    assert!(!fixture.found(needle("production")).contains(&"internal/cart/cart_test.go".to_owned()));
    assert_eq!(fixture.found(needle("module:example.com/tools")), ["tools/gen.go"]);
    assert_eq!(fixture.found(needle("module:example.com/shop")).len(), 6);
    assert_eq!(fixture.found(needle("named:Cart")), ["internal/cart/cart.go", "internal/cart/deep/deep.go"]);
    assert_eq!(fixture.found(needle("named:Tools")), ["tools/gen.go"]);
    let with_libraries = fixture.found(needle("projectAndLibraries"));
    let library = with_libraries.iter().find(|path| path.ends_with("/fmt.go"));
    assert!(
        library.is_some_and(|path| Path::new(path).is_absolute()) && with_libraries.len() == 7,
        "{with_libraries:?}"
    );
    assert_eq!(fixture.found(needle("allPlaces")).len(), 7);
    let error = fixture.registry.dispatch(&mut fixture.session, "search.text.find", needle("named:Broken"));
    assert!(error.unwrap_err().to_string().contains("src"));
    let error = fixture.registry.dispatch(&mut fixture.session, "search.text.find", needle("nowhere"));
    assert!(error.unwrap_err().to_string().contains("nowhere"));
}

#[test]
fn the_directory_searches_its_subfolders_only_when_recursive() {
    let mut fixture = fixture();
    let recursive = json!({ "query": { "text": "needle" }, "directory": "internal/cart" });
    assert_eq!(fixture.found(recursive).len(), 3);
    let flat = json!({ "query": { "text": "needle" }, "directory": "internal/cart", "recursive": false });
    assert_eq!(fixture.found(flat), ["internal/cart/cart.go", "internal/cart/cart_test.go"]);
    let both = json!({ "query": { "text": "needle" }, "directory": "internal", "scope": "tests" });
    assert_eq!(fixture.found(both), ["internal/cart/cart_test.go"]);
}

#[test]
fn the_background_search_and_replace_keep_to_the_scope() {
    let mut fixture = fixture();
    fixture.dispatch("search.text.start", needle("tests"));
    let mut results = fixture.dispatch("search.text.results", json!({}));
    while results["done"] != true {
        std::thread::sleep(Duration::from_millis(5));
        results = fixture.dispatch("search.text.results", json!({}));
    }
    let files: Vec<&str> = results["matches"].as_array().unwrap().iter().filter_map(|m| m["path"].as_str()).collect();
    assert_eq!(files, ["internal/cart/cart_test.go"]);
    let replace = json!({ "query": { "text": "needle" }, "scope": "named:Cart", "replacement": "thread" });
    assert_eq!(fixture.dispatch("search.text.replaceAll", replace)["files"], 2);
    let text = |file: &str| std::fs::read_to_string(fixture.root.join(file)).unwrap();
    assert!(text("internal/cart/cart.go").contains("thread") && text("internal/cart/cart_test.go").contains("needle"));
    let outside = json!({ "query": { "text": "needle" }, "scope": "projectAndLibraries", "replacement": "thread" });
    let refused = fixture.registry.dispatch(&mut fixture.session, "search.text.replaceAll", outside);
    assert!(refused.unwrap_err().to_string().contains("fmt.go"));
    assert!(text("main.go").contains("needle"), "nothing is replaced when a file is outside the project");
}

#[test]
fn the_current_file_and_the_recently_viewed_and_changed_files() {
    let mut fixture = fixture();
    assert!(fixture.files(json!({ "scope": "currentFile" })).is_empty());
    let main = fixture.root.join("main.go");
    let cart = fixture.root.join("internal/cart/cart.go");
    fixture.dispatch("file.document.open", json!({ "path": main }));
    fixture.dispatch("file.document.open", json!({ "path": cart }));
    assert_eq!(fixture.files(json!({ "scope": "currentFile" })), ["internal/cart/cart.go"]);
    assert_eq!(fixture.files(json!({ "scope": "recentlyViewed" })), ["internal/cart/cart.go", "main.go"]);
    assert!(fixture.files(json!({ "scope": "recentlyChanged" })).is_empty());
    fixture
        .session
        .state_mut::<Workbench>()
        .active_document_mut()
        .unwrap()
        .replace_ranges(vec![(0..0, "x".to_owned())]);
    assert_eq!(fixture.files(json!({ "scope": "recentlyChanged" })), ["internal/cart/cart.go"]);
}

#[test]
fn scratches_are_searched_from_their_folder() {
    let mut fixture = fixture();
    assert!(fixture.files(json!({ "scope": "scratches" })).is_empty());
    let scratches = fixture.root.parent().unwrap().join(format!("scratches-{}", std::process::id()));
    std::fs::create_dir_all(&scratches).unwrap();
    std::fs::write(scratches.join("scratch.go"), "package main // needle\n").unwrap();
    fixture.session.state_mut::<SearchScopes>().set_scratches(scratches.clone());
    assert_eq!(fixture.files(json!({ "scope": "scratches" })), ["Scratches and Consoles/scratch.go"]);
    let scratch = scratches.join("scratch.go").display().to_string();
    assert_eq!(fixture.found(needle("scratches")), [scratch], "a file outside the project by its whole path");
    std::fs::remove_dir_all(scratches).unwrap();
}

#[test]
fn patterns_are_checked_and_listed() {
    let mut fixture = fixture();
    let files = fixture.files(json!({ "pattern": "file:internal//*&&!$Tests||file:*.md" }));
    assert_eq!(files, ["README.md", "internal/cart/cart.go", "internal/cart/deep/deep.go"]);
    for (pattern, part) in [("", "empty"), ("file:*.go&&", "column"), ("$Nope", "Nope"), ("lib:*", "lib")] {
        let error =
            fixture.registry.dispatch(&mut fixture.session, "search.scope.files", json!({ "pattern": pattern }));
        assert!(error.unwrap_err().to_string().contains(part), "{pattern}");
    }
    let limited = fixture.dispatch("search.scope.files", json!({ "scope": "projectFiles", "limit": 2 }));
    assert_eq!((limited["files"].as_array().unwrap().len(), limited["total"].as_u64()), (2, Some(6)));
}

#[test]
fn the_scope_row_is_remembered() {
    let mut fixture = fixture();
    let list = fixture.dispatch("search.scope.list", json!({}));
    assert_eq!(
        list["selection"],
        json!({ "area": "project", "module": null, "directory": "", "recursive": true, "scope": "projectFiles", "everywhere": "projectFiles" })
    );
    let chosen = json!({ "area": "scope", "module": "example.com/tools", "directory": "internal", "recursive": false, "scope": "named:Cart", "everywhere": "tests" });
    fixture.dispatch("search.scope.select", chosen.clone());
    assert_eq!(fixture.dispatch("search.scope.list", json!({}))["selection"], chosen);
    let selection: ScopeSelection = serde_json::from_value(chosen).unwrap();
    assert_eq!(selection.search_params(), json!({ "scope": "named:Cart", "directory": "", "recursive": true }));
    let directory = ScopeSelection { area: ScopeArea::Directory, ..selection.clone() };
    assert_eq!(
        directory.search_params(),
        json!({ "scope": "projectFiles", "directory": "internal", "recursive": false })
    );
    let module = ScopeSelection { area: ScopeArea::Module, ..selection };
    assert_eq!(module.search_params()["scope"], "module:example.com/tools");
}

#[test]
fn the_git_scopes_follow_the_status() {
    crate::vcs::testing::isolate();
    let dir = crate::vcs::testing::test_repository();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    crate::vcs::testing::git(&root, &["add", ".gitignore", "b.txt"]);
    crate::vcs::testing::git(&root, &["commit", "-q", "-m", "More"]);
    std::fs::write(root.join("a.txt"), "changed\n").unwrap();
    std::fs::write(root.join("new.txt"), "new\n").unwrap();
    std::fs::write(root.join("trace.log"), "log\n").unwrap();
    std::fs::create_dir_all(root.join("fresh/deep")).unwrap();
    std::fs::write(root.join("fresh/deep/x.txt"), "x\n").unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();
    std::fs::write(root.join("logs/run.log"), "log\n").unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
    while !session.state::<Workbench>().unwrap().index.as_ref().unwrap().is_ready() {
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut files = |scope: &str| {
        let found = registry.dispatch(&mut session, "search.scope.files", json!({ "scope": scope })).unwrap();
        found["files"].as_array().unwrap().iter().map(|file| file.as_str().unwrap().to_owned()).collect::<Vec<_>>()
    };
    assert_eq!(files("allChanged"), ["a.txt"]);
    assert_eq!(files("gitTracked"), [".gitignore", "a.txt", "b.txt"]);
    let not_ignored = files("projectNotIgnored");
    assert!(not_ignored.contains(&"new.txt".to_owned()) && !not_ignored.contains(&"trace.log".to_owned()));
    assert!(not_ignored.contains(&"fresh/deep/x.txt".to_owned()) && !not_ignored.contains(&"logs/run.log".to_owned()));
}

#[test]
fn outside_a_repository_nothing_is_tracked() {
    let mut fixture = fixture();
    assert!(fixture.files(json!({ "scope": "gitTracked" })).is_empty());
    assert!(fixture.files(json!({ "scope": "allChanged" })).is_empty());
    assert_eq!(fixture.files(json!({ "scope": "projectNotIgnored" })).len(), 6);
}

#[test]
fn search_everywhere_paths_are_checked_against_the_scope() {
    let mut fixture = fixture();
    let root = fixture.root.clone();
    let tests = resolve(&mut fixture.session, "tests").unwrap();
    assert!(tests.contains(Some(&root), &root.join("internal/cart/cart_test.go")));
    assert!(!tests.contains(Some(&root), &root.join("main.go")));
    assert!(!tests.contains(Some(&root), Path::new("/elsewhere/x_test.go")));
    let libraries = resolve(&mut fixture.session, "projectAndLibraries").unwrap();
    let sdk = libraries.outside[0].0.join("fmt.go");
    assert!(libraries.contains(Some(&root), &sdk));
    assert!(!resolve(&mut fixture.session, "projectFiles").unwrap().contains(Some(&root), &sdk));
}

#[test]
fn the_projects_own_scopes_are_saved_renamed_and_deleted() {
    let mut fixture = fixture();
    fixture.dispatch("search.scope.save", json!({ "name": "Docs", "pattern": "file:*.md" }));
    let saved = std::fs::read_to_string(fixture.root.join(".zen/scopes.json")).unwrap();
    assert!(saved.contains("file:*.md"), "{saved}");
    assert!(!fixture.root.join(".idea").exists(), "the .idea files are never written");
    let list = fixture.dispatch("search.scope.list", json!({}));
    let docs = list["scopes"].as_array().unwrap().iter().find(|scope| scope["id"] == "named:Docs").unwrap().clone();
    assert_eq!((docs["own"].clone(), docs["pattern"].clone()), (json!(true), json!("file:*.md")));
    assert_eq!(fixture.files(json!({ "scope": "named:Docs" })), ["README.md"]);
    fixture.dispatch("search.scope.save", json!({ "name": "Tests", "pattern": "file:main.go" }));
    assert_eq!(fixture.files(json!({ "scope": "named:Tests" })), ["main.go"], "the own scope hides the .idea one");
    assert_eq!(fixture.found(needle("named:Cart")).len(), 3, "and $Tests refers to it");
    fixture.dispatch("search.scope.save", json!({ "name": "Readme", "pattern": "file:README.md", "previous": "Docs" }));
    let names: Vec<String> = own_scopes(&fixture.root).into_iter().map(|scope| scope.name).collect();
    assert_eq!(names, ["Readme", "Tests"]);
    for (params, part) in [
        (json!({ "name": " ", "pattern": "file:*" }), "name"),
        (json!({ "name": "Bad", "pattern": "file:*.go&&" }), "column"),
        (json!({ "name": "Self", "pattern": "$Self" }), "Self"),
    ] {
        let error = fixture.registry.dispatch(&mut fixture.session, "search.scope.save", params);
        assert!(error.unwrap_err().to_string().contains(part), "{part}");
    }
    fixture.dispatch("search.scope.delete", json!({ "name": "Readme" }));
    let error = fixture.registry.dispatch(&mut fixture.session, "search.scope.delete", json!({ "name": "Cart" }));
    assert!(error.unwrap_err().to_string().contains("read-only"));
    assert_eq!(own_scopes(&fixture.root).len(), 1);
}

#[test]
fn paths_are_filtered_by_a_scope() {
    let mut fixture = fixture();
    let (main, test) = (fixture.root.join("main.go"), fixture.root.join("internal/cart/cart_test.go"));
    let paths = json!([main, test, "/elsewhere/a_test.go"]);
    let kept = fixture.dispatch("search.scope.filter", json!({ "scope": "tests", "paths": paths }));
    assert_eq!(kept, json!([test]));
    let kept = fixture.dispatch("search.scope.filter", json!({ "paths": paths }));
    assert_eq!(kept, json!([main, test]), "Project Files when absent");
    let error = fixture.registry.dispatch(
        &mut fixture.session,
        "search.scope.filter",
        json!({ "scope": "nowhere", "paths": [] }),
    );
    assert!(error.is_err());
}

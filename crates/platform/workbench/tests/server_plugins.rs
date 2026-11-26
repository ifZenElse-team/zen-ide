use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration, ServerPlugin};

const SERVER: &str = "typescript-language-server";

const COUNTER: &str = r#"<script lang="ts">
  let { label, step = 1 }: { label: string; step?: number } = $props()
</script>

<p>{label} {step}</p>
"#;

const MAIN: &str = "import { mount } from 'svelte'
import Counter from './Counter.svelte'

export const app = mount(Counter, { target: document.body, props: { label: 'Clicks', step: 2 } })
export const wrong = mount(Counter, { target: document.body, props: { label: 5 } })
";

const TSCONFIG: &str = r#"{ "compilerOptions": { "target": "ES2020", "module": "ESNext", "moduleResolution": "Bundler", "strict": true, "lib": ["ES2020", "DOM"] }, "include": ["src/**/*"] }"#;

const PACKAGE: &str =
    r#"{ "name": "app", "private": true, "type": "module", "devDependencies": { "svelte": "^5.0.0" } }"#;

struct Window {
    registry: Registry,
    session: Session,
    folder: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Window {
    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder.path().join(name)
    }

    fn text(&self, path: &Path) -> String {
        let workbench = self.session.state::<zen_workbench::Workbench>().unwrap();
        let document = workbench.documents().iter().find(|document| document.path() == path);
        document.map(|document| document.text().to_owned()).unwrap_or_default()
    }

    fn sync(&mut self) {
        self.run("language.servers.sync", json!({}));
    }

    fn until(&mut self, command: &str, params: Value, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            self.sync();
            let answer = self.registry.dispatch(&mut self.session, command, params.clone());
            match answer {
                Ok(answer) if done(&answer) => return answer,
                answer => assert!(Instant::now() < deadline, "{command}: {answer:?}"),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn plugin_json(plugin: &str) -> Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../plugins/{plugin}/plugin.json"));
    serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap()
}

fn plugin_servers(plugin: &str) -> Vec<ServerDeclaration> {
    ServerDeclaration::parse_list(&plugin_json(plugin)["languageServers"].to_string()).unwrap()
}

fn contributed(plugin: &str) -> Vec<ServerPlugin> {
    ServerPlugin::parse_list(&plugin_json(plugin)["serverPlugins"].to_string()).unwrap()
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) {
    std::os::unix::fs::symlink(from, to).unwrap();
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) {
    std::os::windows::fs::symlink_dir(from, to).unwrap();
}

fn text(answer: &Value) -> String {
    answer.to_string()
}

#[test]
fn svelte_contributes_its_typescript_plugin_to_the_typescript_server() {
    let plugins = contributed("svelte");
    let [plugin] = plugins.as_slice() else { panic!("{plugins:?}") };
    assert_eq!((plugin.server.as_str(), plugin.name.as_str()), (SERVER, "typescript-svelte-plugin"));
    assert_eq!(plugin.package(), "typescript-svelte-plugin@0.3.52");
    assert_eq!(plugin.languages, ["svelte"]);
    assert_eq!(plugin.when.as_ref().map(|when| when.packages.clone()), Some(vec!["svelte".to_owned()]));
    let vue = contributed("vue");
    assert_eq!(vue[0].when.as_ref().map(|when| when.packages.clone()), Some(vec!["vue".to_owned()]));
    assert!(plugin_servers("typescript").iter().any(|server| server.id == plugin.server), "a server of the IDE");
}

#[test]
#[ignore = "needs the TypeScript server with typescript-svelte-plugin: ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn a_typescript_file_reads_the_svelte_components_it_imports() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX").expect("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX"),
    );
    let project = PathBuf::from(std::env::var_os("ZEN_SVELTE_PROJECT").expect("ZEN_SVELTE_PROJECT"));
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("src")).unwrap();
    for (name, text) in
        [("src/Counter.svelte", COUNTER), ("src/main.ts", MAIN), ("tsconfig.json", TSCONFIG), ("package.json", PACKAGE)]
    {
        std::fs::write(folder.path().join(name), text).unwrap();
    }
    std::fs::create_dir_all(folder.path().join("node_modules")).unwrap();
    link(&project.join("node_modules/svelte"), &folder.path().join("node_modules/svelte"));
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("language-servers")).unwrap();
    link(&prefix, &data.path().join("language-servers").join(SERVER));
    let svelte_prefix = std::env::var_os("ZEN_SVELTE_LANGUAGE_SERVER_PREFIX").map(PathBuf::from);
    if let Some(svelte_prefix) = &svelte_prefix {
        link(svelte_prefix, &data.path().join("language-servers/svelte-language-server"));
    }
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        std::sync::Arc::new(zen_workbench::language::servers::SystemTools),
    );
    servers.declare(plugin_servers("typescript"));
    servers.declare(plugin_servers("svelte"));
    session.set_state(servers.clone());
    let mut window = Window { registry, session, folder, _data: data };
    let main = window.path("src/main.ts");
    let counter = window.path("src/Counter.svelte");
    window.run("file.document.open", json!({ "path": main }));

    let problems = |answer: &Value| zen_workbench::language::listed_problems(answer);
    let before = window.until("problems.file.list", json!({ "path": main }), |answer| answer["checked"] != false);
    assert!(problems(&before).is_empty(), "{before}");
    checked("without the plug-in: the component's props are not checked");

    servers.contribute("svelte", contributed("svelte"));
    let after = window.until("problems.file.list", json!({ "path": main }), |answer| {
        let list = problems(answer);
        !list.is_empty()
            && list.iter().any(|problem| problem["message"].as_str().is_some_and(|message| message.contains("number")))
    });
    let list = problems(&after);
    assert!(list.iter().all(|problem| problem["line"] == 5), "only the wrong props: {after}");
    checked("problems: the props are typed");

    let at = MAIN.find("mount(Counter").unwrap() + "mount(".len();
    let shown =
        window.until("editor.documentation.show", json!({ "path": main, "offset": at, "waitMs": 3000 }), |answer| {
            answer["pending"] == false
        });
    assert!(shown["markdown"].as_str().unwrap_or_default().contains("Counter"), "{shown}");
    checked("quick documentation: ok");

    let found =
        window.until("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 3000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
    assert!(text(&found).contains("Counter.svelte"), "{found}");
    checked("go to declaration into the component: ok");

    let at = MAIN.find("props: { label: 'Clicks'").unwrap() + "props: { ".len();
    window.run("file.document.open", json!({ "path": main }));
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.completion.start", json!({}));
    let list = window.until("editor.completion.list", json!({}), |list| {
        list["items"].as_array().is_some_and(|items| items.iter().any(|item| item["label"] == "label"))
    });
    assert!(list["items"].as_array().unwrap().iter().any(|item| item["label"] == "label"), "{list}");
    window.run("editor.completion.cancel", json!({}));
    checked("prop completion: ok");

    let at = MAIN.find("import Counter").unwrap() + "import ".len();
    let usages = window.until(
        "navigate.usages.find",
        json!({ "path": main, "offset": at, "waitMs": 5000, "includeDeclaration": true }),
        |answer| answer.get("pending") != Some(&json!(true)),
    );
    assert!(text(&usages).contains("main.ts"), "{usages}");
    checked(&format!(
        "find usages: {}",
        if text(&usages).contains("Counter.svelte") { "reaches the component" } else { "in the .ts file" }
    ));

    let at = MAIN.find("label: 'Clicks'").unwrap();
    let renamed = window.until(
        "refactor.rename.apply",
        json!({ "path": main, "offset": at, "newName": "title", "waitMs": 5000 }),
        |answer| answer["edits"].as_u64().is_some_and(|edits| edits > 0),
    );
    assert!(window.text(&main).contains("title: 'Clicks'"), "{renamed}");
    checked(&format!("rename from the .ts file: {} edit(s), in {}", renamed["edits"], text(&renamed["files"])));
    window.run("editor.edit.undo", json!({}));

    if svelte_prefix.is_none() {
        checked("rename from the component: skipped, no ZEN_SVELTE_LANGUAGE_SERVER_PREFIX");
        return;
    }
    window.run("file.document.open", json!({ "path": counter }));
    let at = COUNTER.find("{ label: string").unwrap() + 2;
    let renamed = window.until(
        "refactor.rename.apply",
        json!({ "path": counter, "offset": at, "newName": "title", "waitMs": 8000 }),
        |answer| answer["edits"].as_u64().is_some_and(|edits| edits > 0),
    );
    let on_disk = std::fs::read_to_string(&main).unwrap();
    let in_editor = window.text(&main);
    assert!(window.text(&counter).contains("title: string"), "{renamed} {}", window.text(&counter));
    assert!(in_editor.contains("title:") || on_disk.contains("title:"), "the .ts file follows: {renamed} {in_editor}");
    checked(&format!("rename from the component: {} edit(s), in {}", renamed["edits"], text(&renamed["files"])));
}

#[allow(clippy::print_stderr, reason = "the list of the features checked is what the test reports")]
fn checked(what: &str) {
    eprintln!("{what}");
}

const APP_VUE: &str = r#"<script setup lang="ts">
defineProps<{ label: string; step?: number }>()
</script>

<template>
  <p>{{ label }}</p>
</template>
"#;

const MAIN_VUE: &str = "import { h } from 'vue'
import App from './App.vue'

export const good = h(App, { label: 'Clicks', step: 2 })
export const wrong = h(App, { label: 5 })
";

#[test]
#[ignore = "needs the TypeScript server with @vue/typescript-plugin and ZEN_VUE_PROJECT (see the function's documentation)"]
fn a_typescript_file_reads_the_vue_components_it_imports() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX").expect("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX"),
    );
    let project = PathBuf::from(std::env::var_os("ZEN_VUE_PROJECT").expect("ZEN_VUE_PROJECT"));
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("src")).unwrap();
    let package = r#"{ "name": "app", "private": true, "type": "module", "dependencies": { "vue": "^3.5.0" } }"#;
    for (name, text) in
        [("src/App.vue", APP_VUE), ("src/main.ts", MAIN_VUE), ("tsconfig.json", TSCONFIG), ("package.json", package)]
    {
        std::fs::write(folder.path().join(name), text).unwrap();
    }
    std::fs::create_dir_all(folder.path().join("node_modules")).unwrap();
    link(&project.join("node_modules/vue"), &folder.path().join("node_modules/vue"));
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("language-servers")).unwrap();
    link(&prefix, &data.path().join("language-servers").join(SERVER));
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        std::sync::Arc::new(zen_workbench::language::servers::SystemTools),
    );
    servers.declare(plugin_servers("typescript"));
    servers.contribute("vue", contributed("vue"));
    session.set_state(servers.clone());
    let mut window = Window { registry, session, folder, _data: data };
    let main = window.path("src/main.ts");
    window.run("file.document.open", json!({ "path": main }));
    let problems = |answer: &Value| zen_workbench::language::listed_problems(answer);
    let answer = window.until("problems.file.list", json!({ "path": main }), |answer| {
        problems(answer).iter().any(|problem| problem["line"] == 5)
    });
    let list = problems(&answer);
    assert!(list.iter().all(|problem| problem["line"] == 5), "only the wrong props: {answer}");
    checked(&format!("vue: the props are typed ({} problem(s) on the wrong line)", list.len()));
    let at = MAIN_VUE.find("h(App").unwrap() + 2;
    let found =
        window.until("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 3000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
    assert!(text(&found).contains("App.vue"), "{found}");
    checked("vue: go to declaration into the component: ok");
}

#[test]
#[ignore = "needs rust-analyzer on PATH (rustup component add rust-analyzer) and cargo"]
fn rust_analyzer_moves_the_file_of_a_renamed_module_and_renames_the_module_of_a_renamed_file() {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    for (name, text) in [
        ("Cargo.toml", "[package]\nname = \"shop\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        (
            "src/main.rs",
            "mod util;\nmod tools;\n\nfn main() {\n    util::greet();\n    tools::help();\n    let _wrong: u32 = \"text\";\n}\n",
        ),
        ("src/util.rs", "pub fn greet() {}\n"),
        ("src/tools.rs", "pub fn help() {}\n"),
    ] {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let data = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let root = folder.path().canonicalize().unwrap();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        std::sync::Arc::new(zen_workbench::language::servers::SystemTools),
    );
    let declared = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lang/rust/language-servers.json");
    servers.declare(ServerDeclaration::parse_list(&std::fs::read_to_string(declared).unwrap()).unwrap());
    session.set_state(servers);
    let mut window = Window { registry, session, folder, _data: data };
    let main = root.join("src/main.rs");
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| problem["code"] == "E0308")
    });
    let at = "mod util;\nmod tools;\n\nfn main() {\n    ".len() + 1;
    let asked = json!({ "path": main, "offset": at, "newName": "helpers", "waitMs": 5000 });
    let deadline = Instant::now() + Duration::from_secs(120);
    let preview = loop {
        window.sync();
        let answer = window
            .registry
            .dispatch(&mut window.session, "refactor.rename.preview", asked.clone())
            .unwrap_or_else(|error| json!({ "error": error.to_string() }));
        if answer["pending"] == false && answer["error"].is_null() && !text(&answer["operations"]).contains("[]") {
            break answer;
        }
        if answer["pending"] != true {
            window.run("refactor.rename.cancel", json!({}));
        }
        assert!(Instant::now() < deadline, "{answer}");
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(text(&preview["operations"]).contains("helpers.rs"), "{preview}");
    window.run("refactor.rename.apply", json!({}));
    assert!(root.join("src/helpers.rs").exists() && !root.join("src/util.rs").exists(), "{preview}");
    checked("module rename renames its file: ok");

    window.until("refactor.rename.prepare", json!({ "path": main, "offset": at, "waitMs": 5000 }), |answer| {
        answer["pending"] == false
    });
    window.run("file.fs.rename", json!({ "path": root.join("src/tools.rs"), "name": "kit.rs" }));
    assert!(root.join("src/kit.rs").exists());
    let after = window.text(&main);
    assert!(after.contains("mod kit;") && after.contains("kit::help()"), "{after}");
    checked("file rename renames the module: ok");
}

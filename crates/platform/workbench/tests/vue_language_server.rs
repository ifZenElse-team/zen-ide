use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration};

const SERVER: &str = "vue-language-server";

const APP: &str = r#"<template>
  <p class="title">{{ greeting }}</p>
  <button @click="increment">{{ count }}</button>
</template>

<script setup lang="ts">
import { double } from './util'

const greeting: string = 'Hello'
const count = { value: 0 }
function increment() {
  count.value = double(count.value)
}
const broken: number = 'not a number'
</script>

<style scoped>
.title { color: red }
</style>
"#;

const UTIL: &str = "export function double(value: number): number {\n  return value * 2\n}\n";

const TSCONFIG: &str = r#"{ "compilerOptions": { "target": "ES2020", "module": "ESNext", "moduleResolution": "Bundler", "strict": true }, "include": ["src/**/*"] }"#;

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

fn plugin_servers() -> Vec<ServerDeclaration> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/vue/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    ServerDeclaration::parse_list(&manifest["languageServers"].to_string()).unwrap()
}

fn window(prefix: &Path) -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("src")).unwrap();
    for (name, text) in [("src/App.vue", APP), ("src/util.ts", UTIL), ("tsconfig.json", TSCONFIG)] {
        std::fs::write(folder.path().join(name), text).unwrap();
    }
    if prefix.join("node_modules/vue").is_dir() {
        std::fs::create_dir_all(folder.path().join("node_modules")).unwrap();
        link(&prefix.join("node_modules/vue"), &folder.path().join("node_modules/vue"));
    }
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("language-servers")).unwrap();
    link(prefix, &data.path().join("language-servers").join(SERVER));
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        std::sync::Arc::new(zen_workbench::language::servers::SystemTools),
    );
    servers.declare(plugin_servers());
    let found = servers.executable(SERVER).expect("the server in the IDE's folder");
    assert!(found.starts_with(data.path().join("language-servers")), "{}", found.display());
    session.set_state(servers);
    Window { registry, session, folder, _data: data }
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) {
    std::os::unix::fs::symlink(from, to).unwrap();
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) {
    std::os::windows::fs::symlink_dir(from, to).unwrap();
}

#[test]
fn the_plugin_declares_the_vue_language_server_in_full_mode() {
    let servers = plugin_servers();
    let [vue] = servers.as_slice() else { panic!("{servers:?}") };
    assert_eq!(
        (vue.id.as_str(), vue.command.as_str(), vue.args.as_slice()),
        (SERVER, SERVER, &["--stdio".to_owned()][..])
    );
    assert!(vue.serves("App.vue") && !vue.serves("App.ts"));
    assert_eq!(vue.initialization_options["typescript"]["tsdk"], "${nodeModulesFolder:typescript/lib/typescript.js}");
    assert_eq!(vue.initialization_options["vue"]["hybridMode"], false);
    let install = vue.install.as_ref().unwrap();
    assert_eq!(install.package.split_whitespace().collect::<Vec<_>>(), ["@vue/language-server@2.2.12", "typescript@5"]);
}

#[test]
#[ignore = "needs the Vue language server: ZEN_VUE_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn problems_completion_navigation_documentation_rename_and_formatting_work_in_vue_files() {
    let prefix =
        PathBuf::from(std::env::var_os("ZEN_VUE_LANGUAGE_SERVER_PREFIX").expect("ZEN_VUE_LANGUAGE_SERVER_PREFIX"));
    let mut window = window(&prefix);
    let app = window.path("src/App.vue");
    window.run("file.document.open", json!({ "path": app }));

    let answer = window.until("problems.file.list", json!({ "path": app }), |answer| {
        !zen_workbench::language::listed_problems(answer).is_empty()
    });
    let problems = zen_workbench::language::listed_problems(&answer);
    let wrong = problems.iter().find(|problem| problem["message"].as_str().unwrap().contains("not assignable"));
    assert_eq!(wrong.unwrap_or_else(|| panic!("{answer}"))["line"], 14, "{answer}");
    assert_eq!(problems.iter().filter(|problem| problem["severity"] == "error").count(), 1, "{answer}");

    let at = APP.find("{{ greeting").unwrap() + 3;
    let shown =
        window.until("editor.documentation.show", json!({ "path": app, "offset": at, "waitMs": 2000 }), |answer| {
            answer["pending"] == false
        });
    assert!(shown["markdown"].as_str().unwrap_or_default().contains("greeting: string"), "{shown}");

    let at = APP.find("\"increment\"").unwrap() + 1;
    let found =
        window.until("navigate.declaration.goTo", json!({ "path": app, "offset": at, "waitMs": 2000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
    let found = found.to_string();
    assert!(found.contains("App.vue") && found.contains("\"line\":11"), "{found}");
    let at = APP.find("double(count").unwrap();
    let found =
        window.until("navigate.declaration.goTo", json!({ "path": app, "offset": at, "waitMs": 2000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
    assert!(found.to_string().contains("util.ts"), "{found}");

    window.run("file.document.open", json!({ "path": app }));
    let at = APP.find("{{ count").unwrap() + 3;
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.edit.type", json!({ "text": "gree" }));
    window.run("editor.completion.start", json!({}));
    let list = window.until("editor.completion.list", json!({}), |list| {
        list["items"].as_array().is_some_and(|items| items.iter().any(|item| item["label"] == "greeting"))
    });
    assert!(list["items"].as_array().unwrap().iter().any(|item| item["label"] == "greeting"), "{list}");
    window.run("editor.completion.cancel", json!({}));
    window.run("editor.edit.undo", json!({}));

    let at = APP.find("const count").unwrap() + 6;
    let renamed = window.until(
        "refactor.rename.apply",
        json!({ "path": app, "offset": at, "newName": "total", "waitMs": 5000 }),
        |answer| answer["edits"].as_u64().is_some_and(|edits| edits > 0),
    );
    assert!(renamed["edits"].as_u64().unwrap() >= 3, "{renamed}");
    let text = window.text(&app);
    assert!(text.contains("{{ total }}") && text.contains("total.value = double(total.value)"), "{text}");

    let messy = "<template>\n<p>{{ a }}</p>\n</template>\n\n<script setup lang=\"ts\">\nconst   a=1\n</script>\n";
    let messy_path = window.path("src/Messy.vue");
    std::fs::write(&messy_path, messy).unwrap();
    window.run("file.document.open", json!({ "path": messy_path }));
    let formatted = window.until("editor.code.reformat", json!({ "path": messy_path, "waitMs": 5000 }), |answer| {
        answer.get("pending") != Some(&json!(true))
    });
    let text = window.text(&messy_path);
    assert!(text.contains("const a = 1"), "{formatted} {text}");
}

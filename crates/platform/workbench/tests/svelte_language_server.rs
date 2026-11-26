use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration};

const SERVER: &str = "svelte-language-server";

const COUNTER: &str = r#"<script lang="ts">
  import { double } from './util'

  let count: number = $state(0)
  const greeting: string = 'Hello'
  const broken: number = 'not a number'
  function increment() {
    count = double(count)
  }
</script>

<button onclick={increment}>{greeting} {count}</button>
"#;

const UTIL: &str = "export function double(value: number): number {\n  return value * 2\n}\n";

const TSCONFIG: &str = r#"{ "compilerOptions": { "target": "ES2020", "module": "ESNext", "moduleResolution": "Bundler", "strict": true, "verbatimModuleSyntax": true }, "include": ["src/**/*"] }"#;

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

fn plugin_servers(plugin: &str) -> Vec<ServerDeclaration> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../plugins/{plugin}/plugin.json"));
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    ServerDeclaration::parse_list(&manifest["languageServers"].to_string()).unwrap()
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) {
    std::os::unix::fs::symlink(from, to).unwrap();
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) {
    std::os::windows::fs::symlink_dir(from, to).unwrap();
}

fn window(prefix: &Path, project: Option<&Path>) -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("src")).unwrap();
    for (name, text) in [("src/Counter.svelte", COUNTER), ("src/util.ts", UTIL), ("tsconfig.json", TSCONFIG)] {
        std::fs::write(folder.path().join(name), text).unwrap();
    }
    if let Some(project) = project {
        std::fs::create_dir_all(folder.path().join("node_modules")).unwrap();
        link(&project.join("node_modules/svelte"), &folder.path().join("node_modules/svelte"));
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
    servers.declare(plugin_servers("svelte"));
    servers.declare(plugin_servers("typescript"));
    let found = servers.executable(SERVER).expect("the server in the IDE's folder");
    assert!(found.starts_with(data.path().join("language-servers")), "{}", found.display());
    session.set_state(servers);
    Window { registry, session, folder, _data: data }
}

#[test]
fn the_plugin_declares_the_svelte_server_for_svelte_files_only() {
    let servers = plugin_servers("svelte");
    let [svelte] = servers.as_slice() else { panic!("{servers:?}") };
    assert_eq!(
        (svelte.id.as_str(), svelte.command.as_str(), svelte.args.as_slice()),
        (SERVER, "svelteserver", &["--stdio".to_owned()][..])
    );
    assert!(svelte.serves("Counter.svelte") && !svelte.serves("util.ts") && !svelte.serves("App.vue"));
    assert_eq!(svelte.language_for("Counter.svelte"), "svelte");
    let install = svelte.install.as_ref().unwrap();
    assert_eq!(install.package, "svelte-language-server@0.18.5 typescript@5");
    let typescript = &plugin_servers("typescript")[0];
    assert!(!typescript.serves("Counter.svelte"), "a component goes to the Svelte server only");
    assert!(typescript.serves("util.ts"), "the scripts of a Svelte project stay with the TypeScript server");
}

#[test]
#[ignore = "needs the Svelte language server: ZEN_SVELTE_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn problems_completion_navigation_documentation_rename_and_formatting_work_in_svelte_files() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_SVELTE_LANGUAGE_SERVER_PREFIX").expect("ZEN_SVELTE_LANGUAGE_SERVER_PREFIX"),
    );
    let project = std::env::var_os("ZEN_SVELTE_PROJECT").map(PathBuf::from);
    let mut window = window(&prefix, project.as_deref());
    let counter = window.path("src/Counter.svelte");
    window.run("file.document.open", json!({ "path": counter }));

    let answer = window.until("problems.file.list", json!({ "path": counter }), |answer| {
        zen_workbench::language::listed_problems(answer)
            .iter()
            .any(|problem| problem["message"].as_str().is_some_and(|message| message.contains("not assignable")))
    });
    let problems = zen_workbench::language::listed_problems(&answer);
    let wrong = problems.iter().find(|problem| problem["message"].as_str().unwrap().contains("not assignable"));
    assert_eq!(wrong.unwrap()["line"], 6, "{answer}");

    let at = COUNTER.find("{greeting}").unwrap() + 1;
    let shown =
        window.until("editor.documentation.show", json!({ "path": counter, "offset": at, "waitMs": 3000 }), |answer| {
            answer["pending"] == false
        });
    assert!(shown["markdown"].as_str().unwrap_or_default().contains("greeting: string"), "{shown}");

    let lookup = |window: &mut Window, command: &str, offset: usize| {
        window.until(command, json!({ "path": counter, "offset": offset, "waitMs": 3000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        })
    };
    let found = lookup(&mut window, "navigate.declaration.goTo", COUNTER.find("{increment}").unwrap() + 1).to_string();
    assert!(found.contains("Counter.svelte") && found.contains("\"line\":7"), "{found}");
    let found = lookup(&mut window, "navigate.declaration.goTo", COUNTER.find("double(count)").unwrap()).to_string();
    assert!(found.contains("util.ts"), "{found}");

    let usages = lookup(&mut window, "navigate.usages.find", COUNTER.find("let count").unwrap() + 4).to_string();
    assert!(usages.matches("Counter.svelte").count() >= 1 && usages.contains("\"line\":12"), "{usages}");

    let at = COUNTER.find("{count}").unwrap() + 1;
    window.run("file.document.open", json!({ "path": counter }));
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.edit.type", json!({ "text": "gree" }));
    window.run("editor.completion.start", json!({}));
    let list = window.until("editor.completion.list", json!({}), |list| {
        list["items"].as_array().is_some_and(|items| items.iter().any(|item| item["label"] == "greeting"))
    });
    assert!(list["items"].as_array().unwrap().iter().any(|item| item["label"] == "greeting"), "{list}");
    window.run("editor.completion.cancel", json!({}));
    window.run("editor.edit.undo", json!({}));

    let at = COUNTER.find("let count").unwrap() + 4;
    let renamed = window.until(
        "refactor.rename.apply",
        json!({ "path": counter, "offset": at, "newName": "total", "waitMs": 5000 }),
        |answer| answer["edits"].as_u64().is_some_and(|edits| edits > 0),
    );
    let text = window.text(&counter);
    assert!(text.contains("{total}</button>") && text.contains("total = double(total)"), "{renamed} {text}");

    let messy_path = window.path("src/Messy.svelte");
    std::fs::write(&messy_path, "<script>\nlet   a=1\n</script>\n\n<p>{a}</p>\n").unwrap();
    window.run("file.document.open", json!({ "path": messy_path }));
    window.until("editor.code.reformat", json!({ "path": messy_path, "waitMs": 5000 }), |answer| {
        answer.get("pending") != Some(&json!(true))
    });
    let formatted = window.text(&messy_path);
    assert!(formatted.contains("let a = 1"), "{formatted}");

    let list = window.run("lsp.servers.list", json!({}));
    let server = list["servers"].as_array().unwrap().iter().find(|server| server["id"] == SERVER).unwrap();
    assert_eq!(server["source"], "local", "{server}");
}

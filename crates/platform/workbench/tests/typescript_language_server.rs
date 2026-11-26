use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration};

const SERVER: &str = "typescript-language-server";

const SHAPES: &str = "export interface Shape {
  area(): number
}

export class Circle implements Shape {
  constructor(private radius: number) {}
  area(): number {
    return Math.PI * this.radius * this.radius
  }
}
";

const MAIN: &str = "import { Circle, type Shape } from './shapes'
import { unusedThing } from './unused'

/** Doubles a number. */
export function double(value: number): number {
  return value * 2
}

const shape: Shape = new Circle(2)
const total: number = double(shape.area())
const wrong: string = double(3)
console.log(total, wrong)
";

const UNUSED: &str = "export const unusedThing = 1\n";

const APP_JS: &str = "import { double } from './main'\n\nexport const four = double(2)\n";

const TSCONFIG: &str = r#"{ "compilerOptions": { "target": "ES2020", "module": "ESNext", "moduleResolution": "Bundler", "strict": true, "allowJs": true }, "include": ["src/**/*"] }"#;

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

    fn until(&mut self, command: &str, params: Value, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            self.run("language.servers.sync", json!({}));
            let answer = self.registry.dispatch(&mut self.session, command, params.clone());
            match answer {
                Ok(answer) if done(&answer) => return answer,
                answer => assert!(Instant::now() < deadline, "{command}: {answer:?}"),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn lookup(&mut self, command: &str, params: Value) -> Value {
        self.until(command, params, |answer| answer.get("pending") != Some(&json!(true)))
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

fn window(prefix: &Path) -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("src")).unwrap();
    for (name, text) in [
        ("src/shapes.ts", SHAPES),
        ("src/main.ts", MAIN),
        ("src/unused.ts", UNUSED),
        ("src/app.js", APP_JS),
        ("tsconfig.json", TSCONFIG),
    ] {
        std::fs::write(folder.path().join(name), text).unwrap();
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
    servers.declare(plugin_servers("typescript"));
    servers.declare(plugin_servers("javascript"));
    servers.declare(plugin_servers("vue"));
    let found = servers.executable(SERVER).expect("the server in the IDE's folder");
    assert!(found.starts_with(data.path().join("language-servers")), "{}", found.display());
    session.set_state(servers);
    Window { registry, session, folder, _data: data }
}

#[test]
fn both_plugins_declare_the_same_server_for_every_script_file_and_never_vue() {
    let typescript = plugin_servers("typescript");
    assert_eq!(typescript, plugin_servers("javascript"), "the first declaration wins, so they are the same");
    let [server] = typescript.as_slice() else { panic!("{typescript:?}") };
    assert_eq!((server.id.as_str(), server.command.as_str()), (SERVER, SERVER));
    for (file, language) in [
        ("a.ts", "typescript"),
        ("a.mts", "typescript"),
        ("a.cts", "typescript"),
        ("a.d.ts", "typescript"),
        ("A.tsx", "typescriptreact"),
        ("a.js", "javascript"),
        ("a.mjs", "javascript"),
        ("a.cjs", "javascript"),
        ("A.jsx", "javascriptreact"),
    ] {
        assert!(server.serves(file), "{file}");
        assert_eq!(server.language_for(file), language, "{file}");
    }
    assert!(!server.serves("App.vue"), "a component stays with the Vue server");
    assert!(plugin_servers("vue")[0].serves("App.vue") && !plugin_servers("vue")[0].serves("main.ts"));
    assert_eq!(server.initialization_options["tsserver"]["fallbackPath"], "${nodeModules:typescript/lib/tsserver.js}");
    let install = server.install.as_ref().unwrap();
    assert_eq!(install.package, "typescript-language-server@5.3.0 typescript@5");
}

fn text(answer: &Value) -> String {
    answer.to_string()
}

#[test]
#[ignore = "needs the TypeScript language server: ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn every_language_feature_works_in_typescript_and_javascript_files() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX").expect("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX"),
    );
    let mut window = window(&prefix);
    let main = window.path("src/main.ts");
    let shapes = window.path("src/shapes.ts");
    let app = window.path("src/app.js");
    window.run("file.document.open", json!({ "path": shapes }));
    window.run("file.document.open", json!({ "path": app }));
    window.run("file.document.open", json!({ "path": main }));

    let answer = window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer)
            .iter()
            .any(|problem| problem["message"].as_str().is_some_and(|message| message.contains("not assignable")))
    });
    let problems = zen_workbench::language::listed_problems(&answer);
    let wrong = problems.iter().find(|problem| problem["message"].as_str().unwrap().contains("not assignable"));
    assert_eq!(wrong.unwrap()["line"], 11, "{answer}");
    checked(&format!("problems: ok ({} in main.ts)", problems.len()));

    let at = MAIN.find("double(shape").unwrap();
    let shown = window.lookup("editor.documentation.show", json!({ "path": main, "offset": at, "waitMs": 2000 }));
    let markdown = shown["markdown"].as_str().unwrap_or_default();
    assert!(markdown.contains("double(value: number): number") && markdown.contains("Doubles a number"), "{shown}");
    checked("quick documentation: ok");

    let at = MAIN.find("new Circle").unwrap() + 4;
    let found = window.lookup("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 3000 }));
    assert!(text(&found).contains("shapes.ts"), "{found}");
    checked("go to declaration: ok");

    let at = MAIN.find("shape.area").unwrap();
    let found = window.lookup("navigate.typeDeclaration.goTo", json!({ "path": main, "offset": at, "waitMs": 3000 }));
    assert!(text(&found).contains("shapes.ts"), "{found}");
    checked("go to type declaration: ok");

    let at = SHAPES.find("area()").unwrap();
    let found = window.lookup("navigate.implementation.goTo", json!({ "path": shapes, "offset": at, "waitMs": 3000 }));
    assert!(text(&found).contains("shapes.ts") && text(&found).contains("\"line\":7"), "{found}");
    checked("go to implementation: ok");

    let at = MAIN.find("function double").unwrap() + 9;
    let usages = window.lookup(
        "navigate.usages.find",
        json!({ "path": main, "offset": at, "waitMs": 5000, "includeDeclaration": false }),
    );
    let usages_text = text(&usages);
    assert!(usages_text.contains("main.ts") && usages_text.contains("app.js"), "{usages}");
    checked("find usages: ok");

    let symbols = window.until("navigate.symbol.goTo", json!({ "query": "Circle", "waitMs": 5000 }), |answer| {
        text(answer).contains("shapes.ts")
    });
    assert!(text(&symbols).contains("Circle"), "{symbols}");
    checked("go to symbol: ok");

    let structure = window
        .until("editor.structure.get", json!({ "path": shapes, "waitMs": 3000 }), |answer| answer["status"] == "ready");
    let names = text(&structure["symbols"]);
    assert!(names.contains("Shape") && names.contains("Circle") && names.contains("area"), "{structure}");
    checked(&format!("structure: ok (source {})", structure["source"]));

    let at = MAIN.find("shape.area").unwrap() + "shape.ar".len();
    window.run("file.document.open", json!({ "path": main }));
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.completion.start", json!({}));
    let list = window.until("editor.completion.list", json!({}), |list| {
        list["items"].as_array().is_some_and(|items| items.iter().any(|item| item["label"] == "area"))
    });
    assert!(list["items"].as_array().unwrap().iter().any(|item| item["label"] == "area"), "{list}");
    window.run("editor.completion.cancel", json!({}));
    checked("completion: ok");

    let at = MAIN.find("wrong: string").unwrap();
    let actions =
        window.until("editor.intentions.list", json!({ "path": main, "offset": at, "waitMs": 3000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
    assert!(!actions["actions"].as_array().unwrap().is_empty(), "{actions}");
    checked(&format!("context actions: ok ({})", actions["actions"].as_array().unwrap().len()));

    let at = MAIN.find("shape.area").unwrap();
    let highlights =
        window.until("editor.highlights.get", json!({ "path": main, "offset": at, "waitMs": 3000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
    assert!(text(&highlights).matches("\"start\"").count() >= 2, "{highlights}");
    checked("highlight usages: ok");

    let at = MAIN.find("double(3)").unwrap() + "double(".len();
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.parameterInfo.show", json!({}));
    let info = window.until("editor.parameterInfo.state", json!({ "waitMs": 2000 }), |answer| {
        answer["pending"] == false && answer["signatures"].as_array().is_some_and(|list| !list.is_empty())
    });
    assert!(text(&info).contains("value: number"), "{info}");
    window.run("editor.parameterInfo.hide", json!({}));
    checked("parameter info: ok");

    let at = MAIN.find("function double").unwrap() + 9;
    window.run("navigate.hierarchy.call", json!({ "path": main, "offset": at, "waitMs": 5000 }));
    let hierarchy = window.until("navigate.hierarchy.get", json!({ "waitMs": 2000 }), |answer| {
        answer["pending"] == false && text(answer).contains("app.js")
    });
    checked(&format!("call hierarchy: ok ({} chars)", text(&hierarchy).len()));

    let optimized = window.until("editor.imports.optimize", json!({ "path": main, "waitMs": 5000 }), |answer| {
        answer.get("pending") != Some(&json!(true))
    });
    let after = window.text(&main);
    assert!(!after.contains("unusedThing"), "{optimized} {after}");
    checked("optimize imports: ok");

    let at = window.text(&main).find("function double").unwrap() + 9;
    let renamed = window.until(
        "refactor.rename.apply",
        json!({ "path": main, "offset": at, "newName": "twice", "waitMs": 5000 }),
        |answer| answer["edits"].as_u64().is_some_and(|edits| edits > 0),
    );
    let app_text = std::fs::read_to_string(&app).unwrap();
    let app_open = window.text(&app);
    assert!(window.text(&main).contains("twice(shape.area())"), "{renamed}");
    assert!(app_open.contains("twice(2)") || app_text.contains("twice(2)"), "{renamed} {app_open}");
    checked(&format!("rename: ok ({} edits)", renamed["edits"]));

    let messy_path = window.path("src/messy.ts");
    std::fs::write(&messy_path, "const   a={b:1}\nfunction f( x:number ){return x}\n").unwrap();
    window.run("file.document.open", json!({ "path": messy_path }));
    window.until("editor.code.reformat", json!({ "path": messy_path, "waitMs": 5000 }), |answer| {
        answer.get("pending") != Some(&json!(true))
    });
    let formatted = window.text(&messy_path);
    assert!(formatted.contains("const a = { b: 1 }"), "{formatted}");
    checked("reformat code: ok");

    let at = APP_JS.find("four").unwrap();
    let shown = window.lookup("editor.documentation.show", json!({ "path": app, "offset": at, "waitMs": 2000 }));
    assert!(shown["markdown"].as_str().unwrap_or_default().contains("four: number"), "{shown}");
    checked("javascript: ok");

    let list = window.run("lsp.servers.list", json!({}));
    let server = list["servers"].as_array().unwrap().iter().find(|server| server["id"] == SERVER).unwrap();
    assert_eq!(server["source"], "local", "{server}");
}

#[test]
#[ignore = "needs the TypeScript language server: ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn renaming_a_module_in_the_project_updates_its_imports() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX").expect("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX"),
    );
    let mut window = window(&prefix);
    let main = window.path("src/main.ts");
    let shapes = window.path("src/shapes.ts");
    window.run("file.document.open", json!({ "path": shapes }));
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| text(problem).contains("not assignable"))
    });
    window.run("file.fs.rename", json!({ "path": shapes, "name": "figures.ts" }));
    assert!(window.path("src/figures.ts").exists() && !shapes.exists());
    let after = window.text(&main);
    assert!(after.contains("from './figures'"), "{after}");
    checked("file rename updates imports: ok");
    assert_eq!(window.run("editor.edit.undo", json!(null)), json!(true));
    assert!(shapes.exists() && window.text(&main).contains("from './shapes'"));
    checked("undo moves it back: ok");
}

#[test]
#[ignore = "needs the TypeScript language server: ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn extract_and_inline_work_in_typescript() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX").expect("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX"),
    );
    let mut window = window(&prefix);
    let main = window.path("src/main.ts");
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| text(problem).contains("not assignable"))
    });
    for (command, needle, caret) in [
        ("refactor.extract.variable", "value * 2", false),
        ("refactor.extract.constant", "value * |2", false),
        ("refactor.extract.method", "value * 2", false),
        ("refactor.inline.apply", "shape.area", true),
    ] {
        let (skip, needle) = needle.split_once('|').unwrap_or(("", needle));
        let at = MAIN.find(&format!("{skip}{needle}")).unwrap() + skip.len();
        let start = MAIN[..at].chars().count();
        let end = if caret { start } else { start + needle.chars().count() };
        let mut params = json!({ "path": main, "start": start, "end": end, "waitMs": 10000 });
        let offered = |answer: &Value| {
            answer["title"].is_string() || answer["variants"].as_array().is_some_and(|variants| !variants.is_empty())
        };
        let mut answer = window.until(command, params.clone(), offered);
        if answer["title"].is_null() {
            checked(&format!("{command} variants: {}", answer["variants"]));
            params["variant"] = json!(0);
            answer = window.until(command, params, offered);
        }
        let changed = window.text(&main);
        assert_ne!(changed, MAIN, "{command}: {answer}");
        if command != "refactor.inline.apply" {
            let name = answer["name"].as_str().unwrap_or_else(|| panic!("{command}: {answer}\n{changed}"));
            assert!(answer["occurrences"].as_array().is_some_and(|all| all.len() >= 2), "{command}: {answer}");
            assert!(changed.contains(name), "{changed}");
        }
        checked(&format!("{command}: ok ({}, name {})", answer["title"], answer["name"]));
        window.run("editor.edit.undo", json!({}));
        assert_eq!(window.text(&main), MAIN, "{command}: one undo step");
    }
}

#[test]
#[ignore = "needs the TypeScript language server: ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn safe_delete_finds_usages_in_typescript_and_deletes_an_unused_declaration() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX").expect("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX"),
    );
    let mut window = window(&prefix);
    let main = window.path("src/main.ts");
    let unused = window.path("src/unused.ts");
    window.run("file.document.open", json!({ "path": unused }));
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| text(problem).contains("not assignable"))
    });
    let found = |answer: &Value| answer["pending"] == false && answer.get("error").is_none();
    let at = UNUSED.find("unusedThing").unwrap() + 2;
    let used = window.until("refactor.safeDelete.find", json!({ "path": unused, "offset": at, "waitMs": 5000 }), found);
    assert_eq!(used["total"], 1, "{used}");
    assert_eq!(used["usages"][0]["path"], "src/main.ts", "{used}");
    checked("safe delete finds the import: ok");
    let file = window.until("refactor.safeDelete.find", json!({ "paths": [unused], "waitMs": 5000 }), found);
    assert_eq!(file["total"], 1, "{file}");
    checked("safe delete of a file: ok");
    let at = MAIN.find("double(value").unwrap() + 1;
    let double = window.until("refactor.safeDelete.find", json!({ "path": main, "offset": at, "waitMs": 5000 }), found);
    assert!(double["total"].as_u64().unwrap_or(0) >= 1, "{double}");
    window.run("refactor.safeDelete.apply", json!({ "path": main, "offset": at, "force": true }));
    let changed = window.text(&main);
    assert!(!changed.contains("Doubles a number") && !changed.contains("function double"), "{changed}");
    checked("safe delete anyway removes the function and its doc comment: ok");
    window.run("editor.edit.undo", json!({}));
    assert_eq!(window.text(&main), MAIN, "one undo step");
}

#[allow(clippy::print_stderr, reason = "the list of the features checked is what the test reports")]
fn checked(what: &str) {
    eprintln!("{what}");
}

const FAKE_TS_PLUGIN: &str = r#"module.exports = function init() {
  return {
    create(info) {
      const service = info.languageService
      const proxy = Object.create(null)
      for (const key of Object.keys(service)) proxy[key] = (...args) => service[key](...args)
      proxy.getSemanticDiagnostics = (file) => [
        ...service.getSemanticDiagnostics(file),
        { file: service.getProgram().getSourceFile(file), start: 0, length: 1, category: 1, code: 99999,
          messageText: 'zen-fake-ts-plugin was here' },
      ]
      return proxy
    },
  }
}
"#;

#[test]
#[ignore = "needs the TypeScript language server: ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX=<folder> (see the module's documentation)"]
fn a_typescript_plugin_named_in_tsconfig_loads_with_the_projects_typescript() {
    let prefix = PathBuf::from(
        std::env::var_os("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX").expect("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX"),
    );
    let mut window = window(&prefix);
    let modules = window.path("node_modules");
    std::fs::create_dir_all(modules.join("zen-fake-ts-plugin")).unwrap();
    copy_folder(&prefix.join("node_modules/typescript"), &modules.join("typescript"));
    std::fs::write(modules.join("zen-fake-ts-plugin/index.js"), FAKE_TS_PLUGIN).unwrap();
    std::fs::write(
        modules.join("zen-fake-ts-plugin/package.json"),
        r#"{ "name": "zen-fake-ts-plugin", "main": "index.js" }"#,
    )
    .unwrap();
    let tsconfig =
        TSCONFIG.replace(r#""allowJs": true }"#, r#""allowJs": true, "plugins": [{ "name": "zen-fake-ts-plugin" }] }"#);
    assert_ne!(tsconfig, TSCONFIG);
    std::fs::write(window.path("tsconfig.json"), tsconfig).unwrap();
    let main = window.path("src/main.ts");
    window.run("file.document.open", json!({ "path": main }));
    let answer = window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| {
            problem["message"].as_str().is_some_and(|message| message.contains("zen-fake-ts-plugin was here"))
        })
    });
    checked(&format!("tsconfig plug-in: ok ({} problems)", zen_workbench::language::listed_problems(&answer).len()));
}

fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

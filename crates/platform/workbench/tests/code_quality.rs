use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_project::support::ProjectSupport;
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration};
use zen_workbench::plugin_projects::{PluginProject, PluginProjects};

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
        let deadline = Instant::now() + Duration::from_secs(60);
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

    fn problems(&mut self, path: &Path, done: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let answer = self.until("problems.file.list", json!({ "path": path }), |answer| {
            done(&zen_workbench::language::listed_problems(answer))
        });
        zen_workbench::language::listed_problems(&answer)
    }
}

fn manifest(plugin: &str) -> Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../plugins/{plugin}/plugin.json"));
    serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap()
}

fn servers_of(plugin: &str) -> Vec<ServerDeclaration> {
    let declared = manifest(plugin)["languageServers"].clone();
    if declared.is_null() { Vec::new() } else { ServerDeclaration::parse_list(&declared.to_string()).unwrap() }
}

fn project_of(plugin: &str) -> PluginProject {
    let manifest = manifest(plugin);
    PluginProject {
        id: plugin.to_owned(),
        name: manifest["name"].as_str().unwrap().to_owned(),
        support: ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
    }
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) {
    std::os::unix::fs::symlink(from, to).unwrap();
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) {
    std::os::windows::fs::symlink_dir(from, to).unwrap();
}

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name}")))
}

fn window(plugin: &str, files: &[(&str, &str)], tools: &Path, server_prefix: Option<&Path>) -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    link(&tools.join("node_modules"), &folder.path().join("node_modules"));
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("language-servers")).unwrap();
    let declared = servers_of(plugin);
    if let (Some(prefix), Some(server)) = (server_prefix, declared.first()) {
        link(prefix, &data.path().join("language-servers").join(&server.id));
    }
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        std::sync::Arc::new(zen_workbench::language::servers::SystemTools),
    );
    servers.declare(declared);
    session.set_state(servers);
    session.state_mut::<PluginProjects>().set(vec![project_of(plugin)]);
    Window { registry, session, folder, _data: data }
}

#[allow(clippy::print_stderr, reason = "the list of the features checked is what the test reports")]
fn checked(what: &str) {
    eprintln!("{what}");
}

#[test]
fn the_three_plugins_are_offered_by_their_projects() {
    let eslint = project_of("eslint").support;
    assert!(eslint.markers.files.iter().any(|file| file == "eslint.config.mjs"));
    assert!(eslint.markers.packages.iter().any(|package| package == "eslint"));
    let prettier = project_of("prettier").support;
    assert_eq!(prettier.formatters[0].command, "prettier");
    assert!(prettier.formatters[0].formats("App.vue") && !prettier.formatters[0].formats("main.go"));
    let biome = &servers_of("biome")[0];
    assert!(biome.from_project && biome.alongside && biome.formats);
    let eslint_server = &servers_of("eslint")[0];
    assert!(eslint_server.alongside && !eslint_server.formats && !eslint_server.from_project);
    assert!(eslint_server.serves("App.vue") && eslint_server.serves("a.tsx") && !eslint_server.serves("a.css"));
}

const ESLINT_CONFIG: &str = "import js from \"@eslint/js\";
export default [
  js.configs.recommended,
  { rules: { \"no-unused-vars\": \"error\", \"semi\": [\"error\", \"always\"], \"eqeqeq\": \"error\" } },
];
";

const A_JS: &str = "const unused = 1\nlet x = 2;\nif (x == 1) { x = 3; }\n";

#[test]
#[ignore = "needs ESLint: ZEN_ESLINT_LANGUAGE_SERVER_PREFIX and ZEN_ESLINT_PROJECT (see the module's documentation)"]
fn eslint_problems_quick_fixes_fix_all_and_fixes_on_save() {
    let prefix = env("ZEN_ESLINT_LANGUAGE_SERVER_PREFIX");
    let project = env("ZEN_ESLINT_PROJECT");
    let files =
        [("eslint.config.mjs", ESLINT_CONFIG), ("src/a.js", A_JS), ("package.json", "{ \"type\": \"module\" }")];
    let mut window = window("eslint", &files, &project, Some(&prefix));
    let a = window.path("src/a.js");
    window.run("file.document.open", json!({ "path": a }));

    let problems = window.problems(&a, |problems| problems.iter().any(|problem| problem["code"] == "eqeqeq"));
    let codes: Vec<&str> = problems.iter().filter_map(|problem| problem["code"].as_str()).collect();
    assert!(codes.contains(&"no-unused-vars") && codes.contains(&"semi") && codes.contains(&"eqeqeq"), "{problems:?}");
    assert!(problems.iter().all(|problem| problem["source"] == "eslint"));
    checked(&format!("eslint problems: {codes:?}"));

    let at = A_JS.find('\n').unwrap();
    let list = window.until("editor.intentions.list", json!({ "path": a, "offset": at, "waitMs": 3000 }), |answer| {
        answer["pending"] == false
    });
    let titles: Vec<&str> =
        list["actions"].as_array().unwrap().iter().filter_map(|action| action["title"].as_str()).collect();
    assert!(titles.iter().any(|title| title.contains("Disable semi for this line")), "{list}");
    let fix =
        list["actions"].as_array().unwrap().iter().find(|action| action["title"] == "Fix this semi problem").unwrap();
    window.run("editor.intentions.apply", json!({ "id": fix["id"], "waitMs": 5000 }));
    assert!(window.text(&a).starts_with("const unused = 1;\n"), "{}", window.text(&a));
    checked(&format!("eslint quick fixes: {titles:?}; applied Fix this semi problem"));

    std::fs::write(window.path("src/b.js"), "let y = 1\nexport { y }\n").unwrap();
    let b = window.path("src/b.js");
    window.run("file.document.open", json!({ "path": b }));
    window.problems(&b, |problems| problems.iter().any(|problem| problem["code"] == "semi"));
    let fixed = window.run("editor.code.fixAll", json!({ "path": b, "waitMs": 5000 }));
    assert_eq!(fixed["changed"], true, "{fixed}");
    assert_eq!(window.text(&b), "let y = 1;\nexport { y };\n");
    checked("fix eslint problems: ok");

    window.session.settings_mut().set("codeQuality.eslint.fixOnSave", true).unwrap();
    window.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    window.run("editor.edit.type", json!({ "text": "const z = 2\nexport { z }\n" }));
    window.problems(&b, |problems| problems.iter().any(|problem| problem["code"] == "semi"));
    window.run("file.document.save", json!({}));
    let saved = std::fs::read_to_string(&b).unwrap();
    assert!(saved.starts_with("const z = 2;\nexport { z };\n"), "{saved}");
    checked("eslint --fix on save: ok");
}

#[test]
#[ignore = "needs Prettier: ZEN_PRETTIER_PROJECT (see the module's documentation)"]
fn prettier_reformats_on_reformat_code_and_on_save_and_respects_its_ignore_file() {
    let project = env("ZEN_PRETTIER_PROJECT");
    let files = [
        (".prettierrc", "{ \"semi\": false, \"singleQuote\": true }\n"),
        (".prettierignore", "src/ignored.ts\n"),
        ("src/a.ts", "const   a = {b:\"x\"};\n"),
        ("src/ignored.ts", "const   kept = 1;\n"),
        ("src/style.css", "a{color:red}\n"),
    ];
    let mut window = window("prettier", &files, &project, None);
    let a = window.path("src/a.ts");
    window.run("file.document.open", json!({ "path": a }));
    let result =
        window.until("editor.code.reformat", json!({ "path": a, "waitMs": 5000 }), |answer| answer["pending"] == false);
    assert_eq!(result["changed"], true, "{result}");
    assert_eq!(window.text(&a), "const a = { b: 'x' }\n");
    checked("prettier reformat code: ok");

    let css = window.path("src/style.css");
    window.run("file.document.open", json!({ "path": css }));
    window.until("editor.code.reformat", json!({ "path": css, "waitMs": 5000 }), |answer| answer["pending"] == false);
    assert_eq!(window.text(&css), "a {\n  color: red;\n}\n");
    checked("prettier on css: ok");

    let ignored = window.path("src/ignored.ts");
    window.run("file.document.open", json!({ "path": ignored }));
    let result = window
        .until("editor.code.reformat", json!({ "path": ignored, "waitMs": 5000 }), |answer| answer["pending"] == false);
    assert_eq!(window.text(&ignored), "const   kept = 1;\n", "{result}");
    checked("prettier respects .prettierignore: ok");

    window.session.settings_mut().set("codeQuality.prettier.onSave", true).unwrap();
    window.run("file.document.open", json!({ "path": a }));
    window.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    window.run("editor.edit.type", json!({ "text": "let   b=1;\n" }));
    window.run("file.document.save", json!({}));
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "let b = 1\nconst a = { b: 'x' }\n");
    checked("prettier on save: ok");
}

#[test]
#[ignore = "needs Biome: ZEN_BIOME_PROJECT (see the module's documentation)"]
fn biome_problems_fixes_and_formatting() {
    let project = env("ZEN_BIOME_PROJECT");
    let config = std::fs::read_to_string(project.join("biome.json")).unwrap();
    let files = [
        ("biome.json", config.as_str()),
        ("src/a.ts", "let x = 1;\nlet y = 3;\nif (x == 1) {\n  x = 2;\n}\nexport { x, y };\n"),
    ];
    let mut window = window("biome", &files, &project, None);
    let a = window.path("src/a.ts");
    window.run("file.document.open", json!({ "path": a }));
    let list = window.run("lsp.servers.list", json!({}));
    let biome = list["servers"].as_array().unwrap().iter().find(|server| server["id"] == "biome").unwrap().clone();
    assert_eq!(biome["source"], "project", "{biome}");

    let problems = window.problems(&a, |problems| {
        problems.iter().any(|problem| problem["code"].as_str().is_some_and(|code| code.contains("noDoubleEquals")))
    });
    checked(&format!(
        "biome problems: {:?}",
        problems.iter().filter_map(|problem| problem["code"].as_str()).collect::<Vec<_>>()
    ));

    let at = window.text(&a).find("==").unwrap();
    let list = window.until("editor.intentions.list", json!({ "path": a, "offset": at, "waitMs": 3000 }), |answer| {
        answer["pending"] == false
    });
    let fix = list["actions"].as_array().unwrap().iter().find(|action| action["title"] == "Use === instead.").cloned();
    let fix = fix.unwrap_or_else(|| panic!("{list}"));
    window.run("editor.intentions.apply", json!({ "id": fix["id"], "waitMs": 5000 }));
    assert!(window.text(&a).contains("x === 1"), "{}", window.text(&a));
    checked("biome quick fix: ok");

    let result =
        window.until("editor.code.reformat", json!({ "path": a, "waitMs": 5000 }), |answer| answer["pending"] == false);
    assert!(window.text(&a).contains("\n\tx = 2;"), "Biome's tabs: {result} {}", window.text(&a));
    checked("biome reformat code: ok");

    let fixed = window.run("editor.code.fixAll", json!({ "path": a, "waitMs": 5000 }));
    assert!(window.text(&a).contains("const y = 3;"), "a safe fix: {fixed} {}", window.text(&a));
    checked("biome safe fixes: ok");
}

const TS_CONFIG: &str = "import js from \"@eslint/js\";
import tseslint from \"typescript-eslint\";
export default [
  js.configs.recommended,
  ...tseslint.configs.recommended.map((c) => ({ ...c, files: [\"**/*.ts\"] })),
  { rules: { \"eqeqeq\": \"error\" } },
];
";

#[test]
#[ignore = "needs ESLint with typescript-eslint and the TypeScript server (see the function's documentation)"]
fn eslint_beside_the_typescript_server_on_one_file() {
    let prefix = env("ZEN_ESLINT_LANGUAGE_SERVER_PREFIX");
    let ts_prefix = env("ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX");
    let project = env("ZEN_ESLINT_PROJECT");
    let source = "const total: number = 'one';\nif (total == 1) {\n  console.log(total);\n}\n";
    let files = [("eslint.config.mjs", TS_CONFIG), ("src/a.ts", source), ("package.json", "{ \"type\": \"module\" }")];
    let mut window = window("eslint", &files, &project, Some(&prefix));
    let data = window._data.path().to_owned();
    link(&ts_prefix, &data.join("language-servers/typescript-language-server"));
    window.session.state::<LanguageServers>().unwrap().declare(servers_of("typescript"));
    let a = window.path("src/a.ts");
    window.run("file.document.open", json!({ "path": a }));
    let problems = window.problems(&a, |problems| {
        problems.iter().any(|problem| problem["source"] == "eslint")
            && problems.iter().any(|problem| problem["source"] == "typescript")
    });
    let found: Vec<String> = problems
        .iter()
        .map(|problem| {
            format!(
                "{}:{}",
                problem["source"].as_str().unwrap_or_default(),
                problem["code"].as_str().unwrap_or_default()
            )
        })
        .collect();
    assert!(found.iter().any(|problem| problem == "eslint:eqeqeq"), "{found:?}");
    checked(&format!("both servers' problems on one file: {found:?}"));
    let at = source.find("==").unwrap();
    let list = window.until("editor.intentions.list", json!({ "path": a, "offset": at, "waitMs": 5000 }), |answer| {
        answer["pending"] == false
    });
    let titles: Vec<&str> =
        list["actions"].as_array().unwrap().iter().filter_map(|action| action["title"].as_str()).collect();
    assert!(titles.iter().any(|title| title.contains("eqeqeq")), "{list}");
    checked(&format!("context actions of both: {titles:?}"));
    let quick_doc = window.until(
        "editor.documentation.show",
        json!({ "path": a, "offset": source.find("total").unwrap(), "waitMs": 3000 }),
        |answer| answer["pending"] == false,
    );
    assert!(quick_doc["markdown"].as_str().unwrap_or_default().contains("total: number"), "{quick_doc}");
    checked("lookups still go to the TypeScript server: ok");
}

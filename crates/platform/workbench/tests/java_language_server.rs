use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration};

const SERVER: &str = "jdtls";

const SHAPE: &str = "package com.acme.shop;

public interface Shape {
    double area();
}
";

const CIRCLE: &str = "package com.acme.shop;

public record Circle(double radius) implements Shape {
    @Override
    public double area() {
        return Math.PI * radius * radius;
    }
}
";

const MAIN: &str = "package com.acme.shop;

import java.util.List;
import java.util.ArrayList;

public class Main {
    /** Doubles a number. */
    static double twice(double value) {
        return value * 2;
    }

    public static void main(String[] args) {
        Shape shape = new Circle(2);
        double total = twice(shape.area());
        String wrong = twice(3);
        System.out.println(total + wrong);
    }
}
";

const POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>maven-shop</artifactId>
  <version>1.0</version>
  <properties>
    <maven.compiler.release>21</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
</project>
"#;

const SETTINGS_GRADLE: &str = "rootProject.name = 'gradle-shop'\n";

const BUILD_GRADLE: &str =
    "plugins {\n    id 'java'\n}\n\njava {\n    sourceCompatibility = JavaVersion.VERSION_21\n}\n";

struct Window {
    registry: Registry,
    session: Session,
    _folder: tempfile::TempDir,
    root: PathBuf,
    data: tempfile::TempDir,
}

impl Window {
    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn text(&self, path: &Path) -> String {
        let workbench = self.session.state::<zen_workbench::Workbench>().unwrap();
        let document = workbench.documents().iter().find(|document| document.path() == path);
        document.map(|document| document.text().to_owned()).unwrap_or_default()
    }

    fn until(&mut self, command: &str, params: Value, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(150);
        loop {
            self.run("language.servers.sync", json!({}));
            let answer = self.registry.dispatch(&mut self.session, command, params.clone());
            match answer {
                Ok(answer) if done(&answer) => return answer,
                answer => {
                    if Instant::now() >= deadline {
                        let log = self.registry.dispatch(&mut self.session, "lsp.server.log", json!({ "id": SERVER }));
                        let log: Vec<String> = log
                            .ok()
                            .and_then(|log| log["lines"].as_array().cloned())
                            .unwrap_or_default()
                            .iter()
                            .rev()
                            .take(80)
                            .rev()
                            .map(|line| line.as_str().unwrap_or_default().chars().take(400).collect())
                            .collect();
                        let log = log.join("\n");
                        let list = self.registry.dispatch(&mut self.session, "lsp.servers.list", json!({}));
                        let _ = list;
                        panic!("{command}: {answer:?}\n{log}");
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn lookup(&mut self, command: &str, params: Value) -> Value {
        self.until(command, params, |answer| answer.get("pending") != Some(&json!(true)))
    }

    fn imported(&self) -> Vec<String> {
        let caches = self.data.path().join("caches/language-servers").join(SERVER);
        let mut names = Vec::new();
        for project in std::fs::read_dir(caches).into_iter().flatten().flatten() {
            let projects = project.path().join("workspace/.metadata/.plugins/org.eclipse.core.resources/.projects");
            for entry in std::fs::read_dir(projects).into_iter().flatten().flatten() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names
    }
}

fn plugin_servers() -> Vec<ServerDeclaration> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/java/plugin.json");
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

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} (see the module's documentation)")))
}

fn window(files: &[(&str, &str)], settings: Value) -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("language-servers")).unwrap();
    link(&env_path("ZEN_JDTLS"), &data.path().join("language-servers").join(SERVER));
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let root = folder.path().canonicalize().unwrap();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        Arc::new(zen_workbench::language::servers::SystemTools),
    );
    let mut declared = plugin_servers();
    merge(&mut declared[0].initialization_options["settings"], settings);
    servers.declare(declared);
    let java = env_path("ZEN_JDTLS_JAVA_HOME");
    servers.set_java_finder(Arc::new(move |_, _| Some(java.clone())));
    let found = servers.executable(SERVER).expect("the server in the IDE's folder");
    assert!(found.starts_with(data.path().join("language-servers")), "{}", found.display());
    session.set_state(servers);
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    Window { registry, session, _folder: folder, root, data }
}

fn merge(into: &mut Value, from: Value) {
    match (into, from) {
        (Value::Object(into), Value::Object(from)) => {
            for (key, value) in from {
                merge(into.entry(key).or_insert(Value::Null), value);
            }
        }
        (into, from) => *into = from,
    }
}

fn sources(build: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = build.to_vec();
    files.extend([
        ("src/main/java/com/acme/shop/Shape.java", SHAPE),
        ("src/main/java/com/acme/shop/Circle.java", CIRCLE),
        ("src/main/java/com/acme/shop/Main.java", MAIN),
    ]);
    files
}

fn text(answer: &Value) -> String {
    answer.to_string()
}

#[allow(clippy::print_stderr, reason = "the list of the features checked is what the test reports")]
fn checked(what: &str) {
    eprintln!("{what}");
}

#[test]
fn the_plugin_declares_the_server_unpacked_from_its_archive_and_run_with_java() {
    let servers = plugin_servers();
    let [server] = servers.as_slice() else { panic!("{servers:?}") };
    assert_eq!(server.id, SERVER);
    assert!(server.serves("Main.java") && !server.serves("pom.xml"));
    assert_eq!(server.command, "${javaHome}/bin/java");
    assert_eq!(server.java, Some(25), "JDT LS 1.63 runs on Java 25");
    let install = server.install.as_ref().unwrap();
    assert_eq!(install.kind, zen_workbench::language::servers::InstallKind::Archive);
    assert!(install.url.starts_with("https://download.eclipse.org/jdtls/milestones/"), "{}", install.url);
    assert_eq!(install.sha256.len(), 64);
    assert!(server.args.iter().any(|arg| arg == "${workspaceData}/workspace"), "the data is in the IDE's cache");
    assert!(
        server.watches.iter().any(|name| name == "pom.xml") && server.watches.iter().any(|name| name == "build.gradle")
    );
    let contents = server.virtual_documents.as_ref().expect("library classes open through the server");
    assert!(contents.serves("jdt://contents/rt.jar/java.lang/String.class") && !contents.serves("file:///a.java"));
    assert_eq!((contents.request.as_str(), contents.extension.as_str()), ("java/classFileContents", "java"));
    assert_eq!(server.initialization_options["extendedClientCapabilities"]["classFileContentsSupport"], true);
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn every_language_feature_works_in_a_maven_project() {
    let files = sources(&[("pom.xml", POM)]);
    let repository = tempfile::tempdir().unwrap();
    let repository = std::env::var_os("ZEN_MAVEN_REPO").map_or(repository.path().to_owned(), PathBuf::from);
    let user_settings = repository.join("zen-test-settings.xml");
    std::fs::create_dir_all(&repository).unwrap();
    std::fs::write(
        &user_settings,
        format!(
            "<settings><localRepository>{}</localRepository></settings>\n",
            repository.join("repository").display()
        ),
    )
    .unwrap();
    let mut window =
        window(&files, json!({ "java": { "configuration": { "maven": { "userSettings": user_settings } } } }));
    let main = window.path("src/main/java/com/acme/shop/Main.java");
    let shape = window.path("src/main/java/com/acme/shop/Shape.java");
    let circle = window.path("src/main/java/com/acme/shop/Circle.java");
    window.run("file.document.open", json!({ "path": shape }));
    window.run("file.document.open", json!({ "path": circle }));
    window.run("file.document.open", json!({ "path": main }));

    let answer = window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer)
            .iter()
            .any(|problem| problem["message"].as_str().is_some_and(|message| message.contains("Type mismatch")))
    });
    let problems = zen_workbench::language::listed_problems(&answer);
    let wrong = problems.iter().find(|problem| problem["message"].as_str().unwrap().contains("Type mismatch"));
    assert_eq!(wrong.unwrap()["line"], 15, "{answer}");
    checked(&format!("problems: ok ({} in Main.java)", problems.len()));
    let imported = window.imported();
    assert!(imported.iter().any(|name| name == "maven-shop"), "the Maven project is imported: {imported:?}");
    checked(&format!("maven import: ok ({imported:?})"));

    let at = MAIN.find("twice(shape").unwrap();
    let shown = window.lookup("editor.documentation.show", json!({ "path": main, "offset": at, "waitMs": 5000 }));
    let markdown = shown["markdown"].as_str().unwrap_or_default();
    assert!(markdown.contains("twice") && markdown.contains("Doubles a number"), "{shown}");
    checked("quick documentation: ok");

    let at = MAIN.find("new Circle").unwrap() + 4;
    let found = window.lookup("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 5000 }));
    assert!(text(&found).contains("Circle.java"), "{found}");
    checked("go to declaration: ok");

    let at = MAIN.find("ArrayList;").unwrap() + 2;
    let found = window.lookup("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 8000 }));
    assert_eq!(found["navigated"], true, "{found}");
    let library =
        window.session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap().path().to_owned();
    assert!(library.ends_with("ArrayList.java"), "{}", library.display());
    assert!(window.text(&library).contains("class ArrayList"), "{}", window.text(&library));
    let read_only = window.session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap().read_only();
    assert!(read_only.is_some(), "a library class cannot be edited");
    let at = window.text(&library).find("AbstractList").unwrap() + 2;
    let found = window.lookup("navigate.declaration.goTo", json!({ "path": library, "offset": at, "waitMs": 8000 }));
    assert_eq!(found["navigated"], true, "navigation goes on from inside a library class: {found}");
    checked("go to declaration into the JDK: ok");

    let at = MAIN.find("shape.area").unwrap();
    let found = window.lookup("navigate.typeDeclaration.goTo", json!({ "path": main, "offset": at, "waitMs": 5000 }));
    assert!(text(&found).contains("Shape.java"), "{found}");
    checked("go to type declaration: ok");

    let at = SHAPE.find("area()").unwrap();
    let found = window.lookup("navigate.implementation.goTo", json!({ "path": shape, "offset": at, "waitMs": 5000 }));
    assert!(text(&found).contains("Circle.java"), "{found}");
    checked("go to implementation: ok");

    let at = MAIN.find("double twice").unwrap() + 7;
    let usages = window.lookup(
        "navigate.usages.find",
        json!({ "path": main, "offset": at, "waitMs": 8000, "includeDeclaration": false }),
    );
    assert!(text(&usages).matches("Main.java").count() >= 1, "{usages}");
    checked("find usages: ok");

    let symbols = window.until("navigate.symbol.goTo", json!({ "query": "Circle", "waitMs": 5000 }), |answer| {
        text(answer).contains("Circle.java")
    });
    assert!(text(&symbols).contains("Circle"), "{symbols}");
    checked("go to symbol: ok");

    let at = MAIN.find("shape.area").unwrap() + "shape.ar".len();
    window.run("file.document.open", json!({ "path": main }));
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.completion.start", json!({}));
    let list = window.until("editor.completion.list", json!({}), |list| {
        list["items"].as_array().is_some_and(|items| items.iter().any(|item| text(&item["label"]).contains("area")))
    });
    checked(&format!("completion: ok ({} items)", list["items"].as_array().unwrap().len()));
    window.run("editor.completion.cancel", json!({}));

    let at = MAIN.find("String wrong").unwrap() + "String wrong = ".len();
    let actions = window.lookup("editor.intentions.list", json!({ "path": main, "offset": at, "waitMs": 5000 }));
    assert!(!actions["actions"].as_array().unwrap().is_empty(), "{actions}");
    checked(&format!("quick fixes: ok ({})", actions["actions"].as_array().unwrap().len()));

    let at = MAIN.find("double twice").unwrap() + 7;
    window.run("navigate.hierarchy.call", json!({ "path": main, "offset": at, "waitMs": 8000 }));
    let hierarchy = window.until("navigate.hierarchy.get", json!({ "waitMs": 2000 }), |answer| {
        answer["pending"] == false && text(answer).contains("main")
    });
    checked(&format!("call hierarchy: ok ({} chars)", text(&hierarchy).len()));

    let at = SHAPE.find("Shape").unwrap();
    window.run("navigate.hierarchy.type", json!({ "path": shape, "offset": at, "waitMs": 8000 }));
    let hierarchy = window.until("navigate.hierarchy.get", json!({ "waitMs": 2000 }), |answer| {
        answer["pending"] == false && text(answer).contains("Circle")
    });
    checked(&format!("type hierarchy: ok ({} chars)", text(&hierarchy).len()));

    window.until("editor.imports.optimize", json!({ "path": main, "waitMs": 8000 }), |answer| {
        answer.get("pending") != Some(&json!(true))
    });
    let after = window.text(&main);
    assert!(!after.contains("import java.util.ArrayList"), "{after}");
    checked("optimize imports: ok");

    let at = window.text(&circle).find("double area").unwrap() + 7;
    let renamed = window.until(
        "refactor.rename.apply",
        json!({ "path": circle, "offset": at, "newName": "surface", "waitMs": 8000 }),
        |answer| answer["edits"].as_u64().is_some_and(|edits| edits > 0),
    );
    let shape_text = window.text(&shape) + &std::fs::read_to_string(&shape).unwrap();
    assert!(window.text(&main).contains("shape.surface()"), "{renamed}");
    assert!(shape_text.contains("double surface()"), "{renamed}");
    checked(&format!("rename: ok ({} edits)", renamed["edits"]));

    let messy = window.path("src/main/java/com/acme/shop/Messy.java");
    std::fs::write(&messy, "package com.acme.shop;\nclass Messy{int a=1;void f( int x ){if(x>0){a=x;}}}\n").unwrap();
    window.run("file.document.open", json!({ "path": messy }));
    window.until("editor.code.reformat", json!({ "path": messy, "waitMs": 8000 }), |answer| {
        answer.get("pending") != Some(&json!(true))
    });
    let formatted = window.text(&messy);
    assert!(formatted.contains("int a = 1;") && formatted.contains("void f(int x) {"), "{formatted}");
    checked("reformat code: ok");

    let list = window.run("lsp.servers.list", json!({}));
    let server = list["servers"].as_array().unwrap().iter().find(|server| server["id"] == SERVER).unwrap();
    assert_eq!(server["source"], "local", "{server}");
    assert!(!window.root.join(".metadata").exists(), "no server data in the project");
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn renaming_a_class_renames_its_file_and_renaming_a_file_renames_its_class() {
    let files = sources(&[("pom.xml", POM)]);
    let mut window = window(&files, json!({}));
    let main = window.path("src/main/java/com/acme/shop/Main.java");
    let circle = window.path("src/main/java/com/acme/shop/Circle.java");
    let round = window.path("src/main/java/com/acme/shop/Round.java");
    window.run("file.document.open", json!({ "path": circle }));
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| text(problem).contains("Type mismatch"))
    });
    let at = CIRCLE.find("Circle(").unwrap() + 2;
    let preview = window.until(
        "refactor.rename.preview",
        json!({ "path": circle, "offset": at, "newName": "Round", "waitMs": 8000 }),
        |answer| answer["pending"] == false && answer["usageCount"].as_u64().is_some_and(|count| count > 0),
    );
    let operations = preview["operations"].as_array().unwrap();
    assert!(
        operations.iter().any(|operation| operation["kind"] == "rename" && operation["newPath"] == json!(round)),
        "{preview}"
    );
    window.run("refactor.rename.apply", json!({}));
    assert!(round.exists() && !circle.exists(), "{preview}");
    assert!(window.text(&round).contains("record Round("), "the moved tab carries the change");
    assert!(window.text(&main).contains("new Round(2)"));
    checked("class rename renames the file: ok");
    window.run("file.document.save", json!({ "path": round }));
    window.run("file.document.save", json!({ "path": main }));

    let shape = window.path("src/main/java/com/acme/shop/Shape.java");
    let figure = window.path("src/main/java/com/acme/shop/Figure.java");
    window.run("file.document.open", json!({ "path": shape }));
    window.until(
        "refactor.rename.prepare",
        json!({ "path": shape, "offset": SHAPE.find("Shape").unwrap(), "waitMs": 5000 }),
        |answer| answer["pending"] == false,
    );
    window.run("file.fs.rename", json!({ "path": shape, "name": "Figure.java" }));
    assert!(figure.exists() && !shape.exists());
    let main_text = window.text(&main);
    let figure_text = window.text(&figure);
    assert!(figure_text.contains("interface Figure"), "{figure_text}");
    let messages = window.session.state_mut::<zen_workbench::Workbench>().take_messages();
    let told = messages.iter().any(|message| message.contains("Shape") && message.contains("Main.java"));
    assert!(main_text.contains("Figure shape") || told, "updated, or the person is told: {messages:?}\n{main_text}");
    checked("file rename renames the class: ok");
}

#[test]
#[ignore = "needs Eclipse JDT LS, a JDK 25 and Gradle: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME, ZEN_GRADLE_HOME (see the module's documentation)"]
fn a_gradle_project_is_imported_through_the_server() {
    let files = sources(&[("settings.gradle", SETTINGS_GRADLE), ("build.gradle", BUILD_GRADLE)]);
    let gradle = env_path("ZEN_GRADLE_HOME");
    let java = env_path("ZEN_JDTLS_JAVA_HOME");
    let settings = json!({ "java": { "import": { "gradle": {
        "wrapper": { "enabled": false }, "home": gradle, "java": { "home": java }, "offline": { "enabled": true }
    } } } });
    let mut window = window(&files, settings);
    let main = window.path("src/main/java/com/acme/shop/Main.java");
    window.run("file.document.open", json!({ "path": main }));
    let answer = window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer)
            .iter()
            .any(|problem| problem["message"].as_str().is_some_and(|message| message.contains("Type mismatch")))
    });
    checked(&format!("problems: ok ({})", zen_workbench::language::listed_problems(&answer).len()));
    let at = MAIN.find("new Circle").unwrap() + 4;
    let found = window.lookup("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 5000 }));
    assert!(text(&found).contains("Circle.java"), "{found}");
    let imported = window.imported();
    assert!(
        imported.iter().any(|name| name.starts_with("gradle-shop")),
        "the Gradle project is imported: {imported:?}"
    );
    checked(&format!("gradle import: ok ({imported:?})"));
}

const BASE: &str = "package com.acme.shop;

public class Base {
    public Base() {
    }

    public Base(String name) {
    }
}
";

const ORDER: &str = "package com.acme.shop;

import java.util.List;

public class Order extends Base implements Shape {
    private String id;
    private int count;
    private List<String> items;

}
";

fn generate(window: &mut Window, path: &Path, title: &str, members: &[&str]) -> String {
    let at = ORDER.rfind('}').unwrap();
    let listed =
        window.until("editor.generate.list", json!({ "path": path, "offset": at, "waitMs": 5000 }), |answer| {
            answer["pending"] == false && text(answer).contains(title)
        });
    let generator = listed["generators"].as_array().unwrap().iter().find(|generator| generator["title"] == title);
    let id = generator.unwrap_or_else(|| panic!("{title}: {listed}"))["id"].clone();
    let chooser = window.lookup("editor.generate.members", json!({ "id": id, "waitMs": 8000 }));
    assert!(!chooser["chooser"]["members"].as_array().unwrap().is_empty(), "{title}: {chooser}");
    window.run("editor.generate.apply", json!({ "id": id, "members": members, "waitMs": 8000 }));
    let written = window.text(path);
    window.run("editor.edit.undo", json!({}));
    assert_eq!(window.text(path), ORDER, "{title} is one undo step");
    checked(&format!("generate {title}: ok"));
    written
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn every_generator_of_the_server_writes_code_in_a_maven_project() {
    let files = [
        ("pom.xml", POM),
        ("src/main/java/com/acme/shop/Shape.java", SHAPE),
        ("src/main/java/com/acme/shop/Base.java", BASE),
        ("src/main/java/com/acme/shop/Order.java", ORDER),
    ];
    let repository = tempfile::tempdir().unwrap();
    let repository = std::env::var_os("ZEN_MAVEN_REPO").map_or(repository.path().to_owned(), PathBuf::from);
    let user_settings = repository.join("zen-test-settings.xml");
    std::fs::create_dir_all(&repository).unwrap();
    std::fs::write(
        &user_settings,
        format!(
            "<settings><localRepository>{}</localRepository></settings>\n",
            repository.join("repository").display()
        ),
    )
    .unwrap();
    let mut window =
        window(&files, json!({ "java": { "configuration": { "maven": { "userSettings": user_settings } } } }));
    let order = window.path("src/main/java/com/acme/shop/Order.java");
    window.run("file.document.open", json!({ "path": order }));
    let at = ORDER.rfind('}').unwrap();
    let listed =
        window.until("editor.generate.list", json!({ "path": order, "offset": at, "waitMs": 5000 }), |answer| {
            answer["pending"] == false && text(answer).contains("Delegate Methods")
        });
    let titles: Vec<&str> =
        listed["generators"].as_array().unwrap().iter().filter_map(|generator| generator["title"].as_str()).collect();
    assert_eq!(
        titles,
        [
            "Constructor",
            "Getter",
            "Setter",
            "Getter and Setter",
            "equals() and hashCode()",
            "toString()",
            "Override Methods",
            "Implement Methods",
            "Delegate Methods"
        ],
        "{listed}"
    );
    checked("generate list: ok");

    let written = generate(&mut window, &order, "Constructor", &["constructors.1", "fields.0"]);
    assert!(
        written.contains("public Order(String name, String id) {") && written.contains("super(name);"),
        "{written}"
    );
    let written = generate(&mut window, &order, "Getter", &["fields.0", "fields.1"]);
    assert!(written.contains("public String getId()") && written.contains("public int getCount()"), "{written}");
    assert!(!written.contains("setId"), "{written}");
    let written = generate(&mut window, &order, "Setter", &["fields.1"]);
    assert!(written.contains("public void setCount(int count)") && !written.contains("getCount"), "{written}");
    let written = generate(&mut window, &order, "Getter and Setter", &["fields.2"]);
    assert!(written.contains("public List<String> getItems()") && written.contains("setItems("), "{written}");
    let written = generate(&mut window, &order, "equals() and hashCode()", &["fields.0", "fields.1"]);
    assert!(written.contains("public int hashCode()") && written.contains("public boolean equals(Object"), "{written}");
    let written = generate(&mut window, &order, "toString()", &["fields.0"]);
    assert!(written.contains("public String toString()") && written.contains("\"Order [id=\" + id"), "{written}");
    let written = generate(&mut window, &order, "Implement Methods", &["methods.0"]);
    assert!(written.contains("public double area()"), "{written}");
    let written = generate(&mut window, &order, "Override Methods", &["methods.0"]);
    assert!(written.contains("@Override") && !written.contains("double area()"), "{written}");
    let written = generate(&mut window, &order, "Delegate Methods", &["methods.0"]);
    assert!(written.contains("return id.charAt(index);"), "{written}");
}

const PERSON: &str = "package com.acme.shop;

import lombok.Builder;
import lombok.Getter;
import lombok.extern.slf4j.Slf4j;

@Getter
@Builder
@Slf4j
public class Person {
    private final String name;

    void greet() {
        log.info(\"hello {}\", name);
    }
}
";

const USE_PERSON: &str = "package com.acme.shop;

public class UsePerson {
    static String run() {
        Person person = Person.builder().name(\"lombok\").build();
        return person.getName();
    }
}
";

const LOMBOK_POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId><artifactId>lombok-shop</artifactId><version>1.0</version>
  <properties><maven.compiler.release>17</maven.compiler.release></properties>
  <dependencies>
    <dependency><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId><version>1.18.38</version><scope>provided</scope></dependency>
    <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.16</version></dependency>
  </dependencies>
</project>
"#;

const LOMBOK_GRADLE: &str = "plugins { id 'java' }
repositories { mavenCentral() }
dependencies {
    compileOnly 'org.projectlombok:lombok:1.18.38'
    annotationProcessor 'org.projectlombok:lombok:1.18.38'
    implementation 'org.slf4j:slf4j-api:2.0.16'
}
";

fn lombok_members_work(window: &mut Window) {
    let person = window.path("src/main/java/com/acme/shop/Person.java");
    let using = window.path("src/main/java/com/acme/shop/UsePerson.java");
    window.run("file.document.open", json!({ "path": person }));
    window.run("file.document.open", json!({ "path": using }));
    let errors = |answer: &Value| {
        zen_workbench::language::listed_problems(answer)
            .iter()
            .filter(|problem| problem["severity"] == "error")
            .map(|problem| problem["message"].as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
    };
    let launched = window.until("lsp.server.log", json!({ "id": SERVER }), |log| text(log).contains("-javaagent:"));
    assert!(text(&launched).contains("lombok-1.18.38.jar"), "JDT LS runs with the project's Lombok");
    for path in [&person, &using] {
        let answer = window.until("problems.file.list", json!({ "path": path }), |answer| {
            answer["checked"] == true && errors(answer).is_empty()
        });
        checked(&format!("no false errors: ok ({})", zen_workbench::language::listed_problems(&answer).len()));
    }
    let at = USE_PERSON.find("getName").unwrap() + "getN".len();
    window.run("file.document.open", json!({ "path": using }));
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.completion.start", json!({}));
    window.until("editor.completion.list", json!({}), |list| {
        list["items"].as_array().is_some_and(|items| items.iter().any(|item| text(&item["label"]).contains("getName")))
    });
    window.run("editor.completion.cancel", json!({}));
    checked("getter completes: ok");
    let at = USE_PERSON.find("builder()").unwrap() + 2;
    let found = window.lookup("navigate.declaration.goTo", json!({ "path": using, "offset": at, "waitMs": 5000 }));
    assert!(text(&found).contains("Person.java"), "{found}");
    checked("builder navigates: ok");
}

#[test]
#[ignore = "needs Eclipse JDT LS, a JDK 25, Maven's local repository with Lombok 1.18.38 and slf4j-api 2.0.16 (online or ZEN_MAVEN_REPO): ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME"]
fn lombok_members_complete_and_navigate_in_a_maven_project() {
    let repository = std::env::var_os("ZEN_MAVEN_REPO")
        .map_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap().join(".m2"), PathBuf::from);
    let scratch = tempfile::tempdir().unwrap();
    let user_settings = scratch.path().join("settings.xml");
    std::fs::write(
        &user_settings,
        format!(
            "<settings><localRepository>{}</localRepository></settings>\n",
            repository.join("repository").display()
        ),
    )
    .unwrap();
    let files = [
        ("pom.xml", LOMBOK_POM),
        ("src/main/java/com/acme/shop/Person.java", PERSON),
        ("src/main/java/com/acme/shop/UsePerson.java", USE_PERSON),
    ];
    let mut window =
        window(&files, json!({ "java": { "configuration": { "maven": { "userSettings": user_settings } } } }));
    zen_workbench::build_tools::BuildEnv::global()
        .set_project_settings(
            zen_workbench::maven::settings::TOOL,
            &window.root,
            &zen_workbench::maven::settings::MavenSettings {
                local_repository: repository.join("repository").display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
    lombok_members_work(&mut window);
}

#[test]
#[ignore = "needs Eclipse JDT LS, a JDK 25, Gradle and a JDK it runs on, Lombok 1.18.38 and slf4j-api 2.0.16 in the Gradle cache: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME, ZEN_GRADLE_HOME, ZEN_PROJECT_JAVA_HOME"]
fn lombok_members_complete_and_navigate_in_a_gradle_project() {
    let files = [
        ("settings.gradle", "rootProject.name = 'lombok-shop'\n"),
        ("build.gradle", LOMBOK_GRADLE),
        ("src/main/java/com/acme/shop/Person.java", PERSON),
        ("src/main/java/com/acme/shop/UsePerson.java", USE_PERSON),
    ];
    let settings = json!({ "java": { "import": { "gradle": {
        "wrapper": { "enabled": false }, "home": env_path("ZEN_GRADLE_HOME"),
        "java": { "home": env_path("ZEN_PROJECT_JAVA_HOME") }, "offline": { "enabled": true }
    } } } });
    let mut window = window(&files, settings);
    lombok_members_work(&mut window);
}

#[test]
#[ignore = "downloads Eclipse JDT LS (about 50 MB) from download.eclipse.org, as lsp.server.install does"]
fn installing_downloads_checks_and_unpacks_the_archive() {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let data = tempfile::tempdir().unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        Arc::new(zen_workbench::language::servers::SystemTools),
    );
    servers.declare(plugin_servers());
    assert!(servers.executable(SERVER).is_none());
    assert!(servers.installable(SERVER));
    assert!(servers.install_line(SERVER).unwrap().starts_with("https://download.eclipse.org/"));
    servers.install(SERVER).unwrap();
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        match servers.install_state(SERVER) {
            Some(zen_workbench::language::servers::Install::Done) => break,
            Some(zen_workbench::language::servers::Install::Failed(why)) => panic!("{why}"),
            _ => assert!(Instant::now() < deadline, "still installing"),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let found = servers.executable(SERVER).unwrap();
    assert!(found.starts_with(data.path().join("language-servers/jdtls/plugins")), "{}", found.display());
    assert_eq!(servers.source(SERVER), zen_workbench::language::servers::ServerSource::Local);
    checked(&format!("install: ok (version {:?})", servers.version(SERVER)));
}

fn refactored(window: &mut Window, command: &str, path: &Path, text: &str, needle: &str, caret: bool) -> Value {
    let (skip, needle) = needle.split_once('|').unwrap_or(("", needle));
    let at = text.find(&format!("{skip}{needle}")).unwrap() + skip.len();
    let start = text[..at].chars().count();
    let end = if caret { start } else { start + needle.chars().count() };
    let mut params = json!({ "path": path, "start": start, "end": end, "waitMs": 10000 });
    let offered = |answer: &Value| {
        answer["title"].is_string() || answer["variants"].as_array().is_some_and(|variants| !variants.is_empty())
    };
    let mut answer = window.until(command, params.clone(), offered);
    if answer["title"].is_null() {
        checked(&format!("{command} variants: {}", answer["variants"]));
        params["variant"] = json!(0);
        answer = window.until(command, params, offered);
    }
    answer
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn every_extract_refactoring_and_inline_work_in_java() {
    let files = sources(&[("pom.xml", POM)]);
    let mut window = window(&files, json!({}));
    let main = window.path("src/main/java/com/acme/shop/Main.java");
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| problem["message"].as_str().is_some())
    });
    for (command, needle, caret) in [
        ("refactor.extract.variable", "shape.area()", false),
        ("refactor.extract.constant", "Circle(|2", false),
        ("refactor.extract.field", "shape.area()", false),
        ("refactor.extract.parameter", "value * 2", false),
        ("refactor.extract.method", "twice(shape.area())", false),
        ("refactor.inline.apply", "total + wrong", true),
    ] {
        let answer = refactored(&mut window, command, &main, MAIN, needle, caret);
        let changed = window.text(&main);
        assert_ne!(changed, MAIN, "{command}: {answer}");
        if command != "refactor.inline.apply" {
            let name = answer["name"].as_str().unwrap_or_else(|| panic!("{command}: {answer}"));
            assert!(answer["occurrences"].as_array().is_some_and(|all| all.len() >= 2), "{command}: {answer}");
            assert!(changed.contains(name), "{changed}");
        }
        checked(&format!("{command}: ok ({}, name {})", answer["title"], answer["name"]));
        window.run("editor.edit.undo", json!({}));
        assert_eq!(window.text(&main), MAIN, "{command}: one undo step");
    }
}

fn plugin_code_style() -> zen_workbench::code_style_options::CodeStyleDeclaration {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/java/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let declarations =
        zen_workbench::code_style_options::CodeStyleDeclaration::parse_list(&manifest["codeStyles"].to_string())
            .unwrap();
    let [declaration] = declarations.as_slice() else { panic!("{declarations:?}") };
    declaration.clone()
}

fn labels_of(label: &zen_project::code_style::Label) -> Vec<String> {
    match label {
        zen_project::code_style::Label::Texts(texts) => {
            ["en", "ru"].iter().map(|code| texts.get(*code).cloned().unwrap_or_default()).collect()
        }
        zen_project::code_style::Label::Message(id) => panic!("a plug-in label is text, not the message {id}"),
    }
}

#[test]
fn the_plugin_declares_java_code_style_as_eclipse_formatter_settings_and_server_settings() {
    let style = plugin_code_style();
    assert_eq!(style.language, "java");
    let file = style.config_file.as_ref().unwrap();
    assert_eq!(file.names, [".zen/java-formatter.xml"]);
    assert_eq!(file.format, zen_project::code_style::ConfigFormat::EclipseProfile);
    let location = file.location.as_ref().unwrap();
    assert_eq!((location.section.as_str(), location.path.as_str()), ("java", "format.settings.url"));
    for option in &style.options {
        let labels = labels_of(&option.label);
        assert!(labels.iter().all(|label| !label.is_empty()), "{}: {labels:?}", option.id);
        let keys: Vec<&String> = option.config_key.iter().chain(&option.more_config_keys).collect();
        assert!(
            keys.iter().all(|key| key.starts_with("org.eclipse.jdt.core.formatter.")),
            "{}: only formatter settings go into the profile",
            option.id
        );
        assert!(!keys.is_empty() || option.server_setting.is_some(), "{} reaches the server", option.id);
        if let zen_project::code_style::OptionKind::Choice { choices } = &option.kind {
            for choice in choices {
                assert!(labels_of(&choice.label).iter().all(|label| !label.is_empty()), "{}", option.id);
            }
        }
    }
    let tabs: std::collections::BTreeSet<_> = style.options.iter().map(|option| option.tab).collect();
    assert_eq!(tabs.len(), 5, "every tab but Arrangement has options: {tabs:?}");
    let given: zen_workbench::code_style_options::Values =
        [("importOrder".to_owned(), json!("java;org;#;")), ("classCountToUseStar".to_owned(), json!(5))].into();
    assert_eq!(
        style.server_settings(&given)["java"],
        json!({ "completion": { "importOrder": ["java", "org", "#", ""] },
                "sources": { "organizeImports": { "starThreshold": 5 } } })
    );
    let profile = style.write_config("", &[("bracesMethod".to_owned(), json!("next_line"))].into());
    for key in ["brace_position_for_method_declaration", "brace_position_for_constructor_declaration"] {
        assert!(
            profile.contains(&format!("id=\"org.eclipse.jdt.core.formatter.{key}\" value=\"next_line\"")),
            "{profile}"
        );
    }
    assert!(profile.contains("insert_space_before_opening_paren_in_if\" value=\"insert\""), "{profile}");
}

fn reformatted(window: &mut Window, path: &Path, wanted: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        window.until("editor.code.reformat", json!({ "path": path, "waitMs": 8000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
        let text = window.text(path);
        if wanted(&text) || Instant::now() >= deadline {
            return text;
        }
        window.run("language.servers.sync", json!({}));
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn safe_delete_finds_the_usages_of_a_method_and_deletes_it_with_its_javadoc() {
    let files = sources(&[("pom.xml", POM)]);
    let mut window = window(&files, json!({}));
    let main = window.path("src/main/java/com/acme/shop/Main.java");
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| problem["message"].as_str().is_some())
    });
    let at = MAIN.find("twice(double").unwrap() + 1;
    let place = json!({ "path": main, "offset": at, "waitMs": 10000 });
    let found = window
        .until("refactor.safeDelete.find", place, |answer| answer["pending"] == false && answer.get("error").is_none());
    assert_eq!((found["name"].clone(), found["total"].clone()), (json!("twice"), json!(2)), "{found}");
    checked("safe delete finds both calls: ok");
    window.run("refactor.safeDelete.apply", json!({ "path": main, "offset": at, "force": true }));
    let changed = window.text(&main);
    assert!(!changed.contains("Doubles a number") && !changed.contains("static double twice"), "{changed}");
    checked("safe delete anyway removes the method and its Javadoc: ok");
    window.run("editor.edit.undo", json!({}));
    assert_eq!(window.text(&main), MAIN, "one undo step");
}

const POINT: &str = "package com.acme.geo;\n\npublic class Point {\n    double x;\n}\n";

fn dialog(window: &mut Window, command: &str, provider: &str, context: &Value, values: Value) -> Value {
    let params = json!({ "provider": provider, "context": context, "values": values, "waitMs": 10000 });
    window.until(command, params, |answer| answer["pending"] != true)
}

fn option(form: &Value, label: &str) -> Value {
    let options = form["fields"][0]["options"].as_array().cloned().unwrap_or_default();
    options
        .iter()
        .find(|option| option["label"] == label)
        .map(|option| option["value"].clone())
        .unwrap_or_else(|| panic!("{form}"))
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn move_and_change_signature_work_through_the_refactoring_dialog() {
    let mut files = sources(&[("pom.xml", POM)]);
    files.push(("src/main/java/com/acme/geo/Point.java", POINT));
    let mut window = window(&files, json!({}));
    let main = window.path("src/main/java/com/acme/shop/Main.java");
    window.run("file.document.open", json!({ "path": main }));
    window.until("problems.file.list", json!({ "path": main }), |answer| {
        zen_workbench::language::listed_problems(answer).iter().any(|problem| problem["message"].as_str().is_some())
    });

    let at = MAIN[..MAIN.find("twice(double").unwrap()].chars().count() + 1;
    let context = json!({ "path": main, "offset": at });
    let form = dialog(&mut window, "refactor.dialog.form", "javaChangeSignature", &context, json!({}));
    assert_eq!(form["fields"][2]["value"], "twice", "{form}");
    assert_eq!(form["fields"][3]["value"][0]["name"], "value", "{form}");
    let values = json!({
        "name": "thrice",
        "parameters": [
            { "type": "double", "name": "value", "defaultValue": "", "originalIndex": 0 },
            { "type": "String", "name": "label", "defaultValue": "\"x\"", "originalIndex": -1 }
        ]
    });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaChangeSignature", &context, values.clone());
    assert_eq!(plan["conflicts"], json!([]), "{plan}");
    assert!(plan["usageCount"].as_u64().is_some_and(|count| count >= 2), "the declaration and the calls apart: {plan}");
    checked(&format!("change signature preview: ok ({} changes)", plan["usageCount"]));
    let applied = json!({ "provider": "javaChangeSignature", "context": context, "values": values, "waitMs": 10000 });
    window.run("refactor.dialog.apply", applied);
    let changed = window.text(&main);
    assert!(changed.contains("static double thrice(double value, String label)"), "{changed}");
    assert!(changed.contains("thrice(shape.area(), \"x\")"), "{changed}");
    checked("change signature renames, adds a parameter and updates the calls: ok");
    window.run("editor.edit.undo", json!({}));
    assert_eq!(window.text(&main), MAIN, "one undo step");

    let shape = window.path("src/main/java/com/acme/shop/Shape.java");
    let context = json!({ "paths": [shape] });
    let form = dialog(&mut window, "refactor.dialog.form", "javaMove", &context, json!({}));
    let values = json!({ "destination": option(&form, "com.acme.geo") });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaMove", &context, values.clone());
    assert_eq!(plan["operations"][0]["newRelative"], "src/main/java/com/acme/geo/Shape.java", "{plan}");
    checked(&format!("move class preview: ok ({} changes)", plan["usageCount"]));
    window.run(
        "refactor.dialog.apply",
        json!({ "provider": "javaMove", "context": context, "values": values, "waitMs": 10000 }),
    );
    let moved = window.path("src/main/java/com/acme/geo/Shape.java");
    assert!(moved.is_file() && !shape.exists());
    assert!(std::fs::read_to_string(&moved).unwrap().starts_with("package com.acme.geo;"));
    assert!(window.text(&main).contains("import com.acme.geo.Shape;"), "{}", window.text(&main));
    checked("move class moves the file and imports it where it is used: ok");
    window.run("editor.edit.undo", json!({}));
    assert!(shape.is_file() && !moved.exists(), "one undo step");
    assert_eq!(window.text(&main), MAIN);

    let geo = window.path("src/main/java/com/acme/geo");
    let context = json!({ "paths": [geo] });
    let form = dialog(&mut window, "refactor.dialog.form", "javaMove", &context, json!({}));
    let values = json!({ "destination": option(&form, "com.acme.shop") });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaMove", &context, values.clone());
    assert_eq!(plan["operations"][0]["newRelative"], "src/main/java/com/acme/shop/geo", "{plan}");
    window.run(
        "refactor.dialog.apply",
        json!({ "provider": "javaMove", "context": context, "values": values, "waitMs": 10000 }),
    );
    let point = window.path("src/main/java/com/acme/shop/geo/Point.java");
    assert!(std::fs::read_to_string(&point).unwrap().starts_with("package com.acme.shop.geo;"));
    checked("move package moves the folder and renames its package: ok");
}

const ANIMAL: &str = "package com.acme.zoo;\n\npublic class Animal {\n    protected String name;\n\n    public String describe() {\n        return name;\n    }\n}\n";

const DOG: &str = "package com.acme.zoo;\n\nimport java.util.List;\n\npublic class Dog extends Animal {\n    public int age;\n    private String owner;\n\n    public String bark(int times) {\n        return name + \" barks \" + times;\n    }\n\n    public String whose() {\n        return owner;\n    }\n\n    List<String> tricks() {\n        return List.of();\n    }\n}\n";

const SHOP: &str = "package com.acme.zoo;\n\npublic class Shop {\n    public static int total(int price, int count, String label) {\n        return price * count + label.length();\n    }\n\n    public static void main(String[] args) {\n        Dog dog = new Dog();\n        dog.age = 3;\n        dog.age += 2;\n        System.out.println(dog.age + total(1, 2, \"x\") + dog.describe());\n    }\n}\n";

fn disk_or_open(window: &Window, path: &Path) -> String {
    let open = window.text(path);
    if open.is_empty() { std::fs::read_to_string(path).unwrap_or_default() } else { open }
}

fn apply(window: &mut Window, provider: &str, context: &Value, values: &Value, force: bool) {
    let params = json!({ "provider": provider, "context": context, "values": values, "waitMs": 10000, "force": force });
    window.run("refactor.dialog.apply", params);
}

fn select_rows(form: &Value, field: &str, key: &str, chosen: &[&str]) -> Value {
    let fields = form["fields"].as_array().cloned().unwrap_or_default();
    let rows = fields.iter().find(|known| known["id"] == field).unwrap_or_else(|| panic!("{form}"))["value"].clone();
    Value::Array(
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let mut row = row.clone();
                row["selected"] = json!(chosen.contains(&row[key].as_str().unwrap_or_default()));
                row
            })
            .collect(),
    )
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn the_class_hierarchy_refactorings_work_through_the_refactoring_dialog() {
    let files = [
        ("pom.xml", POM),
        ("src/main/java/com/acme/zoo/Animal.java", ANIMAL),
        ("src/main/java/com/acme/zoo/Dog.java", DOG),
        ("src/main/java/com/acme/zoo/Shop.java", SHOP),
    ];
    let mut window = window(&files, json!({}));
    let (animal, dog, shop) = (
        window.path("src/main/java/com/acme/zoo/Animal.java"),
        window.path("src/main/java/com/acme/zoo/Dog.java"),
        window.path("src/main/java/com/acme/zoo/Shop.java"),
    );
    window.run("file.document.open", json!({ "path": dog }));
    let caret = |needle: &str| DOG[..DOG.find(needle).unwrap()].chars().count() + 1;
    let ready = json!({ "path": dog, "offset": caret("Animal {"), "waitMs": 8000 });
    window.until("navigate.declaration.goTo", ready, |answer| answer.to_string().contains("Animal.java"));

    let context = json!({ "path": dog, "offset": caret("bark(int") });
    let form = dialog(&mut window, "refactor.dialog.form", "javaPullUp", &context, json!({}));
    let values = json!({ "destination": option(&form, "com.acme.zoo.Animal"), "members": select_rows(&form, "members", "signature", &["bark(int)"]) });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaPullUp", &context, values.clone());
    assert_eq!(plan["conflicts"], json!([]), "{plan}");
    apply(&mut window, "javaPullUp", &context, &values, false);
    assert!(disk_or_open(&window, &animal).contains("public String bark(int times)"));
    assert!(!window.text(&dog).contains("bark("));
    checked("pull members up moves bark() into Animal: ok");
    window.run("editor.edit.undo", json!({}));
    assert_eq!(window.text(&dog), DOG, "one undo step");

    window.run("file.document.open", json!({ "path": animal }));
    let at = ANIMAL[..ANIMAL.find("describe").unwrap()].chars().count() + 1;
    let context = json!({ "path": animal, "offset": at });
    let form = dialog(&mut window, "refactor.dialog.form", "javaPushDown", &context, json!({}));
    let values = json!({ "members": select_rows(&form, "members", "signature", &["describe()"]) });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaPushDown", &context, values.clone());
    let conflicts = plan["conflicts"].as_array().cloned().unwrap_or_default();
    assert!(
        conflicts.iter().any(|conflict| conflict["relative"].as_str().is_some_and(|file| file.ends_with("Shop.java"))),
        "{plan}"
    );
    checked("push members down reports the use of describe() in Shop: ok");
    apply(&mut window, "javaPushDown", &context, &values, true);
    assert!(disk_or_open(&window, &dog).contains("public String describe()"));
    window.run("editor.edit.undo", json!({}));
    assert_eq!(disk_or_open(&window, &animal), ANIMAL, "one undo step");

    window.run("file.document.open", json!({ "path": dog }));
    let context = json!({ "path": dog, "offset": caret("bark(int") });
    let form = dialog(&mut window, "refactor.dialog.form", "javaExtractInterface", &context, json!({}));
    let values =
        json!({ "name": "Barker", "members": select_rows(&form, "members", "member", &["bark(int): String"]) });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaExtractInterface", &context, values.clone());
    assert_eq!(plan["error"], Value::Null, "{plan}");
    apply(&mut window, "javaExtractInterface", &context, &values, false);
    let barker = window.path("src/main/java/com/acme/zoo/Barker.java");
    assert!(disk_or_open(&window, &barker).contains("String bark(int times);"), "{}", disk_or_open(&window, &barker));
    let changed = window.text(&dog);
    assert!(changed.contains("extends Animal implements Barker") && !changed.contains("non-Javadoc"), "{changed}");
    checked("extract interface creates Barker through the server: ok");
    window.run("editor.edit.undo", json!({}));
    assert!(!barker.exists(), "one undo step");

    let context = json!({ "path": dog, "offset": caret("Dog extends") });
    let values = json!({ "name": "Pet", "members": [{ "selected": true, "signature": "tricks()" }] });
    apply(&mut window, "javaExtractSuperclass", &context, &values, false);
    let pet = window.path("src/main/java/com/acme/zoo/Pet.java");
    assert!(disk_or_open(&window, &pet).contains("public class Pet extends Animal {"));
    assert!(window.text(&dog).contains("public class Dog extends Pet {"));
    checked("extract superclass creates Pet between Dog and Animal: ok");
    window.run("editor.edit.undo", json!({}));
    assert!(!pet.exists(), "one undo step");

    let context = json!({ "path": dog, "offset": caret("age;") });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaEncapsulateFields", &context, json!({}));
    assert_eq!(plan["conflicts"], json!([]), "{plan}");
    apply(&mut window, "javaEncapsulateFields", &context, &json!({}), false);
    let used = disk_or_open(&window, &shop);
    assert!(used.contains("dog.setAge(3);") && used.contains("dog.setAge(dog.getAge() + 2);"), "{used}");
    assert!(window.text(&dog).contains("private int age;"));
    checked("encapsulate fields adds accessors and rewrites the uses in Shop: ok");
    window.run("editor.edit.undo", json!({}));
    assert_eq!(disk_or_open(&window, &shop), SHOP, "one undo step");

    window.run("file.document.open", json!({ "path": shop }));
    let at = SHOP[..SHOP.find("total(int").unwrap()].chars().count() + 1;
    let context = json!({ "path": shop, "offset": at });
    let values = json!({ "className": "Order", "parameterName": "order", "kind": "record", "parameters": [
        { "selected": true, "name": "price" }, { "selected": true, "name": "count" }, { "selected": false, "name": "label" }
    ] });
    let plan = dialog(&mut window, "refactor.dialog.preview", "javaIntroduceParameterObject", &context, values.clone());
    assert_eq!((&plan["error"], &plan["conflicts"]), (&Value::Null, &json!([])), "{plan}");
    apply(&mut window, "javaIntroduceParameterObject", &context, &values, false);
    let changed = window.text(&shop);
    assert!(
        changed.contains("total(Order order, String label)") && changed.contains("total(new Order(1, 2), \"x\")"),
        "{changed}"
    );
    checked("introduce parameter object creates the record and rewrites the call: ok");
}

#[test]
#[ignore = "needs Eclipse JDT LS and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME (see the module's documentation)"]
fn code_style_options_reach_the_eclipse_formatter() {
    let style = plugin_code_style();
    let preview = style.preview.clone();
    let mut window = window(&[("pom.xml", POM), ("src/main/java/com/example/shop/Basket.java", &preview)], json!({}));
    window.session.set_state(zen_workbench::code_style_options::CodeStyles::new(vec![style], Vec::new()));
    let data = window.data.path().to_owned();
    window.session.state_mut::<zen_workbench::workspaces::Workspaces>().set_data_dir(data.clone());
    window.session.settings_mut().set("editor.projectCodeStyle", false).unwrap();
    let basket = window.path("src/main/java/com/example/shop/Basket.java");
    window.run("file.document.open", json!({ "path": basket }));
    let messy = preview
        .replace("    public Basket(Item first) {", "public Basket(Item first){")
        .replace("if (items", "if(items");
    window.run("editor.text.write", json!({ "path": basket, "text": messy }));
    let formatted = reformatted(&mut window, &basket, |text| text != messy);
    assert_eq!(formatted, preview, "the preview is what the server makes at the defaults");
    checked("defaults: the preview is the server's own formatting");

    window.session.settings_mut().set("editor.codeStyle.java.bracesMethod", "next_line").unwrap();
    let formatted = reformatted(&mut window, &basket, |text| text.contains("public Basket(Item first)\n    {"));
    assert!(formatted.contains("public Basket(Item first)\n    {"), "{formatted}");
    assert!(formatted.contains("if (items"), "{formatted}");
    let stored = std::fs::read_dir(data.join("code-style/java")).unwrap().count();
    assert_eq!(stored, 1, "the IDE keeps its profile in its own folder");
    checked("IDE scheme: braces of methods on the next line");

    window.session.settings_mut().set("editor.codeStyle.java.spaceBeforeIfParentheses", false).unwrap();
    let formatted = reformatted(&mut window, &basket, |text| text.contains("if(items"));
    assert!(formatted.contains("if(items") && formatted.contains("public Basket(Item first)\n    {"), "{formatted}");
    checked("IDE scheme: a second change to the same profile is read again");

    window.run("editor.codeStyle.writeProject", json!({ "language": "java", "values": {} }));
    window.session.settings_mut().set("editor.projectCodeStyle", true).unwrap();
    window.run("editor.text.write", json!({ "path": basket, "text": messy }));
    let formatted = reformatted(&mut window, &basket, |text| text == preview);
    assert_eq!(formatted, preview, "a profile of every declared default formats as the server's own defaults");
    checked("Project scheme: a profile of the defaults formats as the server does without one");

    window.run("editor.codeStyle.writeProject", json!({ "language": "java", "values": { "continuationIndent": 1 } }));
    let long = format!(
        "package com.example.shop;\n\nclass Long {{\n    String f(String a) {{\n        return {};\n    }}\n}}\n",
        vec!["a"; 50].join(" + ")
    );
    window.run("editor.text.write", json!({ "path": basket, "text": long }));
    let continued = |text: &str| {
        text.lines().any(|line| line.starts_with("            + a") || line.starts_with("            a +"))
    };
    let formatted = reformatted(&mut window, &basket, continued);
    assert!(continued(&formatted), "{formatted}");
    assert!(window.root.join(".zen/java-formatter.xml").is_file());
    checked("Project scheme: the project's profile, continuation indent of one indent");

    window.session.settings_mut().set("editor.codeStyle.java.importOrder", "java;org;#;").unwrap();
    window.session.settings_mut().set("editor.codeStyle.java.classCountToUseStar", 2).unwrap();
    let imports = "package com.example.shop;\n\nimport static java.util.Objects.requireNonNull;\n\nimport java.util.ArrayList;\nimport java.util.List;\n\nclass Order {\n    List<String> names = new ArrayList<>(requireNonNull(List.of()));\n}\n";
    window.run("editor.text.write", json!({ "path": basket, "text": imports }));
    let laid_out = |text: &str| {
        let star = text.find("import java.util.*;");
        star.is_some() && star < text.find("import static")
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    let organized = loop {
        window.until("editor.imports.optimize", json!({ "path": basket, "waitMs": 8000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
        let text = window.text(&basket);
        if laid_out(&text) || Instant::now() >= deadline {
            break text;
        }
        window.run("editor.text.write", json!({ "path": basket, "text": imports }));
        std::thread::sleep(Duration::from_millis(500));
    };
    assert!(laid_out(&organized), "{organized}");
    checked("imports: the order and the class count to use * reach Optimize Imports");
}

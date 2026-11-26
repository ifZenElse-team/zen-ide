use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_run::{Configuration, RunProvider};
use zen_workbench::java::JavaTestRun;
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration};

const CALC: &str = "package com.acme;

public class Calc {
    public int add(int a, int b) {
        return a + b;
    }
}
";

const CALC_TEST: &str = "package com.acme;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Disabled;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

class CalcTest {
    @Test
    void adds() {
        int sum = new Calc().add(1, 2);
        System.out.println(\"adding \" + sum);
        assertEquals(3, sum);
    }

    @Test
    void fails() {
        assertEquals(5, new Calc().add(2, 2), \"two and two\");
    }

    @Disabled(\"later\")
    @Test
    void skipped() {
    }

    @ParameterizedTest
    @ValueSource(ints = {1, 2})
    void positive(int value) {
        assertEquals(value + 1, new Calc().add(value, 1));
    }
}
";

const APP: &str = "package com.acme;

public class App {
    public static void main(String[] args) {
        int total = 0;
        for (int i = 0; i < 3; i++) {
            total += i;
        }
        System.out.println(\"total=\" + total);
        if (args.length > 0) {
            throw new IllegalStateException(\"boom\");
        }
    }
}
";

const COUNTER: &str = "package com.acme;

public class Counter {
    static int value() {
        return 1;
    }
}
";

const COUNTING: &str = "package com.acme;

public class Counting {
    public static void main(String[] args) {
        for (int i = 0; i < 3; i++) {
            System.out.println(\"value=\" + Counter.value());
        }
    }
}
";

const POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId><artifactId>calc</artifactId><version>1.0</version>
  <properties>
    <maven.compiler.release>21</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencies>
    <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><version>5.11.4</version><scope>test</scope></dependency>
  </dependencies>
  <build><plugins><plugin><artifactId>maven-surefire-plugin</artifactId><version>3.5.2</version></plugin></plugins></build>
</project>
"#;

const BUILD_GRADLE: &str = "plugins { id 'java' }
repositories { mavenCentral() }
dependencies {
    testImplementation 'org.junit.jupiter:junit-jupiter:5.11.4'
    testRuntimeOnly 'org.junit.platform:junit-platform-launcher'
}
test { useJUnitPlatform() }
";

const SERVER: &str = "jdtls";

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} (see the module's documentation)")))
}

struct Window {
    registry: Registry,
    session: Session,
    _folder: tempfile::TempDir,
    root: PathBuf,
    _data: tempfile::TempDir,
}

impl Window {
    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn until(&mut self, what: &str, seconds: u64, mut done: impl FnMut(&mut Self) -> Option<Value>) -> Value {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            self.run("language.servers.sync", json!({}));
            if let Some(value) = done(self) {
                return value;
            }
            assert!(Instant::now() < deadline, "{what}: timed out");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn tests(&mut self, run_id: u64) -> Value {
        self.until("the run ends", 600, |window| {
            let output = window.run("run.output.get", json!({ "runId": run_id, "waitMs": 200 }));
            (output["state"] != "running").then(|| window.run("run.tests.get", json!({ "runId": run_id })))
        })
    }

    fn output(&mut self, run_id: u64) -> String {
        let output = self.run("run.output.get", json!({ "runId": run_id }));
        output["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn debug_state(&mut self, what: &str, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            self.run("language.servers.sync", json!({}));
            let got = self.run("debug.session.get", json!({ "waitMs": 500 }));
            if got["state"] == state {
                return got;
            }
            if Instant::now() >= deadline {
                let runs = self.run("run.process.list", json!({}));
                let run = runs["runs"]
                    .as_array()
                    .and_then(|runs| runs.iter().filter_map(|run| run["runId"].as_u64()).max())
                    .unwrap_or_default();
                let output = self.output(run);
                let log = self.run("lsp.server.log", json!({ "id": SERVER }));
                let log: Vec<String> = log["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rev()
                    .take(40)
                    .rev()
                    .map(|line| line.as_str().unwrap_or_default().chars().take(300).collect())
                    .collect();
                panic!("{what}: timed out in {got}\n--- output\n{output}\n--- server\n{}", log.join("\n"));
            }
        }
    }

    fn local(&mut self, name: &str) -> String {
        let scopes = self.run("debug.variables.list", json!({}));
        let reference = scopes["variables"][0]["reference"].as_i64().unwrap();
        let locals = self.run("debug.variables.list", json!({ "reference": reference }));
        let found = locals["variables"].as_array().unwrap().iter().find(|variable| variable["name"] == name);
        found.unwrap_or_else(|| panic!("{name}: {locals}"))["value"].as_str().unwrap().to_owned()
    }

    fn targets(&mut self, file: &str) -> Vec<Value> {
        let path = self.path(file);
        self.run("run.targets.list", json!({ "path": path, "full": true }))["targets"].as_array().unwrap().clone()
    }
}

fn names(node: &Value, out: &mut Vec<String>, depth: usize) {
    for child in node["children"].as_array().into_iter().flatten() {
        out.push(format!(
            "{}{} {}",
            "  ".repeat(depth),
            child["name"].as_str().unwrap(),
            child["status"].as_str().unwrap()
        ));
        names(child, out, depth + 1);
    }
}

fn tree(tests: &Value) -> Vec<String> {
    let mut out = Vec::new();
    names(&tests["tree"], &mut out, 0);
    out
}

#[allow(clippy::print_stderr, reason = "the list of what was checked is what the test reports")]
fn checked(what: &str) {
    eprintln!("{what}");
}

fn window(build: &[(&str, &str)]) -> Window {
    window_with(build, &[], true)
}

fn kotlin_file(path: &Path, text: &str) -> Option<zen_syntax::JvmFile> {
    let found = zen_plugin_kotlin::jvm(text);
    let line = |offset: usize| u32::try_from(text[..offset.min(text.len())].matches('\n').count()).unwrap();
    let file = zen_syntax::JvmFile {
        package: found.package.clone(),
        file_class: if found.jvm_name.is_empty() {
            String::new()
        } else {
            format!("{}.{}", found.package, found.jvm_name)
        },
        classes: found
            .classes
            .iter()
            .map(|class| zen_syntax::JvmClass {
                name: class.name.clone(),
                first_line: line(class.start),
                last_line: line(class.end),
            })
            .collect(),
        mains: found
            .mains
            .iter()
            .map(|main| zen_syntax::JvmMain {
                class: main.class.clone(),
                class_line: line(main.owner.unwrap_or(main.start)),
                line: line(main.start),
                end_line: line(main.end),
            })
            .collect(),
        tests: found
            .tests
            .iter()
            .map(|test| zen_syntax::JvmTestClass {
                name: test.name.clone(),
                line: line(test.start),
                testng: test.testng,
                methods: test.methods.iter().map(|(name, start)| (name.clone(), line(*start))).collect(),
            })
            .collect(),
    };
    Some(file.named_after(&path.file_stem()?.to_string_lossy()))
}

fn window_with(build: &[(&str, &str)], extra: &[(&str, &str)], java_sources: bool) -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    let mut files = build.to_vec();
    if java_sources {
        files.extend([
            ("src/main/java/com/acme/Calc.java", CALC),
            ("src/main/java/com/acme/App.java", APP),
            ("src/main/java/com/acme/Counter.java", COUNTER),
            ("src/main/java/com/acme/Counting.java", COUNTING),
            ("src/test/java/com/acme/CalcTest.java", CALC_TEST),
        ]);
    }
    files.extend(extra.iter().copied());
    for (name, text) in files {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let root = folder.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("language-servers")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(env_path("ZEN_JDTLS"), data.path().join("language-servers").join(SERVER)).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        Arc::new(zen_workbench::language::servers::SystemTools),
    );
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/java/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let mut declared = ServerDeclaration::parse_list(&manifest["languageServers"].to_string()).unwrap();
    let repository = env_path("ZEN_MAVEN_REPO");
    let settings = data.path().join("maven-settings.xml");
    std::fs::write(
        &settings,
        format!(
            "<settings><localRepository>{}</localRepository></settings>\n",
            repository.join("repository").display()
        ),
    )
    .unwrap();
    declared[0].initialization_options["settings"]["java"]["configuration"]["maven"]["userSettings"] = json!(settings);
    declared[0].initialization_options["settings"]["java"]["import"]["gradle"] = json!({
        "wrapper": { "enabled": false }, "home": env_path("ZEN_GRADLE_HOME"), "offline": { "enabled": true },
        "java": { "home": env_path("ZEN_PROJECT_JAVA_HOME") }, "user": { "home": env_path("ZEN_GRADLE_USER_HOME") }
    });
    servers.declare(declared);
    let java = env_path("ZEN_JDTLS_JAVA_HOME");
    servers.set_java_finder(Arc::new(move |_, _| Some(java.clone())));
    assert!(servers.executable(SERVER).is_some(), "the server and its java-debug bundle are in the IDE's folder");
    session.set_state(servers);
    let tools = vec![env_path("ZEN_MAVEN_HOME").join("bin"), env_path("ZEN_GRADLE_HOME").join("bin")];
    let build = zen_workbench::build_tools::BuildEnv::default();
    build.set_folders(Some(tools));
    build.set_jvm_reader(Some(Arc::new(kotlin_file)));
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    session.set_state(build.clone());
    let maven = env_path("ZEN_MAVEN_HOME").join("bin");
    let providers: Vec<Arc<dyn RunProvider>> = vec![
        Arc::new(JavaTestRun::new(build.clone())),
        Arc::new(
            zen_workbench::java_run::JavaRun::in_folders(vec![maven, env_path("ZEN_GRADLE_HOME").join("bin")])
                .with_env(build),
        ),
    ];
    session.state_mut::<zen_workbench::run::Runs>().set_providers(providers, zen_run::RunEnv::default());
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    let mut window = Window { registry, session, _folder: folder, root, _data: data };
    if !java_sources {
        return window;
    }
    let app = window.path("src/main/java/com/acme/App.java");
    window.run("file.document.open", json!({ "path": app }));
    let test = window.path("src/test/java/com/acme/CalcTest.java");
    window.run("file.document.open", json!({ "path": test }));
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        window.run("language.servers.sync", json!({}));
        let answer = window.run("problems.file.list", json!({ "path": test }));
        if answer["checked"] == true {
            break;
        }
        if Instant::now() > deadline {
            let servers = window.run("lsp.servers.list", json!({}));
            let log = window.run("lsp.server.log", json!({ "id": SERVER }));
            panic!("the server checks the test: timed out\n{answer}\n{servers}\n{log}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = settings;
    window
}

fn with_tools(target: &Value, tool_arguments: &str, env: Vec<(String, String)>) -> Value {
    let mut configuration: Configuration = serde_json::from_value(target["configuration"].clone()).unwrap();
    configuration.common.tool_arguments = tool_arguments.to_owned();
    configuration.common.env = env;
    serde_json::to_value(configuration).unwrap()
}

fn maven_args(window: &Window) -> String {
    format!("-s {}", window._data.path().join("maven-settings.xml").display())
}

fn project_java() -> Vec<(String, String)> {
    vec![("JAVA_HOME".to_owned(), env_path("ZEN_PROJECT_JAVA_HOME").display().to_string())]
}

#[test]
#[ignore = "needs Eclipse JDT LS with java-debug, JDKs, Maven and Gradle (see the module's documentation)"]
fn maven_runs_and_debugs_tests_and_java_debug_debugs_a_program() {
    let mut window = window(&[("pom.xml", POM)]);

    let targets = window.targets("src/test/java/com/acme/CalcTest.java");
    let shown: Vec<String> =
        targets.iter().map(|target| format!("{} {}", target["line"], target["name"].as_str().unwrap())).collect();
    assert_eq!(shown, ["10 com.acme.CalcTest", "12 adds", "19 fails", "25 skipped", "30 positive"], "{shown:?}");
    checked("gutter: ok");

    let configuration = with_tools(&targets[0], &maven_args(&window), project_java());
    let run = window.run("run.process.start", json!({ "configuration": configuration }))["runId"].as_u64().unwrap();
    let tests = window.tests(run);
    let shown = tree(&tests);
    assert_eq!(
        shown,
        [
            "com.acme.CalcTest failed",
            "  adds passed",
            "  fails failed",
            "  positive(int) passed",
            "    [1] passed",
            "    [2] passed",
            "  skipped skipped"
        ],
        "{}",
        window.output(run)
    );
    let failed = window.run("run.tests.get", json!({ "runId": run, "failedOnly": true }));
    let fails = &failed["failed"][0];
    assert_eq!((fails["test"].as_str(), fails["line"].as_u64()), (Some("fails"), Some(20)), "{failed}");
    assert!(fails["file"].as_str().unwrap().ends_with("src/test/java/com/acme/CalcTest.java"), "{failed}");
    assert!(fails["message"].as_str().unwrap().contains("two and two"), "{failed}");
    let adds = window.run("run.tests.get", json!({ "runId": run, "test": "adds", "package": "com.acme.CalcTest" }));
    checked(&format!("maven run: ok ({}) {}", tests["counts"], adds["output"]["status"]));

    let rerun = window.run("run.tests.rerunFailed", json!({ "runId": run }))["runId"].as_u64().unwrap();
    let tests = window.tests(rerun);
    assert_eq!(tree(&tests), ["com.acme.CalcTest failed", "  fails failed"], "{}", window.output(rerun));
    checked("rerun failed: ok");

    let test_file = window.path("src/test/java/com/acme/CalcTest.java");
    window.run("debug.breakpoint.set", json!({ "path": test_file, "line": 14 }));
    let configuration = with_tools(&targets[1], &maven_args(&window), project_java());
    let started = window.run("debug.session.start", json!({ "configuration": configuration }));
    let debug_run = started["runId"].as_u64().unwrap();
    let stopped = window.debug_state("the test stops", "stopped");
    assert_eq!(stopped["frame"]["line"], 14, "{stopped}");
    let frames = window.run("debug.frames.list", json!({}));
    assert!(frames["frames"][0]["name"].as_str().unwrap().contains("adds"), "{frames}");
    let evaluated = window.run("debug.expression.evaluate", json!({ "expression": "sum * 10" }));
    assert_eq!(evaluated["result"], "30", "{evaluated}");
    window.run("debug.breakpoint.remove", json!({ "path": test_file, "line": 14 }));
    window.run("debug.execution.resume", json!({}));
    window.debug_state("the test ends", "ended");
    let tests = window.tests(debug_run);
    assert_eq!(tree(&tests), ["com.acme.CalcTest passed", "  adds passed"], "{}", window.output(debug_run));
    checked("debug a test: ok");

    let app_file = window.path("src/main/java/com/acme/App.java");
    window.run("debug.breakpoint.set", json!({ "path": app_file, "line": 6, "condition": "i == 2" }));
    let maven_opts = vec![
        (
            "MAVEN_OPTS".to_owned(),
            format!("-Dmaven.repo.local={}", env_path("ZEN_MAVEN_REPO").join("repository").display()),
        ),
        ("JAVA_HOME".to_owned(), env_path("ZEN_PROJECT_JAVA_HOME").display().to_string()),
    ];
    let program = json!({ "name": "App", "kind": "java.application",
        "options": { "mainClass": "com.acme.App", "build": true, "jre": env_path("ZEN_PROJECT_JAVA_HOME") },
        "common": { "env": maven_opts.clone() } });
    let started = window.run("debug.session.start", json!({ "configuration": program }));
    let app_run = started["runId"].as_u64().unwrap();
    let stopped = window.debug_state("the program stops", "stopped");
    assert_eq!(stopped["frame"]["line"], 6, "{stopped}");
    let scopes = window.run("debug.variables.list", json!({}));
    let reference = scopes["variables"][0]["reference"].as_i64().unwrap();
    let locals = window.run("debug.variables.list", json!({ "reference": reference }));
    let i = locals["variables"].as_array().unwrap().iter().find(|variable| variable["name"] == "i").unwrap();
    assert_eq!(i["value"], "2", "{locals}");
    assert_eq!(window.run("debug.expression.evaluate", json!({ "expression": "total + i" }))["result"], "3");
    window.run("debug.watch.add", json!({ "expression": "total" }));
    assert_eq!(window.run("debug.watch.list", json!({}))["watches"][0]["value"], "1");
    window.run("debug.execution.stepOver", json!({}));
    let stepped = window.debug_state("the step ends", "stopped");
    assert_eq!(stepped["reason"], "step", "{stepped}");
    window.run("debug.breakpoint.remove", json!({ "path": app_file, "line": 6 }));
    window.run("debug.execution.resume", json!({}));
    window.debug_state("the program ends", "ended");
    assert!(window.output(app_run).contains("total=3"), "{}", window.output(app_run));
    checked("debug a program: ok");

    let counter = window.path("src/main/java/com/acme/Counter.java");
    let counting = window.path("src/main/java/com/acme/Counting.java");
    window.run("file.document.open", json!({ "path": counter }));
    window.run("debug.breakpoint.set", json!({ "path": counting, "line": 5 }));
    let program = json!({ "name": "Counting", "kind": "java.application",
        "options": { "mainClass": "com.acme.Counting", "build": true, "jre": env_path("ZEN_PROJECT_JAVA_HOME") },
        "common": { "env": maven_opts.clone() } });
    let started = window.run("debug.session.start", json!({ "configuration": program }));
    let counter_run = started["runId"].as_u64().unwrap();
    window.debug_state("the loop stops", "stopped");
    window.run(
        "editor.text.replace",
        json!({ "path": counter, "oldText": "return 1;", "newText": "return 2;", "save": true }),
    );
    let reloaded = window.until("the class is reloaded after the build", 120, |window| {
        let got = window.run("debug.session.get", json!({}));
        got.get("reloaded")
            .cloned()
            .filter(|reloaded| reloaded["changedClasses"].as_array().is_some_and(|c| !c.is_empty()))
    });
    assert!(reloaded["changedClasses"].as_array().unwrap().contains(&json!("com.acme.Counter")), "{reloaded}");
    assert_eq!(reloaded["automatic"], true);
    let again = window.run("debug.classes.reload", json!({}));
    assert!(again["error"].is_null(), "{again}");
    let notices = window.run("ui.notifications.list", json!({}));
    assert!(notices.to_string().contains("com.acme.Counter") && notices.to_string().contains("Reloaded"), "{notices}");
    window.run("debug.breakpoint.remove", json!({ "path": counting, "line": 5 }));
    window.run("debug.execution.resume", json!({}));
    window.debug_state("the loop ends", "ended");
    let printed = window.output(counter_run);
    assert!(printed.contains("value=2") && !printed.contains("value=1"), "{printed}");
    checked("hot code replace: ok");

    window.run("debug.exceptionBreakpoint.set", json!({ "filters": ["uncaught"] }));
    let program = json!({ "name": "App", "kind": "java.application",
        "options": { "mainClass": "com.acme.App", "build": true, "jre": env_path("ZEN_PROJECT_JAVA_HOME") },
        "common": { "env": maven_opts.clone(), "programArguments": "boom" } });
    window.run("debug.session.start", json!({ "configuration": program }));
    let stopped = window.debug_state("the exception stops it", "stopped");
    assert_eq!(
        (stopped["reason"].as_str(), stopped["frame"]["line"].as_u64()),
        (Some("exception"), Some(10)),
        "{stopped}"
    );
    let offered = window.run("debug.exceptionBreakpoint.list", json!({}));
    assert!(offered["offered"].as_array().unwrap().iter().any(|filter| filter["filter"] == "caught"), "{offered}");
    window.run("debug.session.stop", json!({}));
    checked("exception breakpoint: ok");
}

#[test]
#[ignore = "needs Eclipse JDT LS with java-debug, JDKs, Maven and Gradle (see the module's documentation)"]
fn gradle_runs_and_debugs_tests() {
    let mut window = window(&[("settings.gradle", "rootProject.name = 'calc'\n"), ("build.gradle", BUILD_GRADLE)]);
    let targets = window.targets("src/test/java/com/acme/CalcTest.java");
    let env = {
        let mut env = project_java();
        env.push(("GRADLE_USER_HOME".to_owned(), env_path("ZEN_GRADLE_USER_HOME").display().to_string()));
        env
    };
    let configuration = with_tools(&targets[0], "--offline", env.clone());
    let run = window.run("run.process.start", json!({ "configuration": configuration }))["runId"].as_u64().unwrap();
    let tests = window.tests(run);
    let shown = tree(&tests);
    assert!(
        shown.contains(&"  fails failed".to_owned()) && shown.contains(&"    [2] passed".to_owned()),
        "{shown:?} {}",
        window.output(run)
    );
    assert!(shown.contains(&"  skipped skipped".to_owned()), "{shown:?}");
    let failed = window.run("run.tests.get", json!({ "runId": run, "failedOnly": true }));
    assert_eq!(failed["failed"][0]["line"], 20, "{failed}");
    let adds = window.run("run.tests.get", json!({ "runId": run, "test": "adds", "package": "com.acme.CalcTest" }));
    assert!(adds["output"]["output"].as_array().unwrap().iter().any(|line| line == "adding 3"), "{adds}");
    checked(&format!("gradle run: ok ({})", tests["counts"]));

    let rerun = window.run("run.tests.rerunFailed", json!({ "runId": run }))["runId"].as_u64().unwrap();
    assert_eq!(tree(&window.tests(rerun)), ["com.acme.CalcTest failed", "  fails failed"], "{}", window.output(rerun));
    checked("gradle rerun failed: ok");

    let test_file = window.path("src/test/java/com/acme/CalcTest.java");
    window.run("debug.breakpoint.set", json!({ "path": test_file, "line": 14 }));
    let configuration = with_tools(&targets[1], "--offline", env);
    window.run("debug.session.start", json!({ "configuration": configuration }));
    let stopped = window.debug_state("the test stops", "stopped");
    assert_eq!(stopped["frame"]["line"], 14, "{stopped}");
    assert_eq!(window.run("debug.expression.evaluate", json!({ "expression": "sum" }))["result"], "3");
    window.run("debug.breakpoint.remove", json!({ "path": test_file, "line": 14 }));
    window.run("debug.execution.resume", json!({}));
    window.debug_state("the test ends", "ended");
    checked("gradle debug a test: ok");
}

const KOTLIN_VERSION: &str = "2.1.10";

const GREETER: &str = "package com.acme

inline fun twice(block: () -> Int): Int {
    val first = block()
    return first + block()
}

fun main(args: Array<String>) {
    val name = \"Kotlin\"
    val total = twice { 21 }
    println(\"hello $name $total\")
}
";

const GREETER_TEST: &str = "package com.acme

import kotlin.test.Test
import kotlin.test.assertEquals

class GreeterTest {
    @Test
    fun doubles() {
        val sum = twice { 2 }
        assertEquals(4, sum)
    }

    @Test
    fun fails() {
        assertEquals(5, twice { 2 }, \"two and two\")
    }
}
";

const SHOP: &str = "import java.util.*;
import java.util.stream.*;

public class Shop {
    public static void main(String[] args) {
        List<String> names = new ArrayList<>(List.of(\"ann\", \"bob\", \"cid\", \"dora\"));
        Map<String, Integer> ages = new HashMap<>();
        ages.put(\"ann\", 30);
        ages.put(\"bob\", 25);
        int[] numbers = {3, 1, 2};
        Set<String> tags = new TreeSet<>(Set.of(\"x\", \"y\"));
        List<Integer> lengths = names.stream().filter(n -> n.length() == 3).map(String::length).collect(Collectors.toList());
        System.out.println(lengths + \" \" + ages + \" \" + numbers.length + tags);
    }
}
";

fn kotlin_pom() -> String {
    format!(
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId><artifactId>greeter</artifactId><version>1.0</version>
  <properties>
    <maven.compiler.release>17</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencies>
    <dependency><groupId>org.jetbrains.kotlin</groupId><artifactId>kotlin-stdlib</artifactId><version>{KOTLIN_VERSION}</version></dependency>
    <dependency><groupId>org.jetbrains.kotlin</groupId><artifactId>kotlin-test-junit5</artifactId><version>{KOTLIN_VERSION}</version><scope>test</scope></dependency>
    <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><version>5.11.4</version><scope>test</scope></dependency>
  </dependencies>
  <build>
    <plugins>
      <plugin>
        <groupId>org.jetbrains.kotlin</groupId><artifactId>kotlin-maven-plugin</artifactId><version>{KOTLIN_VERSION}</version>
        <configuration><jvmTarget>17</jvmTarget></configuration>
        <executions>
          <execution><id>compile</id><goals><goal>compile</goal></goals>
            <configuration><sourceDirs><sourceDir>${{project.basedir}}/src/main/kotlin</sourceDir><sourceDir>${{project.basedir}}/src/main/java</sourceDir></sourceDirs></configuration></execution>
          <execution><id>test-compile</id><goals><goal>test-compile</goal></goals>
            <configuration><sourceDirs><sourceDir>${{project.basedir}}/src/test/kotlin</sourceDir><sourceDir>${{project.basedir}}/src/test/java</sourceDir></sourceDirs></configuration></execution>
        </executions>
      </plugin>
      <plugin>
        <artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version>
        <executions>
          <execution><id>default-compile</id><phase>none</phase></execution>
          <execution><id>default-testCompile</id><phase>none</phase></execution>
          <execution><id>java-compile</id><phase>compile</phase><goals><goal>compile</goal></goals></execution>
          <execution><id>java-test-compile</id><phase>test-compile</phase><goals><goal>testCompile</goal></goals></execution>
        </executions>
      </plugin>
      <plugin><artifactId>maven-surefire-plugin</artifactId><version>3.5.2</version></plugin>
    </plugins>
  </build>
</project>
"#
    )
}

fn kotlin_gradle() -> String {
    format!(
        "plugins {{ kotlin(\"jvm\") version \"{KOTLIN_VERSION}\" }}
repositories {{ mavenCentral() }}
java {{ targetCompatibility = JavaVersion.VERSION_21 }}
kotlin {{ compilerOptions {{ jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_21) }} }}
dependencies {{
    testImplementation(kotlin(\"test\"))
    testImplementation(\"org.junit.jupiter:junit-jupiter:5.11.4\")
    testRuntimeOnly(\"org.junit.platform:junit-platform-launcher\")
}}
tasks.test {{ useJUnitPlatform() }}
"
    )
}

fn kotlin_sources() -> [(&'static str, &'static str); 2] {
    [("src/main/kotlin/com/acme/Greeter.kt", GREETER), ("src/test/kotlin/com/acme/GreeterTest.kt", GREETER_TEST)]
}

fn shown_targets(window: &mut Window, file: &str) -> Vec<String> {
    window
        .targets(file)
        .iter()
        .map(|target| format!("{} {}", target["line"], target["name"].as_str().unwrap()))
        .collect()
}

#[test]
#[ignore = "needs Eclipse JDT LS with java-debug, JDKs, Maven and Gradle (see the module's documentation)"]
fn kotlin_runs_and_debugs_with_maven() {
    let pom = kotlin_pom();
    let mut window = window_with(&[("pom.xml", pom.as_str())], &kotlin_sources(), true);
    assert_eq!(shown_targets(&mut window, "src/main/kotlin/com/acme/Greeter.kt"), ["8 GreeterKt"]);
    let targets = window.targets("src/test/kotlin/com/acme/GreeterTest.kt");
    let shown: Vec<String> =
        targets.iter().map(|target| format!("{} {}", target["line"], target["name"].as_str().unwrap())).collect();
    assert_eq!(shown, ["6 com.acme.GreeterTest", "8 doubles", "14 fails"]);
    checked("kotlin gutter: ok");

    let configuration = with_tools(&targets[0], &maven_args(&window), project_java());
    let run = window.run("run.process.start", json!({ "configuration": configuration }))["runId"].as_u64().unwrap();
    let tests = window.tests(run);
    let mut shown = tree(&tests);
    assert!(!shown.is_empty(), "{}", window.output(run));
    shown[1..].sort();
    assert_eq!(shown, ["com.acme.GreeterTest failed", "  doubles passed", "  fails failed"], "{}", window.output(run));
    let failed = window.run("run.tests.get", json!({ "runId": run, "failedOnly": true }));
    let fails = &failed["failed"][0];
    assert_eq!(fails["line"], 15, "{failed}");
    assert!(fails["file"].as_str().unwrap().ends_with("src/test/kotlin/com/acme/GreeterTest.kt"), "{failed}");
    let rerun = window.run("run.tests.rerunFailed", json!({ "runId": run }))["runId"].as_u64().unwrap();
    assert_eq!(
        tree(&window.tests(rerun)),
        ["com.acme.GreeterTest failed", "  fails failed"],
        "{}",
        window.output(rerun)
    );
    checked("kotlin maven tests and rerun failed: ok");

    let maven_opts = vec![
        (
            "MAVEN_OPTS".to_owned(),
            format!("-Dmaven.repo.local={}", env_path("ZEN_MAVEN_REPO").join("repository").display()),
        ),
        ("JAVA_HOME".to_owned(), env_path("ZEN_PROJECT_JAVA_HOME").display().to_string()),
    ];
    let main = window.targets("src/main/kotlin/com/acme/Greeter.kt").remove(0);
    let mut program: Configuration = serde_json::from_value(main["configuration"].clone()).unwrap();
    program.options["jre"] = json!(env_path("ZEN_PROJECT_JAVA_HOME"));
    program.common.env = maven_opts;
    let program = serde_json::to_value(program).unwrap();
    let ran = window.run("run.process.start", json!({ "configuration": program.clone() }))["runId"].as_u64().unwrap();
    window.tests(ran);
    assert!(window.output(ran).contains("hello Kotlin 42"), "{}", window.output(ran));
    checked("kotlin maven application: ok");

    let greeter = window.path("src/main/kotlin/com/acme/Greeter.kt");
    window.run("debug.breakpoint.set", json!({ "path": greeter, "line": 11 }));
    let started = window.run("debug.session.start", json!({ "configuration": program.clone() }));
    let debug_run = started["runId"].as_u64().unwrap();
    let stopped = window.debug_state("the Kotlin program stops", "stopped");
    assert_eq!(stopped["frame"]["line"], 11, "{stopped}");
    let frames = window.run("debug.frames.list", json!({}));
    assert!(stopped["frame"]["path"].as_str().is_some_and(|path| path.ends_with("Greeter.kt")), "{stopped} {frames}");
    assert_eq!(window.local("name"), "\"Kotlin\"");
    assert_eq!(window.local("total"), "42");
    window.run("debug.breakpoint.remove", json!({ "path": greeter, "line": 11 }));
    window.run("debug.execution.resume", json!({}));
    window.debug_state("the Kotlin program ends", "ended");
    assert!(window.output(debug_run).contains("hello Kotlin 42"));
    checked("kotlin breakpoint binds and stops: ok");

    window.run("debug.breakpoint.set", json!({ "path": greeter, "line": 10 }));
    window.run("debug.session.start", json!({ "configuration": program }));
    window.debug_state("the call of the inline function stops", "stopped");
    window.run("debug.breakpoint.remove", json!({ "path": greeter, "line": 10 }));
    window.run("debug.execution.stepOver", json!({}));
    let stepped = window.debug_state("the step over the inlined call ends", "stopped");
    assert_eq!(stepped["frame"]["line"], 11, "{stepped}");
    assert!(stepped["frame"]["path"].as_str().is_some_and(|path| path.ends_with("Greeter.kt")), "{stepped}");
    checked("kotlin step over an inlined call: ok");
    window.run("debug.session.stop", json!({}));

    let test_file = window.path("src/test/kotlin/com/acme/GreeterTest.kt");
    window.run("debug.breakpoint.set", json!({ "path": test_file, "line": 10 }));
    let configuration = with_tools(&targets[1], &maven_args(&window), project_java());
    window.run("debug.session.start", json!({ "configuration": configuration }));
    let stopped = window.debug_state("the Kotlin test stops", "stopped");
    assert_eq!(stopped["frame"]["line"], 10, "{stopped}");
    assert_eq!(window.local("sum"), "4");
    window.run("debug.breakpoint.remove", json!({ "path": test_file, "line": 10 }));
    window.run("debug.execution.resume", json!({}));
    window.debug_state("the Kotlin test ends", "ended");
    checked("kotlin maven debug a test: ok");
}

#[test]
#[ignore = "needs Eclipse JDT LS with java-debug, JDKs, Maven and Gradle (see the module's documentation)"]
fn kotlin_runs_and_debugs_with_gradle() {
    let build = kotlin_gradle();
    let mut window = window_with(
        &[("settings.gradle.kts", "rootProject.name = \"greeter\"\n"), ("build.gradle.kts", build.as_str())],
        &kotlin_sources(),
        false,
    );
    let targets = window.targets("src/test/kotlin/com/acme/GreeterTest.kt");
    let env = {
        let mut env = project_java();
        env.push(("GRADLE_USER_HOME".to_owned(), env_path("ZEN_GRADLE_USER_HOME").display().to_string()));
        env
    };
    let configuration = with_tools(&targets[0], "", env.clone());
    let run = window.run("run.process.start", json!({ "configuration": configuration }))["runId"].as_u64().unwrap();
    let tests = window.tests(run);
    let mut shown = tree(&tests);
    assert!(!shown.is_empty(), "{}", window.output(run));
    shown[1..].sort();
    assert_eq!(shown, ["com.acme.GreeterTest failed", "  doubles passed", "  fails failed"], "{}", window.output(run));
    let rerun = window.run("run.tests.rerunFailed", json!({ "runId": run }))["runId"].as_u64().unwrap();
    assert_eq!(
        tree(&window.tests(rerun)),
        ["com.acme.GreeterTest failed", "  fails failed"],
        "{}",
        window.output(rerun)
    );
    checked("kotlin gradle tests and rerun failed: ok");

    let main = window.targets("src/main/kotlin/com/acme/Greeter.kt").remove(0);
    let mut program: Configuration = serde_json::from_value(main["configuration"].clone()).unwrap();
    program.options["jre"] = json!(env_path("ZEN_PROJECT_JAVA_HOME"));
    program.common.env = env.clone();
    let program = serde_json::to_value(program).unwrap();
    let ran = window.run("run.process.start", json!({ "configuration": program.clone() }))["runId"].as_u64().unwrap();
    window.tests(ran);
    assert!(window.output(ran).contains("hello Kotlin 42"), "{}", window.output(ran));
    checked("kotlin gradle application: ok");

    let test_file = window.path("src/test/kotlin/com/acme/GreeterTest.kt");
    window.run("debug.breakpoint.set", json!({ "path": test_file, "line": 10 }));
    let configuration = with_tools(&targets[1], "", env);
    window.run("debug.session.start", json!({ "configuration": configuration }));
    let stopped = window.debug_state("the Kotlin test stops", "stopped");
    assert_eq!(stopped["frame"]["line"], 10, "{stopped}");
    assert_eq!(window.local("sum"), "4");
    window.run("debug.breakpoint.remove", json!({ "path": test_file, "line": 10 }));
    window.run("debug.execution.resume", json!({}));
    window.debug_state("the Kotlin test ends", "ended");
    checked("kotlin gradle debug a test: ok");
}

fn plain_window() -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("Shop.java");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, SHOP).unwrap();
    let java = env_path("ZEN_JDTLS_JAVA_HOME");
    let classes = folder.path().join("out");
    let compiled = std::process::Command::new(java.join("bin/javac"))
        .args(["-g", "-d"])
        .arg(&classes)
        .arg(&source)
        .status()
        .unwrap();
    assert!(compiled.success());
    let packed = std::process::Command::new(java.join("bin/jar"))
        .args(["--create", "--file"])
        .arg(folder.path().join("shop.jar"))
        .args(["--main-class", "Shop", "-C"])
        .arg(&classes)
        .arg(".")
        .status()
        .unwrap();
    assert!(packed.success());
    let root = folder.path().canonicalize().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("language-servers")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(env_path("ZEN_JDTLS"), data.path().join("language-servers").join(SERVER)).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        Arc::new(zen_workbench::language::servers::SystemTools),
    );
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/java/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    servers.declare(ServerDeclaration::parse_list(&manifest["languageServers"].to_string()).unwrap());
    let server_java = java.clone();
    servers.set_java_finder(Arc::new(move |_, _| Some(server_java.clone())));
    session.set_state(servers);
    let providers: Vec<Arc<dyn RunProvider>> = vec![Arc::new(zen_workbench::java_run::JavaRun::in_folders(Vec::new()))];
    session.state_mut::<zen_workbench::run::Runs>().set_providers(providers, zen_run::RunEnv::default());
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    let mut window = Window { registry, session, _folder: folder, root, _data: data };
    let shop = window.path("Shop.java");
    window.run("file.document.open", json!({ "path": shop }));
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        window.run("language.servers.sync", json!({}));
        let answer = window.run("problems.file.list", json!({ "path": shop }));
        if answer["checked"] == true || answer.as_array().is_some_and(Vec::is_empty) {
            break;
        }
        if Instant::now() > deadline {
            let log = window.run("lsp.server.log", json!({ "id": SERVER }));
            let servers = window.run("lsp.servers.list", json!({}));
            panic!("not checked: {answer}\n{servers}\n{log}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    window
}

fn children(window: &mut Window, reference: &Value) -> Vec<Value> {
    window.run("debug.variables.list", json!({ "reference": reference }))["variables"].as_array().unwrap().clone()
}

fn values(variables: &[Value]) -> Vec<String> {
    variables.iter().map(|variable| variable["value"].as_str().unwrap_or_default().to_owned()).collect()
}

#[test]
#[ignore = "needs Eclipse JDT LS with java-debug and a JDK 25: ZEN_JDTLS, ZEN_JDTLS_JAVA_HOME"]
fn java_debug_shows_collections_by_their_elements_and_traces_a_stream_chain() {
    let mut window = plain_window();
    let shop = window.path("Shop.java");
    window.run("debug.breakpoint.set", json!({ "path": shop, "line": 11 }));
    let program = json!({ "name": "Shop", "kind": "java.jar",
        "options": { "jar": "shop.jar", "jre": env_path("ZEN_JDTLS_JAVA_HOME") } });
    window.run("debug.session.start", json!({ "configuration": program }));
    let stopped = window.debug_state("the program stops", "stopped");
    assert_eq!(stopped["frame"]["line"], 11, "{stopped}");
    let scopes = window.run("debug.variables.list", json!({}));
    let locals = children(&mut window, &scopes["variables"][0]["reference"]);
    let local = |name: &str| locals.iter().find(|local| local["name"] == name).unwrap()["reference"].clone();
    let names = children(&mut window, &local("names"));
    assert_eq!(values(&names), ["\"ann\"", "\"bob\"", "\"cid\"", "\"dora\""]);
    assert_eq!(values(&children(&mut window, &local("numbers"))), ["3", "1", "2"]);
    assert_eq!(values(&children(&mut window, &local("tags"))), ["\"x\"", "\"y\""]);
    let ages = children(&mut window, &local("ages"));
    let entries: Vec<(String, String)> = ages
        .iter()
        .map(|entry| (entry["entry"]["key"].to_string(), entry["entry"]["value"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(entries.len(), 2, "{ages:?}");
    assert!(entries[0].0.contains("ann") && entries[0].1.ends_with("\"30\""), "{entries:?}");
    checked(&format!("elements view: ok ({entries:?})"));

    let raw = window.run("debug.variables.list", json!({ "reference": local("names"), "raw": true }));
    let fields: Vec<&str> =
        raw["variables"].as_array().unwrap().iter().map(|field| field["name"].as_str().unwrap()).collect();
    assert!(fields.contains(&"elementData") && fields.contains(&"size"), "{raw}");
    let again = window.run("debug.variables.list", json!({ "reference": local("names") }));
    assert_eq!(values(again["variables"].as_array().unwrap()), values(&names));
    window.run("debug.variables.raw", json!({ "raw": true }));
    let tags = children(&mut window, &local("tags"));
    assert!(!tags.is_empty() && tags.iter().all(|field| field["name"] != "0"), "{tags:?}");
    window.run("debug.variables.raw", json!({ "raw": false }));
    checked(&format!("raw fields: ok ({fields:?})"));

    let trace = window.run("debug.stream.trace", json!({}));
    let columns: Vec<(String, usize)> = trace["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| (column["label"].as_str().unwrap().to_owned(), column["values"].as_array().unwrap().len()))
        .collect();
    assert_eq!(
        columns,
        [
            ("names.stream()".to_owned(), 4),
            ("filter(n -> n.length() == 3)".to_owned(), 3),
            ("map(String::length)".to_owned(), 3),
            ("collect(Collectors.toList())".to_owned(), 3),
        ],
        "{trace}"
    );
    assert!(trace["columns"][2]["values"][0].as_str().unwrap().ends_with("\"3\""), "{trace}");
    checked(&format!("stream trace: ok ({})", trace["chain"]));
    window.run("debug.session.stop", json!({}));
}

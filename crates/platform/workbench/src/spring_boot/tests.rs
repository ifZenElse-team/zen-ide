use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_run::{Configuration, RunContext, RunEnv, RunProvider};

use super::config::{self, Format, Place};
use super::metadata::{self, Metadata};
use super::run::{KIND, SpringBootOptions, SpringBootRun, annotated_class, spring_arguments};
use crate::ide_settings::zip::ZipWriter;

const WEB: &str = r#"{
  "groups": [{ "name": "server", "type": "org.springframework.boot.autoconfigure.web.ServerProperties" }],
  "properties": [
    { "name": "server.port", "type": "java.lang.Integer", "description": "Server HTTP port.", "defaultValue": 8080,
      "sourceType": "org.springframework.boot.autoconfigure.web.ServerProperties" },
    { "name": "server.servlet.context-path", "type": "java.lang.String", "description": "Context path of the application." },
    { "name": "server.compression.enabled", "type": "java.lang.Boolean", "defaultValue": false },
    { "name": "logging.level", "type": "java.util.Map<java.lang.String,java.lang.String>", "description": "Log levels." },
    { "name": "spring.profiles.include", "type": "java.util.List<java.lang.String>" },
    { "name": "server.max-http-header-size", "type": "org.springframework.util.unit.DataSize",
      "deprecation": { "level": "error", "replacement": "server.max-http-request-header-size", "reason": "Renamed." } },
    { "name": "spring.datasource.initialization-mode", "type": "java.lang.String", "deprecated": true }
  ],
  "hints": [
    { "name": "logging.level.values", "values": [{ "value": "debug" }, { "value": "info", "description": "Info level." }] }
  ]
}"#;

const ADDITIONAL: &str = r#"{
  "properties": [
    { "name": "server.port", "description": "Ignored: the main file says it first." },
    { "name": "management.server.port", "type": "java.lang.Integer", "description": "Management port." }
  ],
  "hints": [{ "name": "server.port", "values": [{ "value": 8080 }, { "value": 0, "description": "A free port." }] }]
}"#;

const OWN: &str = r#"{ "properties": [
  { "name": "shop.title", "type": "java.lang.String", "description": "The shop's title.", "sourceType": "shop.ShopProperties" }
] }"#;

fn jar(path: &Path, entries: &[(&str, &str)]) {
    let mut writer = ZipWriter::new(0);
    for (name, data) in entries {
        writer.add(name, data.as_bytes()).unwrap();
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, writer.finish()).unwrap();
}

#[test]
fn properties_are_found_by_their_relaxed_names_map_keys_and_list_indexes() {
    let metadata = Metadata::parse(WEB.as_bytes()).unwrap();
    assert_eq!(metadata.len(), 7);
    let name = |key: &str| metadata.find(key).map(|property| property.name.clone());
    assert_eq!(name("server.port").as_deref(), Some("server.port"));
    assert_eq!(name("server.servlet.contextPath").as_deref(), Some("server.servlet.context-path"));
    assert_eq!(name("SERVER.SERVLET.CONTEXT_PATH").as_deref(), Some("server.servlet.context-path"));
    assert_eq!(name("logging.level.org.hibernate.SQL").as_deref(), Some("logging.level"));
    assert_eq!(name("logging.level.[org.hibernate]").as_deref(), Some("logging.level"));
    assert_eq!(name("spring.profiles.include[0]").as_deref(), Some("spring.profiles.include"));
    assert_eq!(name("server.port.extra"), None, "an Integer takes no keys");
    assert_eq!(name("server.nope"), None);
    let deprecated = metadata.get("spring.datasource.initialization-mode").unwrap();
    assert!(deprecated.deprecation.is_some(), "the old `deprecated: true` form counts");
    assert!(metadata.get("server.max-http-header-size").unwrap().deprecation.as_ref().unwrap().is_error());
    let level = metadata.get("logging.level").unwrap();
    assert_eq!(level.values.len(), 2, "a map's `.values` hint is its values");
    assert_eq!(metadata.get("server.port").unwrap().default_text().as_deref(), Some("8080"));
}

#[test]
fn metadata_is_read_from_the_projects_jars_and_its_own_build_output() {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("repo/spring-boot-autoconfigure.jar");
    jar(
        &web,
        &[
            ("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n"),
            (metadata::MAIN, WEB),
            (metadata::ADDITIONAL, ADDITIONAL),
        ],
    );
    let plain = dir.path().join("repo/plain.jar");
    jar(&plain, &[("a/B.class", "")]);
    let broken = dir.path().join("repo/broken.jar");
    jar(&broken, &[(metadata::MAIN, "{ not json")]);
    let root = dir.path().join("shop");
    let own = metadata::own_files(&root, &[]);
    std::fs::create_dir_all(own[0].parent().unwrap()).unwrap();
    std::fs::write(&own[0], OWN).unwrap();
    let loaded = metadata::load(&[web.clone(), plain, broken, dir.path().join("missing.jar")], &own);
    assert_eq!(loaded.len(), 9);
    let port = loaded.get("server.port").unwrap();
    assert_eq!(port.description.as_deref(), Some("Server HTTP port."));
    assert_eq!(port.values.len(), 2, "the additional metadata's hints join");
    assert!(loaded.get("management.server.port").is_some());
    assert_eq!(loaded.get("shop.title").unwrap().source_type.as_deref(), Some("shop.ShopProperties"));
    assert!(Arc::ptr_eq(&metadata::of_jar(&web), &metadata::of_jar(&web)), "a jar is read once while it is unchanged");
}

#[test]
fn the_jars_come_from_the_library_model_of_the_build() {
    struct Listed(Vec<crate::jvm_libraries::Coordinates>);
    impl crate::jvm_libraries::LibrarySource for Listed {
        fn list(
            &self,
            _root: &Path,
            _env: &crate::build_tools::BuildEnv,
            _places: &crate::jvm_libraries::Places,
        ) -> Option<Vec<crate::jvm_libraries::Listed>> {
            Some(
                self.0
                    .iter()
                    .map(|coordinates| crate::jvm_libraries::Listed {
                        coordinates: coordinates.clone(),
                        tool: crate::java_run::BuildTool::Maven,
                        jar: None,
                    })
                    .collect(),
            )
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("shop");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
    let places = crate::jvm_libraries::Places {
        maven: dir.path().join("m2"),
        gradle: dir.path().join("gradle"),
        cache: dir.path().join("cache"),
    };
    let boot = crate::jvm_libraries::Coordinates::new("org.springframework.boot", "spring-boot", "4.1.2");
    let jar_path = places.maven.join("org/springframework/boot/spring-boot/4.1.2/spring-boot-4.1.2.jar");
    jar(&jar_path, &[(metadata::MAIN, WEB)]);
    let missing = crate::jvm_libraries::Coordinates::new("a", "b", "1");
    let libraries = crate::jvm_libraries::JvmLibraries::with_places(
        crate::build_tools::BuildEnv::default(),
        places,
        vec![Box::new(Listed(vec![boot.clone(), missing, boot]))],
    );
    assert_eq!(libraries.jars(&root), [jar_path]);
    assert!(libraries.jars(dir.path()).is_empty(), "a folder without a build has no jars");
}

const YAML: &str = "spring:\n  profiles:\n    include:\n    - dev\n  datasource:\n    url: jdbc:h2:mem:db # memory\nserver:\n  port: 8080\n  servlet:\n    context-path: /shop\nlogging:\n  level:\n    \"[org.hibernate]\": debug\nitems:\n  - name: a\n    size: 1\ntext: |\n  not: a key\n---\nshop.title: Shop\n";

#[test]
fn yaml_keys_have_their_full_names_ranges_and_values() {
    let keys = config::keys(Format::Yaml, YAML);
    let leaves: Vec<(&str, bool)> =
        keys.iter().filter(|key| key.leaf).map(|key| (key.name.as_str(), key.listed)).collect();
    assert_eq!(
        leaves,
        [
            ("spring.profiles.include", false),
            ("spring.datasource.url", false),
            ("server.port", false),
            ("server.servlet.context-path", false),
            ("logging.level.[org.hibernate]", false),
            ("items.name", true),
            ("items.size", true),
            ("text", false),
            ("shop.title", false),
        ]
    );
    let url = keys.iter().find(|key| key.name == "spring.datasource.url").unwrap();
    assert_eq!(&YAML[url.range.clone()], "url");
    assert_eq!(&YAML[url.value.clone().unwrap()], "jdbc:h2:mem:db");
    assert_eq!(config::indent_unit(&keys), 2);
    let properties = config::keys(Format::Properties, "a.b = 1\n# c=2\nd\\ e:3\n");
    let names: Vec<&str> = properties.iter().map(|key| key.name.as_str()).collect();
    assert_eq!(names, ["a.b", "d e"]);
}

#[test]
fn the_caret_is_on_a_key_or_on_a_value() {
    let at = |text: &str| text.find('|').unwrap();
    let place = |format, text: &str| config::place_at(format, &text.replace('|', ""), at(text));
    assert_eq!(
        place(Format::Properties, "a=1\nserver.po|"),
        Some(Place::Key { from: 4, parent: String::new(), indent: 0 })
    );
    assert_eq!(
        place(Format::Properties, "server.port = 80|"),
        Some(Place::Value { from: 14, key: "server.port".into() })
    );
    assert_eq!(place(Format::Properties, "# server|"), None);
    assert_eq!(
        place(Format::Yaml, "server:\n  port: 1\n  servlet:\n    co|"),
        Some(Place::Key { from: 33, parent: "server.servlet".into(), indent: 4 })
    );
    assert_eq!(
        place(Format::Yaml, "server:\n  po|"),
        Some(Place::Key { from: 10, parent: "server".into(), indent: 2 })
    );
    assert_eq!(
        place(Format::Yaml, "server:\n  compression:\n    enabled: t|"),
        Some(Place::Value { from: 36, key: "server.compression.enabled".into() })
    );
    assert_eq!(config::yaml_insert("servlet.context-path", 2, 2, "\n"), "servlet:\n    context-path: ");
    assert_eq!(config::format_of(Path::new("/p/application-dev.yml")), Some(Format::Yaml));
    assert_eq!(config::format_of(Path::new("/p/application.properties")), Some(Format::Properties));
    assert_eq!(config::format_of(Path::new("/p/application-.yml")), None);
    assert_eq!(config::format_of(Path::new("/p/messages.properties")), None);
}

fn spring_plugin() -> crate::plugin_projects::PluginProject {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/spring-boot/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    crate::plugin_projects::PluginProject {
        id: super::PLUGIN.to_owned(),
        name: "Spring Boot".to_owned(),
        support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new(plugin: bool) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("shop");
        let resources = root.join("src/main/resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        std::fs::write(resources.join("application.properties"), "server.port=8080\nserver.nope=1\nspring.datasource.initialization-mode=always\nserver.max-http-header-size=8KB\nlogging.level.root=info\n\n").unwrap();
        std::fs::write(resources.join("application-dev.yml"), "server:\n  servlet:\n    context-path: /x\n  \n")
            .unwrap();
        let web = dir.path().join("repo/spring-boot-autoconfigure.jar");
        jar(&web, &[(metadata::MAIN, WEB), (metadata::ADDITIONAL, ADDITIONAL)]);
        let mut session = Session::new();
        if plugin {
            session.state_mut::<crate::plugin_projects::PluginProjects>().set(vec![spring_plugin()]);
        }
        session.state_mut::<super::SpringBoot>().set_jars(Arc::new(move |_: &Path| vec![web.clone()]));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        Self { registry, session, _dir: dir, root }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.root.join("src/main/resources").join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap()
    }

    fn open_at(&mut self, path: &Path, offset: usize) {
        self.run("navigate.place.goTo", json!({ "path": path, "offset": offset }));
    }

    fn text(&self, path: &Path) -> String {
        let workbench = self.session.state::<crate::Workbench>().unwrap();
        workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
    }

    fn loaded(&mut self) -> Value {
        self.run("springBoot.properties.list", json!({ "waitMs": 60_000, "limit": 3 }))
    }

    fn complete(&mut self, path: &Path, offset: usize) -> Value {
        self.open_at(path, offset);
        self.loaded();
        self.run("editor.completion.start", json!({}))
    }
}

fn labels(list: &Value) -> Vec<String> {
    list["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).map(str::to_owned).collect()
}

#[test]
fn an_agent_lists_the_properties_of_the_projects_jars() {
    let mut fixture = Fixture::new(true);
    let listed = fixture.loaded();
    assert_eq!(listed["loading"], false);
    assert_eq!(listed["jars"], 1);
    assert_eq!(listed["total"], 8);
    let found = fixture.run("springBoot.properties.list", json!({ "query": "contextPath" }));
    assert_eq!(found["properties"][0]["name"], "server.servlet.context-path");
    assert_eq!(found["properties"][0]["type"], "java.lang.String");
    let off = Fixture::new(false);
    let mut off = off;
    assert!(off.registry.dispatch(&mut off.session, "springBoot.properties.list", json!({})).is_err());
}

#[test]
fn keys_and_values_are_completed_in_application_properties() {
    let mut fixture = Fixture::new(true);
    let path = fixture.file("application.properties");
    let text = std::fs::read_to_string(&path).unwrap();
    let end = text.chars().count();
    let list = fixture.complete(&path, end);
    let offered = labels(&list);
    assert!(offered.contains(&"server.servlet.context-path".to_owned()), "{offered:?}");
    assert!(offered.contains(&"management.server.port".to_owned()));
    assert!(!offered.contains(&"server.port".to_owned()), "a key the file has is left out");
    let item =
        list["items"].as_array().unwrap().iter().find(|item| item["label"] == "server.servlet.context-path").unwrap();
    assert_eq!(item["detail"], "String");
    assert!(item["documentation"].as_str().unwrap().contains("Context path of the application."));
    let index = item["index"].clone();
    fixture.run("editor.completion.accept", json!({ "index": index }));
    let text = fixture.text(&path);
    assert!(text.ends_with("\nserver.servlet.context-path="), "{text:?}");

    let at = text.find("info").unwrap();
    let list = fixture.complete(&path, at);
    assert_eq!(labels(&list), ["debug", "info"], "a map's values come from its hint");
}

#[test]
fn nested_keys_are_completed_in_application_yml() {
    let mut fixture = Fixture::new(true);
    let path = fixture.file("application-dev.yml");
    let text = std::fs::read_to_string(&path).unwrap();
    let at = text.chars().count() - 1;
    let list = fixture.complete(&path, at);
    let offered = labels(&list);
    assert!(offered.contains(&"compression.enabled".to_owned()), "{offered:?}");
    assert!(offered.contains(&"port".to_owned()));
    assert!(!offered.iter().any(|label| label.contains("context-path")), "the file sets it already");
    assert!(!offered.iter().any(|label| label.starts_with("server.") || label.starts_with("logging")));
    let index =
        list["items"].as_array().unwrap().iter().find(|item| item["label"] == "compression.enabled").unwrap()["index"]
            .clone();
    fixture.run("editor.completion.accept", json!({ "index": index }));
    let text = fixture.text(&path);
    assert!(text.ends_with("  compression:\n    enabled: \n"), "{text:?}");
    let list = fixture.complete(&path, text.chars().count() - 1);
    assert_eq!(labels(&list), ["true", "false"]);
}

#[test]
fn unknown_and_deprecated_keys_are_reported_and_documented() {
    let mut fixture = Fixture::new(true);
    let path = fixture.file("application.properties");
    fixture.open_at(&path, 0);
    fixture.loaded();
    let problems = fixture.run("problems.file.list", json!({ "path": path }));
    let found: Vec<(String, String)> = problems["problems"]
        .as_array()
        .or_else(|| problems.as_array())
        .unwrap()
        .iter()
        .map(|problem| {
            (
                problem["severity"].as_str().unwrap_or_default().to_owned(),
                problem["message"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        found.contains(&("weakWarning".to_owned(), "'server.nope' is not a known configuration property".to_owned())),
        "{found:?}"
    );
    assert!(found.iter().any(|(severity, message)| severity == "warning"
        && message == "'spring.datasource.initialization-mode' is deprecated"));
    assert!(found.iter().any(|(severity, message)| severity == "error"
        && message.ends_with("the property to use is 'server.max-http-request-header-size'")));
    assert_eq!(found.len(), 3, "server.port and the logging.level map key are known: {found:?}");

    let doc = fixture.run("editor.documentation.show", json!({ "path": path, "offset": 3 }));
    let markdown = doc["markdown"].as_str().unwrap();
    assert!(markdown.starts_with("**server.port**\n\n`java.lang.Integer`\n\nServer HTTP port."), "{markdown}");
    assert!(markdown.contains("Default: `8080`"));
    assert_eq!(doc["range"], json!([0, 11]));
}

#[test]
fn without_the_plugin_application_files_are_left_alone() {
    let mut fixture = Fixture::new(false);
    let path = fixture.file("application.properties");
    fixture.open_at(&path, 0);
    let problems = fixture.run("problems.file.list", json!({ "path": path }));
    assert!(!problems.to_string().contains("Cannot resolve"), "{problems}");
    assert!(super::complete(&mut fixture.session, &path, "server.p", 8).is_none());
}

const JAVA_APP: &str = "package shop;\n\nimport org.springframework.boot.SpringApplication;\n\n@SpringBootApplication(scanBasePackageClasses = Shop.class)\npublic class ShopApplication {\n    public static void main(String[] args) {\n        SpringApplication.run(ShopApplication.class, args);\n    }\n}\n";

#[test]
fn spring_boot_applications_are_found_in_java_and_kotlin() {
    assert_eq!(annotated_class(JAVA_APP).as_deref(), Some("ShopApplication"));
    assert_eq!(annotated_class("@SpringBootApplication\nopen class App\n\nfun main() {}\n").as_deref(), Some("App"));
    assert_eq!(annotated_class("// @SpringBootApplication\nclass App\n"), None);
    assert_eq!(annotated_class("@SpringBootApplicationX\nclass App\n"), None);
    let options = SpringBootOptions {
        active_profiles: " dev, local ".to_owned(),
        overrides: vec![("server.port".to_owned(), "9090".to_owned()), (" ".to_owned(), "x".to_owned())],
        ..SpringBootOptions::default()
    };
    assert_eq!(spring_arguments(&options), ["--spring.profiles.active=dev,local", "--server.port=9090"]);
}

#[test]
fn a_spring_boot_configuration_runs_as_a_java_application_with_profiles_and_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("shop");
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    std::fs::write(root.join("src/main/java/shop/ShopApplication.java"), JAVA_APP).unwrap();
    let jdk = dir.path().join("jdk");
    std::fs::create_dir_all(jdk.join("bin")).unwrap();
    for program in ["java", "javac"] {
        std::fs::write(jdk.join("bin").join(program), "").unwrap();
    }
    let mut configuration = SpringBootRun::configuration("ShopApplication", "shop.ShopApplication", "");
    let mut options: SpringBootOptions = serde_json::from_value(configuration.options.clone()).unwrap();
    options.active_profiles = "dev".to_owned();
    options.overrides = vec![("app.greeting".to_owned(), "hello world".to_owned())];
    configuration.options = serde_json::to_value(options).unwrap();
    configuration.common.program_arguments = "--verbose".to_owned();
    let java = SpringBootRun::as_java(&configuration);
    assert_eq!(java.kind, crate::java_run::APPLICATION);
    assert_eq!(java.common.program_arguments, "--verbose --spring.profiles.active=dev \"--app.greeting=hello world\"");
    let context = RunContext {
        root: root.clone(),
        scratch: root.join(".scratch"),
        env: RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] },
        ..RunContext::default()
    };
    let lines = SpringBootRun::default().command_lines(&configuration, &context).unwrap();
    let run = lines.last().unwrap();
    assert_eq!(run.program, jdk.join("bin/java"));
    let tail: Vec<&str> = run.args.iter().rev().take(4).rev().map(String::as_str).collect();
    assert_eq!(
        tail,
        ["shop.ShopApplication", "--verbose", "--spring.profiles.active=dev", "--app.greeting=hello world"]
    );
    let other = Configuration { kind: "java.application".to_owned(), ..configuration };
    assert!(SpringBootRun::default().command_lines(&other, &context).is_err());
}

#[test]
fn the_gutter_runs_an_application_class_as_spring_boot_while_the_plugin_is_on() {
    let mut fixture = Fixture::new(true);
    let java = fixture.root.join("src/main/java/shop");
    std::fs::create_dir_all(&java).unwrap();
    let file = java.join("ShopApplication.java");
    std::fs::write(&file, JAVA_APP).unwrap();
    std::fs::write(java.join("Tool.java"), "package shop;\nclass Tool {\n public static void main(String[] a) {}\n}\n")
        .unwrap();
    let providers: Vec<Arc<dyn RunProvider>> =
        vec![Arc::new(SpringBootRun::default()), Arc::new(crate::java_run::JavaRun::default())];
    fixture.session.state_mut::<crate::run::Runs>().set_providers(providers, RunEnv::default());
    let targets = fixture.run("run.targets.list", json!({ "path": file, "full": true }));
    let shown: Vec<(String, u64, String)> = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|target| {
            (
                target["provider"].as_str().unwrap().to_owned(),
                target["line"].as_u64().unwrap(),
                target["configuration"]["kind"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(shown, [("springBoot".to_owned(), 6, KIND.to_owned()), ("springBoot".to_owned(), 7, KIND.to_owned())]);
    let context = fixture
        .run("run.context.get", json!({ "path": file, "offset": JAVA_APP.find("SpringApplication.run").unwrap() }));
    assert_eq!(context["configuration"]["kind"], KIND);
    assert_eq!(context["configuration"]["name"], "ShopApplication");
    assert_eq!(context["configuration"]["options"]["mainClass"], "shop.ShopApplication");
    let tool = fixture.run("run.targets.list", json!({ "path": java.join("Tool.java") }));
    assert_eq!(tool["targets"][0]["provider"], "java", "a plain main stays a Java application");
    let applications = fixture.run("springBoot.applications.list", json!({}));
    assert_eq!(applications["applications"].as_array().unwrap().len(), 1);
    assert_eq!(applications["applications"][0]["line"], 6);
    assert_eq!(applications["applications"][0]["configuration"]["kind"], KIND);

    fixture.session.state_mut::<crate::plugin_projects::PluginProjects>().set(Vec::new());
    let targets = fixture.run("run.targets.list", json!({ "path": file }));
    assert!(targets["targets"].as_array().unwrap().iter().all(|target| target["provider"] == "java"));
}

#[test]
fn a_kotlin_application_runs_its_top_level_main() {
    let env = crate::build_tools::BuildEnv::default();
    env.set_jvm_reader(Some(Arc::new(|path: &Path, _: &str| {
        let main = zen_syntax::JvmMain { class: String::new(), class_line: 5, line: 5, end_line: 7 };
        let tool = zen_syntax::JvmMain { class: "shop.Tool".into(), class_line: 9, line: 11, end_line: 11 };
        let file = zen_syntax::JvmFile { package: "shop".into(), mains: vec![main, tool], ..Default::default() };
        Some(file.named_after(&path.file_stem()?.to_string_lossy()))
    })));
    let text = "package shop\n\n@SpringBootApplication\nclass ShopApplication\n\nfun main(args: Array<String>) {\n    runApplication<ShopApplication>(*args)\n}\n\nobject Tool {\n    @JvmStatic\n    fun main(args: Array<String>) {}\n}\n";
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("src/main/kotlin/shop/ShopApplication.kt");
    let provider = SpringBootRun::new(crate::java_run::JavaRun::default().with_env(env));
    let found = provider.applications(dir.path(), &file, text);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].1.name, "ShopApplication");
    assert_eq!(found[0].1.options["mainClass"], "shop.ShopApplicationKt");
    let context = RunContext { root: dir.path().to_owned(), ..RunContext::default() };
    let lines: Vec<u32> = provider.targets(&context, &file, text).iter().map(|target| target.line).collect();
    assert_eq!(lines, [5]);
}

const SERVICE: &str = r#"package shop;

@Service
public class OrderService implements Orders, Auditable {
    private final OrderRepository repository;

    public OrderService(OrderRepository repository, @Qualifier("x") Clock clock) {
        this.repository = repository;
    }
}
"#;

const REPOSITORY: &str =
    "package shop;\n\n@Repository\npublic class JdbcOrderRepository implements OrderRepository {\n}\n";

const CONTROLLER: &str = r#"package shop;

@RestController
@RequestMapping("/api/orders")
public class OrderController {
    @Autowired
    private Orders orders;

    @GetMapping
    public List<Order> all() { return orders.all(); }

    @GetMapping("/{id}")
    public Order one(@PathVariable long id) { return null; }

    @PostMapping(path = "/", consumes = "application/json")
    public Order create(@RequestBody Order order) { return order; }

    @RequestMapping(value = {"/a", "/b"}, method = {RequestMethod.PUT, RequestMethod.PATCH})
    public void both() {}
}
"#;

const CONFIG: &str = r#"package shop;

@Configuration
public class ClockConfig {
    /** @Bean in a comment */
    @Bean
    public Clock clock() { return Clock.systemUTC(); }

    @Autowired
    void setRepository(OrderRepository repository) {}
}
"#;

const KOTLIN_CONTROLLER: &str = r#"package shop

@RestController
@RequestMapping("/kt")
class StockController(private val orders: Orders, val clock: Clock?) {
    @Autowired
    lateinit var repository: OrderRepository

    @DeleteMapping("/{id}")
    fun remove(@PathVariable id: Long) {}

    @Bean
    fun stock(): Stock = Stock()
}
"#;

#[test]
fn beans_injection_points_and_mappings_are_read_from_java_and_kotlin() {
    let service = super::source::read(Path::new("OrderService.java"), SERVICE);
    assert_eq!(service.beans.len(), 1);
    assert_eq!(service.beans[0].type_name, "OrderService");
    assert_eq!(service.beans[0].supertypes, ["Orders", "Auditable"]);
    assert_eq!(service.beans[0].line, 3);
    let injected: Vec<(&str, &str, u32)> =
        service.injections.iter().map(|i| (i.type_name.as_str(), i.name.as_str(), i.line)).collect();
    assert_eq!(injected, [("OrderRepository", "repository", 6), ("Clock", "clock", 6)]);

    let controller = super::source::read(Path::new("OrderController.java"), CONTROLLER);
    let injected: Vec<(&str, &str)> =
        controller.injections.iter().map(|i| (i.type_name.as_str(), i.name.as_str())).collect();
    assert_eq!(injected, [("Orders", "orders")]);
    let mapped: Vec<(String, String, String)> =
        controller.endpoints.iter().map(|e| (e.method.clone(), e.path.clone(), e.handler.clone())).collect();
    let mapped: Vec<(&str, &str, &str)> = mapped.iter().map(|(m, p, h)| (m.as_str(), p.as_str(), h.as_str())).collect();
    assert_eq!(
        mapped,
        [
            ("GET", "/api/orders", "OrderController.all"),
            ("GET", "/api/orders/{id}", "OrderController.one"),
            ("POST", "/api/orders", "OrderController.create"),
            ("PUT", "/api/orders/a", "OrderController.both"),
            ("PATCH", "/api/orders/a", "OrderController.both"),
            ("PUT", "/api/orders/b", "OrderController.both"),
            ("PATCH", "/api/orders/b", "OrderController.both"),
        ]
    );
    assert_eq!(controller.endpoints[1].line, 12);

    let config = super::source::read(Path::new("ClockConfig.java"), CONFIG);
    let beans: Vec<(&str, &str)> = config.beans.iter().map(|b| (b.type_name.as_str(), b.name.as_str())).collect();
    assert_eq!(beans, [("ClockConfig", "ClockConfig"), ("Clock", "clock")]);
    assert_eq!(config.injections[0].type_name, "OrderRepository", "an @Autowired setter");

    let kotlin = super::source::read(Path::new("StockController.kt"), KOTLIN_CONTROLLER);
    let injected: Vec<(&str, &str)> =
        kotlin.injections.iter().map(|i| (i.type_name.as_str(), i.name.as_str())).collect();
    assert_eq!(injected, [("Orders", "orders"), ("Clock", "clock"), ("OrderRepository", "repository")]);
    assert_eq!(kotlin.endpoints[0].path, "/kt/{id}");
    assert_eq!(kotlin.endpoints[0].method, "DELETE");
    assert!(kotlin.beans.iter().any(|bean| bean.type_name == "Stock"));
    assert!(super::source::read(Path::new("A.java"), "class A { @Autowired Foo foo; }").injections.len() == 1);
    for text in ["", "@", "class", "@Service class", "@Service class X(", "@GetMapping(\"/x\" fun", "\"unclosed"] {
        let _ = super::source::read(Path::new("A.kt"), text);
        let _ = super::source::read(Path::new("A.java"), text);
    }
}

fn web_project(fixture: &Fixture) -> PathBuf {
    let java = fixture.root.join("src/main/java/shop");
    std::fs::create_dir_all(&java).unwrap();
    for (name, text) in [
        ("OrderService.java", SERVICE),
        ("JdbcOrderRepository.java", REPOSITORY),
        ("OrderController.java", CONTROLLER),
        ("ClockConfig.java", CONFIG),
    ] {
        std::fs::write(java.join(name), text).unwrap();
    }
    std::fs::create_dir_all(fixture.root.join("target/classes")).unwrap();
    std::fs::write(fixture.root.join("target/classes/Ignored.java"), "@Service class Ignored {}").unwrap();
    java
}

#[test]
fn gutter_marks_lead_from_beans_to_injection_points_and_back() {
    let mut fixture = Fixture::new(true);
    let java = web_project(&fixture);
    let service = java.join("OrderService.java");
    let marks = fixture.run("editor.lineMarkers.list", json!({ "path": service, "waitMs": 60_000 }));
    let shown: Vec<(u64, String, usize)> = marks["markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["line"].as_u64().unwrap(),
                m["kind"].as_str().unwrap().to_owned(),
                m["targets"].as_array().unwrap().len(),
            )
        })
        .collect();
    assert_eq!(shown, [(4, "bean".to_owned(), 1), (7, "injection".to_owned(), 2)]);
    assert_eq!(marks["markers"][0]["targets"][0]["name"], "OrderController.orders", "injected by its interface");
    let candidates = &marks["markers"][1]["targets"];
    assert_eq!(candidates[0]["name"], "JdbcOrderRepository");
    assert_eq!(candidates[1]["name"], "clock", "the @Bean method of type Clock");

    let navigated = fixture.run("editor.lineMarkers.navigate", json!({ "path": service, "line": 4 }));
    assert_eq!(navigated["navigated"], true);
    let active = fixture.session.state::<crate::Workbench>().unwrap().active_document().unwrap().path().to_owned();
    assert_eq!(active, java.join("OrderController.java"));
    let choose = fixture.run("editor.lineMarkers.navigate", json!({ "path": service, "line": 7 }));
    assert_eq!(choose["navigated"], false);
    assert_eq!(choose["targets"].as_array().unwrap().len(), 2);
    fixture.run("editor.lineMarkers.navigate", json!({ "path": service, "line": 7, "index": 1 }));
    let active = fixture.session.state::<crate::Workbench>().unwrap().active_document().unwrap().path().to_owned();
    assert_eq!(active, java.join("ClockConfig.java"));

    fixture.session.state_mut::<crate::plugin_projects::PluginProjects>().set(Vec::new());
    let marks = fixture.run("editor.lineMarkers.list", json!({ "path": service }));
    assert!(marks["markers"].as_array().unwrap().is_empty(), "nothing while the plug-in is off");
}

#[test]
fn the_endpoints_are_the_projects_request_mappings_and_open_their_handlers() {
    let mut fixture = Fixture::new(true);
    let java = web_project(&fixture);
    let list = fixture.run("springBoot.endpoints.list", json!({ "waitMs": 60_000 }));
    assert_eq!(list["loading"], false);
    let endpoints = list["endpoints"].as_array().unwrap();
    assert_eq!(endpoints.len(), 7);
    assert_eq!((endpoints[0]["method"].as_str(), endpoints[0]["path"].as_str()), (Some("GET"), Some("/api/orders")));
    assert_eq!(endpoints[0]["file"], json!(java.join("OrderController.java")));
    fixture.run("springBoot.endpoints.open", json!({ "handler": "OrderController.one" }));
    let workbench = fixture.session.state::<crate::Workbench>().unwrap();
    let document = workbench.active_document().unwrap();
    assert_eq!(document.path(), java.join("OrderController.java"));
    let caret = document.selections().primary().head;
    let line = document.text().chars().take(caret).filter(|c| *c == '\n').count();
    assert_eq!(line, 12);

    std::fs::write(
        java.join("Extra.kt"),
        "@RestController\nclass Extra {\n  @GetMapping(\"/extra\")\n  fun extra() = 1\n}\n",
    )
    .unwrap();
    let mut found = false;
    for _ in 0..200 {
        let list = fixture.run("springBoot.endpoints.list", json!({ "waitMs": 100 }));
        if list["endpoints"].as_array().unwrap().iter().any(|endpoint| endpoint["path"] == "/extra") {
            found = true;
            break;
        }
        super::index::force_rescan(&mut fixture.session);
    }
    assert!(found, "a new file is read again");
}

const RECORDED: &str = include_str!("generations.json");

struct Recorded {
    asked: std::sync::Mutex<Vec<Option<String>>>,
}

impl super::support::Source for Recorded {
    fn get(&self, etag: Option<&str>, _last_modified: Option<&str>) -> Result<super::support::Answer, String> {
        self.asked.lock().unwrap().push(etag.map(str::to_owned));
        Ok(match etag {
            Some(_) => super::support::Answer::NotModified,
            None => super::support::Answer::Fresh {
                body: RECORDED.as_bytes().to_vec(),
                etag: Some("\"v1\"".to_owned()),
                last_modified: None,
            },
        })
    }
}

#[test]
fn the_boot_version_of_a_build_file_is_found() {
    use super::support::version_in;
    let found = |name: &str, text: &str| version_in(Path::new(name), text).map(|range| text[range].to_owned());
    let parent = "<project>\n  <parent>\n    <groupId>org.springframework.boot</groupId>\n    <artifactId>spring-boot-starter-parent</artifactId>\n    <version> 3.3.5 </version>\n  </parent>\n  <version>1.0</version>\n</project>\n";
    assert_eq!(found("pom.xml", parent).as_deref(), Some("3.3.5"));
    let bom = "<project><properties><boot.version>2.7.18</boot.version></properties><dependencyManagement><dependencies><dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-dependencies</artifactId><version>${boot.version}</version></dependency></dependencies></dependencyManagement></project>";
    assert_eq!(found("pom.xml", bom).as_deref(), Some("2.7.18"));
    assert_eq!(found("pom.xml", "<project><version>1.0</version></project>"), None);
    assert_eq!(
        found("build.gradle", "plugins {\n  id 'org.springframework.boot' version '3.5.7'\n}\n").as_deref(),
        Some("3.5.7")
    );
    assert_eq!(
        found("build.gradle.kts", "plugins {\n    id(\"org.springframework.boot\") version \"4.1.2\"\n}\n").as_deref(),
        Some("4.1.2")
    );
    assert_eq!(
        found(
            "build.gradle.kts",
            "dependencies { implementation(platform(\"org.springframework.boot:spring-boot-dependencies:3.3.1\")) }\n"
        )
        .as_deref(),
        Some("3.3.1")
    );
    assert_eq!(found("libs.versions.toml", "[versions]\nspring-boot = \"3.3.0\"\n").as_deref(), Some("3.3.0"));
    assert_eq!(found("build.gradle", "plugins { id 'org.springframework.boot' version bootVersion }"), None);
}

#[test]
fn a_boot_version_past_its_open_source_support_is_marked_from_the_recorded_generations() {
    use zen_project::inspect::{InspectContext, Inspector};
    let dir = tempfile::tempdir().unwrap();
    let source = Arc::new(Recorded { asked: std::sync::Mutex::new(Vec::new()) });
    let support = super::support::BootSupport::new(source.clone()).with_today(Arc::new(|| "2026-11-04".to_owned()));
    support.set_cache_dir(dir.path().to_owned());
    support.fetch();
    assert_eq!(support.generations().len(), 4);
    let path = Path::new("/p/build.gradle.kts");
    let inspect = |support: &super::support::BootSupport, text: &str, on: bool| {
        let setting = move |key: &str| (key == super::support::CHECK).then(|| json!(on));
        let context =
            InspectContext { root: Path::new("/p"), path, text, tool: Path::new(""), env: &[], setting: &setting };
        support.inspect(&context)
    };
    let old = "plugins { id(\"org.springframework.boot\") version \"3.3.5\" }\n";
    let found = inspect(&support, old, true);
    assert_eq!(found.len(), 1);
    assert_eq!(&old[found[0].start..found[0].end], "3.3.5");
    assert_eq!(found[0].message, "Spring Boot 3.3.5: open-source support of 3.3.x ended on 2025-06-30");
    let extended = "plugins { id(\"org.springframework.boot\") version \"2.7.18\" }\n";
    assert!(inspect(&support, extended, true)[0].message.ends_with("commercial support lasts until 2029-06-30"));
    assert!(inspect(&support, "plugins { id(\"org.springframework.boot\") version \"4.1.2\" }\n", true).is_empty());
    assert!(inspect(&support, "plugins { id(\"org.springframework.boot\") version \"9.9.0\" }\n", true).is_empty());
    assert!(inspect(&support, old, false).is_empty(), "the setting turns it off");
    let unset = |_: &str| None;
    let context =
        InspectContext { root: Path::new("/p"), path, text: old, tool: Path::new(""), env: &[], setting: &unset };
    assert!(support.inspect(&context).is_empty(), "off until the person turns it on (no network by default)");

    support.refresh();
    support.fetch();
    assert_eq!(*source.asked.lock().unwrap(), [None, Some("\"v1\"".to_owned())], "asked again with its validator");
    assert_eq!(support.generations().len(), 4, "a 304 keeps the cached answer");

    let offline = Arc::new(Recorded { asked: std::sync::Mutex::new(Vec::new()) });
    let restarted = super::support::BootSupport::new(offline).with_today(Arc::new(|| "2026-11-04".to_owned()));
    restarted.set_cache_dir(dir.path().to_owned());
    assert_eq!(inspect(&restarted, old, true).len(), 1, "the cached answer is read from disk");
    let setting = support.settings();
    assert_eq!(setting[0].network, ["api.spring.io"]);
    assert_eq!(super::support::date_of(0), "1970-01-01");
    assert_eq!(super::support::date_of(1_793_790_176), "2026-11-04");
}

#[test]
fn without_answers_nothing_is_marked() {
    struct Offline;
    impl super::support::Source for Offline {
        fn get(&self, _: Option<&str>, _: Option<&str>) -> Result<super::support::Answer, String> {
            Err("offline".to_owned())
        }
    }
    let support = super::support::BootSupport::new(Arc::new(Offline));
    support.fetch();
    let text = "plugins { id 'org.springframework.boot' version '2.0.0' }";
    assert!(super::support::check(Path::new("build.gradle"), text, &support.generations(), "2026-11-04").is_empty());
}

#[test]
fn the_build_file_shows_the_support_end_while_the_plugin_is_on() {
    let mut fixture = Fixture::new(true);
    let pom = fixture.root.join("pom.xml");
    std::fs::write(&pom, "<project>\n  <parent>\n    <artifactId>spring-boot-starter-parent</artifactId>\n    <version>3.3.5</version>\n  </parent>\n</project>\n").unwrap();
    let support = Arc::new(
        super::support::BootSupport::new(Arc::new(Recorded { asked: std::sync::Mutex::new(Vec::new()) }))
            .with_today(Arc::new(|| "2026-11-04".to_owned())),
    );
    support.fetch();
    fixture.session.state_mut::<crate::inspections::Inspections>().set_inspectors(vec![support], None);
    fixture.open_at(&pom, 0);
    let problems = fixture.run("problems.file.list", json!({ "path": pom }));
    assert!(!problems.to_string().contains("open-source support"), "off by default: {problems}");
    fixture.session.settings_mut().set(super::support::CHECK, true).unwrap();
    let problems = fixture.run("problems.file.list", json!({ "path": pom }));
    assert!(problems.to_string().contains("open-source support of 3.3.x ended on 2025-06-30"), "{problems}");
    fixture.session.state_mut::<crate::plugin_projects::PluginProjects>().set(Vec::new());
    fixture.session.state_mut::<crate::inspections::Inspections>().set_inspectors(
        vec![Arc::new(super::support::BootSupport::new(Arc::new(Recorded {
            asked: std::sync::Mutex::new(Vec::new()),
        })))],
        None,
    );
    let problems = fixture.run("problems.file.list", json!({ "path": pom }));
    assert!(!problems.to_string().contains("open-source support"), "{problems}");
}

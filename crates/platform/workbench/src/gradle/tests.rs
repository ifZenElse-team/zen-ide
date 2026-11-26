use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_run::{RunEnv, RunProvider};

use super::*;
use crate::build_tools::BuildEnv;
use crate::run::Runs;

fn fake_gradle(path: &Path, name: &str) {
    let script = format!(
        r#"#!/bin/sh
for arg in "$@"; do
  case "$arg" in
    -Dzen.model.out=*) out="${{arg#-Dzen.model.out=}}" ;;
  esac
done
echo "$*" >> "$PWD/.gradle-calls"
if [ -n "$out" ]; then
  [ -f "$PWD/.fail-model" ] && {{ echo "FAILURE: the build failed"; exit 1; }}
  mkdir -p "$out"
  cat > "$out/root.json" <<JSON
{{"build":":","path":":","name":"shop","folder":"$PWD","buildFile":null,"tasks":[{{"name":"build","group":"build","description":"Assembles and tests this project."}}],"sourceSets":[],"configurations":[]}}
JSON
  cat > "$out/app.json" <<JSON
{{"build":":","path":":app","name":"app","folder":"$PWD/app","buildFile":"$PWD/app/build.gradle","tasks":[{{"name":"build","group":"build"}},{{"name":"run","group":"application","description":"Runs this project as a JVM application"}}],"sourceSets":[{{"name":"main","sources":["$PWD/app/src/main/java"],"resources":[],"output":[]}}],"configurations":[{{"name":"compileClasspath","dependencies":[{{"groupId":"org.slf4j","artifactId":"slf4j-api","version":"2.0.13"}}]}}]}}
JSON
  cat > "$out/tools.json" <<JSON
{{"build":":tools","path":":tools","name":"tools","folder":"$PWD/tools","tasks":[],"sourceSets":[],"configurations":[]}}
JSON
  exit 0
fi
echo "{name} $*"
echo "in $(pwd -P)"
echo "java $JAVA_HOME"
for arg in "$@"; do
  [ "$arg" = fail ] && {{ echo "FAILURE: the build failed"; exit 1; }}
done
exit 0
"#
    );
    std::fs::write(path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: PathBuf,
    env: BuildEnv,
    bin: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(project.join("app")).unwrap();
        std::fs::write(project.join("settings.gradle"), "rootProject.name = 'shop'\ninclude 'app'\n").unwrap();
        std::fs::write(project.join("app/build.gradle"), "plugins { id 'application' }\n").unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        fake_gradle(&bin.join("gradle"), "gradle");
        let jdk = dir.path().join("jdk");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        let env = BuildEnv::default();
        env.set_folders(Some(vec![bin.clone()]));
        env.set_jdk(Some(Arc::new(move |_root: &Path| Some(jdk.clone()))));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Runs>().set_providers(vec![Arc::new(GradleRun::new(env.clone()))], RunEnv::default());
        session.set_state(env.clone());
        crate::trust::trust_in_tests(&env, &project);
        Self { registry, session, project, env, bin, _dir: dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn output(&mut self, run_id: &Value) -> String {
        let read = self.run("run.output.get", json!({ "runId": run_id }));
        read["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn the_wrapper_comes_first_and_runs_in_the_build_folder() {
    let fixture = Fixture::new();
    let provider = GradleRun::new(fixture.env.clone());
    let mut configuration = GradleRun::tasks("shop", ".", ":app:run --args='a b'");
    configuration.common.program_arguments = "--offline".to_owned();
    let line = provider.command_line(&configuration, &fixture.project).unwrap();
    assert_eq!(line.program, fixture.bin.join("gradle"));
    assert_eq!(line.args, [":app:run", "--args=a b", "--offline"]);
    assert_eq!(line.cwd, fixture.project);
    assert_eq!(configuration.name, "shop [:app:run --args='a b']");
    fake_gradle(&fixture.project.join("gradlew"), "gradlew");
    let nested = GradleRun::tasks("app", "app", "build");
    let line = provider.command_line(&nested, &fixture.project).unwrap();
    assert_eq!(line.program, fixture.project.join("gradlew"), "the wrapper above the folder");
    assert_eq!(line.cwd, fixture.project.join("app"));
    assert_eq!(line.display, "./gradlew build");
    assert!(provider.command_line(&GradleRun::tasks("x", ".", ""), &fixture.project).is_err());
}

#[test]
fn a_shared_gradle_configuration_is_adopted() {
    let options: std::collections::BTreeMap<String, String> = [
        ("ExternalSystemSettings.externalProjectPath", "/p"),
        ("ExternalSystemSettings.taskNames.list.option.0", ":app:clean"),
        ("ExternalSystemSettings.taskNames.list.option.1", ":app:build"),
        ("ExternalSystemSettings.scriptParameters", "--info"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    let foreign = ForeignRunConfiguration {
        source_file: PathBuf::from("/p/.run/build.run.xml"),
        name: "build app".to_owned(),
        type_id: "GradleRunConfiguration".to_owned(),
        factory: Some("Gradle".to_owned()),
        options,
        env: Vec::new(),
        shared: true,
        before_launch: Vec::new(),
    };
    let adopted = GradleRun::default().adopt(&foreign, Path::new("/p")).unwrap();
    assert_eq!(adopted.options, json!({ "projectPath": ".", "tasks": ":app:clean :app:build" }));
    assert_eq!(adopted.common.program_arguments, "--info");
}

#[cfg(unix)]
#[test]
fn a_build_without_a_wrapper_finds_gradle_in_the_user_homes_wrapper_distributions() {
    let mut fixture = Fixture::new();
    fixture.env.set_folders(Some(Vec::new()));
    fixture.env.set_home(Some(fixture._dir.path().join("home")));
    let missing = fixture.run("gradle.project.get", json!({}));
    assert!(missing.get("executable").is_none(), "{missing}");
    let unpacked = fixture._dir.path().join("home/.gradle/wrapper/dists/gradle-8.10.2-bin/abc");
    let home = unpacked.join("gradle-8.10.2");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::create_dir_all(home.join("lib")).unwrap();
    fake_gradle(&home.join("bin/gradle"), "dists gradle");
    std::fs::write(home.join("lib/gradle-core-8.10.2.jar"), "").unwrap();
    std::fs::write(unpacked.join("gradle-8.10.2-bin.zip.ok"), "").unwrap();
    let build = fixture.run("gradle.project.get", json!({}));
    assert_eq!(build["executable"]["path"], json!(home.join("bin/gradle")));
    assert_eq!(build["executable"]["source"], "wrapperDistribution");
    assert_eq!(build["executable"]["version"], "8.10.2");
    let elsewhere = fixture._dir.path().join("gradle-home");
    fixture.run("gradle.settings.set", json!({ "key": "userHome", "value": elsewhere }));
    let build = fixture.run("gradle.project.get", json!({}));
    assert!(build.get("executable").is_none(), "only the chosen user home is searched: {build}");
}

#[test]
fn the_build_is_read_tasks_load_on_reload_and_run() {
    let mut fixture = Fixture::new();
    let build = fixture.run("gradle.project.get", json!({}));
    assert_eq!(build["projects"][0]["name"], "shop");
    assert_eq!(build["projects"][1]["path"], ":app");
    assert_eq!(build["executable"]["wrapper"], false);
    assert!(build["taskList"].get("state").is_none(), "nothing ran yet");
    let reloaded = fixture.run("gradle.project.reload", json!({ "waitMs": 20000 }));
    assert_eq!(reloaded["reimported"], false);
    assert_eq!(reloaded["taskList"]["state"], "loaded");
    let tasks = fixture.run("gradle.tasks.list", json!({ "project": ":app" }));
    let names: Vec<&str> = tasks["tasks"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["build", "run"]);
    assert_eq!(fixture.run("gradle.project.get", json!({}))["tasks"], 3);
    let started = fixture.run("gradle.task.run", json!({ "tasks": ":app:run", "waitMs": 20000 }));
    assert_eq!(started["summary"]["exitCode"], 0);
    let output = fixture.output(&started["runId"]);
    assert!(output.contains("gradle :app:run"), "{output}");
    assert!(output.contains("java ") && output.contains("jdk"), "{output}");
    std::fs::remove_file(fixture.project.join("settings.gradle")).unwrap();
    assert!(fixture.registry.dispatch(&mut fixture.session, "gradle.project.get", json!({})).is_err());
}

#[test]
#[ignore = "runs a real Gradle: ZEN_TEST_GRADLE, ZEN_TEST_JDK"]
fn a_real_gradle_build_lists_its_tasks_and_runs_one() {
    let (Some(gradle), Some(jdk)) = (std::env::var_os("ZEN_TEST_GRADLE"), std::env::var_os("ZEN_TEST_JDK")) else {
        panic!("set ZEN_TEST_GRADLE and ZEN_TEST_JDK");
    };
    let mut fixture = Fixture::new();
    let jdk = PathBuf::from(jdk);
    fixture.env.set_folders(Some(vec![PathBuf::from(gradle)]));
    fixture.env.set_jdk(Some(Arc::new(move |_root: &Path| Some(jdk.clone()))));
    std::fs::write(
        fixture.project.join("app/build.gradle"),
        "tasks.register('hello') {\n    group = 'custom'\n    description = 'Says hello.'\n    doLast { println 'Hello from Gradle' }\n}\n",
    )
    .unwrap();
    let reloaded = fixture.run("gradle.project.reload", json!({ "waitMs": 600_000 }));
    assert_eq!(reloaded["taskList"]["state"], "loaded", "{reloaded}");
    let tasks = fixture.run("gradle.tasks.list", json!({ "project": ":app" }));
    let hello = tasks["tasks"].as_array().unwrap().iter().find(|task| task["name"] == "hello").cloned();
    assert_eq!(hello.as_ref().map(|task| task["group"].clone()), Some(json!("custom")), "{tasks}");
    let started = fixture.run("gradle.task.run", json!({ "tasks": ":app:hello", "waitMs": 600_000 }));
    let output = fixture.output(&started["runId"]);
    assert_eq!(started["summary"]["exitCode"], 0, "{output}");
    assert!(output.contains("Hello from Gradle"), "{output}");

    std::fs::write(
        fixture.project.join("app/build.gradle"),
        "plugins { id 'java' }\nrepositories { mavenCentral() }\ndependencies { implementation 'commons-io:commons-io:2.16.1' }\n",
    )
    .unwrap();
    fixture.run("gradle.settings.set", json!({ "values": { "downloadSources": true, "vmOptions": "-Xmx512m" } }));
    let reloaded = fixture.run("gradle.project.reload", json!({ "tasks": false }));
    wait_load(&fixture.env, &sources_key(&fixture.project), Duration::from_secs(600));
    let log = fixture.run("tasks.log.read", json!({ "id": reloaded["sources"] })).to_string();
    assert!(log.contains(":app: the sources of 1 of 1 libraries"), "{log}");
}

#[test]
fn the_gradle_settings_reach_tasks_reload_and_the_project() {
    let mut fixture = Fixture::new();
    let gradle_jvm = fixture._dir.path().join("gradle-jvm");
    std::fs::create_dir_all(gradle_jvm.join("bin")).unwrap();
    let settings = fixture.run(
        "gradle.settings.set",
        json!({ "values": {
            "offline": true,
            "vmOptions": "-Xmx2g -Dfile.encoding=UTF-8",
            "userHome": "/caches/gradle",
            "jvm": gradle_jvm.display().to_string(),
        } }),
    );
    assert_eq!(settings["offline"], true);
    assert_eq!(fixture.run("gradle.settings.get", json!({}))["userHome"], "/caches/gradle");
    let line = GradleRun::new(fixture.env.clone())
        .command_line(&GradleRun::tasks("shop", ".", "build"), &fixture.project)
        .unwrap();
    assert_eq!(line.args, ["--offline", "-Dorg.gradle.jvmargs=-Xmx2g -Dfile.encoding=UTF-8", "build"]);
    let var = |name: &str| line.env.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.clone());
    assert_eq!(var("GRADLE_USER_HOME").as_deref(), Some("/caches/gradle"));
    assert_eq!(var("JAVA_HOME"), Some(gradle_jvm.display().to_string()));
    assert_eq!(fixture.run("gradle.project.get", json!({}))["javaHome"], json!(gradle_jvm));

    let started = fixture.run("gradle.task.run", json!({ "tasks": "build", "waitMs": 20000 }));
    let output = fixture.output(&started["runId"]);
    assert!(output.contains("gradle --offline -Dorg.gradle.jvmargs=-Xmx2g"), "{output}");
    assert!(output.contains("java ") && output.contains("gradle-jvm"), "{output}");

    fixture.run("gradle.settings.set", json!({ "key": "downloadSources", "value": true }));
    let reloaded = fixture.run("gradle.project.reload", json!({ "waitMs": 20000 }));
    assert!(reloaded["sources"].is_u64(), "{reloaded}");
    wait_load(&fixture.env, &sources_key(&fixture.project), Duration::from_secs(20));
    let log = fixture.run("tasks.log.read", json!({ "id": reloaded["sources"] })).to_string();
    assert!(log.contains("zenDownloadSources") && log.contains("--no-configuration-cache"), "{log}");

    let back = fixture.run("gradle.settings.set", json!({ "values": { "offline": null, "vmOptions": null } }));
    assert_eq!((back["offline"].clone(), back["vmOptions"].clone()), (json!(false), json!("")));
    let error = fixture.registry.dispatch(&mut fixture.session, "gradle.settings.set", json!({ "key": "nope" }));
    assert!(error.unwrap_err().to_string().contains("nope"));
    let error = fixture.registry.dispatch(
        &mut fixture.session,
        "gradle.settings.set",
        json!({ "key": "runTests", "value": "maven" }),
    );
    assert!(error.unwrap_err().to_string().contains("maven"));
}

#[test]
fn the_distribution_chooses_the_wrapper_gradle_here_or_a_folder() {
    let mut fixture = Fixture::new();
    fake_gradle(&fixture.project.join("gradlew"), "gradlew");
    let program = |fixture: &mut Fixture| fixture.run("gradle.project.get", json!({}))["executable"]["path"].clone();
    assert_eq!(program(&mut fixture), json!(fixture.project.join("gradlew")));
    fixture.run("gradle.settings.set", json!({ "key": "distribution", "value": "installed" }));
    assert_eq!(program(&mut fixture), json!(fixture.bin.join("gradle")));
    let home = fixture._dir.path().join("gradle-9");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    fake_gradle(&home.join("bin/gradle"), "custom");
    fixture.run(
        "gradle.settings.set",
        json!({ "values": { "distribution": "custom", "homePath": home.display().to_string() } }),
    );
    assert_eq!(program(&mut fixture), json!(home.join("bin/gradle")));
    fixture.run("gradle.settings.set", json!({ "key": "homePath", "value": "/nowhere" }));
    assert_eq!(program(&mut fixture), Value::Null);
    let form = fixture.run("gradle.settings.form", json!({}));
    let items = form["items"].as_array().unwrap();
    let button = items.iter().find(|item| item["kind"] == "button").unwrap();
    assert_eq!(
        (button["label"].clone(), button["action"]["command"].clone()),
        (json!("Download Gradle"), json!("gradle.distribution.download"))
    );
    let labels: Vec<&str> = items.iter().filter_map(|item| item["label"].as_str()).collect();
    for label in [
        "Gradle user home:",
        "Offline work",
        "Build and run using:",
        "Run tests using:",
        "Distribution:",
        "Gradle home:",
        "Gradle JVM:",
        "VM options:",
        "Download sources",
    ] {
        assert!(labels.contains(&label), "{label} in {labels:?}");
    }
}

#[test]
fn a_gradle_download_is_official_and_checked() {
    use crate::ide_settings::zip::ZipWriter;
    let current =
        br#"{ "version": "9.8.2", "downloadUrl": "https://services.gradle.org/distributions/gradle-9.8.2-bin.zip",
        "checksumUrl": "https://services.gradle.org/distributions/gradle-9.8.2-bin.zip.sha256", "current": true }"#;
    let found = download::parse_current(current).unwrap();
    assert_eq!(found, download::release("9.8.2"));
    let elsewhere = String::from_utf8_lossy(current).replace("services.gradle.org/distributions", "example.com");
    assert!(download::parse_current(elsewhere.as_bytes()).is_err(), "only services.gradle.org");
    assert!(!download::valid_version("../9") && !download::valid_version("") && download::valid_version("9.2-rc-1"));

    let mut zip = ZipWriter::new(0);
    zip.add("gradle-9.8.2/bin/gradle", b"#!/bin/sh\necho gradle\n").unwrap();
    zip.add("gradle-9.8.2/lib/gradle-core.jar", b"jar").unwrap();
    let archive = zip.finish();
    let dir = tempfile::tempdir().unwrap();
    let target = download::folder(dir.path(), "9.8.2");
    let error = download::install(archive.clone(), "00", "9.8.2", &target).unwrap_err();
    assert!(error.contains("SHA-256"), "{error}");
    assert!(!target.exists(), "nothing is installed");
    let sum = format!("{}  gradle-9.8.2-bin.zip\n", zen_update::sha256_hex(&archive));
    download::install(archive, &sum, "9.8.2", &target).unwrap();
    assert_eq!(std::fs::read(target.join("lib/gradle-core.jar")).unwrap(), b"jar");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(target.join("bin/gradle")).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }
    let env = BuildEnv::default();
    env.set_data_dir(Some(dir.path().to_owned()));
    assert!(env.folders("gradle").contains(&target.join("bin")), "it is Gradle on this computer");

    let mut fixture = Fixture::new();
    fixture.env.set_data_dir(Some(dir.path().to_owned()));
    let there = fixture.run("gradle.distribution.download", json!({ "version": "9.8.2" }));
    assert_eq!(there, json!({ "version": "9.8.2", "folder": target }), "no download when it is there");
    let error =
        fixture.registry.dispatch(&mut fixture.session, "gradle.distribution.download", json!({ "version": "../x" }));
    assert!(error.is_err());
}

#[test]
#[ignore = "unpacks a real Gradle distribution: ZEN_TEST_GRADLE_ZIP, ZEN_TEST_GRADLE_SHA256, ZEN_TEST_GRADLE_VERSION"]
fn a_real_gradle_distribution_installs() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let archive = std::fs::read(var("ZEN_TEST_GRADLE_ZIP")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let version = var("ZEN_TEST_GRADLE_VERSION");
    let target = download::folder(dir.path(), &version);
    download::install(archive, &var("ZEN_TEST_GRADLE_SHA256"), &version, &target).unwrap();
    let output = std::process::Command::new(target.join("bin/gradle")).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("Gradle {version}")), "{output:?}");
}

#[cfg(unix)]
#[test]
fn a_gradle_task_runs_before_launch_and_a_failing_one_cancels_the_start() {
    let mut fixture = Fixture::new();
    let types = fixture.run("run.beforeLaunch.types", json!({}));
    let gradle = types["types"].as_array().unwrap().iter().find(|kind| kind["id"] == TASK_TASK).unwrap().clone();
    assert_eq!((gradle["provider"].clone(), gradle["name"].clone()), (json!("gradle"), json!("Run Gradle Task")));
    let mut configuration = serde_json::to_value(GradleRun::tasks("run", ".", ":app:run")).unwrap();
    configuration["common"]["beforeLaunch"] = json!([{ "type": TASK_TASK, "projectPath": ".", "tasks": "clean" }]);
    let created = fixture.run("run.configuration.create", json!({ "configuration": configuration }));
    let started = fixture.run("run.process.start", json!({ "configurationId": created["id"], "waitMs": 20000 }));
    assert_eq!(started["summary"]["state"], "finished");
    let output = fixture.output(&started["runId"]);
    let task = output.find("gradle clean").unwrap_or_else(|| panic!("{output}"));
    let program = output.find("gradle :app:run").unwrap_or_else(|| panic!("{output}"));
    assert!(output.contains("#before-launch Run Gradle Task “clean”") && task < program, "{output}");

    configuration["common"]["beforeLaunch"] = json!([{ "type": TASK_TASK, "tasks": "fail" }]);
    configuration["name"] = json!("failing");
    let created = fixture.run("run.configuration.create", json!({ "configuration": configuration }));
    let started = fixture.run("run.process.start", json!({ "configurationId": created["id"], "waitMs": 20000 }));
    assert_eq!(started["summary"]["state"], "beforeLaunchFailed");
    assert!(!fixture.output(&started["runId"]).contains("gradle :app:run"));
}

#[test]
fn an_adopted_gradle_task_keeps_its_tasks_and_build_folder() {
    let foreign = ForeignTask {
        id: "Gradle.BeforeRunTask".to_owned(),
        enabled: true,
        options: [("tasks".to_owned(), "build".to_owned()), ("externalProjectPath".to_owned(), "/p/app".to_owned())]
            .into_iter()
            .collect(),
    };
    let task = GradleRun::default().adopt_task(&foreign, Path::new("/p")).unwrap();
    assert_eq!(task.kind, TASK_TASK);
    assert_eq!((task.text("tasks"), task.text("projectPath")), ("build", "app"));
}

#[test]
fn run_anything_runs_gradle_tasks_in_the_build_of_the_folder() {
    let fixture = Fixture::new();
    let provider = GradleRun::new(fixture.env.clone());
    let context = RunContext { root: fixture.project.clone(), ..RunContext::default() };
    let words = |line: &str| zen_run::args::split(line);
    assert_eq!(provider.anything_prefixes()[0].prefix, "gradle");
    assert!(provider.anything(&words("mvn test"), &fixture.project, &context).is_none());
    assert!(provider.anything(&words("gradle"), &fixture.project, &context).unwrap().is_err());
    let nested = fixture.project.join("app/src");
    std::fs::create_dir_all(&nested).unwrap();
    let configuration = provider.anything(&words("gradle build --info"), &nested, &context).unwrap().unwrap();
    assert_eq!(configuration.name, "gradle build --info");
    let line = provider.command_line(&configuration, &fixture.project).unwrap();
    assert_eq!(line.cwd, fixture.project.join("app"));
    assert_eq!(line.args, ["build", "--info"]);
    let root = provider.anything(&words("gradle test"), &fixture.project, &context).unwrap().unwrap();
    assert_eq!(provider.command_line(&root, &fixture.project).unwrap().cwd, fixture.project);
}

fn calls(fixture: &Fixture) -> String {
    std::fs::read_to_string(fixture.project.join(".gradle-calls")).unwrap_or_default()
}

fn loaded(fixture: &mut Fixture) -> Value {
    fixture.run("gradle.model.get", json!({ "load": true, "waitMs": 20000 }))
}

#[cfg(unix)]
#[test]
fn the_model_comes_right_after_import_without_a_reload() {
    let mut fixture = Fixture::new();
    let first = fixture.run("gradle.model.get", json!({}));
    let paths: Vec<&str> =
        first["builds"][0]["projects"].as_array().unwrap().iter().map(|p| p["path"].as_str().unwrap()).collect();
    assert_eq!(paths, [":", ":app"], "the settings script until Gradle answers");
    assert!(first["builds"][0].get("state").is_none());
    let answer = loaded(&mut fixture);
    let build = &answer["builds"][0];
    assert_eq!((build["state"].clone(), build["projectPath"].clone()), (json!("loaded"), json!(".")));
    let projects = build["projects"].as_array().unwrap();
    let paths: Vec<&str> = projects.iter().map(|p| p["path"].as_str().unwrap()).collect();
    assert_eq!(paths, [":", ":app", ":tools"], "the root build first, then the included one");
    assert_eq!(projects[2]["build"], ":tools");
    let app = &projects[1];
    assert_eq!(app["tasks"][1]["group"], "application");
    assert_eq!(app["sourceSets"][0]["name"], "main");
    assert_eq!(app["configurations"][0]["name"], "compileClasspath");
    assert_eq!(app["configurations"][0]["dependencies"][0]["artifactId"], "slf4j-api");
    let tasks = fixture.run("gradle.tasks.list", json!({ "project": ":app" }));
    assert_eq!(tasks["tasks"][1]["group"], "application");
    let call = calls(&fixture);
    assert!(call.contains(":zenModel") && call.contains("--no-configuration-cache"), "{call}");
    assert_eq!(fixture.run("gradle.project.get", json!({}))["projects"][2]["path"], ":tools");
}

#[cfg(unix)]
#[test]
fn the_model_is_cached_per_build_file_hash() {
    let mut fixture = Fixture::new();
    let data = fixture.project.parent().unwrap().join("data");
    fixture.env.set_data_dir(Some(data.clone()));
    crate::trust::trust_in_tests(&fixture.env, &fixture.project);
    loaded(&mut fixture);
    assert_eq!(calls(&fixture).lines().count(), 1);
    loaded(&mut fixture);
    assert_eq!(calls(&fixture).lines().count(), 1, "unchanged build files: no Gradle");

    let hash = fixture.run("gradle.project.get", json!({}))["modelHash"].clone();
    let restarted = BuildEnv::default();
    restarted.set_data_dir(Some(data));
    restarted.set_folders(Some(vec![fixture.bin.clone()]));
    crate::trust::trust_in_tests(&restarted, &fixture.project);
    fixture.session.set_state(restarted.clone());
    fixture.env = restarted;
    let answer = loaded(&mut fixture);
    assert_eq!(answer["builds"][0]["state"], "loaded");
    assert_eq!(answer["builds"][0]["projects"].as_array().unwrap().len(), 3, "the model survives a restart");
    assert_eq!(calls(&fixture).lines().count(), 1, "a restart reads the cache");
    assert_eq!(fixture.run("gradle.project.get", json!({}))["modelHash"], hash);

    std::fs::write(fixture.project.join("app/build.gradle"), "plugins { id 'java' }\n").unwrap();
    loaded(&mut fixture);
    assert_eq!(calls(&fixture).lines().count(), 2, "a changed build script asks Gradle again");
    assert_ne!(fixture.run("gradle.project.get", json!({}))["modelHash"], hash);
}

#[cfg(unix)]
#[test]
fn a_failed_load_shows_in_sync_and_keeps_the_last_good_model() {
    let mut fixture = Fixture::new();
    loaded(&mut fixture);
    std::fs::write(fixture.project.join(".fail-model"), "").unwrap();
    fixture.run("gradle.project.reload", json!({ "waitMs": 20000 }));
    let answer = fixture.run("gradle.model.get", json!({}));
    let build = &answer["builds"][0];
    assert_eq!(build["state"], "failed");
    assert!(build["error"].is_string(), "{build}");
    assert_eq!(build["projects"].as_array().unwrap().len(), 3, "the last good model stays");
    assert_eq!(fixture.run("gradle.tasks.list", json!({}))["tasks"].as_array().unwrap().len(), 3);
    let sync = fixture.run("build.sync.get", json!({}));
    assert_eq!(sync["syncs"][0]["system"], "gradle");
    assert!(sync.to_string().contains("failed"), "{sync}");
}

#[cfg(unix)]
#[test]
fn an_untrusted_project_is_not_loaded() {
    let mut fixture = Fixture::new();
    let other = BuildEnv::default();
    other.set_folders(Some(vec![fixture.bin.clone()]));
    fixture.session.set_state(other);
    let answer = loaded(&mut fixture);
    assert_eq!(answer["trusted"], false);
    assert_eq!(calls(&fixture), "", "Gradle's scripts are programs");
}

#[cfg(unix)]
#[test]
fn tasks_around_a_sync_run_in_the_gradle_run_that_reloads() {
    let mut fixture = Fixture::new();
    let task = |tasks: &str| json!({ "type": TASK_TASK, "projectPath": ".", "tasks": tasks });
    fixture.run("build.triggers.set", json!({ "when": "beforeSync", "task": task("generate") }));
    fixture.run("build.triggers.set", json!({ "when": "afterSync", "task": task(":app:after") }));
    fixture.run("gradle.project.reload", json!({ "waitMs": 20000 }));
    let call = calls(&fixture);
    let at = |word: &str| call.find(word).unwrap_or_else(|| panic!("{word} in {call}"));
    assert!(at("generate") < at(":zenModel") && at(":zenModel") < at(":app:after"), "{call}");
    let maven = json!({ "type": "maven.goal", "pom": "pom.xml", "goals": "clean" });
    let refused = fixture.registry.dispatch(
        &mut fixture.session,
        "build.triggers.set",
        json!({ "when": "beforeSync", "task": maven }),
    );
    assert!(refused.is_err(), "only Gradle tasks can join Gradle's run");
}

#[cfg(unix)]
#[test]
fn toggle_offline_is_the_offline_setting() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.run("gradle.project.get", json!({}))["offline"], false);
    fixture.run("gradle.options.set", json!({ "offline": true }));
    assert_eq!(fixture.run("gradle.settings.get", json!({}))["offline"], true);
    assert_eq!(fixture.run("gradle.project.get", json!({}))["offline"], true);
    loaded(&mut fixture);
    assert!(calls(&fixture).contains("--offline"), "{}", calls(&fixture));
    fixture.run("gradle.settings.set", json!({ "key": "offline", "value": false }));
    assert_eq!(fixture.run("gradle.options.set", json!({})), json!({ "offline": false }));
}

#[cfg(unix)]
#[test]
fn execute_task_completes_tasks_and_options_and_lists_the_lines_run_before() {
    let mut fixture = Fixture::new();
    loaded(&mut fixture);
    let texts = |answer: Value| -> Vec<String> {
        answer["suggestions"].as_array().unwrap().iter().map(|s| s["text"].as_str().unwrap().to_owned()).collect()
    };
    let found = texts(fixture.run("gradle.task.complete", json!({ "text": "clean :app:r" })));
    assert_eq!(found, ["clean :app:run"]);
    let found = texts(fixture.run("gradle.task.complete", json!({ "text": "bu" })));
    assert_eq!(found, ["build"]);
    let found = texts(fixture.run("gradle.task.complete", json!({ "text": "build --off" })));
    assert_eq!(found, ["build --offline"]);
    fixture.run("gradle.task.run", json!({ "tasks": "build   --info", "waitMs": 20000 }));
    let found = fixture.run("gradle.task.complete", json!({ "text": "bu" }));
    assert_eq!(found["suggestions"][0], json!({ "text": "build --info", "kind": "history" }));
}

#[cfg(unix)]
#[test]
fn a_gradle_build_is_linked_loaded_and_unlinked() {
    let mut fixture = Fixture::new();
    let other = fixture.project.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("build.gradle"), "").unwrap();
    let nowhere = fixture.registry.dispatch(
        &mut fixture.session,
        "gradle.project.add",
        json!({ "path": fixture.project.join("app/src") }),
    );
    assert!(nowhere.is_err(), "no build there");
    crate::trust::trust_in_tests(&fixture.env, &other);
    let linked = fixture.run("gradle.project.add", json!({ "path": other.join("build.gradle") }));
    assert_eq!(linked["linked"], json!([other]));
    let answer = fixture.run("gradle.model.get", json!({ "load": true }));
    assert_eq!(answer["builds"][1]["projectPath"], other.display().to_string());
    assert_eq!(answer["builds"].as_array().unwrap().len(), 2);
    let started = fixture.run("gradle.task.run", json!({ "tasks": "build", "projectPath": other, "waitMs": 20000 }));
    let output = fixture.output(&started["runId"]);
    assert!(output.contains(&format!("in {}", other.canonicalize().unwrap().display())), "{output}");
    assert_eq!(fixture.run("gradle.project.get", json!({}))["linked"], json!([other]));
    assert_eq!(fixture.run("gradle.project.unlink", json!({ "path": other }))["linked"], json!([]));
}

#[test]
fn create_run_configuration_saves_the_tasks_under_the_build() {
    let mut fixture = Fixture::new();
    let created = fixture.run("gradle.runConfiguration.create", json!({ "tasks": ":app:run" }));
    assert_eq!(created["name"], "shop [:app:run]");
    let list = fixture.run("run.configuration.list", json!({}));
    let saved = list["configurations"].as_array().unwrap().iter().find(|c| c["id"] == created["id"]).unwrap();
    assert_eq!((saved["kind"].clone(), saved["options"]["tasks"].clone()), (json!("gradle"), json!(":app:run")));
}

#[test]
fn debug_starts_the_tasks_with_the_debug_script() {
    let fixture = Fixture::new();
    let provider = GradleRun::new(fixture.env.clone());
    let context = RunContext { root: fixture.project.clone(), ..RunContext::default() };
    let plan = provider.debug(&GradleRun::tasks("app", ".", ":app:run"), &context).unwrap().unwrap();
    let debuggee = plan.debuggee.unwrap();
    let args = debuggee.line.args;
    assert_eq!(args[0], ":app:run");
    assert!(args[2].contains("zen-debug") && args[1] == "-I", "{args:?}");
    assert!(args[3].starts_with("-Dzen.debug.port="), "{args:?}");
    assert_eq!(debuggee.ready, crate::java::junit::DEBUG_READY);
}

#[test]
#[ignore = "runs a real Gradle over a composite build: ZEN_TEST_GRADLE, ZEN_TEST_JDK"]
fn a_real_gradle_writes_the_model_of_a_composite_build() {
    let (Some(gradle), Some(jdk)) = (std::env::var_os("ZEN_TEST_GRADLE"), std::env::var_os("ZEN_TEST_JDK")) else {
        panic!("set ZEN_TEST_GRADLE and ZEN_TEST_JDK");
    };
    let mut fixture = Fixture::new();
    let jdk = PathBuf::from(jdk);
    fixture.env.set_folders(Some(vec![PathBuf::from(gradle)]));
    fixture.env.set_jdk(Some(Arc::new(move |_root: &Path| Some(jdk.clone()))));
    let root = fixture.project.clone();
    let write = |path: &str, text: &str| {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    };
    write("settings.gradle", "rootProject.name = 'shop'\ninclude 'app', 'lib'\nincludeBuild 'tools'\n");
    write(
        "build.gradle",
        "tasks.register('hello') { group = 'custom'; description = 'Says hello'; doLast { println 'hi' } }\n",
    );
    write(
        "app/build.gradle",
        "plugins { id 'java' }\nrepositories { mavenCentral() }\ndependencies { implementation project(':lib')\n  testImplementation 'no.such:thing:1.0' }\n",
    );
    write("lib/build.gradle", "plugins { id 'java-library' }\n");
    write("tools/settings.gradle", "rootProject.name = 'tools'\ninclude 'core'\n");
    write("tools/core/build.gradle", "plugins { id 'java' }\n");
    write("buildSrc/build.gradle", "plugins { id 'java' }\n");
    fixture.run("gradle.options.set", json!({ "offline": true }));
    let answer = fixture.run("gradle.model.get", json!({ "load": true, "waitMs": 240000 }));
    let build = &answer["builds"][0];
    assert_eq!(build["state"], "loaded", "{build}");
    let projects = build["projects"].as_array().unwrap();
    let find = |path: &str| {
        projects.iter().find(|project| project["path"] == path).unwrap_or_else(|| panic!("{path} in {build}"))
    };
    assert_eq!(find(":tools")["build"], ":tools");
    assert_eq!(find(":tools:core")["build"], ":tools");
    assert_eq!(find(":buildSrc")["build"], ":buildSrc");
    let app = find(":app");
    let task = |name: &str| app["tasks"].as_array().unwrap().iter().find(|task| task["name"] == name).cloned();
    assert_eq!(task("build").unwrap()["group"], "build");
    assert!(task("zenModel").is_none(), "the IDE's own task is not shown");
    let hello = find(":")["tasks"].as_array().unwrap().iter().find(|task| task["name"] == "hello").unwrap();
    assert_eq!((hello["group"].clone(), hello["description"].clone()), (json!("custom"), json!("Says hello")));
    let configuration = |name: &str| {
        app["configurations"].as_array().unwrap().iter().find(|found| found["name"] == name).cloned().unwrap()
    };
    assert_eq!(configuration("compileClasspath")["dependencies"][0]["project"], ":lib");
    let tests = configuration("testCompileClasspath");
    let unresolved = tests["dependencies"].as_array().unwrap().iter().find(|node| node["unresolved"].is_string());
    assert_eq!(unresolved.unwrap()["requested"], "no.such:thing:1.0");
    let ran = fixture.run("gradle.task.run", json!({ "tasks": ":tools:core:classes --offline", "waitMs": 120000 }));
    assert_eq!(ran["summary"]["exitCode"], 0, "a task of an included build runs from the composite: {ran}");
}

#[test]
fn a_person_loads_no_build_of_a_project_not_trusted() {
    let mut fixture = Fixture::new();
    fixture.run("project.trust.revoke", json!({}));
    for (command, params) in [
        ("gradle.project.reload", json!({})),
        ("gradle.tasks.list", json!({ "load": true })),
        ("gradle.task.run", json!({ "tasks": "build" })),
        ("gradle.task.debug", json!({ "tasks": "build" })),
    ] {
        match fixture.registry.dispatch(&mut fixture.session, command, params) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{command}: {error:?}")
            }
            other => panic!("{command}: not refused: {other:?}"),
        }
    }
    assert!(fixture.env.load(&build_model::key(&fixture.project)).is_none(), "Gradle did not start");
}

#[test]
fn an_agent_does_not_choose_what_gradle_runs_with() {
    let mut fixture = Fixture::new();
    for key in ["distribution", "homePath", "jvm", "userHome", "vmOptions"] {
        let params = json!({ "key": key, "value": "/tmp/elsewhere" });
        match zen_core::macros::as_agent(&mut fixture.session, |session| {
            fixture.registry.dispatch(session, "gradle.settings.set", params)
        }) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::AGENT_NOT_ALLOWED), "{key}: {error:?}")
            }
            other => panic!("{key}: not refused: {other:?}"),
        }
    }
    let offline = zen_core::macros::as_agent(&mut fixture.session, |session| {
        fixture.registry.dispatch(session, "gradle.settings.set", json!({ "key": "offline", "value": true }))
    });
    assert!(offline.is_ok(), "{offline:?}");
    fixture.run("gradle.settings.set", json!({ "key": "homePath", "value": "/tmp/elsewhere" }));
}

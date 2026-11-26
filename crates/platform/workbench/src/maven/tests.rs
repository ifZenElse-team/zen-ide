use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_run::{RunEnv, RunProvider};

use super::*;
use crate::build_tools::BuildEnv;
use crate::run::Runs;

const POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>demo</groupId><artifactId>shop</artifactId><version>1.0</version><packaging>pom</packaging>
  <modules><module>core</module></modules>
  <profiles>
    <profile><id>fast</id><activation><activeByDefault>true</activeByDefault></activation></profile>
    <profile><id>ci</id></profile>
  </profiles>
</project>"#;

const CORE: &str = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>demo</groupId><artifactId>shop</artifactId><version>1.0</version></parent>
  <artifactId>core</artifactId>
  <dependencies><dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.13</version></dependency></dependencies>
</project>"#;

fn fake_maven(path: &Path, name: &str) {
    let script = format!(
        r#"#!/bin/sh
echo "{name} $*"
echo "in $(pwd -P)"
echo "java $JAVA_HOME"
echo "opts $MAVEN_OPTS"
echo "shop $SHOP"
for arg in "$@"; do
  case "$arg" in
    -DoutputFile=*) out="${{arg#-DoutputFile=}}" ;;
    -Doutput=*) printf '<project><artifactId>effective</artifactId></project>\n' > "${{arg#-Doutput=}}" ;;
    fail) echo "[ERROR] the build failed"; exit 1 ;;
  esac
done
if [ -n "$out" ]; then
  case "$out" in
    *'${{project.artifactId}}'*)
      shop=$(printf '%s' "$out" | sed 's/\${{project.groupId}}/demo/; s/\${{project.artifactId}}/shop/')
      core=$(printf '%s' "$out" | sed 's/\${{project.groupId}}/demo/; s/\${{project.artifactId}}/core/')
      printf 'demo:shop:pom:1.0\n' > "$shop"
      printf 'demo:core:jar:1.0\n\\- org.slf4j:slf4j-api:jar:2.0.13:compile\n' > "$core"
      ;;
    *)
      printf 'demo:shop:pom:1.0\n' > "$out"
      printf 'demo:core:jar:1.0\n\\- org.slf4j:slf4j-api:jar:2.0.13:compile\n' > "$out"
      ;;
  esac
fi
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
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(project.join("core")).unwrap();
        std::fs::write(project.join("pom.xml"), POM).unwrap();
        std::fs::write(project.join("core/pom.xml"), CORE).unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        fake_maven(&bin.join("mvn"), "mvn");
        let jdk = dir.path().join("jdk");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        let env = BuildEnv::default();
        env.set_folders(Some(vec![bin.clone()]));
        env.set_home(Some(dir.path().join("home")));
        env.set_data_dir(Some(dir.path().join("data")));
        env.set_jdk(Some(Arc::new(move |_root: &Path| Some(jdk.clone()))));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Runs>().set_providers(vec![Arc::new(MavenRun::new(env.clone()))], RunEnv::default());
        session.set_state(env.clone());
        crate::trust::trust_in_tests(&env, &project);
        Self { registry, session, project, env, bin, dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
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
fn the_wrapper_comes_first_and_the_options_become_flags() {
    let fixture = Fixture::new();
    let provider = MavenRun::new(fixture.env.clone());
    let mut configuration = MavenRun::goals("core", "core/pom.xml", "clean install", "ci,!fast");
    configuration.options["offline"] = json!(true);
    configuration.options["skipTests"] = json!(true);
    configuration.common.program_arguments = "-Dx='a b'".to_owned();
    let line = provider.command_line(&configuration, &fixture.project).unwrap();
    assert_eq!(line.program, fixture.bin.join("mvn"));
    assert_eq!(line.args, ["-o", "-P", "ci,!fast", "-DskipTests", "clean", "install", "-Dx=a b"]);
    assert_eq!(line.cwd, fixture.project.join("core"), "Maven runs in the pom.xml's folder");
    assert_eq!(line.display, "mvn -o -P ci,!fast -DskipTests clean install -Dx=a b");
    assert!(line.env.iter().any(|(name, value)| name == "JAVA_HOME" && value.ends_with("jdk")));
    assert_eq!(configuration.name, "core [clean install]");

    fake_maven(&fixture.project.join("mvnw"), "mvnw");
    let line = provider.command_line(&configuration, &fixture.project).unwrap();
    assert_eq!(line.program, fixture.project.join("mvnw"), "the project's wrapper, found above the module");
    assert!(line.display.starts_with("./mvnw "));

    let other = MavenRun::goals("x", "build/other.xml", "verify", "");
    let line = provider.command_line(&other, &fixture.project).unwrap();
    assert_eq!(
        &line.args[..2],
        ["-f".to_owned(), fixture.project.join("build/other.xml").to_string_lossy().into_owned()]
    );

    let empty = MavenRun::goals("x", "pom.xml", " ", "");
    assert!(provider.command_line(&empty, &fixture.project).is_err(), "goals are needed");
    std::fs::remove_file(fixture.project.join("mvnw")).unwrap();
    fixture.env.set_folders(Some(vec![fixture.dir.path().join("nowhere")]));
    let error = provider.command_line(&configuration, &fixture.project).unwrap_err();
    assert!(error.contains("Maven"), "{error}");
}

#[test]
fn a_shared_maven_configuration_is_adopted() {
    let root = Path::new("/p");
    let options: std::collections::BTreeMap<String, String> = [
        ("MavenSettings.myRunnerParameters.MavenRunnerParameters.goals.list.option.0", "clean"),
        ("MavenSettings.myRunnerParameters.MavenRunnerParameters.goals.list.option.1", "install"),
        ("MavenSettings.myRunnerParameters.MavenRunnerParameters.workingDirPath", "/p/core"),
        ("MavenSettings.myRunnerParameters.MavenRunnerParameters.profilesMap.map.ci", "true"),
        ("MavenSettings.myRunnerParameters.MavenRunnerParameters.profilesMap.map.fast", "false"),
        ("MavenSettings.myRunnerParameters.MavenRunnerParameters.cmdOptions", "-U"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    let foreign = ForeignRunConfiguration {
        source_file: PathBuf::from("/p/.run/install.run.xml"),
        name: "install core".to_owned(),
        type_id: "MavenRunConfiguration".to_owned(),
        factory: Some("Maven".to_owned()),
        options,
        env: Vec::new(),
        shared: true,
        before_launch: Vec::new(),
    };
    let adopted = MavenRun::default().adopt(&foreign, root).unwrap();
    assert_eq!(adopted.kind, "maven");
    assert_eq!(
        adopted.options,
        json!({ "pom": "core/pom.xml", "goals": "clean install", "profiles": "ci,!fast", "offline": false, "skipTests": false })
    );
    assert_eq!(adopted.common.program_arguments, "-U");
    let other = ForeignRunConfiguration { type_id: "GradleRunConfiguration".to_owned(), ..foreign };
    assert!(MavenRun::default().adopt(&other, root).is_none());
}

#[cfg(unix)]
#[test]
fn a_project_without_a_wrapper_finds_maven_in_the_wrapper_distributions_and_shows_it() {
    let mut fixture = Fixture::new();
    fixture.env.set_folders(Some(Vec::new()));
    let missing = fixture.run("maven.project.get", json!({}));
    assert!(missing.get("executable").is_none(), "{missing}");
    let home = fixture.dir.path().join("home/.m2/wrapper/dists/apache-maven-3.9.11/a2d47e15");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::create_dir_all(home.join("boot")).unwrap();
    std::fs::create_dir_all(home.join("lib")).unwrap();
    fake_maven(&home.join("bin/mvn"), "dists mvn");
    std::fs::write(home.join("boot/plexus-classworlds-2.9.0.jar"), "").unwrap();
    std::fs::write(home.join("lib/maven-core-3.9.11.jar"), "").unwrap();
    let project = fixture.run("maven.project.get", json!({}));
    assert_eq!(project["executable"]["path"], json!(home.join("bin/mvn")));
    assert_eq!(project["executable"]["source"], "wrapperDistribution");
    assert_eq!(project["executable"]["version"], "3.9.11");
    assert_eq!(project["executable"]["wrapper"], false);
    let reloaded = fixture.run("maven.project.reload", json!({ "waitMs": 20000 }));
    assert_eq!(reloaded["dependencyTree"]["state"], "loaded", "{reloaded}");
}

#[test]
fn the_project_is_read_and_goals_run_in_the_run_tool_window() {
    let mut fixture = Fixture::new();
    let project = fixture.run("maven.project.get", json!({}));
    assert_eq!(project["project"]["artifactId"], "shop");
    assert_eq!(project["project"]["modules"][0]["artifactId"], "core");
    assert_eq!(project["selectedProfiles"], json!(["fast"]), "active by default");
    assert_eq!(project["lifecycle"][2], "compile");
    assert_eq!(project["executable"]["wrapper"], false);
    assert!(project["dependencyTree"].get("state").is_none(), "never loaded: nothing ran");

    let started = fixture.run("maven.goal.run", json!({ "goals": "compile", "pom": "core", "waitMs": 20000 }));
    assert_eq!(started["summary"]["exitCode"], 0);
    let output = fixture.output(&started["runId"]);
    assert!(output.contains("mvn -P fast compile"), "{output}");
    assert!(
        output.contains(&format!("in {}", fixture.project.join("core").canonicalize().unwrap().display())),
        "{output}"
    );

    let chosen = fixture.run("maven.options.set", json!({ "profiles": ["ci"], "offline": true }));
    assert_eq!(chosen, json!({ "profiles": ["ci"], "offline": true, "skipTests": false }));
    let started = fixture.run("maven.goal.run", json!({ "goals": "package", "waitMs": 20000 }));
    let output = fixture.output(&started["runId"]);
    assert!(output.contains("mvn -o -P ci package"), "{output}");
    let started = fixture.run("maven.goal.run", json!({ "goals": "fail", "waitMs": 20000 }));
    assert_eq!(started["summary"]["exitCode"], 1);

    assert!(fixture.fail("maven.goal.run", json!({ "goals": "" })).contains("goal"));
    assert!(fixture.fail("maven.goal.run", json!({ "goals": "x", "pom": "nowhere" })).contains("nowhere"));
    std::fs::remove_file(fixture.project.join("pom.xml")).unwrap();
    assert!(fixture.fail("maven.project.get", json!({})).contains("pom.xml"));
}

#[test]
fn reload_asks_the_language_server_and_loads_the_dependency_tree() {
    let mut fixture = Fixture::new();
    let reloaded = fixture.run("maven.project.reload", json!({ "waitMs": 20000 }));
    assert_eq!(reloaded["reimported"], false, "no language server yet");
    assert_eq!(reloaded["modules"], 1);
    assert_eq!(reloaded["dependencyTree"]["state"], "loaded");
    let tree = fixture.run("maven.dependencies.tree", json!({}));
    assert_eq!(tree["roots"][1]["children"][0]["artifactId"], "slf4j-api");
    assert_eq!(tree["roots"].as_array().unwrap().len(), 2, "the output file is read once, not appended twice");
    let task = tree["task"].clone();
    let log = fixture.run("tasks.log.read", json!({ "id": task }));
    assert!(log.to_string().contains("dependency:tree"), "{log}");

    let asked = Arc::new(Mutex::new(0));
    let seen = asked.clone();
    fixture.env.set_reimport(Some(Arc::new(move |_root: &Path| {
        *seen.lock().unwrap() += 1;
        true
    })));
    let reloaded = fixture.run("maven.project.reload", json!({ "dependencies": false }));
    assert_eq!(reloaded["reimported"], true);
    assert_eq!(*asked.lock().unwrap(), 1);
}

#[test]
fn a_dependency_opens_from_the_local_repository() {
    let mut fixture = Fixture::new();
    let at = CORE.find("slf4j-api").unwrap();
    let error = fixture.fail("maven.dependency.open", json!({ "path": "core/pom.xml", "offset": at }));
    assert!(error.contains("org.slf4j:slf4j-api:2.0.13"), "{error}");
    let pom = fixture.dir.path().join("home/.m2/repository/org/slf4j/slf4j-api/2.0.13/slf4j-api-2.0.13.pom");
    std::fs::create_dir_all(pom.parent().unwrap()).unwrap();
    std::fs::write(&pom, "<project/>").unwrap();
    let opened = fixture.run("maven.dependency.open", json!({ "path": "core/pom.xml", "offset": at }));
    assert_eq!(opened["coordinates"], "org.slf4j:slf4j-api:2.0.13");
    let active = fixture
        .session
        .state::<crate::Workbench>()
        .and_then(crate::Workbench::active_document)
        .map(|d| d.path().to_owned());
    assert_eq!(active.as_deref(), Some(pom.as_path()));
    let error = fixture.fail("maven.dependency.open", json!({ "groupId": "a", "artifactId": "b", "version": "${v}" }));
    assert!(error.contains("a:b"), "{error}");
}

#[test]
fn a_downloaded_maven_is_found_and_not_downloaded_again() {
    let mut fixture = Fixture::new();
    let folder = fixture.dir.path().join("data/tools/maven").join(DOWNLOAD_VERSION);
    std::fs::create_dir_all(folder.join("bin")).unwrap();
    fake_maven(&folder.join("bin/mvn"), "downloaded");
    let answer = fixture.run("maven.distribution.download", json!({}));
    assert!(answer.get("task").is_none(), "there already: nothing is downloaded");
    fixture.env.set_folders(None);
    assert!(fixture.env.folders("maven").contains(&folder.join("bin")));
}

#[test]
#[ignore = "runs a real Maven: ZEN_TEST_MAVEN, ZEN_TEST_JDK, ZEN_TEST_MAVEN_REPO"]
fn a_real_maven_build_reloads_its_tree_and_compiles() {
    let (Some(maven), Some(jdk), Some(repo)) =
        (std::env::var_os("ZEN_TEST_MAVEN"), std::env::var_os("ZEN_TEST_JDK"), std::env::var_os("ZEN_TEST_MAVEN_REPO"))
    else {
        panic!("set ZEN_TEST_MAVEN, ZEN_TEST_JDK and ZEN_TEST_MAVEN_REPO");
    };
    let mut fixture = Fixture::new();
    let jdk = PathBuf::from(jdk);
    fixture.env.set_folders(Some(vec![PathBuf::from(maven)]));
    fixture.env.set_jdk(Some(Arc::new(move |_root: &Path| Some(jdk.clone()))));
    std::fs::create_dir_all(fixture.project.join(".mvn")).unwrap();
    std::fs::write(
        fixture.project.join(".mvn/maven.config"),
        format!("-Dmaven.repo.local={}\n", PathBuf::from(repo).display()),
    )
    .unwrap();
    let source = fixture.project.join("core/src/main/java/demo");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("App.java"), "package demo;\npublic class App { public static void main(String[] a) { org.slf4j.LoggerFactory.getLogger(App.class); } }\n").unwrap();
    let reloaded = fixture.run("maven.project.reload", json!({ "waitMs": 600_000 }));
    assert_eq!(reloaded["dependencyTree"]["state"], "loaded", "{reloaded}");
    let tree = fixture.run("maven.dependencies.tree", json!({}));
    assert_eq!(tree["roots"][1]["artifactId"], "core", "{tree}");
    assert_eq!(tree["roots"][1]["children"][0]["artifactId"], "slf4j-api", "{tree}");
    let started = fixture.run("maven.goal.run", json!({ "goals": "compile", "waitMs": 600_000 }));
    let output = fixture.output(&started["runId"]);
    assert_eq!(started["summary"]["exitCode"], 0, "{output}");
    assert!(fixture.project.join("core/target/classes/demo/App.class").is_file(), "{output}");
}

#[test]
#[ignore = "runs a real Maven: ZEN_TEST_MAVEN, ZEN_TEST_JDK, ZEN_TEST_MAVEN_REPO"]
fn a_real_maven_computes_a_modules_effective_pom_with_inherited_plugins() {
    let (Some(maven), Some(jdk), Some(repo)) =
        (std::env::var_os("ZEN_TEST_MAVEN"), std::env::var_os("ZEN_TEST_JDK"), std::env::var_os("ZEN_TEST_MAVEN_REPO"))
    else {
        panic!("set ZEN_TEST_MAVEN, ZEN_TEST_JDK and ZEN_TEST_MAVEN_REPO");
    };
    let mut fixture = Fixture::new();
    let jdk = PathBuf::from(jdk);
    fixture.env.set_folders(Some(vec![PathBuf::from(maven)]));
    fixture.env.set_jdk(Some(Arc::new(move |_root: &Path| Some(jdk.clone()))));
    std::fs::create_dir_all(fixture.project.join(".mvn")).unwrap();
    std::fs::write(
        fixture.project.join(".mvn/maven.config"),
        format!("-Dmaven.repo.local={}\n", PathBuf::from(repo).display()),
    )
    .unwrap();
    let shown = fixture.run("maven.effectivePom.show", json!({ "pom": "core", "waitMs": 300_000 }));
    let path = shown["path"].as_str().unwrap_or_else(|| panic!("{shown}"));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("<artifactId>core</artifactId>"), "{text}");
    assert!(text.contains("<groupId>demo</groupId>"), "the parent's group is inherited: {text}");
    assert!(text.contains("maven-compiler-plugin"), "the packaging's plug-ins are in: {text}");
}

fn task_log(fixture: &mut Fixture, task: &Value) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let log = fixture.run("tasks.log.read", json!({ "id": task }));
        if log["task"].get("ended").is_some() || Instant::now() > deadline {
            return log.to_string();
        }
        std::thread::yield_now();
    }
}

#[test]
fn the_maven_settings_reach_goals_reload_and_the_dependency_tree() {
    let mut fixture = Fixture::new();
    let runner_jdk = fixture.dir.path().join("runner-jdk");
    std::fs::create_dir_all(runner_jdk.join("bin")).unwrap();
    let importer_jdk = fixture.dir.path().join("importer-jdk");
    std::fs::create_dir_all(importer_jdk.join("bin")).unwrap();
    let settings = fixture.run(
        "maven.settings.set",
        json!({ "values": {
            "userSettings": "/conf/settings.xml",
            "localRepository": "/repo",
            "threads": "4",
            "updateSnapshots": true,
            "outputLevel": "debug",
            "checksumPolicy": "fail",
            "failPolicy": "failAtEnd",
            "runner.vmOptions": "-Xmx1g",
            "runner.jre": runner_jdk.display().to_string(),
            "runner.environment": "SHOP=blue; OTHER=1",
            "runner.properties": "env=dev",
            "importing.vmOptions": "-Xmx3g",
            "importing.jdk": importer_jdk.display().to_string(),
        } }),
    );
    assert_eq!(settings["runner"]["environment"], json!({ "OTHER": "1", "SHOP": "blue" }));
    let provider = MavenRun::new(fixture.env.clone());
    let line = provider.command_line(&MavenRun::goals("shop", "pom.xml", "verify", ""), &fixture.project).unwrap();
    assert_eq!(
        line.args,
        [
            "-s",
            "/conf/settings.xml",
            "-Dmaven.repo.local=/repo",
            "-T",
            "4",
            "-U",
            "-X",
            "-C",
            "-fae",
            "-Denv=dev",
            "verify"
        ]
    );
    let var = |line: &CommandLine, name: &str| {
        line.env.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.clone()).unwrap_or_default()
    };
    assert_eq!(var(&line, "MAVEN_OPTS"), "-Xmx1g");
    assert_eq!(var(&line, "SHOP"), "blue");
    assert_eq!(var(&line, "JAVA_HOME"), runner_jdk.display().to_string(), "the Runner's JRE");
    assert!(var(&line, "PATH").starts_with(&runner_jdk.join("bin").display().to_string()));

    let started = fixture.run("maven.goal.run", json!({ "goals": "compile", "waitMs": 20000 }));
    let output = fixture.output(&started["runId"]);
    assert!(output.contains("-T 4 -U -X -C -fae -Denv=dev -P fast compile"), "{output}");
    assert!(output.contains("opts -Xmx1g") && output.contains("shop blue"), "{output}");

    let reloaded = fixture.run("maven.project.reload", json!({ "waitMs": 20000 }));
    assert_eq!(reloaded["dependencyTree"]["state"], "loaded", "{reloaded}");
    let log = task_log(&mut fixture, &reloaded["dependencyTree"]["task"]);
    assert!(
        log.contains("-s /conf/settings.xml -Dmaven.repo.local=/repo -T 4 -U -X -C -fae -P fast dependency:tree"),
        "{log}"
    );
    assert!(!log.contains("-Denv=dev") && !log.contains("shop blue"), "the Runner's are not the importer's: {log}");
    assert!(log.contains("opts -Xmx3g"), "{log}");
    assert!(log.contains(&format!("java {}", importer_jdk.display())), "the importer's JDK: {log}");
    assert!(reloaded.get("downloads").is_none(), "nothing more to download: {reloaded}");
}

#[test]
fn reload_downloads_sources_and_documentation_when_importing_asks() {
    let mut fixture = Fixture::new();
    fixture.run(
        "maven.settings.set",
        json!({ "values": { "importing.downloadSources": true, "importing.downloadDocumentation": true } }),
    );
    let reloaded = fixture.run("maven.project.reload", json!({ "waitMs": 20000 }));
    let tasks = reloaded["downloads"].as_array().unwrap().clone();
    assert_eq!(tasks.len(), 2, "{reloaded}");
    let log = task_log(&mut fixture, &tasks[0]);
    assert!(log.contains("dependency:sources -B"), "{log}");
    assert!(task_log(&mut fixture, &tasks[1]).contains("dependency:resolve -Dclassifier=javadoc -B"));
}

#[test]
fn the_tool_window_toggles_are_the_settings_and_are_kept_with_the_project() {
    let mut fixture = Fixture::new();
    fixture.run("maven.options.set", json!({ "offline": true }));
    assert_eq!(fixture.run("maven.settings.get", json!({}))["offline"], true);
    fixture.run("maven.settings.set", json!({ "key": "runner.skipTests", "value": true }));
    let project = fixture.run("maven.project.get", json!({}));
    assert_eq!((project["offline"].clone(), project["skipTests"].clone()), (json!(true), json!(true)));
    let started = fixture.run("maven.goal.run", json!({ "goals": "package", "waitMs": 20000 }));
    assert!(fixture.output(&started["runId"]).contains("mvn -o -P fast -DskipTests package"));

    let again = BuildEnv::default();
    again.set_data_dir(Some(fixture.dir.path().join("data")));
    let kept = MavenSettings::read(&again, &fixture.project);
    assert!(kept.offline && kept.runner.skip_tests, "kept in the IDE's data folder for this project");
    assert_eq!(MavenSettings::read(&again, &fixture.dir.path().join("other")), MavenSettings::default());

    fixture.run("maven.settings.set", json!({ "values": { "offline": null, "runner.skipTests": false } }));
    assert_eq!(fixture.run("maven.options.set", json!({}))["offline"], false, "null is the default");
    assert!(fixture.fail("maven.settings.set", json!({ "key": "nope", "value": 1 })).contains("nope"));
    assert!(fixture.fail("maven.settings.set", json!({ "key": "outputLevel", "value": "loud" })).contains("loud"));
}

#[test]
fn the_maven_home_is_the_wrapper_maven_on_the_computer_or_a_folder() {
    let mut fixture = Fixture::new();
    fake_maven(&fixture.project.join("mvnw"), "mvnw");
    let provider = MavenRun::new(fixture.env.clone());
    let goals = MavenRun::goals("shop", "pom.xml", "verify", "");
    assert_eq!(provider.command_line(&goals, &fixture.project).unwrap().program, fixture.project.join("mvnw"));
    fixture.run("maven.settings.set", json!({ "key": "home", "value": "installed" }));
    assert_eq!(provider.command_line(&goals, &fixture.project).unwrap().program, fixture.bin.join("mvn"));
    let home = fixture.dir.path().join("apache-maven");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    fake_maven(&home.join("bin/mvn"), "custom");
    fixture
        .run("maven.settings.set", json!({ "values": { "home": "custom", "homePath": home.display().to_string() } }));
    assert_eq!(provider.command_line(&goals, &fixture.project).unwrap().program, home.join("bin/mvn"));
    assert_eq!(fixture.run("maven.project.get", json!({}))["executable"]["path"], json!(home.join("bin/mvn")));
    fixture.run("maven.settings.set", json!({ "key": "homePath", "value": "/nowhere" }));
    assert!(provider.command_line(&goals, &fixture.project).unwrap_err().contains("Maven"));
}

#[test]
fn the_local_repository_is_the_settings_then_the_user_settings_file() {
    let mut fixture = Fixture::new();
    let home = fixture.dir.path().join("home");
    let settings = MavenSettings::default();
    assert_eq!(settings.local_repository(&home), home.join(".m2/repository"));
    std::fs::create_dir_all(home.join(".m2")).unwrap();
    std::fs::write(
        home.join(".m2/settings.xml"),
        "<settings><localRepository>${user.home}/repo</localRepository></settings>",
    )
    .unwrap();
    assert_eq!(settings.local_repository(&home), home.join("repo"));
    let other = fixture.dir.path().join("other-settings.xml");
    std::fs::write(&other, "<settings><localRepository>/elsewhere</localRepository></settings>").unwrap();
    let chosen = MavenSettings { user_settings: other.display().to_string(), ..settings.clone() };
    assert_eq!(chosen.local_repository(&home), PathBuf::from("/elsewhere"));
    let repository = fixture.dir.path().join("repository");
    let pom = repository.join("org/slf4j/slf4j-api/2.0.13/slf4j-api-2.0.13.pom");
    std::fs::create_dir_all(pom.parent().unwrap()).unwrap();
    std::fs::write(&pom, "<project/>").unwrap();
    fixture.run("maven.settings.set", json!({ "key": "localRepository", "value": repository.display().to_string() }));
    let at = CORE.find("slf4j-api").unwrap();
    let opened = fixture.run("maven.dependency.open", json!({ "path": "core/pom.xml", "offset": at }));
    assert_eq!(opened["pom"], json!(pom));
}

#[test]
fn the_settings_pages_show_the_values_and_change_them_by_key() {
    let mut fixture = Fixture::new();
    fixture.run("maven.settings.set", json!({ "values": { "offline": true, "runner.properties": "a=1;b=2" } }));
    let form: zen_modules::form::Form = serde_json::from_value(fixture.run("maven.settings.form", json!({}))).unwrap();
    let offline = form
        .items
        .iter()
        .find_map(|item| match item {
            zen_modules::form::Item::Check { label, value, action } if label == "Work offline" => {
                Some((*value, action))
            }
            _ => None,
        })
        .unwrap();
    assert!(offline.0);
    assert_eq!(offline.1.params_with(Some(json!(false))), json!({ "key": "offline", "value": false }));
    assert!(form.texts().contains(&"Thread count:"), "{:?}", form.texts());
    let runner = fixture.run("maven.runner.form", json!({}));
    assert!(runner.to_string().contains("a=1; b=2"), "{runner}");
    for page in ["buildTools.settings.form", "maven.importing.form", "maven.runningTests.form"] {
        let form: zen_modules::form::Form = serde_json::from_value(fixture.run(page, json!({}))).unwrap();
        assert!(!form.items.is_empty(), "{page}");
    }
    let tests = fixture.run("maven.runningTests.form", json!({}));
    assert!(tests.to_string().contains("systemPropertyVariables"), "{tests}");
}
#[cfg(unix)]
#[test]
fn a_maven_goal_runs_before_launch_and_a_failing_one_cancels_the_start() {
    let mut fixture = Fixture::new();
    let types = fixture.run("run.beforeLaunch.types", json!({}));
    let maven = types["types"].as_array().unwrap().iter().find(|kind| kind["id"] == GOAL_TASK).unwrap().clone();
    assert_eq!((maven["provider"].clone(), maven["name"].clone()), (json!("maven"), json!("Run Maven Goal")));
    let mut configuration = serde_json::to_value(MavenRun::goals("shop", "pom.xml", "package", "")).unwrap();
    configuration["common"]["beforeLaunch"] = json!([{ "type": GOAL_TASK, "pom": "core/pom.xml", "goals": "clean" }]);
    let created = fixture.run("run.configuration.create", json!({ "configuration": configuration }));
    let started = fixture.run("run.process.start", json!({ "configurationId": created["id"], "waitMs": 20000 }));
    assert_eq!(started["summary"]["state"], "finished");
    let output = fixture.output(&started["runId"]);
    let goal = output.find("mvn clean").unwrap_or_else(|| panic!("{output}"));
    let program = output.find("mvn package").unwrap_or_else(|| panic!("{output}"));
    assert!(output.contains("#before-launch Run Maven Goal “clean”") && goal < program, "{output}");
    let core = fixture.project.join("core").canonicalize().unwrap();
    assert!(output.contains(&format!("in {}", core.display())), "the goal runs in its pom.xml's folder: {output}");

    configuration["common"]["beforeLaunch"] = json!([{ "type": GOAL_TASK, "goals": "fail" }]);
    configuration["name"] = json!("failing");
    let created = fixture.run("run.configuration.create", json!({ "configuration": configuration }));
    let started = fixture.run("run.process.start", json!({ "configurationId": created["id"], "waitMs": 20000 }));
    assert_eq!(started["summary"]["state"], "beforeLaunchFailed");
    let output = fixture.output(&started["runId"]);
    assert!(output.contains("[ERROR] the build failed") && !output.contains("mvn package"), "{output}");
}

#[test]
fn an_adopted_maven_goal_keeps_its_goal_and_pom() {
    let foreign = ForeignTask {
        id: "Maven.BeforeRunTask".to_owned(),
        enabled: true,
        options: [("goal".to_owned(), "clean install".to_owned()), ("file".to_owned(), "/p/core/pom.xml".to_owned())]
            .into_iter()
            .collect(),
    };
    let task = MavenRun::default().adopt_task(&foreign, Path::new("/p")).unwrap();
    assert_eq!(task.kind, GOAL_TASK);
    assert_eq!((task.text("goals"), task.text("pom")), ("clean install", "core/pom.xml"));
    assert!(MavenRun::default().adopt_task(&ForeignTask::default(), Path::new("/p")).is_none());
}
#[test]
fn run_anything_runs_mvn_goals_in_the_module_of_the_folder() {
    let fixture = Fixture::new();
    let provider = MavenRun::new(fixture.env.clone());
    let context = RunContext { root: fixture.project.clone(), ..RunContext::default() };
    let words = |line: &str| zen_run::args::split(line);
    assert_eq!(provider.anything_prefixes()[0].prefix, "mvn");
    assert!(provider.anything(&words("gradle build"), &fixture.project, &context).is_none());
    assert!(provider.anything(&words("mvn"), &fixture.project, &context).unwrap().is_err());
    let nested = fixture.project.join("core/src");
    std::fs::create_dir_all(&nested).unwrap();
    let configuration = provider.anything(&words("mvn clean install"), &nested, &context).unwrap().unwrap();
    assert_eq!(configuration.name, "mvn clean install");
    assert!(configuration.temporary);
    let line = provider.command_line(&configuration, &fixture.project).unwrap();
    assert_eq!(line.cwd, fixture.project.join("core"));
    assert!(line.args.ends_with(&["clean".to_owned(), "install".to_owned()]), "{:?}", line.args);
    let root = provider.anything(&words("mvn test"), &fixture.project, &context).unwrap().unwrap();
    assert_eq!(provider.command_line(&root, &fixture.project).unwrap().cwd, fixture.project);
}

#[test]
fn a_multi_module_tree_keeps_every_module_as_a_real_maven_overwrites_one_output_file() {
    let mut fixture = Fixture::new();
    fixture.run("maven.dependencies.tree", json!({ "load": true, "waitMs": 20000 }));
    let tree = fixture.run("maven.dependencies.tree", json!({}));
    let roots: Vec<&str> =
        tree["roots"].as_array().unwrap().iter().map(|root| root["artifactId"].as_str().unwrap()).collect();
    assert_eq!(roots, ["shop", "core"], "{tree}");
    assert_eq!(tree["roots"][1]["children"][0]["artifactId"], "slf4j-api", "{tree}");
}

#[test]
fn profiles_links_ignores_and_the_view_are_kept_with_the_project_across_sessions() {
    let mut fixture = Fixture::new();
    fixture.run("maven.options.set", json!({ "profiles": ["ci"] }));
    let outside = fixture.dir.path().join("tools");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        outside.join("pom.xml"),
        CORE.replace("<artifactId>core</artifactId>", "<artifactId>tools</artifactId>"),
    )
    .unwrap();
    crate::trust::trust_in_tests(&fixture.env, &outside);
    let linked = fixture.run("maven.project.add", json!({ "pom": outside }));
    assert_eq!(linked["linked"], json!([outside.join("pom.xml")]));
    assert!(fixture.fail("maven.project.add", json!({ "pom": "nowhere" })).contains("There is no pom.xml"));
    fixture.run("maven.project.ignore", json!({ "pom": "core" }));
    fixture.run("maven.view.set", json!({ "basicPhases": false, "groupModules": false }));
    fixture.run("maven.goal.run", json!({ "goals": "clean  install", "waitMs": 20000 }));

    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(fixture.env.clone());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": fixture.project })).unwrap();
    let project = registry.dispatch(&mut session, "maven.project.get", json!({})).unwrap();
    assert_eq!(project["selectedProfiles"], json!(["ci"]), "the ticks outlive the IDE");
    assert_eq!(project["linked"][0]["artifactId"], "tools");
    assert_eq!(project["ignored"], json!([fixture.project.join("core/pom.xml")]));
    assert_eq!(project["view"], json!({ "groupModules": false, "showIgnored": true, "basicPhases": false }));
    assert_eq!(project["lifecycle"].as_array().unwrap().len(), 30, "every phase of the three lifecycles");
    assert_eq!(project["history"], json!(["clean install"]));
    assert_eq!(project["rootLinked"], true);

    fixture.run("maven.project.unlink", json!({ "pom": "pom.xml" }));
    let unlinked = fixture.run("maven.project.unlink", json!({ "pom": outside }));
    assert_eq!((unlinked["rootLinked"].clone(), unlinked["linked"].clone()), (json!(false), json!([])));
    assert!(fixture.fail("maven.project.unlink", json!({ "pom": "core" })).contains("goes with its project"));
    assert_eq!(fixture.run("maven.project.add", json!({ "pom": "." }))["rootLinked"], true);
    let back = fixture.run("maven.project.ignore", json!({ "pom": "core/pom.xml" }));
    assert_eq!(back["ignored"], json!([]));
}

#[test]
fn execute_goal_completes_phases_and_plugin_goals_after_the_history() {
    let mut fixture = Fixture::new();
    fixture.run("maven.goal.run", json!({ "goals": "clean package", "waitMs": 20000 }));
    let texts = |answer: Value| -> Vec<String> {
        answer["suggestions"].as_array().unwrap().iter().map(|s| s["text"].as_str().unwrap().to_owned()).collect()
    };
    let answer = fixture.run("maven.goal.complete", json!({ "text": "clean" }));
    assert_eq!(answer["suggestions"][0], json!({ "text": "clean package", "kind": "history" }));
    let after = texts(fixture.run("maven.goal.complete", json!({ "text": "clean ins" })));
    assert_eq!(after, ["clean install", "clean install:install", "clean install:install-file", "clean install:help"]);
    let goals = texts(fixture.run("maven.goal.complete", json!({ "text": "dependency:t" })));
    assert_eq!(goals, ["dependency:tree"]);
    let names = suggestions_of("compiler:", &[], &["compiler:compile".to_owned(), "jar:jar".to_owned()]);
    assert_eq!(names, ["compiler:compile"]);
}

fn suggestions_of(text: &str, history: &[String], goals: &[String]) -> Vec<String> {
    workspace::suggestions(text, history, goals).into_iter().map(|suggestion| suggestion.text).collect()
}

#[test]
fn the_effective_pom_comes_from_maven_once_per_content_and_opens_read_only() {
    let mut fixture = Fixture::new();
    let folder = crate::virtual_documents::folder();
    let shown = fixture.run("maven.effectivePom.show", json!({ "pom": "core", "waitMs": 20000 }));
    let path = PathBuf::from(shown["path"].as_str().unwrap_or_else(|| panic!("{shown}")));
    assert!(path.starts_with(folder.join("effective-pom")), "{shown}");
    assert!(std::fs::read_to_string(&path).unwrap().contains("<artifactId>effective</artifactId>"));
    let active = fixture.session.state::<crate::Workbench>().and_then(crate::Workbench::active_document).unwrap();
    assert_eq!(active.path(), path);
    assert!(active.read_only().unwrap().contains("generated by Maven"), "{:?}", active.read_only());

    std::fs::remove_file(fixture.bin.join("mvn")).unwrap();
    let again = fixture.run("maven.effectivePom.show", json!({ "pom": "core" }));
    assert_eq!(again["path"], shown["path"], "kept while the pom.xml files stay as they are");
    std::fs::write(fixture.project.join("core/pom.xml"), CORE.replace("2.0.13", "2.0.16")).unwrap();
    let changed = fixture.fail("maven.effectivePom.show", json!({ "pom": "core" }));
    assert!(changed.contains("Maven is not found"), "a changed pom.xml asks Maven again: {changed}");
}

#[test]
fn settings_xml_is_created_when_missing_and_a_goal_becomes_a_run_configuration() {
    let mut fixture = Fixture::new();
    let opened = fixture.run("maven.settingsFile.open", json!({}));
    let settings = fixture.dir.path().join("home/.m2/settings.xml");
    assert_eq!((opened["path"].clone(), opened["created"].clone()), (json!(settings), json!(true)));
    assert!(std::fs::read_to_string(&settings).unwrap().contains("<settings"));
    assert_eq!(fixture.run("maven.settingsFile.open", json!({}))["created"], false);

    let created = fixture.run("maven.runConfiguration.create", json!({ "goals": "verify", "pom": "core" }));
    assert_eq!(created["name"], "core [verify]");
    let list = fixture.run("run.configuration.list", json!({}));
    let saved = list["configurations"].as_array().unwrap().iter().find(|c| c["id"] == created["id"]).unwrap().clone();
    assert_eq!(saved["options"]["pom"], "core/pom.xml");
    assert_eq!(saved["options"]["profiles"], "fast");
    assert_ne!(saved["temporary"], true);
    assert_eq!(list["selected"], created["id"]);
}

#[test]
fn download_sources_runs_both_downloads_whatever_importing_says() {
    let mut fixture = Fixture::new();
    let answer = fixture.run("maven.sources.download", json!({ "documentation": false }));
    let tasks = answer["tasks"].as_array().unwrap().clone();
    assert_eq!(tasks.len(), 1);
    assert!(task_log(&mut fixture, &tasks[0]).contains("dependency:sources -B"));
}

#[test]
fn a_goal_debugs_through_maven_opts_and_waits_for_the_debugger() {
    let fixture = Fixture::new();
    let provider = MavenRun::new(fixture.env.clone());
    let context = RunContext { root: fixture.project.clone(), ..RunContext::default() };
    let configuration = MavenRun::goals("core", "core/pom.xml", "exec:java", "");
    let plan = provider.debug(&configuration, &context).unwrap().unwrap();
    let debuggee = plan.debuggee.unwrap();
    let opts = debuggee.line.env.iter().find(|(name, _)| name == "MAVEN_OPTS").unwrap().1.clone();
    assert!(opts.starts_with("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:"), "{opts}");
    assert_eq!(debuggee.line.args, ["exec:java"]);
    assert_eq!(plan.launch["request"], "attach");
}

#[test]
fn a_maven_goal_runs_around_the_ides_builds_as_a_build_trigger() {
    let mut fixture = Fixture::new();
    let task = json!({ "type": "maven.goal", "pom": "core/pom.xml", "goals": "generate-sources" });
    let set = fixture.run("build.triggers.set", json!({ "when": "beforeBuild", "task": task }));
    assert_eq!(set["triggers"][0]["when"], "beforeBuild");
    assert!(fixture.project.join(".zen/build-triggers.json").is_file());
    let root = fixture.project.clone();
    let lines =
        crate::build_triggers::lines(&mut fixture.session, &root, crate::build_triggers::When::BeforeBuild).unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].args, ["generate-sources"]);
    assert_eq!(lines[0].cwd, fixture.project.join("core"));
    assert!(
        crate::build_triggers::lines(&mut fixture.session, &root, crate::build_triggers::When::AfterBuild)
            .unwrap()
            .is_empty()
    );
    let removed = fixture.run("build.triggers.set", json!({ "when": "beforeBuild", "task": task }));
    assert_eq!(removed["triggers"], json!([]), "a second toggle removes it");
    assert!(!fixture.project.join(".zen/build-triggers.json").exists());
    assert!(
        fixture
            .fail("build.triggers.set", json!({ "when": "afterBuild", "task": { "type": "nope" } }))
            .contains("nope")
    );
}

#[test]
fn a_person_loads_no_build_of_a_project_not_trusted() {
    let mut fixture = Fixture::new();
    fixture.run("project.trust.revoke", json!({}));
    for (command, params) in [
        ("maven.project.reload", json!({})),
        ("maven.dependencies.tree", json!({ "load": true })),
        ("maven.sources.download", json!({})),
        ("maven.effectivePom.show", json!({})),
        ("build.dependencies.analyze", json!({ "load": true, "system": "maven" })),
        ("maven.goal.run", json!({ "goals": "compile" })),
        ("maven.goal.debug", json!({ "goals": "compile" })),
        ("run.anything.run", json!({ "text": "mvn compile" })),
    ] {
        match fixture.registry.dispatch(&mut fixture.session, command, params) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{command}: {error:?}")
            }
            other => panic!("{command}: not refused: {other:?}"),
        }
    }
    assert!(fixture.env.load(&tree_key(&fixture.project)).is_none(), "nothing was started");
    let polled = fixture.registry.dispatch(&mut fixture.session, "maven.dependencies.tree", json!({}));
    assert!(polled.is_ok(), "reading what is known starts nothing: {polled:?}");
}

#[test]
fn a_pom_outside_the_project_links_only_from_a_trusted_folder() {
    let mut fixture = Fixture::new();
    let elsewhere = fixture.dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let pom = elsewhere.join("pom.xml");
    std::fs::write(&pom, "<project><artifactId>other</artifactId></project>\n").unwrap();
    let params = json!({ "pom": pom });
    match fixture.registry.dispatch(&mut fixture.session, "maven.project.add", params.clone()) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{error:?}")
        }
        other => panic!("not refused: {other:?}"),
    }
    crate::trust::trust_in_tests(&fixture.env, &elsewhere);
    fixture.run("maven.project.add", params);
}

#[test]
fn run_anything_refuses_a_build_in_safe_mode_for_a_person_and_an_agent() {
    let mut fixture = Fixture::new();
    fixture.run("project.trust.revoke", json!({}));
    let params = json!({ "text": "mvn compile" });
    let person = fixture.registry.dispatch(&mut fixture.session, "run.anything.run", params.clone());
    let agent = zen_core::macros::as_agent(&mut fixture.session, |session| {
        fixture.registry.dispatch(session, "run.anything.run", params)
    });
    for answer in [person, agent] {
        match answer {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{error:?}")
            }
            other => panic!("not refused: {other:?}"),
        }
    }
}

#[test]
fn an_agent_does_not_choose_what_maven_runs_with() {
    let mut fixture = Fixture::new();
    for key in [
        "home",
        "homePath",
        "userSettings",
        "localRepository",
        "importing.jdk",
        "importing.vmOptions",
        "importing",
        "runner.vmOptions",
        "runner.jre",
        "runner.environment",
        "runner.properties",
        "runner",
        "runner/vmOptions",
    ] {
        let params = json!({ "key": key, "value": "/tmp/elsewhere" });
        match zen_core::macros::as_agent(&mut fixture.session, |session| {
            fixture.registry.dispatch(session, "maven.settings.set", params)
        }) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::AGENT_NOT_ALLOWED), "{key}: {error:?}")
            }
            other => panic!("{key}: not refused: {other:?}"),
        }
    }
    let offline = zen_core::macros::as_agent(&mut fixture.session, |session| {
        fixture.registry.dispatch(session, "maven.settings.set", json!({ "key": "offline", "value": true }))
    });
    assert!(offline.is_ok(), "{offline:?}");
    fixture.run("maven.settings.set", json!({ "key": "homePath", "value": "/tmp/elsewhere" }));
}

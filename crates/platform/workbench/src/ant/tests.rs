use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_run::RunEnv;

use super::*;
use crate::build_tools::BuildEnv;
use crate::run::Runs;

const BUILD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project name="shop" default="jar" basedir=".">
  <description>
    Builds the shop.
  </description>
  <property name="src.dir" value="src"/>
  <target name="init">
    <mkdir dir="build"/>
  </target>
  <target name="compile" depends="init" description="Compiles the sources">
    <javac srcdir="${src.dir}" destdir="build" includeantruntime="false"/>
  </target>
  <target name="jar" depends="init, compile" description="Packs the jar">
    <antcall target="init"/>
  </target>
  <extension-point name="ready" depends="jar"/>
  <target name="fail"/>
</project>
"#;

const TOOLS: &str = r#"<project name="tools"><target name="lint"/></project>"#;

fn fake_ant(path: &Path) {
    let script = r#"#!/bin/sh
echo "ant $*"
echo "in $(pwd -P)"
echo "java $JAVA_HOME"
echo "opts $ANT_OPTS"
for arg in "$@"; do
  case "$arg" in
    fail) echo "    [javac] $PWD/src/A.java:3: error: incompatible types"; echo "BUILD FAILED"; echo "$PWD/build.xml:20: the build failed"; exit 1 ;;
  esac
done
echo "BUILD SUCCESSFUL"
"#;
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
    jdk: PathBuf,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(project.join("tools")).unwrap();
        std::fs::write(project.join("build.xml"), BUILD).unwrap();
        std::fs::write(project.join("tools/tools.xml"), TOOLS).unwrap();
        std::fs::write(project.join("tools/pom.xml"), "<project xmlns=\"x\"/>").unwrap();
        std::fs::write(project.join("tools/other.xml"), "<settings/>").unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        fake_ant(&bin.join("ant"));
        let jdk = dir.path().join("jdk");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        let env = BuildEnv::default();
        env.set_folders(Some(vec![bin.clone()]));
        env.set_data_dir(Some(dir.path().join("data")));
        let project_jdk = jdk.clone();
        env.set_jdk(Some(Arc::new(move |_root: &Path| Some(project_jdk.clone()))));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Runs>().set_providers(vec![Arc::new(AntRun::new(env.clone()))], RunEnv::default());
        session.set_state(env.clone());
        crate::trust::trust_in_tests(&env, &project);
        Self { registry, session, project, env, bin, jdk, dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }

    fn output(&mut self, run_id: &Value) -> (String, Value) {
        let read = self.run("run.output.get", json!({ "runId": run_id }));
        let lines = read["lines"].as_array().unwrap();
        let text = lines.iter().map(|line| line["text"].as_str().unwrap_or_default()).collect::<Vec<_>>().join("\n");
        let links = lines.iter().flat_map(|line| line["links"].as_array().cloned().unwrap_or_default()).collect();
        (text, Value::Array(links))
    }
}

#[test]
fn a_build_file_gives_its_project_default_and_targets() {
    let file = model::parse(Path::new("/p/build.xml"), BUILD);
    assert_eq!((file.name.as_str(), file.default.as_str()), ("shop", "jar"));
    assert_eq!(file.description, "Builds the shop.");
    let names: Vec<&str> = file.targets.iter().map(|target| target.name.as_str()).collect();
    assert_eq!(names, ["init", "compile", "jar", "ready", "fail"]);
    let jar = &file.targets[2];
    assert_eq!(jar.depends, ["init", "compile"]);
    assert_eq!(jar.description, "Packs the jar");
    assert_eq!(BUILD.chars().skip(jar.offset).take(3).collect::<String>(), "jar");
    assert_eq!(jar.line, 13);
    assert!(file.targets[3].extension_point);

    let broken = model::parse(Path::new("/p/x.xml"), "<project><target></project>");
    assert!(broken.error.is_some() && broken.targets.is_empty());
    let other = model::parse(Path::new("/p/pom.xml"), "<settings/>");
    assert!(other.error.unwrap().contains("settings"));
    assert_eq!(model::parse(Path::new("/p/b.xml"), "<project/>").name, "b.xml", "the file's name without one");

    assert!(model::is_build_file(BUILD));
    assert!(model::is_build_file("<!-- x -->\n<!DOCTYPE project>\n<project>"));
    assert!(!model::is_build_file("<projects/>") && !model::is_build_file("<settings/>"));
    assert!(!model::is_build_file("<project xmlns=\"http://maven.apache.org/POM/4.0.0\">"), "a POM");
    assert!(model::is_build_file("<project/>"));
}

#[test]
fn the_target_under_a_reference_is_its_name_in_a_list() {
    let at = |needle: &str, inside: usize| model::target_at(BUILD, BUILD.find(needle).unwrap() + inside);
    assert_eq!(at("depends=\"init, compile\"", 15).as_deref(), Some("compile"));
    assert_eq!(at("depends=\"init, compile\"", 10).as_deref(), Some("init"));
    assert_eq!(at("default=\"jar\"", 10).as_deref(), Some("jar"));
    assert_eq!(at("<antcall target=\"init\"", 18).as_deref(), Some("init"));
    assert_eq!(at("srcdir=", 9), None, "not a target");
    assert_eq!(at("<mkdir", 2), None);
}

#[test]
fn ant_runs_with_the_build_files_jdk_classpath_memory_and_properties() {
    let mut fixture = Fixture::new();
    let provider = AntRun::new(fixture.env.clone());
    let configuration = AntRun::targets("tools", "tools/tools.xml", "lint");
    assert_eq!(configuration.name, "tools [lint]");
    std::fs::remove_file(fixture.bin.join("ant")).unwrap();
    fixture.run("ant.buildFile.add", json!({ "path": "tools/tools.xml" }));
    let error = provider.command_line(&configuration, &fixture.project).unwrap_err();
    assert!(error.contains("Ant is not found"), "{error}");
    fake_ant(&fixture.bin.join("ant"));

    let line = provider.command_line(&configuration, &fixture.project).unwrap();
    assert_eq!(line.program, fixture.bin.join("ant"));
    let file = fixture.project.join("tools/tools.xml").to_string_lossy().into_owned();
    assert_eq!(line.args, ["-buildfile", file.as_str(), "lint"]);
    assert_eq!(line.cwd, fixture.project.join("tools"), "Ant runs in the build file's folder");
    assert_eq!(line.display, "ant -buildfile tools/tools.xml lint");
    let java = |line: &CommandLine| line.env.iter().find(|(name, _)| name == "JAVA_HOME").unwrap().1.clone();
    assert_eq!(java(&line), fixture.jdk.to_string_lossy(), "the project's JDK");

    let own = fixture.dir.path().join("jdk-21");
    let refused = fixture.fail("ant.buildFile.configure", json!({ "path": "tools/tools.xml", "jdk": own }));
    assert!(refused.contains("not a JDK"), "{refused}");
    std::fs::create_dir_all(own.join("bin")).unwrap();
    let set = fixture.run(
        "ant.buildFile.configure",
        json!({
            "path": fixture.project.join("tools/tools.xml"), "jdk": own, "classpath": ["lib/extra.jar", " "],
            "maxMemory": 512, "properties": [{ "name": "env", "value": "ci x" }, { "name": " ", "value": "dropped" }]
        }),
    );
    assert_eq!(set["maxMemory"], 512);
    assert_eq!(set["classpath"], json!(["lib/extra.jar"]));
    assert_eq!(set["properties"], json!([{ "name": "env", "value": "ci x" }]));
    let line = provider.command_line(&configuration, &fixture.project).unwrap();
    let extra = fixture.project.join("lib/extra.jar").to_string_lossy().into_owned();
    assert_eq!(line.args, ["-buildfile", file.as_str(), "-lib", extra.as_str(), "-Denv=ci x", "lint"]);
    assert_eq!(java(&line), own.to_string_lossy(), "the build file's own JDK");
    let path = &line.env.iter().find(|(name, _)| name == "PATH").unwrap().1;
    assert!(path.starts_with(&*own.join("bin").to_string_lossy()), "{path}");
    assert!(line.env.iter().any(|(name, value)| name == "ANT_OPTS" && value.ends_with("-Xmx512m")));

    let kept = fixture.run("ant.buildFile.configure", json!({ "path": "tools/tools.xml", "maxMemory": 0 }));
    assert_eq!(kept["jdk"], own.to_string_lossy().as_ref(), "what is not given stays");
    assert!(kept.get("maxMemory").is_none());
    let saved: Value = serde_json::from_slice(&std::fs::read(fixture.project.join(SETTINGS_FILE)).unwrap()).unwrap();
    assert_eq!(saved["buildFiles"][1]["path"], "tools/tools.xml");
}

#[test]
fn build_files_are_listed_added_removed_and_filtered() {
    let mut fixture = Fixture::new();
    let project = fixture.run("ant.project.get", json!({}));
    assert_eq!(project["buildFiles"].as_array().unwrap().len(), 1, "the project's build.xml");
    assert_eq!(project["buildFiles"][0]["name"], "shop");
    assert_eq!(project["buildFiles"][0]["default"], "jar");
    assert_eq!(project["buildFiles"][0]["targets"][1]["description"], "Compiles the sources");
    assert_eq!(project["buildFiles"][0]["settings"]["path"], "build.xml");
    assert_eq!(project["executable"], fixture.bin.join("ant").to_string_lossy().as_ref());
    assert_eq!(project["filterTargets"], false);
    assert!(!fixture.project.join(SETTINGS_FILE).exists(), "nothing written until something changes");

    let added = fixture.run("ant.buildFile.add", json!({ "path": "tools/tools.xml" }));
    assert_eq!(added["buildFiles"][1]["name"], "tools");
    let again = fixture.run("ant.buildFile.add", json!({ "path": fixture.project.join("tools/tools.xml") }));
    assert_eq!(again["buildFiles"].as_array().unwrap().len(), 2, "added once");
    assert!(fixture.fail("ant.buildFile.add", json!({ "path": "tools/other.xml" })).contains("not an Ant build file"));
    assert!(fixture.fail("ant.buildFile.add", json!({ "path": "tools/pom.xml" })).contains("not an Ant build file"));
    let outside = fixture.dir.path().join("outside.xml");
    std::fs::write(&outside, TOOLS).unwrap();
    assert!(fixture.fail("ant.buildFile.add", json!({ "path": outside })).contains("not in the project"));

    let filtered = fixture.run("ant.options.set", json!({ "filterTargets": true }));
    assert_eq!(filtered["filterTargets"], true);
    let removed = fixture.run("ant.buildFile.remove", json!({ "path": "." }));
    assert_eq!(removed["buildFiles"].as_array().unwrap().len(), 1);
    assert_eq!(removed["buildFiles"][0]["name"], "tools");
    assert!(std::fs::read_to_string(fixture.project.join("build.xml")).unwrap().contains("shop"), "the file stays");
    assert!(fixture.fail("ant.buildFile.remove", json!({ "path": "build.xml" })).contains("add it first"));
    assert!(fixture.fail("ant.target.run", json!({ "buildFile": "build.xml", "targets": "jar" })).contains("add it"));
    assert_eq!(fixture.run("ant.project.get", json!({}))["buildFiles"].as_array().unwrap().len(), 1, "kept");
}

#[test]
fn targets_run_in_the_run_tool_window_with_links_to_files() {
    let mut fixture = Fixture::new();
    let started = fixture.run("ant.target.run", json!({ "targets": "compile jar", "waitMs": 20000 }));
    assert_eq!(started["summary"]["exitCode"], 0);
    let (output, _) = fixture.output(&started["runId"]);
    assert!(output.contains("ant -buildfile build.xml compile jar"), "{output}");
    assert!(output.contains(&format!("in {}", fixture.project.canonicalize().unwrap().display())), "{output}");
    assert!(output.contains(&format!("java {}", fixture.jdk.display())), "{output}");

    let started =
        fixture.run("ant.target.run", json!({ "buildFile": "build.xml", "targets": "fail", "waitMs": 20000 }));
    assert_eq!(started["summary"]["exitCode"], 1);
    let (output, links) = fixture.output(&started["runId"]);
    assert!(output.contains("BUILD FAILED"), "{output}");
    assert!(
        links
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link["line"] == 20 && link["file"].as_str().is_some_and(|file| file.ends_with("build.xml"))),
        "{links}"
    );
    assert!(
        links
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link["line"] == 3 && link["file"].as_str().is_some_and(|file| file.ends_with("src/A.java"))),
        "the compiler's message links to the source: {links}"
    );

    let started = fixture.run("ant.target.run", json!({ "waitMs": 20000 }));
    let (output, _) = fixture.output(&started["runId"]);
    assert!(output.contains("ant -buildfile build.xml\n"), "no target: the default one: {output}");
    let error = fixture.fail("ant.target.run", json!({ "targets": "deploy" }));
    assert!(error.contains("no target deploy"), "{error}");
}

#[test]
fn go_to_declaration_on_a_target_name_goes_to_the_target() {
    let mut fixture = Fixture::new();
    let path = fixture.project.join("build.xml");
    let at = BUILD.find("depends=\"init, compile\"").unwrap() + 16;
    let found = fixture.run("navigate.declaration.goTo", json!({ "path": path, "offset": at }));
    assert_eq!(found["navigated"], true, "{found}");
    let document = fixture.session.state::<crate::Workbench>().and_then(crate::Workbench::active_document).unwrap();
    assert_eq!(document.path(), path);
    let selection = document.selections().primary();
    let compile = BUILD.find("name=\"compile\"").unwrap() + 6;
    assert_eq!((selection.anchor.min(selection.head), selection.anchor.max(selection.head)), (compile, compile + 7));

    let calls = fixture.project.join("tools/calls.xml");
    let text = "<project name=\"calls\"><target name=\"a\"><antcall target=\"gone\"/></target></project>";
    std::fs::write(&calls, text).unwrap();
    let at = text.find("target=\"gone\"").unwrap() + 9;
    let missing = fixture.run("navigate.declaration.goTo", json!({ "path": calls, "offset": at }));
    assert!(missing["error"].as_str().unwrap().contains("no target gone"), "{missing}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_download_that_does_not_match_its_checksum_installs_nothing() {
    let fixture = Fixture::new();
    let folder = fixture.dir.path().join("data/tools/ant").join(DOWNLOAD_VERSION);
    let error = install(b"not ant", &folder, "https://example.org/ant.tar.gz").unwrap_err();
    assert!(error.contains("SHA-512"), "{error}");
    assert!(!folder.exists());
    assert!(
        executable(&fixture.env, &[fixture.dir.path().join("nowhere")]).is_none_or(|found| !found.starts_with(&folder))
    );
}

#[test]
fn a_person_runs_no_target_of_a_project_not_trusted() {
    let mut fixture = Fixture::new();
    fixture.run("project.trust.revoke", json!({}));
    let error = fixture.registry.dispatch(&mut fixture.session, "ant.target.run", json!({ "targets": "compile" }));
    match error {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{error:?}")
        }
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn an_agent_does_not_choose_what_ant_runs_with() {
    let mut fixture = Fixture::new();
    for params in [
        json!({ "path": "build.xml", "jdk": "/tmp/elsewhere" }),
        json!({ "path": "build.xml", "classpath": ["/tmp/elsewhere.jar"] }),
        json!({ "path": "build.xml", "properties": [] }),
    ] {
        match zen_core::macros::as_agent(&mut fixture.session, |session| {
            fixture.registry.dispatch(session, "ant.buildFile.configure", params.clone())
        }) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::AGENT_NOT_ALLOWED), "{params}: {error:?}")
            }
            other => panic!("{params}: not refused: {other:?}"),
        }
    }
    let memory = zen_core::macros::as_agent(&mut fixture.session, |session| {
        fixture.registry.dispatch(session, "ant.buildFile.configure", json!({ "path": "build.xml", "maxMemory": 512 }))
    });
    assert!(memory.is_ok(), "{memory:?}");
}

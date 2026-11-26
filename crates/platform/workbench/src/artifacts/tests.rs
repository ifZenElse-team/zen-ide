use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_project::model::ModelEnv;

use super::jar::{Contents, class_path_of, manifest, read_manifest};
use super::*;
use crate::ide_settings::zip::{Archive, Limits, ZipWriter};
use crate::jvm_libraries::{Declared, JvmLibraries, Places};

const POM: &str = "<project><groupId>demo</groupId><artifactId>shop</artifactId><version>1</version></project>";

const APP: &str = "package shop;\n\npublic class App {\n    public static void main(String[] args) {}\n}\n";

struct Fixture {
    registry: Registry,
    session: Session,
    root: PathBuf,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("shop");
        std::fs::create_dir_all(&root).unwrap();
        for (path, text) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        let home = dir.path().join("home");
        let env = BuildEnv::default();
        env.set_home(Some(home.clone()));
        env.set_folders(Some(Vec::new()));
        env.set_jdk(Some(Arc::new(|_: &Path| None)));
        let places = Places {
            maven: home.join(".m2/repository"),
            gradle: home.join(".gradle/files"),
            cache: dir.path().join("cache"),
        };
        let provider = JvmLibraries::with_places(env.clone(), places, vec![Box::new(Declared)]);
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(env);
        session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(provider)], ModelEnv::default());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, &root);
        Self { registry, session, root, dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn fails(&mut self, command: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err().to_string()
    }
}

fn library(path: &Path, files: &[(&str, &[u8])]) {
    let mut writer = ZipWriter::new(1_792_000_000);
    for (name, data) in files {
        writer.add(name, data).unwrap();
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, writer.finish()).unwrap();
}

fn entries(jar: &Path) -> Vec<(String, Vec<u8>)> {
    let archive = Archive::parse(std::fs::read(jar).unwrap(), Limits::SETTINGS).unwrap();
    archive.entries().iter().map(|entry| (entry.name.clone(), archive.read(entry).unwrap())).collect()
}

fn names(jar: &Path) -> Vec<String> {
    entries(jar).into_iter().map(|(name, _)| name).collect()
}

#[test]
fn the_manifest_wraps_at_72_bytes_and_names_the_libraries_next_to_the_jar() {
    let libraries: Vec<String> = (0..6).map(|index| format!("a-long-library-name-{index} with space.jar")).collect();
    let text = manifest("shop.App", &libraries);
    assert!(text.starts_with("Manifest-Version: 1.0\r\nCreated-By: Zen IDE\r\nMain-Class: shop.App\r\n"));
    assert!(text.ends_with("\r\n\r\n"));
    assert!(text.split("\r\n").all(|line| line.len() <= 72), "{text}");
    assert!(text.contains("with%20space.jar"));
    assert_eq!(class_path_of(&text), libraries);
    assert!(!manifest("", &[]).contains("Main-Class"), "a library jar has no Main-Class");
}

#[test]
fn a_jar_takes_the_classes_and_extracts_the_libraries_once() {
    let dir = tempfile::tempdir().unwrap();
    let classes = dir.path().join("classes");
    std::fs::create_dir_all(classes.join("shop")).unwrap();
    std::fs::create_dir_all(classes.join("META-INF/services")).unwrap();
    std::fs::write(classes.join("shop/App.class"), "app").unwrap();
    std::fs::write(classes.join("META-INF/MANIFEST.MF"), "Manifest-Version: 1.0\r\n").unwrap();
    std::fs::write(classes.join("META-INF/services/demo.Plugin"), "shop.Mine\n").unwrap();
    let first = dir.path().join("repo/first.jar");
    library(
        &first,
        &[
            ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\n"),
            ("META-INF/FIRST.SF", b"signed"),
            ("META-INF/FIRST.RSA", b"key"),
            ("META-INF/services/demo.Plugin", b"lib.Theirs"),
            ("module-info.class", b"module"),
            ("lib/Util.class", b"first"),
        ],
    );
    let second = dir.path().join("repo/second.jar");
    library(&second, &[("lib/Util.class", b"second"), ("lib/Other.class", b"other")]);
    let jar = dir.path().join("out/shop.jar");
    let contents = Contents { folders: vec![classes], libraries: vec![first, second] };
    let written = jar::write(&jar, "shop.App", &contents, true, 1_792_000_000).unwrap();
    assert!(written.copied.is_empty());
    let found = entries(&jar);
    let listed: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        listed,
        [
            "META-INF/MANIFEST.MF",
            "shop/App.class",
            "lib/Util.class",
            "lib/Other.class",
            "META-INF/services/demo.Plugin"
        ]
    );
    assert_eq!(written.entries, found.len());
    let text =
        |name: &str| String::from_utf8(found.iter().find(|(entry, _)| entry == name).unwrap().1.clone()).unwrap();
    assert!(text("META-INF/MANIFEST.MF").contains("Main-Class: shop.App\r\n"));
    assert!(!text("META-INF/MANIFEST.MF").contains("Class-Path"));
    assert_eq!(text("lib/Util.class"), "first", "the first library wins");
    assert_eq!(text("META-INF/services/demo.Plugin"), "shop.Mine\nlib.Theirs", "service files are merged");
}

#[test]
fn libraries_next_to_the_jar_are_copied_and_cleaned_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let classes = dir.path().join("classes");
    std::fs::create_dir_all(&classes).unwrap();
    std::fs::write(classes.join("App.class"), "app").unwrap();
    let one = dir.path().join("a/util.jar");
    let two = dir.path().join("b/util.jar");
    library(&one, &[("a/A.class", b"a")]);
    library(&two, &[("b/B.class", b"b")]);
    let folder = dir.path().join("out");
    let jar = folder.join("shop.jar");
    let contents = Contents { folders: vec![classes], libraries: vec![one, two, dir.path().join("missing.jar")] };
    let written = jar::write(&jar, "App", &contents, false, 0).unwrap();
    assert_eq!(written.copied, [folder.join("util.jar"), folder.join("2-util.jar")]);
    assert_eq!(names(&jar), ["META-INF/MANIFEST.MF", "App.class"]);
    let text = read_manifest(&jar).unwrap();
    assert_eq!(class_path_of(&text), ["util.jar", "2-util.jar"]);
    std::fs::write(folder.join("notes.txt"), "mine").unwrap();
    let removed = jar::remove(&jar);
    assert_eq!(removed, [folder.join("util.jar"), folder.join("2-util.jar"), jar.clone()]);
    assert!(folder.join("notes.txt").is_file(), "only what the jar names is removed");
}

#[test]
fn classpath_files_read_in_both_spellings() {
    let sep = if cfg!(windows) { ";" } else { ":" };
    assert_eq!(
        jar::classpath_file(&format!("/a/b.jar{sep}/c/d\n")),
        [PathBuf::from("/a/b.jar"), PathBuf::from("/c/d")]
    );
    assert_eq!(
        jar::classpath_file(&format!("\"/a/x \\\"y\\\".jar{sep}/c\"\n")),
        [PathBuf::from("/a/x \"y\".jar"), PathBuf::from("/c")]
    );
}

#[test]
fn a_maven_project_offers_its_own_packaging_first() {
    let mut fixture =
        Fixture::new(&[("pom.xml", POM), ("src/main/java/shop/App.java", APP), ("src/main/resources/a.txt", "")]);
    let root = fixture.root.clone();
    let section: ArtifactsSection =
        serde_json::from_value(fixture.run("project.structure.artifacts", json!({}))).unwrap();
    assert!(section.artifacts.is_empty());
    let offered: Vec<(&str, ArtifactKind, &str, &str)> = section
        .suggestions
        .iter()
        .map(|artifact| (artifact.name.as_str(), artifact.kind, artifact.task.as_str(), artifact.main_class.as_str()))
        .collect();
    assert_eq!(
        offered,
        [("shop:mvn package", ArtifactKind::Build, "package", ""), ("shop:jar", ArtifactKind::Jar, "", "shop.App")]
    );
    assert_eq!(
        section.jars,
        [ArtifactFile { name: "shop:jar".into(), path: root.join("out/artifacts/shop/shop.jar") }]
    );
    assert_eq!(buildable(&section), section.suggestions.as_slice(), "offered while none is kept");
}

#[test]
fn a_gradle_spring_project_offers_boot_jar() {
    let mut fixture = Fixture::new(&[
        ("settings.gradle.kts", "rootProject.name = \"shop\"\n"),
        ("build.gradle.kts", "plugins { id(\"org.springframework.boot\") version \"3.3.0\"\n java }\n"),
        ("src/main/java/shop/App.java", APP),
    ]);
    let section: ArtifactsSection =
        serde_json::from_value(fixture.run("project.structure.artifacts", json!({}))).unwrap();
    let first = &section.suggestions[0];
    assert_eq!(
        (first.name.as_str(), first.kind, first.task.as_str()),
        ("shop:gradle bootJar", ArtifactKind::Build, "bootJar")
    );
    assert_eq!(section.suggestions[1].kind, ArtifactKind::Jar);
}

#[test]
fn artifacts_are_kept_in_the_project_and_changed_by_name() {
    let mut fixture = Fixture::new(&[("pom.xml", POM), ("src/main/java/shop/App.java", APP)]);
    let root = fixture.root.clone();
    let artifact = json!({ "name": " shop:jar ", "modules": ["shop"], "mainClass": "shop.App", "libraries": "nextTo" });
    let section = fixture.run("project.artifact.save", json!({ "artifact": artifact }));
    assert_eq!(section["artifacts"][0]["name"], "shop:jar");
    assert_eq!(section["artifacts"][0]["libraries"], "nextTo");
    assert_eq!(saved(&root)[0].main_class, "shop.App");
    assert!(std::fs::read_to_string(root.join(FILE)).unwrap().contains("\"mainClass\": \"shop.App\""));

    let build = json!({ "name": "package", "kind": "build", "modules": ["shop"], "task": "package" });
    fixture.run("project.artifact.save", json!({ "artifact": build }));
    let renamed = json!({ "name": "app", "modules": ["shop"], "mainClass": "shop.App" });
    fixture.run("project.artifact.save", json!({ "artifact": renamed, "previous": "shop:jar" }));
    let names: Vec<String> = saved(&root).into_iter().map(|artifact| artifact.name).collect();
    assert_eq!(names, ["app", "package"], "a renamed artifact keeps its place");

    let taken = json!({ "name": "package", "modules": ["shop"] });
    let error = fixture.fails("project.artifact.save", json!({ "artifact": taken, "previous": "app" }));
    assert!(error.contains("there already"), "{error}");
    let unknown = json!({ "name": "x", "modules": ["nope"] });
    assert!(fixture.fails("project.artifact.save", json!({ "artifact": unknown })).contains("no module nope"));
    assert!(fixture.fails("project.artifact.save", json!({ "artifact": { "name": " " } })).contains("needs a name"));

    fixture.run("project.artifact.delete", json!({ "name": "package" }));
    assert_eq!(saved(&root).len(), 1);
    assert!(fixture.fails("project.artifact.delete", json!({ "name": "package" })).contains("no artifact package"));
    let section: ArtifactsSection =
        serde_json::from_value(fixture.run("project.structure.artifacts", json!({}))).unwrap();
    assert_eq!(buildable(&section), section.artifacts.as_slice(), "kept artifacts replace the suggestions");
}

fn plan_of(root: &Path, model: &ProjectModel, artifacts: &[Artifact], action: Action, scratch: &Path) -> Plan {
    plan(root, model, artifacts, action, &BuildEnv::default(), scratch).unwrap()
}

#[test]
fn the_plan_compiles_each_module_once_through_its_build() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("shop");
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(root.join("settings.gradle"), "include 'app'\n").unwrap();
    std::fs::write(root.join("app/build.gradle"), "plugins { id 'java' }\n").unwrap();
    std::fs::write(root.join(if cfg!(windows) { "gradlew.cmd" } else { "gradlew" }), "").unwrap();
    let app = Module {
        name: "app".into(),
        root: root.join("app"),
        details: json!({ "build": "Gradle" }),
        ..Module::default()
    };
    let model = ProjectModel { modules: vec![app], ..ProjectModel::default() };
    let jar = Artifact {
        name: "app:jar".into(),
        modules: vec!["app".into()],
        main_class: "App".into(),
        ..Artifact::default()
    };
    let next_to = Artifact { name: "thin".into(), libraries: LibraryPlacement::NextTo, ..jar.clone() };
    let build =
        Artifact { name: "b".into(), kind: ArtifactKind::Build, modules: vec!["app".into()], ..Artifact::default() };
    let scratch = dir.path().join("scratch");
    let artifacts = [jar.clone(), next_to, build];
    let built = plan_of(&root, &model, &artifacts, Action::Build, &scratch);
    let shown: Vec<&str> = built.lines.iter().map(|line| line.display.as_str()).collect();
    let init = scratch.join("zen-classpath.gradle").display().to_string();
    let classpath = scratch.join("classpath-0.args").display().to_string();
    let app_folder = root.join("app").display().to_string();
    let wrapper = if cfg!(windows) { "gradlew.cmd" } else { "gradlew" };
    assert_eq!(
        shown,
        [
            format!("{wrapper} --init-script {init} -PzenClasspathOut={classpath} -p {app_folder} zenClasspath"),
            format!("{wrapper} -p {app_folder} jar"),
        ]
    );
    assert_eq!(built.packs.len(), 2, "two jars from one compilation");
    assert_eq!((built.packs[0].extract, built.packs[1].extract), (true, false));
    assert!(built.clean.is_empty());

    let rebuild = plan_of(&root, &model, std::slice::from_ref(&jar), Action::Rebuild, &scratch);
    assert!(rebuild.lines[0].args.contains(&"clean".to_owned()));
    assert_eq!(rebuild.clean, [root.join("out/artifacts/app/app.jar")]);
    let clean = plan_of(&root, &model, &artifacts, Action::Clean, &scratch);
    assert!(clean.packs.is_empty());
    let shown: Vec<&str> = clean.lines.iter().map(|line| line.display.as_str()).collect();
    assert_eq!(shown, [format!("{wrapper} -p {app_folder} clean")]);

    let plain = dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let artifact = Artifact { name: "x".into(), ..Artifact::default() };
    let error = plan(&plain, &ProjectModel::default(), &[artifact], Action::Build, &BuildEnv::default(), &scratch);
    assert!(error.is_err(), "a project without a build needs a JDK and sources");
}

#[cfg(unix)]
#[test]
fn building_a_maven_jar_compiles_then_writes_it_and_clean_removes_it() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new(&[("pom.xml", POM), ("src/main/java/shop/App.java", APP)]);
    let root = fixture.root.clone();
    let lib = fixture.dir.path().join("repo/lib.jar");
    library(&lib, &[("lib/Util.class", b"util")]);
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -Dmdep.outputFile=*) out=\"${{a#-Dmdep.outputFile=}}\";; esac; done\n\
         echo \"compiling $*\"\nmkdir -p target/classes/shop\nprintf app > target/classes/shop/App.class\n\
         printf '%s' '{}' > \"$out\"\n",
        lib.display()
    );
    let wrapper = root.join("mvnw");
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();

    let answer = fixture.run("build.artifacts.build", json!({ "names": ["shop:jar"], "waitMs": 60_000 }));
    assert_eq!(
        (&answer["summary"]["state"], &answer["summary"]["exitCode"]),
        (&json!("finished"), &json!(0)),
        "{answer}"
    );
    let jar = root.join("out/artifacts/shop/shop.jar");
    assert_eq!(answer["jars"], json!([jar]));
    assert_eq!(names(&jar), ["META-INF/MANIFEST.MF", "shop/App.class", "lib/Util.class"]);
    assert!(read_manifest(&jar).unwrap().contains("Main-Class: shop.App"));
    let run_id = answer["runId"].as_u64().unwrap();
    let console = fixture.run("run.output.get", json!({ "runId": run_id }));
    assert!(console.to_string().contains("Built shop:jar"), "{console}");

    let cleaned = fixture.run("build.artifacts.clean", json!({ "names": ["shop:jar"] }));
    assert_eq!(cleaned["removed"], json!([jar]));
    assert!(cleaned.get("runId").is_none(), "a jar is cleaned at once");
    assert!(!jar.exists());

    std::fs::write(&wrapper, "#!/bin/sh\necho broken\nexit 3\n").unwrap();
    let answer = fixture.run("build.artifacts.build", json!({ "waitMs": 60_000 }));
    assert_eq!(answer["summary"]["exitCode"], 3, "{answer}");
    assert!(!jar.exists(), "nothing is packed after a failed compilation");
    assert!(fixture.fails("build.artifacts.build", json!({ "names": ["nope"] })).contains("no artifact nope"));
}

#[cfg(unix)]
#[test]
fn build_artifacts_before_launch_builds_the_jar_first_and_a_failure_cancels_the_start() {
    use std::os::unix::fs::PermissionsExt;
    use zen_run::fake::FakeProvider;
    let mut fixture = Fixture::new(&[("pom.xml", POM), ("src/main/java/shop/App.java", APP)]);
    let root = fixture.root.clone();
    let wrapper = root.join("mvnw");
    let script = "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -Dmdep.outputFile=*) out=\"${a#-Dmdep.outputFile=}\";; esac; \
                  done\necho compiling\nmkdir -p target/classes/shop\nprintf app > target/classes/shop/App.class\n\
                  printf '' > \"$out\"\n";
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fake = FakeProvider::new();
    fake.set_script(&[(zen_run::Stream::Stdout, "the program ran")], 0);
    fixture.session.state_mut::<Runs>().set_providers(vec![Arc::new(fake)], zen_run::RunEnv::default());
    crate::before_launch::register_type(&mut fixture.session, task_type());

    let types = fixture.run("run.beforeLaunch.types", json!({}));
    let listed = types["types"].as_array().unwrap().iter().find(|kind| kind["id"] == TASK).unwrap().clone();
    assert_eq!(listed["name"], "Build Artifacts");
    assert_eq!(listed["fields"][0]["kind"]["type"], "artifacts");

    let mut configuration = serde_json::to_value(FakeProvider::configuration("app")).unwrap();
    configuration["common"]["beforeLaunch"] = json!([{ "type": TASK, "artifacts": ["shop:jar"] }]);
    let created = fixture.run("run.configuration.create", json!({ "configuration": configuration }));
    let id = created["id"].as_str().unwrap().to_owned();
    let tasks = fixture.run("run.beforeLaunch.list", json!({ "configurationId": id }));
    assert_eq!(tasks["tasks"][0]["label"], "Build Artifacts “shop:jar”");

    let jar = root.join("out/artifacts/shop/shop.jar");
    let started = fixture.run("run.process.start", json!({ "configurationId": id, "waitMs": 60_000 }));
    assert_eq!(started["summary"]["state"], "finished", "{started}");
    assert!(jar.is_file(), "the jar is written before the configuration starts");
    let console = fixture.run("run.output.get", json!({ "runId": started["runId"] })).to_string();
    let done = console.find("#before-launch Build Artifacts “shop:jar” finished").expect(&console);
    assert!(done < console.find("the program ran").expect(&console), "{console}");

    std::fs::write(&wrapper, "#!/bin/sh\necho broken\nexit 3\n").unwrap();
    let started = fixture.run("run.process.start", json!({ "configurationId": id, "waitMs": 60_000 }));
    assert_eq!(started["summary"]["state"], "beforeLaunchFailed", "{started}");
    let console = fixture.run("run.output.get", json!({ "runId": started["runId"] })).to_string();
    assert!(!console.contains("the program ran"), "{console}");

    let task = json!({ "type": TASK, "artifacts": [] });
    fixture.run("run.beforeLaunch.update", json!({ "configurationId": id, "index": 0, "task": task }));
    let started = fixture.run("run.process.start", json!({ "configurationId": id, "waitMs": 60_000 }));
    assert_eq!(started["summary"]["state"], "beforeLaunchFailed");
    assert!(started["summary"]["error"].as_str().unwrap().contains("Choose the artifacts"), "{started}");
    let task = json!({ "type": TASK, "artifacts": ["nope"] });
    fixture.run("run.beforeLaunch.update", json!({ "configurationId": id, "index": 0, "task": task }));
    let started = fixture.run("run.process.start", json!({ "configurationId": id, "waitMs": 60_000 }));
    assert!(started["summary"]["error"].as_str().unwrap().contains("no artifact nope"), "{started}");
}

#[cfg(unix)]
#[test]
fn build_triggers_run_before_and_after_the_compilation_in_the_builds_run() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new(&[("pom.xml", POM), ("src/main/java/shop/App.java", APP)]);
    let root = fixture.root.clone();
    let env = fixture.session.state::<BuildEnv>().cloned().unwrap();
    fixture
        .session
        .state_mut::<Runs>()
        .set_providers(vec![Arc::new(crate::maven::MavenRun::new(env))], zen_run::RunEnv::default());
    let script = "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -Dmdep.outputFile=*) out=\"${a#-Dmdep.outputFile=}\";; esac; done\n\
         echo \"maven $*\"\nmkdir -p target/classes/shop\nprintf app > target/classes/shop/App.class\n\
         [ -n \"$out\" ] && printf '' > \"$out\"\nexit 0\n";
    let wrapper = root.join("mvnw");
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (when, goals) in [("beforeBuild", "generate-sources"), ("afterBuild", "verify"), ("beforeRebuild", "validate")]
    {
        let task = json!({ "type": "maven.goal", "pom": "pom.xml", "goals": goals });
        fixture.run("build.triggers.set", json!({ "when": when, "task": task }));
    }

    let answer = fixture.run("build.artifacts.build", json!({ "names": ["shop:jar"], "waitMs": 60_000 }));
    assert_eq!(answer["summary"]["exitCode"], 0, "{answer}");
    let console = fixture.run("run.output.get", json!({ "runId": answer["runId"] })).to_string();
    let at = |text: &str| console.find(text).unwrap_or_else(|| panic!("{text} in {console}"));
    assert!(at("maven generate-sources") < at("maven -B compile"), "{console}");
    assert!(at("maven -B compile") < at("maven verify"), "{console}");
    assert!(!console.contains("maven validate"), "Before Rebuild is not a build's: {console}");
}

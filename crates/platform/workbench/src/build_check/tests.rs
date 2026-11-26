use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use crate::build_tools::BuildEnv;
use crate::jdk::{Jdks, Sources, Store};

const JAVA_17: &str =
    "<project><artifactId>shop</artifactId><properties><java.version>17</java.version></properties></project>";

fn fake_jdk(home: &Path, version: &str) {
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("bin/java"), "").unwrap();
    std::fs::write(home.join("bin/javac"), "").unwrap();
    std::fs::write(home.join("release"), format!("JAVA_VERSION=\"{version}\"\n")).unwrap();
}

fn fake_tool(path: &Path) {
    std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
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
    bin: PathBuf,
    jvm: PathBuf,
    env: BuildEnv,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new(file: &str, text: &str) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(file), text).unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let jvm = dir.path().join("jvm");
        fake_jdk(&jvm.join("a-21"), "21.0.5");
        let env = BuildEnv::default();
        env.set_folders(Some(vec![bin.clone()]));
        env.set_home(Some(dir.path().join("home")));
        env.set_data_dir(Some(dir.path().join("data")));
        let store = Store::with_sources(Sources { system_folders: vec![jvm.clone()], ..Sources::default() });
        let jdks = Jdks::new(store);
        let lookup = jdks.clone();
        env.set_jdk(Some(Arc::new(move |root: &Path| lookup.store().project(root).map(|project| project.jdk.home))));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(jdks);
        session.set_state(env.clone());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        Self { registry, session, project, bin, jvm, env, _dir: dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn trust(&self) {
        crate::trust::trust_in_tests(&self.env, &self.project);
    }

    fn refresh_jdks(&mut self) {
        self.run("toolchain.jdk.refresh", json!({}));
    }
}

#[test]
fn a_maven_project_gives_its_reasons_in_order_and_each_goes_without_a_restart() {
    let mut fixture = Fixture::new("pom.xml", JAVA_17);
    let blocked = fixture.run("maven.project.get", json!({}))["blocked"].clone();
    assert_eq!(blocked["kind"], "toolNotFound", "{blocked}");
    assert_eq!(blocked["fixes"][0]["command"], "maven.distribution.download");
    assert_eq!(blocked["fixes"][0]["reload"], false, "the download runs in the background");

    fake_tool(&fixture.bin.join("mvn"));
    let blocked = fixture.run("maven.project.get", json!({}))["blocked"].clone();
    assert_eq!(blocked["kind"], "notTrusted", "{blocked}");
    assert_eq!(blocked["fixes"][0]["command"], "project.trust.grant");

    fixture.trust();
    let project = fixture.run("maven.project.get", json!({}));
    let blocked = project["blocked"].clone();
    assert_eq!(blocked["kind"], "jdkMismatch", "{blocked}");
    assert_eq!(blocked["text"], "The build asks for Java 17, and it runs on JDK 21.");
    assert_eq!(blocked["fixes"][0]["command"], "toolchain.jdk.download");
    assert_eq!(blocked["fixes"][0]["params"], json!({ "version": 17 }));
    assert_eq!(blocked["fixes"][1]["command"], "view.settings.open");
    assert_eq!(project["jdk"]["wanted"], 17);
    assert_eq!(project["jdk"]["major"], 21);
    assert_eq!(project["jdk"]["wantedBy"], json!(fixture.project.join("pom.xml")));

    fake_jdk(&fixture.jvm.join("b-17"), "17.0.12");
    fixture.refresh_jdks();
    let project = fixture.run("maven.project.get", json!({}));
    assert!(project.get("blocked").is_none(), "{project}");
    assert_eq!(project["jdk"]["major"], 17);
}

#[test]
fn a_gradle_build_says_the_same_with_its_own_download() {
    let mut fixture = Fixture::new("build.gradle", "sourceCompatibility = 17\n");
    let blocked = fixture.run("gradle.project.get", json!({}))["blocked"].clone();
    assert_eq!(blocked["kind"], "toolNotFound");
    assert_eq!(blocked["fixes"][0]["command"], "gradle.distribution.download");
    fake_tool(&fixture.bin.join("gradle"));
    assert_eq!(fixture.run("gradle.project.get", json!({}))["blocked"]["kind"], "notTrusted");
    fixture.trust();
    assert_eq!(fixture.run("gradle.project.get", json!({}))["blocked"]["kind"], "jdkMismatch");
}

#[test]
fn a_gradle_toolchain_finds_its_own_jdk_and_is_no_mismatch() {
    let mut fixture = Fixture::new("build.gradle.kts", "kotlin { jvmToolchain(17) }\n");
    fake_tool(&fixture.bin.join("gradle"));
    fixture.trust();
    let build = fixture.run("gradle.project.get", json!({}));
    assert!(build.get("blocked").is_none(), "{build}");
    assert_eq!(build["jdk"]["differs"], false);
}

#[test]
fn a_newer_jdk_the_person_chose_is_reported_without_a_warning_and_an_older_one_warns() {
    let mut fixture = Fixture::new("pom.xml", JAVA_17);
    fake_tool(&fixture.bin.join("mvn"));
    fixture.trust();
    fake_jdk(&fixture.jvm.join("b-11"), "11.0.25");
    fixture.refresh_jdks();
    fixture.run("toolchain.jdk.choose", json!({ "home": fixture.jvm.join("a-21") }));
    let project = fixture.run("maven.project.get", json!({}));
    assert!(project.get("blocked").is_none(), "{project}");
    assert_eq!((project["jdk"]["differs"].clone(), project["jdk"]["warned"].clone()), (json!(true), json!(false)));

    fixture.run("toolchain.jdk.choose", json!({ "home": fixture.jvm.join("b-11") }));
    let blocked = fixture.run("maven.project.get", json!({}))["blocked"].clone();
    assert_eq!(blocked["text"], "The build asks for Java 17, and it runs on JDK 11.");
}

#[test]
fn a_chosen_jdk_that_differs_is_offered_the_installed_match() {
    let mut fixture = Fixture::new("pom.xml", JAVA_17);
    fake_tool(&fixture.bin.join("mvn"));
    fixture.trust();
    fake_jdk(&fixture.jvm.join("b-11"), "11.0.25");
    fake_jdk(&fixture.jvm.join("c-17"), "17.0.12");
    fixture.refresh_jdks();
    fixture.run("toolchain.jdk.choose", json!({ "home": fixture.jvm.join("b-11") }));
    let blocked = fixture.run("maven.project.get", json!({}))["blocked"].clone();
    assert_eq!(blocked["fixes"][0]["title"], "Use JDK 17");
    assert_eq!(blocked["fixes"][0]["command"], "toolchain.jdk.choose");
    assert_eq!(blocked["fixes"][0]["params"]["home"], json!(fixture.jvm.join("c-17")));
    let fix = blocked["fixes"][0].clone();
    fixture.run("toolchain.jdk.choose", fix["params"].clone());
    let project = fixture.run("maven.project.get", json!({}));
    assert!(project.get("blocked").is_none(), "{project}");
}

#[test]
fn the_same_reason_shows_in_sync_with_its_fix() {
    let mut fixture = Fixture::new("pom.xml", JAVA_17);
    fake_tool(&fixture.bin.join("mvn"));
    let refused =
        fixture.registry.dispatch(&mut fixture.session, "maven.project.reload", json!({ "dependencies": false }));
    assert!(refused.is_err(), "the build does not load: {refused:?}");
    let sync = fixture.run("build.sync.get", json!({}))["syncs"][0].clone();
    let blocked = fixture.run("maven.project.get", json!({}))["blocked"].clone();
    let message = &sync["steps"][1]["messages"][0];
    assert_eq!(sync["steps"][1]["title"], "Check the project");
    assert_eq!(message["severity"], "warning");
    assert_eq!(message["text"], blocked["text"]);
    assert_eq!(message["fix"]["command"], "project.trust.grant");
}

#[test]
fn without_gradle_the_sync_says_what_the_banner_says_and_offers_the_download() {
    let mut fixture = Fixture::new("build.gradle", "plugins { id 'java' }\n");
    fixture.trust();
    let error = fixture.registry.dispatch(&mut fixture.session, "gradle.project.reload", json!({})).unwrap_err();
    assert!(!error.text().contains("gradle.project.reload"), "{}", error.text());
    let blocked = fixture.run("gradle.project.get", json!({}))["blocked"].clone();
    let sync = fixture.run("build.sync.get", json!({}))["syncs"][0].clone();
    let steps = sync["steps"].as_array().unwrap();
    let message = &steps.last().unwrap()["messages"][0];
    assert_eq!(message["text"], blocked["text"]);
    assert_eq!(message["fix"]["command"], "gradle.distribution.download");
}

#[test]
fn an_agent_can_see_the_trust_fix_and_cannot_use_it() {
    let mut fixture = Fixture::new("pom.xml", JAVA_17);
    fake_tool(&fixture.bin.join("mvn"));
    let blocked = fixture.run("maven.project.get", json!({}))["blocked"].clone();
    let fix = blocked["fixes"][0].clone();
    let registry = &fixture.registry;
    let refused = zen_core::macros::as_agent(&mut fixture.session, |session| {
        registry.dispatch(session, fix["command"].as_str().unwrap(), fix["params"].clone())
    })
    .unwrap_err();
    assert!(refused.text().contains("Only the person trusts"), "{refused}");
    assert_eq!(fixture.run("maven.project.get", json!({}))["blocked"]["kind"], "notTrusted");
}

#[test]
fn the_texts_people_read_name_no_command() {
    let registry = {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        registry
    };
    let texts = crate::LOCALES
        .iter()
        .filter(|(language, _)| ["en", "ru"].contains(language))
        .map(|(_, text)| *text)
        .collect::<Vec<_>>()
        .join("\n");
    for id in ["maven.distribution.download", "gradle.distribution.download", "ant.distribution.download"] {
        assert!(registry.info(id).is_some(), "{id}");
        assert!(!texts.contains(id), "{id} is in a message");
    }
}

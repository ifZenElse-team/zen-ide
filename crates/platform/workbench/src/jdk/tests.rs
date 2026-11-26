use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::{Jdks, ProjectJdk, Sources, Store, detect};

fn fake_jdk(home: &Path, version: &str, vendor: &str) {
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("bin/java"), "").unwrap();
    std::fs::write(home.join("bin/javac"), "").unwrap();
    std::fs::write(home.join("release"), format!("IMPLEMENTOR=\"{vendor}\"\nJAVA_VERSION=\"{version}\"\n")).unwrap();
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: PathBuf,
    data: PathBuf,
    jvm: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("pom.xml"), "<project/>").unwrap();
        let jvm = dir.path().join("jvm");
        fake_jdk(&jvm.join("zulu-17.jdk/Contents/Home"), "17.0.12", "Azul Systems, Inc.");
        fake_jdk(&jvm.join("temurin-21.jdk/Contents/Home"), "21.0.5", "Eclipse Adoptium");
        let data = dir.path().join("data");
        let mut store = Store::with_sources(Sources { system_folders: vec![jvm.clone()], ..Sources::default() });
        store.set_data_dir(data.clone());
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Jdks::new(store));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        Self { registry, session, project, data, jvm, _dir: dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn project(&mut self) -> ProjectJdk {
        serde_json::from_value(self.run("toolchain.jdk.project", json!({}))).unwrap()
    }
}

const BOOT: &str =
    "<project><artifactId>shop</artifactId><properties><java.version>17</java.version></properties></project>";

#[test]
fn a_spring_boot_maven_project_takes_the_installed_jdk_of_its_java_version() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.project.join("pom.xml"), BOOT).unwrap();
    let project = fixture.project();
    assert_eq!(
        (project.jdk.major, project.chosen, project.build_level, project.language_level),
        (17, false, Some(17), 17),
        "the newest would be 21"
    );
}

#[test]
fn a_gradle_project_takes_the_installed_jdk_of_its_toolchain() {
    let mut fixture = Fixture::new();
    std::fs::remove_file(fixture.project.join("pom.xml")).unwrap();
    std::fs::write(
        fixture.project.join("build.gradle.kts"),
        "java { toolchain { languageVersion = JavaLanguageVersion.of(17) } }",
    )
    .unwrap();
    let project = fixture.project();
    assert_eq!((project.jdk.major, project.chosen, project.build_level), (17, false, Some(17)));
}

#[test]
fn a_toolchain_gradle_cannot_run_on_is_left_to_gradle() {
    let mut fixture = Fixture::new();
    std::fs::remove_file(fixture.project.join("pom.xml")).unwrap();
    fake_jdk(&fixture.jvm.join("temurin-11.jdk/Contents/Home"), "11.0.25", "Eclipse Adoptium");
    fixture.run("toolchain.jdk.refresh", json!({}));
    std::fs::write(
        fixture.project.join("build.gradle"),
        "java { toolchain { languageVersion = JavaLanguageVersion.of(11) } }",
    )
    .unwrap();
    assert_eq!(fixture.project().jdk.major, 11, "older Gradle runs on it");
    std::fs::create_dir_all(fixture.project.join("gradle/wrapper")).unwrap();
    std::fs::write(
        fixture.project.join("gradle/wrapper/gradle-wrapper.properties"),
        "distributionUrl=https\\://services.gradle.org/distributions/gradle-9.1.0-bin.zip\n",
    )
    .unwrap();
    fixture.run("toolchain.jdk.refresh", json!({}));
    let project = fixture.project();
    assert_eq!((project.jdk.major, project.build_level), (21, None), "Gradle 9 needs Java 17 to run");
}

#[test]
fn a_chosen_jdk_is_never_replaced_by_the_builds_level() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.project.join("pom.xml"), BOOT).unwrap();
    let chosen = fixture.jvm.join("temurin-21.jdk/Contents/Home");
    fixture.run("toolchain.jdk.choose", json!({ "home": chosen }));
    let project = fixture.project();
    assert_eq!((project.jdk.major, project.chosen, project.build_level), (21, true, Some(17)));
    assert_eq!(project.language_level, 17, "compiled for the level the build asks for");
}

#[test]
fn a_jre_of_the_builds_level_does_not_match() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.project.join("pom.xml"), BOOT).unwrap();
    std::fs::remove_file(fixture.jvm.join("zulu-17.jdk/Contents/Home/bin/javac")).unwrap();
    fixture.run("toolchain.jdk.refresh", json!({}));
    assert_eq!(fixture.project().jdk.major, 21);
}

#[test]
fn the_level_is_read_again_when_the_build_changes() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.project().jdk.major, 21);
    std::fs::write(fixture.project.join("pom.xml"), BOOT).unwrap();
    assert_eq!(fixture.project().jdk.major, 17);
}

#[test]
fn the_highest_level_of_the_modules_decides() {
    let mut fixture = Fixture::new();
    std::fs::create_dir_all(fixture.project.join("core")).unwrap();
    std::fs::write(
        fixture.project.join("pom.xml"),
        "<project><artifactId>shop</artifactId><properties><maven.compiler.release>11</maven.compiler.release></properties><modules><module>core</module></modules></project>",
    )
    .unwrap();
    std::fs::write(fixture.project.join("core/pom.xml"), BOOT).unwrap();
    assert_eq!(fixture.project().build_level, Some(17));
}

fn no_match_notice(fixture: &mut Fixture) -> Value {
    let notices = fixture.run("ui.notifications.list", json!({}));
    notices["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|notice| notice["title"] == "This project asks for Java 11")
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn no_installed_match_offers_download_or_choose_once_and_downloads_nothing() {
    let mut fixture = Fixture::new();
    let project = fixture.project.clone();
    std::fs::write(
        project.join("pom.xml"),
        "<project><artifactId>shop</artifactId><properties><maven.compiler.release>11</maven.compiler.release></properties></project>",
    )
    .unwrap();
    fixture.run("file.project.open", json!({ "path": project }));
    let notice = no_match_notice(&mut fixture);
    let actions = notice["actions"].as_array().unwrap();
    assert_eq!(notice["body"], "No JDK 11 is on this computer, so the project uses JDK 21; its build may fail.");
    assert_eq!(actions[0]["label"], "Download JDK 11");
    assert_eq!(actions[0]["command"], "toolchain.jdk.download");
    assert_eq!(actions[0]["params"], json!({ "version": 11 }));
    assert_eq!(actions[1]["command"], "view.settings.open");
    assert_eq!(fixture.project().jdk.major, 21, "nothing was changed or downloaded");
    let list = fixture.run("toolchain.jdk.list", json!({}));
    assert!(list.get("downloading").is_none(), "{list}");

    fixture.run("file.project.open", json!({ "path": project }));
    let count = fixture.run("ui.notifications.list", json!({}))["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|notice| notice["title"] == "This project asks for Java 11")
        .count();
    assert_eq!(count, 1, "once a window");
}

#[test]
fn a_chosen_jdk_or_an_installed_match_asks_nothing() {
    let mut fixture = Fixture::new();
    let project = fixture.project.clone();
    std::fs::write(project.join("pom.xml"), BOOT).unwrap();
    fixture.run("file.project.open", json!({ "path": project }));
    assert!(
        fixture.run("ui.notifications.list", json!({}))["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .all(|notice| !notice["title"].as_str().unwrap_or_default().starts_with("This project asks"))
    );
    std::fs::write(
        project.join("pom.xml"),
        "<project><artifactId>shop</artifactId><properties><maven.compiler.release>11</maven.compiler.release></properties></project>",
    )
    .unwrap();
    let chosen = fixture.jvm.join("zulu-17.jdk/Contents/Home");
    fixture.run("toolchain.jdk.choose", json!({ "home": chosen }));
    fixture.run("toolchain.jdk.refresh", json!({}));
    fixture.run("file.project.open", json!({ "path": project }));
    assert_eq!(no_match_notice(&mut fixture), Value::Null);
}

#[test]
fn the_language_server_gets_the_projects_jdk_as_its_runtime() {
    let mut fixture = Fixture::new();
    let project = fixture.project();
    let options = json!({ "settings": { "java": { "configuration": { "runtimes": "${javaRuntimes}" } } } });
    let expanded = super::expand(&options, Some(&project));
    assert_eq!(
        expanded["settings"]["java"]["configuration"]["runtimes"],
        json!([{ "name": "JavaSE-21", "path": project.jdk.home, "default": true }])
    );
    let old = ProjectJdk { jdk: detect::Jdk { major: 8, ..project.jdk.clone() }, ..project };
    assert_eq!(
        super::expand(&options, Some(&old))["settings"]["java"]["configuration"]["runtimes"][0]["name"],
        "JavaSE-1.8"
    );
    assert_eq!(super::expand(&options, None)["settings"]["java"]["configuration"]["runtimes"], json!([]));
}

#[test]
fn the_project_uses_the_newest_jdk_until_it_chooses_one() {
    let mut fixture = Fixture::new();
    let list = fixture.run("toolchain.jdk.list", json!({}));
    let majors: Vec<u64> = list["jdks"].as_array().unwrap().iter().map(|jdk| jdk["major"].as_u64().unwrap()).collect();
    assert_eq!(majors, [21, 17]);
    let project = fixture.project();
    assert_eq!(
        (project.jdk.major, project.chosen, project.language_level, project.language_level_default),
        (21, false, 21, true)
    );

    let seventeen = fixture.jvm.join("zulu-17.jdk/Contents/Home");
    fixture.run("toolchain.jdk.choose", json!({ "home": seventeen }));
    fixture.run("toolchain.jdk.languageLevel", json!({ "level": "11" }));
    let project = fixture.project();
    assert_eq!(
        (project.jdk.major, project.chosen, project.language_level, project.language_level_default),
        (17, true, 11, false)
    );
    assert_eq!(project.jdk.vendor, "Azul Systems, Inc.");

    let kept: Value = serde_json::from_slice(&std::fs::read(fixture.data.join(super::FILE)).unwrap()).unwrap();
    assert_eq!(kept["projects"][fixture.project.display().to_string()]["languageLevel"], 11);
    let mut again = Store::with_sources(Sources { system_folders: vec![fixture.jvm.clone()], ..Sources::default() });
    again.set_data_dir(fixture.data.clone());
    assert_eq!(again.project(&fixture.project).unwrap().jdk.major, 17);

    fixture.run("toolchain.jdk.choose", json!({ "home": "" }));
    fixture.run("toolchain.jdk.languageLevel", json!({ "level": "default" }));
    assert_eq!(fixture.project().jdk.major, 21);
    let error =
        fixture.registry.dispatch(&mut fixture.session, "toolchain.jdk.languageLevel", json!({ "level": "seven" }));
    assert!(error.unwrap_err().to_string().contains("not a language level"));
    let error = fixture.registry.dispatch(&mut fixture.session, "toolchain.jdk.choose", json!({ "home": "/nowhere" }));
    assert!(error.unwrap_err().to_string().contains("No JDK at"));
}

#[test]
fn a_jdk_is_added_from_disk_and_removed_again() {
    let mut fixture = Fixture::new();
    let elsewhere = fixture._dir.path().join("elsewhere/jdk-24");
    fake_jdk(&elsewhere, "24.0.1", "Oracle Corporation");
    let list = fixture.run("toolchain.jdk.add", json!({ "home": elsewhere }));
    assert!(list["jdks"].as_array().unwrap().iter().any(|jdk| jdk["major"] == 24 && jdk["source"] == "added"));
    assert_eq!(fixture.project().jdk.major, 24, "the newest is the default");
    let not_one =
        fixture.registry.dispatch(&mut fixture.session, "toolchain.jdk.add", json!({ "home": fixture.project }));
    assert!(not_one.unwrap_err().to_string().contains("is not a JDK"));
    fixture.run("toolchain.jdk.remove", json!({ "home": elsewhere }));
    assert_eq!(fixture.project().jdk.major, 21);
    assert!(elsewhere.exists(), "a JDK added by hand is only forgotten");
    let system = fixture.jvm.join("temurin-21.jdk/Contents/Home");
    assert!(
        fixture.registry.dispatch(&mut fixture.session, "toolchain.jdk.remove", json!({ "home": system })).is_err()
    );

    let downloaded = fixture.data.join("jdks/jdk-25+36/Contents/Home");
    fake_jdk(&downloaded, "25", "Eclipse Adoptium");
    let list = fixture.run("toolchain.jdk.refresh", json!({}));
    assert!(list["jdks"].as_array().unwrap().iter().any(|jdk| jdk["source"] == "downloaded"));
    fixture.run("toolchain.jdk.remove", json!({ "home": downloaded }));
    assert!(!fixture.data.join("jdks/jdk-25+36").exists());
}

#[test]
fn programs_of_the_project_get_its_jdk() {
    let mut fixture = Fixture::new();
    let project = fixture.project();
    let mut env: Vec<(String, String)> =
        vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned()), ("JAVA_HOME".to_owned(), "/old".to_owned())];
    super::with_project_jdk(&mut env, Some(&project));
    let home = project.jdk.home.display().to_string();
    assert!(env.contains(&("JAVA_HOME".to_owned(), home.clone())));
    assert!(env.contains(&("PATH".to_owned(), format!("{home}/bin:/usr/bin:/bin"))));
    assert_eq!(env.len(), 2);
    let options = json!({ "settings": { "java": { "home": "${jdkHome}", "level": "${javaLanguageLevel}" } }, "n": 1 });
    let expanded = super::expand(&options, Some(&project));
    assert_eq!(expanded["settings"]["java"]["home"], home);
    assert_eq!(expanded["settings"]["java"]["level"], "21");
    assert_eq!(super::expand(&options, None), options, "left as they are without a JDK");
    assert!(super::is_jvm_project(&fixture.project));
    assert!(!super::is_jvm_project(fixture._dir.path()));

    let context = crate::run::run_context(&mut fixture.session, &fixture.project.clone());
    assert!(context.env.env.iter().any(|(key, value)| key == "JAVA_HOME" && *value == home));
}

#[test]
fn the_settings_page_offers_the_jdks_and_downloads() {
    let mut fixture = Fixture::new();
    let form = fixture.run("toolchain.jdk.settings", json!({}));
    let text = form.to_string();
    assert!(text.contains("Project SDK") && text.contains("Language level"), "{text}");
    assert!(text.contains("Default: 21.0.5 (Eclipse Adoptium)"), "{text}");
    assert!(text.contains("Download Eclipse Temurin 21") && text.contains("Add JDK from Disk"), "{text}");
    assert!(text.contains("toolchain.jdk.download"));
    zen_i18n::set_language(Some("ru"));
    assert!(fixture.run("toolchain.jdk.settings", json!({})).to_string().contains("SDK проекта"));
    zen_i18n::set_language(Some("en"));
    let error = fixture.registry.dispatch(
        &mut fixture.session,
        "toolchain.jdk.download",
        json!({ "version": 21, "vendor": "acme" }),
    );
    assert!(error.unwrap_err().to_string().contains("cannot be downloaded"));
    let _ = detect::major_of("21");
}

#[test]
#[ignore = "downloads a JDK from the Adoptium API"]
fn a_real_jdk_download() {
    if std::env::var_os("ZEN_JDK_DOWNLOAD").is_none() {
        return;
    }
    let releases = super::adoptium::fetch_releases().unwrap();
    assert!(releases.lts.contains(&21));
    let package = super::adoptium::fetch_package(21).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let folder = super::adoptium::install(&package, dir.path(), |_, _| {}).unwrap();
    let jdk = detect::read(&folder, super::JdkSource::Downloaded).unwrap();
    assert_eq!(jdk.major, 21);
    let output = std::process::Command::new(jdk.home.join("bin/java")).arg("-version").output().unwrap();
    assert!(output.status.success());
}

#[test]
#[ignore = "reads a real JDK named by ZEN_JDK_HOME"]
fn a_real_jdk_is_read_and_the_machine_listed() {
    let home = crate::build_tools::real_tool_in_tests("ZEN_JDK_HOME");
    let jdk = detect::read(&home, super::JdkSource::Added).unwrap();
    assert!(jdk.major >= 8 && jdk.compiler, "{jdk:?}");
    let mut store = Store::default();
    let added = store.add(&home).unwrap();
    let jdks = store.jdks();
    assert!(jdks.iter().any(|found| found.home == added.home));
}

#[test]
fn an_agent_does_not_choose_the_program_a_person_s_run_uses() {
    let mut fixture = Fixture::new();
    let (project, other) = (fixture.project.join("in-the-project"), fixture.project.join("other"));
    fake_jdk(&project, "21.0.1", "Evil");
    fake_jdk(&other, "22.0.1", "Vendor");
    fixture.run("toolchain.jdk.add", json!({ "home": other }));
    let Fixture { registry, session, .. } = &mut fixture;
    let mut as_agent = |command: &str, params: Value| {
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params))
    };
    let answers = [
        as_agent("toolchain.jdk.add", json!({ "home": project })),
        as_agent("toolchain.jdk.choose", json!({ "home": other })),
        as_agent("toolchain.jdk.remove", json!({ "home": other })),
    ];
    let refused = answers.iter().filter(|answer| format!("{answer:?}").contains(crate::trust::AGENT_NOT_ALLOWED));
    assert_eq!(refused.count(), 3, "{answers:?}");
    assert!(as_agent("toolchain.jdk.list", json!({})).is_ok(), "reading stays open");
    let text = fixture.run("toolchain.jdk.list", json!({})).to_string();
    assert!(!text.contains("in-the-project"), "{text}");
    assert!(text.contains("other"), "{text}");
    assert!(!fixture.project().chosen, "{text}");
}

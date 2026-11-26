use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

const MAVEN_POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>shop</groupId><artifactId>shop</artifactId><version>1</version>
  <properties><lombok.version>1.18.34</lombok.version><mapstruct.version>1.6.3</mapstruct.version></properties>
  <dependencies>
    <dependency><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId><version>${lombok.version}</version><scope>provided</scope></dependency>
  </dependencies>
  <build><plugins><plugin>
    <artifactId>maven-compiler-plugin</artifactId>
    <configuration>
      <annotationProcessorPaths>
        <path><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId><version>${lombok.version}</version></path>
        <path><groupId>org.mapstruct</groupId><artifactId>mapstruct-processor</artifactId><version>${mapstruct.version}</version></path>
      </annotationProcessorPaths>
      <compilerArgs><arg>-Amapstruct.defaultComponentModel=spring</arg><arg>-parameters</arg></compilerArgs>
    </configuration>
  </plugin></plugins></build>
</project>
"#;

const GRADLE_BUILD: &str = "plugins { id 'java' }
repositories { mavenCentral() }
dependencies {
    compileOnly 'org.projectlombok:lombok:1.18.36'
    annotationProcessor 'org.projectlombok:lombok:1.18.36'
    annotationProcessor \"org.mapstruct:mapstruct-processor:1.6.3\"
}
tasks.withType(JavaCompile) { options.compilerArgs += ['-Adebug=true'] }
";

const PERSON: &str = "package shop;

import lombok.Builder;
import lombok.Getter;
import lombok.extern.slf4j.Slf4j;

@Getter
@Builder
@Slf4j
public class Person {
    private final String name;
}
";

struct Sample {
    _folder: tempfile::TempDir,
    root: PathBuf,
    repository: PathBuf,
    gradle_home: PathBuf,
    env: BuildEnv,
}

fn sample(files: &[(&str, &str)]) -> Sample {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("shop");
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let repository = folder.path().join("m2");
    let gradle_home = folder.path().join("gradle home");
    let env = BuildEnv::default();
    crate::trust::trust_in_tests(&env, &root);
    env.set_home(Some(folder.path().join("home")));
    env.set_project_settings(
        crate::maven::settings::TOOL,
        &root,
        &crate::maven::settings::MavenSettings {
            local_repository: repository.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    env.set_project_settings(
        crate::gradle::settings::TOOL,
        &root,
        &crate::gradle::settings::GradleSettings { user_home: gradle_home.display().to_string(), ..Default::default() },
    )
    .unwrap();
    Sample { _folder: folder, root, repository, gradle_home, env }
}

impl Sample {
    fn maven_jar(&self, group: &str, artifact: &str, version: &str) -> PathBuf {
        let jar = self
            .repository
            .join(group.replace('.', "/"))
            .join(artifact)
            .join(version)
            .join(format!("{artifact}-{version}.jar"));
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(&jar, "jar").unwrap();
        jar
    }

    fn gradle_jar(&self, group: &str, artifact: &str, version: &str) -> PathBuf {
        let jar = self
            .gradle_home
            .join("caches/modules-2/files-2.1")
            .join(group)
            .join(artifact)
            .join(version)
            .join("0a1b2c")
            .join(format!("{artifact}-{version}.jar"));
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(&jar, "jar").unwrap();
        jar
    }
}

#[test]
fn a_maven_sample_with_lombok_gives_its_agent_processors_and_options_from_the_pom() {
    let sample = sample(&[("pom.xml", MAVEN_POM), ("src/main/java/shop/Person.java", PERSON)]);
    let lombok = sample.maven_jar("org.projectlombok", "lombok", "1.18.34");
    let mapstruct = sample.maven_jar("org.mapstruct", "mapstruct-processor", "1.6.3");
    let build = from_build(&sample.env, &sample.root);
    assert_eq!(build.tool, "maven");
    assert_eq!(build.processors, ["org.projectlombok:lombok:1.18.34", "org.mapstruct:mapstruct-processor:1.6.3"]);
    assert_eq!(build.processor_jars, [lombok.clone(), mapstruct.clone()]);
    assert_eq!(build.options, BTreeMap::from([("mapstruct.defaultComponentModel".to_owned(), "spring".to_owned())]));
    assert_eq!(build.generated_sources, "target/generated-sources/annotations");
    let found = build.lombok.clone().unwrap();
    assert_eq!((found.version.as_str(), found.version_declared), ("1.18.34", true));
    assert_eq!(found.jar.as_ref(), Some(&lombok));
    assert_eq!(found.build_file, sample.root.join("pom.xml"));
    let launch = launch(&sample.env, &sample.root);
    assert_eq!(launch.agents(), [format!("-javaagent:{}", lombok.display())]);
    assert!(launch.processing);
}

#[test]
fn a_lombok_not_downloaded_yet_gives_no_agent_and_nothing_is_fetched() {
    let sample = sample(&[("pom.xml", MAVEN_POM)]);
    let build = from_build(&sample.env, &sample.root);
    let lombok = build.lombok.unwrap();
    assert_eq!((lombok.version.as_str(), lombok.jar), ("1.18.34", None));
    assert!(launch(&sample.env, &sample.root).agents().is_empty());
    assert!(!sample.repository.exists(), "the IDE never downloads Lombok itself");
}

#[test]
fn a_lombok_version_managed_by_a_parent_is_the_newest_one_in_the_local_repositories() {
    let pom = "<project><modelVersion>4.0.0</modelVersion><groupId>a</groupId><artifactId>a</artifactId>\
               <version>1</version><modules><module>core</module></modules></project>";
    let core = "<project><modelVersion>4.0.0</modelVersion><artifactId>core</artifactId><dependencies>\
                <dependency><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId></dependency>\
                </dependencies></project>";
    let sample = sample(&[("pom.xml", pom), ("core/pom.xml", core)]);
    sample.maven_jar("org.projectlombok", "lombok", "1.18.9");
    sample.gradle_jar("org.projectlombok", "lombok", "1.18.10");
    let lombok = from_build(&sample.env, &sample.root).lombok.unwrap();
    assert_eq!((lombok.version.as_str(), lombok.version_declared), ("1.18.10", false), "versions compare as numbers");
    assert_eq!(lombok.build_file, sample.root.join("core/pom.xml"));
    assert!(lombok.jar.unwrap().ends_with("0a1b2c/lombok-1.18.10.jar"));
}

#[test]
fn a_gradle_sample_with_lombok_finds_it_in_the_gradle_cache() {
    let sample = sample(&[
        ("settings.gradle", "rootProject.name = 'shop'\n"),
        ("build.gradle", GRADLE_BUILD),
        ("src/main/java/shop/Person.java", PERSON),
    ]);
    let jar = sample.gradle_jar("org.projectlombok", "lombok", "1.18.36");
    let build = from_build(&sample.env, &sample.root);
    assert_eq!(build.tool, "gradle");
    assert_eq!(build.processors, ["org.projectlombok:lombok:1.18.36", "org.mapstruct:mapstruct-processor:1.6.3"]);
    assert_eq!(build.options, BTreeMap::from([("debug".to_owned(), "true".to_owned())]));
    assert_eq!(build.generated_sources, "build/generated/sources/annotationProcessor/java/main");
    assert_eq!(build.lombok.unwrap().jar, Some(jar.clone()));
    assert_eq!(launch(&sample.env, &sample.root).agents(), [format!("-javaagent:{}", jar.display())]);
}

#[test]
fn a_gradle_lombok_comes_from_a_version_catalog_a_property_or_the_freefair_plugin() {
    let catalog = "[versions]\nlombok = \"1.18.30\"\n\n[libraries]\n\
                   lombok = { module = \"org.projectlombok:lombok\", version.ref = \"lombok\" }\n";
    let with_catalog = sample(&[
        ("build.gradle.kts", "dependencies {\n    compileOnly(libs.lombok)\n    annotationProcessor(libs.lombok)\n}\n"),
        ("gradle/libs.versions.toml", catalog),
    ]);
    assert_eq!(from_build(&with_catalog.env, &with_catalog.root).lombok.unwrap().version, "1.18.30");

    let with_property = sample_with_property();
    assert_eq!(from_build(&with_property.env, &with_property.root).lombok.unwrap().version, "1.18.32");

    let with_plugin = sample(&[("build.gradle", "plugins {\n    id 'io.freefair.lombok' version '8.10'\n}\n")]);
    with_plugin.maven_jar("org.projectlombok", "lombok", "1.18.38");
    let lombok = from_build(&with_plugin.env, &with_plugin.root).lombok.unwrap();
    assert_eq!((lombok.version.as_str(), lombok.version_declared), ("1.18.38", false));
}

fn sample_with_property() -> Sample {
    sample(&[
        ("build.gradle", "dependencies {\n    compileOnly \"org.projectlombok:lombok:${lombokVersion}\"\n}\n"),
        ("gradle.properties", "lombokVersion=1.18.32\n"),
    ])
}

#[test]
fn a_project_without_lombok_or_a_build_has_no_agent() {
    let gradle = sample(&[("build.gradle", "plugins { id 'java' }\n")]);
    assert_eq!(from_build(&gradle.env, &gradle.root).lombok, None);
    let plain = sample(&[("src/App.java", "class App {}")]);
    assert_eq!(from_build(&plain.env, &plain.root).tool, "none");
    assert_eq!(launch(&plain.env, &plain.root), JavaLaunch { lombok: None, processing: true });
}

fn maven_build_info(lombok: &Path, mapstruct: &Path) -> FromBuild {
    FromBuild {
        tool: "maven".to_owned(),
        processors: Vec::new(),
        processor_jars: vec![lombok.to_owned(), mapstruct.to_owned()],
        options: BTreeMap::from([("a".to_owned(), "1".to_owned()), ("b".to_owned(), "2".to_owned())]),
        generated_sources: "target/generated-sources/annotations".to_owned(),
        lombok: Some(Lombok {
            version: "1.18.34".to_owned(),
            version_declared: true,
            jar: Some(lombok.to_owned()),
            build_file: PathBuf::from("pom.xml"),
        }),
    }
}

#[test]
fn javac_gets_the_processor_path_generated_folder_and_options_with_the_settings_over_the_build() {
    let folder = tempfile::tempdir().unwrap();
    let module = folder.path();
    let (lombok, mapstruct) = (PathBuf::from("/m2/lombok.jar"), PathBuf::from("/m2/mapstruct.jar"));
    let build = maven_build_info(&lombok, &mapstruct);
    let settings = ProcessingSettings {
        options: BTreeMap::from([("b".to_owned(), "own".to_owned()), ("flag".to_owned(), String::new())]),
        ..ProcessingSettings::default()
    };
    let javac = javac_with(&settings, &build, module, BuildTool::Maven, None, Some(25)).unwrap();
    let generated = module.join("target/generated-sources/annotations");
    let path = std::env::join_paths([&lombok, &mapstruct]).unwrap().to_string_lossy().into_owned();
    assert_eq!(
        javac.args,
        [
            "-proc:full".to_owned(),
            "-processorpath".to_owned(),
            path,
            "-s".to_owned(),
            generated.display().to_string(),
            "-Aa=1".to_owned(),
            "-Ab=own".to_owned(),
            "-Aflag".to_owned(),
        ]
    );
    assert!(generated.is_dir(), "javac needs the folder to exist");
    assert_eq!(javac.classpath, std::slice::from_ref(&lombok), "Lombok's annotations compile against its jar");

    let custom = ProcessingSettings {
        processor_path: ProcessorSource::Custom,
        custom_path: "/own/processor.jar".to_owned(),
        generated_sources: "gen".to_owned(),
        ..ProcessingSettings::default()
    };
    let javac = javac_with(&custom, &build, module, BuildTool::Maven, None, Some(17)).unwrap();
    assert_eq!(
        javac.args[..4],
        ["-processorpath", "/own/processor.jar", "-s", &module.join("gen").display().to_string()]
    );
    assert!(!javac.args.contains(&"-proc:full".to_owned()), "javac before 21 does not know -proc:full");

    let gradle = FromBuild { lombok: None, processor_jars: Vec::new(), ..build.clone() };
    let file = module.join("processorpath.args");
    let javac =
        javac_with(&ProcessingSettings::default(), &gradle, module, BuildTool::Gradle, Some(&file), None).unwrap();
    assert_eq!(javac.args[1..3], ["-processorpath".to_owned(), format!("@{}", file.display())]);
    assert!(javac.classpath.is_empty());

    let off = ProcessingSettings { enabled: false, ..ProcessingSettings::default() };
    let javac = javac_with(&off, &build, module, BuildTool::Maven, None, Some(25)).unwrap();
    assert_eq!(javac.args, ["-proc:none"]);
}

#[test]
fn the_language_server_options_get_the_enabled_flag_and_the_launch_says_what_changed() {
    let options = json!({ "java": { "import": { "gradle": { "annotationProcessing": { "enabled": ENABLED } } } }, "n": [ENABLED] });
    assert_eq!(
        expand_options(&options, false),
        json!({ "java": { "import": { "gradle": { "annotationProcessing": { "enabled": false } } } }, "n": [false] })
    );
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let none = JavaLaunch { lombok: None, processing: true };
    let with = JavaLaunch { lombok: Some(("1.18.34".to_owned(), PathBuf::from("/l.jar"))), processing: true };
    assert_eq!(with.change_notice(&none, "JDT LS").unwrap(), "JDT LS restarts with Lombok 1.18.34 as its agent.");
    assert!(none.change_notice(&with, "JDT LS").unwrap().contains("without Lombok"));
    let off = JavaLaunch { processing: false, ..with.clone() };
    assert!(off.change_notice(&with, "JDT LS").unwrap().contains("annotation processing settings"));
    assert_eq!(with.change_notice(&with.clone(), "JDT LS"), None);
}

#[test]
fn the_java_plugin_starts_jdt_ls_with_the_agents_before_its_jar_and_the_processing_flag() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/java/plugin.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let declared = zen_lsp::ServerDeclaration::parse_list(&manifest["languageServers"].to_string()).unwrap();
    let jdtls = &declared[0];
    assert!(names_launch(jdtls));
    let agents = jdtls.args.iter().position(|arg| arg == AGENTS).unwrap();
    assert_eq!(jdtls.args[agents + 1], "-jar", "JVM options go before -jar");
    assert_eq!(
        jdtls.initialization_options["settings"]["java"]["import"]["gradle"]["annotationProcessing"]["enabled"],
        ENABLED
    );
}

#[test]
fn the_settings_change_by_key_and_the_page_shows_what_the_build_gives() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let sample = sample(&[("pom.xml", MAVEN_POM)]);
    let mut session = Session::new();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    session.set_state(sample.env.clone());
    let project = json!(sample.root);
    let set = |session: &mut Session, params: serde_json::Value| {
        registry.dispatch(session, SetSettings::ID, params).map_err(|error| error.to_string())
    };
    let answer = set(
        &mut session,
        json!({ "project": project, "values": { "processorPath": "custom", "customPath": "/p.jar", "options": "-Ax=1; y" } }),
    )
    .unwrap();
    assert_eq!(answer["settings"]["processorPath"], "custom");
    assert_eq!(answer["settings"]["options"], json!({ "x": "1", "y": "" }));
    assert_eq!(answer["fromBuild"]["lombok"]["version"], "1.18.34");
    let read = registry.dispatch(&mut session, GetSettings::ID, json!({ "project": project })).unwrap();
    assert_eq!(read["settings"]["customPath"], "/p.jar", "kept per project");
    assert!(
        set(&mut session, json!({ "project": project, "key": "nothing", "value": 1 })).unwrap_err().contains("nothing")
    );
    assert!(set(&mut session, json!({ "project": project, "key": "processorPath", "value": "elsewhere" })).is_err());
    set(&mut session, json!({ "project": project, "key": "processorPath", "value": null })).unwrap();

    registry.dispatch(&mut session, "file.project.open", json!({ "path": sample.root })).unwrap();
    let form: Form =
        serde_json::from_value(registry.dispatch(&mut session, ProcessorsForm::ID, json!({})).unwrap()).unwrap();
    let texts = form.texts();
    assert!(texts.contains(&"Enable annotation processing"));
    assert!(texts.iter().any(|text| text.starts_with("From the Maven build: org.projectlombok:lombok:1.18.34")));
    assert!(texts.contains(&"From the Maven build: mapstruct.defaultComponentModel=spring"));
    assert!(texts.iter().any(|text| text.contains("depends on Lombok 1.18.34, which is not in the local Maven")));
    assert!(!texts.contains(&"Processor path:"), "the custom path shows only for a custom choice");
}

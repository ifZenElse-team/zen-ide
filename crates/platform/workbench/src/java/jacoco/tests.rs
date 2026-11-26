use std::path::{Path, PathBuf};

use zen_run::{BranchCounts, LineStatus};

use super::*;
use crate::java::junit::{GRADLE_INIT_SCRIPT, JUNIT, TestOptions, configuration};

const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd"><report name="shop"><sessioninfo id="s" start="1" dump="2"/><group name="core"><package name="com/acme"><class name="com/acme/Calc" sourcefilename="Calc.java"/><sourcefile name="Calc.java"><line nr="3" mi="3" ci="0" mb="0" cb="0"/><line nr="5" mi="0" ci="2" mb="1" cb="1"/><line nr="6" mi="0" ci="2" mb="0" cb="0"/><line nr="8" mi="6" ci="0" mb="2" cb="0"/><line nr="9" mi="2" ci="3" mb="0" cb="0"/><counter type="LINE" missed="2" covered="3"/></sourcefile><sourcefile name="Prices.kt"><line nr="4" mi="0" ci="7" mb="0" cb="2"/></sourcefile></package></group><package name="com/acme/util"><sourcefile name="Strings.kt"><line nr="2" mi="1" ci="0"/></sourcefile></package><package name=""><sourcefile name="Main.java"><line nr="1" mi="0" ci="1"/></sourcefile></package></report>"#;

fn write(root: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn statuses(report: &CoverageReport, path: &Path) -> Vec<(u32, LineStatus)> {
    report.file(path).unwrap().lines.iter().map(|line| (line.line, line.status)).collect()
}

#[test]
fn a_jacoco_report_gives_lines_that_ran_partly_and_not_at_all() {
    let files = parse_xml(REPORT).unwrap();
    let names: Vec<(&str, &str)> = files.iter().map(|file| (file.package.as_str(), file.file.as_str())).collect();
    assert_eq!(
        names,
        [("com/acme", "Calc.java"), ("com/acme", "Prices.kt"), ("com/acme/util", "Strings.kt"), ("", "Main.java")]
    );
    let mut builder = CoverageBuilder::new();
    let path = Path::new("/p/Calc.java");
    add(&mut builder, path, &files[0].lines);
    let report = builder.build();
    use LineStatus::*;
    assert_eq!(statuses(&report, path), [(3, NotCovered), (5, Partly), (6, Covered), (8, NotCovered), (9, Partly)]);
    let file = report.file(path).unwrap();
    assert_eq!(file.line(5).unwrap().branches, Some(BranchCounts { covered: 1, total: 2 }));
    assert_eq!(file.line(8).unwrap().branches, Some(BranchCounts { covered: 0, total: 2 }));
    assert_eq!((file.line(6).unwrap().hits, file.line(3).unwrap().hits), (1, 0));
    assert_eq!(file.counts().branches, Some(BranchCounts { covered: 1, total: 4 }));
}

#[test]
fn a_file_that_is_not_a_jacoco_report_is_refused() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    assert_eq!(parse_xml("<project/>"), Err(t!("java-coverage-not-a-report")));
    assert_eq!(
        parse_xml(
            r#"<report><package name="a"><sourcefile name="A.java"><line nr="x"/></sourcefile></package></report>"#
        ),
        Err(t!("java-coverage-bad-line", text = "x"))
    );
    assert!(parse_xml("<report>").is_err());
}

#[test]
fn java_and_kotlin_classes_map_to_their_sources_through_the_reports_file_names() {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        &[
            ("core/src/main/java/com/acme/Calc.java", ""),
            ("core/src/test/java/com/acme/Calc.java", ""),
            ("core/target/generated/com/acme/Calc.java", ""),
            ("core/src/main/kotlin/com/acme/Prices.kt", ""),
            ("core/src/main/kotlin/Strings.kt", ""),
            ("a/Main.java", ""),
            ("b/Main.java", ""),
        ],
    );
    let xml = root.join("report.xml");
    std::fs::write(&xml, REPORT).unwrap();
    let report = read_reports(&[xml], root).unwrap();
    let paths: Vec<PathBuf> =
        report.files.iter().map(|file| file.path.strip_prefix(root).unwrap().to_owned()).collect();
    assert_eq!(
        paths,
        [
            PathBuf::from("core/src/main/java/com/acme/Calc.java"),
            PathBuf::from("core/src/main/kotlin/Strings.kt"),
            PathBuf::from("core/src/main/kotlin/com/acme/Prices.kt"),
        ],
        "a Kotlin file outside its package folder is found by its name; two Main.java are left out"
    );
    assert_eq!(statuses(&report, &root.join("core/src/main/kotlin/com/acme/Prices.kt")), [(4, LineStatus::Covered)]);
    assert_eq!(read_reports(&[], root), Err(t!("java-coverage-no-data")));
}

#[test]
fn only_the_pinned_jacoco_is_used_whichever_folder_holds_it() {
    use sha2::Digest;
    let digest = |text: &str| hex::encode(sha2::Sha512::digest(text.as_bytes()));
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let data = dir.path().join("data");
    let pins = (digest("agent"), digest("cli"));
    let find = || find_pinned(&home, Some(&data), (&pins.0, &pins.1));
    assert_eq!(find(), Jars::default());
    let maven = format!(".m2/repository/org/jacoco/org.jacoco.agent/{VERSION}/org.jacoco.agent-{VERSION}-runtime.jar");
    let gradle = format!(
        ".gradle/caches/modules-2/files-2.1/org.jacoco/org.jacoco.cli/{VERSION}/cd34/org.jacoco.cli-{VERSION}-nodeps.jar"
    );
    write(
        &home,
        &[
            (maven.as_str(), "planted"),
            (gradle.as_str(), "planted"),
            (".m2/repository/org/jacoco/org.jacoco.agent/0.8.99/org.jacoco.agent-0.8.99-runtime.jar", "agent"),
        ],
    );
    assert_eq!(find(), Jars::default(), "a jar with another digest or another version is never run");
    write(&home, &[(maven.as_str(), "agent"), (gradle.as_str(), "cli")]);
    assert_eq!(find(), Jars { agent: Some(home.join(&maven)), cli: Some(home.join(&gradle)) });
    assert_eq!(find_jars(&home, Some(&data)), Jars::default(), "the real digests are not these");
    let own = format!("tools/jacoco/{VERSION}/org.jacoco.cli-{VERSION}-nodeps.jar");
    write(&data, &[(own.as_str(), "cli")]);
    assert_eq!(find().cli, Some(data.join(&own)), "the IDE's own folder comes first");
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_out_of_the_project_is_not_searched_for_reports() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    let root = dir.path().join("root");
    write(&outside, &[("target/site/jacoco/jacoco.xml", "<report/>")]);
    write(&root, &[("app/pom.xml", "<project/>")]);
    std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
    let found = project_folders(&root);
    assert!(found.contains(&root.join("app")), "{found:?}");
    assert!(!found.iter().any(|folder| folder.starts_with(root.join("linked"))), "{found:?}");
}

fn tools(root: &Path) -> PathBuf {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["mvn", "gradle"] {
        let path = bin.join(tool);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    bin
}

fn provider(root: &Path, with_agent: bool) -> JavaTestRun {
    let env = BuildEnv::default();
    env.set_folders(Some(vec![tools(root)]));
    env.set_home(Some(root.join("home")));
    let agent = with_agent.then(|| {
        let agent = root.join("jars/agent.jar");
        write(root, &[("jars/agent.jar", "jar")]);
        agent
    });
    JavaTestRun::new(env).with_jacoco(Jars { agent, cli: None })
}

fn tests() -> Configuration {
    configuration(JUNIT, "all", &TestOptions { scope: "all".to_owned(), ..TestOptions::default() })
}

fn line_for(provider: &JavaTestRun, project: &Path, report: &Path) -> Result<CommandLine, String> {
    let context = RunContext { root: project.to_owned(), ..RunContext::default() };
    let configuration = tests();
    use zen_run::RunProvider;
    let coverage = provider.coverage(&configuration).expect("JUnit runs with coverage");
    coverage.command_lines(&configuration, &context, report).map(|mut lines| lines.remove(0))
}

#[test]
fn maven_tests_run_with_the_builds_jacoco_plugin_or_the_agent_in_the_surefire_jvm() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    let report = dir.path().join("cov/coverage.out");
    let exec = dir.path().join("cov/jacoco.exec");

    write(
        &project,
        &[(
            "pom.xml",
            "<project><build><plugins><plugin><artifactId>jacoco-maven-plugin</artifactId></plugin></plugins></build></project>",
        )],
    );
    let line = line_for(&provider(dir.path(), false), &project, &report).unwrap();
    let at = line.args.iter().position(|arg| arg == "test").unwrap();
    assert_eq!(line.args[at - 1], "jacoco:prepare-agent");
    assert!(line.args.contains(&format!("-Djacoco.destFile={}", exec.display())), "{:?}", line.args);
    assert!(line.display.contains("jacoco:prepare-agent test"), "{}", line.display);

    write(
        &project,
        &[(
            "core/pom.xml",
            "<project><artifactId>jacoco-maven-plugin</artifactId><goal>prepare-agent</goal></project>",
        )],
    );
    let line = line_for(&provider(dir.path(), false), &project, &report).unwrap();
    assert!(!line.args.contains(&"jacoco:prepare-agent".to_owned()), "the build prepares the agent itself");

    write(&project, &[("pom.xml", "<project/>"), ("core/pom.xml", "<project/>")]);
    assert_eq!(line_for(&provider(dir.path(), false), &project, &report), Err(missing()));
    let line = line_for(&provider(dir.path(), true), &project, &report).unwrap();
    let agent = dir.path().join("cov/jacocoagent.jar");
    assert!(agent.is_file(), "the agent is copied beside the data");
    assert_eq!(
        line.args.last().unwrap(),
        &format!("-DargLine=-javaagent:{}=destfile={},append=true", agent.display(), exec.display())
    );
}

#[test]
fn gradle_tests_get_the_coverage_folder_and_the_agent_through_the_init_script() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    let report = dir.path().join("cov/coverage.out");
    write(&project, &[("settings.gradle", ""), ("core/build.gradle", "plugins { id 'java' }")]);
    assert_eq!(line_for(&provider(dir.path(), false), &project, &report), Err(missing()));
    let line = line_for(&provider(dir.path(), true), &project, &report).unwrap();
    let at = line.args.iter().position(|arg| arg == "test").unwrap();
    assert_eq!(
        line.args[at - 2..at],
        [
            format!("-Dzen.coverageDir={}", dir.path().join("cov").display()),
            format!("-Dzen.jacocoAgent={}", dir.path().join("cov/jacocoagent.jar").display()),
        ]
    );
    write(&project, &[("core/build.gradle", "plugins { id 'java'\n id 'jacoco' }")]);
    let line = line_for(&provider(dir.path(), false), &project, &report).unwrap();
    assert!(line.args.iter().any(|arg| arg.starts_with("-Dzen.coverageDir=")));
    assert!(!line.args.iter().any(|arg| arg.starts_with("-Dzen.jacocoAgent=")));
    for needed in [
        "zen.coverageDir",
        "afterEvaluate",
        "hasPlugin('jacoco')",
        "finalizedBy('jacocoTestReport')",
        "zen.jacocoAgent",
        "destfile=",
    ] {
        assert!(GRADLE_INIT_SCRIPT.contains(needed), "{needed}");
    }
}

#[test]
fn the_reports_gradles_jacoco_plugin_wrote_are_read_into_the_project_files() {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    write(
        &project,
        &[
            ("settings.gradle", ""),
            ("core/build.gradle", "plugins { id 'jacoco' }"),
            ("core/src/main/java/com/acme/Calc.java", ""),
        ],
    );
    let folder = dir.path().join("cov");
    write(&folder, &[("gradle/core-test.xml", REPORT), ("gradle/notes.txt", "")]);
    let context = RunContext { root: project.clone(), ..RunContext::default() };
    let read = provider(dir.path(), false).read(&tests(), &context, &folder.join("coverage.out")).unwrap();
    assert_eq!(read.files.len(), 1);
    assert_eq!(read.files[0].path, project.join("core/src/main/java/com/acme/Calc.java"));
    let empty = dir.path().join("empty");
    assert_eq!(
        provider(dir.path(), false).read(&tests(), &context, &empty.join("coverage.out")),
        Err(t!("java-coverage-no-data"))
    );
}

#[test]
fn compiled_classes_are_measured_where_maven_and_gradle_put_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        &[
            ("pom.xml", ""),
            ("target/classes/A.class", ""),
            ("lib/build.gradle.kts", ""),
            ("lib/build/classes/java/main/B.class", ""),
            ("lib/build/classes/kotlin/main/C.class", ""),
            ("lib/build/classes/kotlin/test/D.class", ""),
            ("node_modules/x/pom.xml", ""),
            ("node_modules/x/target/classes/E.class", ""),
        ],
    );
    let folders: Vec<PathBuf> =
        class_folders(root).iter().map(|folder| folder.strip_prefix(root).unwrap().to_owned()).collect();
    assert_eq!(
        folders,
        [
            PathBuf::from("target/classes"),
            PathBuf::from("lib/build/classes/java/main"),
            PathBuf::from("lib/build/classes/kotlin/main")
        ]
    );
    let line = report_line(
        Path::new("/jdk/bin/java"),
        Path::new("/j/cli.jar"),
        Path::new("/c/jacoco.exec"),
        &[PathBuf::from("/a"), PathBuf::from("/b")],
        Path::new("/c/jacoco.xml"),
    );
    assert_eq!(
        line.args,
        [
            "-jar",
            "/j/cli.jar",
            "report",
            "/c/jacoco.exec",
            "--classfiles",
            "/a",
            "--classfiles",
            "/b",
            "--xml",
            "/c/jacoco.xml",
            "--quiet"
        ]
    );
}

const CALC: &str = "package com.acme;\n\npublic class Calc {\n    public static int sign(int x) {\n        if (x > 0) {\n            return 1;\n        }\n        return x < 0 ? -1 : 0;\n    }\n}\n";

const CALC_TEST: &str = "package com.acme;\n\nimport org.junit.jupiter.api.Test;\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nclass CalcTest {\n    @Test\n    void positive() { assertEquals(1, Calc.sign(5)); }\n\n    @Test\n    void fails() { assertEquals(2, Calc.sign(7)); }\n}\n";

fn run_all(lines: &[CommandLine]) -> String {
    let mut output = String::new();
    for line in lines {
        let result = std::process::Command::new(&line.program)
            .args(&line.args)
            .current_dir(&line.cwd)
            .envs(line.env.iter().map(|(key, value)| (key.as_str(), value.as_str())))
            .output()
            .unwrap();
        output.push_str(&String::from_utf8_lossy(&result.stdout));
        output.push_str(&String::from_utf8_lossy(&result.stderr));
    }
    output
}

fn assert_calc(report: &CoverageReport, project: &Path, output: &str) {
    let calc = project.join("src/main/java/com/acme/Calc.java");
    let file = report.file(&calc).unwrap_or_else(|| panic!("{report:?}\n{output}"));
    assert_eq!(file.line(6).unwrap().status, LineStatus::Covered);
    assert_eq!(file.line(5).unwrap().status, LineStatus::Partly);
    assert_eq!(file.line(8).unwrap().status, LineStatus::NotCovered);
}

#[test]
#[ignore = "runs a real JDK, JaCoCo, Maven and Gradle named by ZEN_JDK_HOME, ZEN_APP_JDK_HOME, ZEN_JACOCO_HOME, ZEN_MAVEN and ZEN_GRADLE"]
fn jacoco_real_tools() {
    let jdk = crate::build_tools::real_tool_in_tests("ZEN_JDK_HOME");
    let jacoco_home = crate::build_tools::real_tool_in_tests("ZEN_JACOCO_HOME");
    zen_i18n::register(crate::LOCALES);
    let env_for = |tool: Option<&Path>| {
        let env = BuildEnv::default();
        env.set_home(Some(jacoco_home.clone()));
        env.set_folders(Some(tool.and_then(Path::parent).map(Path::to_path_buf).into_iter().collect()));
        let jdk = jdk.clone();
        env.set_jdk(Some(std::sync::Arc::new(move |_: &Path| Some(jdk.clone()))));
        env
    };
    let dir = tempfile::tempdir().unwrap();
    let configuration = tests();

    let app_jdk = crate::build_tools::real_tool_in_tests("ZEN_APP_JDK_HOME");
    let project = dir.path().join("app shop");
    let main = CALC.replace("    public static int sign", "    public static void main(String[] args) {\n        System.out.println(sign(5));\n    }\n\n    public static int sign").replace("package com.acme;\n\n", "package com.acme;\n");
    write(&project, &[("src/main/java/com/acme/Calc.java", main.as_str())]);
    let provider = crate::java_run::JavaRun::default().with_jacoco(find_jars(&jacoco_home, None));
    let application = crate::java_run::JavaRun::application("com.acme.Calc", "");
    let context = RunContext {
        root: project.clone(),
        env: zen_run::RunEnv {
            tool: PathBuf::new(),
            env: vec![("JAVA_HOME".to_owned(), app_jdk.display().to_string())],
        },
        ..RunContext::default()
    };
    let report = dir.path().join("app cov/coverage.out");
    std::fs::create_dir_all(report.parent().unwrap()).unwrap();
    let coverage = zen_run::RunProvider::coverage(&provider, &application).unwrap();
    let output = run_all(&coverage.command_lines(&application, &context, &report).unwrap());
    let read = coverage.read(&application, &context, &report).unwrap_or_else(|error| panic!("{error}\n{output}"));
    let file =
        read.file(&project.join("src/main/java/com/acme/Calc.java")).unwrap_or_else(|| panic!("{read:?}\n{output}"));
    assert_eq!(file.line(8).unwrap().status, LineStatus::Partly, "{file:?}");
    assert_eq!(file.line(9).unwrap().status, LineStatus::Covered);

    {
        let maven = crate::build_tools::real_tool_in_tests("ZEN_MAVEN");
        for plugin in [false, true] {
            let project = dir.path().join(format!("maven shop {plugin}"));
            let build = if plugin {
                format!(
                    "<build><plugins><plugin><groupId>org.jacoco</groupId><artifactId>jacoco-maven-plugin</artifactId><version>{VERSION}</version></plugin></plugins></build>"
                )
            } else {
                String::new()
            };
            let pom = format!(
                "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"><modelVersion>4.0.0</modelVersion>\
                 <groupId>shop</groupId><artifactId>shop</artifactId><version>1</version>\
                 <properties><maven.compiler.release>17</maven.compiler.release></properties>\
                 <dependencies><dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId>\
                 <version>5.12.2</version><scope>test</scope></dependency></dependencies>\
                 <build><pluginManagement><plugins>\
                 <plugin><artifactId>maven-surefire-plugin</artifactId><version>3.5.3</version></plugin>\
                 <plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin>\
                 <plugin><artifactId>maven-resources-plugin</artifactId><version>3.3.1</version></plugin>\
                 </plugins></pluginManagement></build>{build}</project>"
            )
            .replace("</build><build>", "");
            write(
                &project,
                &[
                    ("pom.xml", pom.as_str()),
                    ("src/main/java/com/acme/Calc.java", CALC),
                    ("src/test/java/com/acme/CalcTest.java", CALC_TEST),
                ],
            );
            let provider = JavaTestRun::new(env_for(Some(&maven)));
            let context = RunContext { root: project.clone(), ..RunContext::default() };
            let report = dir.path().join(format!("maven cov {plugin}/coverage.out"));
            std::fs::create_dir_all(report.parent().unwrap()).unwrap();
            let lines = provider.command_lines(&configuration, &context, &report).unwrap();
            let output = run_all(&lines);
            let read =
                provider.read(&configuration, &context, &report).unwrap_or_else(|error| panic!("{error}\n{output}"));
            assert_calc(&read, &project, &output);
        }
    }

    {
        let gradle = crate::build_tools::real_tool_in_tests("ZEN_GRADLE");
        for plugin in [false, true] {
            let project = dir.path().join(format!("gradle shop {plugin}"));
            let build = format!(
                "plugins {{ id 'java'{} }}\nrepositories {{ mavenCentral() }}\n\
                 dependencies {{\n testImplementation 'org.junit.jupiter:junit-jupiter-api:5.11.4'\n\
                 testRuntimeOnly 'org.junit.jupiter:junit-jupiter-engine:5.11.4'\n\
                 testRuntimeOnly 'org.junit.platform:junit-platform-launcher:1.11.4'\n}}\n\
                 test {{ useJUnitPlatform() }}\n{}",
                if plugin { "\n id 'jacoco'" } else { "" },
                if plugin { format!("jacoco {{ toolVersion = '{VERSION}' }}\n") } else { String::new() }
            );
            write(
                &project,
                &[
                    ("settings.gradle", "rootProject.name = 'shop'\n"),
                    ("build.gradle", build.as_str()),
                    ("src/main/java/com/acme/Calc.java", CALC),
                    ("src/test/java/com/acme/CalcTest.java", CALC_TEST),
                ],
            );
            let provider = JavaTestRun::new(env_for(Some(&gradle)));
            let context = RunContext { root: project.clone(), ..RunContext::default() };
            let report = dir.path().join(format!("gradle cov {plugin}/coverage.out"));
            std::fs::create_dir_all(report.parent().unwrap()).unwrap();
            let lines = provider.command_lines(&configuration, &context, &report).unwrap();
            let output = run_all(&lines);
            let read =
                provider.read(&configuration, &context, &report).unwrap_or_else(|error| panic!("{error}\n{output}"));
            assert_calc(&read, &project, &output);
        }
    }
}

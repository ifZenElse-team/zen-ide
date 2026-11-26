use std::path::{Path, PathBuf};

use zen_run::{Configuration, ForeignRunConfiguration, RunContext, RunEnv, RunProvider, Stream, TargetKind};

use super::*;

const APP: &str = r#"package shop;

import java.util.List;

/** Runs the shop. "class Fake { static void main(String[] a) {} }" */
public class App {
    static String text = "void main() {";

    public static void main(String[] args) {
        System.out.println("hi");
    }

    static class Inner {
        public static void main(String... args) {}
    }

    void run() {
        Runnable r = new Runnable() { public void run() {} };
        Object o = App.class;
    }
}
"#;

#[test]
fn main_methods_are_found_with_their_classes() {
    let mains = main_methods(Path::new("/p/src/main/java/shop/App.java"), APP);
    let found: Vec<(&str, u32, u32)> =
        mains.iter().map(|main| (main.class.as_str(), main.class_line, main.line)).collect();
    assert_eq!(found, [("shop.App", 5, 8), ("shop.App$Inner", 12, 13)]);
    assert_eq!(mains[0].end_line, 20);

    let instance = "class Hello {\n    void main() {\n        System.out.println(1);\n    }\n}\n";
    assert_eq!(main_methods(Path::new("Hello.java"), instance)[0].class, "Hello");
    let implicit = "void main() {\n    IO.println(\"hi\");\n}\n";
    let mains = main_methods(Path::new("/p/Script.java"), implicit);
    assert_eq!((mains[0].class.as_str(), mains[0].line), ("Script", 0));
    assert!(main_methods(Path::new("A.java"), "class A { private static void main(String[] a) {} }").is_empty());
    assert!(main_methods(Path::new("A.java"), "class A { static void main(int x) {} }").is_empty());
    assert!(main_methods(Path::new("A.java"), "class A { static int main(String[] a) { return 0; } }").is_empty());
    let both = "class B {\n void main() {}\n public static void main(String[] a) {}\n}\n";
    assert_eq!(main_methods(Path::new("B.java"), both)[0].line, 2);
}

fn context(root: &Path) -> RunContext {
    RunContext { root: root.to_owned(), scratch: root.join(".scratch"), ..RunContext::default() }
}

#[test]
fn gutter_icons_and_the_context_run_the_class() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("shop/src/main/java/shop")).unwrap();
    std::fs::write(root.join("shop/pom.xml"), "<project/>").unwrap();
    let file = root.join("shop/src/main/java/shop/App.java");
    let targets = JavaRun::default().targets(&context(root), &file, APP);
    let lines: Vec<(u32, &str)> = targets.iter().map(|target| (target.line, target.name.as_str())).collect();
    assert_eq!(lines, [(5, "App"), (8, "App"), (12, "Inner"), (13, "Inner")]);
    assert!(targets.iter().all(|target| target.kind == TargetKind::Main));
    let options: ApplicationOptions = serde_json::from_value(targets[0].configuration.options.clone()).unwrap();
    assert_eq!((options.main_class.as_str(), options.module.as_str(), options.build), ("shop.App", "shop", true));

    let at = |line: usize| APP.lines().take(line).map(|text| text.len() + 1).sum::<usize>();
    let in_inner = JavaRun::default().context(root, &file, APP, at(13)).unwrap();
    assert_eq!(in_inner.name, "Inner");
    let in_run = JavaRun::default().context(root, &file, APP, at(18)).unwrap();
    assert_eq!(in_run.name, "App", "the class the caret is in");
    assert!(JavaRun::default().context(root, &root.join("README.md"), "x", 0).is_none());
}

fn fake_jdk(home: &Path) {
    std::fs::create_dir_all(home.join("bin")).unwrap();
    for program in ["java", "javac"] {
        std::fs::write(home.join("bin").join(program), "").unwrap();
    }
}

fn configured(kind: &str, options: serde_json::Value) -> Configuration {
    Configuration { name: "App".to_owned(), kind: kind.to_owned(), options, ..Configuration::default() }
}

#[test]
fn an_application_builds_with_the_projects_tool_then_runs_with_its_classpath() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("shop");
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    for tool in ["mvn", "gradle"] {
        let path = tools.join(tool);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let provider = JavaRun::in_folders(vec![tools.clone()]);
    let mut context = context(&root);
    context.env = RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] };
    let mut configuration =
        configured(APPLICATION, serde_json::json!({ "mainClass": "shop.App", "module": "", "build": true }));
    configuration.common.tool_arguments = "-Xmx256m -Dmode=dev".to_owned();
    configuration.common.program_arguments = "--port 8080".to_owned();
    configuration.common.env = vec![("SHOP".to_owned(), "1".to_owned())];

    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].program, tools.join("mvn"));
    assert!(lines[0].args.contains(&"dependency:build-classpath".to_owned()), "{:?}", lines[0].args);
    assert!(lines[1].args[0].ends_with("ZenClasspath.java") && lines[1].args[3].ends_with("target/classes"));
    assert!(context.scratch.join("ZenClasspath.java").is_file());
    let run = &lines[2];
    assert_eq!(run.program, jdk.join("bin/java"));
    let classpath = format!("@{}", context.scratch.join("classpath.args").display());
    assert_eq!(run.args, ["-Xmx256m", "-Dmode=dev", "-cp", classpath.as_str(), "shop.App", "--port", "8080"]);
    assert!(run.env.contains(&("JAVA_HOME".to_owned(), jdk.display().to_string())));
    assert!(run.env.contains(&("SHOP".to_owned(), "1".to_owned())));
    std::fs::write(root.join("mvnw"), "#!/bin/sh\n").unwrap();
    assert_eq!(provider.command_lines(&configuration, &context).unwrap()[0].program, root.join("mvnw"));
    std::fs::remove_file(root.join("mvnw")).unwrap();

    let env = crate::build_tools::BuildEnv::default();
    let provider = JavaRun::in_folders(vec![tools.clone()]).with_env(env.clone());
    let runner_jdk = dir.path().join("runner-jdk");
    fake_jdk(&runner_jdk);
    let mut settings = crate::maven::settings::MavenSettings { offline: true, ..Default::default() };
    settings.runner.vm_options = "-Xmx1g".to_owned();
    settings.runner.jre = runner_jdk.display().to_string();
    settings.runner.properties.insert("flavour".to_owned(), "dev".to_owned());
    env.set_project_settings(crate::maven::settings::TOOL, &root, &settings).unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines[0].args[..5], ["-q", "-B", "-o", "-Dflavour=dev", "compile"]);
    assert!(lines[0].env.contains(&("MAVEN_OPTS".to_owned(), "-Xmx1g".to_owned())));
    assert!(
        lines[0].env.contains(&("JAVA_HOME".to_owned(), runner_jdk.display().to_string())),
        "Maven gets the Runner's JRE"
    );
    assert_eq!(lines[2].program, jdk.join("bin/java"), "the application keeps its own");

    settings.runner.delegate = false;
    env.set_project_settings(crate::maven::settings::TOOL, &root, &settings).unwrap();
    assert!(provider.command_lines(&configuration, &context).unwrap_err().contains("source"), "nothing to compile");
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    std::fs::write(root.join("src/main/java/shop/App.java"), APP).unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 4);
    assert!(!lines[0].args.contains(&"compile".to_owned()), "Maven only resolves: {:?}", lines[0].args);
    assert!(lines[1].args[3].ends_with("classes") && lines[1].args[4].ends_with("src/main/resources"));
    assert_eq!(lines[2].program, jdk.join("bin/javac"), "the IDE compiles");
    assert!(lines[2].args.contains(&classpath), "{:?}", lines[2].args);
    let generated = root.join("target/generated-sources/annotations").display().to_string();
    assert!(lines[2].args.windows(2).any(|pair| pair == ["-s", generated.as_str()]), "{:?}", lines[2].args);
    assert!(lines[2].args.contains(&"-proc:full".to_owned()), "processors run unless turned off");
    let sources = std::fs::read_to_string(context.scratch.join("sources.args")).unwrap();
    assert!(sources.contains("src/main/java/shop/App.java"), "{sources}");
    assert_eq!(lines[3].program, jdk.join("bin/java"));
    std::fs::remove_dir_all(root.join("src")).unwrap();
    let provider = JavaRun::in_folders(vec![tools.clone()]);

    std::fs::remove_file(root.join("pom.xml")).unwrap();
    std::fs::write(root.join("build.gradle"), "plugins { id 'java' }").unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].program, tools.join("gradle"));
    assert!(lines[0].args.iter().any(|arg| arg == "zenClasspath"));
    assert!(lines[0].args.iter().any(|arg| arg.starts_with("-PzenClasspathOut=")));
    assert!(
        std::fs::read_to_string(context.scratch.join("zen-classpath.gradle")).unwrap().contains("runtimeClasspath")
    );

    let env = crate::build_tools::BuildEnv::default();
    let gradle_provider = JavaRun::in_folders(vec![tools.clone()]).with_env(env.clone());
    let gradle_jvm = dir.path().join("gradle-jvm");
    fake_jdk(&gradle_jvm);
    let mut settings = crate::gradle::settings::GradleSettings {
        offline: true,
        vm_options: "-Xmx2g".to_owned(),
        user_home: "/caches/gradle".to_owned(),
        jvm: gradle_jvm.display().to_string(),
        ..Default::default()
    };
    env.set_project_settings(crate::gradle::settings::TOOL, &root, &settings).unwrap();
    let lines = gradle_provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].args[3..5], ["--offline", "-Dorg.gradle.jvmargs=-Xmx2g"]);
    assert!(lines[0].env.contains(&("GRADLE_USER_HOME".to_owned(), "/caches/gradle".to_owned())));
    assert!(lines[0].env.contains(&("JAVA_HOME".to_owned(), gradle_jvm.display().to_string())), "the Gradle JVM");
    assert!(!lines[0].args.iter().any(|arg| arg.starts_with("-PzenClasspathOnly")), "Gradle builds");
    assert!(lines[1].env.contains(&("JAVA_HOME".to_owned(), jdk.display().to_string())), "the application's JDK");

    settings.build_and_run = crate::gradle::settings::Runner::Ide;
    env.set_project_settings(crate::gradle::settings::TOOL, &root, &settings).unwrap();
    assert!(
        gradle_provider.command_lines(&configuration, &context).unwrap_err().contains("source"),
        "nothing to compile"
    );
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    std::fs::write(root.join("src/main/java/shop/App.java"), APP).unwrap();
    let lines = gradle_provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].args.contains(&"-PzenClasspathOnly=true".to_owned()), "Gradle only resolves");
    let own = std::env::join_paths([context.scratch.join("classes"), root.join("src/main/resources")]).unwrap();
    assert!(lines[0].args.contains(&format!("-PzenClasspathPrefix={}", own.to_string_lossy())), "{:?}", lines[0].args);
    assert_eq!(lines[1].program, jdk.join("bin/javac"), "the IDE compiles");
    assert!(lines[1].args.contains(&classpath), "{:?}", lines[1].args);
    let processors = context.scratch.join("processorpath.args");
    assert!(lines[0].args.contains(&format!("-PzenProcessorPathOut={}", processors.display())));
    let processor_path = format!("@{}", processors.display());
    assert!(lines[1].args.windows(2).any(|pair| pair == ["-processorpath", processor_path.as_str()]));
    assert!(
        std::fs::read_to_string(context.scratch.join("zen-classpath.gradle")).unwrap().contains("annotationProcessor")
    );
    assert_eq!(lines[2].program, jdk.join("bin/java"));
    std::fs::remove_dir_all(root.join("src")).unwrap();

    std::fs::remove_file(root.join("build.gradle")).unwrap();
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(root.join("src/shop/App.java"), APP).unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines[0].program, jdk.join("bin/javac"));
    let sources = std::fs::read_to_string(context.scratch.join("sources.args")).unwrap();
    assert!(sources.contains("src/shop/App.java"), "{sources}");
    let written = std::fs::read_to_string(context.scratch.join("classpath.args")).unwrap();
    assert_eq!(written, argument_file(&context.scratch.join("classes").display().to_string()));

    let empty = configured(APPLICATION, serde_json::json!({ "mainClass": "" }));
    assert!(provider.command_lines(&empty, &context).unwrap_err().contains("main class"));
    let mut elsewhere = configuration.clone();
    elsewhere.options["jre"] = serde_json::json!(dir.path().join("no-jdk").display().to_string());
    assert!(provider.command_lines(&elsewhere, &context).is_err());
}

pub(crate) fn kotlin_reader() -> crate::build_tools::BuildEnv {
    let env = crate::build_tools::BuildEnv::default();
    env.set_jvm_reader(Some(std::sync::Arc::new(|path: &Path, _: &str| {
        let main = zen_syntax::JvmMain { class: String::new(), class_line: 2, line: 2, end_line: 4 };
        let tool = zen_syntax::JvmMain { class: "shop.Tool".into(), class_line: 6, line: 8, end_line: 8 };
        let file = zen_syntax::JvmFile { package: "shop".into(), mains: vec![main, tool], ..Default::default() };
        Some(file.named_after(&path.file_stem()?.to_string_lossy()))
    })));
    env
}

const KOTLIN: &str = "package shop\n\nfun main() {\n    println(1)\n}\n\nobject Tool {\n    @JvmStatic\n    fun main(args: Array<String>) {}\n}\n";

#[test]
fn kotlin_mains_get_gutter_icons_and_are_built_by_maven_or_gradle() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("shop");
    std::fs::create_dir_all(root.join("src/main/kotlin/shop")).unwrap();
    let file = root.join("src/main/kotlin/shop/App.kt");
    std::fs::write(&file, KOTLIN).unwrap();
    std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    for tool in ["mvn", "gradle"] {
        std::fs::write(tools.join(tool), "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(tools.join(tool), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let env = kotlin_reader();
    let provider = JavaRun::in_folders(vec![tools.clone()]).with_env(env.clone());
    let mut context = context(dir.path());
    context.env = RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] };
    let targets = provider.targets(&context, &file, KOTLIN);
    let shown: Vec<(u32, &str, String)> = targets
        .iter()
        .map(|target| {
            let options: ApplicationOptions = serde_json::from_value(target.configuration.options.clone()).unwrap();
            (target.line, target.name.as_str(), format!("{} {}", options.main_class, options.module))
        })
        .collect();
    assert_eq!(
        shown,
        [
            (2, "AppKt", "shop.AppKt shop".to_owned()),
            (6, "Tool", "shop.Tool shop".to_owned()),
            (8, "Tool", "shop.Tool shop".to_owned())
        ]
    );
    let caret = KOTLIN.find("println").unwrap();
    assert_eq!(provider.context(dir.path(), &file, KOTLIN, caret).unwrap().name, "AppKt");
    assert!(JavaRun::default().targets(&context, &file, KOTLIN).is_empty(), "nothing without the Kotlin plug-in");

    let configuration = targets[0].configuration.clone();
    let mut settings = crate::maven::settings::MavenSettings::default();
    settings.runner.delegate = false;
    env.set_project_settings(crate::maven::settings::TOOL, dir.path(), &settings).unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 3, "Maven compiles, the IDE does not");
    assert!(lines[0].args.contains(&"compile".to_owned()), "{:?}", lines[0].args);
    assert!(lines[0].display.starts_with("The module has Kotlin sources, so Maven builds it"), "{}", lines[0].display);
    assert!(lines[1].args[3].ends_with("target/classes"));
    assert_eq!(lines[2].args.last().unwrap(), "shop.AppKt");

    std::fs::remove_file(root.join("pom.xml")).unwrap();
    std::fs::write(root.join("build.gradle.kts"), "plugins { kotlin(\"jvm\") }").unwrap();
    let gradle = crate::gradle::settings::GradleSettings {
        build_and_run: crate::gradle::settings::Runner::Ide,
        ..Default::default()
    };
    env.set_project_settings(crate::gradle::settings::TOOL, dir.path(), &gradle).unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 2, "Gradle builds, the IDE does not");
    assert!(!lines[0].args.iter().any(|arg| arg.starts_with("-PzenClasspathOnly")), "{:?}", lines[0].args);
    assert!(lines[0].display.starts_with("The module has Kotlin sources, so Gradle builds it"));

    std::fs::remove_file(root.join("build.gradle.kts")).unwrap();
    let error = provider.command_lines(&configuration, &context).unwrap_err();
    assert!(error.contains("Maven or Gradle"), "{error}");
}

#[test]
fn a_java_scratch_runs_as_a_single_source_file_with_the_classpath_only_when_asked() {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let scratch = dir.path().join("scratches/scratch.java");
    let text =
        "class Scratch {\n    public static void main(String[] args) {\n        System.out.println(1);\n    }\n}\n";
    let targets = JavaRun::default().single_file(&scratch, text);
    assert_eq!(targets.iter().map(|target| target.line).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(targets[0].configuration.name, "Scratch");
    assert_eq!(targets[0].configuration.kind, FILE);
    assert!(JavaRun::default().single_file(&scratch, "class Scratch {}\n").is_empty(), "no main, no ▶");
    assert!(JavaRun::default().single_file(&dir.path().join("scratch.kt"), "fun main() {}\n").is_empty());

    let root = dir.path().join("shop");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Lib.java"), "public class Lib {}\n").unwrap();
    let mut context = context(&root);
    context.env.env = vec![("JAVA_HOME".to_owned(), jdk.display().to_string())];
    let mut configuration = targets[0].configuration.clone();
    configuration.common.tool_arguments = "-ea".to_owned();
    configuration.common.program_arguments = "a b".to_owned();
    let lines = JavaRun::default().command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 1, "nothing is built first: {lines:?}");
    assert_eq!(lines[0].program, jdk.join("bin/java"));
    let file = scratch.display().to_string();
    assert_eq!(lines[0].args, ["-ea", file.as_str(), "a", "b"]);
    assert_eq!(lines[0].cwd, root);
    assert!(lines[0].env.contains(&("JAVA_HOME".to_owned(), jdk.display().to_string())), "the project JDK");
    assert!(!context.scratch.exists(), "no folder is made for a plain run");

    configuration.options["classpath"] = serde_json::json!(true);
    let lines = JavaRun::default().command_lines(&configuration, &context).unwrap();
    assert_eq!(lines.len(), 2, "javac builds the project first: {lines:?}");
    assert_eq!(lines[0].program, jdk.join("bin/javac"));
    let classpath = format!("@{}", context.scratch.join("classpath.args").display());
    assert_eq!(lines[1].args, ["-ea", "-cp", classpath.as_str(), file.as_str(), "a", "b"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&context.scratch).unwrap().permissions().mode() & 0o777, 0o700);
    }
    assert!(JavaRun::default().debug(&configuration, &context).is_none(), "debugging a scratch is not offered");
    assert!(JavaRun::default().command_lines(&configured(FILE, serde_json::json!({})), &context).is_err());
}

#[test]
fn the_class_at_a_line_is_the_innermost_type_around_it() {
    let file = Path::new("/p/src/main/java/shop/App.java");
    let at = |line: u32| class_at(file, APP, line);
    assert_eq!(at(0).as_deref(), Some("shop.App"), "before any type: the file's first class");
    assert_eq!(at(9).as_deref(), Some("shop.App"));
    assert_eq!(at(13).as_deref(), Some("shop.App$Inner"));
    assert_eq!(at(17).as_deref(), Some("shop.App"), "an anonymous class is not a type of its own");
    assert_eq!(at(26).as_deref(), Some("shop.App"));
    let two = "class A {\n}\nrecord B(int x) {\n  enum C { X }\n}\n";
    assert_eq!(class_at(Path::new("A.java"), two, 1).as_deref(), Some("A"));
    assert_eq!(class_at(Path::new("A.java"), two, 3).as_deref(), Some("B$C"));
    assert_eq!(class_at(Path::new("A.java"), two, 2).as_deref(), Some("B"));
    let implicit = "void main() {\n    IO.println(1);\n}\n";
    assert_eq!(class_at(Path::new("/s/Script.java"), implicit, 1).as_deref(), Some("Script"));
    assert_eq!(class_at(Path::new("/s/Empty.java"), "", 0), None);
}

fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn a_kotlin_scratch_is_compiled_by_kotlinc_and_run_by_the_project_jdk() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let scratch = dir.path().join("scratches/scratch.kt");
    let text = "fun main() {\n    println(1)\n}\n";
    let provider = JavaRun::in_folders(vec![tools.clone()]).with_env(kotlin_reader());
    let targets = provider.single_file(&scratch, text);
    assert_eq!(targets.iter().map(|target| target.line).collect::<Vec<_>>(), [2]);
    let mut configuration = targets[0].configuration.clone();
    assert_eq!((configuration.kind.as_str(), configuration.name.as_str()), (KOTLIN_FILE, "ScratchKt"));
    assert_eq!(configuration.options["mainClass"], "shop.ScratchKt");
    assert!(JavaRun::default().single_file(&scratch, text).is_empty(), "no Kotlin reader, no ▶");

    let root = dir.path().join("shop");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Lib.java"), "public class Lib {}\n").unwrap();
    let mut context = context(&root);
    context.env.env = vec![("JAVA_HOME".to_owned(), jdk.display().to_string())];
    let missing = provider.command_lines(&configuration, &context).unwrap_err();
    assert!(missing.contains("kotlinc") && missing.contains("does not ship or download"), "{missing}");
    assert!(provider.debug(&configuration, &context).is_none());

    executable(&tools.join("kotlinc"), "#!/bin/sh\n");
    configuration.common.program_arguments = "a".to_owned();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    let jar = context.scratch.join("kotlin-scratch.jar").display().to_string();
    let file = scratch.display().to_string();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].program, tools.join("kotlinc"));
    assert_eq!(lines[0].args, [file.as_str(), "-include-runtime", "-d", jar.as_str()]);
    assert_eq!(lines[1].program, jdk.join("bin/java"));
    assert_eq!(lines[1].args, ["-cp", jar.as_str(), "shop.ScratchKt", "a"]);

    configuration.options["classpath"] = serde_json::json!(true);
    let lines = provider.command_lines(&configuration, &context).unwrap();
    let programs: Vec<String> =
        lines.iter().map(|line| line.program.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(programs, ["javac", "kotlinc", "java", "java"], "{lines:?}");
    let classpath = format!("@{}", context.scratch.join("classpath.args").display());
    assert_eq!(lines[1].args[..2], ["-cp".to_owned(), classpath]);
    assert!(lines[2].args[0].ends_with("ZenClasspath.java") && lines[2].args[3] == jar, "{:?}", lines[2].args);
    let joined = format!("@{}", context.scratch.join("run-classpath.args").display());
    assert_eq!(lines[3].args, ["-cp", joined.as_str(), "shop.ScratchKt", "a"]);
}

#[test]
#[ignore = "runs the real java of the JDK named by ZEN_JDK_HOME"]
fn a_java_scratch_runs_with_a_real_jdk() {
    let jdk = crate::build_tools::real_tool_in_tests("ZEN_JDK_HOME");
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("Application Support/scratches/scratch.java");
    std::fs::create_dir_all(scratch.parent().unwrap()).unwrap();
    let text = "class Scratch {\n    public static void main(String[] args) {\n        System.out.println(\"hi \" + args[0]);\n    }\n}\n";
    std::fs::write(&scratch, text).unwrap();
    let mut configuration = JavaRun::default().single_file(&scratch, text)[0].configuration.clone();
    configuration.common.program_arguments = "there".to_owned();
    let mut context = context(dir.path());
    context.env.env = vec![("JAVA_HOME".to_owned(), jdk.display().to_string())];
    let lines = JavaRun::default().command_lines(&configuration, &context).unwrap();
    let line = &lines[0];
    let output = std::process::Command::new(&line.program).args(&line.args).current_dir(&line.cwd).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hi there\n");
}

#[test]
fn an_application_and_a_jar_run_with_the_jacoco_agent_in_their_jvm() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("shop");
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(root.join("src/shop/App.java"), APP).unwrap();
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let agent = dir.path().join("org.jacoco.agent-0.8.16-runtime.jar");
    std::fs::write(&agent, "jar").unwrap();
    let mut context = context(&root);
    context.env.env = vec![("JAVA_HOME".to_owned(), jdk.display().to_string())];
    let application = JavaRun::application("shop.App", "");
    let report = dir.path().join("cov/coverage.out");

    let without = JavaRun::default().with_jacoco(Jars::default());
    let coverage = without.coverage(&application).expect("applications run with coverage");
    assert_eq!(coverage.command_lines(&application, &context, &report), Err(jacoco::missing()));

    let provider = JavaRun::default().with_jacoco(Jars { agent: Some(agent), cli: None });
    let coverage = provider.coverage(&application).unwrap();
    let lines = coverage.command_lines(&application, &context, &report).unwrap();
    let copy = dir.path().join("cov/jacocoagent.jar");
    let option =
        format!("-javaagent:{}=destfile={},append=true", copy.display(), dir.path().join("cov/jacoco.exec").display());
    let run = lines.last().unwrap();
    assert_eq!(run.args[0], option);
    assert!(run.display.starts_with(&format!("java {option} -cp")), "{}", run.display);
    assert!(lines[0].args.contains(&dir.path().join("cov/build/classes").display().to_string()), "{:?}", lines[0].args);
    assert_eq!(
        coverage.read(&application, &context, &report),
        Err(t!("java-coverage-no-data")),
        "no data before the run wrote it"
    );

    let jar = configured(JAR, serde_json::json!({ "jar": "app.jar" }));
    let lines = provider.coverage(&jar).unwrap().command_lines(&jar, &context, &report).unwrap();
    assert_eq!(lines[0].args[..2], [option, "-jar".to_owned()]);
    assert!(JavaRun::default().coverage(&configured("go.test", serde_json::json!({}))).is_none());
}

#[test]
fn the_classpath_file_gives_the_folders_with_classes() {
    let dir = tempfile::tempdir().unwrap();
    let classes = dir.path().join("my \"app\"/classes");
    std::fs::create_dir_all(&classes).unwrap();
    let jar = dir.path().join("lib.jar");
    std::fs::write(&jar, "").unwrap();
    let joined = std::env::join_paths([&classes, &jar, &dir.path().join("gone")]).unwrap();
    let text = argument_file(&joined.to_string_lossy());
    assert_eq!(classpath_folders(&text), [classes]);
}

#[test]
fn a_jar_runs_with_java_jar() {
    let dir = tempfile::tempdir().unwrap();
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let mut configuration =
        configured(JAR, serde_json::json!({ "jar": "build/libs/shop.jar", "jre": jdk.display().to_string() }));
    configuration.common.tool_arguments = "-Xmx1g".to_owned();
    configuration.common.program_arguments = "serve".to_owned();
    let lines = JavaRun::default().command_lines(&configuration, &context(dir.path())).unwrap();
    let jar = dir.path().join("build/libs/shop.jar").display().to_string();
    assert_eq!(lines[0].args, ["-Xmx1g", "-jar", jar.as_str(), "serve"]);
    assert!(JavaRun::default().command_lines(&configured(JAR, serde_json::json!({})), &context(dir.path())).is_err());
    assert_eq!(argument_file(r#"C:\a "b""#), "\"C:\\\\a \\\"b\\\"\"\n");
}

#[test]
fn an_application_and_a_jar_are_debugged_as_they_run_with_java_debug_attached() {
    let dir = tempfile::tempdir().unwrap();
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let mut jar =
        configured(JAR, serde_json::json!({ "jar": "build/libs/shop.jar", "jre": jdk.display().to_string() }));
    jar.common.tool_arguments = "-Xmx1g".to_owned();
    let plan = JavaRun::default().debug(&jar, &context(dir.path())).unwrap().unwrap();
    assert!(plan.build.is_empty());
    let debuggee = plan.debuggee.unwrap();
    assert_eq!(debuggee.line.program, jdk.join("bin/java"));
    assert!(debuggee.line.args[0].starts_with("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y"));
    assert_eq!(debuggee.line.args[1..3], ["-Xmx1g", "-jar"]);
    assert_eq!(plan.launch["request"], "attach");
    assert_eq!(plan.adapter.server.unwrap().server, "jdtls", "java-debug runs in the Java plug-in's server");

    let root = dir.path().join("shop");
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(root.join("src/shop/App.java"), APP).unwrap();
    let mut context = context(&root);
    context.env = RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] };
    let application =
        configured(APPLICATION, serde_json::json!({ "mainClass": "shop.App", "jre": jdk.display().to_string() }));
    let plan = JavaRun::default().debug(&application, &context).unwrap().unwrap();
    assert_eq!(plan.build.len(), 1, "javac");
    assert_eq!(plan.build[0].program, jdk.join("bin/javac"));
    let debuggee = plan.debuggee.unwrap();
    assert!(debuggee.line.args.ends_with(&["shop.App".to_owned()]), "{:?}", debuggee.line.args);
    assert!(debuggee.line.args[0].starts_with("-agentlib:jdwp="));
    let empty = configured(APPLICATION, serde_json::json!({ "mainClass": "" }));
    assert!(JavaRun::default().debug(&empty, &context).unwrap().is_err());
}

#[test]
fn stack_frames_link_to_their_sources() {
    assert_eq!(trace::frame("\tat shop.App.run(App.java:42)"), Some(("shop.App".to_owned(), "App.java".to_owned())));
    assert_eq!(
        trace::frame("    at java.base/java.lang.Thread.run(Thread.java:1583)").map(|frame| frame.0),
        Some("java.lang.Thread".to_owned())
    );
    assert_eq!(trace::frame("at shop.App.run(Native Method)"), None);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("shop/src/main/java/shop")).unwrap();
    std::fs::write(root.join("shop/src/main/java/shop/App.java"), APP).unwrap();
    let mut parser = trace::TraceParser::new(root, "shop");
    let parsed = OutputParser::line(&mut parser, Stream::Stderr, "\tat shop.App$Inner.main(App.java:14)");
    let link = &parsed.console.unwrap().links[0];
    assert_eq!((link.file.clone(), link.line), (root.join("shop/src/main/java/shop/App.java"), 14));
    let parsed =
        OutputParser::line(&mut parser, Stream::Stderr, "\tat java.base/java.lang.Thread.run(Thread.java:1583)");
    assert_eq!(parsed.console.unwrap().links[0].file, PathBuf::from("Thread.java"), "not in the project: as written");
}

#[test]
fn shared_java_configurations_are_adopted() {
    let foreign = |type_id: &str, options: &[(&str, &str)]| ForeignRunConfiguration {
        source_file: PathBuf::from("/p/.run/App.run.xml"),
        name: "App".to_owned(),
        type_id: type_id.to_owned(),
        factory: None,
        options: options.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
        env: vec![("MODE".to_owned(), "dev".to_owned())],
        shared: true,
        before_launch: Vec::new(),
    };
    let application = JavaRun::default()
        .adopt(
            &foreign(
                "Application",
                &[
                    ("MAIN_CLASS_NAME", "shop.App"),
                    ("PROGRAM_PARAMETERS", "--port 1"),
                    ("VM_PARAMETERS", "-ea"),
                    ("WORKING_DIRECTORY", "/p/data"),
                ],
            ),
            Path::new("/p"),
        )
        .unwrap();
    assert_eq!(application.kind, APPLICATION);
    assert_eq!(application.options["mainClass"], "shop.App");
    assert_eq!(
        (application.common.program_arguments.as_str(), application.common.tool_arguments.as_str()),
        ("--port 1", "-ea")
    );
    assert_eq!(application.common.working_directory, Some(PathBuf::from("data")));
    let jar =
        JavaRun::default().adopt(&foreign("JarApplication", &[("JAR_PATH", "/p/app.jar")]), Path::new("/p")).unwrap();
    assert_eq!((jar.kind.as_str(), jar.options["jar"].as_str()), (JAR, Some("app.jar")));
    assert!(JavaRun::default().adopt(&foreign("GoApplicationRunConfiguration", &[]), Path::new("/p")).is_none());
}

#[test]
#[ignore = "runs a real JDK, Maven and Gradle named by ZEN_JDK_HOME, ZEN_MAVEN, ZEN_GRADLE and ZEN_GRADLE_JVM"]
fn java_run_real_tools() {
    let jdk = crate::build_tools::real_tool_in_tests("ZEN_JDK_HOME");
    let run = |provider: &JavaRun, configuration: &Configuration, context: &RunContext| -> String {
        let mut output = String::new();
        for line in provider.command_lines(configuration, context).unwrap() {
            let result = std::process::Command::new(&line.program)
                .args(&line.args)
                .current_dir(&line.cwd)
                .envs(line.env.iter().map(|(key, value)| (key.as_str(), value.as_str())))
                .output()
                .unwrap();
            output.push_str(&String::from_utf8_lossy(&result.stdout));
            output.push_str(&String::from_utf8_lossy(&result.stderr));
            assert!(result.status.success(), "{} failed:\n{output}", line.display);
        }
        output
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("my shop");
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(
        root.join("src/shop/App.java"),
        "package shop;\npublic class App {\n    public static void main(String[] args) {\n        System.out.println(\"hello \" + String.join(\",\", args));\n    }\n}\n",
    )
    .unwrap();
    let mut context = context(&root);
    context.env.env = vec![("JAVA_HOME".to_owned(), jdk.display().to_string())];
    let mut configuration = JavaRun::application("shop.App", "");
    configuration.common.program_arguments = "a b".to_owned();
    assert!(run(&JavaRun::default(), &configuration, &context).contains("hello a,b"));

    {
        let maven = crate::build_tools::real_tool_in_tests("ZEN_MAVEN");
        let project = dir.path().join("maven shop");
        std::fs::create_dir_all(project.join("src/main/java/shop")).unwrap();
        std::fs::write(
            project.join("pom.xml"),
            "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"><modelVersion>4.0.0</modelVersion>\
             <groupId>shop</groupId><artifactId>shop</artifactId><version>1</version>\
             <properties><maven.compiler.release>17</maven.compiler.release></properties>\
             <dependencies><dependency><groupId>commons-io</groupId><artifactId>commons-io</artifactId>\
             <version>2.16.1</version></dependency></dependencies></project>",
        )
        .unwrap();
        std::fs::write(
            project.join("src/main/java/shop/App.java"),
            "package shop;\npublic class App { public static void main(String[] a) { \
             System.out.println(\"maven ok \" + org.apache.commons.io.FilenameUtils.getExtension(\"a.txt\")); } }\n",
        )
        .unwrap();
        let maven_context = RunContext {
            root: project.clone(),
            scratch: dir.path().join("maven scratch"),
            env: RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] },
            ..RunContext::default()
        };
        let provider = JavaRun::in_folders(vec![maven.parent().unwrap().to_owned()]);
        let output = run(&provider, &JavaRun::application("shop.App", ""), &maven_context);
        assert!(output.contains("maven ok txt"), "{output}");
    }

    {
        let gradle = crate::build_tools::real_tool_in_tests("ZEN_GRADLE");
        let project = dir.path().join("gradle shop");
        std::fs::create_dir_all(project.join("src/main/java/shop")).unwrap();
        std::fs::write(project.join("settings.gradle"), "rootProject.name = 'shop'\n").unwrap();
        std::fs::write(project.join("build.gradle"), "plugins { id 'java' }\n").unwrap();
        std::fs::write(
            project.join("src/main/java/shop/App.java"),
            "package shop;\npublic class App { public static void main(String[] a) { System.out.println(\"gradle ok\"); } }\n",
        )
        .unwrap();
        let gradle_context = RunContext {
            root: project.clone(),
            scratch: dir.path().join("gradle scratch"),
            env: RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] },
            ..RunContext::default()
        };
        let provider = JavaRun::in_folders(vec![gradle.parent().unwrap().to_owned()]);
        assert!(run(&provider, &JavaRun::application("shop.App", ""), &gradle_context).contains("gradle ok"));

        std::fs::write(
            project.join("build.gradle"),
            "plugins { id 'java' }\nrepositories { mavenCentral() }\ndependencies {\n\
             testImplementation 'org.junit.jupiter:junit-jupiter:5.11.4'\n}\ntest { useJUnitPlatform() }\n",
        )
        .unwrap();
        std::fs::create_dir_all(project.join("src/test/java/shop")).unwrap();
        std::fs::write(
            project.join("src/test/java/shop/AppTest.java"),
            "package shop;\nimport org.junit.jupiter.api.Test;\nclass AppTest {\n    @Test void works() { System.out.println(\"tested\"); }\n\
             @Test void fails() { org.junit.jupiter.api.Assertions.assertEquals(1, 2); }\n\
             @org.junit.jupiter.params.ParameterizedTest @org.junit.jupiter.params.provider.ValueSource(ints = {1, 2})\n\
             void positive(int x) { org.junit.jupiter.api.Assertions.assertTrue(x > 0); }\n}\n",
        )
        .unwrap();
        let env = crate::build_tools::BuildEnv::default();
        env.set_folders(Some(vec![gradle.parent().unwrap().to_owned()]));
        let java = jdk.clone();
        env.set_jdk(Some(std::sync::Arc::new(move |_: &Path| Some(java.clone()))));
        let settings = crate::gradle::settings::GradleSettings {
            build_and_run: crate::gradle::settings::Runner::Ide,
            run_tests: crate::gradle::settings::Runner::Ide,
            vm_options: "-Xmx512m".to_owned(),
            ..Default::default()
        };
        env.set_project_settings(crate::gradle::settings::TOOL, &project, &settings).unwrap();
        let provider = JavaRun::in_folders(vec![gradle.parent().unwrap().to_owned()]).with_env(env.clone());
        assert!(run(&provider, &JavaRun::application("shop.App", ""), &gradle_context).contains("gradle ok"));
        assert!(gradle_context.scratch.join("classes/shop/App.class").is_file(), "the IDE compiled it");

        let tests = crate::java::JavaTestRun::new(env);
        let file = project.join("src/test/java/shop/AppTest.java");
        let text = std::fs::read_to_string(&file).unwrap();
        let class = tests.targets(&gradle_context, &file, &text).remove(0).configuration;
        let run_tests = |configuration: &Configuration| {
            let mut output = String::new();
            for line in tests.command_lines(configuration, &gradle_context).unwrap() {
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
        };
        let mut rerun = class.clone();
        rerun.options["scope"] = serde_json::json!("tests");
        rerun.options["tests"] = serde_json::json!("shop.AppTest#positive");
        let output = run_tests(&rerun);
        assert!(output.contains(r#""test":"positive(int)[2]","result":"SUCCESS""#), "{output}");
        assert!(!output.contains("works()"), "{output}");
        let output = run_tests(&class);
        assert!(
            output.contains(r#"{"event":"testFinished","suite":"shop.AppTest","test":"works()","result":"SUCCESS""#),
            "{output}"
        );
        assert!(output.contains(r#""test":"fails()","result":"FAILURE""#), "{output}");
        assert!(output.contains("tested"), "{output}");
    }

    let person = "package shop;\nimport lombok.Builder;\nimport lombok.Getter;\nimport lombok.extern.slf4j.Slf4j;\n\
                  @Getter @Builder @Slf4j\npublic class Person {\n    private final String name;\n\
                  public static void main(String[] a) {\n        Person person = Person.builder().name(\"lombok\").build();\n\
                  log.info(\"logged\");\n        System.out.println(\"built \" + person.getName());\n    }\n}\n";
    {
        let maven = crate::build_tools::real_tool_in_tests("ZEN_MAVEN");
        let project = dir.path().join("maven lombok");
        crate::jdk::Jdks::default().store().set_language_level(&project, Some(17));
        std::fs::create_dir_all(project.join("src/main/java/shop")).unwrap();
        std::fs::write(
            project.join("pom.xml"),
            "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"><modelVersion>4.0.0</modelVersion>\
             <groupId>shop</groupId><artifactId>shop</artifactId><version>1</version>\
             <properties><maven.compiler.release>17</maven.compiler.release></properties><dependencies>\
             <dependency><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId>\
             <version>1.18.38</version><scope>provided</scope></dependency>\
             <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.16</version></dependency>\
             </dependencies></project>",
        )
        .unwrap();
        std::fs::write(project.join("src/main/java/shop/Person.java"), person).unwrap();
        let env = crate::build_tools::BuildEnv::default();
        let mut settings = crate::maven::settings::MavenSettings::default();
        settings.runner.delegate = false;
        env.set_project_settings(crate::maven::settings::TOOL, &project, &settings).unwrap();
        let lombok_context = RunContext {
            root: project.clone(),
            scratch: dir.path().join("maven lombok scratch"),
            env: RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] },
            ..RunContext::default()
        };
        let provider = JavaRun::in_folders(vec![maven.parent().unwrap().to_owned()]).with_env(env);
        let output = run(&provider, &JavaRun::application("shop.Person", ""), &lombok_context);
        assert!(output.contains("built lombok"), "{output}");
        assert!(lombok_context.scratch.join("classes/shop/Person$PersonBuilder.class").is_file(), "javac ran Lombok");
    }

    {
        let gradle = crate::build_tools::real_tool_in_tests("ZEN_GRADLE");
        let project = dir.path().join("gradle lombok");
        crate::jdk::Jdks::default().store().set_language_level(&project, Some(17));
        std::fs::create_dir_all(project.join("src/main/java/shop")).unwrap();
        std::fs::write(project.join("settings.gradle"), "rootProject.name = 'shop'\n").unwrap();
        std::fs::write(
            project.join("build.gradle"),
            "plugins { id 'java' }\nrepositories { mavenCentral() }\ndependencies {\n\
             compileOnly 'org.projectlombok:lombok:1.18.38'\n    annotationProcessor 'org.projectlombok:lombok:1.18.38'\n\
             implementation 'org.slf4j:slf4j-api:2.0.16'\n}\n",
        )
        .unwrap();
        std::fs::write(project.join("src/main/java/shop/Person.java"), person).unwrap();
        let env = crate::build_tools::BuildEnv::default();
        let settings = crate::gradle::settings::GradleSettings {
            build_and_run: crate::gradle::settings::Runner::Ide,
            jvm: crate::build_tools::real_tool_in_tests("ZEN_GRADLE_JVM").display().to_string(),
            ..Default::default()
        };
        env.set_project_settings(crate::gradle::settings::TOOL, &project, &settings).unwrap();
        let lombok_context = RunContext {
            root: project.clone(),
            scratch: dir.path().join("gradle lombok scratch"),
            env: RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] },
            ..RunContext::default()
        };
        let provider = JavaRun::in_folders(vec![gradle.parent().unwrap().to_owned()]).with_env(env);
        let output = run(&provider, &JavaRun::application("shop.Person", ""), &lombok_context);
        assert!(output.contains("built lombok"), "{output}");
        let processors = std::fs::read_to_string(lombok_context.scratch.join("processorpath.args")).unwrap();
        assert!(processors.contains("lombok-1.18.38.jar"), "{processors}");
    }
}

#[test]
fn a_maven_application_with_lombok_compiles_with_its_jar_as_the_processor_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("shop");
    let jdk = dir.path().join("jdk");
    fake_jdk(&jdk);
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let mvn = tools.join("mvn");
    std::fs::write(&mvn, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mvn, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    std::fs::write(root.join("src/main/java/shop/App.java"), APP).unwrap();
    std::fs::write(
        root.join("pom.xml"),
        "<project><dependencies><dependency><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId>\
         <version>1.18.34</version><scope>provided</scope></dependency></dependencies></project>",
    )
    .unwrap();
    let repository = dir.path().join("m2");
    let lombok = repository.join("org/projectlombok/lombok/1.18.34/lombok-1.18.34.jar");
    std::fs::create_dir_all(lombok.parent().unwrap()).unwrap();
    std::fs::write(&lombok, "jar").unwrap();
    let env = crate::build_tools::BuildEnv::default();
    let mut settings = crate::maven::settings::MavenSettings {
        local_repository: repository.display().to_string(),
        ..Default::default()
    };
    settings.runner.delegate = false;
    env.set_project_settings(crate::maven::settings::TOOL, &root, &settings).unwrap();
    let provider = JavaRun::in_folders(vec![tools]).with_env(env.clone());
    let mut context = context(&root);
    context.env = RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] };
    let configuration =
        configured(APPLICATION, serde_json::json!({ "mainClass": "shop.App", "module": "", "build": true }));
    let lines = provider.command_lines(&configuration, &context).unwrap();
    let jar = lombok.display().to_string();
    assert_eq!(lines[1].args.last(), Some(&jar), "Lombok's annotations are on the classpath javac compiles with");
    assert!(lines[2].args.windows(2).any(|pair| pair == ["-processorpath", jar.as_str()]), "{:?}", lines[2].args);

    let off = crate::java::processing::ProcessingSettings { enabled: false, ..Default::default() };
    env.set_project_settings(crate::java::processing::TOOL, &root, &off).unwrap();
    let lines = provider.command_lines(&configuration, &context).unwrap();
    assert!(lines[2].args.contains(&"-proc:none".to_owned()));
    assert!(!lines[2].args.contains(&"-processorpath".to_owned()));
}

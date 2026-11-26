use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_run::{
    AdapterLine, AdapterViews, CommandLine, Configuration, DebugPlan, Debuggee, FieldKind, FieldSpec, KindSpec,
    OutputParser, RunContext, RunProvider, RunTarget, ServerAdapter, TargetKind, TestId,
};

use super::parser::JavaTestParser;
use super::scan::scan;
use crate::build_tools::BuildEnv;

pub const JUNIT: &str = "junit";
pub const TESTNG: &str = "testng";

pub const DEBUG_READY: &str = "Listening for transport dt_socket at address";

pub const JDTLS: &str = "jdtls";

pub const GRADLE_INIT_SCRIPT: &str = include_str!("zen-tests.gradle");

pub const IDE_LAUNCHER: &str = include_str!("ZenTests.java");

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TestOptions {
    pub scope: String,
    pub package: String,
    pub class: String,
    pub method: String,
    pub tests: String,
    pub build_tool: String,
    pub build_folder: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTool {
    Maven,
    Gradle,
}

#[derive(Clone, Debug, Default)]
pub struct JavaTestRun {
    env: BuildEnv,
    jacoco: Option<crate::java::jacoco::Jars>,
}

impl JavaTestRun {
    pub fn new(env: BuildEnv) -> Self {
        Self { env, jacoco: None }
    }

    pub fn with_jacoco(self, jars: crate::java::jacoco::Jars) -> Self {
        Self { jacoco: Some(jars), ..self }
    }

    pub(crate) fn jacoco(&self) -> crate::java::jacoco::Jars {
        self.jacoco.clone().unwrap_or_else(|| crate::java::jacoco::jars(&self.env))
    }

    pub(crate) fn env(&self) -> &BuildEnv {
        &self.env
    }
}

pub(crate) fn options(configuration: &Configuration) -> TestOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

fn choice(values: &[&str]) -> FieldKind {
    FieldKind::Choice(values.iter().map(|value| (*value).to_owned()).collect())
}

pub fn configuration(kind: &str, name: &str, options: &TestOptions) -> Configuration {
    Configuration {
        name: name.to_owned(),
        kind: kind.to_owned(),
        options: serde_json::to_value(options).unwrap_or_default(),
        ..Configuration::default()
    }
}

fn simple_name(class: &str) -> &str {
    let simple = class.rsplit('.').next().unwrap_or(class);
    simple.rsplit('$').next().unwrap_or(simple)
}

pub fn find_build(root: &Path, path: &Path) -> Option<(BuildTool, PathBuf)> {
    let folders: Vec<&Path> = path.ancestors().skip(1).take_while(|folder| folder.starts_with(root)).collect();
    let gradle = |folder: &Path| {
        ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"]
            .iter()
            .any(|name| folder.join(name).is_file())
    };
    let settings =
        |folder: &Path| ["settings.gradle", "settings.gradle.kts"].iter().any(|name| folder.join(name).is_file());
    for folder in &folders {
        if folder.join("pom.xml").is_file() {
            return Some((BuildTool::Maven, folder.to_path_buf()));
        }
        if gradle(folder) {
            let top = folders.iter().rev().find(|outer| settings(outer)).copied().unwrap_or(folder);
            return Some((BuildTool::Gradle, top.to_path_buf()));
        }
    }
    None
}

fn kotlin_tests(file: zen_syntax::JvmFile) -> super::scan::TestFile {
    super::scan::TestFile {
        package: file.package,
        classes: file
            .tests
            .into_iter()
            .map(|test| super::scan::TestClass {
                name: test.name,
                line: test.line,
                methods: test.methods.into_iter().map(|(name, line)| super::scan::TestMethod { name, line }).collect(),
                testng: test.testng,
            })
            .collect(),
    }
}

fn tool_of(options: &TestOptions, folder: &Path) -> Option<BuildTool> {
    match options.build_tool.as_str() {
        "maven" => Some(BuildTool::Maven),
        "gradle" => Some(BuildTool::Gradle),
        _ if folder.join("pom.xml").is_file() => Some(BuildTool::Maven),
        _ => find_build(folder, &folder.join("x")).map(|(tool, _)| tool),
    }
}

fn method_of(test: &str) -> &str {
    let first = test.split('/').next().unwrap_or(test);
    first.split('(').next().unwrap_or(first)
}

fn listed(tests: &str) -> Vec<(String, String)> {
    tests
        .split(',')
        .filter_map(|item| {
            let (class, method) = item.trim().split_once('#')?;
            (!class.is_empty() && !method.is_empty()).then(|| (class.to_owned(), method.to_owned()))
        })
        .collect()
}

fn maven_filter(options: &TestOptions) -> Result<Option<String>, String> {
    Ok(match options.scope.as_str() {
        "" | "all" => None,
        "package" => Some(format!("{}/**/*", options.package.replace('.', "/"))),
        "class" => Some(options.class.clone()),
        "method" => Some(format!("{}#{}", options.class, options.method)),
        "tests" => {
            let mut by_class: Vec<(String, Vec<String>)> = Vec::new();
            for (class, method) in listed(&options.tests) {
                match by_class.iter_mut().find(|(known, _)| *known == class) {
                    Some((_, methods)) => methods.push(method),
                    None => by_class.push((class, vec![method])),
                }
            }
            Some(
                by_class
                    .iter()
                    .map(|(class, methods)| format!("{class}#{}", methods.join("+")))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        }
        other => return Err(t!("java-test-unknown-scope", scope = other)),
    })
}

fn gradle_filters(options: &TestOptions) -> Result<Vec<String>, String> {
    Ok(match options.scope.as_str() {
        "" | "all" => Vec::new(),
        "package" => vec![format!("{}.*", options.package)],
        "class" => vec![options.class.clone()],
        "method" => vec![format!("{}.{}", options.class, options.method)],
        "tests" => listed(&options.tests).into_iter().map(|(class, method)| format!("{class}.{method}")).collect(),
        other => return Err(t!("java-test-unknown-scope", scope = other)),
    })
}

pub(crate) fn build_of(configuration: &Configuration, context: &RunContext) -> Result<(BuildTool, PathBuf), String> {
    let options = options(configuration);
    let folder = if options.build_folder.trim().is_empty() {
        context.root.clone()
    } else {
        context.root.join(&options.build_folder)
    };
    let tool = tool_of(&options, &folder).ok_or_else(|| t!("java-test-no-build"))?;
    Ok((tool, folder))
}

pub fn gradle_init_script() -> Result<PathBuf, String> {
    crate::gradle::init_script("zen-tests", GRADLE_INIT_SCRIPT)
}

fn ide_selectors(options: &TestOptions, roots: &Path) -> Result<Vec<String>, String> {
    Ok(match options.scope.as_str() {
        "" | "all" => vec![format!("roots:{}", roots.display())],
        "package" => vec![format!("package:{}", options.package)],
        "class" => vec![format!("class:{}", options.class)],
        "method" => vec![format!("method:{}#{}", options.class, options.method)],
        "tests" => {
            listed(&options.tests).into_iter().map(|(class, method)| format!("method:{class}#{method}")).collect()
        }
        other => return Err(t!("java-test-unknown-scope", scope = other)),
    })
}

impl JavaTestRun {
    pub fn test_line(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        debug_port: Option<u16>,
    ) -> Result<CommandLine, String> {
        let options = options(configuration);
        let (tool, folder) = build_of(configuration, context)?;
        let mut args: Vec<String> = Vec::new();
        let maven = crate::maven::settings::MavenSettings::read(&self.env, &context.root);
        let gradle = crate::gradle::settings::GradleSettings::read(&self.env, &context.root);
        let mut maven_env = Vec::new();
        let program = match tool {
            BuildTool::Maven => {
                let (settings_args, env) = crate::maven::settings_line(
                    &self.env,
                    &maven,
                    crate::maven::settings::Purpose::Test,
                    &context.root,
                    &self.env.folders("maven"),
                );
                maven_env = env;
                args.extend(settings_args);
                args.extend(
                    [
                        "-B",
                        "test",
                        "-Dsurefire.useFile=false",
                        "-Dsurefire.reportFormat=plain",
                        "-DtrimStackTrace=false",
                        "-DfailIfNoTests=false",
                        "-Dsurefire.failIfNoSpecifiedTests=false",
                    ]
                    .map(str::to_owned),
                );
                if let Some(filter) = maven_filter(&options)? {
                    args.push(format!("-Dtest={filter}"));
                }
                if debug_port.is_some() {
                    args.push("-DforkCount=0".to_owned());
                    let plugins = std::fs::read_to_string(folder.join("pom.xml"))
                        .map(|text| crate::maven::pom::test_plugins(&text))
                        .unwrap_or_default();
                    if maven.tests.system_properties {
                        args.extend(plugins.system_properties.iter().map(|(name, value)| format!("-D{name}={value}")));
                    }
                    if maven.tests.environment {
                        maven_env.extend(plugins.environment);
                    }
                    if maven.tests.arg_line && !plugins.arg_line.is_empty() {
                        let given = maven_env
                            .iter()
                            .rev()
                            .find(|(name, _)| name == "MAVEN_OPTS")
                            .map(|(_, value)| value.clone())
                            .unwrap_or_default();
                        maven_env
                            .push(("MAVEN_OPTS".to_owned(), format!("{given} {}", plugins.arg_line).trim().to_owned()));
                    }
                }
                crate::maven::settings::executable(
                    &maven,
                    &folder,
                    &context.root,
                    &self.env,
                    &self.env.folders("maven"),
                )
                .map(|found| found.path)
                .ok_or_else(|| t!("java-test-tool-missing", tool = "Maven"))?
            }
            BuildTool::Gradle => {
                let script = gradle_init_script()?;
                args.extend(gradle.args());
                args.extend(["--console=plain".to_owned(), "-I".to_owned(), script.to_string_lossy().into_owned()]);
                if let Some(port) = debug_port {
                    args.push(format!("-Dzen.debugPort={port}"));
                }
                args.extend(["test".to_owned(), "--rerun".to_owned()]);
                for filter in gradle_filters(&options)? {
                    args.extend(["--tests".to_owned(), filter]);
                }
                crate::gradle::settings::executable(
                    &gradle,
                    &folder,
                    &context.root,
                    &self.env,
                    &self.env.folders("gradle"),
                )
                .map(|found| found.path)
                .ok_or_else(|| t!("java-test-tool-missing", tool = "Gradle"))?
            }
        };
        args.extend(zen_run::args::split(&configuration.common.tool_arguments));
        let mut env = match tool {
            BuildTool::Maven => maven_env,
            BuildTool::Gradle => gradle.environment(&self.env, &context.root, &self.env.folders("gradle")),
        };
        env.extend(configuration.common.env.iter().cloned());
        if let (BuildTool::Maven, Some(port)) = (tool, debug_port) {
            let given = env
                .iter()
                .rev()
                .find(|(name, _)| name == "MAVEN_OPTS")
                .map(|(_, value)| value.clone())
                .or_else(|| std::env::var("MAVEN_OPTS").ok())
                .unwrap_or_default();
            let agent = format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:{port}");
            env.push(("MAVEN_OPTS".to_owned(), format!("{given} {agent}").trim().to_owned()));
        }
        let name = program
            .file_name()
            .map_or_else(|| program.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned());
        let display = std::iter::once(name).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        Ok(CommandLine { program, args, cwd: folder, env, display })
    }
}

fn command_line(program: PathBuf, args: Vec<String>, cwd: PathBuf, env: Vec<(String, String)>) -> CommandLine {
    let name = program
        .file_name()
        .map_or_else(|| program.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned());
    let display = std::iter::once(name).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
    CommandLine { program, args, cwd, env, display }
}

impl JavaTestRun {
    pub fn test_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        debug_port: Option<u16>,
    ) -> Result<Vec<CommandLine>, String> {
        let options = options(configuration);
        let folder = if options.build_folder.trim().is_empty() {
            context.root.clone()
        } else {
            context.root.join(&options.build_folder)
        };
        let gradle = crate::gradle::settings::GradleSettings::read(&self.env, &context.root);
        if tool_of(&options, &folder) != Some(BuildTool::Gradle)
            || gradle.run_tests != crate::gradle::settings::Runner::Ide
        {
            return Ok(vec![self.test_line(configuration, context, debug_port)?]);
        }
        let scratch = if context.scratch.as_os_str().is_empty() {
            std::env::temp_dir().join("zen-ide")
        } else {
            context.scratch.clone()
        };
        let write = |path: &Path, text: &str| {
            std::fs::create_dir_all(&scratch)
                .and_then(|()| std::fs::write(path, text))
                .map_err(|error| format!("{}: {error}", path.display()))
        };
        let init = scratch.join("zen-classpath.gradle");
        write(&init, crate::java_run::GRADLE_INIT)?;
        let launcher = scratch.join("ZenTests.java");
        write(&launcher, IDE_LAUNCHER)?;
        let classpath = scratch.join("test-classpath.args");
        let roots = scratch.join("test-roots.txt");
        let folders = self.env.folders("gradle");
        let program = crate::gradle::settings::executable(&gradle, &folder, &context.root, &self.env, &folders)
            .map(|found| found.path)
            .ok_or_else(|| t!("java-test-tool-missing", tool = "Gradle"))?;
        let mut args = vec!["-q".to_owned(), "--init-script".to_owned(), init.display().to_string()];
        args.extend(gradle.args());
        args.extend([
            format!("-PzenClasspathOut={}", classpath.display()),
            format!("-PzenTestRootsOut={}", roots.display()),
            ":zenTestClasspath".to_owned(),
        ]);
        let mut env = gradle.environment(&self.env, &context.root, &folders);
        env.extend(configuration.common.env.iter().cloned());
        let build = command_line(program, args, folder.clone(), env);
        let home = self.env.java_home(&context.root).ok_or_else(|| t!("workbench-java-no-jdk"))?;
        let java = home.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
        let mut args = zen_run::args::split(&configuration.common.tool_arguments);
        if let Some(port) = debug_port {
            args.push(format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:{port}"));
        }
        args.extend(["-cp".to_owned(), format!("@{}", classpath.display()), launcher.display().to_string()]);
        args.extend(ide_selectors(&options, &roots)?);
        let mut env = self.env.environment_with(Some(home), &[]);
        env.extend(configuration.common.env.iter().cloned());
        Ok(vec![build, command_line(java, args, folder, env)])
    }
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    listener.local_addr().map(|address| address.port()).map_err(|error| error.to_string())
}

pub fn server_adapter() -> AdapterLine {
    AdapterLine {
        line: CommandLine::default(),
        address_flag: String::new(),
        server: Some(ServerAdapter {
            server: JDTLS.to_owned(),
            command: "vscode.java.startDebugSession".to_owned(),
            resolve: Vec::new(),
            hot_code_replace: true,
        }),
        views: Some(debug_views()),
        builtin: None,
    }
}

pub const DEBUG_SETTINGS: &str = "vscode.java.updateDebugSettings";

fn debug_views() -> AdapterViews {
    let settings =
        |logical: bool| serde_json::Value::from(serde_json::json!({ "showLogicalStructure": logical }).to_string());
    AdapterViews { command: DEBUG_SETTINGS.to_owned(), logical: vec![settings(true)], raw: vec![settings(false)] }
}

impl RunProvider for JavaTestRun {
    fn id(&self) -> &str {
        JUNIT
    }

    fn kinds(&self) -> Vec<KindSpec> {
        let fields = || {
            vec![
                field("scope", "java-test-field-scope", choice(&["all", "package", "class", "method", "tests"])),
                field("package", "java-test-field-package", FieldKind::Text),
                field("class", "java-test-field-class", FieldKind::Text),
                field("method", "java-test-field-method", FieldKind::Text),
                field("tests", "java-test-field-tests", FieldKind::Text),
                field("buildTool", "java-test-field-build-tool", choice(&["auto", "maven", "gradle"])),
                field("buildFolder", "java-test-field-build-folder", FieldKind::Path),
            ]
        };
        let defaults = serde_json::to_value(TestOptions {
            scope: "all".to_owned(),
            build_tool: "auto".to_owned(),
            ..TestOptions::default()
        })
        .unwrap_or_default();
        vec![
            KindSpec {
                id: JUNIT.to_owned(),
                name_message: "java-test-kind-junit".to_owned(),
                fields: fields(),
                defaults: defaults.clone(),
            },
            KindSpec {
                id: TESTNG.to_owned(),
                name_message: "java-test-kind-testng".to_owned(),
                fields: fields(),
                defaults,
            },
        ]
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        let file = match path.extension().and_then(|extension| extension.to_str()) {
            Some("java") => scan(text),
            Some("kt") => self.env.jvm_file(path, text).map(kotlin_tests).unwrap_or_default(),
            _ => return Vec::new(),
        };
        if file.classes.is_empty() {
            return Vec::new();
        }
        let build_folder = find_build(&context.root, path)
            .and_then(|(_, folder)| {
                folder.strip_prefix(&context.root).ok().map(|relative| relative.to_string_lossy().replace('\\', "/"))
            })
            .unwrap_or_default();
        let mut targets = Vec::new();
        for class in &file.classes {
            let kind = if class.testng { TESTNG } else { JUNIT };
            let base = TestOptions {
                class: class.name.clone(),
                build_tool: "auto".to_owned(),
                build_folder: build_folder.clone(),
                ..TestOptions::default()
            };
            let simple = simple_name(&class.name);
            targets.push(RunTarget {
                line: class.line,
                kind: TargetKind::Test,
                name: class.name.clone(),
                package: class.name.clone(),
                configuration: configuration(kind, simple, &TestOptions { scope: "class".to_owned(), ..base.clone() }),
            });
            for method in &class.methods {
                let options = TestOptions { scope: "method".to_owned(), method: method.name.clone(), ..base.clone() };
                targets.push(RunTarget {
                    line: method.line,
                    kind: TargetKind::Test,
                    name: method.name.clone(),
                    package: class.name.clone(),
                    configuration: configuration(kind, &format!("{simple}.{}", method.name), &options),
                });
            }
        }
        targets
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        let context = RunContext { root: root.to_owned(), ..RunContext::default() };
        let line = u32::try_from(text.get(..offset.min(text.len()))?.matches('\n').count()).ok()?;
        let targets = self.targets(&context, path, text);
        targets
            .into_iter()
            .filter(|target| target.line <= line)
            .max_by_key(|target| target.line)
            .map(|target| target.configuration)
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        self.test_lines(configuration, context, None)
    }

    fn parser(&self, _configuration: &Configuration, context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(JavaTestParser::new(&context.root))
    }

    fn coverage(&self, configuration: &Configuration) -> Option<&dyn zen_run::CoverageProvider> {
        [JUNIT, TESTNG].contains(&configuration.kind.as_str()).then_some(self as &dyn zen_run::CoverageProvider)
    }

    fn rerun_failed(&self, configuration: &Configuration, failed: &[TestId]) -> Option<Configuration> {
        let mut tests: Vec<String> =
            failed.iter().map(|test| format!("{}#{}", test.package, method_of(&test.test))).collect();
        tests.dedup();
        if tests.is_empty() {
            return None;
        }
        let options = TestOptions { scope: "tests".to_owned(), tests: tests.join(","), ..options(configuration) };
        Some(Configuration { options: serde_json::to_value(options).unwrap_or_default(), ..configuration.clone() })
    }

    fn debug(&self, configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        Some(free_port().and_then(|port| {
            let mut build = self.test_lines(configuration, context, Some(port))?;
            let line = build.pop().ok_or_else(|| t!("java-test-no-build"))?;
            Ok(DebugPlan {
                build,
                adapter: server_adapter(),
                launch: super::debug::attach_launch(port, &context.root),
                debuggee: Some(Debuggee { line, ready: DEBUG_READY.to_owned(), agent: None }),
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST: &str = "package com.acme;\n\nimport org.junit.jupiter.api.Test;\n\nclass CalcTest {\n    @Test\n    void adds() {}\n\n    @Test\n    void fails() {}\n}\n";

    fn project(build: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (name, text) in build {
            let path = root.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    fn provider(folders: Vec<PathBuf>) -> JavaTestRun {
        let env = BuildEnv::default();
        env.set_folders(Some(folders));
        JavaTestRun::new(env)
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

    #[test]
    fn gutter_targets_run_a_class_or_a_method_with_maven() {
        let root = project(&[("app/pom.xml", "<project/>"), ("app/src/test/java/com/acme/CalcTest.java", TEST)]);
        let provider = provider(vec![tools(root.path())]);
        let context = RunContext { root: root.path().to_owned(), ..RunContext::default() };
        let path = root.path().join("app/src/test/java/com/acme/CalcTest.java");
        let targets = provider.targets(&context, &path, TEST);
        let lines: Vec<(u32, &str, &str)> = targets
            .iter()
            .map(|target| (target.line, target.name.as_str(), target.configuration.name.as_str()))
            .collect();
        assert_eq!(
            lines,
            [(4, "com.acme.CalcTest", "CalcTest"), (6, "adds", "CalcTest.adds"), (9, "fails", "CalcTest.fails")]
        );
        assert_eq!(targets[1].configuration.kind, JUNIT);
        let line = provider.test_line(&targets[1].configuration, &context, None).unwrap();
        assert_eq!(line.cwd, root.path().join("app"));
        assert!(line.args.contains(&"-Dtest=com.acme.CalcTest#adds".to_owned()), "{:?}", line.args);
        assert!(line.args.contains(&"-Dsurefire.reportFormat=plain".to_owned()));
        let caret = TEST.find("void fails").unwrap();
        assert_eq!(provider.context(root.path(), &path, TEST, caret).unwrap().name, "CalcTest.fails");
        assert!(provider.context(root.path(), &path, TEST, 0).is_none());
        let plan = provider.debug(&targets[0].configuration, &context).unwrap().unwrap();
        let debuggee = plan.debuggee.unwrap();
        assert!(debuggee.line.args.contains(&"-DforkCount=0".to_owned()), "{:?}", debuggee.line.args);
        let opts = debuggee.line.env.iter().find(|(name, _)| name == "MAVEN_OPTS").unwrap();
        assert!(opts.1.contains("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y"), "{opts:?}");
        assert_eq!(plan.launch["request"], "attach");
        assert_eq!(plan.adapter.server.unwrap().command, "vscode.java.startDebugSession");
    }

    #[test]
    fn gradle_runs_with_the_init_script_and_rerun_failed_lists_the_tests() {
        let root = project(&[
            ("settings.gradle", "include 'core'"),
            ("core/build.gradle", "plugins { id 'java' }"),
            ("core/src/test/java/com/acme/CalcTest.java", TEST),
        ]);
        let provider = provider(vec![tools(root.path())]);
        let context = RunContext { root: root.path().to_owned(), ..RunContext::default() };
        let path = root.path().join("core/src/test/java/com/acme/CalcTest.java");
        let class = provider.targets(&context, &path, TEST).remove(0).configuration;
        assert_eq!(options(&class).build_folder, "", "the build with the settings file");
        let line = provider.test_line(&class, &context, Some(5005)).unwrap();
        let script = line.args.iter().position(|arg| arg == "-I").map(|at| PathBuf::from(&line.args[at + 1])).unwrap();
        assert!(std::fs::read_to_string(script).unwrap().contains("##zen-test"));
        assert!(
            line.args.ends_with(&["test".into(), "--rerun".into(), "--tests".into(), "com.acme.CalcTest".into()]),
            "{:?}",
            line.args
        );
        assert!(line.args.contains(&"-Dzen.debugPort=5005".to_owned()));

        let failed = [
            TestId { package: "com.acme.CalcTest".into(), test: "fails".into() },
            TestId { package: "com.acme.CalcTest".into(), test: "positive(int)/[2]".into() },
            TestId { package: "com.acme.OrderTest".into(), test: "ships".into() },
        ];
        let rerun = provider.rerun_failed(&class, &failed).unwrap();
        assert_eq!(
            options(&rerun).tests,
            "com.acme.CalcTest#fails,com.acme.CalcTest#positive,com.acme.OrderTest#ships"
        );
        assert_eq!(
            gradle_filters(&options(&rerun)).unwrap(),
            ["com.acme.CalcTest.fails", "com.acme.CalcTest.positive", "com.acme.OrderTest.ships"]
        );
        assert_eq!(
            maven_filter(&options(&rerun)).unwrap().unwrap(),
            "com.acme.CalcTest#fails+positive,com.acme.OrderTest#ships"
        );
        let package = TestOptions { scope: "package".into(), package: "com.acme".into(), ..TestOptions::default() };
        assert_eq!(maven_filter(&package).unwrap().unwrap(), "com/acme/**/*");
        assert_eq!(gradle_filters(&package).unwrap(), ["com.acme.*"]);
    }

    #[test]
    fn maven_test_runs_get_the_maven_settings_and_a_debugged_one_the_surefire_arguments() {
        let surefire = "<project><build><plugins><plugin><artifactId>maven-surefire-plugin</artifactId><configuration>\
            <argLine>-Xss4m</argLine><systemPropertyVariables><db>h2</db></systemPropertyVariables>\
            <environmentVariables><TZ>UTC</TZ></environmentVariables></configuration></plugin></plugins></build></project>";
        let root = project(&[("pom.xml", surefire), ("src/test/java/com/acme/CalcTest.java", TEST)]);
        let env = BuildEnv::default();
        env.set_folders(Some(vec![tools(root.path())]));
        let jre = root.path().join("jre");
        std::fs::create_dir_all(jre.join("bin")).unwrap();
        let mut settings =
            crate::maven::settings::MavenSettings { offline: true, threads: "2".into(), ..Default::default() };
        settings.runner.vm_options = "-Xmx1g".into();
        settings.runner.jre = jre.display().to_string();
        settings.runner.properties.insert("profile".into(), "ci".into());
        settings.runner.environment.insert("SHOP".into(), "1".into());
        env.set_project_settings(crate::maven::settings::TOOL, root.path(), &settings).unwrap();
        let provider = JavaTestRun::new(env.clone());
        let context = RunContext { root: root.path().to_owned(), ..RunContext::default() };
        let path = root.path().join("src/test/java/com/acme/CalcTest.java");
        let class = provider.targets(&context, &path, TEST).remove(0).configuration;
        let var = |line: &CommandLine, name: &str| {
            line.env.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.clone()).unwrap_or_default()
        };
        let line = provider.test_line(&class, &context, None).unwrap();
        assert_eq!(line.args[..5], ["-o", "-T", "2", "-Dprofile=ci", "-B"]);
        assert_eq!(var(&line, "MAVEN_OPTS"), "-Xmx1g");
        assert_eq!(var(&line, "SHOP"), "1");
        assert_eq!(var(&line, "JAVA_HOME"), jre.display().to_string());
        assert!(!line.args.contains(&"-Ddb=h2".to_owned()), "a forked test gets them from Surefire");

        let debugged = provider.test_line(&class, &context, Some(5005)).unwrap();
        assert!(debugged.args.contains(&"-Ddb=h2".to_owned()), "{:?}", debugged.args);
        assert_eq!(var(&debugged, "TZ"), "UTC");
        let opts = var(&debugged, "MAVEN_OPTS");
        assert!(opts.starts_with("-Xmx1g -Xss4m -agentlib:jdwp="), "{opts}");

        settings.tests =
            crate::maven::settings::RunningTests { arg_line: false, system_properties: false, environment: false };
        env.set_project_settings(crate::maven::settings::TOOL, root.path(), &settings).unwrap();
        let debugged = provider.test_line(&class, &context, Some(5005)).unwrap();
        assert!(!debugged.args.contains(&"-Ddb=h2".to_owned()));
        assert_eq!(var(&debugged, "TZ"), "");
        assert!(!var(&debugged, "MAVEN_OPTS").contains("-Xss4m"));
    }

    #[test]
    fn gradle_test_runs_get_the_gradle_settings_and_the_ide_runs_them_when_asked() {
        let root = project(&[
            ("settings.gradle", "include 'core'"),
            ("core/build.gradle", "plugins { id 'java' }"),
            ("core/src/test/java/com/acme/CalcTest.java", TEST),
        ]);
        let env = BuildEnv::default();
        let bin = tools(root.path());
        env.set_folders(Some(vec![bin.clone()]));
        let jdk = root.path().join("jdk");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        let project_jdk = jdk.clone();
        env.set_jdk(Some(std::sync::Arc::new(move |_: &Path| Some(project_jdk.clone()))));
        let gradle_jvm = root.path().join("gradle-jvm");
        std::fs::create_dir_all(gradle_jvm.join("bin")).unwrap();
        let mut settings = crate::gradle::settings::GradleSettings {
            offline: true,
            vm_options: "-Xmx2g".into(),
            user_home: "/caches/gradle".into(),
            jvm: gradle_jvm.display().to_string(),
            ..Default::default()
        };
        env.set_project_settings(crate::gradle::settings::TOOL, root.path(), &settings).unwrap();
        let provider = JavaTestRun::new(env.clone());
        let context =
            RunContext { root: root.path().to_owned(), scratch: root.path().join("scratch"), ..RunContext::default() };
        let path = root.path().join("core/src/test/java/com/acme/CalcTest.java");
        let targets = provider.targets(&context, &path, TEST);
        let var = |line: &CommandLine, name: &str| {
            line.env.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.clone()).unwrap_or_default()
        };
        let lines = provider.command_lines(&targets[0].configuration, &context).unwrap();
        assert_eq!(lines.len(), 1, "Gradle runs them");
        assert_eq!(lines[0].args[..2], ["--offline", "-Dorg.gradle.jvmargs=-Xmx2g"]);
        assert_eq!(var(&lines[0], "GRADLE_USER_HOME"), "/caches/gradle");
        assert_eq!(var(&lines[0], "JAVA_HOME"), gradle_jvm.display().to_string());

        settings.run_tests = crate::gradle::settings::Runner::Ide;
        env.set_project_settings(crate::gradle::settings::TOOL, root.path(), &settings).unwrap();
        let lines = provider.command_lines(&targets[2].configuration, &context).unwrap();
        assert_eq!(lines.len(), 2, "Gradle resolves, the IDE runs");
        assert_eq!(lines[0].program, bin.join("gradle"));
        assert_eq!(lines[0].args.last().unwrap(), ":zenTestClasspath");
        assert!(lines[0].args.contains(&"--offline".to_owned()), "{:?}", lines[0].args);
        assert_eq!(var(&lines[0], "JAVA_HOME"), gradle_jvm.display().to_string());
        let init = std::fs::read_to_string(context.scratch.join("zen-classpath.gradle")).unwrap();
        assert!(init.contains("zenTestClasspath") && init.contains("junit-platform-launcher"), "{init}");
        assert_eq!(lines[1].program, jdk.join("bin/java"), "the project's JDK runs the tests");
        let classpath = format!("@{}", context.scratch.join("test-classpath.args").display());
        let launcher = context.scratch.join("ZenTests.java").display().to_string();
        assert_eq!(lines[1].args, ["-cp", classpath.as_str(), launcher.as_str(), "method:com.acme.CalcTest#fails"]);
        assert!(std::fs::read_to_string(&launcher).unwrap().contains("##zen-test"));
        assert_eq!(var(&lines[1], "JAVA_HOME"), jdk.display().to_string());

        let all = configuration(JUNIT, "all", &TestOptions { scope: "all".into(), ..TestOptions::default() });
        let lines = provider.command_lines(&all, &context).unwrap();
        let roots = format!("roots:{}", context.scratch.join("test-roots.txt").display());
        assert_eq!(lines[1].args.last(), Some(&roots));

        let plan = provider.debug(&targets[0].configuration, &context).unwrap().unwrap();
        assert_eq!(plan.build.len(), 1);
        let debuggee = plan.debuggee.unwrap().line;
        assert!(debuggee.args[0].starts_with("-agentlib:jdwp=transport=dt_socket,server=y"), "{:?}", debuggee.args);
        assert_eq!(debuggee.args.last().unwrap(), "class:com.acme.CalcTest");
    }

    #[test]
    fn kotlin_test_classes_run_like_java_ones() {
        let text = "package com.acme\n\nclass CalcTest {\n    @Test\n    fun adds() {}\n}\n";
        let root = project(&[("pom.xml", "<project/>"), ("src/test/kotlin/com/acme/CalcTest.kt", text)]);
        let env = BuildEnv::default();
        env.set_folders(Some(vec![tools(root.path())]));
        env.set_jvm_reader(Some(std::sync::Arc::new(|_: &Path, _: &str| {
            let test = zen_syntax::JvmTestClass {
                name: "com.acme.CalcTest".into(),
                line: 2,
                testng: false,
                methods: vec![("adds".into(), 4)],
            };
            Some(zen_syntax::JvmFile { package: "com.acme".into(), tests: vec![test], ..Default::default() })
        })));
        let provider = JavaTestRun::new(env);
        let context = RunContext { root: root.path().to_owned(), ..RunContext::default() };
        let path = root.path().join("src/test/kotlin/com/acme/CalcTest.kt");
        let targets = provider.targets(&context, &path, text);
        let shown: Vec<(u32, &str, &str)> = targets
            .iter()
            .map(|target| (target.line, target.name.as_str(), target.configuration.name.as_str()))
            .collect();
        assert_eq!(shown, [(2, "com.acme.CalcTest", "CalcTest"), (4, "adds", "CalcTest.adds")]);
        let line = provider.test_line(&targets[1].configuration, &context, None).unwrap();
        assert!(line.args.contains(&"-Dtest=com.acme.CalcTest#adds".to_owned()), "{:?}", line.args);
        let caret = text.find("fun adds").unwrap();
        assert_eq!(provider.context(root.path(), &path, text, caret).unwrap().name, "CalcTest.adds");
        assert!(JavaTestRun::new(BuildEnv::default()).targets(&context, &path, text).is_empty());
        assert_eq!(super::super::parser::source_roots(root.path()), [root.path().join("src/test/kotlin")]);
        let plan = provider.debug(&targets[0].configuration, &context).unwrap().unwrap();
        let sources = root.path().join("src/test/kotlin").to_string_lossy().into_owned();
        assert_eq!(plan.launch["sourcePaths"], serde_json::json!([sources]));
    }

    #[test]
    fn a_project_without_a_build_says_so() {
        let root = project(&[("src/CalcTest.java", TEST)]);
        let provider = provider(Vec::new());
        let context = RunContext { root: root.path().to_owned(), ..RunContext::default() };
        let class = provider.targets(&context, &root.path().join("src/CalcTest.java"), TEST).remove(0).configuration;
        assert!(provider.command_lines(&class, &context).is_err());
    }
}

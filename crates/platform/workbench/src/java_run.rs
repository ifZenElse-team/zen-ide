mod adopt;
mod source;
#[cfg(test)]
pub(crate) mod tests;
mod trace;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, DebugPlan, FieldKind, FieldSpec, ForeignRunConfiguration, KindSpec, OutputParser,
    RunContext, RunProvider, RunTarget, TargetKind,
};

pub use source::{MainMethod, class_at, main_methods};

use crate::java::jacoco::{self, Jars};

pub const APPLICATION: &str = "java.application";

pub const JAR: &str = "java.jar";

pub const FILE: &str = "java.file";

pub const KOTLIN_FILE: &str = "kotlin.file";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApplicationOptions {
    pub main_class: String,
    pub module: String,
    pub jre: String,
    pub build: bool,
    pub coroutine_debug: String,
    pub track_coroutines: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JarOptions {
    pub jar: String,
    pub jre: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileOptions {
    pub file: String,
    pub jre: String,
    pub classpath: bool,
    pub module: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KotlinFileOptions {
    pub file: String,
    pub main_class: String,
    pub compiler: String,
    pub jre: String,
    pub classpath: bool,
    pub module: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTool {
    Maven,
    Gradle,
    Javac,
}

pub fn build_tool(folder: &Path) -> BuildTool {
    if folder.join("pom.xml").is_file() {
        BuildTool::Maven
    } else if ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"]
        .iter()
        .any(|name| folder.join(name).is_file())
    {
        BuildTool::Gradle
    } else {
        BuildTool::Javac
    }
}

pub fn module_of(root: &Path, file: &Path) -> String {
    let mut folder = file.parent();
    while let Some(current) = folder {
        if !current.starts_with(root) {
            break;
        }
        if build_tool(current) != BuildTool::Javac {
            return relative(root, current);
        }
        folder = current.parent();
    }
    String::new()
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|inside| inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .unwrap_or_else(|_| path.display().to_string())
}

#[derive(Clone, Debug, Default)]
pub struct JavaRun {
    folders: Option<Vec<PathBuf>>,
    env: Option<crate::build_tools::BuildEnv>,
    jacoco: Option<Jars>,
}

impl JavaRun {
    pub fn in_folders(folders: Vec<PathBuf>) -> Self {
        Self { folders: Some(folders), env: None, jacoco: None }
    }

    pub fn with_env(mut self, env: crate::build_tools::BuildEnv) -> Self {
        self.env = Some(env);
        self
    }

    fn build_env(&self) -> crate::build_tools::BuildEnv {
        self.env.clone().unwrap_or_else(crate::build_tools::BuildEnv::global)
    }

    pub fn with_jacoco(self, jars: Jars) -> Self {
        Self { jacoco: Some(jars), ..self }
    }

    fn jacoco(&self) -> Jars {
        self.jacoco.clone().unwrap_or_else(|| jacoco::jars(&self.build_env()))
    }

    pub fn kotlinc(&self) -> Option<PathBuf> {
        let folders = self.folders.clone().unwrap_or_else(|| {
            let mut folders: Vec<PathBuf> = std::env::var_os("KOTLIN_HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join("bin"))
                .into_iter()
                .collect();
            folders.extend(self.build_env().folders("kotlin"));
            folders
        });
        zen_lsp::servers::find_executable("kotlinc", &folders)
    }

    pub fn project_classpath(
        &self,
        module: &str,
        jre: &str,
        context: &RunContext,
    ) -> Result<(PathBuf, Vec<CommandLine>, PathBuf), String> {
        let home = jdk_home(jre, context)?;
        let java = program(&home, "java");
        if !java.is_file() {
            return Err(t!("workbench-java-no-java", path = home.display().to_string()));
        }
        let env = environment(&home, &Configuration::default(), context);
        let options = ApplicationOptions {
            module: module.to_owned(),
            jre: jre.to_owned(),
            build: true,
            ..ApplicationOptions::default()
        };
        let (lines, classpath) = self.classpath_lines(&options, context, &home, &java, &env)?;
        Ok((home, lines, classpath))
    }

    pub fn application(main_class: &str, module: &str) -> Configuration {
        let name = main_class.rsplit(['.', '$']).next().unwrap_or(main_class).to_owned();
        let options = ApplicationOptions {
            main_class: main_class.to_owned(),
            module: module.to_owned(),
            build: true,
            ..ApplicationOptions::default()
        };
        Configuration {
            name,
            kind: APPLICATION.to_owned(),
            options: serde_json::to_value(options).unwrap_or_default(),
            ..Configuration::default()
        }
    }

    pub(crate) fn mains(&self, path: &Path, text: &str) -> Vec<MainMethod> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("java") => main_methods(path, text),
            Some("kt") => self
                .build_env()
                .jvm_file(path, text)
                .map(|file| {
                    file.mains
                        .into_iter()
                        .map(|main| MainMethod {
                            class: main.class,
                            class_line: main.class_line,
                            line: main.line,
                            end_line: main.end_line,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn processing(
        &self,
        root: &Path,
        module: &Path,
        tool: BuildTool,
        gradle_processors: Option<&Path>,
        home: &Path,
    ) -> Result<crate::java::processing::Javac, String> {
        let major = crate::language::servers::archive::java_version(home);
        crate::java::processing::javac(&self.build_env(), root, module, tool, gradle_processors, major)
    }

    fn tool(&self, tool: BuildTool, folder: &Path, root: &Path) -> Result<PathBuf, String> {
        let (candidate, program): (&str, &str) = match tool {
            BuildTool::Maven => ("maven", "mvn"),
            BuildTool::Gradle => ("gradle", "gradle"),
            BuildTool::Javac => return Err(String::new()),
        };
        let env = self.build_env();
        let folders = self.folders.clone().unwrap_or_else(|| env.folders(candidate));
        let found = match tool {
            BuildTool::Maven => crate::maven::settings::executable(
                &crate::maven::settings::MavenSettings::read(&env, root),
                folder,
                root,
                &env,
                &folders,
            ),
            _ => crate::gradle::settings::executable(
                &crate::gradle::settings::GradleSettings::read(&env, root),
                folder,
                root,
                &env,
                &folders,
            ),
        };
        found.map(|found| found.path).ok_or_else(|| t!("workbench-java-tool-missing", program = program))
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

pub(crate) fn jdk_home(jre: &str, context: &RunContext) -> Result<PathBuf, String> {
    if !jre.trim().is_empty() {
        let home = PathBuf::from(jre.trim());
        let home = if home.join("Contents/Home/bin").is_dir() { home.join("Contents/Home") } else { home };
        return Ok(home);
    }
    if let Some((_, home)) = context.env.env.iter().find(|(key, _)| key == "JAVA_HOME") {
        return Ok(PathBuf::from(home));
    }
    crate::jdk::project_home(&context.root).ok_or_else(|| t!("workbench-java-no-jdk"))
}

fn program(home: &Path, name: &str) -> PathBuf {
    home.join("bin").join(if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() })
}

fn environment(home: &Path, configuration: &Configuration, context: &RunContext) -> Vec<(String, String)> {
    let mut env = context.env.env.clone();
    let jdk = crate::jdk::ProjectJdk {
        jdk: crate::jdk::Jdk {
            home: home.to_owned(),
            version: String::new(),
            major: 0,
            vendor: String::new(),
            source: crate::jdk::JdkSource::Added,
            compiler: true,
        },
        chosen: true,
        language_level: 0,
        language_level_default: true,
        build_level: None,
    };
    crate::jdk::with_project_jdk(&mut env, Some(&jdk));
    env.extend(configuration.common.env.iter().cloned());
    env
}

fn line(program: PathBuf, args: Vec<String>, cwd: PathBuf, env: Vec<(String, String)>) -> CommandLine {
    let name = program.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let display = std::iter::once(name).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
    CommandLine { program, args, cwd, env, display }
}

pub fn argument_file(value: &str) -> String {
    format!("\"{}\"\n", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub const GRADLE_INIT: &str = r#"// Written by Zen IDE for a run configuration (ADR-0152).
def zenWrite = { out, List files ->
    def joined = files.collect { it.toString() }.join(File.pathSeparator)
    new File(out.toString()).text = '"' + joined.replace('\\', '\\\\').replace('"', '\\"') + '"\n'
}

allprojects { p ->
    p.plugins.withId('java') {
        def out = p.findProperty('zenClasspathOut')
        def only = p.findProperty('zenClasspathOnly') == 'true'
        def prefix = p.findProperty('zenClasspathPrefix')
        def classpath = only
            ? p.configurations.compileClasspath + p.configurations.runtimeClasspath
            : p.sourceSets.main.runtimeClasspath
        def processorsOut = p.findProperty('zenProcessorPathOut')
        def processors = p.configurations.findByName('annotationProcessor')
        p.tasks.register('zenClasspath') {
            dependsOn(only ? classpath : p.tasks.named('classes'))
            if (processorsOut && processors) dependsOn(processors)
            doLast {
                def files = classpath.files.collect { it.absolutePath }
                zenWrite(out, prefix ? [prefix] + files : files)
                if (processorsOut) zenWrite(processorsOut, processors ? processors.files.collect { it.absolutePath } : [])
            }
        }
    }
}

gradle.rootProject { root ->
    root.tasks.register('zenTestClasspath') { task ->
        def out = root.findProperty('zenClasspathOut')
        def rootsOut = root.findProperty('zenTestRootsOut')
        def java = root.allprojects.findAll { it.plugins.hasPlugin('java') }
        def sets = java.collect { it.sourceSets.test }
        def owner = java ? java[0] : root
        def handler = owner.dependencies
        def configurations = owner.configurations
        sets.each { task.dependsOn(it.runtimeClasspath) }
        doLast {
            def files = new LinkedHashSet()
            sets.each { files.addAll(it.runtimeClasspath.files) }
            def names = files.collect { it.name }
            def engine = names.find { it.startsWith('junit-platform-engine-') }
            if (engine && !names.any { it.startsWith('junit-platform-launcher-') }) {
                def version = engine - 'junit-platform-engine-' - '.jar'
                def launcher = handler.create("org.junit.platform:junit-platform-launcher:${version}")
                files.addAll(configurations.detachedConfiguration(launcher).files)
            }
            zenWrite(out, files.collect { it.absolutePath })
            def roots = sets.collectMany { it.output.classesDirs.files.findAll { it.isDirectory() } }
            new File(rootsOut.toString()).text = roots.collect { it.absolutePath }.join('\n') + '\n'
        }
    }
}
"#;

pub const CLASSPATH_HELPER: &str = r#"// Written by Zen IDE for a run configuration (ADR-0152).
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

public class ZenClasspath {
    public static void main(String[] args) throws Exception {
        List<String> parts = new ArrayList<>();
        for (int i = 2; i < args.length; i++) parts.add(args[i]);
        Path deps = Path.of(args[0]);
        if (Files.exists(deps)) {
            String text = Files.readString(deps).trim();
            if (text.length() > 1 && text.startsWith("\"") && text.endsWith("\"")) {
                text = text.substring(1, text.length() - 1).replace("\\\"", "\"").replace("\\\\", "\\");
            }
            if (!text.isEmpty()) parts.addAll(List.of(text.split(java.io.File.pathSeparator)));
        }
        String joined = String.join(java.io.File.pathSeparator, parts);
        Files.writeString(Path.of(args[1]), "\"" + joined.replace("\\", "\\\\").replace("\"", "\\\"") + "\"\n");
    }
}
"#;

fn scratch_of(context: &RunContext) -> PathBuf {
    if context.scratch.as_os_str().is_empty() { std::env::temp_dir() } else { context.scratch.clone() }
}

fn javac_line(
    home: &Path,
    module: &Path,
    classes: &Path,
    classpath: Option<&Path>,
    context: &RunContext,
    env: &[(String, String)],
    processing: &[String],
) -> Result<CommandLine, String> {
    let scratch = scratch_of(context);
    let sources = source::source_files(module);
    if sources.is_empty() {
        return Err(t!("workbench-java-no-sources", path = module.display().to_string()));
    }
    let list: String = sources.iter().map(|file| argument_file(&file.display().to_string())).collect();
    let list_file = scratch.join("sources.args");
    write(&list_file, &list)?;
    let mut args = vec!["-d".to_owned(), classes.display().to_string(), "-encoding".to_owned(), "UTF-8".to_owned()];
    if let Some(classpath) = classpath {
        args.extend(["-cp".to_owned(), format!("@{}", classpath.display())]);
    }
    if let Some(level) = crate::jdk::project_jdk(&context.root).map(|project| project.language_level) {
        args.extend(["--release".to_owned(), level.to_string()]);
    }
    args.extend(processing.iter().cloned());
    args.push(format!("@{}", list_file.display()));
    Ok(line(program(home, "javac"), args, module.to_owned(), env.to_vec()))
}

pub fn gradle_environment(
    env: &[(String, String)],
    settings: &crate::gradle::settings::GradleSettings,
) -> Vec<(String, String)> {
    let mut env = env.to_vec();
    if let Some(home) = crate::maven::settings::jdk_home(&settings.jvm) {
        crate::maven::settings::with_java(&mut env, &home);
    }
    if !settings.user_home.trim().is_empty() {
        env.push((crate::gradle::settings::USER_HOME.to_owned(), settings.user_home.trim().to_owned()));
    }
    env
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

impl JavaRun {
    fn application_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
    ) -> Result<Vec<CommandLine>, String> {
        let options: ApplicationOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        let main_class = options.main_class.trim();
        if main_class.is_empty() {
            return Err(t!("workbench-java-no-main-class"));
        }
        let home = jdk_home(&options.jre, context)?;
        let java = program(&home, "java");
        if !java.is_file() {
            return Err(t!("workbench-java-no-java", path = home.display().to_string()));
        }
        let env = environment(&home, configuration, context);
        let (mut lines, classpath) = self.classpath_lines(&options, context, &home, &java, &env)?;
        let mut args = zen_run::args::split(&configuration.common.tool_arguments);
        args.extend(["-cp".to_owned(), format!("@{}", classpath.display()), main_class.to_owned()]);
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let cwd = configuration.common.working_directory_in(&context.root);
        lines.push(line(java, args, cwd, env));
        Ok(lines)
    }

    fn classpath_lines(
        &self,
        options: &ApplicationOptions,
        context: &RunContext,
        home: &Path,
        java: &Path,
        env: &[(String, String)],
    ) -> Result<(Vec<CommandLine>, PathBuf), String> {
        let (home, java, env) = (home.to_owned(), java.to_owned(), env.to_vec());
        let module = if options.module.trim().is_empty() {
            context.root.clone()
        } else {
            context.root.join(options.module.trim())
        };
        let scratch = scratch_of(context);
        zen_run::create_private_folder(&scratch).map_err(|error| error.to_string())?;
        let classpath = scratch.join("classpath.args");
        let kotlin = source::has_kotlin(&module);
        let mut lines = Vec::new();
        match build_tool(&module) {
            BuildTool::Maven => {
                let mut settings = crate::maven::settings::MavenSettings::read(&self.build_env(), &context.root);
                let note =
                    (!settings.runner.delegate && kotlin).then(|| t!("workbench-java-kotlin-built-by", tool = "Maven"));
                settings.runner.delegate |= kotlin;
                let invocation = settings.invocation(crate::maven::settings::Purpose::Build);
                let mut maven_env = env.clone();
                if let Some(home) = &invocation.java_home {
                    crate::maven::settings::with_java(&mut maven_env, home);
                }
                maven_env.extend(invocation.env.iter().cloned());
                let deps = scratch.join("dependencies.txt");
                let own_classes = scratch.join("classes");
                if options.build {
                    let mvn = self.tool(BuildTool::Maven, &module, &context.root)?;
                    let mut args = vec!["-q".to_owned(), "-B".to_owned()];
                    args.extend(invocation.args.iter().cloned());
                    if settings.runner.delegate {
                        args.push("compile".to_owned());
                    }
                    args.extend([
                        "dependency:build-classpath".to_owned(),
                        format!("-Dmdep.outputFile={}", deps.display()),
                        "-Dmdep.includeScope=runtime".to_owned(),
                    ]);
                    let mut build = line(mvn, args, module.clone(), maven_env);
                    if let Some(note) = note {
                        build.display = format!("{note}\n{}", build.display);
                    }
                    lines.push(build);
                }
                let processing = (options.build && !settings.runner.delegate)
                    .then(|| self.processing(&context.root, &module, BuildTool::Maven, None, &home))
                    .transpose()?
                    .unwrap_or_default();
                let helper = scratch.join("ZenClasspath.java");
                write(&helper, CLASSPATH_HELPER)?;
                let mut args =
                    vec![helper.display().to_string(), deps.display().to_string(), classpath.display().to_string()];
                if settings.runner.delegate {
                    args.push(module.join("target/classes").display().to_string());
                } else {
                    args.push(own_classes.display().to_string());
                    args.push(module.join("src/main/resources").display().to_string());
                    args.extend(processing.classpath.iter().map(|jar| jar.display().to_string()));
                }
                lines.push(line(java.clone(), args, module.clone(), env.clone()));
                if options.build && !settings.runner.delegate {
                    lines.push(javac_line(
                        &home,
                        &module,
                        &own_classes,
                        Some(&classpath),
                        context,
                        &env,
                        &processing.args,
                    )?);
                }
            }
            BuildTool::Gradle => {
                let build_env = self.build_env();
                let settings = crate::gradle::settings::GradleSettings::read(&build_env, &context.root);
                let ide_chosen = settings.build_and_run == crate::gradle::settings::Runner::Ide;
                let by_ide = ide_chosen && !kotlin;
                let gradle = self.tool(BuildTool::Gradle, &module, &context.root)?;
                let init = scratch.join("zen-classpath.gradle");
                write(&init, GRADLE_INIT)?;
                let mut args = vec!["-q".to_owned(), "--init-script".to_owned(), init.display().to_string()];
                args.extend(settings.args());
                args.push(format!("-PzenClasspathOut={}", classpath.display()));
                let own_classes = scratch.join("classes");
                let processors = scratch.join("processorpath.args");
                if by_ide {
                    args.push("-PzenClasspathOnly=true".to_owned());
                    args.push(format!("-PzenProcessorPathOut={}", processors.display()));
                    let own = [own_classes.clone(), module.join("src/main/resources")];
                    let joined = std::env::join_paths(own).map_err(|error| error.to_string())?;
                    args.push(format!("-PzenClasspathPrefix={}", joined.to_string_lossy()));
                }
                args.extend(["-p".to_owned(), module.display().to_string(), "zenClasspath".to_owned()]);
                let mut build = line(gradle, args, module.clone(), gradle_environment(&env, &settings));
                if ide_chosen && kotlin {
                    build.display =
                        format!("{}\n{}", t!("workbench-java-kotlin-built-by", tool = "Gradle"), build.display);
                }
                lines.push(build);
                if by_ide && options.build {
                    let processing =
                        self.processing(&context.root, &module, BuildTool::Gradle, Some(&processors), &home)?;
                    lines.push(javac_line(
                        &home,
                        &module,
                        &own_classes,
                        Some(&classpath),
                        context,
                        &env,
                        &processing.args,
                    )?);
                }
            }
            BuildTool::Javac if kotlin => return Err(t!("workbench-java-kotlin-needs-build")),
            BuildTool::Javac => {
                let classes = scratch.join("classes");
                let processing = self.processing(&context.root, &module, BuildTool::Javac, None, &home)?;
                lines.push(javac_line(&home, &module, &classes, None, context, &env, &processing.args)?);
                write(&classpath, &argument_file(&classes.display().to_string()))?;
            }
        }
        Ok((lines, classpath))
    }

    fn file_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        let options: FileOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        if options.file.trim().is_empty() {
            return Err(t!("workbench-java-no-file"));
        }
        let file = PathBuf::from(options.file.trim());
        let home = jdk_home(&options.jre, context)?;
        let java = program(&home, "java");
        if !java.is_file() {
            return Err(t!("workbench-java-no-java", path = home.display().to_string()));
        }
        let env = environment(&home, configuration, context);
        let mut lines = Vec::new();
        let mut args = zen_run::args::split(&configuration.common.tool_arguments);
        if options.classpath {
            let project = ApplicationOptions {
                module: options.module.clone(),
                jre: options.jre.clone(),
                build: true,
                ..ApplicationOptions::default()
            };
            let (build, classpath) = self.classpath_lines(&project, context, &home, &java, &env)?;
            lines.extend(build);
            args.extend(["-cp".to_owned(), format!("@{}", classpath.display())]);
        }
        args.push(file.display().to_string());
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let cwd = configuration.common.working_directory_in(&context.root);
        lines.push(line(java, args, cwd, env));
        Ok(lines)
    }

    fn kotlin_file_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
    ) -> Result<Vec<CommandLine>, String> {
        let options: KotlinFileOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        if options.file.trim().is_empty() {
            return Err(t!("workbench-kotlin-no-file"));
        }
        let file = PathBuf::from(options.file.trim());
        let kotlinc = if options.compiler.trim().is_empty() {
            self.kotlinc().ok_or_else(|| t!("workbench-kotlin-no-compiler"))?
        } else {
            PathBuf::from(options.compiler.trim())
        };
        let home = jdk_home(&options.jre, context)?;
        let java = program(&home, "java");
        if !java.is_file() {
            return Err(t!("workbench-java-no-java", path = home.display().to_string()));
        }
        let env = environment(&home, configuration, context);
        let scratch = scratch_of(context);
        zen_run::create_private_folder(&scratch).map_err(|error| error.to_string())?;
        let jar = scratch.join("kotlin-scratch.jar");
        let mut lines = Vec::new();
        let mut compile = Vec::new();
        let run_classpath = if options.classpath {
            let project = ApplicationOptions {
                module: options.module.clone(),
                jre: options.jre.clone(),
                build: true,
                ..ApplicationOptions::default()
            };
            let (build, classpath) = self.classpath_lines(&project, context, &home, &java, &env)?;
            lines.extend(build);
            compile.extend(["-cp".to_owned(), format!("@{}", classpath.display())]);
            let helper = scratch.join("ZenClasspath.java");
            write(&helper, CLASSPATH_HELPER)?;
            let joined = scratch.join("run-classpath.args");
            lines.push(line(
                java.clone(),
                vec![
                    helper.display().to_string(),
                    classpath.display().to_string(),
                    joined.display().to_string(),
                    jar.display().to_string(),
                ],
                context.root.clone(),
                env.clone(),
            ));
            format!("@{}", joined.display())
        } else {
            jar.display().to_string()
        };
        compile.extend([file.display().to_string(), "-include-runtime".to_owned(), "-d".to_owned()]);
        compile.push(jar.display().to_string());
        let position = lines.len() - usize::from(options.classpath);
        lines.insert(position, line(kotlinc, compile, context.root.clone(), env.clone()));
        let mut args = zen_run::args::split(&configuration.common.tool_arguments);
        args.extend(["-cp".to_owned(), run_classpath]);
        let main_class = options.main_class.trim();
        if main_class.is_empty() {
            return Err(t!("workbench-java-no-main-class"));
        }
        args.push(main_class.to_owned());
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let cwd = configuration.common.working_directory_in(&context.root);
        lines.push(line(java, args, cwd, env));
        Ok(lines)
    }

    fn jar_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        let options: JarOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        if options.jar.trim().is_empty() {
            return Err(t!("workbench-java-no-jar"));
        }
        let jar = context.root.join(options.jar.trim());
        let home = jdk_home(&options.jre, context)?;
        let java = program(&home, "java");
        if !java.is_file() {
            return Err(t!("workbench-java-no-java", path = home.display().to_string()));
        }
        let mut args = zen_run::args::split(&configuration.common.tool_arguments);
        args.extend(["-jar".to_owned(), jar.display().to_string()]);
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let env = environment(&home, configuration, context);
        Ok(vec![line(java, args, configuration.common.working_directory_in(&context.root), env)])
    }
}

impl RunProvider for JavaRun {
    fn id(&self) -> &str {
        "java"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![
            KindSpec {
                id: APPLICATION.to_owned(),
                name_message: "workbench-java-kind-application".to_owned(),
                fields: vec![
                    field("mainClass", "workbench-java-field-main-class", FieldKind::Text),
                    field("module", "workbench-java-field-module", FieldKind::Path),
                    field("jre", "workbench-java-field-jre", FieldKind::Path),
                    field("build", "workbench-java-field-build", FieldKind::Bool),
                    field(
                        crate::java::coroutines::OPTION,
                        "workbench-java-field-coroutine-debug",
                        FieldKind::Choice(
                            crate::java::coroutines::CHOICES.iter().map(|choice| (*choice).to_owned()).collect(),
                        ),
                    ),
                    field(crate::java::coroutines::TRACK, "workbench-java-field-track-coroutines", FieldKind::Bool),
                ],
                defaults: serde_json::to_value(ApplicationOptions {
                    build: true,
                    coroutine_debug: "default".to_owned(),
                    ..ApplicationOptions::default()
                })
                .unwrap_or_default(),
            },
            KindSpec {
                id: JAR.to_owned(),
                name_message: "workbench-java-kind-jar".to_owned(),
                fields: vec![
                    field("jar", "workbench-java-field-jar", FieldKind::Path),
                    field("jre", "workbench-java-field-jre", FieldKind::Path),
                ],
                defaults: serde_json::to_value(JarOptions::default()).unwrap_or_default(),
            },
            KindSpec {
                id: FILE.to_owned(),
                name_message: "workbench-java-kind-file".to_owned(),
                fields: vec![
                    field("file", "workbench-java-field-file", FieldKind::Path),
                    field("jre", "workbench-java-field-jre", FieldKind::Path),
                    field("classpath", "workbench-java-field-classpath", FieldKind::Bool),
                    field("module", "workbench-java-field-module", FieldKind::Path),
                ],
                defaults: serde_json::to_value(FileOptions::default()).unwrap_or_default(),
            },
            KindSpec {
                id: KOTLIN_FILE.to_owned(),
                name_message: "workbench-kotlin-kind-file".to_owned(),
                fields: vec![
                    field("file", "workbench-java-field-file", FieldKind::Path),
                    field("mainClass", "workbench-java-field-main-class", FieldKind::Text),
                    field("compiler", "workbench-kotlin-field-compiler", FieldKind::Path),
                    field("jre", "workbench-java-field-jre", FieldKind::Path),
                    field("classpath", "workbench-java-field-classpath", FieldKind::Bool),
                    field("module", "workbench-java-field-module", FieldKind::Path),
                ],
                defaults: serde_json::to_value(KotlinFileOptions::default()).unwrap_or_default(),
            },
        ]
    }

    fn single_file(&self, path: &Path, text: &str) -> Vec<RunTarget> {
        let kotlin = match path.extension().and_then(|extension| extension.to_str()) {
            Some("java") => false,
            Some("kt") => true,
            _ => return Vec::new(),
        };
        let Some(main) = self.mains(path, text).into_iter().next() else { return Vec::new() };
        let file = path.display().to_string();
        let (kind, options) = if kotlin {
            let options = KotlinFileOptions { file, main_class: main.class.clone(), ..KotlinFileOptions::default() };
            (KOTLIN_FILE, serde_json::to_value(options))
        } else {
            (FILE, serde_json::to_value(FileOptions { file, ..FileOptions::default() }))
        };
        let name = main.class.rsplit(['.', '$']).next().unwrap_or(&main.class).to_owned();
        let configuration = Configuration {
            name: name.clone(),
            kind: kind.to_owned(),
            options: options.unwrap_or_default(),
            ..Configuration::default()
        };
        [main.class_line, main.line]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|line| RunTarget {
                line,
                kind: TargetKind::Main,
                name: name.clone(),
                package: String::new(),
                configuration: configuration.clone(),
            })
            .collect()
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        let module = module_of(&context.root, path);
        self.mains(path, text)
            .into_iter()
            .flat_map(|main| {
                let configuration = Self::application(&main.class, &module);
                let name = main.class.rsplit(['.', '$']).next().unwrap_or(&main.class).to_owned();
                [main.class_line, main.line].into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().map(
                    move |line| RunTarget {
                        line,
                        kind: TargetKind::Main,
                        name: name.clone(),
                        package: String::new(),
                        configuration: configuration.clone(),
                    },
                )
            })
            .collect()
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        let line = u32::try_from(text.get(..offset.min(text.len()))?.matches('\n').count()).ok()?;
        let mains = self.mains(path, text);
        let main = mains
            .iter()
            .filter(|main| main.class_line <= line && line <= main.end_line)
            .max_by_key(|main| main.class_line)
            .or_else(|| (mains.len() == 1).then(|| &mains[0]))?;
        Some(Self::application(&main.class, &module_of(root, path)))
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        match configuration.kind.as_str() {
            APPLICATION => self.application_lines(configuration, context),
            JAR => self.jar_lines(configuration, context),
            FILE => self.file_lines(configuration, context),
            KOTLIN_FILE => self.kotlin_file_lines(configuration, context),
            other => Err(t!("workbench-java-unknown-kind", kind = other)),
        }
    }

    fn debug(&self, configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        if configuration.kind == FILE || configuration.kind == KOTLIN_FILE {
            return None;
        }
        let lines = match RunProvider::command_lines(self, configuration, context) {
            Ok(lines) => lines,
            Err(error) => return Some(Err(error)),
        };
        let mut build = lines;
        let program = build.pop()?;
        Some(crate::java::debug::attach_plan(build, program, &context.root))
    }

    fn parser(&self, configuration: &Configuration, context: &RunContext) -> Box<dyn OutputParser> {
        let module = serde_json::from_value::<ApplicationOptions>(configuration.options.clone())
            .map(|options| options.module)
            .unwrap_or_default();
        Box::new(trace::TraceParser::new(&context.root, &module))
    }

    fn adopt(&self, foreign: &ForeignRunConfiguration, root: &Path) -> Option<Configuration> {
        adopt::adopt(foreign, root)
    }

    fn coverage(&self, configuration: &Configuration) -> Option<&dyn zen_run::CoverageProvider> {
        [APPLICATION, JAR].contains(&configuration.kind.as_str()).then_some(self as &dyn zen_run::CoverageProvider)
    }
}

fn coverage_scratch(report: &Path) -> PathBuf {
    jacoco::folder_of(report).join("build")
}

fn jre_of(configuration: &Configuration) -> String {
    match configuration.kind.as_str() {
        JAR => serde_json::from_value::<JarOptions>(configuration.options.clone()).unwrap_or_default().jre,
        _ => serde_json::from_value::<ApplicationOptions>(configuration.options.clone()).unwrap_or_default().jre,
    }
}

pub fn classpath_folders(text: &str) -> Vec<PathBuf> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')).unwrap_or(trimmed);
    let mut joined = String::new();
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => joined.extend(characters.next()),
            other => joined.push(other),
        }
    }
    std::env::split_paths(&joined).filter(|path| path.is_dir()).collect()
}

impl zen_run::CoverageProvider for JavaRun {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        let folder = jacoco::folder_of(report);
        let agent = jacoco::copy_agent(&self.jacoco().agent.ok_or_else(jacoco::missing)?, &folder)?;
        let context = RunContext { scratch: coverage_scratch(report), ..context.clone() };
        let mut lines = RunProvider::command_lines(self, configuration, &context)?;
        if let Some(last) = lines.last_mut() {
            let option = jacoco::agent_option(&agent, &folder.join(jacoco::EXEC));
            last.args.insert(0, option.clone());
            let (program, rest) = last.display.split_once(' ').unwrap_or((last.display.as_str(), ""));
            last.display = format!("{program} {option} {rest}").trim_end().to_owned();
        }
        Ok(lines)
    }

    fn read(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<zen_run::CoverageReport, String> {
        let folder = jacoco::folder_of(report);
        if !folder.join(jacoco::EXEC).is_file() {
            return Err(t!("java-coverage-no-data"));
        }
        let classes = match configuration.kind.as_str() {
            JAR => {
                let options: JarOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
                vec![context.root.join(options.jar.trim())]
            }
            _ => std::fs::read_to_string(coverage_scratch(report).join("classpath.args"))
                .map(|text| classpath_folders(&text))
                .unwrap_or_default(),
        };
        let java = program(&jdk_home(&jre_of(configuration), context)?, "java");
        let cli = self.jacoco().cli;
        let xml = jacoco::convert(&java, cli.as_deref(), &folder, &classes)?;
        jacoco::read_reports(&[xml], &context.root)
    }
}

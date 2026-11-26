use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::coverage::CoverageBuilder;
use zen_run::{CommandLine, Configuration, CoverageProvider, CoverageReport, RunContext};

use super::junit::{BuildTool, JavaTestRun, build_of};
use crate::build_tools::BuildEnv;

pub const VERSION: &str = "0.8.16";
const AGENT_SHA512: &str = "210a21e9bfc0aa644659237e0f09645c9e81c6d965d009abc76d8da50c4f0d0c06da70c4828ba7139067650f34f2a3df4628c21bc7c27eb8ff3e68ff0c90c677";
const CLI_SHA512: &str = "6a5e250a17f62a27f738010c50b6b2ca843d5358f051377368154700bcb746a6b615d3478766275ea537364eb8292aca9f9af431cfc00988a4671476fd83cb4b";
#[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "no download in the browser"))]
const CENTRAL: &str = "https://repo.maven.apache.org/maven2/org/jacoco";

pub const EXEC: &str = "jacoco.exec";
pub const GRADLE_REPORTS: &str = "gradle";
const AGENT_COPY: &str = "jacocoagent.jar";
const XML: &str = "jacoco.xml";
const AGENT: (&str, &str) = ("org.jacoco.agent", "runtime");
const CLI: (&str, &str) = ("org.jacoco.cli", "nodeps");
const SKIPPED: [&str; 4] = ["target", "build", "node_modules", "out"];
const MAX_FOLDERS: usize = 50_000;
const MAX_JAR: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Jars {
    pub agent: Option<PathBuf>,
    pub cli: Option<PathBuf>,
}

pub fn jars(env: &BuildEnv) -> Jars {
    find_jars(&env.home(), env.data_dir().as_deref())
}

pub fn find_jars(home: &Path, data: Option<&Path>) -> Jars {
    find_pinned(home, data, (AGENT_SHA512, CLI_SHA512))
}

fn find_pinned(home: &Path, data: Option<&Path>, (agent, cli): (&str, &str)) -> Jars {
    Jars { agent: pinned(home, data, AGENT, agent), cli: pinned(home, data, CLI, cli) }
}

fn entries(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    found.sort();
    found
}

fn has_digest(jar: &Path, sha512: &str) -> bool {
    use sha2::Digest;
    std::fs::metadata(jar).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_JAR)
        && std::fs::read(jar).is_ok_and(|bytes| hex::encode(sha2::Sha512::digest(&bytes)) == sha512)
}

fn pinned(home: &Path, data: Option<&Path>, jar: (&str, &str), sha512: &str) -> Option<PathBuf> {
    let name = file_name(jar);
    let maven = home.join(".m2/repository/org/jacoco").join(jar.0).join(VERSION).join(&name);
    let gradle = home.join(".gradle/caches/modules-2/files-2.1/org.jacoco").join(jar.0).join(VERSION);
    data.map(|data| data.join("tools/jacoco").join(VERSION).join(&name))
        .into_iter()
        .chain([maven])
        .chain(entries(&gradle).into_iter().map(|place| place.join(&name)))
        .find(|path| has_digest(path, sha512))
}

pub fn missing() -> String {
    t!("java-coverage-no-jacoco")
}

pub fn agent_option(agent: &Path, exec: &Path) -> String {
    format!("-javaagent:{}=destfile={},append=true", agent.display(), exec.display())
}

pub fn quoted_agent_option(agent: &Path, exec: &Path) -> String {
    if agent.display().to_string().contains(' ') || exec.display().to_string().contains(' ') {
        format!("\"{}\"", agent_option(agent, exec))
    } else {
        agent_option(agent, exec)
    }
}

pub fn copy_agent(agent: &Path, folder: &Path) -> Result<PathBuf, String> {
    let copy = folder.join(AGENT_COPY);
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    std::fs::copy(agent, &copy).map_err(|error| format!("{}: {error}", agent.display()))?;
    Ok(copy)
}

pub fn folder_of(report: &Path) -> PathBuf {
    report.parent().map_or_else(std::env::temp_dir, Path::to_path_buf)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JacocoLine {
    pub line: u32,
    pub missed_instructions: u32,
    pub covered_instructions: u32,
    pub missed_branches: u32,
    pub covered_branches: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceLines {
    pub package: String,
    pub file: String,
    pub lines: Vec<JacocoLine>,
}

pub fn parse_xml(text: &str) -> Result<Vec<SourceLines>, String> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() };
    let document = roxmltree::Document::parse_with_options(text, options).map_err(|error| error.to_string())?;
    if !document.root_element().has_tag_name("report") {
        return Err(t!("java-coverage-not-a-report"));
    }
    let number = |node: roxmltree::Node, name: &str| -> Result<u32, String> {
        match node.attribute(name) {
            None => Ok(0),
            Some(value) => value.trim().parse().map_err(|_| t!("java-coverage-bad-line", text = value)),
        }
    };
    let mut files = Vec::new();
    for package in document.descendants().filter(|node| node.has_tag_name("package")) {
        let name = package.attribute("name").unwrap_or_default();
        for source in package.children().filter(|node| node.has_tag_name("sourcefile")) {
            let mut lines = Vec::new();
            for line in source.children().filter(|node| node.has_tag_name("line")) {
                let at = number(line, "nr")?;
                if at == 0 {
                    continue;
                }
                lines.push(JacocoLine {
                    line: at,
                    missed_instructions: number(line, "mi")?,
                    covered_instructions: number(line, "ci")?,
                    missed_branches: number(line, "mb")?,
                    covered_branches: number(line, "cb")?,
                });
            }
            files.push(SourceLines {
                package: name.to_owned(),
                file: source.attribute("name").unwrap_or_default().to_owned(),
                lines,
            });
        }
    }
    Ok(files)
}

pub fn add(builder: &mut CoverageBuilder, path: &Path, lines: &[JacocoLine]) {
    for line in lines {
        let at = line.line;
        builder.part(path, at..=at, u64::from(line.covered_instructions > 0));
        if line.covered_instructions > 0 && line.missed_instructions > 0 {
            builder.part(path, at..=at, 0);
        }
        for _ in 0..line.covered_branches {
            builder.branch(path, at, true);
        }
        for _ in 0..line.missed_branches {
            builder.branch(path, at, false);
        }
    }
}

fn skipped(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .is_some_and(|name| name.starts_with('.') || SKIPPED.contains(&name.as_ref()))
}

pub fn project_folders(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![root.to_path_buf()];
    let mut at = 0;
    while at < found.len() && found.len() < MAX_FOLDERS {
        let children: Vec<PathBuf> = entries(&found[at])
            .into_iter()
            .filter(|path| !path.is_symlink() && path.is_dir() && !skipped(path))
            .collect();
        found.extend(children);
        at += 1;
    }
    found
}

#[derive(Clone, Debug, Default)]
pub struct Sources {
    files: Vec<PathBuf>,
}

impl Sources {
    pub fn scan(root: &Path) -> Self {
        let mut files: Vec<PathBuf> = project_folders(root)
            .iter()
            .flat_map(|folder| entries(folder))
            .filter(|path| {
                path.is_file()
                    && path.extension().is_some_and(|extension| {
                        ["java", "kt", "groovy", "scala"].contains(&extension.to_string_lossy().as_ref())
                    })
            })
            .collect();
        files.sort();
        Self { files }
    }

    pub fn resolve(&self, package: &str, file: &str) -> Option<PathBuf> {
        let suffix = if package.is_empty() { PathBuf::from(file) } else { Path::new(package).join(file) };
        let mut matching: Vec<&PathBuf> = self.files.iter().filter(|path| path.ends_with(&suffix)).collect();
        if matching.is_empty() {
            matching = self.files.iter().filter(|path| path.file_name().is_some_and(|name| name == file)).collect();
        }
        let main: Vec<&PathBuf> =
            matching.iter().copied().filter(|path| path.components().any(|part| part.as_os_str() == "main")).collect();
        match (main.as_slice(), matching.as_slice()) {
            ([only], _) | (_, [only]) => Some((*only).clone()),
            _ => None,
        }
    }
}

pub fn class_folders(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for folder in project_folders(root) {
        if folder.join("pom.xml").is_file() {
            found.push(folder.join("target/classes"));
        }
        if ["build.gradle", "build.gradle.kts"].iter().any(|name| folder.join(name).is_file()) {
            found.extend(entries(&folder.join("build/classes")).into_iter().map(|language| language.join("main")));
        }
    }
    found.retain(|folder| folder.is_dir());
    found
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MavenPlugin {
    pub declared: bool,
    pub prepares_agent: bool,
}

pub fn maven_plugin(root: &Path) -> MavenPlugin {
    let mut plugin = MavenPlugin::default();
    for folder in project_folders(root) {
        let Ok(text) = std::fs::read_to_string(folder.join("pom.xml")) else { continue };
        if text.contains("jacoco-maven-plugin") {
            plugin.declared = true;
            plugin.prepares_agent |= text.contains("prepare-agent");
        }
    }
    plugin
}

pub fn gradle_plugin(root: &Path) -> bool {
    project_folders(root).iter().any(|folder| {
        ["build.gradle", "build.gradle.kts"]
            .iter()
            .filter_map(|name| std::fs::read_to_string(folder.join(name)).ok())
            .any(|text| text.contains("jacoco"))
    })
}

pub fn java_program(env: &BuildEnv, root: &Path) -> PathBuf {
    let name = if cfg!(windows) { "java.exe" } else { "java" };
    env.java_home(root)
        .map(|home| home.join("bin").join(name))
        .filter(|java| java.is_file())
        .unwrap_or_else(|| name.into())
}

pub fn report_line(java: &Path, cli: &Path, exec: &Path, classes: &[PathBuf], xml: &Path) -> CommandLine {
    let mut args = vec!["-jar".to_owned(), cli.display().to_string(), "report".to_owned(), exec.display().to_string()];
    for folder in classes {
        args.extend(["--classfiles".to_owned(), folder.display().to_string()]);
    }
    args.extend(["--xml".to_owned(), xml.display().to_string(), "--quiet".to_owned()]);
    let display = std::iter::once("java".to_owned()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
    let cwd = xml.parent().map(Path::to_path_buf).unwrap_or_default();
    CommandLine { program: java.to_owned(), args, cwd, env: Vec::new(), display }
}

pub fn convert(java: &Path, cli: Option<&Path>, folder: &Path, classes: &[PathBuf]) -> Result<PathBuf, String> {
    let cli = cli.ok_or_else(missing)?;
    if classes.is_empty() {
        return Err(t!("java-coverage-no-classes"));
    }
    let xml = folder.join(XML);
    let line = report_line(java, cli, &folder.join(EXEC), classes, &xml);
    crate::build_tools::capture(&line, None, &AtomicBool::new(false))?;
    Ok(xml)
}

pub fn gradle_reports(folder: &Path) -> Vec<PathBuf> {
    entries(&folder.join(GRADLE_REPORTS))
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "xml"))
        .collect()
}

pub fn read_reports(reports: &[PathBuf], root: &Path) -> Result<CoverageReport, String> {
    if reports.is_empty() {
        return Err(t!("java-coverage-no-data"));
    }
    let sources = Sources::scan(root);
    let mut builder = CoverageBuilder::new();
    for report in reports {
        let text = std::fs::read_to_string(report).map_err(|error| format!("{}: {error}", report.display()))?;
        let files = parse_xml(&text).map_err(|error| format!("{}: {error}", report.display()))?;
        for file in files {
            if let Some(path) = sources.resolve(&file.package, &file.file) {
                add(&mut builder, &path, &file.lines);
            }
        }
    }
    Ok(builder.build())
}

fn display(line: &mut CommandLine) {
    let name = line.program.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    line.display = std::iter::once(name).chain(line.args.iter().cloned()).collect::<Vec<_>>().join(" ");
}

fn fresh_maven_reports(root: &Path, since: SystemTime) -> Vec<PathBuf> {
    project_folders(root)
        .into_iter()
        .filter(|folder| folder.join("pom.xml").is_file())
        .map(|folder| folder.join("target/site/jacoco").join(XML))
        .filter(|xml| {
            std::fs::metadata(xml)
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| modified + std::time::Duration::from_secs(2) >= since)
        })
        .collect()
}

impl CoverageProvider for JavaTestRun {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        let folder = folder_of(report);
        let exec = folder.join(EXEC);
        let (tool, build) = build_of(configuration, context)?;
        let mut line = self.test_line(configuration, context, None)?;
        let found = self.jacoco();
        let at = line.args.iter().position(|arg| arg == "test").unwrap_or(line.args.len());
        match tool {
            BuildTool::Maven => {
                let plugin = maven_plugin(&build);
                if plugin.declared {
                    if !plugin.prepares_agent {
                        line.args.insert(at, "jacoco:prepare-agent".to_owned());
                    }
                    line.args.push(format!("-Djacoco.destFile={}", exec.display()));
                    line.args.push("-Djacoco.append=true".to_owned());
                } else {
                    let agent = copy_agent(&found.agent.ok_or_else(missing)?, &folder)?;
                    line.args.push(format!("-DargLine={}", quoted_agent_option(&agent, &exec)));
                }
            }
            BuildTool::Gradle => {
                let mut extra = vec![format!("-Dzen.coverageDir={}", folder.display())];
                match found.agent {
                    Some(agent) => {
                        let agent = copy_agent(&agent, &folder)?;
                        extra.push(format!("-Dzen.jacocoAgent={}", agent.display()));
                    }
                    None if gradle_plugin(&build) => {}
                    None => return Err(missing()),
                }
                line.args.splice(at..at, extra);
            }
        }
        display(&mut line);
        Ok(vec![line])
    }

    fn read(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<CoverageReport, String> {
        let folder = folder_of(report);
        let exec = folder.join(EXEC);
        let (tool, build) = build_of(configuration, context)?;
        let mut reports = gradle_reports(&folder);
        if exec.is_file() {
            if tool == BuildTool::Maven && maven_plugin(&build).declared {
                let since = SystemTime::now();
                let base = self.test_line(configuration, context, None)?;
                let mut args = vec![
                    "-B".to_owned(),
                    "-q".to_owned(),
                    "jacoco:report".to_owned(),
                    format!("-Djacoco.dataFile={}", exec.display()),
                ];
                args.extend(zen_run::args::split(&configuration.common.tool_arguments));
                let mut line = CommandLine { args, ..base };
                display(&mut line);
                crate::build_tools::capture(&line, None, &AtomicBool::new(false))?;
                reports.extend(fresh_maven_reports(&build, since));
            } else {
                let java = java_program(self.env(), &context.root);
                let cli = self.jacoco().cli;
                reports.push(convert(&java, cli.as_deref(), &folder, &class_folders(&build))?);
            }
        }
        read_reports(&reports, &context.root)
    }
}

/// What `java.jacoco.download` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub version: String,
    /// Where the agent and the command-line jars are kept.
    pub folder: PathBuf,
    /// The background task that downloads them (`tasks.log.read`), when they are not there already.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

pub struct DownloadJacoco;

fn file_name((artifact, classifier): (&str, &str)) -> String {
    format!("{artifact}-{VERSION}-{classifier}.jar")
}

impl Command for DownloadJacoco {
    type Params = NoParams;
    type Output = Download;
    const ID: &'static str = "java.jacoco.download";
    const TITLE: &'static str = "Download JaCoCo";
    const DESCRIPTION: &'static str = "Downloads the JaCoCo 0.8.16 agent and command-line jars from Maven Central \
         (checked against their SHA-512) into the IDE's data folder, in a background task, for Run with Coverage \
         of Java and Kotlin tests and applications when neither the local Maven repository nor the Gradle cache \
         has them: the network is used only when this command runs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Download, CommandError> {
        let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
        let data = env.data_dir().ok_or_else(|| CommandError::new(t!("java-coverage-download-no-folder")))?;
        let folder = data.join("tools/jacoco").join(VERSION);
        if [AGENT, CLI].iter().all(|jar| folder.join(file_name(*jar)).is_file()) {
            return Ok(Download { version: VERSION.to_owned(), folder, task: None });
        }
        let title = t!("java-coverage-download-task", version = VERSION);
        let task = session.state_mut::<crate::tasks::Tasks>().start(&title, "jacoco", true);
        let id = task.id();
        let target = folder.clone();
        std::thread::spawn(move || task.finish(download(&target, &task)));
        Ok(Download { version: VERSION.to_owned(), folder, task: Some(id) })
    }
}

#[cfg(target_arch = "wasm32")]
fn download(_folder: &Path, _task: &crate::tasks::TaskHandle) -> Result<String, String> {
    Err(t!("java-coverage-download-unavailable"))
}

#[cfg(not(target_arch = "wasm32"))]
fn download(folder: &Path, task: &crate::tasks::TaskHandle) -> Result<String, String> {
    use sha2::Digest;
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    for (jar, sha512) in [(AGENT, AGENT_SHA512), (CLI, CLI_SHA512)] {
        let name = file_name(jar);
        let url = format!("{CENTRAL}/{}/{VERSION}/{name}", jar.0);
        task.log(&url);
        let bytes = zen_update::fetch::document(&url, 16 * 1024 * 1024, std::time::Duration::from_secs(300))
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("{url}: not found"))?;
        if hex::encode(sha2::Sha512::digest(&bytes)) != sha512 {
            return Err(t!("java-coverage-download-checksum", url = url.as_str()));
        }
        let partial = folder.join(format!("{name}.partial"));
        std::fs::write(&partial, &bytes).map_err(|error| error.to_string())?;
        std::fs::rename(&partial, folder.join(&name)).map_err(|error| error.to_string())?;
    }
    Ok(t!("java-coverage-download-done", folder = folder.display().to_string()))
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(DownloadJacoco)
}

#[cfg(test)]
mod tests;

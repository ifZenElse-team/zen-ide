use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Browse, Choice, Form, Item, Target, Tone};

use crate::build_tools::BuildEnv;
use crate::java_run::BuildTool;

pub const TOOL: &str = "annotationProcessing";

pub const COMPILER_PAGE: &str = "compiler";

pub const PAGE: &str = "annotationProcessors";

pub const AGENTS: &str = "${javaAgents}";

pub const ENABLED: &str = "${annotationProcessingEnabled}";

const LOMBOK_GROUP: &str = "org.projectlombok";

const LOMBOK: &str = "lombok";

const MAVEN_GENERATED: &str = "target/generated-sources/annotations";

const GRADLE_GENERATED: &str = "build/generated/sources/annotationProcessor/java/main";

/// The annotation processing settings of a project: Settings → Build, Execution, Deployment → Compiler →
/// Annotation Processors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProcessingSettings {
    /// Enable annotation processing: off, javac runs with `-proc:none` and the language server does not
    /// run processors of Gradle projects.
    pub enabled: bool,
    /// Where the processors come from.
    pub processor_path: ProcessorSource,
    /// The processor path when `processorPath` is `custom`: jars and folders separated by the platform's
    /// path separator (`:` on macOS and Linux).
    pub custom_path: String,
    /// The folder generated sources are written to (`-s`), relative to the module; empty for the
    /// build's own (`target/generated-sources/annotations`, `build/generated/sources/annotationProcessor/java/main`).
    pub generated_sources: String,
    /// Processor options passed as `-Akey=value`, over the ones the build gives.
    pub options: BTreeMap<String, String>,
}

impl Default for ProcessingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            processor_path: ProcessorSource::Build,
            custom_path: String::new(),
            generated_sources: String::new(),
            options: BTreeMap::new(),
        }
    }
}

/// Where annotation processors come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessorSource {
    /// The project's build: Maven's `annotationProcessorPaths` (else Lombok when the project depends on it),
    /// Gradle's `annotationProcessor` configuration.
    #[default]
    Build,
    /// The path in `customPath`.
    Custom,
}

/// What the project's build configures for annotation processing, read from its build files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FromBuild {
    /// `maven`, `gradle`, or `none` for a project without a build.
    pub tool: String,
    /// The processors the build names, as `group:artifact:version`.
    pub processors: Vec<String>,
    /// The jars of Maven's `annotationProcessorPaths` found in the local repository.
    pub processor_jars: Vec<PathBuf>,
    /// The processor options the build passes (`-Akey=value` in `compilerArgs` or `options.compilerArgs`).
    pub options: BTreeMap<String, String>,
    /// The folder the build writes generated sources to, relative to the module.
    pub generated_sources: String,
    /// Lombok, when the project depends on it.
    pub lombok: Option<Lombok>,
}

/// The project's Lombok dependency.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Lombok {
    pub version: String,
    /// Whether the build names the version; when it does not (a parent or a plug-in manages it), the newest
    /// version in the local repositories is used.
    pub version_declared: bool,
    /// The jar in the local Maven repository or the Gradle cache; absent until the build has downloaded it.
    pub jar: Option<PathBuf>,
    /// The build file that declares it.
    pub build_file: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JavaLaunch {
    pub lombok: Option<(String, PathBuf)>,
    pub processing: bool,
}

impl JavaLaunch {
    pub fn agents(&self) -> Vec<String> {
        self.lombok.iter().map(|(_, jar)| format!("-javaagent:{}", jar.display())).collect()
    }

    pub fn change_notice(&self, before: &JavaLaunch, server: &str) -> Option<String> {
        if self.lombok != before.lombok {
            return Some(match &self.lombok {
                Some((version, _)) => t!("processing-restart-lombok", server = server, version = version.as_str()),
                None => t!("processing-restart-no-lombok", server = server),
            });
        }
        (self.processing != before.processing).then(|| t!("processing-restart-settings", server = server))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stores {
    maven: PathBuf,
    gradle: PathBuf,
}

impl Stores {
    fn of(env: &BuildEnv, root: &Path) -> Self {
        let home = env.home();
        Self {
            maven: crate::maven::settings::MavenSettings::read(env, root).local_repository(&home),
            gradle: crate::gradle::settings::GradleSettings::read(env, root)
                .user_home_folder(&home)
                .join("caches/modules-2/files-2.1"),
        }
    }

    fn jar(&self, group: &str, artifact: &str, version: &str) -> Option<PathBuf> {
        let name = format!("{artifact}-{version}.jar");
        let maven = self.maven.join(group.replace('.', "/")).join(artifact).join(version).join(&name);
        if maven.is_file() {
            return Some(maven);
        }
        let folder = self.gradle.join(group).join(artifact).join(version);
        let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
            .ok()?
            .flatten()
            .map(|entry| entry.path().join(&name))
            .filter(|path| path.is_file())
            .collect();
        found.sort();
        found.into_iter().next()
    }

    fn newest(&self, group: &str, artifact: &str) -> Option<String> {
        let listed = |folder: PathBuf| -> Vec<String> {
            std::fs::read_dir(folder)
                .map(|entries| {
                    entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()
                })
                .unwrap_or_default()
        };
        let mut versions = listed(self.maven.join(group.replace('.', "/")).join(artifact));
        versions.extend(listed(self.gradle.join(group).join(artifact)));
        versions
            .into_iter()
            .filter(|version| self.jar(group, artifact, version).is_some())
            .max_by(|a, b| version_key(a).cmp(&version_key(b)))
    }
}

fn version_key(version: &str) -> Vec<(u64, String)> {
    version
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|part| match part.parse::<u64>() {
            Ok(number) => (number, String::new()),
            Err(_) => (0, part.to_owned()),
        })
        .collect()
}

pub fn read(env: &BuildEnv, root: &Path) -> ProcessingSettings {
    env.project_settings(TOOL, root)
}

pub fn from_build(env: &BuildEnv, root: &Path) -> FromBuild {
    let stores = Stores::of(env, root);
    match crate::java_run::build_tool(root) {
        BuildTool::Maven => maven_build(root, &stores),
        BuildTool::Gradle => gradle_build(root, &stores),
        BuildTool::Javac => FromBuild { tool: "none".to_owned(), ..FromBuild::default() },
    }
}

pub fn launch(env: &BuildEnv, root: &Path) -> JavaLaunch {
    if !crate::trust::is_trusted(env, root) {
        return JavaLaunch::default();
    }
    let lombok = from_build(env, root).lombok.and_then(|lombok| Some((lombok.version, lombok.jar?)));
    JavaLaunch { lombok, processing: read(env, root).enabled }
}

pub fn without_build_scripts(mut options: serde_json::Value) -> serde_json::Value {
    if let Some(gradle) = options.pointer_mut("/settings/java/import/gradle").and_then(serde_json::Value::as_object_mut)
    {
        gradle.insert("enabled".to_owned(), serde_json::Value::Bool(false));
    }
    options
}

fn lombok_at(stores: &Stores, build_file: &Path, version: Option<String>) -> Lombok {
    let declared = version.filter(|version| !version.is_empty() && !version.contains("${"));
    let version_declared = declared.is_some();
    let version = declared.or_else(|| stores.newest(LOMBOK_GROUP, LOMBOK)).unwrap_or_default();
    let jar = (!version.is_empty()).then(|| stores.jar(LOMBOK_GROUP, LOMBOK, &version)).flatten();
    Lombok { version, version_declared, jar, build_file: build_file.to_owned() }
}

fn element<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(|child| child.is_element() && child.tag_name().name() == name)
}

fn elements<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    node.children().filter(move |child| child.is_element() && child.tag_name().name() == name)
}

fn text(node: roxmltree::Node<'_, '_>, name: &str) -> String {
    element(node, name).and_then(|child| child.text()).map(str::trim).unwrap_or_default().to_owned()
}

fn substitute(value: &str, properties: &BTreeMap<String, String>) -> String {
    let mut value = value.to_owned();
    for _ in 0..4 {
        let Some(start) = value.find("${") else { break };
        let Some(end) = value[start..].find('}') else { break };
        let name = value[start + 2..start + end].to_owned();
        let Some(found) = properties.get(&name) else { break };
        value.replace_range(start..start + end + 1, found);
    }
    value
}

fn pom_files(project: &crate::maven::pom::PomProject, out: &mut Vec<PathBuf>) {
    out.push(project.pom.clone());
    for module in &project.modules {
        pom_files(module, out);
    }
}

fn maven_build(root: &Path, stores: &Stores) -> FromBuild {
    let mut found = FromBuild { tool: "maven".to_owned(), ..FromBuild::default() };
    let mut files = Vec::new();
    pom_files(&crate::maven::pom::read(&root.join("pom.xml")), &mut files);
    let mut inherited: BTreeMap<String, String> = BTreeMap::new();
    let mut managed: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut lombok: Option<(PathBuf, Option<String>)> = None;
    for file in files {
        let Ok(source) = std::fs::read_to_string(&file) else { continue };
        let Ok(document) = roxmltree::Document::parse(&source) else { continue };
        let project = document.root_element();
        let mut properties = inherited.clone();
        if let Some(declared) = element(project, "properties") {
            for property in declared.children().filter(roxmltree::Node::is_element) {
                let value = property.text().map(str::trim).unwrap_or_default().to_owned();
                properties.insert(property.tag_name().name().to_owned(), value);
            }
        }
        properties.insert("project.build.directory".to_owned(), "target".to_owned());
        properties.insert("project.basedir".to_owned(), String::new());
        properties.insert("basedir".to_owned(), String::new());
        if file == root.join("pom.xml") {
            inherited = properties.clone();
        }
        let value = |node: roxmltree::Node<'_, '_>, name: &str| substitute(&text(node, name), &properties);
        let managing = element(project, "dependencyManagement").and_then(|node| element(node, "dependencies"));
        for dependency in managing.into_iter().flat_map(|list| elements(list, "dependency")) {
            let version = value(dependency, "version");
            if !version.is_empty() {
                managed.insert((value(dependency, "groupId"), value(dependency, "artifactId")), version);
            }
        }
        let builds: Vec<roxmltree::Node<'_, '_>> = element(project, "build")
            .map(|build| {
                let mut lists: Vec<_> = element(build, "plugins").into_iter().collect();
                lists.extend(element(build, "pluginManagement").and_then(|node| element(node, "plugins")));
                lists
            })
            .unwrap_or_default();
        let compilers = builds
            .iter()
            .flat_map(|list| elements(*list, "plugin"))
            .filter(|plugin| text(*plugin, "artifactId") == "maven-compiler-plugin")
            .filter_map(|plugin| element(plugin, "configuration"));
        let mut paths: Vec<(String, String, String)> = Vec::new();
        for configuration in compilers {
            for path in element(configuration, "annotationProcessorPaths")
                .into_iter()
                .flat_map(|list| list.children().filter(roxmltree::Node::is_element))
            {
                paths.push((value(path, "groupId"), value(path, "artifactId"), value(path, "version")));
            }
            let args = element(configuration, "compilerArgs")
                .into_iter()
                .flat_map(|list| list.children().filter(roxmltree::Node::is_element))
                .filter_map(|arg| arg.text().map(|arg| substitute(arg.trim(), &properties)))
                .chain(Some(value(configuration, "compilerArgument")));
            for arg in args {
                if let Some((name, given)) = option(&arg) {
                    found.options.insert(name, given);
                }
            }
            let generated = value(configuration, "generatedSourcesDirectory");
            if !generated.is_empty() {
                found.generated_sources = generated.trim_start_matches('/').to_owned();
            }
        }
        for (group, artifact, version) in paths {
            if group.is_empty() || artifact.is_empty() {
                continue;
            }
            let version = match version {
                version if version.is_empty() => managed.get(&(group.clone(), artifact.clone())).cloned(),
                version => Some(version),
            }
            .unwrap_or_else(|| stores.newest(&group, &artifact).unwrap_or_default());
            if group == LOMBOK_GROUP && artifact == LOMBOK && lombok.is_none() {
                lombok = Some((file.clone(), Some(version.clone())));
            }
            found.processors.push(format!("{group}:{artifact}:{version}"));
            found.processor_jars.extend(stores.jar(&group, &artifact, &version));
        }
        let dependencies = element(project, "dependencies");
        for dependency in dependencies.into_iter().flat_map(|list| elements(list, "dependency")) {
            if lombok.is_none()
                && value(dependency, "groupId") == LOMBOK_GROUP
                && value(dependency, "artifactId") == LOMBOK
            {
                let version = value(dependency, "version");
                lombok = Some((file.clone(), (!version.is_empty()).then_some(version)));
            }
        }
    }
    if found.generated_sources.is_empty() {
        found.generated_sources = MAVEN_GENERATED.to_owned();
    }
    found.lombok = lombok.map(|(file, version)| {
        let version = version.or_else(|| managed.get(&(LOMBOK_GROUP.to_owned(), LOMBOK.to_owned())).cloned());
        lombok_at(stores, &file, version)
    });
    found
}

fn option(arg: &str) -> Option<(String, String)> {
    let rest = arg.trim().strip_prefix("-A")?;
    let (name, value) = rest.split_once('=').unwrap_or((rest, ""));
    (!name.is_empty()).then(|| (name.to_owned(), value.to_owned()))
}

fn quoted(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(['\'', '"']) {
        let quote = rest[start..].chars().next().unwrap_or('"');
        let after = &rest[start + 1..];
        let Some(end) = after.find(quote) else { break };
        found.push(&after[..end]);
        rest = &after[end + 1..];
    }
    found
}

fn gradle_properties(root: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(root.join("gradle.properties"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn gradle_version(version: &str, properties: &BTreeMap<String, String>) -> Option<String> {
    let name = version.trim_start_matches('$').trim_start_matches('{').trim_end_matches('}');
    if version.starts_with('$') {
        return properties.get(name).cloned();
    }
    (!version.is_empty()).then(|| version.to_owned())
}

fn catalog_lombok(root: &Path) -> Option<Option<String>> {
    let catalog = std::fs::read_to_string(root.join("gradle/libs.versions.toml")).ok()?;
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    let mut table = "";
    for line in catalog.lines().map(str::trim) {
        if line.starts_with('[') {
            table = line;
        } else if table == "[versions]"
            && let Some((name, value)) = line.split_once('=')
        {
            versions.insert(name.trim().trim_matches('"').to_owned(), value.trim().trim_matches('"').to_owned());
        }
    }
    let line = catalog.lines().find(|line| {
        line.contains(&format!("{LOMBOK_GROUP}:{LOMBOK}"))
            || (line.contains(&format!("\"{LOMBOK_GROUP}\"")) && line.contains(&format!("\"{LOMBOK}\"")))
    })?;
    let strings = quoted(line);
    if let Some(full) = strings.iter().find_map(|text| text.strip_prefix(&format!("{LOMBOK_GROUP}:{LOMBOK}:"))) {
        return Some(Some(full.to_owned()));
    }
    let after = |key: &str| {
        let at = line.find(key)?;
        quoted(&line[at..]).first().map(|value| (*value).to_owned())
    };
    Some(after("version.ref").and_then(|name| versions.get(&name).cloned()).or_else(|| after("version")))
}

fn gradle_build(root: &Path, stores: &Stores) -> FromBuild {
    let mut found =
        FromBuild { tool: "gradle".to_owned(), generated_sources: GRADLE_GENERATED.to_owned(), ..FromBuild::default() };
    let properties = gradle_properties(root);
    let mut files: Vec<PathBuf> = crate::gradle::model::build_file(root).into_iter().collect();
    for project in crate::gradle::model::projects(root) {
        if let Some(file) = project.build_file
            && !files.contains(&file)
        {
            files.push(file);
        }
    }
    let mut lombok: Option<(PathBuf, Option<String>)> = None;
    for file in &files {
        let Ok(script) = std::fs::read_to_string(file) else { continue };
        for line in script.lines() {
            let strings = quoted(line);
            for string in &strings {
                if let Some((name, value)) = option(string) {
                    found.options.insert(name, value);
                }
                if let Some(rest) = string.strip_prefix(&format!("{LOMBOK_GROUP}:{LOMBOK}")) {
                    let version = rest.strip_prefix(':').and_then(|version| gradle_version(version, &properties));
                    match &lombok {
                        Some((_, Some(_))) => {}
                        _ => lombok = Some((file.clone(), version)),
                    }
                }
            }
            if line.trim_start().starts_with("annotationProcessor") {
                for string in strings.iter().filter(|string| string.matches(':').count() >= 1) {
                    if !found.processors.iter().any(|known| known == string) {
                        found.processors.push((*string).to_owned());
                    }
                }
            }
            if line.contains("io.freefair.lombok") && lombok.is_none() {
                lombok = Some((file.clone(), None));
            }
            if line.contains("libs.lombok") && lombok.is_none() {
                lombok = catalog_lombok(root).map(|version| (file.clone(), version));
            }
        }
    }
    found.lombok = lombok.map(|(file, version)| lombok_at(stores, &file, version));
    found
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Javac {
    pub args: Vec<String>,
    pub classpath: Vec<PathBuf>,
}

pub fn javac(
    env: &BuildEnv,
    root: &Path,
    module: &Path,
    tool: BuildTool,
    gradle_processors: Option<&Path>,
    java_major: Option<u32>,
) -> Result<Javac, String> {
    let settings = read(env, root);
    let build = from_build(env, root);
    javac_with(&settings, &build, module, tool, gradle_processors, java_major)
}

pub fn javac_with(
    settings: &ProcessingSettings,
    build: &FromBuild,
    module: &Path,
    tool: BuildTool,
    gradle_processors: Option<&Path>,
    java_major: Option<u32>,
) -> Result<Javac, String> {
    let mut javac = Javac::default();
    let lombok = build.lombok.as_ref().and_then(|lombok| lombok.jar.clone());
    if tool == BuildTool::Maven {
        javac.classpath.extend(lombok.clone());
    }
    if !settings.enabled {
        javac.args.push("-proc:none".to_owned());
        return Ok(javac);
    }
    if java_major.is_none_or(|major| major >= 21) {
        javac.args.push("-proc:full".to_owned());
    }
    let joined = |paths: Vec<PathBuf>| {
        std::env::join_paths(paths)
            .map(|joined| joined.to_string_lossy().into_owned())
            .map_err(|error| error.to_string())
    };
    match (settings.processor_path, tool) {
        (ProcessorSource::Custom, _) if !settings.custom_path.trim().is_empty() => {
            javac.args.extend(["-processorpath".to_owned(), settings.custom_path.trim().to_owned()]);
        }
        (ProcessorSource::Custom, _) | (ProcessorSource::Build, BuildTool::Javac) => {}
        (ProcessorSource::Build, BuildTool::Maven) => {
            let jars = if build.processor_jars.is_empty() {
                lombok.into_iter().collect()
            } else {
                build.processor_jars.clone()
            };
            if !jars.is_empty() {
                javac.args.extend(["-processorpath".to_owned(), joined(jars)?]);
            }
        }
        (ProcessorSource::Build, BuildTool::Gradle) => {
            if let Some(file) = gradle_processors {
                javac.args.extend(["-processorpath".to_owned(), format!("@{}", file.display())]);
            }
        }
    }
    let generated = match settings.generated_sources.trim() {
        "" if build.generated_sources.is_empty() => None,
        "" => Some(module.join(&build.generated_sources)),
        own => Some(module.join(own)),
    };
    if let Some(folder) = generated {
        std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        javac.args.extend(["-s".to_owned(), folder.display().to_string()]);
    }
    let mut options = build.options.clone();
    options.extend(settings.options.iter().map(|(name, value)| (name.clone(), value.clone())));
    javac.args.extend(
        options.iter().map(
            |(name, value)| {
                if value.is_empty() { format!("-A{name}") } else { format!("-A{name}={value}") }
            },
        ),
    );
    Ok(javac)
}

pub fn expand_options(value: &serde_json::Value, enabled: bool) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) if text == ENABLED => serde_json::Value::Bool(enabled),
        serde_json::Value::Array(items) => items.iter().map(|item| expand_options(item, enabled)).collect(),
        serde_json::Value::Object(fields) => {
            fields.iter().map(|(name, item)| (name.clone(), expand_options(item, enabled))).collect()
        }
        other => other.clone(),
    }
}

pub fn is_build_file(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
        matches!(
            name,
            "pom.xml"
                | "build.gradle"
                | "build.gradle.kts"
                | "settings.gradle"
                | "settings.gradle.kts"
                | "gradle.properties"
                | "libs.versions.toml"
        )
    })
}

pub fn names_launch(server: &zen_lsp::ServerDeclaration) -> bool {
    server.args.iter().any(|arg| arg == AGENTS) || server.initialization_options.to_string().contains(ENABLED)
}

fn parse_options(text: &str) -> BTreeMap<String, String> {
    text.split([';', '\n'])
        .filter_map(|pair| {
            let pair = pair.trim();
            let pair = pair.strip_prefix("-A").unwrap_or(pair);
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (!name.trim().is_empty()).then(|| (name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn show_options(options: &BTreeMap<String, String>) -> String {
    options
        .iter()
        .map(|(name, value)| if value.is_empty() { name.clone() } else { format!("{name}={value}") })
        .collect::<Vec<_>>()
        .join("; ")
}

fn change(
    settings: &ProcessingSettings,
    key: &str,
    value: serde_json::Value,
) -> Result<ProcessingSettings, CommandError> {
    let unknown = || CommandError::new(t!("processing-unknown", key = key));
    let mut whole = serde_json::to_value(settings).map_err(|error| CommandError::new(error.to_string()))?;
    let defaults = serde_json::to_value(ProcessingSettings::default()).unwrap_or_default();
    let pointer = format!("/{key}");
    let default = defaults.pointer(&pointer).cloned().ok_or_else(unknown)?;
    let value = match (&default, value) {
        (_, serde_json::Value::Null) => default.clone(),
        (serde_json::Value::Object(_), serde_json::Value::String(text)) => {
            serde_json::to_value(parse_options(&text)).unwrap_or_default()
        }
        (_, value) => value,
    };
    let shown = value.to_string();
    *whole.pointer_mut(&pointer).ok_or_else(unknown)? = value;
    serde_json::from_value(whole)
        .map_err(|_| CommandError::new(t!("processing-bad-value", key = key, value = shown.as_str())))
}

/// What `annotationProcessing.settings.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingState {
    pub settings: ProcessingSettings,
    /// What the project's build configures; processors, options and the generated sources folder from here
    /// apply unless the settings say otherwise.
    pub from_build: FromBuild,
}

/// Which project's settings a command is about.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectParams {
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

fn root(session: &Session, given: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    match given {
        Some(root) => Ok(root),
        None => crate::run::project_root(session),
    }
}

fn env(session: &Session) -> BuildEnv {
    session.state::<BuildEnv>().cloned().unwrap_or_default()
}

fn state(env: &BuildEnv, root: &Path) -> ProcessingState {
    ProcessingState { settings: read(env, root), from_build: from_build(env, root) }
}

pub struct GetSettings;

impl Command for GetSettings {
    type Params = ProjectParams;
    type Output = ProcessingState;
    const ID: &'static str = "annotationProcessing.settings.get";
    const TITLE: &'static str = "Annotation Processing Settings";
    const DESCRIPTION: &'static str = "The project's annotation processing settings (Settings → Build, Execution, \
         Deployment → Compiler → Annotation Processors), kept per project, with what the build configures: \
         processors, options, the generated sources folder and Lombok. When the project depends on Lombok, the Java \
         language server runs with Lombok's jar from the local Maven repository or the Gradle cache as its agent \
         (never downloaded by the IDE). The settings reach javac when the IDE compiles a Java application, and the \
         language server's processing of Gradle projects. Change them with annotationProcessing.settings.set.";

    fn run(&self, session: &mut Session, params: ProjectParams) -> Result<ProcessingState, CommandError> {
        let root = root(session, params.project)?;
        Ok(state(&env(session), &root))
    }
}

/// The parameters of `annotationProcessing.settings.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetParams {
    /// The setting, as annotationProcessing.settings.get names it under `settings`: `enabled`, `processorPath`
    /// (`build` or `custom`), `customPath`, `generatedSources`, `options`.
    #[serde(default)]
    pub key: Option<String>,
    /// Its new value; `null` puts the default back. `options` also takes `name=value; other=value`.
    #[serde(default)]
    pub value: serde_json::Value,
    /// Or several settings at once, by key: `{ "enabled": true, "generatedSources": "gen" }`.
    #[serde(default)]
    pub values: BTreeMap<String, serde_json::Value>,
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

pub struct SetSettings;

impl Command for SetSettings {
    type Params = SetParams;
    type Output = ProcessingState;
    const ID: &'static str = "annotationProcessing.settings.set";
    const TITLE: &'static str = "Change Annotation Processing Settings";
    const DESCRIPTION: &'static str = "Changes the project's annotation processing settings, one by key and value or \
         several with values, and answers them as annotationProcessing.settings.get does. Turning processing on or \
         off restarts the Java language server.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<ProcessingState, CommandError> {
        let root = root(session, params.project)?;
        let env = env(session);
        let mut settings = read(&env, &root);
        let mut changes: Vec<(String, serde_json::Value)> = params.values.into_iter().collect();
        if let Some(key) = params.key {
            changes.push((key, params.value));
        }
        for (key, value) in changes {
            settings = change(&settings, &key, value)?;
        }
        env.set_project_settings(TOOL, &root, &settings).map_err(CommandError::new)?;
        if let Some(servers) = session.state::<crate::language::servers::LanguageServers>() {
            servers.forget_java_launches();
        }
        Ok(state(&env, &root))
    }
}

fn set(key: &str) -> Action {
    Action::with_value(SetSettings::ID, "value").param("key", key)
}

fn note(text: String) -> Item {
    Item::Text { text, tone: Tone::Secondary, spinner: false }
}

fn from_build_note(build: &FromBuild, given: &str) -> Option<Item> {
    let tool = match build.tool.as_str() {
        "maven" => "Maven",
        "gradle" => "Gradle",
        _ => return None,
    };
    (!given.is_empty()).then(|| note(t!("processing-from-build", tool = tool, value = given)))
}

pub struct CompilerForm;

impl Command for CompilerForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "compiler.settings.form";
    const TITLE: &'static str = "Compiler Settings Page";
    const DESCRIPTION: &'static str = "Settings → Build, Execution, Deployment → Compiler as a form: a link to \
         Annotation Processors.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        Ok(Form {
            items: vec![
                note(t!("processing-compiler-note")),
                Item::Link { label: t!("processing-page"), target: Target::Page { page: PAGE.to_owned() } },
            ],
            busy: false,
        })
    }
}

pub struct ProcessorsForm;

impl Command for ProcessorsForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "annotationProcessing.settings.form";
    const TITLE: &'static str = "Annotation Processors Page";
    const DESCRIPTION: &'static str = "Settings → Build, Execution, Deployment → Compiler → Annotation Processors as \
         a form: enable annotation processing, obtain processors from the build or a processor path, the generated \
         sources folder and processor options, with what the build configures and the project's Lombok. Its \
         changes run annotationProcessing.settings.set.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let Ok(root) = crate::run::project_root(session) else {
            return Ok(Form { items: vec![note(t!("processing-no-project"))], busy: false });
        };
        let ProcessingState { settings, from_build: build } = state(&env(session), &root);
        let mut items = vec![
            Item::Check { label: t!("processing-enabled"), value: settings.enabled, action: set("enabled") },
            Item::Choice {
                label: t!("processing-source"),
                options: vec![
                    Choice { value: "build".to_owned(), label: t!("processing-source-build") },
                    Choice { value: "custom".to_owned(), label: t!("processing-source-custom") },
                ],
                selected: Some(
                    match settings.processor_path {
                        ProcessorSource::Build => "build",
                        ProcessorSource::Custom => "custom",
                    }
                    .to_owned(),
                ),
                action: set("processorPath"),
            },
        ];
        if settings.processor_path == ProcessorSource::Custom {
            items.push(Item::Field {
                label: t!("processing-custom-path"),
                value: settings.custom_path.clone(),
                action: set("customPath"),
                browse: Browse::File,
            });
        }
        items.extend(from_build_note(&build, &build.processors.join(", ")));
        items.extend([
            Item::Group { title: t!("processing-generated") },
            Item::Field {
                label: t!("processing-generated-main"),
                value: settings.generated_sources.clone(),
                action: set("generatedSources"),
                browse: Browse::None,
            },
        ]);
        items.extend(from_build_note(&build, &build.generated_sources));
        items.extend([
            Item::Group { title: t!("processing-options") },
            Item::Field {
                label: t!("processing-options-field"),
                value: show_options(&settings.options),
                action: set("options"),
                browse: Browse::None,
            },
            note(t!("processing-options-note")),
        ]);
        items.extend(from_build_note(&build, &show_options(&build.options)));
        items.push(Item::Group { title: t!("processing-lombok") });
        items.push(match &build.lombok {
            None => note(t!("processing-lombok-none")),
            Some(Lombok { version, jar: Some(jar), version_declared, .. }) => note(if *version_declared {
                t!("processing-lombok-found", version = version.as_str(), jar = jar.display().to_string())
            } else {
                t!("processing-lombok-newest", version = version.as_str(), jar = jar.display().to_string())
            }),
            Some(Lombok { version, jar: None, build_file, .. }) => Item::Text {
                text: t!(
                    "processing-lombok-missing",
                    version = version.as_str(),
                    file = build_file.display().to_string()
                ),
                tone: Tone::Error,
                spinner: false,
            },
        });
        items.push(note(t!("processing-reach-note")));
        Ok(Form { items, busy: false })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetSettings)?;
    registry.register(SetSettings)?;
    registry.register(CompilerForm)?;
    registry.register(ProcessorsForm)?;
    Ok(())
}

#[cfg(test)]
mod tests;

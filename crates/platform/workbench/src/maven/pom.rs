use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

pub const LIFECYCLE: [&str; 9] =
    ["clean", "validate", "compile", "test", "package", "verify", "install", "site", "deploy"];

pub const FULL_LIFECYCLE: [&str; 30] = [
    "pre-clean",
    "clean",
    "post-clean",
    "validate",
    "initialize",
    "generate-sources",
    "process-sources",
    "generate-resources",
    "process-resources",
    "compile",
    "process-classes",
    "generate-test-sources",
    "process-test-sources",
    "generate-test-resources",
    "process-test-resources",
    "test-compile",
    "process-test-classes",
    "test",
    "prepare-package",
    "package",
    "pre-integration-test",
    "integration-test",
    "post-integration-test",
    "verify",
    "install",
    "deploy",
    "pre-site",
    "site",
    "post-site",
    "site-deploy",
];

/// A Maven project read from its `pom.xml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PomProject {
    /// The `pom.xml`.
    pub pom: PathBuf,
    /// `groupId`, the parent's when the project has none.
    pub group_id: String,
    /// `artifactId`.
    pub artifact_id: String,
    /// `version`, the parent's when the project has none.
    pub version: String,
    /// `packaging`: `jar` when it is not given.
    pub packaging: String,
    /// `name`, else the artifact id: what the tool window shows.
    pub name: String,
    /// The modules' `pom.xml` files, read in turn.
    pub modules: Vec<PomProject>,
    /// The profiles, with whether each is active by default.
    pub profiles: Vec<Profile>,
    /// The dependencies it declares.
    pub dependencies: Vec<Dependency>,
    /// The plug-ins: bound by default by the packaging, then declared by the build.
    pub plugins: Vec<Plugin>,
    /// What `<build>` and the compiler settings say about folders and the language level.
    pub build: PomBuild,
    /// Why the file did not read, when it did not; the rest is then empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The folders of `<build>` as written (empty when Maven's default applies) and the language level.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PomBuild {
    pub directory: String,
    pub output_directory: String,
    pub test_output_directory: String,
    pub source_directory: String,
    pub test_source_directory: String,
    pub resources: Vec<String>,
    pub test_resources: Vec<String>,
    /// `maven.compiler.release`, else `maven.compiler.source`, else the compiler plug-in's `release` or
    /// `source`, else `java.version`, the property a Spring Boot parent passes to the compiler.
    pub language_level: String,
}

/// A profile.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Its id, for `-P`.
    pub id: String,
    /// Whether `<activation><activeByDefault>` is `true`.
    pub active_by_default: bool,
}

/// A declared dependency.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    /// `groupId`.
    pub group_id: String,
    /// `artifactId`.
    pub artifact_id: String,
    /// `version`, empty when a BOM or the parent manages it.
    pub version: String,
    /// `scope`: `compile` when it is not given.
    pub scope: String,
    /// Whether it is `optional`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

/// A plug-in and the goals it offers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    /// `groupId`: `org.apache.maven.plugins` when it is not given.
    pub group_id: String,
    /// `artifactId`.
    pub artifact_id: String,
    /// `version`, when the build gives one.
    pub version: String,
    /// What a goal is prefixed with on the command line: `compiler` for `maven-compiler-plugin`,
    /// `exec` for `exec-maven-plugin`, else `groupId:artifactId`.
    pub prefix: String,
    /// Its goals, as `mvn <prefix>:<goal>` runs them.
    pub goals: Vec<String>,
    /// Whether the packaging binds it by default rather than the build declaring it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub implicit: bool,
}

impl Plugin {
    pub fn goal(&self, goal: &str) -> String {
        format!("{}:{goal}", self.prefix)
    }
}

const KNOWN_GOALS: &[(&str, &[&str])] = &[
    ("maven-clean-plugin", &["clean", "help"]),
    ("maven-resources-plugin", &["resources", "testResources", "copy-resources", "help"]),
    ("maven-compiler-plugin", &["compile", "testCompile", "help"]),
    ("maven-surefire-plugin", &["test", "help"]),
    ("maven-failsafe-plugin", &["integration-test", "verify", "help"]),
    ("maven-jar-plugin", &["jar", "test-jar", "help"]),
    ("maven-war-plugin", &["war", "exploded", "inplace", "help"]),
    ("maven-ear-plugin", &["ear", "generate-application-xml", "help"]),
    ("maven-install-plugin", &["install", "install-file", "help"]),
    ("maven-deploy-plugin", &["deploy", "deploy-file", "help"]),
    (
        "maven-site-plugin",
        &["site", "deploy", "run", "stage", "stage-deploy", "attach-descriptor", "jar", "effective-site", "help"],
    ),
    ("maven-source-plugin", &["jar", "jar-no-fork", "test-jar", "test-jar-no-fork", "aggregate", "help"]),
    (
        "maven-javadoc-plugin",
        &["javadoc", "jar", "test-javadoc", "test-jar", "aggregate", "aggregate-jar", "fix", "help"],
    ),
    (
        "maven-dependency-plugin",
        &[
            "tree",
            "analyze",
            "list",
            "resolve",
            "resolve-plugins",
            "copy",
            "copy-dependencies",
            "unpack",
            "unpack-dependencies",
            "sources",
            "go-offline",
            "purge-local-repository",
            "help",
        ],
    ),
    ("maven-enforcer-plugin", &["enforce", "display-info", "help"]),
    ("maven-shade-plugin", &["shade", "help"]),
    ("maven-assembly-plugin", &["single", "help"]),
    ("maven-antrun-plugin", &["run", "help"]),
    ("maven-release-plugin", &["prepare", "perform", "rollback", "clean", "branch", "update-versions", "help"]),
    ("exec-maven-plugin", &["exec", "java", "help"]),
    (
        "versions-maven-plugin",
        &[
            "display-dependency-updates",
            "display-plugin-updates",
            "display-property-updates",
            "set",
            "update-properties",
            "use-latest-releases",
            "help",
        ],
    ),
    ("build-helper-maven-plugin", &["add-source", "add-test-source", "add-resource", "attach-artifact", "help"]),
    ("spring-boot-maven-plugin", &["run", "repackage", "start", "stop", "build-image", "build-info", "help"]),
    ("quarkus-maven-plugin", &["dev", "build", "test", "help"]),
    ("jacoco-maven-plugin", &["prepare-agent", "report", "check", "help"]),
];

fn implicit_plugins(packaging: &str) -> Vec<&'static str> {
    let mut plugins = vec!["maven-clean-plugin"];
    match packaging {
        "pom" => {}
        "war" => plugins.extend([
            "maven-resources-plugin",
            "maven-compiler-plugin",
            "maven-surefire-plugin",
            "maven-war-plugin",
        ]),
        "ear" => plugins.extend(["maven-ear-plugin"]),
        _ => plugins.extend([
            "maven-resources-plugin",
            "maven-compiler-plugin",
            "maven-surefire-plugin",
            "maven-jar-plugin",
        ]),
    }
    plugins.extend(["maven-install-plugin", "maven-deploy-plugin", "maven-site-plugin", "maven-dependency-plugin"]);
    plugins
}

pub fn prefix(group_id: &str, artifact_id: &str) -> String {
    if let Some(name) = artifact_id.strip_prefix("maven-").and_then(|rest| rest.strip_suffix("-plugin")) {
        return name.to_owned();
    }
    if let Some(name) = artifact_id.strip_suffix("-maven-plugin") {
        return name.to_owned();
    }
    format!("{group_id}:{artifact_id}")
}

fn known_goals(artifact_id: &str) -> Vec<String> {
    KNOWN_GOALS
        .iter()
        .find(|(id, _)| *id == artifact_id)
        .map(|(_, goals)| goals.iter().map(|goal| (*goal).to_owned()).collect())
        .unwrap_or_default()
}

fn pom_build(root: roxmltree::Node<'_, '_>, properties: &BTreeMap<String, String>) -> PomBuild {
    let value = |node: roxmltree::Node<'_, '_>, name: &str| interpolate(&text_of(node, name), properties);
    let mut found = PomBuild::default();
    if let Some(build) = child(root, "build") {
        found.directory = value(build, "directory");
        found.output_directory = value(build, "outputDirectory");
        found.test_output_directory = value(build, "testOutputDirectory");
        found.source_directory = value(build, "sourceDirectory");
        found.test_source_directory = value(build, "testSourceDirectory");
        let folders = |list: &str, item: &'static str| -> Vec<String> {
            child(build, list)
                .into_iter()
                .flat_map(|list| children(list, item))
                .map(|resource| value(resource, "directory"))
                .filter(|directory| !directory.is_empty())
                .collect()
        };
        found.resources = folders("resources", "resource");
        found.test_resources = folders("testResources", "testResource");
    }
    let property = |name: &str| properties.get(name).map(|value| interpolate(value, properties));
    let plugin = child(root, "build")
        .and_then(|build| child(build, "plugins"))
        .into_iter()
        .flat_map(|list| children(list, "plugin"))
        .find(|plugin| text_of(*plugin, "artifactId") == "maven-compiler-plugin")
        .and_then(|plugin| child(plugin, "configuration"));
    found.language_level =
        [property("maven.compiler.release"), property("maven.compiler.source")]
            .into_iter()
            .chain(plugin.iter().flat_map(|configuration| {
                [Some(value(*configuration, "release")), Some(value(*configuration, "source"))]
            }))
            .chain([property("java.version")])
            .flatten()
            .find(|level| !level.is_empty() && !level.contains("${"))
            .unwrap_or_default();
    found
}

pub(super) fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(|child| child.is_element() && child.tag_name().name() == name)
}

pub(super) fn children<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    name: &'a str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    node.children().filter(move |child| child.is_element() && child.tag_name().name() == name)
}

pub(super) fn text_of(node: roxmltree::Node<'_, '_>, name: &str) -> String {
    child(node, name).and_then(|child| child.text()).map(str::trim).unwrap_or_default().to_owned()
}

pub(super) fn interpolate(value: &str, properties: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let Some(end) = rest[start..].find('}') else { break };
        let name = &rest[start + 2..start + end];
        out.push_str(&rest[..start]);
        match properties.get(name) {
            Some(found) if !found.contains("${") => out.push_str(found),
            _ => out.push_str(&rest[start..start + end + 1]),
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

const MAX_DEPTH: usize = 12;

pub fn read(pom: &Path) -> PomProject {
    read_at(pom, 0)
}

fn read_at(pom: &Path, depth: usize) -> PomProject {
    let failed = |error: String| PomProject { pom: pom.to_owned(), error: Some(error), ..PomProject::default() };
    let text = match std::fs::read_to_string(pom) {
        Ok(text) => text,
        Err(error) => return failed(format!("{}: {error}", pom.display())),
    };
    let mut project = match parse(&text) {
        Ok(project) => project,
        Err(error) => return failed(format!("{}: {error}", pom.display())),
    };
    project.pom = pom.to_owned();
    let folder = pom.parent().unwrap_or(Path::new(""));
    if depth < MAX_DEPTH {
        project.modules = module_paths(&text)
            .into_iter()
            .map(|module| {
                let path = folder.join(&module);
                let file = if path.extension().is_some_and(|ext| ext == "xml") { path } else { path.join("pom.xml") };
                read_at(&file, depth + 1)
            })
            .collect();
    }
    project
}

pub(super) fn module_paths(text: &str) -> Vec<String> {
    let Ok(document) = roxmltree::Document::parse(text) else { return Vec::new() };
    let root = document.root_element();
    let mut lists = vec![root];
    if let Some(profiles) = child(root, "profiles") {
        lists.extend(children(profiles, "profile"));
    }
    let mut found: Vec<String> = Vec::new();
    for list in lists {
        for name in ["modules", "subprojects"] {
            if let Some(modules) = child(list, name) {
                for module in modules.children().filter(roxmltree::Node::is_element) {
                    let path = module.text().map(str::trim).unwrap_or_default().to_owned();
                    if !path.is_empty() && !found.contains(&path) {
                        found.push(path);
                    }
                }
            }
        }
    }
    found
}

pub(super) fn coordinates_and_properties(
    root: roxmltree::Node<'_, '_>,
) -> (String, String, String, BTreeMap<String, String>) {
    let parent = child(root, "parent");
    let parent_text = |name: &str| parent.map(|parent| text_of(parent, name)).unwrap_or_default();
    let or_parent = |value: String, name: &str| if value.is_empty() { parent_text(name) } else { value };
    let mut properties: BTreeMap<String, String> = BTreeMap::new();
    if let Some(declared) = child(root, "properties") {
        for property in declared.children().filter(roxmltree::Node::is_element) {
            properties.insert(
                property.tag_name().name().to_owned(),
                property.text().map(str::trim).unwrap_or_default().to_owned(),
            );
        }
    }
    let group_id = or_parent(text_of(root, "groupId"), "groupId");
    let artifact_id = text_of(root, "artifactId");
    let version = or_parent(text_of(root, "version"), "version");
    for (name, value) in [
        ("project.groupId", &group_id),
        ("project.artifactId", &artifact_id),
        ("project.version", &version),
        ("pom.version", &version),
    ] {
        properties.insert(name.to_owned(), value.clone());
    }
    properties.insert("project.parent.version".to_owned(), parent_text("version"));
    properties.insert("project.parent.groupId".to_owned(), parent_text("groupId"));
    (group_id, artifact_id, version, properties)
}

pub fn parse(text: &str) -> Result<PomProject, String> {
    let document = roxmltree::Document::parse(text).map_err(|error| error.to_string())?;
    let root = document.root_element();
    if root.tag_name().name() != "project" {
        return Err(format!("the root element is <{}>, not <project>", root.tag_name().name()));
    }
    let (group_id, artifact_id, version, properties) = coordinates_and_properties(root);
    let value = |node: roxmltree::Node<'_, '_>, name: &str| interpolate(&text_of(node, name), &properties);
    let packaging = match text_of(root, "packaging") {
        packaging if packaging.is_empty() => "jar".to_owned(),
        packaging => packaging,
    };
    let name = match value(root, "name") {
        name if name.is_empty() => artifact_id.clone(),
        name => name,
    };
    let profiles = child(root, "profiles")
        .map(|profiles| {
            children(profiles, "profile")
                .map(|profile| Profile {
                    id: text_of(profile, "id"),
                    active_by_default: child(profile, "activation")
                        .is_some_and(|activation| text_of(activation, "activeByDefault") == "true"),
                })
                .filter(|profile| !profile.id.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let dependencies = child(root, "dependencies")
        .map(|list| {
            children(list, "dependency")
                .map(|dependency| Dependency {
                    group_id: value(dependency, "groupId"),
                    artifact_id: value(dependency, "artifactId"),
                    version: value(dependency, "version"),
                    scope: match text_of(dependency, "scope") {
                        scope if scope.is_empty() => "compile".to_owned(),
                        scope => scope,
                    },
                    optional: text_of(dependency, "optional") == "true",
                })
                .collect()
        })
        .unwrap_or_default();
    let mut plugins: Vec<Plugin> = implicit_plugins(&packaging)
        .into_iter()
        .map(|artifact_id| Plugin {
            group_id: "org.apache.maven.plugins".to_owned(),
            artifact_id: artifact_id.to_owned(),
            version: String::new(),
            prefix: prefix("org.apache.maven.plugins", artifact_id),
            goals: known_goals(artifact_id),
            implicit: true,
        })
        .collect();
    let declared = child(root, "build").and_then(|build| child(build, "plugins"));
    for plugin in declared.into_iter().flat_map(|list| children(list, "plugin")) {
        let group_id = match value(plugin, "groupId") {
            group if group.is_empty() => "org.apache.maven.plugins".to_owned(),
            group => group,
        };
        let artifact_id = value(plugin, "artifactId");
        if artifact_id.is_empty() {
            continue;
        }
        let mut goals = known_goals(&artifact_id);
        for execution in child(plugin, "executions").into_iter().flat_map(|list| children(list, "execution")) {
            for goal in child(execution, "goals").into_iter().flat_map(|list| children(list, "goal")) {
                let goal = goal.text().map(str::trim).unwrap_or_default().to_owned();
                if !goal.is_empty() && !goals.contains(&goal) {
                    goals.push(goal);
                }
            }
        }
        let found = Plugin {
            prefix: prefix(&group_id, &artifact_id),
            version: value(plugin, "version"),
            group_id,
            artifact_id,
            goals,
            implicit: false,
        };
        match plugins
            .iter_mut()
            .find(|known| known.artifact_id == found.artifact_id && known.group_id == found.group_id)
        {
            Some(known) => {
                let mut goals = known.goals.clone();
                goals.extend(found.goals.iter().filter(|goal| !known.goals.contains(goal)).cloned());
                *known = Plugin { goals, ..found };
            }
            None => plugins.push(found),
        }
    }
    let build = pom_build(root, &properties);
    Ok(PomProject {
        pom: PathBuf::new(),
        build,
        group_id,
        artifact_id,
        version,
        packaging,
        name,
        modules: Vec::new(),
        profiles,
        dependencies,
        plugins,
        error: None,
    })
}

pub fn coordinates_at(text: &str, offset: usize) -> Option<(String, String, String)> {
    let document = roxmltree::Document::parse(text).ok()?;
    let node = document
        .descendants()
        .filter(|node| node.is_element() && matches!(node.tag_name().name(), "dependency" | "plugin" | "parent"))
        .rfind(|node| node.range().contains(&offset))?;
    let (_, _, _, properties) = coordinates_and_properties(document.root_element());
    let value = |name: &str| interpolate(&text_of(node, name), &properties);
    let group = match value("groupId") {
        group if group.is_empty() && node.tag_name().name() == "plugin" => "org.apache.maven.plugins".to_owned(),
        group => group,
    };
    Some((group, value("artifactId"), value("version")))
}

pub fn local_pom(repository: &Path, group: &str, artifact: &str, version: &str) -> PathBuf {
    let mut path = repository.to_owned();
    for part in group.split('.') {
        path.push(part);
    }
    path.join(artifact).join(version).join(format!("{artifact}-{version}.pom"))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestPlugins {
    pub arg_line: String,
    pub system_properties: Vec<(String, String)>,
    pub environment: Vec<(String, String)>,
}

pub fn test_plugins(text: &str) -> TestPlugins {
    let Ok(document) = roxmltree::Document::parse(text) else { return TestPlugins::default() };
    let (_, _, _, properties) = coordinates_and_properties(document.root_element());
    let mut found = TestPlugins::default();
    let plugins = document.descendants().filter(|node| {
        node.has_tag_name("plugin")
            && matches!(text_of(*node, "artifactId").as_str(), "maven-surefire-plugin" | "maven-failsafe-plugin")
    });
    for configuration in plugins.filter_map(|plugin| child(plugin, "configuration")) {
        let arg_line = interpolate(&text_of(configuration, "argLine"), &properties);
        if !arg_line.is_empty() && !found.arg_line.split_whitespace().eq(arg_line.split_whitespace()) {
            found.arg_line = format!("{} {arg_line}", found.arg_line).trim().to_owned();
        }
        let pairs = |name: &str| -> Vec<(String, String)> {
            child(configuration, name)
                .map(|node| {
                    node.children()
                        .filter(roxmltree::Node::is_element)
                        .map(|entry| {
                            let value = entry.text().unwrap_or_default().trim();
                            (entry.tag_name().name().to_owned(), interpolate(value, &properties))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        for (name, value) in pairs("systemPropertyVariables") {
            if !found.system_properties.iter().any(|(known, _)| *known == name) {
                found.system_properties.push((name, value));
            }
        }
        for (name, value) in pairs("environmentVariables") {
            if !found.environment.iter().any(|(known, _)| *known == name) {
                found.environment.push((name, value));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>demo</groupId>
  <artifactId>parent</artifactId>
  <version>1.0-SNAPSHOT</version>
  <packaging>pom</packaging>
  <name>Demo Parent</name>
  <modules><module>core</module></modules>
  <profiles>
    <profile><id>fast</id><activation><activeByDefault>true</activeByDefault></activation></profile>
    <profile><id>ci</id><modules><module>extra</module></modules></profile>
  </profiles>
</project>
"#;

    const CORE: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <parent><groupId>demo</groupId><artifactId>parent</artifactId><version>1.0-SNAPSHOT</version></parent>
  <artifactId>core</artifactId>
  <properties><slf4j.version>2.0.13</slf4j.version></properties>
  <dependencies>
    <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>${slf4j.version}</version></dependency>
    <dependency><groupId>junit</groupId><artifactId>junit</artifactId><version>4.13.2</version><scope>test</scope><optional>true</optional></dependency>
    <dependency><groupId>${project.groupId}</groupId><artifactId>api</artifactId><version>${project.version}</version></dependency>
  </dependencies>
  <build><plugins>
    <plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin>
    <plugin><groupId>org.codehaus.mojo</groupId><artifactId>exec-maven-plugin</artifactId><version>3.5.0</version>
      <executions><execution><goals><goal>java</goal><goal>exec</goal></goals></execution></executions></plugin>
    <plugin><groupId>com.example</groupId><artifactId>odd</artifactId><executions><execution><goals><goal>make</goal></goals></execution></executions></plugin>
  </plugins></build>
</project>
"#;

    #[test]
    fn a_project_and_its_modules_are_read_from_their_poms() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), PARENT).unwrap();
        std::fs::create_dir_all(dir.path().join("core")).unwrap();
        std::fs::write(dir.path().join("core/pom.xml"), CORE).unwrap();
        let project = read(&dir.path().join("pom.xml"));
        assert_eq!(project.error, None);
        assert_eq!(
            (
                project.group_id.as_str(),
                project.artifact_id.as_str(),
                project.packaging.as_str(),
                project.name.as_str()
            ),
            ("demo", "parent", "pom", "Demo Parent")
        );
        assert_eq!(
            project.profiles,
            [
                Profile { id: "fast".into(), active_by_default: true },
                Profile { id: "ci".into(), active_by_default: false }
            ]
        );
        assert_eq!(project.modules.len(), 2, "core, and extra of the ci profile");
        let extra = &project.modules[1];
        assert!(extra.error.as_deref().is_some_and(|error| error.contains("extra")), "a missing module says so");
        let core = &project.modules[0];
        assert_eq!(
            (core.group_id.as_str(), core.version.as_str(), core.packaging.as_str()),
            ("demo", "1.0-SNAPSHOT", "jar")
        );
        assert_eq!(core.name, "core", "the artifact id without a name");
        let versions: Vec<(&str, &str, &str)> =
            core.dependencies.iter().map(|d| (d.artifact_id.as_str(), d.version.as_str(), d.scope.as_str())).collect();
        assert_eq!(
            versions,
            [("slf4j-api", "2.0.13", "compile"), ("junit", "4.13.2", "test"), ("api", "1.0-SNAPSHOT", "compile")]
        );
        assert_eq!(core.dependencies[2].group_id, "demo");
        assert!(core.dependencies[1].optional);
        let compiler = core.plugins.iter().find(|p| p.artifact_id == "maven-compiler-plugin").unwrap();
        assert_eq!(
            (compiler.version.as_str(), compiler.implicit),
            ("3.13.0", false),
            "declared in place of the default"
        );
        assert_eq!(compiler.goal("compile"), "compiler:compile");
        let exec = core.plugins.iter().find(|p| p.artifact_id == "exec-maven-plugin").unwrap();
        assert_eq!(
            (exec.prefix.as_str(), exec.goals.as_slice()),
            ("exec", ["exec", "java", "help"].map(String::from).as_slice())
        );
        let odd = core.plugins.iter().find(|p| p.artifact_id == "odd").unwrap();
        assert_eq!(odd.goal("make"), "com.example:odd:make");
        assert!(core.plugins.iter().any(|p| p.artifact_id == "maven-surefire-plugin" && p.implicit));
        assert!(
            !project.plugins.iter().any(|p| p.artifact_id == "maven-compiler-plugin"),
            "a pom packaging compiles nothing"
        );
    }

    #[test]
    fn broken_and_foreign_files_say_why() {
        assert!(parse("<project><artifactId>a</project>").is_err());
        assert!(parse("<settings/>").unwrap_err().contains("<project>"));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><modules><module>.</module></modules><artifactId>loop</artifactId></project>",
        )
        .unwrap();
        let looped = read(&dir.path().join("pom.xml"));
        let mut depth = 0;
        let mut at = &looped;
        while let Some(next) = at.modules.first() {
            depth += 1;
            at = next;
        }
        assert_eq!(depth, MAX_DEPTH, "a module list that loops stops");
    }

    #[test]
    fn interpolation_leaves_unknown_properties() {
        let properties = BTreeMap::from([("a".to_owned(), "1".to_owned()), ("b".to_owned(), "${a}".to_owned())]);
        assert_eq!(interpolate("x-${a}-${c}-${b}", &properties), "x-1-${c}-${b}");
        assert_eq!(interpolate("${unclosed", &properties), "${unclosed");
    }

    #[test]
    fn the_coordinates_under_the_caret_and_their_pom_in_the_local_repository() {
        let at = CORE.find("slf4j-api").unwrap();
        assert_eq!(coordinates_at(CORE, at), Some(("org.slf4j".into(), "slf4j-api".into(), "2.0.13".into())));
        let at = CORE.find("maven-compiler-plugin").unwrap();
        assert_eq!(coordinates_at(CORE, at).unwrap().0, "org.apache.maven.plugins");
        assert_eq!(coordinates_at(CORE, 0), None);
        assert_eq!(local_pom(Path::new("/repo"), "a.b", "c", "1"), Path::new("/repo/a/b/c/1/c-1.pom"));
    }

    #[test]
    fn surefire_and_failsafe_give_their_arg_line_properties_and_environment() {
        let pom = r#"<project>
  <properties><mem>512m</mem></properties>
  <build><plugins>
    <plugin><artifactId>maven-surefire-plugin</artifactId><configuration>
      <argLine>-Xmx${mem} -Dfile.encoding=UTF-8</argLine>
      <systemPropertyVariables><db.url>jdbc:h2:mem</db.url><mode>test</mode></systemPropertyVariables>
      <environmentVariables><TZ>UTC</TZ></environmentVariables>
    </configuration></plugin>
    <plugin><artifactId>maven-failsafe-plugin</artifactId><configuration>
      <systemPropertyVariables><mode>it</mode><it>yes</it></systemPropertyVariables>
    </configuration></plugin>
    <plugin><artifactId>maven-jar-plugin</artifactId><configuration><argLine>no</argLine></configuration></plugin>
  </plugins></build>
</project>"#;
        let found = test_plugins(pom);
        assert_eq!(found.arg_line, "-Xmx512m -Dfile.encoding=UTF-8");
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
        };
        assert_eq!(found.system_properties, pairs(&[("db.url", "jdbc:h2:mem"), ("mode", "test"), ("it", "yes")]));
        assert_eq!(found.environment, pairs(&[("TZ", "UTC")]));
        assert_eq!(test_plugins("<project/>"), TestPlugins::default());
    }
}

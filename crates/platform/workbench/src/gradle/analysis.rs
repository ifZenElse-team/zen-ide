use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::settings::GradleSettings;
use super::{GradleRun, ReloadBuild, ReloadParams, init_script, model};
use crate::build_tools::BuildEnv;
use crate::dependency_analysis::edits::{self, Exclusion, GradleDependency};
use crate::dependency_analysis::{AnalyzedModule, DependencyAnalyzer, DependencyNode, Reason, lost_conflict};

pub const SCRIPT: &str = include_str!("zen-dependencies.gradle");

const CATALOG: &str = "gradle/libs.versions.toml";
const MAX_NODES: usize = 4000;

fn key(root: &Path) -> String {
    format!("gradle.dependencyAnalysis\n{}", root.display())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Edge {
    requested: String,
    requested_version: String,
    group: String,
    module: String,
    selected: Option<String>,
    failure: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Cause {
    cause: String,
    text: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Component {
    id: String,
    group: String,
    module: String,
    version: String,
    project: String,
    causes: Vec<Cause>,
    dependencies: Vec<Edge>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Configuration {
    name: String,
    root: String,
    components: Vec<Component>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Report {
    path: String,
    name: String,
    build_file: PathBuf,
    configurations: Vec<Configuration>,
}

fn reason_of(component: &Component) -> Reason {
    let has = |name: &str| component.causes.iter().any(|cause| cause.cause == name);
    if has("FORCED") {
        Reason::Forced
    } else if has("SELECTED_BY_RULE") {
        Reason::Rule
    } else if has("CONSTRAINT") {
        Reason::Constraint
    } else {
        Reason::Conflict
    }
}

fn note_of(component: &Component) -> String {
    let plain = ["REQUESTED", "ROOT"];
    let said: Vec<&str> = component
        .causes
        .iter()
        .filter(|cause| !plain.contains(&cause.cause.as_str()))
        .map(|cause| cause.text.as_str())
        .collect();
    said.join("; ")
}

fn failed_node(edge: &Edge, scope: &str) -> DependencyNode {
    let mut parts = edge.requested.split(':');
    let group = if edge.group.is_empty() { parts.next().unwrap_or_default() } else { edge.group.as_str() };
    let module = if edge.module.is_empty() { parts.next().unwrap_or_default() } else { edge.module.as_str() };
    DependencyNode {
        group_id: group.to_owned(),
        artifact_id: module.to_owned(),
        requested_version: edge.requested_version.clone(),
        scope: scope.to_owned(),
        omitted: true,
        reason: Some(Reason::Failed),
        note: edge.failure.as_deref().unwrap_or_default().lines().next().unwrap_or_default().to_owned(),
        ..DependencyNode::default()
    }
}

struct Graph<'a> {
    components: HashMap<&'a str, &'a Component>,
    scope: &'a str,
    budget: usize,
}

impl<'a> Graph<'a> {
    fn node(&mut self, edge: &'a Edge, path: &mut Vec<&'a str>) -> Option<DependencyNode> {
        let Some(selected) = edge.selected.as_deref() else { return Some(failed_node(edge, self.scope)) };
        let component = *self.components.get(selected)?;
        let is_project = !component.project.is_empty();
        let version = if is_project { "" } else { component.version.as_str() };
        let conflict = lost_conflict(&edge.requested_version, version);
        let mut node = DependencyNode {
            group_id: component.group.clone(),
            artifact_id: component.module.clone(),
            requested_version: edge.requested_version.clone(),
            selected_version: version.to_owned(),
            scope: self.scope.to_owned(),
            conflict,
            reason: conflict.then(|| reason_of(component)),
            note: if conflict { note_of(component) } else { String::new() },
            project: is_project.then(|| component.project.clone()),
            ..DependencyNode::default()
        };
        if path.contains(&selected) {
            node.omitted = true;
            node.reason = Some(Reason::Cycle);
            return Some(node);
        }
        if self.budget == 0 {
            if !component.dependencies.is_empty() {
                node.omitted = true;
                node.reason = node.reason.or(Some(Reason::Duplicate));
            }
            return Some(node);
        }
        self.budget -= 1;
        path.push(selected);
        node.children = self.nodes(component, path);
        path.pop();
        Some(node)
    }

    fn nodes(&mut self, component: &'a Component, path: &mut Vec<&'a str>) -> Vec<DependencyNode> {
        component.dependencies.iter().filter_map(|edge| self.node(edge, path)).collect()
    }
}

fn configuration_nodes(configuration: &Configuration) -> Vec<DependencyNode> {
    let components: HashMap<&str, &Component> =
        configuration.components.iter().map(|component| (component.id.as_str(), component)).collect();
    let Some(root) = components.get(configuration.root.as_str()).copied() else { return Vec::new() };
    let mut graph = Graph { components, scope: &configuration.name, budget: MAX_NODES };
    graph.nodes(root, &mut vec![configuration.root.as_str()])
}

pub fn parse(output: &str) -> Vec<AnalyzedModule> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("zen-dependencies "))
        .filter_map(|json| serde_json::from_str::<Report>(json).ok())
        .map(|report| AnalyzedModule {
            nodes: report.configurations.iter().flat_map(configuration_nodes).collect(),
            id: report.path,
            name: report.name,
            build_file: Some(report.build_file).filter(|file| file.is_file()),
        })
        .collect()
}

pub struct GradleAnalyzer;

fn wanted(root: &Path, direct: &DependencyNode) -> GradleDependency {
    let catalog = std::fs::read_to_string(root.join(CATALOG)).unwrap_or_default();
    GradleDependency {
        group: direct.group_id.clone(),
        artifact: direct.artifact_id.clone(),
        project: direct.project.clone(),
        accessors: edits::catalog_accessors(&catalog, &direct.group_id, &direct.artifact_id),
    }
}

fn is_catalog(file: &Path) -> bool {
    file.extension().is_some_and(|extension| extension == "toml")
}

impl DependencyAnalyzer for GradleAnalyzer {
    fn id(&self) -> &'static str {
        super::KIND
    }

    fn applies(&self, root: &Path) -> bool {
        model::is_build(root)
    }

    fn key(&self, root: &Path) -> String {
        key(root)
    }

    fn by_configuration(&self) -> bool {
        true
    }

    fn start(&self, session: &mut Session, env: &BuildEnv, root: &Path) -> Result<(), CommandError> {
        let script = init_script("zen-dependencies", SCRIPT).map_err(CommandError::new)?;
        let offline = GradleSettings::read(env, root).offline;
        let configuration = GradleRun::tasks("dependencies", ".", "zenDependencies");
        let mut line = GradleRun::new(env.clone()).command_line(&configuration, root).map_err(CommandError::new)?;
        let mut extra = vec!["-q", "--console=plain", "--no-configuration-cache", "-I"];
        let script = script.display().to_string();
        extra.push(&script);
        if offline {
            extra.push("--offline");
        }
        line.args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        line.display = format!("{} {}", line.display, extra.join(" "));
        let task = session.state_mut::<crate::tasks::Tasks>().start(&t!("workbench-dependency-task"), "gradle", false);
        env.spawn(&key(root), line, Some(task), |output| Ok(output.to_owned()));
        Ok(())
    }

    fn modules(&self, _root: &Path, output: &str) -> Vec<AnalyzedModule> {
        parse(output)
    }

    fn declaring_files(&self, root: &Path, module: &AnalyzedModule) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = module.build_file.iter().cloned().collect();
        files.extend(model::build_file(root).filter(|file| !files.contains(file)));
        files.extend(Some(root.join(CATALOG)).filter(|file| file.is_file()));
        files
    }

    fn declaration(&self, root: &Path, file: &Path, text: &str, direct: &DependencyNode) -> Option<usize> {
        if is_catalog(file) {
            return edits::catalog_declaration(text, &direct.group_id, &direct.artifact_id);
        }
        edits::gradle_declaration(text, &wanted(root, direct))
    }

    fn exclusion(
        &self,
        root: &Path,
        file: &Path,
        text: &str,
        direct: &DependencyNode,
        group: &str,
        artifact: &str,
    ) -> Result<Exclusion, String> {
        if is_catalog(file) {
            return Ok(Exclusion::NotDeclared);
        }
        let kotlin = file.extension().is_some_and(|extension| extension == "kts");
        Ok(edits::gradle_exclusion(text, kotlin, &wanted(root, direct), group, artifact))
    }

    fn reload(&self, session: &mut Session) -> Result<(), CommandError> {
        ReloadBuild.run(session, ReloadParams::default()).map(|_| ())
    }
}

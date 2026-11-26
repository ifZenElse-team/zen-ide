use std::path::{Path, PathBuf};

use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::settings::{MavenSettings, Purpose};
use super::{MavenRun, ReloadParams, ReloadProject, pom, selected_profiles, tree};
use crate::build_tools::BuildEnv;
use crate::dependency_analysis::edits::{self, Exclusion};
use crate::dependency_analysis::{AnalyzedModule, DependencyAnalyzer, DependencyNode, Reason, lost_conflict};

const MAX_PARENTS: usize = 8;

fn key(root: &Path) -> String {
    format!("maven.dependencyAnalysis\n{}", root.display())
}

fn notes_of(text: &str) -> Vec<&str> {
    text.split(';').map(|note| note.trim().trim_matches(['(', ')']).trim()).filter(|note| !note.is_empty()).collect()
}

fn node_of(text: &str) -> Option<DependencyNode> {
    let (coordinates, notes) = match text.strip_prefix('(').and_then(|inner| inner.strip_suffix(')')) {
        Some(inner) => inner.split_once(" - ").unwrap_or((inner, "")),
        None => text.split_once(" (").unwrap_or((text, "")),
    };
    let parsed = tree::artifact(coordinates.trim(), false)?;
    let notes = notes_of(notes);
    let mut node = DependencyNode {
        group_id: parsed.group_id,
        artifact_id: parsed.artifact_id,
        classifier: parsed.classifier,
        requested_version: parsed.version.clone(),
        selected_version: parsed.version,
        scope: parsed.scope,
        note: notes.join("; "),
        ..DependencyNode::default()
    };
    for note in notes {
        if let Some(version) = note.strip_prefix("version managed from ") {
            node.requested_version = version.to_owned();
            node.reason = Some(Reason::Managed);
        } else if let Some(version) = note.strip_prefix("omitted for conflict with ") {
            node.selected_version = version.to_owned();
            node.omitted = true;
            node.reason = Some(Reason::Conflict);
        } else if note == "omitted for duplicate" {
            node.omitted = true;
            node.reason = Some(Reason::Duplicate);
        } else if note == "omitted for cycle" {
            node.omitted = true;
            node.reason = Some(Reason::Cycle);
        }
    }
    node.conflict = lost_conflict(&node.requested_version, &node.selected_version);
    Some(node)
}

pub fn parse(text: &str) -> Vec<(tree::TreeNode, Vec<DependencyNode>)> {
    let mut modules: Vec<(tree::TreeNode, Vec<DependencyNode>)> = Vec::new();
    let mut path: Vec<usize> = Vec::new();
    for raw in text.lines() {
        let line = raw.strip_prefix("[INFO] ").unwrap_or(raw).trim_end();
        if line.is_empty() {
            continue;
        }
        let Some((level, rest)) = tree::depth(line) else {
            if let Some(project) = tree::artifact(line, true) {
                modules.push((project, Vec::new()));
                path.clear();
            }
            continue;
        };
        let (Some((_, roots)), Some(node)) = (modules.last_mut(), node_of(rest)) else { continue };
        if level > path.len() + 1 {
            continue;
        }
        path.truncate(level - 1);
        let mut siblings = roots;
        for index in &path {
            siblings = &mut siblings[*index].children;
        }
        siblings.push(node);
        path.push(siblings.len() - 1);
    }
    modules
}

fn all_projects(project: &pom::PomProject, found: &mut Vec<(String, String, PathBuf)>) {
    found.push((project.group_id.clone(), project.artifact_id.clone(), project.pom.clone()));
    for module in &project.modules {
        all_projects(module, found);
    }
}

pub struct MavenAnalyzer;

impl DependencyAnalyzer for MavenAnalyzer {
    fn id(&self) -> &'static str {
        super::KIND
    }

    fn applies(&self, root: &Path) -> bool {
        root.join("pom.xml").is_file()
    }

    fn key(&self, root: &Path) -> String {
        key(root)
    }

    fn by_configuration(&self) -> bool {
        false
    }

    fn start(&self, session: &mut Session, env: &BuildEnv, root: &Path) -> Result<(), CommandError> {
        let project = pom::read(&root.join("pom.xml"));
        let settings = MavenSettings::read(env, root);
        let profiles = selected_profiles(session, &project).join(",");
        let mut configuration = MavenRun::goals(&project.artifact_id, "pom.xml", "dependency:tree", &profiles);
        configuration.options["offline"] = serde_json::json!(settings.offline);
        let mut line =
            MavenRun::new(env.clone()).line_for(&configuration, root, Purpose::Import).map_err(CommandError::new)?;
        line.args.extend(["-B".to_owned(), "-Dverbose".to_owned()]);
        line.display.push_str(" -B -Dverbose");
        let task = session.state_mut::<crate::tasks::Tasks>().start(&t!("workbench-dependency-task"), "maven", false);
        env.spawn(&key(root), line, Some(task), |output| Ok(output.to_owned()));
        Ok(())
    }

    fn modules(&self, root: &Path, output: &str) -> Vec<AnalyzedModule> {
        let mut projects = Vec::new();
        all_projects(&pom::read(&root.join("pom.xml")), &mut projects);
        parse(output)
            .into_iter()
            .map(|(project, nodes)| AnalyzedModule {
                id: format!("{}:{}", project.group_id, project.artifact_id),
                build_file: projects
                    .iter()
                    .find(|(group, artifact, _)| {
                        *artifact == project.artifact_id && (group.is_empty() || *group == project.group_id)
                    })
                    .map(|(_, _, file)| file.clone()),
                name: project.artifact_id,
                nodes,
            })
            .collect()
    }

    fn declaring_files(&self, _root: &Path, module: &AnalyzedModule) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = module.build_file.iter().cloned().collect();
        while let Some(file) = files.last().filter(|_| files.len() < MAX_PARENTS) {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            match edits::pom_parent(file, &text).filter(|parent| parent.is_file() && !files.contains(parent)) {
                Some(parent) => files.push(parent),
                None => break,
            }
        }
        files
    }

    fn declaration(&self, _root: &Path, _file: &Path, text: &str, direct: &DependencyNode) -> Option<usize> {
        edits::pom_declaration(text, &direct.group_id, &direct.artifact_id)
    }

    fn exclusion(
        &self,
        _root: &Path,
        _file: &Path,
        text: &str,
        direct: &DependencyNode,
        group: &str,
        artifact: &str,
    ) -> Result<Exclusion, String> {
        edits::pom_exclusion(text, &direct.group_id, &direct.artifact_id, group, artifact)
    }

    fn reload(&self, session: &mut Session) -> Result<(), CommandError> {
        ReloadProject.run(session, ReloadParams::default()).map(|_| ())
    }
}

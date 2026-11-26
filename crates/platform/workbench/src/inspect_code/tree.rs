use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_project::inspect::DeclaredInspection;

use super::{InspectionResults, is_inspection, known_inspections, run_of, run_or_latest};
use crate::language::{Problem, Severity};

/// The parameters of `inspect.results.tree`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TreeParams {
    /// The run; the latest when absent.
    pub run: Option<u64>,
    /// Whether files are grouped under their directories.
    pub group_by_directory: bool,
    /// Whether excluded items are listed too.
    pub show_excluded: bool,
}

/// A file of an inspection, with its items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TreeFile {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the project.
    pub relative: String,
    /// The ids of its items.
    pub items: Vec<u64>,
}

/// The files of a directory, or all of an inspection's files when not grouping by directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TreeFolder {
    /// The directory, relative to the project; absent when not grouping by directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    /// Its files.
    pub files: Vec<TreeFile>,
}

/// An inspection of a severity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionNode {
    /// Its id.
    pub id: String,
    /// The name shown.
    pub name: String,
    /// The group it belongs to.
    pub group: String,
    /// What it reports.
    pub description: String,
    /// How many items it has here.
    pub count: usize,
    /// Its files.
    pub folders: Vec<TreeFolder>,
}

/// A severity with its inspections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeverityNode {
    /// The severity.
    pub severity: Severity,
    /// How many items it has.
    pub count: usize,
    /// Its inspections.
    pub inspections: Vec<InspectionNode>,
}

/// The results of a run as a tree: severity, inspection, file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultsTree {
    /// The run.
    pub run: u64,
    /// How many items the tree holds.
    pub count: usize,
    /// The severities, the worst first.
    pub severities: Vec<SeverityNode>,
}

pub(super) fn fallback_name(problem: &Problem) -> (String, String) {
    let source = problem.source.clone().unwrap_or_default();
    let code = problem.code.clone().unwrap_or_default();
    let name = if !code.is_empty() {
        code
    } else if !source.is_empty() {
        source.clone()
    } else {
        t!("workbench-inspect-other")
    };
    (name, source)
}

fn node_of(known: &[DeclaredInspection], id: &str, problem: &Problem) -> InspectionNode {
    let declared = known.iter().find(|inspection| is_inspection(problem, &inspection.id));
    let (name, group) = declared
        .map_or_else(|| fallback_name(problem), |inspection| (inspection.title.clone(), inspection.group.clone()));
    let description = declared.map(|inspection| inspection.description.clone()).unwrap_or_default();
    InspectionNode { id: id.to_owned(), name, group, description, count: 0, folders: Vec::new() }
}

pub(super) fn build(
    results: &InspectionResults,
    known: &[DeclaredInspection],
    by_directory: bool,
    show_excluded: bool,
) -> ResultsTree {
    type Files = BTreeMap<Option<String>, BTreeMap<String, TreeFile>>;
    let mut grouped: BTreeMap<Severity, BTreeMap<String, (InspectionNode, Files)>> = BTreeMap::new();
    let mut count = 0;
    for item in results.items.iter().filter(|item| show_excluded || !item.excluded) {
        count += 1;
        let inspections = grouped.entry(item.problem.severity).or_default();
        let (node, files) = inspections
            .entry(item.inspection.clone())
            .or_insert_with(|| (node_of(known, &item.inspection, &item.problem), Files::new()));
        node.count += 1;
        let directory =
            by_directory.then(|| item.relative.rsplit_once('/').map_or("", |(directory, _)| directory).to_owned());
        files
            .entry(directory)
            .or_default()
            .entry(item.relative.clone())
            .or_insert_with(|| TreeFile { path: item.path.clone(), relative: item.relative.clone(), items: Vec::new() })
            .items
            .push(item.id);
    }
    let severities = grouped
        .into_iter()
        .map(|(severity, inspections)| {
            let mut inspections: Vec<InspectionNode> = inspections
                .into_values()
                .map(|(mut node, files)| {
                    node.folders = files
                        .into_iter()
                        .map(|(directory, files)| TreeFolder { directory, files: files.into_values().collect() })
                        .collect();
                    node
                })
                .collect();
            inspections.sort_by_key(|node| node.name.to_lowercase());
            SeverityNode { severity, count: inspections.iter().map(|node| node.count).sum(), inspections }
        })
        .collect();
    ResultsTree { run: results.run, count, severities }
}

pub struct ResultsTreeCommand;

impl Command for ResultsTreeCommand {
    type Params = TreeParams;
    type Output = ResultsTree;
    const ID: &'static str = "inspect.results.tree";
    const TITLE: &'static str = "Inspection Results Tree";
    const DESCRIPTION: &'static str = "The results of a run as the tree the Problems tool window shows: severity, inspection, \
         file (under its directory with groupByDirectory), with the ids of the items, which inspect.results.list \
         describes.";

    fn run(&self, session: &mut Session, params: TreeParams) -> Result<ResultsTree, CommandError> {
        let run = run_or_latest(session, params.run)?;
        let known = known_inspections(session);
        let results = super::shown_to_caller(session, run_of(session, run)?.clone());
        Ok(build(&results, &known, params.group_by_directory, params.show_excluded))
    }
}

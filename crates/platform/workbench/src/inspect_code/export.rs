use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::{InspectionResults, known_inspections, run_of, run_or_latest, tree};

/// The format of an export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    /// A page to read.
    #[default]
    Html,
    /// XML to process.
    Xml,
}

/// The parameters of `inspect.results.export`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    /// The run; the latest when absent.
    #[serde(default)]
    pub run: Option<u64>,
    /// The format.
    #[serde(default)]
    pub format: ExportFormat,
    /// The file to write.
    pub path: PathBuf,
}

fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            control if control.is_control() && control != '\n' && control != '\t' => {}
            other => out.push(other),
        }
    }
    out
}

fn severity_name(item: &super::ResultItem) -> String {
    serde_json::to_value(item.problem.severity)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub(super) fn xml(results: &InspectionResults) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!("<problems scope=\"{}\">\n", escaped(&results.title)));
    for item in results.items.iter().filter(|item| !item.excluded) {
        out.push_str("  <problem>\n");
        out.push_str(&format!("    <file>{}</file>\n", escaped(&item.relative)));
        out.push_str(&format!("    <line>{}</line>\n", item.problem.line));
        out.push_str(&format!("    <column>{}</column>\n", item.problem.column));
        out.push_str(&format!("    <severity>{}</severity>\n", escaped(&severity_name(item))));
        out.push_str(&format!("    <inspection>{}</inspection>\n", escaped(&item.inspection)));
        out.push_str(&format!("    <description>{}</description>\n", escaped(&item.problem.message)));
        out.push_str("  </problem>\n");
    }
    out.push_str("</problems>\n");
    out
}

pub(super) fn html(results: &InspectionResults, tree: &tree::ResultsTree) -> String {
    let title = t!("workbench-inspect-results-title", scope = results.title.as_str());
    let mut out = format!(
        "<!doctype html>\n<html>\n<head><meta charset=\"utf-8\"><title>{0}</title></head>\n<body>\n<h1>{0}</h1>\n",
        escaped(&title)
    );
    for severity in &tree.severities {
        let name = serde_json::to_value(severity.severity).unwrap_or_default();
        out.push_str(&format!("<h2>{} ({})</h2>\n", escaped(name.as_str().unwrap_or_default()), severity.count));
        for inspection in &severity.inspections {
            out.push_str(&format!("<h3>{} ({})</h3>\n<ul>\n", escaped(&inspection.name), inspection.count));
            let files = inspection.folders.iter().flat_map(|folder| &folder.files);
            for id in files.flat_map(|file| &file.items) {
                let Some(item) = results.items.iter().find(|item| item.id == *id) else { continue };
                out.push_str(&format!(
                    "<li>{}:{}:{} {}</li>\n",
                    escaped(&item.relative),
                    item.problem.line,
                    item.problem.column,
                    escaped(&item.problem.message)
                ));
            }
            out.push_str("</ul>\n");
        }
    }
    out.push_str("</body>\n</html>\n");
    out
}

pub struct ExportResults;

impl Command for ExportResults {
    type Params = ExportParams;
    type Output = usize;
    const ID: &'static str = "inspect.results.export";
    const TITLE: &'static str = "Export Inspection Results";
    const DESCRIPTION: &'static str = "Writes the results of a run to a file as HTML or XML, without the excluded items. Returns \
         how many items it wrote.";

    fn run(&self, session: &mut Session, params: ExportParams) -> Result<usize, CommandError> {
        if zen_core::macros::by_agent(session) {
            let root =
                session.state::<crate::Workbench>().and_then(crate::Workbench::project).map(std::path::Path::to_owned);
            let inside = root.as_deref().is_some_and(|root| crate::inside_project(root, &params.path));
            let (true, Some(root)) = (inside, root) else {
                return Err(CommandError::new(t!(
                    "workbench-error-outside-project",
                    path = params.path.display().to_string()
                )));
            };
            crate::hidden_files::guard_change(session, &root, std::slice::from_ref(&params.path))?;
        }
        let run = run_or_latest(session, params.run)?;
        let known = known_inspections(session);
        let results = super::shown_to_caller(session, run_of(session, run)?.clone());
        let tree = tree::build(&results, &known, false, false);
        let text = match params.format {
            ExportFormat::Html => html(&results, &tree),
            ExportFormat::Xml => xml(&results),
        };
        std::fs::write(&params.path, text).map_err(|error| {
            CommandError::new(t!(
                "workbench-inspect-export-failed",
                path = params.path.display().to_string(),
                error = error.to_string()
            ))
        })?;
        Ok(tree.count)
    }
}

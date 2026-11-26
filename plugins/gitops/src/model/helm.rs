use std::collections::BTreeMap;

use super::{Builder, Completion, File, IndexNode, Reference, file_name, folder_of};
use crate::yaml::{Document, Node, Value};

pub(super) const MAX_KEYS: usize = 2000;

const PREFIX: &str = ".Values.";

pub(super) fn chart_folders(files: &[File]) -> Vec<String> {
    let mut charts: Vec<String> = files
        .iter()
        .filter(|file| file_name(&file.path) == "Chart.yaml")
        .map(|file| folder_of(&file.path).to_owned())
        .collect();
    charts.sort_by_key(|folder| std::cmp::Reverse(folder.len()));
    charts
}

fn templates_of(chart: &str) -> String {
    if chart.is_empty() { "templates/".to_owned() } else { format!("{chart}/templates/") }
}

pub(super) fn chart_of_template<'a>(charts: &'a [String], path: &str) -> Option<&'a str> {
    charts.iter().find(|chart| path.starts_with(&templates_of(chart))).map(String::as_str)
}

fn is_values_file(chart: &str, path: &str) -> bool {
    let name = file_name(path);
    folder_of(path) == chart
        && name.starts_with("values")
        && (name.ends_with(".yaml") || name.ends_with(".yml"))
        && !path.contains("/templates/")
}

struct Key {
    id: String,
    open: bool,
}

fn keys(node: &Node, prefix: &str, found: &mut Vec<(String, usize, usize, bool)>) {
    let Value::Map(entries) = &node.value else { return };
    for entry in entries {
        if found.len() >= MAX_KEYS {
            return;
        }
        let path = if prefix.is_empty() { entry.key.clone() } else { format!("{prefix}.{}", entry.key) };
        let open = !matches!(&entry.value.value, Value::Map(inner) if !inner.is_empty());
        found.push((path.clone(), entry.key_start, entry.key_end, open));
        keys(&entry.value, &path, found);
    }
}

fn chart_name(parsed: &[(&File, Vec<Document>)], chart: &str) -> String {
    let chart_file = if chart.is_empty() { "Chart.yaml".to_owned() } else { format!("{chart}/Chart.yaml") };
    parsed
        .iter()
        .find(|(file, _)| file.path == chart_file)
        .and_then(|(_, docs)| docs.first())
        .and_then(|doc| doc.root.get("name"))
        .and_then(Node::str)
        .map_or_else(|| file_name(chart).to_owned(), str::to_owned)
}

pub(super) fn uses(text: &str) -> Vec<(usize, usize, String)> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = text[at..].find(PREFIX) {
        let start = at + offset;
        let from = start + PREFIX.len();
        let path_end = text[from..]
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.')))
            .map_or(text.len(), |(index, _)| from + index);
        let path = text[from..path_end].trim_end_matches('.');
        let joined = text[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !joined && !path.is_empty() && !path.contains("..") {
            found.push((start, from + path.len(), path.to_owned()));
        }
        at = path_end.max(from);
    }
    found
}

pub(super) fn values(builder: &mut Builder, parsed: &[(&File, Vec<Document>)], charts: &[String]) {
    for chart in charts {
        let mut values_files: Vec<&(&File, Vec<Document>)> =
            parsed.iter().filter(|(file, _)| is_values_file(chart, &file.path)).collect();
        values_files.sort_by_key(|(file, _)| (file_name(&file.path) != "values.yaml", file.path.clone()));
        let mut known: BTreeMap<String, Key> = BTreeMap::new();
        let mut items: Vec<(String, String)> = Vec::new();
        for (file, docs) in &values_files {
            let Some(doc) = docs.first() else { continue };
            let mut found = Vec::new();
            keys(&doc.root, "", &mut found);
            for (path, start, end, open) in found {
                let id = format!("value:{chart}:{path}");
                if !known.contains_key(&path) && known.len() < MAX_KEYS {
                    builder.index.nodes.push(IndexNode {
                        id: id.clone(),
                        kind: "Value".to_owned(),
                        group: "helm",
                        name: path.clone(),
                        namespace: String::new(),
                        path: file.path.clone(),
                        start,
                        end,
                        detail: String::new(),
                        in_graph: false,
                    });
                    items.push((path.clone(), file_name(&file.path).to_owned()));
                    known.insert(path.clone(), Key { id: id.clone(), open });
                }
                if let Some(key) = known.get_mut(&path) {
                    key.open |= open;
                    let target = key.id.clone();
                    builder.index.references.push(Reference { path: file.path.clone(), start, end, target });
                }
            }
        }
        let name = chart_name(parsed, chart);
        let templates: Vec<&File> = parsed
            .iter()
            .map(|(file, _)| *file)
            .filter(|file| chart_of_template(charts, &file.path) == Some(chart.as_str()))
            .collect();
        for template in &templates {
            for (start, end, path) in uses(&template.text) {
                match resolve(&known, &path) {
                    Ok(target) => builder.index.references.push(Reference {
                        path: template.path.clone(),
                        start,
                        end,
                        target: target.to_owned(),
                    }),
                    Err(()) if path == "global" || path.starts_with("global.") => {}
                    Err(()) => builder.index.problems.push(super::Problem {
                        path: template.path.clone(),
                        start,
                        end,
                        error: false,
                        message: format!(
                            "No key {path} in the values files of the chart {name}: it has to come from --set, -f or a parent chart"
                        ),
                    }),
                }
            }
        }
        if !templates.is_empty() && !items.is_empty() {
            builder.index.completions.push(Completion {
                files: templates.iter().map(|file| file.path.clone()).collect(),
                after: PREFIX,
                items,
            });
        }
    }
}

fn resolve<'a>(known: &'a BTreeMap<String, Key>, path: &str) -> Result<&'a str, ()> {
    let mut prefix = String::new();
    for part in path.split('.') {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(part);
        let key = known.get(&prefix).ok_or(())?;
        if key.open || prefix.len() == path.len() {
            return Ok(&key.id);
        }
    }
    Err(())
}

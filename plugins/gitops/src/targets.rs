use serde_json::{Value as Json, json};

use crate::yaml::{self, Node};

fn preview(line: usize, id: &str, name: &str, program: &str, args: &[&str]) -> Json {
    json!({
        "line": line, "id": id, "name": name, "program": program, "args": args,
        "output": "document", "documentType": "yaml",
    })
}

pub fn is_remote(name: &str) -> bool {
    name.contains("://") || name.starts_with("github.com/") || name.starts_with("git@") || name.starts_with("git::")
}

fn names_remote(text: &str) -> bool {
    yaml::documents(text).iter().any(|doc| {
        ["resources", "components", "bases"]
            .iter()
            .flat_map(|key| doc.root.get(key).map(Node::items).unwrap_or_default())
            .filter_map(Node::str)
            .any(is_remote)
    })
}

fn line_of(text: &str, key: &str) -> usize {
    text.lines().position(|line| line.starts_with(key)).unwrap_or(0)
}

pub fn run_targets(path: &str, text: &str) -> Json {
    if let Some(folder) = path.strip_suffix('/') {
        let chart = if folder.is_empty() { "Chart.yaml".to_owned() } else { format!("{folder}/Chart.yaml") };
        if text.lines().any(|line| line == chart) {
            return json!([preview(
                0,
                "helm-template-folder",
                "Preview with helm template",
                "helm",
                &["template", "${file}"]
            )]);
        }
        return json!([]);
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    match name {
        "Chart.yaml" => {
            json!([preview(
                line_of(text, "name:"),
                "helm-template",
                "Preview with helm template",
                "helm",
                &["template", "${fileDir}"]
            )])
        }
        "kustomization.yaml" | "kustomization.yml" | "Kustomization" if !names_remote(text) => {
            json!([preview(
                0,
                "kustomize-build",
                "Preview with kustomize build",
                "kustomize",
                &["build", "${fileDir}"]
            )])
        }
        _ => json!([]),
    }
}

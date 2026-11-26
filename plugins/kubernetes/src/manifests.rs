use serde_json::{Value, json};

pub const PERSON_ONLY: [&str; 15] = [
    "apply", "create", "replace", "patch", "delete", "edit", "scale", "rollout", "label", "annotate", "set", "drain",
    "cordon", "uncordon", "taint",
];

const SCOPE: [&str; 4] = ["--context", "${window.kubernetes.context}", "--namespace", "${window.kubernetes.namespace}"];

const EXTERNAL_DIFF: &str = "diff -N -U1000000";

fn manifest_line(path: &str, text: &str) -> Option<usize> {
    let json = path.to_ascii_lowercase().ends_with(".json");
    let starts = |line: &str, key: &str| {
        if json { line.trim_start().starts_with(&format!("\"{key}\"")) } else { line.starts_with(&format!("{key}:")) }
    };
    let first = text.lines().position(|line| starts(line, "apiVersion"))?;
    text.lines().any(|line| starts(line, "kind")).then_some(first)
}

fn target(line: usize, id: &str, name: &str, verb: &str, folder: bool) -> Value {
    let mut args: Vec<&str> = SCOPE.to_vec();
    args.extend([verb, "-f", "${file}"]);
    if folder {
        args.push("-R");
    }
    let mut target = json!({
        "line": line, "id": id, "name": name, "program": "kubectl", "args": args, "cwd": "${projectDir}", "kind": "main",
    });
    if verb == "diff" {
        target["output"] = json!("diff");
        target["env"] = json!({ "KUBECTL_EXTERNAL_DIFF": EXTERNAL_DIFF });
    }
    target
}

pub fn run_targets(path: &str, text: &str) -> Value {
    if let Some(folder) = path.strip_suffix('/') {
        if folder.is_empty() || text.lines().all(|line| line.trim().is_empty()) {
            return json!([]);
        }
        return json!([
            target(0, "diff-folder", "Diff Folder with Cluster", "diff", true),
            target(0, "apply-folder", "Apply Folder to Cluster", "apply", true),
        ]);
    }
    match manifest_line(path, text) {
        Some(line) => json!([
            target(line, "diff", "Diff with Cluster", "diff", false),
            target(line, "apply", "Apply to Cluster", "apply", false),
        ]),
        None => json!([]),
    }
}

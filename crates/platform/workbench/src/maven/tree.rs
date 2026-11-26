use schemars::JsonSchema;
use serde::Serialize;

/// An artifact of the tree and what it brings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    /// `groupId`.
    pub group_id: String,
    /// `artifactId`.
    pub artifact_id: String,
    /// The packaging or type, e.g. `jar`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The classifier, e.g. `tests`, when it has one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub classifier: String,
    /// The version.
    pub version: String,
    /// The scope, e.g. `compile`; empty for a project.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope: String,
    /// What Maven adds after it, e.g. `(optional)` or `(version managed from 1.0)`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// What it brings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn coordinates(&self) -> String {
        format!("{}:{}:{}", self.group_id, self.artifact_id, self.version)
    }

    pub fn count(&self) -> usize {
        1 + self.children.iter().map(TreeNode::count).sum::<usize>()
    }
}

pub(super) fn artifact(text: &str, is_root: bool) -> Option<TreeNode> {
    let (coordinates, note) = match text.find(" (") {
        Some(at) => (&text[..at], text[at + 1..].trim()),
        None => (text.trim(), ""),
    };
    let parts: Vec<&str> = coordinates.split(':').collect();
    let (classifier, version, scope) = match (parts.len(), is_root) {
        (4, true) => ("", parts[3], ""),
        (5, true) => (parts[3], parts[4], ""),
        (5, false) => ("", parts[3], parts[4]),
        (6, false) => (parts[3], parts[4], parts[5]),
        _ => return None,
    };
    if parts.iter().any(|part| part.is_empty() || part.contains(char::is_whitespace)) {
        return None;
    }
    Some(TreeNode {
        group_id: parts[0].to_owned(),
        artifact_id: parts[1].to_owned(),
        kind: parts[2].to_owned(),
        classifier: classifier.to_owned(),
        version: version.to_owned(),
        scope: scope.to_owned(),
        note: note.to_owned(),
        children: Vec::new(),
    })
}

pub(super) fn depth(line: &str) -> Option<(usize, &str)> {
    let mut level = 0;
    let mut rest = line;
    loop {
        if let Some(after) = rest.strip_prefix("+- ").or_else(|| rest.strip_prefix("\\- ")) {
            return Some((level + 1, after));
        }
        if let Some(after) = rest.strip_prefix("|  ").or_else(|| rest.strip_prefix("   ")) {
            level += 1;
            rest = after;
            continue;
        }
        return None;
    }
}

pub fn parse(text: &str) -> Vec<TreeNode> {
    let mut roots: Vec<TreeNode> = Vec::new();
    let mut path: Vec<usize> = Vec::new();
    for raw in text.lines() {
        let line = raw.strip_prefix("[INFO] ").unwrap_or(raw).trim_end();
        if line.is_empty() {
            continue;
        }
        match depth(line) {
            Some((level, rest)) => {
                let Some(root) = roots.last_mut() else { continue };
                let Some(node) = artifact(rest, false) else { continue };
                if level > path.len() + 1 {
                    continue;
                }
                path.truncate(level - 1);
                let mut parent = root;
                for index in &path {
                    parent = &mut parent.children[*index];
                }
                parent.children.push(node);
                path.push(parent.children.len() - 1);
            }
            None => {
                if let Some(node) = artifact(line, true) {
                    roots.push(node);
                    path.clear();
                }
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: &str = "demo:parent:pom:1.0-SNAPSHOT
demo:core:jar:1.0-SNAPSHOT
+- org.slf4j:slf4j-api:jar:2.0.13:compile
+- com.example:lib:jar:tests:1.2:test (optional)
|  \\- com.example:inner:jar:1.0:test
|     \\- com.example:deep:jar:0.1:test
\\- junit:junit:jar:4.13.2:test
   \\- org.hamcrest:hamcrest-core:jar:1.3:test (version managed from 1.1)
";

    #[test]
    fn the_tree_has_a_root_per_project_and_each_dependency_under_what_brings_it() {
        let roots = parse(TREE);
        assert_eq!(roots.len(), 2);
        assert_eq!((roots[0].coordinates().as_str(), roots[0].kind.as_str()), ("demo:parent:1.0-SNAPSHOT", "pom"));
        let core = &roots[1];
        assert_eq!(core.count(), 7);
        let names: Vec<&str> = core.children.iter().map(|node| node.artifact_id.as_str()).collect();
        assert_eq!(names, ["slf4j-api", "lib", "junit"]);
        let lib = &core.children[1];
        assert_eq!((lib.classifier.as_str(), lib.version.as_str(), lib.note.as_str()), ("tests", "1.2", "(optional)"));
        assert_eq!(lib.children[0].children[0].artifact_id, "deep");
        let hamcrest = &core.children[2].children[0];
        assert_eq!((hamcrest.scope.as_str(), hamcrest.note.as_str()), ("test", "(version managed from 1.1)"));
    }

    #[test]
    fn maven_log_lines_and_noise_are_read_or_skipped() {
        let logged = "[INFO] Finished at: 2026-10-25T22:41:44+07:00\n[INFO] --- dependency:3.11.0:tree (default-cli) @ core ---\n[INFO] demo:core:jar:1.0\n[INFO] \\- a:b:jar:1:compile\n[INFO] BUILD SUCCESS\n";
        let roots = parse(logged);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].children[0].coordinates(), "a:b:1");
        assert!(parse("+- a:b:jar:1:compile\n").is_empty(), "a dependency without a project is skipped");
        assert_eq!(
            parse("x:y:jar:1\n      \\- a:b:jar:1:compile\n")[0].children.len(),
            0,
            "a level skipped is dropped"
        );
    }
}

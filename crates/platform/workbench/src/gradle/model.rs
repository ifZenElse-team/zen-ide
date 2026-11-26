use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

pub const SETTINGS: [&str; 2] = ["settings.gradle", "settings.gradle.kts"];
pub const BUILD: [&str; 2] = ["build.gradle", "build.gradle.kts"];

/// A project of the build.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GradleProject {
    /// The build it belongs to: `:` for the project's own, `:tools` for an included build, `:buildSrc`.
    pub build: String,
    /// Its path from the root build: `:` for the root, `:app`, `:libs:core`, `:tools:core`.
    pub path: String,
    /// Its name: the root's `rootProject.name` (else its folder's name), else the last part of its path.
    pub name: String,
    /// Its folder.
    pub folder: PathBuf,
    /// Its build script, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_file: Option<PathBuf>,
}

/// A task of the build.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GradleTask {
    /// The path of the project it belongs to: `:`, `:app`, `:tools:core`.
    pub project: String,
    /// Its name, e.g. `build`.
    pub name: String,
    /// Its group as the build gives it, e.g. `build`; `other` for a task without one.
    pub group: String,
    /// Its description, when it has one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl From<super::build_model::ModelProject> for GradleProject {
    fn from(project: super::build_model::ModelProject) -> Self {
        Self {
            build: project.build,
            path: project.path,
            name: project.name,
            folder: project.folder,
            build_file: project.build_file,
        }
    }
}

impl GradleTask {
    pub fn path(&self) -> String {
        if self.project == ":" { format!(":{}", self.name) } else { format!("{}:{}", self.project, self.name) }
    }
}

pub fn settings_file(folder: &Path) -> Option<PathBuf> {
    SETTINGS.iter().map(|name| folder.join(name)).find(|path| path.is_file())
}

pub fn build_file(folder: &Path) -> Option<PathBuf> {
    BUILD.iter().map(|name| folder.join(name)).find(|path| path.is_file())
}

pub fn is_build(folder: &Path) -> bool {
    settings_file(folder).is_some() || build_file(folder).is_some()
}

fn strings(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\'' || c == '"' {
            let value: String = chars.by_ref().take_while(|next| *next != c).collect();
            found.push(value);
        }
    }
    found
}

fn code_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_block = false;
    for raw in text.lines() {
        let mut line = String::new();
        let mut rest = raw;
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(end) => {
                        rest = &rest[end + 2..];
                        in_block = false;
                    }
                    None => break,
                }
            }
            match (rest.find("/*"), rest.find("//")) {
                (Some(block), line_comment) if line_comment.is_none_or(|at| block < at) => {
                    line.push_str(&rest[..block]);
                    rest = &rest[block + 2..];
                    in_block = true;
                }
                (_, Some(at)) => {
                    line.push_str(&rest[..at]);
                    break;
                }
                _ => {
                    line.push_str(rest);
                    break;
                }
            }
        }
        lines.push(line);
    }
    lines
}

pub fn read_settings(text: &str) -> (Option<String>, Vec<String>) {
    let mut name = None;
    let mut included = Vec::new();
    for line in code_lines(text) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("rootProject.name") {
            if let Some(value) = strings(rest).into_iter().next() {
                name = Some(value);
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("include") else { continue };
        if !rest.starts_with([' ', '(', '\t']) {
            continue;
        }
        for path in strings(rest) {
            let path = if path.starts_with(':') { path } else { format!(":{path}") };
            if path.len() > 1 && !included.contains(&path) {
                included.push(path);
            }
        }
    }
    (name, included)
}

pub fn projects(root: &Path) -> Vec<GradleProject> {
    let (name, included) = settings_file(root)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| read_settings(&text))
        .unwrap_or_default();
    let root_name =
        name.unwrap_or_else(|| root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
    let mut projects = vec![GradleProject {
        build: ":".to_owned(),
        path: ":".to_owned(),
        name: root_name,
        folder: root.to_owned(),
        build_file: build_file(root),
    }];
    for path in included {
        let folder = path.trim_start_matches(':').split(':').fold(root.to_owned(), |folder, part| folder.join(part));
        let name = path.rsplit(':').next().unwrap_or_default().to_owned();
        projects.push(GradleProject { build: ":".to_owned(), path, name, build_file: build_file(&folder), folder });
    }
    projects
}

pub fn included_builds(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in code_lines(text) {
        let Some(rest) = line.trim().strip_prefix("includeBuild") else { continue };
        if let Some(folder) = strings(rest).into_iter().next().filter(|folder| !found.contains(folder)) {
            found.push(folder);
        }
    }
    found
}

fn nested(tree_path: &str, build: GradleProject) -> GradleProject {
    let inner = if build.path == ":" { String::new() } else { build.path };
    GradleProject { build: tree_path.to_owned(), path: format!("{tree_path}{inner}"), ..build }
}

pub fn builds(root: &Path) -> Vec<GradleProject> {
    let mut found = projects(root);
    let included = settings_file(root)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| included_builds(&text))
        .unwrap_or_default();
    let mut folders: Vec<PathBuf> = included.iter().map(|folder| root.join(folder)).filter(|f| f.is_dir()).collect();
    let source = root.join("buildSrc");
    if source.is_dir() {
        folders.push(source);
    }
    for folder in folders {
        let inner = projects(&folder);
        let tree_path = format!(":{}", inner.first().map(|head| head.name.as_str()).unwrap_or_default());
        found.extend(inner.into_iter().map(|project| nested(&tree_path, project)));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_name_the_root_and_include_projects() {
        let groovy = "// a comment: include 'nope'\nrootProject.name = 'demo'\ninclude 'app', ':libs:core'\n/* include 'old'\n*/\ninclude(\"web\")\nincludeBuild 'tools'\n";
        assert_eq!(
            read_settings(groovy),
            (Some("demo".to_owned()), vec![":app".to_owned(), ":libs:core".to_owned(), ":web".to_owned()])
        );
        let kotlin = "rootProject.name = \"shop\"\ninclude(\":api\", \":impl\")\n";
        assert_eq!(read_settings(kotlin).1, [":api", ":impl"]);
    }

    #[test]
    fn the_projects_of_a_build_are_its_root_and_its_included_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("settings.gradle.kts"),
            "rootProject.name = \"shop\"\ninclude(\":app\", \":libs:core\")\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("app/build.gradle.kts"), "").unwrap();
        let projects = projects(root);
        let found: Vec<(&str, &str)> = projects.iter().map(|p| (p.path.as_str(), p.name.as_str())).collect();
        assert_eq!(found, [(":", "shop"), (":app", "app"), (":libs:core", "core")]);
        assert_eq!(projects[1].build_file, Some(root.join("app/build.gradle.kts")));
        assert_eq!(projects[2].folder, root.join("libs/core"));
        assert!(is_build(root) && !is_build(&root.join("libs")));
    }

    #[test]
    fn included_builds_and_build_src_are_projects_before_gradle_answers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("settings.gradle"),
            "pluginManagement { includeBuild('plugins') }\nrootProject.name = 'shop'\nincludeBuild 'tools'\n// includeBuild 'old'\nincludeBuild 'gone'\n",
        )
        .unwrap();
        for folder in ["tools/core", "plugins", "buildSrc"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join("tools/settings.gradle"), "rootProject.name = 'kit'\ninclude 'core'\n").unwrap();
        std::fs::write(root.join("buildSrc/build.gradle"), "").unwrap();
        let found: Vec<(String, String)> =
            builds(root).into_iter().map(|project| (project.build, project.path)).collect();
        let pair = |build: &str, path: &str| (build.to_owned(), path.to_owned());
        assert_eq!(
            found,
            [pair(":", ":"), pair(":kit", ":kit"), pair(":kit", ":kit:core"), pair(":buildSrc", ":buildSrc")],
            "a line in a block is not read, and a missing folder is left out"
        );
    }
}

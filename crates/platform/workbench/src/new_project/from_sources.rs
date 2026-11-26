use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::INVALID_LOCATION;

const BUILD_FILES: [(&str, &str); 5] = [
    ("pom.xml", "maven"),
    ("settings.gradle.kts", "gradle"),
    ("settings.gradle", "gradle"),
    ("build.gradle.kts", "gradle"),
    ("build.gradle", "gradle"),
];

const PLAIN: &str = "plain";

/// What was found in existing sources.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetectedSources {
    /// The folder that is the project: the one chosen, or the one that holds the chosen file.
    pub root: PathBuf,
    /// `maven`, `gradle` or `plain`.
    pub build: String,
    /// The build file it is read from, relative to `root`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_file: Option<String>,
}

pub fn detect_sources(path: &Path) -> Option<DetectedSources> {
    let root = if path.is_dir() { path } else { path.parent().filter(|_| path.is_file())? };
    let chosen = path.file_name().and_then(|name| name.to_str());
    let found = BUILD_FILES
        .iter()
        .find(|(file, _)| chosen == Some(*file) && path.is_file())
        .or_else(|| BUILD_FILES.iter().find(|(file, _)| root.join(file).is_file()));
    Some(DetectedSources {
        root: root.to_owned(),
        build: found.map_or(PLAIN, |(_, build)| *build).to_owned(),
        build_file: found.map(|(file, _)| (*file).to_owned()),
    })
}

/// Which sources.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SourcesParams {
    /// A folder or a build file in it.
    pub path: String,
}

pub struct FromSources;

impl Command for FromSources {
    type Params = SourcesParams;
    type Output = DetectedSources;
    const ID: &'static str = "project.fromSources.detect";
    const TITLE: &'static str = "Project from Existing Sources";
    const DESCRIPTION: &'static str = "Finds how existing sources are built: `path` is a folder or a build file in it. Answers the folder that is the project and `maven`, `gradle` or `plain`. Writes nothing and trusts nothing (ADR-0205): call `open_project` with `root`, and the IDE loads what it found.";

    fn run(&self, _session: &mut Session, params: SourcesParams) -> Result<DetectedSources, CommandError> {
        let path = super::expanded(&params.path);
        detect_sources(&path).ok_or_else(|| {
            let path = path.display().to_string();
            CommandError::with_code(INVALID_LOCATION, t!("workbench-sources-not-found", path = path))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(files: &[&str]) -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        for file in files {
            std::fs::write(folder.path().join(file), "").unwrap();
        }
        folder
    }

    #[test]
    fn a_folder_is_read_by_the_build_file_it_holds() {
        let maven = with(&["pom.xml"]);
        assert_eq!(detect_sources(maven.path()).unwrap().build, "maven");
        let gradle = with(&["build.gradle.kts"]);
        let found = detect_sources(gradle.path()).unwrap();
        assert_eq!((found.build.as_str(), found.build_file.as_deref()), ("gradle", Some("build.gradle.kts")));
        let plain = with(&["Main.java"]);
        let found = detect_sources(plain.path()).unwrap();
        assert_eq!((found.build.as_str(), found.build_file), ("plain", None));
    }

    #[test]
    fn a_chosen_build_file_decides_and_its_folder_is_the_project() {
        let both = with(&["pom.xml", "build.gradle"]);
        let found = detect_sources(&both.path().join("build.gradle")).unwrap();
        assert_eq!(found.root, both.path());
        assert_eq!(found.build, "gradle");
    }

    #[test]
    fn another_chosen_file_means_its_folder() {
        let maven = with(&["pom.xml", "Main.java"]);
        let found = detect_sources(&maven.path().join("Main.java")).unwrap();
        assert_eq!((found.root.as_path(), found.build.as_str()), (maven.path(), "maven"));
    }

    #[test]
    fn a_path_that_is_not_there_is_refused() {
        assert_eq!(detect_sources(Path::new("/no/such/place")), None);
    }
}

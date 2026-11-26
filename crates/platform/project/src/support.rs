use std::path::{Component, Path};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::run::Configuration;

pub const DEPENDENCY_FIELDS: [&str; 2] = ["dependencies", "devDependencies"];

/// What a plug-in gives the projects it supports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSupport {
    /// How such a project is recognised.
    #[serde(skip_serializing_if = "ProjectMarkers::is_empty")]
    pub markers: ProjectMarkers,
    /// The run configurations it adds to such a project, each a name, a kind and its options, and
    /// optionally the markers of the projects it applies to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub run_configurations: Vec<DeclaredConfiguration>,
    /// The step that prepares such a project for the plug-in's tools, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<ProjectSetup>,
    /// The project's own formatters it runs on Reformat Code and on save (Prettier, ADR-0136).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub formatters: Vec<DeclaredFormatter>,
    /// The build system it loads such a project with: its build scripts and the command that loads
    /// them again (Load Changes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<DeclaredBuild>,
}

/// A build system a plug-in loads projects with, as Maven or Gradle.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DeclaredBuild {
    /// The names of its build scripts anywhere in the project, e.g. `pom.xml`, `build.gradle.kts`.
    pub files: Vec<String>,
    /// The command that loads the project again from them, e.g. `maven.project.reload`.
    pub reload: String,
    /// The files that list a project's modules, the one to edit first; the module's `addModule`
    /// step edits them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub module_files: Vec<String>,
}

impl DeclaredBuild {
    pub fn is_build_file(&self, path: &Path) -> bool {
        path.file_name().and_then(|name| name.to_str()).is_some_and(|name| self.files.iter().any(|file| file == name))
    }
}

/// A formatter that is the project's own program (`node_modules/.bin/<command>` of the project or
/// a folder above it, never installed by the IDE): it reads the file's text on its standard input
/// and writes the formatted text on its standard output.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DeclaredFormatter {
    /// Its id, e.g. `prettier`.
    pub id: String,
    /// Its name, e.g. `Prettier`.
    pub name: String,
    /// The program, looked for in the project's `node_modules/.bin`, e.g. `prettier`.
    pub command: String,
    /// Its arguments; `${file}` stands for the file's path, e.g. `["--stdin-filepath", "${file}"]`.
    pub args: Vec<String>,
    /// The extensions of the files it formats, without the dot.
    pub extensions: Vec<String>,
    /// The on/off setting that makes Reformat Code use it, on by default; always when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub on_reformat: String,
    /// The on/off setting that makes saving a file run it, off by default; never when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub on_save: String,
    /// Whether it only fixes on a save and leaves Reformat Code to the language's formatter.
    pub fixer: bool,
}

impl DeclaredFormatter {
    pub fn formats(&self, name: &str) -> bool {
        name.rsplit_once('.')
            .is_some_and(|(_, extension)| self.extensions.iter().any(|known| known.eq_ignore_ascii_case(extension)))
    }

    pub fn args_for(&self, path: &Path) -> Vec<String> {
        self.args.iter().map(|arg| arg.replace("${file}", &path.to_string_lossy())).collect()
    }
}

/// How a project of a plug-in is recognised: by a file at its root, or by a package it depends on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectMarkers {
    /// Files at the project's root, any of which marks it, e.g. `nuxt.config.ts`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// npm packages, any of which in the `dependencies` or `devDependencies` of the root's
    /// `package.json` marks it, e.g. `nuxt`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<String>,
}

/// A run configuration a plug-in adds, to every project it supports or only to those that also
/// match `when` (React's `vite` configurations, only in a Vite project).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredConfiguration {
    /// The configuration.
    #[serde(flatten)]
    pub configuration: Configuration,
    /// The markers a project must also match for the configuration to be added to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ProjectMarkers>,
}

impl DeclaredConfiguration {
    pub fn applies(&self, root: &Path) -> bool {
        self.when.as_ref().is_none_or(|when| when.matches(root))
    }
}

/// A step that prepares a project for a plug-in's tools, offered while what it creates is missing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSetup {
    /// The file or folder the step creates, relative to the project's root, e.g. `.nuxt`; the step
    /// is offered while it is missing.
    pub creates: String,
    /// The configuration that runs the step.
    pub configuration: Configuration,
}

impl ProjectSupport {
    pub fn is_empty(&self) -> bool {
        self.markers.is_empty()
            && self.run_configurations.is_empty()
            && self.setup.is_none()
            && self.formatters.is_empty()
            && self.build.is_none()
    }

    pub fn supports(&self, root: &Path) -> bool {
        self.markers.matches(root)
    }

    pub fn configurations_for(&self, root: &Path) -> Vec<Configuration> {
        self.run_configurations
            .iter()
            .filter(|declared| declared.applies(root))
            .map(|declared| declared.configuration.clone())
            .collect()
    }

    pub fn needs_setup(&self, root: &Path) -> bool {
        self.setup.as_ref().is_some_and(|setup| !setup.done(root))
    }

    pub fn parse(json: &str) -> Result<Self, String> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let support: Self =
            serde_json::from_str(json).map_err(|error| format!("the project support is invalid: {error}"))?;
        support.check()?;
        Ok(support)
    }

    pub fn check(&self) -> Result<(), String> {
        for file in &self.markers.files {
            if !inside(file) {
                return Err(format!("the marker file `{file}` is not a path inside the project"));
            }
        }
        let setup = self.setup.iter().map(|setup| &setup.configuration);
        let declared = self.run_configurations.iter().map(|declared| &declared.configuration);
        for configuration in declared.chain(setup) {
            if configuration.name.trim().is_empty() || configuration.kind.trim().is_empty() {
                return Err("a run configuration needs a name and a kind".to_owned());
            }
        }
        if let Some(setup) = &self.setup
            && !inside(&setup.creates)
        {
            return Err(format!("the setup's `{}` is not a path inside the project", setup.creates));
        }
        for formatter in &self.formatters {
            let bare = !formatter.command.is_empty() && !formatter.command.contains(['/', '\\']);
            if formatter.id.trim().is_empty() || !bare || formatter.extensions.is_empty() {
                return Err("a formatter needs an id, a program name and the extensions it formats".to_owned());
            }
        }
        if let Some(build) = &self.build {
            let names = build.files.iter().all(|file| !file.is_empty() && !file.contains(['/', '\\']));
            if build.files.is_empty() || !names || build.reload.trim().is_empty() {
                return Err("a build needs the file names of its scripts and the command that reloads them".to_owned());
            }
        }
        Ok(())
    }
}

impl ProjectMarkers {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.packages.is_empty()
    }

    pub fn matches(&self, root: &Path) -> bool {
        if self.files.iter().any(|file| inside(file) && root.join(file).is_file()) {
            return true;
        }
        if self.packages.is_empty() {
            return false;
        }
        let Ok(text) = std::fs::read_to_string(root.join("package.json")) else { return false };
        let dependencies = package_dependencies(&text);
        self.packages.iter().any(|package| dependencies.iter().any(|known| known == package))
    }
}

impl ProjectSetup {
    pub fn done(&self, root: &Path) -> bool {
        inside(&self.creates) && root.join(&self.creates).exists()
    }
}

pub fn package_dependencies(text: &str) -> Vec<String> {
    let Ok(serde_json::Value::Object(manifest)) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    DEPENDENCY_FIELDS
        .iter()
        .filter_map(|field| manifest.get(*field)?.as_object())
        .flat_map(|packages| packages.keys().cloned())
        .collect()
}

fn inside(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty() && path.components().all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nuxt() -> ProjectSupport {
        ProjectSupport::parse(
            r#"{
                "markers": { "files": ["nuxt.config.ts", "nuxt.config.js"], "packages": ["nuxt"] },
                "runConfigurations": [{ "name": "dev", "kind": "npm", "options": { "command": "exec" } }],
                "setup": { "creates": ".nuxt", "configuration": { "name": "prepare", "kind": "npm" } }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn a_marker_file_at_the_root_marks_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let support = nuxt();
        assert!(!support.supports(dir.path()));
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/nuxt.config.ts"), "").unwrap();
        assert!(!support.supports(dir.path()), "only the root counts");
        std::fs::write(dir.path().join("nuxt.config.js"), "export default {}").unwrap();
        assert!(support.supports(dir.path()));
    }

    #[test]
    fn a_dependency_in_package_json_marks_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let support = nuxt();
        let write = |text: &str| std::fs::write(dir.path().join("package.json"), text).unwrap();
        write(r#"{ "dependencies": { "vue": "^3" } }"#);
        assert!(!support.supports(dir.path()));
        write(r#"{ "devDependencies": { "nuxt": "^3.13.0" } }"#);
        assert!(support.supports(dir.path()));
        write(r#"{ "dependencies": { "nuxt": "^4.0.0" } }"#);
        assert!(support.supports(dir.path()));
        write(r#"{ "peerDependencies": { "nuxt": "^4" }, "scripts": { "nuxt": "x" } }"#);
        assert!(!support.supports(dir.path()), "only dependencies and devDependencies count");
        write("{ broken");
        assert!(!support.supports(dir.path()));
    }

    #[test]
    fn the_setup_is_needed_while_what_it_creates_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let support = nuxt();
        assert!(support.needs_setup(dir.path()));
        std::fs::create_dir(dir.path().join(".nuxt")).unwrap();
        assert!(!support.needs_setup(dir.path()));
        assert!(!ProjectSupport::default().needs_setup(dir.path()), "no setup, nothing to do");
    }

    #[test]
    fn declarations_that_cannot_work_are_refused() {
        assert!(ProjectSupport::parse("").unwrap().is_empty());
        for bad in [
            r#"{ "markers": { "files": ["../nuxt.config.ts"] } }"#,
            r#"{ "markers": { "files": ["/etc/passwd"] } }"#,
            r#"{ "runConfigurations": [{ "name": "", "kind": "npm" }] }"#,
            r#"{ "setup": { "creates": "../x", "configuration": { "name": "p", "kind": "npm" } } }"#,
            r#"{ "markers": "nuxt" }"#,
            r#"{ "build": { "files": [], "reload": "maven.project.reload" } }"#,
            r#"{ "build": { "files": ["app/pom.xml"], "reload": "maven.project.reload" } }"#,
            r#"{ "build": { "files": ["pom.xml"], "reload": " " } }"#,
        ] {
            assert!(ProjectSupport::parse(bad).is_err(), "{bad}");
        }
        let round = serde_json::to_string(&nuxt()).unwrap();
        assert_eq!(ProjectSupport::parse(&round).unwrap(), nuxt());
    }

    #[test]
    fn a_configuration_with_when_is_added_only_to_the_projects_it_matches() {
        let dir = tempfile::tempdir().unwrap();
        let support = ProjectSupport::parse(
            r#"{
                "markers": { "packages": ["react"] },
                "runConfigurations": [
                    { "name": "always", "kind": "npm" },
                    { "name": "vite", "kind": "npm", "when": { "files": ["vite.config.ts"], "packages": ["vite"] } }
                ]
            }"#,
        )
        .unwrap();
        let names = |root: &Path| support.configurations_for(root).into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(dir.path()), ["always"]);
        std::fs::write(dir.path().join("package.json"), r#"{ "devDependencies": { "vite": "^6" } }"#).unwrap();
        assert_eq!(names(dir.path()), ["always", "vite"]);
        let round = serde_json::to_value(&support).unwrap();
        assert_eq!(round["runConfigurations"][1]["when"]["packages"], serde_json::json!(["vite"]));
        assert_eq!(round["runConfigurations"][1]["name"], "vite");
        assert!(round["runConfigurations"][0].get("when").is_none());
    }

    #[test]
    fn a_build_names_its_scripts_anywhere_in_the_project() {
        let support =
            ProjectSupport::parse(r#"{ "build": { "files": ["pom.xml"], "reload": "maven.project.reload" } }"#)
                .unwrap();
        let build = support.build.unwrap();
        assert!(build.is_build_file(Path::new("/p/core/pom.xml")));
        assert!(!build.is_build_file(Path::new("/p/core/pom.xml.bak")));
        assert_eq!(build.reload, "maven.project.reload");
    }
}

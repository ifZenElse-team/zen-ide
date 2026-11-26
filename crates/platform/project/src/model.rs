use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What the language modules know about a project's structure.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectModel {
    /// The modules, in the order their files were found.
    pub modules: Vec<Module>,
    /// The workspace file that ties the modules together, when there is one.
    pub workspace: Option<WorkspaceFile>,
    /// The SDK and the dependencies the modules use.
    pub libraries: Vec<Library>,
    /// What could not be read.
    pub problems: Vec<ModelProblem>,
}

/// A module: a folder with a module file, such as `go.mod`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Module {
    /// The module's name, e.g. its import path.
    pub name: String,
    /// The folder the module file is in.
    pub root: PathBuf,
    /// The module file.
    pub file: PathBuf,
    /// The language version the module asks for, e.g. `1.22`.
    pub language_version: Option<String>,
    /// The toolchain the module asks for, e.g. `go1.22.3`.
    pub toolchain: Option<String>,
    /// The modules it requires.
    pub dependencies: Vec<Dependency>,
    /// The module versions it excludes.
    pub excluded: Vec<ExcludedVersion>,
    /// Anything else the language module knows, in its own shape.
    pub details: serde_json::Value,
    /// The source, test and resource roots and the excluded folders the build gives.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<ModuleFolder>,
    /// Where the build puts the compiled sources, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<PathBuf>,
    /// Where the build puts the compiled tests, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_output: Option<PathBuf>,
    /// Whether the folders and the output are the build system's conventions rather than read from
    /// the build, which may change them.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub assumed: bool,
}

/// What a folder of a module is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FolderKind {
    #[default]
    Sources,
    TestSources,
    Resources,
    TestResources,
    /// Left out of the project: build output and caches.
    Excluded,
}

/// A folder of a module with its kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModuleFolder {
    pub path: PathBuf,
    pub kind: FolderKind,
}

/// A module that another module requires.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Dependency {
    /// The required module's name.
    pub name: String,
    /// The required version.
    pub version: String,
    /// Whether it is required only by other dependencies.
    pub indirect: bool,
    /// What replaces it, when something does.
    pub replacement: Option<Replacement>,
    /// Where it is used, as the build names it: `compile`, `test`, `implementation`, `dev`...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// What a module is replaced with: another module or a local folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Replacement {
    /// The replacing module's name, or the folder as written.
    pub name: String,
    /// The replacing version; absent for a local folder.
    pub version: Option<String>,
    /// The local folder, resolved against the module's root.
    pub local_path: Option<PathBuf>,
}

/// A module version that a module excludes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExcludedVersion {
    /// The module's name.
    pub name: String,
    /// The excluded version.
    pub version: String,
}

/// A replacement made by a workspace file for every module it uses.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceReplacement {
    /// The replaced module's name.
    pub name: String,
    /// The replaced version; absent when every version is replaced.
    pub version: Option<String>,
    /// What replaces it.
    pub replacement: Replacement,
}

/// A workspace file, such as `go.work`, that combines several modules.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceFile {
    /// The workspace file.
    pub file: PathBuf,
    /// The language version it asks for.
    pub language_version: Option<String>,
    /// The toolchain it asks for.
    pub toolchain: Option<String>,
    /// The module folders it uses, resolved against its folder.
    pub uses: Vec<PathBuf>,
    /// The replacements it makes.
    pub replacements: Vec<WorkspaceReplacement>,
    /// Anything else the language module knows, in its own shape.
    pub details: serde_json::Value,
}

/// What kind of library a project uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LibraryKind {
    /// The language's SDK, e.g. the Go SDK.
    Sdk,
    /// A dependency downloaded into the package cache.
    #[default]
    Dependency,
}

/// A library a project uses, shown under External Libraries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Library {
    /// Whether it is the SDK or a dependency.
    pub kind: LibraryKind,
    /// The library's name, e.g. a module path.
    pub name: String,
    /// Its version, when known.
    pub version: Option<String>,
    /// The folder its sources are in.
    pub root: PathBuf,
    /// Whether that folder exists, i.e. the library is downloaded.
    pub exists: bool,
    /// The name of the node External Libraries lists a dependency under, e.g. `Crates`; the SDK's
    /// language and "Modules" (`Go Modules`) when absent; empty for right under External Libraries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// What External Libraries shows for it, e.g. `Maven: org.slf4j:slf4j-api:2.0.13`; `name@version`
    /// when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A problem found while reading a module or workspace file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelProblem {
    /// The file it is in.
    pub file: PathBuf,
    /// The line it is on, one-based.
    pub line: u32,
    /// What is wrong.
    pub message: String,
    /// Whether the model was read all the same (only the dependencies could not be resolved): a
    /// warning in the Problems tool window rather than an error.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub warning: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelEnv {
    pub sdk_root: Option<PathBuf>,
    pub sdk_version: Option<String>,
    pub package_cache: Option<PathBuf>,
}

pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &str;

    fn watches(&self, file_name: &str) -> bool;

    fn load(&self, root: &Path, env: &ModelEnv) -> Option<ProjectModel>;

    fn generation(&self, _root: &Path) -> u64 {
        0
    }

    fn runs_programs(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProjectModel {
        ProjectModel {
            modules: vec![Module {
                name: "example.com/app".to_owned(),
                root: PathBuf::from("/p"),
                file: PathBuf::from("/p/go.mod"),
                language_version: Some("1.22".to_owned()),
                toolchain: Some("go1.22.3".to_owned()),
                dependencies: vec![Dependency {
                    name: "example.com/lib".to_owned(),
                    version: "v1.2.3".to_owned(),
                    indirect: true,
                    replacement: Some(Replacement {
                        name: "../lib".to_owned(),
                        version: None,
                        local_path: Some(PathBuf::from("/lib")),
                    }),
                    scope: Some("test".to_owned()),
                }],
                excluded: vec![ExcludedVersion { name: "example.com/bad".to_owned(), version: "v0.1.0".to_owned() }],
                details: serde_json::json!({ "retract": ["v1.0.0"] }),
                folders: vec![ModuleFolder { path: PathBuf::from("/p/src"), kind: FolderKind::TestSources }],
                output: Some(PathBuf::from("/p/out")),
                test_output: None,
                assumed: true,
            }],
            workspace: Some(WorkspaceFile {
                file: PathBuf::from("/go.work"),
                language_version: Some("1.22".to_owned()),
                toolchain: None,
                uses: vec![PathBuf::from("/p")],
                replacements: vec![WorkspaceReplacement {
                    name: "example.com/old".to_owned(),
                    version: None,
                    replacement: Replacement {
                        name: "example.com/new".to_owned(),
                        version: Some("v2.0.0".to_owned()),
                        local_path: None,
                    },
                }],
                details: serde_json::Value::Null,
            }),
            libraries: vec![
                Library {
                    kind: LibraryKind::Sdk,
                    name: "Go".to_owned(),
                    version: Some("1.22.3".to_owned()),
                    root: PathBuf::from("/sdk"),
                    exists: true,
                    group: None,
                    label: None,
                },
                Library {
                    kind: LibraryKind::Dependency,
                    name: "example.com/lib".to_owned(),
                    version: Some("v1.2.3".to_owned()),
                    root: PathBuf::from("/cache/example.com/lib@v1.2.3"),
                    exists: false,
                    group: Some("Crates".to_owned()),
                    label: None,
                },
            ],
            problems: vec![ModelProblem {
                file: PathBuf::from("/p/go.mod"),
                line: 3,
                message: "bad".to_owned(),
                ..ModelProblem::default()
            }],
        }
    }

    #[test]
    fn the_model_survives_json() {
        let model = sample();
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(json["modules"][0]["languageVersion"], "1.22");
        assert_eq!(json["modules"][0]["dependencies"][0]["replacement"]["localPath"], "/lib");
        assert_eq!(json["modules"][0]["dependencies"][0]["scope"], "test");
        assert_eq!(json["modules"][0]["folders"][0]["kind"], "testSources");
        assert!(json["modules"][0].get("testOutput").is_none());
        assert_eq!(json["libraries"][0]["kind"], "sdk");
        assert_eq!(json["libraries"][1]["kind"], "dependency");
        assert!(json["libraries"][0].get("group").is_none());
        assert_eq!(json["libraries"][1]["group"], "Crates");
        assert_eq!(serde_json::from_value::<ProjectModel>(json).unwrap(), model);
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let model: ProjectModel = serde_json::from_str(r#"{ "modules": [{ "name": "m" }] }"#).unwrap();
        assert_eq!(model.modules[0].name, "m");
        assert!(model.modules[0].dependencies.is_empty());
        assert!(model.workspace.is_none());
        assert_eq!(serde_json::from_str::<ProjectModel>("{}").unwrap(), ProjectModel::default());
    }

    #[test]
    fn the_model_has_a_schema() {
        let schema = serde_json::to_value(schemars::schema_for!(ProjectModel)).unwrap();
        assert!(schema.to_string().contains("languageVersion"));
    }
}

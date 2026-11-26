use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde_json::json;
use zen_i18n::t;
use zen_project::model::{
    Dependency, FolderKind, Library, LibraryKind, ModelEnv, ModelProblem, ModelProvider, Module, ModuleFolder,
    ProjectModel, Replacement, WorkspaceFile,
};

use crate::cargo::{self, Metadata, MetadataError, Package};
use crate::toolchain::{ProjectRust, Toolchain, rust};

const WATCHED: &[&str] = &["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain"];

#[derive(Default)]
pub struct CargoModels {
    toolchain: Toolchain,
    cache: Mutex<HashMap<PathBuf, (Fingerprint, ProjectModel)>>,
    loading: Mutex<std::collections::HashSet<PathBuf>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    generation: u64,
    files: Vec<(PathBuf, Option<SystemTime>)>,
}

impl Fingerprint {
    fn of(generation: u64, files: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut files: Vec<PathBuf> = files.into_iter().collect();
        files.sort();
        files.dedup();
        let files = files
            .into_iter()
            .map(|file| {
                let modified = std::fs::metadata(&file).and_then(|meta| meta.modified()).ok();
                (file, modified)
            })
            .collect();
        Self { generation, files }
    }

    fn is_current(&self, generation: u64) -> bool {
        *self == Self::of(generation, self.files.iter().map(|(file, _)| file.clone()))
    }
}

impl CargoModels {
    pub fn new(toolchain: Toolchain) -> Self {
        Self { toolchain, cache: Mutex::default(), loading: Mutex::default() }
    }

    pub fn known(self: &std::sync::Arc<Self>, root: &Path) -> Option<ProjectModel> {
        let generation = self.toolchain.generation();
        let (model, current) = {
            let cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match cache.get(root) {
                Some((fingerprint, model)) => (Some(model.clone()), fingerprint.is_current(generation)),
                None => (None, false),
            }
        };
        if !current && root.join("Cargo.toml").is_file() {
            let mut loading = self.loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if loading.insert(root.to_owned()) {
                let (models, folder) = (std::sync::Arc::clone(self), root.to_owned());
                let started = std::thread::Builder::new().name("zen-cargo-model".to_owned()).spawn(move || {
                    models.load(&folder, &ModelEnv::default());
                    models.loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&folder);
                });
                if started.is_err() {
                    loading.remove(root);
                }
            }
        }
        model
    }
}

impl ModelProvider for CargoModels {
    fn id(&self) -> &str {
        "cargo"
    }

    fn watches(&self, file_name: &str) -> bool {
        file_name.rsplit(['/', '\\']).next().is_some_and(|name| WATCHED.contains(&name))
    }

    fn runs_programs(&self) -> bool {
        true
    }

    fn load(&self, root: &Path, _env: &ModelEnv) -> Option<ProjectModel> {
        if !root.join("Cargo.toml").is_file() {
            return None;
        }
        let generation = self.toolchain.generation();
        let cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((fingerprint, model)) = cache.get(root)
            && fingerprint.is_current(generation)
        {
            return Some(model.clone());
        }
        drop(cache);
        let project = self.toolchain.project(root);
        let model = load(root, &project);
        let mut files = watched_files(root, &model);
        if model.problems.iter().any(|problem| problem.warning)
            && let Some(home) = self.toolchain.cargo_home()
        {
            files.extend(downloads(&home));
        }
        let fingerprint = Fingerprint::of(generation, files);
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(root.to_owned(), (fingerprint, model.clone()));
        Some(model)
    }
}

fn watched_files(root: &Path, model: &ProjectModel) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = WATCHED.iter().map(|name| root.join(name)).collect();
    files.extend(model.modules.iter().map(|module| module.file.clone()));
    if let Some(workspace) = &model.workspace
        && let Some(folder) = workspace.file.parent()
    {
        files.extend(WATCHED.iter().map(|name| folder.join(name)));
    }
    files.extend(model.problems.iter().map(|problem| problem.file.clone()));
    files.extend(model.problems.iter().flat_map(|problem| named_manifests(&problem.message)));
    files.sort();
    files.dedup();
    files
}

fn downloads(cargo_home: &Path) -> Vec<PathBuf> {
    let cache = cargo_home.join("registry/cache");
    let mut folders = vec![cache.clone(), cargo_home.join("git/db")];
    if let Ok(entries) = std::fs::read_dir(&cache) {
        folders.extend(entries.flatten().map(|entry| entry.path()));
    }
    folders
}

pub fn load(root: &Path, project: &ProjectRust) -> ProjectModel {
    let mut model = ProjectModel::default();
    let metadata = match cargo::metadata(root, project, true) {
        Ok(metadata) => metadata,
        Err(error) => {
            model.problems.push(problem(root, &error));
            match cargo::metadata(root, project, false) {
                Ok(metadata) => {
                    model.problems = vec![unresolved(root, &error)];
                    metadata
                }
                Err(_) => return model,
            }
        }
    };
    from_metadata(root, &metadata, project, &mut model);
    model
}

fn problem(root: &Path, error: &MetadataError) -> ModelProblem {
    let file = error.manifest.as_deref().map_or_else(|| root.join("Cargo.toml"), normalized);
    let line = error.line.or_else(|| member_line(&file, &error.message)).unwrap_or(1);
    ModelProblem { file, line, message: error.message.clone(), warning: false }
}

fn normalized(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir if clean.file_name().is_some() => {
                clean.pop();
            }
            std::path::Component::CurDir => {}
            _ => clean.push(part),
        }
    }
    clean
}

fn named_manifests(message: &str) -> Vec<PathBuf> {
    message
        .split('`')
        .skip(1)
        .step_by(2)
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .map(|path| if path.ends_with("Cargo.toml") { path.to_owned() } else { path.join("Cargo.toml") })
        .collect()
}

fn member_line(manifest: &Path, message: &str) -> Option<u32> {
    let member = message.split_once("workspace member `")?.1.split('`').next()?;
    let relative = Path::new(member).strip_prefix(manifest.parent()?).ok()?;
    let quoted = format!("\"{}\"", relative.to_string_lossy());
    let text = std::fs::read_to_string(manifest).ok()?;
    text.lines().position(|line| line.contains(&quoted)).map(|index| index as u32 + 1)
}

fn unresolved(root: &Path, error: &MetadataError) -> ModelProblem {
    let said = error.message.split(": note: ").next().unwrap_or_default();
    let said = said.split(": help: ").next().unwrap_or_default();
    let (file, line) = dependency_place(said).unwrap_or_else(|| (root.join("Cargo.toml"), 1));
    ModelProblem { file, line, message: t!("rust-model-offline", message = said), warning: true }
}

fn dependency_place(message: &str) -> Option<(PathBuf, u32)> {
    let manifest = message.split("package `").skip(1).find_map(|rest| {
        let package = rest.split('`').next()?;
        let folder = package.rsplit_once(" (")?.1.strip_suffix(')')?;
        Some(PathBuf::from(folder).join("Cargo.toml"))
    })?;
    let text = std::fs::read_to_string(&manifest).ok()?;
    let name = ["named `", "requirement `", "failed to get `"].iter().find_map(|lead| {
        let name = message.split_once(lead)?.1.split(['`', ' ']).next()?;
        (!name.is_empty()).then_some(name)
    });
    let line = name.and_then(|name| declared_at(&text, name)).unwrap_or(1);
    Some((manifest, line))
}

fn declared_at(text: &str, name: &str) -> Option<u32> {
    let mut in_dependencies = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(table) = line.strip_prefix('[') {
            let table = table.trim_start_matches('[').trim_end_matches(']').trim();
            if table.contains("dependencies") && table.rsplit('.').next() == Some(name) {
                return Some(index as u32 + 1);
            }
            in_dependencies = table.ends_with("dependencies");
            continue;
        }
        let declares = line.strip_prefix(name).is_some_and(|rest| rest.trim_start().starts_with(['=', '.']));
        if in_dependencies && declares {
            return Some(index as u32 + 1);
        }
    }
    None
}

pub fn from_metadata(root: &Path, metadata: &Metadata, project: &ProjectRust, model: &mut ProjectModel) {
    let members: Vec<&Package> = metadata
        .workspace_members
        .iter()
        .filter_map(|id| metadata.packages.iter().find(|package| package.id == *id))
        .collect();
    for package in &members {
        model.modules.push(module(package, metadata));
    }
    let workspace_manifest = metadata.workspace_root.join("Cargo.toml");
    let virtual_or_shared = std::fs::read_to_string(&workspace_manifest)
        .is_ok_and(|text| text.lines().any(|line| line.trim() == "[workspace]"));
    if virtual_or_shared {
        model.workspace = Some(WorkspaceFile {
            file: workspace_manifest,
            language_version: None,
            toolchain: cargo::toolchain_channel(&metadata.workspace_root).or_else(|| cargo::toolchain_channel(root)),
            uses: members.iter().map(|package| package.root()).collect(),
            replacements: Vec::new(),
            details: json!({
                "targetDirectory": metadata.target_directory,
                "defaultMembers": metadata
                    .workspace_default_members
                    .iter()
                    .filter_map(|id| metadata.packages.iter().find(|package| package.id == *id))
                    .map(|package| package.name.clone())
                    .collect::<Vec<_>>(),
            }),
        });
    }
    if let Some(sysroot) = &project.sysroot {
        let sources = rust::library_sources(sysroot);
        model.libraries.push(Library {
            kind: LibraryKind::Sdk,
            name: "Rust".to_owned(),
            version: project.version.clone(),
            exists: sources.is_dir(),
            root: sources,
            group: None,
            label: None,
        });
    }
    let mut dependencies: Vec<&Package> =
        metadata.packages.iter().filter(|package| !metadata.workspace_members.contains(&package.id)).collect();
    dependencies.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    let group = t!("rust-libraries-crates");
    for package in dependencies {
        let root = package.root();
        model.libraries.push(Library {
            kind: LibraryKind::Dependency,
            name: package.name.clone(),
            version: Some(package.version.clone()),
            exists: root.is_dir(),
            root,
            label: None,
            group: Some(group.clone()),
        });
    }
}

fn module(package: &Package, metadata: &Metadata) -> Module {
    let dependencies = package
        .dependencies
        .iter()
        .map(|dependency| Dependency {
            name: dependency.name.clone(),
            version: dependency.req.clone(),
            indirect: false,
            replacement: dependency.path.as_ref().map(|path| Replacement {
                name: path.display().to_string(),
                version: None,
                local_path: Some(path.clone()),
            }),
            scope: dependency.kind.clone(),
        })
        .collect();
    let targets: Vec<serde_json::Value> = package
        .targets
        .iter()
        .map(|target| {
            json!({
                "name": target.name,
                "kind": target.kind,
                "srcPath": target.src_path,
                "test": target.test,
                "doctest": target.doctest,
                "requiredFeatures": target.required_features,
            })
        })
        .collect();
    let kinds: Vec<serde_json::Value> = package
        .dependencies
        .iter()
        .map(|dependency| json!({ "name": dependency.name, "kind": dependency.kind, "optional": dependency.optional }))
        .collect();
    let root = package.root();
    let folders = [
        ("src", FolderKind::Sources),
        ("examples", FolderKind::Sources),
        ("tests", FolderKind::TestSources),
        ("benches", FolderKind::TestSources),
    ]
    .into_iter()
    .map(|(name, kind)| ModuleFolder { path: root.join(name), kind })
    .filter(|folder| folder.path.is_dir())
    .chain(
        (metadata.target_directory.starts_with(&root))
            .then(|| ModuleFolder { path: metadata.target_directory.clone(), kind: FolderKind::Excluded }),
    )
    .collect();
    Module {
        name: package.name.clone(),
        folders,
        output: Some(metadata.target_directory.clone()),
        test_output: None,
        assumed: false,
        root,
        file: package.manifest_path.clone(),
        language_version: package.rust_version.clone(),
        toolchain: None,
        dependencies,
        excluded: Vec::new(),
        details: json!({
            "version": package.version,
            "edition": package.edition,
            "targets": targets,
            "features": package.features.keys().collect::<Vec<_>>(),
            "dependencyKinds": kinds,
            "defaultMember": metadata.workspace_default_members.contains(&package.id),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> Metadata {
        let json = r#"{"packages":[
            {"name":"app","version":"0.1.0","id":"app-id","source":null,"manifest_path":"/w/app/Cargo.toml",
             "edition":"2024","rust_version":"1.85",
             "dependencies":[{"name":"serde","req":"^1","kind":null,"optional":false},
                             {"name":"core-lib","req":"*","kind":"dev","optional":false,"path":"/w/core"}],
             "targets":[{"name":"app","kind":["bin"],"src_path":"/w/app/src/main.rs","test":true,"doctest":false}],
             "features":{"default":[]}},
            {"name":"core-lib","version":"0.2.0","id":"core-id","source":null,"manifest_path":"/w/core/Cargo.toml",
             "edition":"2021","dependencies":[],
             "targets":[{"name":"core_lib","kind":["lib"],"src_path":"/w/core/src/lib.rs","test":true,"doctest":true}]},
            {"name":"serde","version":"1.0.228","id":"serde-id","source":"registry+https://github.com/rust-lang/crates.io-index",
             "manifest_path":"/cargo/registry/src/index/serde-1.0.228/Cargo.toml","edition":"2021","dependencies":[],"targets":[]},
            {"name":"anyhow","version":"1.0.99","id":"anyhow-id","source":"registry+https://github.com/rust-lang/crates.io-index",
             "manifest_path":"/cargo/registry/src/index/anyhow-1.0.99/Cargo.toml","edition":"2021","dependencies":[],"targets":[]}],
            "workspace_members":["app-id","core-id"],"workspace_default_members":["app-id"],
            "target_directory":"/w/target","workspace_root":"/w"}"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn members_are_modules_and_the_rest_are_libraries() {
        zen_i18n::register(crate::LOCALES);
        let project = ProjectRust {
            version: Some("1.99.1".to_owned()),
            sysroot: Some(PathBuf::from("/rustup/toolchains/stable")),
            ..ProjectRust::default()
        };
        let mut model = ProjectModel::default();
        from_metadata(Path::new("/w"), &metadata(), &project, &mut model);
        let names: Vec<&str> = model.modules.iter().map(|module| module.name.as_str()).collect();
        assert_eq!(names, ["app", "core-lib"]);
        let app = &model.modules[0];
        assert_eq!(app.root, PathBuf::from("/w/app"));
        assert_eq!(app.file, PathBuf::from("/w/app/Cargo.toml"));
        assert_eq!(app.language_version.as_deref(), Some("1.85"));
        assert_eq!(app.details["edition"], "2024");
        assert_eq!(app.details["targets"][0]["kind"][0], "bin");
        assert_eq!(app.details["defaultMember"], true);
        assert_eq!(app.dependencies[0].name, "serde");
        assert_eq!(app.dependencies[0].version, "^1");
        assert_eq!(app.dependencies[1].replacement.as_ref().unwrap().local_path, Some(PathBuf::from("/w/core")));
        assert_eq!((app.dependencies[0].scope.as_deref(), app.dependencies[1].scope.as_deref()), (None, Some("dev")));
        assert_eq!(app.output, Some(PathBuf::from("/w/target")));
        assert!(app.folders.is_empty(), "the folders that do not exist are left out, the target is outside the member");

        let sdk = &model.libraries[0];
        assert_eq!((sdk.kind, sdk.name.as_str(), sdk.version.as_deref()), (LibraryKind::Sdk, "Rust", Some("1.99.1")));
        assert_eq!(sdk.root, PathBuf::from("/rustup/toolchains/stable/lib/rustlib/src/rust/library"));
        let crates: Vec<(String, Option<String>)> =
            model.libraries[1..].iter().map(|library| (library.name.clone(), library.version.clone())).collect();
        assert_eq!(
            crates,
            [("anyhow".to_owned(), Some("1.0.99".to_owned())), ("serde".to_owned(), Some("1.0.228".to_owned()))]
        );
        assert_eq!(model.libraries[2].root, PathBuf::from("/cargo/registry/src/index/serde-1.0.228"));
        assert_eq!(model.libraries[2].group.as_deref(), Some(t!("rust-libraries-crates").as_str()));
        assert!(model.workspace.is_none(), "no /w/Cargo.toml on this computer");
    }

    #[test]
    fn a_broken_member_manifest_is_watched_so_that_its_fix_is_picked_up() {
        let model = ProjectModel {
            problems: vec![ModelProblem {
                file: PathBuf::from("/w/core/Cargo.toml"),
                line: 3,
                message: "invalid string".to_owned(),
                warning: false,
            }],
            ..ProjectModel::default()
        };
        let files = watched_files(Path::new("/w"), &model);
        assert!(model.modules.is_empty(), "a failed `cargo metadata` lists no members");
        assert!(files.contains(&PathBuf::from("/w/core/Cargo.toml")), "{files:?}");
        assert!(files.contains(&PathBuf::from("/w/Cargo.toml")), "{files:?}");
    }

    #[test]
    fn only_cargo_files_are_watched() {
        let models = CargoModels::default();
        for name in ["Cargo.toml", "/w/app/Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
            assert!(models.watches(name), "{name}");
        }
        for name in ["main.rs", "go.mod", "cargo.toml.bak"] {
            assert!(!models.watches(name), "{name}");
        }
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(models.load(temp.path(), &ModelEnv::default()), None, "not a Cargo project");
    }

    #[test]
    fn a_run_context_never_waits_for_cargo_metadata() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let models = std::sync::Arc::new(CargoModels::default());
        assert_eq!(models.known(temp.path()), None, "nothing is known before a load, and nothing waits for it");
        let loaded = models.load(temp.path(), &ModelEnv::default());
        assert_eq!(models.known(temp.path()), loaded);
    }

    #[test]
    fn a_problem_outside_the_project_names_its_manifest_plainly() {
        let error = MetadataError {
            message: "string values must be quoted".to_owned(),
            manifest: Some(PathBuf::from("/w/project/../outside/./Cargo.toml")),
            line: Some(3),
        };
        let problem = problem(Path::new("/w/project"), &error);
        assert_eq!((problem.file, problem.line), (PathBuf::from("/w/outside/Cargo.toml"), 3));
        assert_eq!(normalized(Path::new("../a/../b")), PathBuf::from("../b"));
    }
}

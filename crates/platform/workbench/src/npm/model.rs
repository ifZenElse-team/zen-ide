use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use zen_project::model::{
    Dependency, FolderKind, Library, LibraryKind, ModelEnv, ModelProblem, ModelProvider, Module, ModuleFolder,
    ProjectModel,
};

pub const GROUP: &str = "node_modules";

const WATCHED: &[&str] = &[
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
];

const SCOPES: [(&str, Option<&str>); 4] = [
    ("dependencies", None),
    ("devDependencies", Some("dev")),
    ("peerDependencies", Some("peer")),
    ("optionalDependencies", Some("optional")),
];

const MOST_PACKAGES: usize = 500;
const GLOB_DEPTH: usize = 4;

/// A script of a `package.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub name: String,
    /// The command line it stands for.
    pub command: String,
    /// Its line in the file, from 0.
    pub line: u32,
}

/// One `package.json` of the project: the root's, a workspace's or one in a folder of the root.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    /// `name` in the file; the folder's name when it has none.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the project.
    pub relative: String,
    /// The package manager its runs use when a configuration says `auto`.
    pub manager: String,
    pub scripts: Vec<Script>,
    /// The dependencies it declares, with the installed version where there is one.
    pub dependencies: Vec<Dependency>,
    /// Why the file cannot be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn is_package_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    parts.len() <= 2 && parts.iter().all(|part| !part.is_empty() && !part.starts_with('.') && !part.contains('\\'))
}

fn dependencies(manifest: &Value, root: &Path, project: &Path) -> Vec<(Dependency, Option<PathBuf>)> {
    let mut found: Vec<(Dependency, Option<PathBuf>)> = Vec::new();
    for (key, scope) in SCOPES {
        let Some(declared) = manifest.get(key).and_then(Value::as_object) else { continue };
        for (name, range) in declared {
            if !is_package_name(name) || found.iter().any(|(dependency, _)| dependency.name == *name) {
                continue;
            }
            let folder = installed_folder(root, project, name);
            let version = folder
                .as_deref()
                .and_then(installed_version)
                .unwrap_or_else(|| range.as_str().unwrap_or_default().to_owned());
            let dependency =
                Dependency { name: name.clone(), version, scope: scope.map(str::to_owned), ..Dependency::default() };
            found.push((dependency, folder));
        }
    }
    found
}

fn installed_folder(root: &Path, project: &Path, name: &str) -> Option<PathBuf> {
    let stays_inside = Path::new(name).components().all(|part| matches!(part, std::path::Component::Normal(_)));
    if !stays_inside {
        return None;
    }
    root.ancestors()
        .take_while(|folder| folder.starts_with(project))
        .map(|folder| folder.join("node_modules").join(name))
        .find(|folder| folder.is_dir())
}

fn installed_version(folder: &Path) -> Option<String> {
    let bytes = std::fs::read(folder.join("package.json")).ok()?;
    let manifest: Value = serde_json::from_slice(&bytes).ok()?;
    manifest.get("version")?.as_str().map(str::to_owned)
}

fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn folder_name(folder: &Path) -> String {
    folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn read_package(project: &Path, path: &Path) -> Package {
    let folder = path.parent().unwrap_or(project);
    let mut package = Package {
        name: folder_name(folder),
        path: path.to_owned(),
        relative: relative_to(project, path),
        manager: super::package_manager(folder).to_owned(),
        ..Package::default()
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            package.error = Some(error.to_string());
            return package;
        }
    };
    let manifest: Value = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            package.error = Some(error.to_string());
            return package;
        }
    };
    if let Some(name) = manifest.get("name").and_then(Value::as_str).filter(|name| !name.is_empty()) {
        package.name = name.to_owned();
    }
    package.version = manifest.get("version").and_then(Value::as_str).map(str::to_owned);
    let commands = manifest.get("scripts").and_then(Value::as_object);
    package.scripts = super::scripts(&text)
        .into_iter()
        .map(|(name, line)| {
            let command = commands
                .and_then(|commands| commands.get(&name))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Script { name, command, line }
        })
        .collect();
    package.dependencies =
        dependencies(&manifest, folder, project).into_iter().map(|(dependency, _)| dependency).collect();
    package
}

fn workspace_globs(root: &Path) -> Vec<String> {
    let manifest = std::fs::read(root.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or_default();
    let declared = manifest.get("workspaces");
    let list = declared.and_then(|workspaces| workspaces.get("packages").or(Some(workspaces)));
    let mut globs: Vec<String> =
        list.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    if let Ok(text) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) {
        globs.extend(pnpm_globs(&text));
    }
    globs
}

fn pnpm_globs(text: &str) -> Vec<String> {
    let mut within = false;
    let mut globs = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !line.starts_with([' ', '\t', '-']) && !trimmed.is_empty() {
            within = trimmed.starts_with("packages:");
            continue;
        }
        if within && let Some(item) = trimmed.strip_prefix('-') {
            let item = item.split(" #").next().unwrap_or_default().trim();
            globs.push(item.trim_matches(|quote| quote == '"' || quote == '\'').to_owned());
        }
    }
    globs
}

fn expand(folder: &Path, parts: &[&str], found: &mut Vec<PathBuf>, depth: usize) {
    let Some((part, rest)) = parts.split_first() else {
        found.push(folder.to_owned());
        return;
    };
    if !part.contains('*') {
        expand(&folder.join(part), rest, found, depth);
        return;
    }
    if *part == "**" {
        expand(folder, rest, found, depth);
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name().is_some_and(|name| name != "node_modules" && !name.to_string_lossy().starts_with('.'))
        })
        .collect();
    folders.sort();
    for child in folders {
        if *part == "**" {
            expand(&child, parts, found, depth - 1);
        } else if matches_part(part, &folder_name(&child)) {
            expand(&child, rest, found, depth - 1);
        }
    }
}

fn matches_part(pattern: &str, name: &str) -> bool {
    let Some((start, end)) = pattern.split_once('*') else { return pattern == name };
    name.len() >= start.len() + end.len() && name.starts_with(start) && name.ends_with(end)
}

fn workspaces(root: &Path) -> Vec<PathBuf> {
    let globs = workspace_globs(root);
    let mut included = Vec::new();
    let mut left_out = Vec::new();
    for glob in &globs {
        let (glob, list) = match glob.strip_prefix('!') {
            Some(glob) => (glob, &mut left_out),
            None => (glob.as_str(), &mut included),
        };
        let glob = glob.trim_start_matches("./").trim_end_matches('/');
        let parts: Vec<&str> = glob.split('/').filter(|part| !part.is_empty()).collect();
        if parts.contains(&"..") {
            continue;
        }
        expand(root, &parts, list, GLOB_DEPTH);
    }
    included.retain(|folder| !left_out.contains(folder));
    included
}

fn folders_of_root(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| name != "node_modules" && !name.to_string_lossy().starts_with('.'))
        })
        .filter(|path| path.join("package.json").is_file())
        .collect();
    folders.sort();
    folders
}

pub fn package_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let project = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    let stays_inside = |file: &Path| file.canonicalize().is_ok_and(|real| real.starts_with(&project));
    let mut add = |file: PathBuf| {
        if stays_inside(&file) && file.is_file() && !files.contains(&file) && files.len() < MOST_PACKAGES {
            files.push(file);
        }
    };
    add(root.join("package.json"));
    for folder in workspaces(root).into_iter().chain(folders_of_root(root)) {
        add(folder.join("package.json"));
    }
    files
}

pub fn packages(root: &Path) -> Vec<Package> {
    package_files(root).iter().map(|path| read_package(root, path)).collect()
}

pub fn has_packages(root: &Path) -> bool {
    !package_files(root).is_empty()
}

fn problem(path: &Path, text: &str, error: &serde_json::Error) -> ModelProblem {
    let line =
        u32::try_from(error.line().max(1)).unwrap_or(1).min(u32::try_from(text.lines().count().max(1)).unwrap_or(1));
    ModelProblem { file: path.to_owned(), line, message: error.to_string(), ..ModelProblem::default() }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NpmModel;

impl ModelProvider for NpmModel {
    fn id(&self) -> &str {
        "npm"
    }

    fn watches(&self, file_name: &str) -> bool {
        WATCHED.contains(&file_name)
    }

    fn generation(&self, root: &Path) -> u64 {
        std::fs::metadata(root.join("node_modules"))
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs())
    }

    fn load(&self, root: &Path, _env: &ModelEnv) -> Option<ProjectModel> {
        let files = package_files(root);
        if files.is_empty() {
            return None;
        }
        let mut model = ProjectModel::default();
        for path in files {
            let folder = path.parent().unwrap_or(root).to_owned();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let manifest: Value = match serde_json::from_str(&text) {
                Ok(manifest) => manifest,
                Err(error) => {
                    model.problems.push(problem(&path, &text, &error));
                    continue;
                }
            };
            let name = manifest.get("name").and_then(Value::as_str).filter(|name| !name.is_empty());
            let found = dependencies(&manifest, &folder, root);
            for (dependency, installed) in &found {
                let library_root =
                    installed.clone().unwrap_or_else(|| folder.join("node_modules").join(&dependency.name));
                model.libraries.push(Library {
                    kind: LibraryKind::Dependency,
                    name: dependency.name.clone(),
                    version: Some(dependency.version.clone()).filter(|version| !version.is_empty()),
                    exists: installed.is_some(),
                    root: library_root,
                    group: Some(GROUP.to_owned()),
                    label: None,
                });
            }
            let excluded = folder.join("node_modules");
            model.modules.push(Module {
                name: name.map_or_else(|| folder_name(&folder), str::to_owned),
                root: folder,
                file: path,
                dependencies: found.into_iter().map(|(dependency, _)| dependency).collect(),
                details: serde_json::json!({
                    "version": manifest.get("version"),
                    "packageManager": manifest.get("packageManager"),
                }),
                folders: vec![ModuleFolder { path: excluded, kind: FolderKind::Excluded }],
                ..Module::default()
            });
        }
        model.libraries.sort_by(|a, b| (&a.name, &a.root).cmp(&(&b.name, &b.root)));
        model.libraries.dedup_by(|a, b| a.name == b.name && a.root == b.root);
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str, text: &str) {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    #[test]
    fn a_dependency_name_that_is_no_package_name_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work/shop");
        write(
            &root,
            "package.json",
            r#"{ "dependencies": { "../../../outside": "1", "/": "1", "a/../../b": "1", "a/b/c": "1", "@scope/ok": "1", "ok": "1" } }"#,
        );
        let names: Vec<String> =
            read_package(&root, &root.join("package.json")).dependencies.into_iter().map(|found| found.name).collect();
        assert_eq!(names, ["@scope/ok", "ok"]);
        let model = NpmModel.load(&root, &ModelEnv::default()).unwrap();
        let roots: Vec<&PathBuf> = model.libraries.iter().map(|library| &library.root).collect();
        assert_eq!(roots, [&root.join("node_modules/@scope/ok"), &root.join("node_modules/ok")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_project_is_no_package() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("shop");
        write(&root, "package.json", r#"{ "workspaces": ["linked", "stars/*", "**"] }"#);
        write(&root, "real/package.json", "{}");
        write(dir.path(), "outside/package.json", r#"{ "scripts": { "secret": "a" } }"#);
        std::os::unix::fs::symlink(dir.path().join("outside"), root.join("linked")).unwrap();
        std::fs::create_dir_all(root.join("stars")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"), root.join("stars/one")).unwrap();
        let found: Vec<String> = package_files(&root)
            .iter()
            .map(|file| file.strip_prefix(&root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, ["package.json", "real/package.json"]);
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "package.json",
            r#"{
  "name": "shop",
  "version": "1.2.0",
  "workspaces": ["packages/*", "!packages/skipped"],
  "scripts": { "dev": "vite", "build": "vite build" },
  "dependencies": { "vue": "^3.4.0", "lodash": "^4.0.0" },
  "devDependencies": { "vite": "^5.0.0" }
}
"#,
        );
        write(root, "node_modules/vue/package.json", r#"{ "version": "3.4.21" }"#);
        write(root, "node_modules/@scope/ui/package.json", r#"{ "version": "0.3.0" }"#);
        write(root, "packages/api/package.json", r#"{ "name": "api", "dependencies": { "@scope/ui": "*" } }"#);
        write(root, "packages/skipped/package.json", r#"{ "name": "skipped" }"#);
        write(root, "web/package.json", r#"{ "scripts": { "start": "serve" } }"#);
        write(root, "node_modules/inner/package.json", r#"{ "name": "not a package of the project" }"#);
        dir
    }

    #[test]
    fn the_packages_are_the_roots_its_workspaces_and_those_in_folders_of_the_root() {
        let dir = project();
        let names: Vec<String> = packages(dir.path()).into_iter().map(|package| package.relative).collect();
        assert_eq!(names, ["package.json", "packages/api/package.json", "web/package.json"]);
        let root = read_package(dir.path(), &dir.path().join("package.json"));
        assert_eq!(root.name, "shop");
        assert_eq!(root.version.as_deref(), Some("1.2.0"));
        let scripts: Vec<(&str, &str, u32)> =
            root.scripts.iter().map(|s| (s.name.as_str(), s.command.as_str(), s.line)).collect();
        assert_eq!(scripts, [("dev", "vite", 4), ("build", "vite build", 4)]);
        assert_eq!(read_package(dir.path(), &dir.path().join("web/package.json")).name, "web");
    }

    #[test]
    fn dependencies_are_libraries_with_the_installed_version_and_node_modules_is_excluded() {
        let dir = project();
        let model = NpmModel.load(dir.path(), &ModelEnv::default()).unwrap();
        let found: Vec<(&str, Option<&str>, bool)> =
            model.libraries.iter().map(|l| (l.name.as_str(), l.version.as_deref(), l.exists)).collect();
        assert_eq!(
            found,
            [
                ("@scope/ui", Some("0.3.0"), true),
                ("lodash", Some("^4.0.0"), false),
                ("vite", Some("^5.0.0"), false),
                ("vue", Some("3.4.21"), true)
            ]
        );
        assert!(model.libraries.iter().all(|library| library.group.as_deref() == Some(GROUP)));
        assert_eq!(model.libraries[0].root, dir.path().join("node_modules/@scope/ui"), "hoisted to the root");
        let shop = &model.modules[0];
        assert_eq!(shop.name, "shop");
        let scopes: Vec<(&str, Option<&str>)> =
            shop.dependencies.iter().map(|d| (d.name.as_str(), d.scope.as_deref())).collect();
        assert_eq!(scopes, [("lodash", None), ("vue", None), ("vite", Some("dev"))]);
        assert_eq!(shop.folders, [ModuleFolder { path: dir.path().join("node_modules"), kind: FolderKind::Excluded }]);
    }

    #[test]
    fn a_file_that_does_not_read_is_a_problem_on_its_line() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "package.json", "{\n  \"name\": \"shop\",\n  broken\n}\n");
        let model = NpmModel.load(dir.path(), &ModelEnv::default()).unwrap();
        assert!(model.modules.is_empty());
        assert_eq!(model.problems.len(), 1);
        assert_eq!(model.problems[0].line, 3);
        assert!(packages(dir.path())[0].error.is_some(), "the tool window says so too");
    }

    #[test]
    fn a_project_without_a_package_json_has_no_model() {
        let dir = tempfile::tempdir().unwrap();
        assert!(NpmModel.load(dir.path(), &ModelEnv::default()).is_none());
        assert!(!has_packages(dir.path()));
    }

    #[test]
    fn lock_files_and_the_node_modules_folder_refresh_the_model() {
        assert!(NpmModel.watches("package.json") && NpmModel.watches("pnpm-lock.yaml"));
        assert!(!NpmModel.watches("go.mod"));
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(NpmModel.generation(dir.path()), 0);
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        assert_ne!(NpmModel.generation(dir.path()), 0);
    }

    #[test]
    fn pnpm_workspaces_and_deep_globs_are_followed() {
        assert_eq!(
            pnpm_globs("packages:\n  - 'apps/*'\n  - \"libs/**\" # all\ncatalog:\n  - nothing\n"),
            ["apps/*", "libs/**"]
        );
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "package.json", "{}");
        write(dir.path(), "pnpm-workspace.yaml", "packages:\n  - 'libs/**'\n");
        write(dir.path(), "libs/a/b/package.json", "{}");
        write(dir.path(), "libs/a/node_modules/c/package.json", "{}");
        let names: Vec<String> = packages(dir.path()).into_iter().map(|package| package.relative).collect();
        assert_eq!(names, ["package.json", "libs/a/b/package.json"]);
    }

    #[test]
    fn a_dependency_name_never_points_outside_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("node_modules/@acme/ok")).unwrap();
        assert!(installed_folder(root, root, "@acme/ok").is_some());
        for name in ["../..", "/", "../../etc"] {
            assert_eq!(installed_folder(root, root, name), None, "{name}");
        }
    }

    #[test]
    fn a_workspace_glob_never_leaves_the_project() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "outside/package.json", "{}");
        write(dir.path(), "in/package.json", r#"{ "workspaces": { "packages": ["../outside"] } }"#);
        let names: Vec<String> = packages(&dir.path().join("in")).into_iter().map(|package| package.relative).collect();
        assert_eq!(names, ["package.json"]);
    }
}

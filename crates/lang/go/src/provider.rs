use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use zen_project::model::{
    Dependency, ExcludedVersion, FolderKind, Library, LibraryKind, ModelEnv, ModelProblem, ModelProvider, Module,
    ModuleFolder, ProjectModel, Replacement, WorkspaceFile, WorkspaceReplacement,
};

use crate::gomod::{GoMod, Replace, is_directory_path, parse_go_mod, parse_go_work};
use crate::modcache::module_dir;

#[derive(Clone, Copy, Debug, Default)]
pub struct GoModules;

impl ModelProvider for GoModules {
    fn id(&self) -> &str {
        "go"
    }

    fn watches(&self, file_name: &str) -> bool {
        matches!(file_name.rsplit(['/', '\\']).next(), Some("go.mod" | "go.work"))
    }

    fn load(&self, root: &Path, env: &ModelEnv) -> Option<ProjectModel> {
        load(root, env)
    }
}

pub fn compare_versions(a: &str, b: &str) -> Ordering {
    match (parse_version(a), parse_version(b)) {
        (Some(a), Some(b)) => a.0.cmp(&b.0).then_with(|| compare_prerelease(&a.1, &b.1)),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

pub(crate) fn is_release(version: &str) -> bool {
    parse_version(version).is_some_and(|(_, prerelease)| prerelease.is_empty())
}

fn parse_version(version: &str) -> Option<([u64; 3], Vec<&str>)> {
    let rest = version.strip_prefix('v')?;
    let rest = rest.split_once('+').map_or(rest, |(core, _)| core);
    let (core, prerelease) = rest.split_once('-').map_or((rest, ""), |(core, prerelease)| (core, prerelease));
    let mut numbers = [0; 3];
    for (index, part) in core.split('.').enumerate() {
        if index == 3 || part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        numbers[index] = part.parse().ok()?;
    }
    let prerelease: Vec<&str> = if prerelease.is_empty() { Vec::new() } else { prerelease.split('.').collect() };
    if prerelease.iter().any(|part| part.is_empty()) {
        return None;
    }
    Some((numbers, prerelease))
}

fn compare_prerelease(a: &[&str], b: &[&str]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for (a, b) in a.iter().zip(b) {
        let numeric = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        let order = match (numeric(a), numeric(b)) {
            (true, true) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => a.cmp(b),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

struct Loaded {
    dir: PathBuf,
    file: PathBuf,
    go_mod: GoMod,
}

fn load(root: &Path, env: &ModelEnv) -> Option<ProjectModel> {
    let mut model = ProjectModel::default();
    let work_file = root.join("go.work");
    let mut work_replaces: Vec<Replace> = Vec::new();
    let mut dirs: Vec<(PathBuf, Option<u32>)> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&work_file) {
        let work = parse_go_work(&work_file, &text);
        model.problems.extend(work.problems.iter().cloned());
        for directive in &work.uses {
            let dir = resolve(root, &directive.path);
            if !dirs.iter().any(|(known, _)| *known == dir) {
                dirs.push((dir, Some(directive.line)));
            }
        }
        model.workspace = Some(WorkspaceFile {
            file: work_file.clone(),
            language_version: work.go.clone(),
            toolchain: work.toolchain.clone(),
            uses: dirs.iter().map(|(dir, _)| dir.clone()).collect(),
            replacements: work
                .replaces
                .iter()
                .map(|replace| WorkspaceReplacement {
                    name: replace.path.clone(),
                    version: replace.version.clone(),
                    replacement: replacement(replace, root),
                })
                .collect(),
            details: serde_json::json!({ "godebug": work.godebugs }),
        });
        work_replaces = work.replaces;
    } else if root.join("go.mod").is_file() {
        dirs.push((root.to_owned(), None));
    } else {
        return None;
    }

    let mut loaded: Vec<Loaded> = Vec::new();
    for (dir, line) in dirs {
        let file = dir.join("go.mod");
        match std::fs::read_to_string(&file) {
            Ok(text) => loaded.push(Loaded { go_mod: parse_go_mod(&file, &text), dir, file }),
            Err(error) => model.problems.push(match line {
                Some(line) => ModelProblem {
                    file: work_file.clone(),
                    line,
                    message: format!("no go.mod file in {}", dir.display()),
                    ..ModelProblem::default()
                },
                None => ModelProblem {
                    file,
                    line: 1,
                    message: format!("cannot read go.mod: {error}"),
                    ..ModelProblem::default()
                },
            }),
        }
    }

    let workspace_modules: Vec<&str> = loaded
        .iter()
        .filter_map(|module| module.go_mod.module.as_ref().map(|directive| directive.path.as_str()))
        .collect();
    let mut selected: BTreeMap<&str, &str> = BTreeMap::new();
    for module in &loaded {
        model.problems.extend(module.go_mod.problems.iter().cloned());
        for require in &module.go_mod.requires {
            if workspace_modules.contains(&require.path.as_str()) {
                continue;
            }
            let entry = selected.entry(&require.path).or_insert(&require.version);
            if compare_versions(&require.version, entry) == Ordering::Greater {
                *entry = &require.version;
            }
        }
    }

    if let Some(sdk_root) = &env.sdk_root {
        let src = sdk_root.join("src");
        model.libraries.push(Library {
            kind: LibraryKind::Sdk,
            name: "Go".to_owned(),
            version: env.sdk_version.as_ref().map(|version| version.strip_prefix("go").unwrap_or(version).to_owned()),
            exists: src.is_dir(),
            root: src,
            group: None,
            label: None,
        });
    }
    for (&path, &version) in &selected {
        let found = find_replacement(path, version, &work_replaces, root).or_else(|| {
            loaded.iter().find_map(|module| find_replacement(path, version, &module.go_mod.replaces, &module.dir))
        });
        let (version, root) = match found {
            Some(Replacement { local_path: Some(local), .. }) => (None, local),
            Some(Replacement { name, version: Some(new_version), .. }) => {
                let root = env.package_cache.as_deref().map(|cache| module_dir(cache, &name, &new_version));
                (Some(new_version), root.unwrap_or_default())
            }
            _ => {
                let root = env.package_cache.as_deref().map(|cache| module_dir(cache, path, version));
                (Some(version.to_owned()), root.unwrap_or_default())
            }
        };
        let exists = !root.as_os_str().is_empty() && root.is_dir();
        model.libraries.push(Library {
            kind: LibraryKind::Dependency,
            name: path.to_owned(),
            version,
            root,
            exists,
            label: None,
            group: None,
        });
    }

    for module in loaded {
        let go_mod = &module.go_mod;
        let dependencies = go_mod
            .requires
            .iter()
            .map(|require| Dependency {
                name: require.path.clone(),
                version: require.version.clone(),
                indirect: require.indirect,
                replacement: find_replacement(&require.path, &require.version, &work_replaces, root)
                    .or_else(|| find_replacement(&require.path, &require.version, &go_mod.replaces, &module.dir)),
                scope: None,
            })
            .collect();
        let excluded = go_mod
            .excludes
            .iter()
            .map(|exclude| ExcludedVersion { name: exclude.path.clone(), version: exclude.version.clone() })
            .collect();
        let details = serde_json::json!({
            "deprecated": go_mod.module.as_ref().and_then(|directive| directive.deprecated.clone()),
            "retract": go_mod.retracts,
            "godebug": go_mod.godebugs,
            "tool": go_mod.tools,
            "ignore": go_mod.ignores,
            "replace": go_mod.replaces,
        });
        let folders = vec![ModuleFolder { path: module.dir.clone(), kind: FolderKind::Sources }];
        model.modules.push(Module {
            name: go_mod.module.as_ref().map(|directive| directive.path.clone()).unwrap_or_default(),
            folders,
            output: None,
            test_output: None,
            assumed: false,
            root: module.dir,
            file: module.file,
            language_version: go_mod.go.clone(),
            toolchain: go_mod.toolchain.clone(),
            dependencies,
            excluded,
            details,
        });
    }
    Some(model)
}

fn find_replacement(path: &str, version: &str, replaces: &[Replace], base: &Path) -> Option<Replacement> {
    replaces
        .iter()
        .filter(|replace| replace.matches(path, version))
        .max_by_key(|replace| replace.version.is_some())
        .map(|replace| replacement(replace, base))
}

fn replacement(replace: &Replace, base: &Path) -> Replacement {
    let local = replace.is_local() && is_directory_path(&replace.new_path);
    Replacement {
        name: replace.new_path.clone(),
        version: replace.new_version.clone(),
        local_path: local.then(|| resolve(base, &replace.new_path)),
    }
}

pub(crate) fn resolve(base: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() { normalize(path) } else { normalize(&base.join(path)) }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_precedence() {
        let ordered = [
            "v0.0.0-20230101000000-abcdefabcdef",
            "v0.1.0",
            "v1.0.0-alpha",
            "v1.0.0-alpha.1",
            "v1.0.0-beta.2",
            "v1.0.0-beta.11",
            "v1.0.0-rc.1",
            "v1.0.0",
            "v1.2.4-0.20240101000000-abcdefabcdef",
            "v1.2.4",
            "v1.10.0",
            "v2.0.0+incompatible",
            "v10.0.0",
        ];
        for pair in ordered.windows(2) {
            assert_eq!(compare_versions(pair[0], pair[1]), Ordering::Less, "{pair:?}");
            assert_eq!(compare_versions(pair[1], pair[0]), Ordering::Greater, "{pair:?}");
        }
        assert_eq!(compare_versions("v2.0.0+incompatible", "v2.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("master", "v0.0.1"), Ordering::Less);
        assert_eq!(compare_versions("v1", "v1.0.0"), Ordering::Equal);
    }

    #[test]
    fn paths_are_normalized() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), Path::new("/a/c"));
        assert_eq!(resolve(Path::new("/w/m"), "../lib"), Path::new("/w/lib"));
        assert_eq!(resolve(Path::new("/w"), "/abs/x"), Path::new("/abs/x"));
    }
}

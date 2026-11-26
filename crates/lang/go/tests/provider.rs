use std::path::Path;

use zen_lang_go::GoModules;
use zen_project::model::{Library, LibraryKind, ModelEnv, ModelProvider};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn library<'a>(libraries: &'a [Library], name: &str) -> &'a Library {
    libraries.iter().find(|library| library.name == name).unwrap_or_else(|| panic!("no {name} in {libraries:?}"))
}

#[test]
fn a_workspace_with_two_modules() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let cache = temp.path().join("cache");
    let sdk = temp.path().join("sdk");
    std::fs::create_dir_all(sdk.join("src")).unwrap();
    write(&root.join("go.work"), include_str!("fixtures/work/go.work"));
    write(
        &root.join("app/go.mod"),
        "module example.com/app\n\ngo 1.22\n\nrequire (\n\texample.com/lib v0.0.0\n\texample.com/BurntSushi/toml v1.2.0\n\texample.com/shared v1.0.0\n\texample.com/old v1.0.0 // indirect\n)\n\nreplace example.com/old => example.com/new v2.1.0\n\nexclude example.com/BurntSushi/toml v1.1.0\n",
    );
    write(
        &root.join("lib/go.mod"),
        "module example.com/lib\n\ngo 1.23\n\nrequire example.com/BurntSushi/toml v1.10.0\nrequire example.com/pseudo v0.0.0-20240101000000-abcdefabcdef\nrequire example.com/local v1.0.0\nreplace example.com/local => ../vendor/local\nretract v0.0.1\nbroken line\n",
    );
    std::fs::create_dir_all(cache.join("example.com/!burnt!sushi/toml@v1.10.0")).unwrap();
    std::fs::create_dir_all(root.join("forks/shared")).unwrap();
    let env = ModelEnv {
        sdk_root: Some(sdk.clone()),
        sdk_version: Some("go1.23.4".to_owned()),
        package_cache: Some(cache.clone()),
    };

    let model = GoModules.load(&root, &env).unwrap();

    let workspace = model.workspace.as_ref().unwrap();
    assert_eq!(workspace.file, root.join("go.work"));
    assert_eq!(workspace.language_version.as_deref(), Some("1.23"));
    assert_eq!(workspace.uses, [root.join("app"), root.join("lib"), root.join("missing")]);
    assert_eq!(workspace.replacements[0].replacement.local_path.as_deref(), Some(root.join("forks/shared").as_path()));
    assert_eq!(workspace.details["godebug"][0]["key"], "default");

    let names: Vec<&str> = model.modules.iter().map(|module| module.name.as_str()).collect();
    assert_eq!(names, ["example.com/app", "example.com/lib"]);
    let app = &model.modules[0];
    assert_eq!(app.root, root.join("app"));
    assert_eq!(app.file, root.join("app/go.mod"));
    assert_eq!(app.language_version.as_deref(), Some("1.22"));
    let folders: Vec<_> = app.folders.iter().map(|folder| (folder.path.clone(), folder.kind)).collect();
    assert_eq!(
        folders,
        [(root.join("app"), zen_project::model::FolderKind::Sources)],
        "a Go module's folder is its sources"
    );
    assert_eq!(app.excluded[0].name, "example.com/BurntSushi/toml");
    assert_eq!(app.excluded[0].version, "v1.1.0");
    let old = app.dependencies.iter().find(|dependency| dependency.name == "example.com/old").unwrap();
    assert!(old.indirect);
    assert_eq!(old.replacement.as_ref().unwrap().name, "example.com/new");
    let shared = app.dependencies.iter().find(|dependency| dependency.name == "example.com/shared").unwrap();
    assert_eq!(shared.replacement.as_ref().unwrap().local_path.as_deref(), Some(root.join("forks/shared").as_path()));
    let lib = &model.modules[1];
    assert_eq!(lib.details["retract"][0]["low"], "v0.0.1");
    let local = lib.dependencies.iter().find(|dependency| dependency.name == "example.com/local").unwrap();
    assert_eq!(local.replacement.as_ref().unwrap().local_path.as_deref(), Some(root.join("vendor/local").as_path()));

    assert_eq!(model.libraries[0].kind, LibraryKind::Sdk);
    assert_eq!(model.libraries[0].name, "Go");
    assert_eq!(model.libraries[0].version.as_deref(), Some("1.23.4"));
    assert_eq!(model.libraries[0].root, sdk.join("src"));
    assert!(model.libraries[0].exists);
    assert!(
        model.libraries.iter().all(|library| library.name != "example.com/lib"),
        "workspace modules are not libraries"
    );
    let toml = library(&model.libraries, "example.com/BurntSushi/toml");
    assert_eq!(toml.kind, LibraryKind::Dependency);
    assert_eq!(toml.version.as_deref(), Some("v1.10.0"), "the highest version wins");
    assert_eq!(toml.root, cache.join("example.com/!burnt!sushi/toml@v1.10.0"));
    assert!(toml.exists);
    let shared = library(&model.libraries, "example.com/shared");
    assert_eq!(shared.version, None);
    assert_eq!(shared.root, root.join("forks/shared"));
    assert!(shared.exists);
    let old = library(&model.libraries, "example.com/old");
    assert_eq!(old.version.as_deref(), Some("v2.1.0"));
    assert_eq!(old.root, cache.join("example.com/new@v2.1.0"));
    assert!(!old.exists);
    let local = library(&model.libraries, "example.com/local");
    assert_eq!(local.root, root.join("vendor/local"));
    assert!(!local.exists);
    let pseudo = library(&model.libraries, "example.com/pseudo");
    assert_eq!(pseudo.version.as_deref(), Some("v0.0.0-20240101000000-abcdefabcdef"));
    assert!(!pseudo.exists);
    assert_eq!(model.libraries.len(), 6);

    let problems: Vec<(String, u32)> = model
        .problems
        .iter()
        .map(|problem| (problem.file.file_name().unwrap().to_string_lossy().into_owned(), problem.line))
        .collect();
    assert_eq!(problems, [("go.work".to_owned(), 9), ("go.mod".to_owned(), 10)]);
    assert_eq!(model.problems[1].file, root.join("lib/go.mod"));
}

#[test]
fn a_single_module_without_a_workspace() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join("go.mod"),
        "module example.com/one\nrequire example.com/dep v1.0.0\nrequire example.com/dep v1.0.1-pre\n",
    );
    let model = GoModules.load(temp.path(), &ModelEnv::default()).unwrap();
    assert!(model.workspace.is_none());
    assert_eq!(model.modules.len(), 1);
    assert_eq!(model.modules[0].root, temp.path());
    assert_eq!(model.libraries.len(), 1, "no SDK without an SDK root");
    let dep = &model.libraries[0];
    assert_eq!(dep.version.as_deref(), Some("v1.0.1-pre"));
    assert_eq!(dep.root, Path::new(""));
    assert!(!dep.exists);
}

#[test]
fn a_folder_without_module_files_has_no_model() {
    let temp = tempfile::tempdir().unwrap();
    assert!(GoModules.load(temp.path(), &ModelEnv::default()).is_none());
}

#[test]
fn the_provider_watches_module_files() {
    assert_eq!(GoModules.id(), "go");
    for name in ["go.mod", "go.work", "a/b/go.mod", "a\\go.work"] {
        assert!(GoModules.watches(name), "{name}");
    }
    for name in ["go.sum", "go.work.sum", "main.go", "xgo.mod"] {
        assert!(!GoModules.watches(name), "{name}");
    }
}

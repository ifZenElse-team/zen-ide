use std::path::{Path, PathBuf};
use std::time::Duration;

use zen_lang_rust::CargoModels;
use zen_lang_rust::toolchain::Toolchain;
use zen_project::model::{LibraryKind, ModelEnv, ModelProvider};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn workspace(root: &Path) {
    write(&root.join("Cargo.toml"), "[workspace]\nmembers = [\"app\", \"core\"]\nresolver = \"3\"\n");
    write(
        &root.join("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.85\"\n\n[dependencies]\ncore-lib = { path = \"../core\" }\nhelper = { path = \"../../outside/helper\" }\n",
    );
    write(&root.join("app/src/main.rs"), "fn main() {\n    core_lib::hello();\n}\n");
    write(&root.join("core/Cargo.toml"), "[package]\nname = \"core-lib\"\nversion = \"0.2.0\"\nedition = \"2024\"\n");
    write(&root.join("core/src/lib.rs"), "pub fn hello() {}\n");
    let outside = root.parent().unwrap().join("outside/helper");
    write(&outside.join("Cargo.toml"), "[package]\nname = \"helper\"\nversion = \"1.2.3\"\nedition = \"2024\"\n");
    write(&outside.join("src/lib.rs"), "");
    write(&root.join("rust-toolchain.toml"), "[toolchain]\nchannel = \"stable\"\n");
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap().join("ws");
    workspace(&root);
    (temp, root)
}

#[test]
fn a_workspace_has_its_members_as_modules_and_its_other_crates_as_libraries() {
    zen_i18n::register(zen_lang_rust::LOCALES);
    let (_temp, root) = project();
    let models = CargoModels::new(Toolchain::load(None));
    let model = models.load(&root, &ModelEnv::default()).expect("a Cargo project");
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let modules: Vec<(&str, PathBuf)> =
        model.modules.iter().map(|module| (module.name.as_str(), module.root.clone())).collect();
    assert_eq!(modules, [("app", root.join("app")), ("core-lib", root.join("core"))]);
    let app = &model.modules[0];
    assert_eq!(app.file, root.join("app/Cargo.toml"));
    assert_eq!(app.language_version.as_deref(), Some("1.85"));
    assert_eq!(app.details["targets"][0]["name"], "app");
    assert_eq!(app.details["targets"][0]["kind"][0], "bin");
    let workspace = model.workspace.as_ref().expect("a workspace");
    assert_eq!(workspace.file, root.join("Cargo.toml"));
    assert_eq!(workspace.uses, [root.join("app"), root.join("core")]);
    assert_eq!(workspace.toolchain.as_deref(), Some("stable"));

    let sdk = model.libraries.iter().find(|library| library.kind == LibraryKind::Sdk).expect("the standard library");
    assert_eq!(sdk.name, "Rust");
    assert!(sdk.version.is_some());
    let helper = model.libraries.iter().find(|library| library.name == "helper").expect("the outside crate");
    assert_eq!(helper.version.as_deref(), Some("1.2.3"));
    assert_eq!(helper.root, root.parent().unwrap().join("outside/helper"));
    assert!(helper.exists);
    assert!(!model.libraries.iter().any(|library| library.name == "core-lib"), "members are not libraries");
}

#[test]
fn the_model_follows_its_manifests() {
    zen_i18n::register(zen_lang_rust::LOCALES);
    let (_temp, root) = project();
    let models = CargoModels::new(Toolchain::load(None));
    let first = models.load(&root, &ModelEnv::default()).unwrap();
    assert_eq!(first.modules.len(), 2);
    assert_eq!(models.load(&root, &ModelEnv::default()).unwrap(), first, "kept while nothing changed");

    std::thread::sleep(Duration::from_millis(20));
    write(&root.join("Cargo.toml"), "[workspace]\nmembers = [\"app\", \"core\", \"tool\"]\nresolver = \"3\"\n");
    write(&root.join("tool/Cargo.toml"), "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    write(&root.join("tool/src/main.rs"), "fn main() {}\n");
    let names: Vec<String> =
        models.load(&root, &ModelEnv::default()).unwrap().modules.into_iter().map(|module| module.name).collect();
    assert_eq!(names, ["app", "core-lib", "tool"]);

    std::thread::sleep(Duration::from_millis(20));
    write(&root.join("tool/Cargo.toml"), "[package]\nname = \"tool\"\nversion = \n");
    let broken = models.load(&root, &ModelEnv::default()).unwrap();
    assert_eq!(broken.problems.len(), 1, "{:?}", broken.problems);
    assert_eq!(broken.problems[0].file, root.join("tool/Cargo.toml"));
    assert_eq!(broken.problems[0].line, 3);
    assert!(!broken.problems[0].warning, "the members cannot be read");

    std::thread::sleep(Duration::from_millis(20));
    write(&root.join("tool/Cargo.toml"), "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    assert!(models.load(&root, &ModelEnv::default()).unwrap().problems.is_empty());
    std::fs::remove_file(root.join("tool/Cargo.toml")).unwrap();
    let deleted = models.load(&root, &ModelEnv::default()).unwrap();
    let places: Vec<(PathBuf, u32)> =
        deleted.problems.iter().map(|problem| (problem.file.clone(), problem.line)).collect();
    assert_eq!(places, [(root.join("Cargo.toml"), 2)], "{:?}", deleted.problems);
    write(&root.join("tool/Cargo.toml"), "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    assert!(models.load(&root, &ModelEnv::default()).unwrap().problems.is_empty(), "read again once it is back");
}

#[test]
fn a_dependency_that_cannot_be_resolved_offline_is_a_warning() {
    zen_i18n::register(zen_lang_rust::LOCALES);
    let (_temp, root) = project();
    write(
        &root.join("core/Cargo.toml"),
        "[package]\nname = \"core-lib\"\nversion = \"0.2.0\"\nedition = \"2024\"\n\n[dependencies]\nzen-no-such-crate-anywhere = \"999\"\n",
    );
    let model = CargoModels::new(Toolchain::load(None)).load(&root, &ModelEnv::default()).unwrap();
    assert_eq!(model.modules.len(), 2, "the members are still read");
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    let problem = &model.problems[0];
    assert!(problem.warning);
    assert_eq!((problem.file.clone(), problem.line), (root.join("core/Cargo.toml"), 7), "on the dependency's line");
    assert!(problem.message.contains("zen-no-such-crate-anywhere"), "{}", problem.message);
    assert!(problem.message.contains("cargo fetch") && !problem.message.contains("--offline"), "{}", problem.message);
}

#[test]
fn a_dependency_table_is_found_where_it_is_declared() {
    zen_i18n::register(zen_lang_rust::LOCALES);
    let (_temp, root) = project();
    write(
        &root.join("core/Cargo.toml"),
        "[package]\nname = \"core-lib\"\nversion = \"0.2.0\"\nedition = \"2024\"\n\n[dev-dependencies.zen-no-such-crate-anywhere]\nversion = \"999\"\n",
    );
    let model = CargoModels::new(Toolchain::load(None)).load(&root, &ModelEnv::default()).unwrap();
    let places: Vec<(PathBuf, u32)> =
        model.problems.iter().map(|problem| (problem.file.clone(), problem.line)).collect();
    assert_eq!(places, [(root.join("core/Cargo.toml"), 6)]);
}

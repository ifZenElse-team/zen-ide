use std::path::{Path, PathBuf};

use zen_modules::{FileTemplateSet, TemplateVariable};

use crate::test_support::{GoTests, package_clause};
use zen_run::test_support::TestSupport;

const TEMPLATES: &str = include_str!("../file-templates.json");

const VARIABLES: &[TemplateVariable] = &[
    TemplateVariable { name: "GO_PACKAGE_NAME", description: "go-file-templates-var-package" },
    TemplateVariable { name: "GO_PACKAGE_NAME_WITH_TEST_SUFFIX", description: "go-file-templates-var-test-package" },
];

pub const FILE_TEMPLATES: FileTemplateSet =
    FileTemplateSet { origin: "go", templates: TEMPLATES, variables: VARIABLES, values: Some(values) };

fn go_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "go"))
        .collect();
    files.sort_by_key(|path| (GoTests.is_test_file(path), path.clone()));
    files
}

fn folder_package(folder: &Path) -> String {
    let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let name: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    match name.chars().next() {
        None => "main".to_owned(),
        Some(first) if first.is_numeric() => format!("_{name}"),
        Some(_) => name,
    }
}

fn package_of(folder: &Path) -> String {
    go_files(folder)
        .iter()
        .find_map(|file| package_clause(&std::fs::read_to_string(file).ok()?))
        .map(|package| package.strip_suffix("_test").map(str::to_owned).unwrap_or(package))
        .unwrap_or_else(|| folder_package(folder))
}

fn values(folder: &Path) -> Vec<(String, String)> {
    let package = package_of(folder);
    let with_suffix = format!("{package}_test");
    vec![("GO_PACKAGE_NAME".to_owned(), package), ("GO_PACKAGE_NAME_WITH_TEST_SUFFIX".to_owned(), with_suffix)]
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn value(folder: &Path, name: &str) -> String {
        values(folder).into_iter().find(|(known, _)| known == name).unwrap().1
    }

    #[test]
    fn the_package_comes_from_the_sibling_files_before_their_tests() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a_test.go"), "package shop_test\n").unwrap();
        assert_eq!(value(dir.path(), "GO_PACKAGE_NAME"), "shop", "an external test's package names the package");
        std::fs::write(dir.path().join("cart.go"), "// Package store.\npackage store\n").unwrap();
        assert_eq!(value(dir.path(), "GO_PACKAGE_NAME"), "store");
        assert_eq!(value(dir.path(), "GO_PACKAGE_NAME_WITH_TEST_SUFFIX"), "store_test");
    }

    #[test]
    fn a_folder_without_go_files_names_the_package_as_go_allows() {
        let dir = tempfile::tempdir().unwrap();
        for (folder, package) in [("orders", "orders"), ("my-app.v2", "my_app_v2"), ("2fa", "_2fa")] {
            std::fs::create_dir(dir.path().join(folder)).unwrap();
            assert_eq!(value(&dir.path().join(folder), "GO_PACKAGE_NAME"), package);
        }
    }

    #[test]
    fn the_templates_are_the_go_files_kinds_and_only_for_go_folders() {
        let templates: Vec<Value> = serde_json::from_str(TEMPLATES).unwrap();
        let names: Vec<&str> = templates.iter().map(|template| template["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Go File", "Simple Application", "Go Test File"]);
        assert_eq!(templates[1]["kindOf"], "Go File");
        assert!(templates.iter().all(|template| template["markers"][0] == "go.mod"));
    }
}

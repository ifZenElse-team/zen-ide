use std::path::{Path, PathBuf};

use zen_jb_import::IdeaProject;
use zen_project::settings::{ReadFiles, SettingsProvider};

struct Disk(PathBuf);

impl ReadFiles for Disk {
    fn read(&self, relative: &Path) -> Option<String> {
        std::fs::read_to_string(self.0.join(relative)).ok()
    }

    fn list(&self, relative_dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.0.join(relative_dir)) else { return Vec::new() };
        let mut listed: Vec<PathBuf> =
            entries.filter_map(Result::ok).map(|entry| relative_dir.join(entry.file_name())).collect();
        listed.sort();
        listed
    }
}

fn pairs(env: &[(String, String)]) -> Vec<(&str, &str)> {
    env.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect()
}

#[test]
fn the_shared_run_configurations_are_read_and_the_personal_ones_are_not() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/idea-project");
    let settings = IdeaProject::with_user_home(None).load(&root, &Disk(root.clone())).unwrap();
    assert!(settings.problems.is_empty(), "{:?}", settings.problems);
    let names: Vec<&str> =
        settings.run_configurations.iter().map(|configuration| configuration.name.as_str()).collect();
    assert_eq!(names, ["Serve", "Unit tests"], "the Scratch configuration of workspace.xml is not read");
    let project = root.to_string_lossy().trim_end_matches('/').to_owned();

    let serve = &settings.run_configurations[0];
    assert_eq!(serve.source_file, root.join(".idea/runConfigurations/Serve.xml"));
    assert_eq!(serve.type_id, "GoApplicationRunConfiguration");
    assert_eq!(serve.factory.as_deref(), Some("Go Application"));
    assert!(serve.shared);
    assert_eq!(serve.options["module.name"], "orchard");
    assert_eq!(serve.options["working_directory"], project);
    assert_eq!(serve.options["parameters"], "--listen :7311");
    assert_eq!(serve.options["kind"], "PACKAGE");
    assert_eq!(serve.options["package"], "example.org/orchard");
    assert_eq!(serve.options["filePath"], format!("{project}/main.go"));
    assert_eq!(serve.options["method.v"], "2");
    assert!(!serve.options.contains_key("envs"));
    assert_eq!(pairs(&serve.env), [("ORCHARD_MODE", "dev"), ("ORCHARD_SEED", "17")]);

    let tests = &settings.run_configurations[1];
    assert_eq!(tests.source_file, root.join(".run/Unit_tests.run.xml"));
    assert_eq!(tests.type_id, "GoTestRunConfiguration");
    assert_eq!(tests.options["kind"], "DIRECTORY");
    assert_eq!(tests.options["directory"], project);
    assert_eq!(tests.options["framework"], "gotest");
    assert_eq!(pairs(&tests.env), [("ORCHARD_MODE", "test")]);
}

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_lang_go::new_project::{FoundSdks, GoProject};
use zen_workbench::build_tools::BuildEnv;
use zen_workbench::new_project::NewProjects;

fn sdk(dir: &Path) -> zen_lang_go::toolchain::go::GoSdk {
    let root = dir.join("go");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("VERSION"), "go1.27.2\n").unwrap();
    let go = root.join("bin/go");
    std::fs::write(&go, "#!/bin/sh\necho \"module $3\" > go.mod\n").unwrap();
    std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
    zen_lang_go::toolchain::go::sdk_at(&root).unwrap()
}

fn setup(found: FoundSdks) -> (Registry, Session) {
    zen_i18n::register(zen_lang_go::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(BuildEnv::default());
    let projects = NewProjects::default();
    let found = Arc::new(found);
    projects.declare(Arc::new(GoProject::new(Arc::new(move || FoundSdks {
        sdks: found.sdks.clone(),
        chosen: found.chosen.clone(),
    }))));
    session.set_state(projects);
    (registry, session)
}

fn create(registry: &Registry, session: &mut Session, location: &Path) -> Result<Value, String> {
    let params = json!({ "generator": "go", "location": location, "waitMs": 10000 });
    registry.dispatch(session, "project.new.create", params).map_err(|error| error.to_string())
}

#[test]
fn a_person_s_go_module_opens_trusted_and_an_agent_s_does_not() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let go = sdk(&root);
    let (registry, mut session) = setup(FoundSdks { sdks: vec![go.clone()], chosen: Some(go) });

    let person = root.join("shop");
    let created = create(&registry, &mut session, &person).unwrap();
    assert_eq!(created["state"], "ready", "{created}");
    assert_eq!(created["trusted"], true, "the person pressed Create");
    assert_eq!(std::fs::read_to_string(person.join("go.mod")).unwrap(), "module shop\n");
    assert!(person.join("main.go").is_file());

    let agent = root.join("tools");
    let created = zen_core::macros::as_agent(&mut session, |session| create(&registry, session, &agent)).unwrap();
    assert_eq!(created["state"], "ready", "{created}");
    assert_eq!(created["trusted"], false, "agents never grant trust");
    assert!(!zen_workbench::trust::trusted(&session, &agent));
}

#[test]
fn without_a_go_sdk_create_is_refused_with_the_reason() {
    let temp = tempfile::tempdir().unwrap();
    let (registry, mut session) = setup(FoundSdks { sdks: Vec::new(), chosen: None });
    let location = std::fs::canonicalize(temp.path()).unwrap().join("shop");
    let refused = create(&registry, &mut session, &location).unwrap();
    assert_eq!(refused["state"], "failed", "{refused}");
    assert!(refused["error"].as_str().is_some_and(|error| error.contains("SDK")), "{refused}");
    assert!(!location.join("go.mod").exists());
    let form =
        registry.dispatch(&mut session, "project.new.form", json!({ "generator": "go", "waitMs": 10000 })).unwrap();
    assert_eq!(form["form"]["fields"][0]["id"], "goroot", "{form}");
}

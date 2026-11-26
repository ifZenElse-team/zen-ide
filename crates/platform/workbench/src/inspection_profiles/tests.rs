use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_project::inspect::{
    DeclaredInspection, Finding, FindingSeverity, FixJob, InspectContext, InspectionFix, Inspector,
};

use super::*;
use crate::inspections::Inspections;

struct Marks;

impl Inspector for Marks {
    fn id(&self) -> &str {
        "marks"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".txt")
    }

    fn inspections(&self) -> Vec<DeclaredInspection> {
        let declared = |id: &str, title: &str, cleanup: bool, off: bool| DeclaredInspection {
            id: id.to_owned(),
            title: title.to_owned(),
            group: "Marks".to_owned(),
            description: format!("{title} here"),
            cleanup,
            off,
        };
        vec![
            declared("todo", "To do", false, false),
            declared("tidy", "Tidy", true, false),
            declared("loud", "Loud", false, true),
        ]
    }

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
        let found = context.text.match_indices("TODO").map(|(at, word)| Finding {
            start: at,
            end: at + word.len(),
            severity: FindingSeverity::Warning,
            message: "To do".to_owned(),
            note: None,
            rows: Vec::new(),
            details: None,
            fixes: Vec::new(),
            source: "marks".to_owned(),
            code: "todo".to_owned(),
            deprecated: false,
            unused: false,
        });
        found.collect()
    }

    fn apply(&self, _: &InspectContext<'_>, _: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        Err("no fixes".to_owned())
    }
}

struct Project {
    dir: tempfile::TempDir,
    ide: tempfile::TempDir,
    registry: Registry,
    session: Session,
}

fn project() -> Project {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let ide = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "TODO\n").unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.state_mut::<Inspections>().set_inspectors(vec![std::sync::Arc::new(Marks)], None);
    session.state_mut::<InspectionProfiles>().set_ide_folder(ide.path().join("inspectionProfiles"));
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    Project { dir, ide, registry, session }
}

impl Project {
    fn dispatch(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn refused(&mut self, command: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err().to_string()
    }

    fn inspection(&mut self, id: &str) -> Value {
        let view = self.dispatch("inspect.profile.get", json!({}));
        view["inspections"].as_array().unwrap().iter().find(|row| row["id"] == id).unwrap().clone()
    }

    fn file_problems(&mut self) -> Vec<Value> {
        let path = self.dir.path().join("a.txt");
        self.dispatch("file.document.open", json!({ "path": path }));
        let answer = self.dispatch("problems.file.list", json!({ "path": path }));
        crate::language::listed_problems(&answer)
    }
}

fn ide(name: &str) -> Value {
    json!({ "name": name, "level": "ide" })
}

#[test]
fn project_default_is_there_with_nothing_on_disk_and_cannot_be_removed_or_renamed() {
    let mut project = project();
    let list = project.dispatch("inspect.profile.list", json!({}));
    assert_eq!(list["active"], json!({ "name": "Project Default", "level": "project" }));
    assert_eq!(list["profiles"], json!([{ "name": "Project Default", "level": "project" }]));
    assert!(!project.dir.path().join(PROJECT_FOLDER).exists());
    project.refused("inspect.profile.delete", json!({}));
    project.refused("inspect.profile.rename", json!({ "name": "Other" }));
}

#[test]
fn changes_go_to_the_project_folder_and_a_cleared_override_stops_being_modified() {
    let mut project = project();
    assert_eq!(project.inspection("todo")["modified"], false);
    let set = json!({ "inspections": { "todo": { "enabled": false, "severity": "error" } } });
    project.dispatch("inspect.profile.set", set);
    let row = project.inspection("todo");
    assert_eq!((&row["enabled"], &row["severity"], &row["modified"]), (&json!(false), &json!("error"), &json!(true)));
    let file = project.dir.path().join(PROJECT_FOLDER).join("Project_Default.json");
    let stored: InspectionProfile = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(stored.name, "Project Default");
    assert_eq!(stored.inspections["todo"].enabled, Some(false));
    project.dispatch("inspect.profile.set", json!({ "inspections": { "todo": {} } }));
    assert_eq!(project.inspection("todo")["modified"], false);
}

#[test]
fn turning_on_what_is_off_by_default_is_modified_and_saying_it_is_off_is_not() {
    let mut project = project();
    let row = project.inspection("loud");
    assert_eq!((&row["enabled"], &row["enabledByDefault"]), (&json!(false), &json!(false)));
    project.dispatch("inspect.profile.set", json!({ "inspections": { "loud": { "enabled": false } } }));
    assert_eq!(project.inspection("loud")["modified"], false);
    project.dispatch("inspect.profile.set", json!({ "inspections": { "loud": { "enabled": true } } }));
    assert_eq!(project.inspection("loud")["modified"], true);
}

#[test]
fn what_the_ide_does_not_know_stays_in_the_profile_and_is_shown_apart() {
    let mut project = project();
    project.dispatch("inspect.profile.set", json!({ "inspections": { "gone": { "enabled": false } } }));
    project.dispatch("inspect.profile.set", json!({ "inspections": { "todo": { "severity": "hint" } } }));
    let view = project.dispatch("inspect.profile.get", json!({}));
    assert_eq!(view["unknown"], json!({ "gone": { "enabled": false } }));
}

#[test]
fn the_gear_menu_copies_duplicates_renames_and_deletes() {
    let mut project = project();
    project.dispatch("inspect.profile.set", json!({ "inspections": { "todo": { "enabled": false } } }));
    project.dispatch("inspect.profile.copy", json!({ "to": ide("Team") }));
    assert!(project.ide.path().join("inspectionProfiles/Team.json").is_file());
    project.dispatch("inspect.profile.copy", json!({ "profile": ide("Team"), "to": ide("Team copy") }));
    project.refused("inspect.profile.copy", json!({ "profile": ide("Team"), "to": ide("Team copy") }));
    project.dispatch("inspect.profile.rename", json!({ "profile": ide("Team copy"), "name": "Strict" }));
    let list = project.dispatch("inspect.profile.list", json!({}));
    assert_eq!(list["profiles"][1], json!({ "name": "Strict", "level": "ide" }));
    assert_eq!(list["profiles"][2], json!({ "name": "Team", "level": "ide" }));
    let strict = project.dispatch("inspect.profile.get", json!({ "profile": ide("Strict") }));
    assert_eq!(
        strict["inspections"].as_array().unwrap().iter().find(|row| row["id"] == "todo").unwrap()["enabled"],
        false
    );
    project.dispatch("inspect.profile.delete", json!({ "profile": ide("Strict") }));
    assert_eq!(project.dispatch("inspect.profile.list", json!({}))["profiles"].as_array().unwrap().len(), 2);
    assert!(project.refused("inspect.profile.copy", json!({ "to": ide(" ") })).contains("name"));
}

#[test]
fn the_active_profile_follows_a_rename_and_falls_back_when_deleted() {
    let mut project = project();
    project.dispatch("inspect.profile.copy", json!({ "to": ide("Team") }));
    project.dispatch("inspect.profile.select", json!({ "profile": ide("Team") }));
    project.dispatch("inspect.profile.rename", json!({ "name": "Crew" }));
    assert_eq!(project.dispatch("inspect.profile.list", json!({}))["active"], ide("Crew"));
    project.dispatch("inspect.profile.delete", json!({}));
    assert_eq!(project.dispatch("inspect.profile.list", json!({}))["active"]["name"], "Project Default");
}

#[test]
fn restore_defaults_takes_every_change_out() {
    let mut project = project();
    project.dispatch("inspect.profile.set", json!({ "inspections": { "todo": { "enabled": false } } }));
    assert!(project.file_problems().is_empty());
    project.dispatch("inspect.profile.restore", json!({}));
    assert_eq!(project.inspection("todo")["modified"], false);
    assert_eq!(project.file_problems().len(), 1);
}

#[test]
fn export_and_import_carry_a_profile_and_import_leaves_one_of_that_name_alone() {
    let mut project = project();
    project.dispatch("inspect.profile.copy", json!({ "to": ide("Team") }));
    project.dispatch(
        "inspect.profile.set",
        json!({ "profile": ide("Team"), "inspections": { "todo": { "severity": "error" } } }),
    );
    let file = project.dir.path().join("team.json");
    project.dispatch("inspect.profile.export", json!({ "profile": ide("Team"), "path": file }));
    let imported = project.dispatch("inspect.profile.import", json!({ "path": file, "level": "project" }));
    assert_eq!(imported, json!({ "name": "Team", "level": "project" }));
    let view = project.dispatch("inspect.profile.get", json!({ "profile": imported }));
    assert_eq!(
        view["inspections"].as_array().unwrap().iter().find(|row| row["id"] == "todo").unwrap()["severity"],
        "error"
    );
    assert!(project.refused("inspect.profile.import", json!({ "path": file })).contains("Team"));
    std::fs::write(&file, "not json").unwrap();
    project.refused("inspect.profile.import", json!({ "path": file }));
}

#[test]
fn a_profile_changed_on_disk_is_read_again() {
    let mut project = project();
    assert_eq!(project.file_problems().len(), 1);
    let folder = project.dir.path().join(PROJECT_FOLDER);
    std::fs::create_dir_all(&folder).unwrap();
    let text = r#"{ "name": "Project Default", "inspections": { "marks/todo": { "enabled": false } } }"#;
    std::fs::write(folder.join("Project_Default.json"), text).unwrap();
    project.session.state_mut::<InspectionProfiles>().checked = None;
    assert!(project.file_problems().is_empty());
}

#[test]
fn a_chosen_profile_is_kept_in_the_project_and_read_when_it_is_opened_again() {
    let mut project = project();
    project.dispatch("inspect.profile.copy", json!({ "to": ide("Team") }));
    project.dispatch("inspect.profile.select", json!({ "profile": ide("Team") }));
    project.session.state_mut::<InspectionProfiles>().root = None;
    assert_eq!(project.dispatch("inspect.profile.list", json!({}))["active"], ide("Team"));
    std::fs::remove_file(project.ide.path().join("inspectionProfiles/Team.json")).unwrap();
    project.session.state_mut::<InspectionProfiles>().root = None;
    assert_eq!(project.dispatch("inspect.profile.list", json!({}))["active"]["name"], "Project Default");
}

#[test]
fn an_agent_writes_no_profile_where_the_person_hid_the_folder() {
    let mut project = project();
    project.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".zen/**".to_owned()]);
    let Project { registry, session, dir, .. } = &mut project;
    let change = json!({ "inspections": { "todo": { "enabled": false } } });
    let answer =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "inspect.profile.set", change));
    assert!(format!("{answer:?}").contains(crate::hidden_files::REDACTED_CODE), "{answer:?}");
    assert!(!dir.path().join(PROJECT_FOLDER).exists());
}

#[test]
fn an_agent_copies_no_profile_out_of_a_hidden_file_or_over_one() {
    let mut project = project();
    let secret = project.dir.path().join("secret.txt");
    std::fs::write(&secret, r#"{"name":"Secret","inspections":{}}"#).unwrap();
    project.dispatch("inspect.profile.copy", json!({ "to": { "name": "Mine", "level": "project" } }));
    project.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret.txt".to_owned()]);
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = elsewhere.path().join("out.json");
    let mine = json!({ "name": "Mine", "level": "project" });
    let attempts = [
        ("inspect.profile.import", json!({ "path": secret })),
        ("inspect.profile.export", json!({ "profile": mine, "path": secret })),
        ("inspect.profile.export", json!({ "profile": mine, "path": outside })),
    ];
    let Project { registry, session, .. } = &mut project;
    for (command, params) in attempts {
        let answer = zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params.clone()));
        assert!(answer.is_err(), "{command} {params}: {answer:?}");
    }
    assert!(std::fs::read_to_string(&secret).unwrap().contains("Secret"));
    assert!(!outside.exists());
    project.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".zen/**".to_owned()]);
    let Project { registry, session, .. } = &mut project;
    for (command, params) in [
        ("inspect.profile.delete", json!({ "profile": mine })),
        ("inspect.profile.rename", json!({ "profile": mine, "name": "Other" })),
    ] {
        let answer = zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params.clone()));
        assert!(format!("{answer:?}").contains(crate::hidden_files::REDACTED_CODE), "{command}: {answer:?}");
    }
    let file = project.dir.path().join(PROJECT_FOLDER).join("Mine.json");
    assert!(project.dir.path().join(PROJECT_FOLDER).read_dir().unwrap().next().is_some(), "{file:?}");
}

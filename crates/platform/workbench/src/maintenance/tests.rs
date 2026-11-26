use serde_json::{Value, json};

use super::*;

struct Fixture {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        session.state_mut::<Maintenance>().set_data_dir(dir.path().to_owned());
        Self { dir, registry, session }
    }

    fn with_project() -> Self {
        let mut fixture = Self::new();
        std::fs::create_dir(fixture.project()).unwrap();
        std::fs::write(fixture.project().join("main.go"), "package main\n").unwrap();
        let project = fixture.project();
        fixture.run("file.project.open", json!({ "path": project }));
        fixture.wait_for_index();
        fixture
    }

    fn project(&self) -> PathBuf {
        self.dir.path().join("project")
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn wait_for_index(&mut self) {
        while self.run("search.index.status", json!({}))["ready"] != true {
            std::thread::yield_now();
        }
    }

    fn make(&self, folder: &str) -> PathBuf {
        let path = self.dir.path().join(folder);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("kept"), "x").unwrap();
        path
    }

    fn shown(&self) -> Vec<(String, Vec<String>)> {
        let store = self.session.state::<Notifications>().unwrap();
        let actions = |item: &crate::notifications::Notification| {
            item.actions.iter().map(|action| action.id.clone()).collect::<Vec<_>>()
        };
        store.newest_first().map(|item| (item.body.clone(), actions(item))).collect()
    }
}

#[test]
fn caches_stay_until_the_next_start_which_removes_only_what_was_chosen() {
    let mut fixture = Fixture::new();
    let servers = fixture.make("caches/language-servers/jdtls");
    let libraries = fixture.make("caches/libraries");
    let avatars = fixture.make("caches/avatars");
    let history = fixture.make("local-history");
    let databases = fixture.make("caches/databases");

    let answer = fixture.run("ide.caches.invalidate", json!({}));

    assert_eq!(answer["restartRequired"], true);
    assert_eq!(answer["folders"].as_array().unwrap().len(), 3, "{answer}");
    assert!(servers.exists() && avatars.exists(), "a running IDE keeps using them");

    let removed = apply_scheduled_invalidation(fixture.dir.path());

    assert_eq!(removed.len(), 3, "{removed:?}");
    assert!(!servers.exists() && !libraries.exists() && !avatars.exists());
    assert!(history.exists(), "Local History goes only when it is asked for");
    assert!(databases.exists(), "a connection's structure is what offline work reads");
}

#[test]
fn the_vcs_log_checkbox_keeps_its_caches_and_the_local_history_one_clears_history() {
    let mut fixture = Fixture::new();
    let avatars = fixture.make("caches/avatars");
    let history = fixture.make("local-history");

    fixture.run("ide.caches.invalidate", json!({ "clearLocalHistory": true, "clearVcsLog": false }));
    apply_scheduled_invalidation(fixture.dir.path());

    assert!(avatars.exists());
    assert!(!history.exists());
}

#[test]
fn an_agent_can_never_clear_local_history() {
    let mut fixture = Fixture::new();
    let history = fixture.make("local-history");
    let Fixture { registry, session, .. } = &mut fixture;
    let asked = json!({ "clearLocalHistory": true });
    let refused =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "ide.caches.invalidate", asked));
    match refused {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some(crate::trust::AGENT_NOT_ALLOWED), "{error:?}")
        }
        other => panic!("not refused: {other:?}"),
    }
    assert!(!fixture.dir.path().join(SCHEDULE).exists(), "nothing is scheduled");
    let Fixture { registry, session, .. } = &mut fixture;
    let caches =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "ide.caches.invalidate", json!({})));
    assert!(caches.is_ok(), "the caches themselves are derived data: {caches:?}");
    apply_scheduled_invalidation(fixture.dir.path());
    assert!(history.exists());
}

#[test]
fn a_schedule_applies_once_and_a_broken_one_removes_nothing() {
    let mut fixture = Fixture::new();
    fixture.run("ide.caches.invalidate", json!({}));
    apply_scheduled_invalidation(fixture.dir.path());
    let servers = fixture.make("caches/language-servers");

    assert!(apply_scheduled_invalidation(fixture.dir.path()).is_empty());

    std::fs::write(fixture.dir.path().join(SCHEDULE), "{ broken").unwrap();
    assert!(apply_scheduled_invalidation(fixture.dir.path()).is_empty());
    assert!(servers.exists());
    assert!(!fixture.dir.path().join(SCHEDULE).exists());
}

#[test]
fn invalidating_without_a_data_folder_is_refused() {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Workbench::default());
    assert!(registry.dispatch(&mut session, "ide.caches.invalidate", json!({})).is_err());
}

#[test]
fn repair_goes_through_its_steps_in_order_with_one_notification_at_a_time() {
    let mut fixture = Fixture::with_project();
    let mut step = json!({});
    let mut seen = Vec::new();
    loop {
        let report = fixture.run("ide.repair.run", step.clone());
        seen.push(report["step"].as_str().unwrap().to_owned());
        let shown = fixture.shown();
        assert_eq!(shown.len(), 1, "{shown:?}");
        assert!(shown[0].0.contains(report["did"].as_str().unwrap()), "{shown:?}");
        if report["next"].is_null() {
            assert_eq!(shown[0].1, ["works", "more"], "the last step has nothing to continue to");
            break;
        }
        assert_eq!(shown[0].1, ["works", "continue", "more"]);
        step = json!({ "step": report["next"] });
    }
    assert_eq!(seen, ["refreshFileSystem", "rescanIndexes", "resync", "dropIndexes"]);

    fixture.run("ide.repair.finish", json!({}));
    assert!(fixture.shown().is_empty(), "Everything Works Now ends it");
}

#[test]
fn repair_needs_a_project() {
    let mut fixture = Fixture::new();
    assert!(fixture.registry.dispatch(&mut fixture.session, "ide.repair.run", json!({})).is_err());
}

#[test]
fn refreshing_reloads_an_open_file_changed_behind_the_ides_back() {
    let mut fixture = Fixture::with_project();
    let file = fixture.project().join("main.go");
    fixture.run("file.document.open", json!({ "path": file }));
    std::fs::write(&file, "package other\n").unwrap();

    let report = fixture.run("ide.repair.run", json!({ "step": "refreshFileSystem" }));

    assert!(report["did"].as_str().unwrap().contains('1'), "{report}");
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.active_document().unwrap().text(), "package other\n");
}

#[test]
fn dropping_the_indexes_finds_files_nothing_told_the_index_about() {
    let mut fixture = Fixture::with_project();
    std::fs::write(fixture.project().join("added.go"), "package main\n").unwrap();

    fixture.run("ide.repair.run", json!({ "step": "dropIndexes" }));
    fixture.wait_for_index();

    assert_eq!(fixture.session.state::<Workbench>().unwrap().file_index().unwrap().len(), 2);
}

#[test]
fn rescanning_finds_them_too_and_keeps_the_index() {
    let mut fixture = Fixture::with_project();
    std::fs::write(fixture.project().join("added.go"), "package main\n").unwrap();

    fixture.run("ide.repair.run", json!({ "step": "rescanIndexes" }));

    while fixture.session.state::<Workbench>().unwrap().file_index().unwrap().len() != 2 {
        std::thread::yield_now();
    }
}

#[test]
fn repair_on_file_reloads_that_file_and_reads_it_into_the_index() {
    let mut fixture = Fixture::with_project();
    let file = fixture.project().join("main.go");
    fixture.run("file.document.open", json!({ "path": file }));
    std::fs::write(&file, "package needle\n").unwrap();
    let added = fixture.project().join("added.go");
    std::fs::write(&added, "package main\n").unwrap();

    let report = fixture.run("ide.repair.file", json!({ "path": file }));
    assert!(report["did"].as_str().unwrap().contains("main.go"), "{report}");
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.active_document().unwrap().text(), "package needle\n");

    fixture.run("ide.repair.file", json!({ "path": added }));
    assert_eq!(fixture.session.state::<Workbench>().unwrap().file_index().unwrap().len(), 2);
    assert_eq!(fixture.shown().len(), 2, "each reports what it did");
    assert!(fixture.shown()[0].1.is_empty(), "it offers no further steps");
}

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_project::inspect::{Finding, FindingSeverity, FixJob, InspectContext, InspectionFix, Inspector};

use super::*;
use crate::inspections::Inspections;
use crate::language::testing;

const MAIN: &str = "package main\n\nfunc main() {\n\tx := 1\n}\n";

#[derive(Default)]
struct Counting {
    asked: AtomicUsize,
}

impl Inspector for Counting {
    fn id(&self) -> &str {
        "counting"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name == "go.mod"
    }

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        vec![Finding {
            start: 0,
            end: context.text.len().min(6),
            severity: FindingSeverity::Warning,
            message: "a newer version".to_owned(),
            note: None,
            rows: Vec::new(),
            details: None,
            fixes: Vec::new(),
            source: "counting".to_owned(),
            code: "update".to_owned(),
            deprecated: false,
            unused: false,
        }]
    }

    fn apply(&self, _: &InspectContext<'_>, _: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        Err("no fixes".to_owned())
    }
}

fn problems(fixture: &mut testing::Fixture, name: &str) -> Value {
    let path = fixture.path(name);
    fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap()
}

#[test]
fn inspections_and_their_network_checks_wait_while_power_save_is_on() {
    let mut fixture = testing::fixture(&[("go.mod", "module example.com/m\n\ngo 1.22\n")]);
    let counting = Arc::new(Counting::default());
    fixture.session.state_mut::<Inspections>().set_inspectors(vec![Arc::clone(&counting) as Arc<dyn Inspector>], None);
    let path = fixture.path("go.mod");
    fixture.dispatch("file.document.open", json!({ "path": path })).unwrap();
    fixture.session.settings_mut().set(ENABLED, true).unwrap();
    let answer = problems(&mut fixture, "go.mod");
    assert_eq!(answer["reason"], "powerSave", "{answer}");
    assert!(crate::inspections::all_problems(&mut fixture.session, &path).is_empty());
    assert_eq!(counting.asked.load(Ordering::SeqCst), 0, "the inspector is not asked while it is on");
    fixture.session.settings_mut().set(ENABLED, false).unwrap();
    let back = crate::inspections::all_problems(&mut fixture.session, &path);
    assert_eq!(back.len(), 1);
    assert_eq!(counting.asked.load(Ordering::SeqCst), 1);
}

#[test]
fn changed_files_wait_for_the_index_while_power_save_is_on() {
    let mut fixture = testing::fixture(&[("main.go", MAIN)]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fixture
        .session
        .state::<Workbench>()
        .and_then(|w| w.index.as_ref())
        .is_some_and(|index| index.text_status().ready)
    {
        assert!(Instant::now() < deadline, "the index is built");
        std::thread::sleep(Duration::from_millis(5));
    }
    fixture.session.settings_mut().set(ENABLED, true).unwrap();
    let added = fixture.path("added.go");
    std::fs::write(&added, "package main\n").unwrap();
    fixture.dispatch("file.index.refresh", json!({ "paths": [added] })).unwrap();
    let answer = fixture.dispatch("powerSave.status.get", json!({})).unwrap();
    assert_eq!(answer["deferredFiles"], 1, "{answer}");
    fixture.session.settings_mut().set(ENABLED, false).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let answer = fixture.dispatch("powerSave.status.get", json!({})).unwrap();
    assert_eq!(answer, json!({ "on": false, "paused": [], "deferredFiles": 0, "serversPaused": [] }));
}

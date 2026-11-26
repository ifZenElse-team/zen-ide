use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::memory::MemoryDriver;
use zen_db::recovery::Recovery;

use super::{Store, capture, finish};
use crate::Workbench;
use crate::notifications::Notifications;
use crate::workspaces::{SavedWorkspace, Workspaces};

const WAIT: Duration = Duration::from_secs(10);

struct Ide {
    registry: Registry,
    session: Session,
    recovery: Arc<Recovery>,
    sql: Arc<MemoryDriver>,
}

impl Ide {
    fn start(dir: &Path, saved: Vec<SavedWorkspace>) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        super::super::register(&mut registry).unwrap();
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut session = Session::new();
        super::super::install(&mut session);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Workspaces>().set_data_dir(dir.join("data"));
        session.set_state(super::super::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
        let sql = Arc::new(MemoryDriver::sql());
        let kv = Arc::new(MemoryDriver::new(false));
        let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::clone(&sql) as _, kv];
        session.state_mut::<super::Database>().set_drivers(drivers);
        let recovery = Arc::new(Recovery::open(&dir.join("data/databases")));
        session.set_state(Store(Arc::clone(&recovery)));
        crate::workspaces::restore(&mut session, saved);
        assert!(recovery.flush(WAIT));
        Self { registry, session, recovery, sql }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn refused(&mut self, command: &str, params: Value) -> zen_core::CommandError {
        match self.registry.dispatch(&mut self.session, command, params).unwrap_err() {
            zen_core::DispatchError::Failed { error, .. } => error,
            other => panic!("{other}"),
        }
    }

    fn capture(&mut self) {
        capture(&mut self.session);
        assert!(self.recovery.flush(WAIT));
    }

    fn document(&self, path: &Path) -> (String, bool) {
        let workbench = self.session.state::<Workbench>().unwrap();
        let document = workbench.documents().iter().find(|document| document.path() == path).unwrap();
        (document.text().to_owned(), document.is_dirty())
    }

    fn type_into(&mut self, path: &Path, text: &str) {
        let workbench = self.session.state_mut::<Workbench>();
        let document = workbench.documents.iter_mut().find(|document| document.path == path).unwrap();
        let end = document.snapshot().len_chars();
        assert!(document.replace_ranges(vec![(end..end, text.to_owned())]));
    }
}

fn shop(ide: &mut Ide, dir: &Path) -> String {
    let added = ide.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "s3cret-pw", "type": "dev" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let file = dir.join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &super::super::tests::sql_snapshot(&id, true)).unwrap();
    id
}

fn until_rows(ide: &mut Ide, tab: &str) {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let state = ide.run("db.object.state", json!({ "tab": tab }));
        if state["data"]["result"].is_u64() && !state["data"]["reading"].as_bool().unwrap_or(true) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "the rows were read: {state}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn after_a_crash_consoles_come_back_unsaved_and_edits_and_transactions_are_offered_for_review() {
    let dir = tempfile::tempdir().unwrap();
    let mut ide = Ide::start(dir.path(), Vec::new());
    let id = shop(&mut ide, dir.path());
    let opened =
        ide.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    ide.run("db.object.readData", json!({ "tab": tab }));
    until_rows(&mut ide, &tab);
    ide.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Ann" }));
    ide.run("db.result.deleteRows", json!({ "rows": [1] }));

    let kv = ide.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "kv-pw" }));
    let kv = kv["id"].as_str().unwrap().to_owned();
    let console = ide.run("db.console.new", json!({ "connection": kv, "text": "SET a 1\n" }));
    let path = PathBuf::from(console["path"].as_str().unwrap());
    ide.run("db.transaction.mode", json!({ "mode": "manual" }));
    let started = ide.run("db.console.executeScript", json!({}));
    let ran = ide.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }));
    assert_eq!(ran["status"], "done", "{ran}");
    ide.type_into(&path, "GET a -- typed, not saved\n");
    assert!(ide.document(&path).1);

    ide.capture();
    let kept = std::fs::read_to_string(ide.recovery.file()).unwrap();
    assert!(kept.contains("typed, not saved") && kept.contains("Customer 1"), "{kept}");
    assert!(!kept.contains("s3cret-pw") && !kept.contains("kv-pw"), "no secrets are kept");
    let executed_before = ide.sql.server.executed().len();
    let saved = crate::workspaces::saved(&mut ide.session);
    drop(ide);

    let mut ide = Ide::start(dir.path(), saved);
    ide.capture();
    let (text, dirty) = ide.document(&path);
    assert_eq!(text, "SET a 1\nGET a -- typed, not saved\n");
    assert!(dirty, "the text comes back as unsaved changes");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "SET a 1\n", "the file is not written by recovering");
    let notes = ide.session.state::<Notifications>().unwrap();
    let bodies: Vec<String> = notes.newest_first().map(|note| note.body.clone()).collect();
    assert!(bodies.iter().any(|body| body.contains("2 unsaved items were kept")), "{bodies:?}");
    assert!(bodies.iter().any(|body| body.contains("console.sql")), "{bodies:?}");

    let list = ide.run("db.recovery.list", json!({}));
    assert_eq!(list["total"], 2, "{list}");
    let kinds: Vec<&str> =
        list["items"].as_array().unwrap().iter().map(|item| item["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"edits") && kinds.contains(&"transaction"), "{kinds:?}");
    let find = |kind: &str| list["items"].as_array().unwrap().iter().find(|item| item["kind"] == kind).unwrap().clone();
    let edits = find("edits");
    assert_eq!((edits["count"].as_u64(), edits["connectionName"].as_str()), (Some(2), Some("Shop")));
    let transaction = find("transaction");
    assert_eq!((transaction["count"].as_u64(), transaction["modifying"].as_u64()), (Some(1), Some(1)));

    let detail = ide.run("db.recovery.get", json!({ "id": edits["id"] }));
    let sql = detail["text"].as_str().unwrap();
    assert!(sql.starts_with("DELETE FROM \"public\".\"customers\" WHERE \"id\" = 2;"), "{sql}");
    assert!(
        sql.contains(
            "UPDATE \"public\".\"customers\" SET \"name\" = 'Ann' WHERE \"id\" = 1 AND \"name\" IS NOT DISTINCT FROM 'Customer 1'"
        ),
        "{sql}"
    );
    let detail = ide.run("db.recovery.get", json!({ "id": transaction["id"] }));
    assert_eq!(detail["log"], json!(["SET a 1"]));

    let asked = ide.refused("db.recovery.discard", json!({ "all": true }));
    assert_eq!(asked.code.as_deref(), Some(super::CONFIRM_DISCARD), "unsaved edits are never dropped unasked");
    let data = asked.data.unwrap();
    assert_eq!(data["items"].as_array().map(Vec::len), Some(1), "only the edits: {data:?}");
    assert_eq!(data["retry"]["params"]["confirmed"], true);
    assert_eq!(ide.run("db.recovery.list", json!({}))["total"], 2, "nothing dropped yet");
    let refused = ide.refused("db.recovery.restore", json!({ "id": transaction["id"] }));
    assert_eq!(refused.code.as_deref(), Some("rolledBack"));
    let restored = ide.run("db.recovery.restore", json!({ "id": edits["id"] }));
    let written = std::fs::read_to_string(restored["path"].as_str().unwrap()).unwrap();
    assert!(
        written.starts_with("-- Unsaved changes of \"public\".\"customers\" on Shop") && written.contains(sql),
        "{written}"
    );
    let status = ide.run("db.connection.status", json!({}));
    assert!(
        status["connections"].as_array().unwrap().iter().all(|connection| connection["state"] == "disconnected"),
        "recovering connects nowhere: {status}"
    );
    assert_eq!(ide.sql.server.executed().len(), 0, "nothing is run (the old process ran {executed_before})");

    let nothing = ide.run("db.recovery.discard", json!({}));
    assert_eq!(nothing["discarded"], 0, "no ids and no `all`: nothing is dropped");
    let discarded = ide.run("db.recovery.discard", json!({ "all": true }));
    assert_eq!(
        (discarded["discarded"].as_u64(), discarded["left"].as_u64()),
        (Some(1), Some(0)),
        "a notice drops at once"
    );
    ide.capture();
    assert!(ide.recovery.file().exists(), "the console's unsaved text is still kept");
    let (saved, _) = crate::workspaces::save_consoles(&mut ide.session);
    assert_eq!(saved, std::slice::from_ref(&path));
    ide.capture();
    assert!(!ide.recovery.file().exists(), "nothing left to keep, no file");
    finish(&mut ide.session);
}

#[test]
fn a_clean_exit_forgets_the_open_transactions_it_dealt_with() {
    let dir = tempfile::tempdir().unwrap();
    let mut ide = Ide::start(dir.path(), Vec::new());
    let kv = ide.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "kv-pw" }));
    let kv = kv["id"].as_str().unwrap().to_owned();
    ide.run("db.console.new", json!({ "connection": kv, "text": "SET a 1\n" }));
    ide.run("db.transaction.mode", json!({ "mode": "manual" }));
    let started = ide.run("db.console.executeScript", json!({}));
    ide.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }));
    ide.capture();
    assert!(ide.recovery.file().exists());
    finish(&mut ide.session);
    assert!(ide.recovery.flush(WAIT));
    assert!(!ide.recovery.file().exists(), "{:?}", std::fs::read_to_string(ide.recovery.file()));
}

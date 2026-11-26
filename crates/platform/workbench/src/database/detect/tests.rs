use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_secrets::SecretStore;
use zen_secrets::database::Kind;

use crate::workspaces::Workspaces;

struct Window {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
    secrets: std::sync::Arc<zen_secrets::MemoryStore>,
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const COMPOSE: &str = r#"services:
  db:
    image: postgres:16
    ports: ["5433:5432"]
    environment:
      POSTGRES_USER: app
      POSTGRES_PASSWORD: ${DB_PASSWORD}
      POSTGRES_DB: shop
  cache:
    image: mongo:7
    ports: ["27018:27017"]
  web:
    image: nginx
"#;

impl Window {
    fn new(fixture: impl FnOnce(&Path)) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        super::super::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir(&project).unwrap();
        fixture(&project);
        let mut session = Session::new();
        super::super::install(&mut session);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
        let secrets = std::sync::Arc::new(zen_secrets::MemoryStore::new());
        session.set_state(super::super::Secrets(secrets.clone()));
        Self { registry, session, dir, secrets }
    }

    fn project(&self) -> PathBuf {
        self.dir.path().join("shop")
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn settled(&mut self, operation: &Value) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let state = self.run("db.operation.state", json!({ "operation": operation }));
            if state["status"] != "running" {
                assert_eq!(state["status"], "done", "{state}");
                return;
            }
            assert!(Instant::now() < deadline, "{state}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn error(&mut self, command: &str, params: Value) -> zen_core::DispatchError {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err()
    }

    fn detections(&mut self, params: Value) -> Value {
        let started = Instant::now();
        loop {
            let listed = self.run("db.detect.list", params.clone());
            if listed["scanning"] == false {
                return listed;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "the scan never ended");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn changed(&mut self, file: &str, text: &str) {
        let path = self.project().join(file);
        write(&path, text);
        super::files_changed(&mut self.session, &[path]);
    }
}

fn keys(listed: &Value) -> Vec<String> {
    listed["detections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|offer| format!("{}:{}", offer["file"].as_str().unwrap(), offer["key"].as_str().unwrap_or("?")))
        .collect()
}

fn shop(project: &Path) {
    write(
        &project.join(".env"),
        "DATABASE_URL=postgres://app:env-s3cret@localhost:5432/shop_dev\nDB_PASSWORD=compose-s3cret\n",
    );
    write(&project.join("docker-compose.yml"), COMPOSE);
    write(&project.join("api/.env.example"), "REDIS_URL=redis://localhost:6379/0\n");
    write(&project.join("web/package.json"), "{}");
    write(&project.join("web/node_modules/pkg/.env"), "DATABASE_URL=postgres://x@excluded/db\n");
    write(&project.join(".gitignore"), "build/\n");
    write(&project.join("build/.env"), "DATABASE_URL=postgres://x@ignored/db\n");
    write(&project.join("a/b/c/d/e/.env"), "DATABASE_URL=postgres://x@deep/db\n");
    write(&project.join(".git/.env"), "DATABASE_URL=postgres://x@git/db\n");
}

#[test]
fn env_and_compose_files_are_scanned_in_the_background_without_excluded_ignored_or_deep_folders() {
    let mut window = Window::new(shop);
    let listed = window.detections(json!({}));
    assert_eq!(
        keys(&listed),
        [".env:DATABASE_URL", "docker-compose.yml:db", "docker-compose.yml:cache", "api/.env.example:REDIS_URL"],
        "{listed}"
    );
    let text = listed.to_string();
    for secret in ["env-s3cret", "compose-s3cret"] {
        assert!(!text.contains(secret), "no password in the answer: {text}");
    }
    for elsewhere in ["excluded", "ignored", "deep", "\"git\""] {
        assert!(!text.contains(elsewhere), "{elsewhere}: {text}");
    }
    let env = &listed["detections"][0];
    assert_eq!(
        (env["driver"].as_str(), env["host"].as_str(), env["port"].as_u64()),
        (Some("postgresql"), Some("localhost"), Some(5432))
    );
    assert_eq!((env["hasPassword"].as_bool(), env["supported"].as_bool()), (Some(true), Some(true)));
    let db = &listed["detections"][1];
    assert_eq!(
        (db["port"].as_u64(), db["user"].as_str(), db["database"].as_str()),
        (Some(5433), Some("app"), Some("shop"))
    );
    assert_eq!((db["published"].as_bool(), db["hasPassword"].as_bool()), (Some(true), Some(true)));
    assert_eq!(listed["detections"][2]["supported"], false, "no MongoDB driver is compiled in yet");
    let limited = window.run("db.detect.list", json!({ "limit": 1 }));
    assert_eq!((limited["total"].as_u64(), limited["truncated"].as_bool()), (Some(4), Some(true)));
}

#[test]
fn adding_an_offer_stores_its_password_in_the_keychain_only_and_a_changed_file_offers_it_again() {
    let mut window = Window::new(shop);
    let listed = window.detections(json!({}));
    let id = listed["detections"][1]["id"].as_str().unwrap().to_owned();
    let added = window.run("db.detect.accept", json!({ "id": id }));
    assert_eq!(added["replaced"], false);
    let connection = added["connection"]["id"].as_str().unwrap().to_owned();
    assert_eq!(added["connection"]["name"], "db");
    assert_eq!(added["connection"]["hasPassword"], true);
    assert_eq!(added["connection"]["origin"]["kind"], "compose");
    assert_eq!(added["connection"]["origin"]["file"], "docker-compose.yml");
    assert!(!added.to_string().contains("compose-s3cret"), "{added}");
    window.settled(&added["connection"]["secretOperation"]);
    let key = zen_secrets::database::id(None, &connection, Kind::Password).unwrap();
    assert_eq!(window.secrets.get(&key).unwrap().unwrap().expose(), "compose-s3cret");
    let file = std::fs::read_to_string(window.dir.path().join("data/databases/data-sources.json")).unwrap();
    assert!(!file.contains("compose-s3cret") && file.contains("\"detection\""), "{file}");
    assert!(!window.project().join(".zen").exists(), "nothing written to the project");
    let listed = window.detections(json!({}));
    assert!(!keys(&listed).contains(&"docker-compose.yml:db".to_owned()), "added offers go: {listed}");

    window.changed("docker-compose.yml", &COMPOSE.replace("5433:5432", "5434:5432"));
    let listed = window.detections(json!({}));
    let offer = listed["detections"].as_array().unwrap().iter().find(|offer| offer["key"] == "db").unwrap().clone();
    assert_eq!((offer["connection"].as_str(), offer["port"].as_u64()), (Some(connection.as_str()), Some(5434)));
    let again = window.run("db.detect.accept", json!({ "id": id, "name": "Shop DB" }));
    assert_eq!(
        (again["replaced"].as_bool(), again["connection"]["id"].as_str()),
        (Some(true), Some(connection.as_str()))
    );
    assert_eq!(
        (again["connection"]["port"].as_u64(), again["connection"]["name"].as_str()),
        (Some(5434), Some("Shop DB"))
    );
    assert_eq!(window.run("db.dataSource.list", json!({}))["total"], 1);

    let refused = window.error("db.detect.accept", json!({ "id": "env-000000000000" }));
    assert!(
        refused.to_string().contains("no longer found") && !refused.to_string().contains("db.detect.list"),
        "{refused}"
    );
    let registry = &window.registry;
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.detect.accept", json!({ "id": "env-000000000000" })).unwrap_err()
    });
    let text = refused.to_string();
    assert!(text.contains("env-000000000000") && text.contains("`db.detect.list`"), "{text}");
}

#[test]
fn a_connection_like_the_offer_hides_it_and_unsupported_drivers_are_refused() {
    let mut window = Window::new(shop);
    window
        .run("db.dataSource.add", json!({ "name": "Mine", "host": "localhost", "port": 5432, "database": "shop_dev" }));
    let listed = window.detections(json!({}));
    assert!(!keys(&listed).contains(&".env:DATABASE_URL".to_owned()), "{listed}");
    let cache = listed["detections"].as_array().unwrap().iter().find(|offer| offer["key"] == "cache").unwrap().clone();
    let refused = window.error("db.detect.accept", json!({ "id": cache["id"] }));
    assert!(refused.to_string().contains("mongodb"), "{refused}");
}

#[test]
fn dismissed_offers_are_remembered_per_project_until_their_file_changes() {
    let mut window = Window::new(shop);
    let listed = window.detections(json!({}));
    let id = listed["detections"][0]["id"].as_str().unwrap().to_owned();
    window.run("db.detect.dismiss", json!({ "id": id }));
    let listed = window.detections(json!({}));
    assert_eq!((listed["total"].as_u64(), listed["dismissed"].as_u64()), (Some(3), Some(1)));
    let all = window.detections(json!({ "includeDismissed": true }));
    assert_eq!(all["detections"][0]["dismissed"], true);
    let file = window
        .dir
        .path()
        .join("data/databases")
        .join(super::super::project_key(&window.project()))
        .join("detections.json");
    assert!(file.is_file());
    assert!(!std::fs::read_to_string(&file).unwrap().contains("s3cret"));

    window.changed(".env", "DATABASE_URL=postgres://app:env-s3cret@localhost:5432/shop_test\n");
    let listed = window.detections(json!({}));
    assert_eq!(listed["dismissed"], 0, "another database is offered again: {listed}");
    window.run("db.detect.dismiss", json!({ "id": id }));
    window.run("db.detect.dismiss", json!({ "id": id, "undo": true }));
    assert_eq!(window.detections(json!({}))["dismissed"], 0);
}

#[test]
fn agents_see_offers_from_hidden_files_without_values_and_cannot_add_them() {
    let mut window = Window::new(shop);
    window.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned(), ".env.*".to_owned()]);
    window.detections(json!({}));
    let listed = zen_core::macros::as_agent(&mut window.session, |session| {
        window.registry.dispatch(session, "db.detect.list", json!({})).unwrap()
    });
    let text = listed.to_string();
    for value in ["shop_dev", "localhost", "DATABASE_URL", "REDIS_URL", "postgresql", "app"] {
        assert!(!text.contains(value), "{value}: {text}");
    }
    assert!(listed["detections"].as_array().unwrap().iter().all(|offer| offer["redacted"] == true), "{listed}");
    let id = listed["detections"][0]["id"].clone();
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        window.registry.dispatch(session, "db.detect.accept", json!({ "id": id })).unwrap_err()
    });
    assert!(refused.to_string().contains(".env"), "{refused}");
    let user = window.detections(json!({}));
    assert_eq!(user["detections"][0]["host"], "localhost", "the person sees every value");
}

const DBEAVER: &str = r#"{
  "folders": {},
  "connections": {
    "postgres-jdbc-1": { "provider": "postgresql", "driver": "postgres-jdbc", "name": "Billing", "folder": "Team",
      "configuration": { "host": "billing.local", "port": "5432", "database": "billing", "user": "ann", "type": "prod" } },
    "oracle-2": { "provider": "oracle", "driver": "oracle_thin", "name": "Legacy",
      "configuration": { "host": "legacy.local", "port": "1521", "database": "legacy" } }
  }
}"#;

#[test]
fn dbeaver_connections_are_previewed_imported_once_and_its_files_never_changed() {
    let mut window = Window::new(|_| {});
    let workspace = window.dir.path().join("dbeaver/workspace6");
    let file = workspace.join("General/.dbeaver/data-sources.json");
    write(&file, DBEAVER);
    write(&workspace.join("General/.dbeaver/credentials-config.json"), "encrypted");
    let before = std::fs::read(&file).unwrap();
    let preview = window.run("db.import.dbeaver", json!({ "path": workspace, "preview": true }));
    assert_eq!(preview["added"], 0);
    let statuses: Vec<(&str, &str)> = preview["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (entry["name"].as_str().unwrap(), entry["status"].as_str().unwrap()))
        .collect();
    assert_eq!(statuses, [("Billing", "ready"), ("Legacy", "unsupported")]);
    assert_eq!(window.run("db.dataSource.list", json!({}))["total"], 0);
    let imported = window.run("db.import.dbeaver", json!({ "path": workspace, "keys": ["postgres-jdbc-1"] }));
    assert_eq!(imported["added"], 1);
    let id = imported["entries"][0]["connection"].as_str().unwrap().to_owned();
    let connection = window.run("db.dataSource.get", json!({ "id": id }));
    assert_eq!(
        (connection["host"].as_str(), connection["type"].as_str(), connection["folder"].as_str()),
        (Some("billing.local"), Some("prod"), Some("Team"))
    );
    assert_eq!((connection["user"].as_str(), connection["hasPassword"].as_bool()), (Some("ann"), Some(false)));
    assert_eq!(connection["origin"]["kind"], "dbeaver");
    let again = window.run("db.import.dbeaver", json!({ "path": file }));
    assert_eq!((again["added"].as_u64(), again["entries"][0]["status"].as_str()), (Some(0), Some("exists")));
    assert_eq!(std::fs::read(&file).unwrap(), before, "DBeaver's file is never changed");
    assert!(window.secrets.ids().is_empty(), "no secret is read or stored");
    let missing = window.error("db.import.dbeaver", json!({ "path": window.dir.path().join("nowhere") }));
    assert!(missing.to_string().contains("data-sources.json"), "{missing}");
}

#[test]
fn jetbrains_data_sources_of_the_open_project_are_imported_into_the_project_scope() {
    let shared = r#"<project version="4"><component name="DataSourceManagerImpl" format="xml" multifile-model="true">
        <data-source source="LOCAL" name="shop@localhost" uuid="u1"><driver-ref>postgresql</driver-ref>
        <jdbc-url>jdbc:postgresql://localhost:5432/shop</jdbc-url></data-source></component></project>"#;
    let local = r#"<project version="4"><component name="dataSourceStorageLocal">
        <data-source name="shop@localhost" uuid="u1"><user-name>ann</user-name></data-source></component></project>"#;
    let mut window = Window::new(|project| {
        write(&project.join(".idea/dataSources.xml"), shared);
        write(&project.join(".idea/dataSources.local.xml"), local);
    });
    let imported = window.run("db.import.jetbrains", json!({ "scope": "project" }));
    assert_eq!(imported["added"], 1, "{imported}");
    let listed = window.run("db.dataSource.list", json!({ "scope": "project" }));
    assert_eq!(listed["connections"][0]["user"], "ann");
    let shared_file = std::fs::read_to_string(window.project().join(".zen/data-sources.json")).unwrap();
    assert!(!shared_file.contains("\"ann\""), "the user name is the person's own: {shared_file}");
    assert_eq!(std::fs::read_to_string(window.project().join(".idea/dataSources.xml")).unwrap(), shared);
}

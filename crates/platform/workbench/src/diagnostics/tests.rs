use serde_json::{Value, json};

use super::*;
use crate::ide_settings::zip::{Archive, Limits};

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
        session.set_state(crate::Workbench::default());
        session.state_mut::<Diagnostics>().set_product(dir.path().to_owned(), "GoGarden", "26.11.2");
        Self { dir, registry, session }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn collected(&mut self) -> (Value, std::collections::BTreeMap<String, String>) {
        let path = self.dir.path().join("out/report.zip");
        let answer = self.run("diagnostics.logs.collect", json!({ "path": path, "noReveal": true, "wait": true }));
        let archive = Archive::parse(std::fs::read(path).unwrap(), Limits::SETTINGS).unwrap();
        let read = |entry| String::from_utf8(archive.read(entry).unwrap()).unwrap();
        (answer, archive.entries().iter().map(|entry| (entry.name.clone(), read(entry))).collect())
    }
}

#[test]
fn collecting_zips_the_logs_servers_versions_and_settings_and_hides_secrets() {
    let mut fixture = Fixture::new();
    let logs = fixture.dir.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("zen.log"), "today\n").unwrap();
    std::fs::write(logs.join("zen.1.log"), "yesterday\n").unwrap();
    std::fs::write(logs.join("other.txt"), "not a log\n").unwrap();
    let settings = json!({
        "ligatures": true,
        "proxy": { "user": "denis", "password": "hunter2" },
        "agentToken": "abc",
        "servers": [{ "api_key": 1, "apiKey": "abc", "name": "kept" }],
    });
    std::fs::write(fixture.dir.path().join("settings.json"), settings.to_string()).unwrap();
    let servers = fixture.session.state_mut::<LanguageServers>().clone();
    servers.log("gopls").push("gopls started".to_owned());

    let (answer, files) = fixture.collected();

    assert!(answer["bytes"].as_u64().unwrap() > 0, "{answer}");
    assert_eq!(files["logs/zen.log"], "today\n");
    assert_eq!(files["logs/zen.1.log"], "yesterday\n");
    assert!(!files.keys().any(|name| name.contains("other")), "{files:?}");
    assert!(files["language-servers/gopls.log"].contains("gopls started"));
    assert!(files["versions.txt"].starts_with("GoGarden 26.11.2\n"));
    let saved = &files["settings.json"];
    assert!(saved.contains("denis") && saved.contains("ligatures") && saved.contains("kept"), "{saved}");
    assert!(!saved.contains("hunter2") && !saved.contains("\"abc\""), "{saved}");
}

#[test]
fn a_background_collection_writes_the_file_and_finishes_its_task() {
    let mut fixture = Fixture::new();
    let path = fixture.dir.path().join("report.zip");

    let answer = fixture.run("diagnostics.logs.collect", json!({ "path": path, "noReveal": true }));

    assert_eq!(answer["bytes"], Value::Null, "{answer}");
    while !path.exists() {
        std::thread::yield_now();
    }
}

#[test]
fn the_default_archive_goes_to_the_log_folder_with_a_date() {
    let folder = Path::new("/logs");
    let path = default_path(folder);
    assert_eq!(path.parent(), Some(folder));
    assert!(path.file_name().unwrap().to_string_lossy().starts_with("diagnostics-2"));
}

#[test]
fn show_log_selects_the_current_log_and_creates_it_when_nothing_was_written() {
    let mut fixture = Fixture::new();
    let answer = fixture.run("diagnostics.log.show", json!({ "dryRun": true }));
    let file = fixture.dir.path().join("logs/zen.log");
    assert!(file.is_file());
    assert_eq!(answer["ran"], false);
    assert!(answer["args"].to_string().contains("zen.log"), "{answer}");
}

#[test]
fn debug_log_categories_are_stored_without_empty_lines() {
    let mut fixture = Fixture::new();
    let kept = fixture.run("diagnostics.logCategories.set", json!({ "categories": [" zen_lsp ", "", "egui:trace"] }));
    assert_eq!(kept["categories"], json!(["zen_lsp", "egui:trace"]));
    assert_eq!(fixture.run("diagnostics.logCategories.get", json!({})), kept);
}

#[test]
fn special_folders_name_settings_data_plugins_caches_and_logs_with_their_paths() {
    let mut fixture = Fixture::new();
    let data = fixture.dir.path().to_owned();
    let found = fixture.run("diagnostics.folders.list", json!({}));
    let rows: Vec<(&str, &str)> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["name"].as_str().unwrap(), row["path"].as_str().unwrap()))
        .collect();
    let path = |name: &str| data.join(name).display().to_string();
    let (settings, plugins, caches, logs) = (path("settings.json"), path("plugins"), path("caches"), path("logs"));
    let data = data.display().to_string();
    assert_eq!(
        rows,
        [
            ("Settings", settings.as_str()),
            ("Data", data.as_str()),
            ("Plug-ins", plugins.as_str()),
            ("Caches", caches.as_str()),
            ("Logs", logs.as_str()),
        ]
    );
}

#[test]
fn without_a_data_folder_the_commands_are_refused() {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    assert!(registry.dispatch(&mut session, "diagnostics.folders.list", json!({})).is_err());
}

fn agent_code(fixture: &mut Fixture, command: &str, params: Value) -> String {
    let Fixture { registry, session, .. } = fixture;
    match zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params)) {
        Err(zen_core::DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn only_a_person_changes_what_the_log_records() {
    let mut fixture = Fixture::new();
    let wanted = json!({ "categories": ["rustls:trace"] });
    assert_eq!(agent_code(&mut fixture, "diagnostics.logCategories.set", wanted.clone()), "agentNotAllowed");
    assert_eq!(fixture.run("diagnostics.logCategories.get", json!({}))["categories"], json!([]));
    fixture.run("diagnostics.logCategories.set", wanted);
    assert_eq!(fixture.run("diagnostics.logCategories.get", json!({}))["categories"], json!(["rustls:trace"]));
}

#[test]
fn an_agent_writes_the_archive_only_to_a_new_file_of_the_project_not_hidden_from_it() {
    let mut fixture = Fixture::new();
    let project = fixture.dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("kept.txt"), "mine").unwrap();
    fixture.run("file.project.open", json!({ "path": project }));
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret/**".to_owned()]);
    let collect = |path: PathBuf| json!({ "path": path, "noReveal": true, "wait": true });
    let outside = fixture.dir.path().join("outside.zip");
    for path in [outside.clone(), project.join("../up.zip"), project.join("kept.txt")] {
        assert_eq!(
            agent_code(&mut fixture, "diagnostics.logs.collect", collect(path.clone())),
            "agentNotAllowed",
            "{path:?}"
        );
    }
    assert_eq!(agent_code(&mut fixture, "diagnostics.logs.collect", collect(project.join("secret/a.zip"))), "redacted");
    assert!(!outside.exists() && !project.join("secret/a.zip").exists());
    assert_eq!(std::fs::read_to_string(project.join("kept.txt")).unwrap(), "mine");

    let Fixture { registry, session, .. } = &mut fixture;
    let inside = zen_core::macros::as_agent(session, |session| {
        registry.dispatch(
            session,
            "diagnostics.logs.collect",
            json!({ "path": "report.zip", "noReveal": true, "wait": true }),
        )
    });
    assert!(inside.is_ok(), "{inside:?}");
    assert!(project.join("report.zip").is_file(), "a relative path is the project's");
    fixture.run("diagnostics.logs.collect", collect(outside.clone()));
    assert!(outside.is_file(), "a person chooses any file");
}

#[test]
fn settings_lose_more_kinds_of_secret_and_the_credentials_of_a_url() {
    let mut settings = json!({
        "proxyUrl": "http://me:hunter2@proxy:8080",
        "deployPrivateKey": "k",
        "sessionCookie": "c",
        "kept": "http://proxy:8080",
    });
    hide_secrets(&mut settings);
    assert_eq!(settings["proxyUrl"], "http://***@proxy:8080");
    assert_eq!(settings["deployPrivateKey"], HIDDEN);
    assert_eq!(settings["sessionCookie"], HIDDEN);
    assert_eq!(settings["kept"], "http://proxy:8080");
}

#[cfg(unix)]
#[test]
fn the_archive_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new();
    let path = fixture.dir.path().join("report.zip");
    fixture.run("diagnostics.logs.collect", json!({ "path": path, "noReveal": true, "wait": true }));
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::language::testing::{Fixture, fixture};

#[derive(Default)]
struct FakeTools {
    found: Mutex<BTreeMap<String, PathBuf>>,
    install: Mutex<Option<String>>,
    tool_missing: Mutex<bool>,
    prefixes: Mutex<Vec<PathBuf>>,
    packages: Mutex<Vec<String>>,
}

impl ServerTools for FakeTools {
    fn locate(&self, server: &ServerDeclaration) -> Option<PathBuf> {
        let found = self.found.lock().unwrap().get(&server.id).cloned();
        found.or_else(|| {
            let path = PathBuf::from(&server.command);
            (path.is_absolute() && path.is_file()).then_some(path)
        })
    }

    fn environment(&self, _: &ServerDeclaration) -> Vec<(OsString, OsString)> {
        Vec::new()
    }

    fn install_command(&self, server: &ServerDeclaration) -> Result<std::process::Command, NotInstallable> {
        if *self.tool_missing.lock().unwrap() {
            return Err(NotInstallable::ToolMissing(InstallKind::Go));
        }
        let script = self
            .install
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| NotInstallable::Manual(format!("no installer for {}", server.id)))?;
        let mut command = std::process::Command::new("sh");
        command.args(["-c", &script]);
        Ok(command)
    }

    fn install_command_into(
        &self,
        server: &ServerDeclaration,
        prefix: &Path,
    ) -> Result<std::process::Command, NotInstallable> {
        self.prefixes.lock().unwrap().push(prefix.to_owned());
        self.packages
            .lock()
            .unwrap()
            .push(server.install.as_ref().map(|recipe| recipe.package.clone()).unwrap_or_default());
        self.install_command(server)
    }
}

fn declaration(id: &str, extensions: &[&str], command: &str) -> ServerDeclaration {
    ServerDeclaration {
        id: id.to_owned(),
        name: format!("{id} server"),
        languages: vec![id.to_owned()],
        extensions: extensions.iter().map(|extension| (*extension).to_owned()).collect(),
        names: Vec::new(),
        watches: Vec::new(),
        command: command.to_owned(),
        args: vec!["--stdio".to_owned()],
        version_args: Vec::new(),
        install: Some(InstallRecipe {
            kind: InstallKind::Npm,
            url: String::new(),
            sha256: String::new(),
            files: Vec::new(),
            package: format!("{id}-server"),
            hint: String::new(),
            platforms: Default::default(),
        }),
        initialization_options: Value::Null,
        inlay_hints: None,
        language_ids: Default::default(),
        workspace_symbols: true,
        optimize_imports: Vec::new(),
        alongside: false,
        formats: false,
        fix_on_save: None,
        settings: Default::default(),
        from_project: false,
        when: None,
        power_save: None,
        java: None,
        notice: None,
        virtual_documents: None,
        generate: None,
        inspections: Vec::new(),
        inspection_settings: None,
        suppression: None,
        diagnostics_settle_ms: 0,
        schema_support: None,
    }
}

fn with_registry(files: &[(&str, &str)]) -> (Fixture, Arc<FakeTools>, tempfile::TempDir) {
    let (fixture, tools, data) = in_safe_mode(files);
    crate::trust::trust_in_tests(&crate::build_tools::BuildEnv::global(), &fixture.path(""));
    (fixture, tools, data)
}

fn in_safe_mode(files: &[(&str, &str)]) -> (Fixture, Arc<FakeTools>, tempfile::TempDir) {
    let mut fixture = fixture(files);
    let data = tempfile::tempdir().unwrap();
    let tools = Arc::new(FakeTools::default());
    let registry = LanguageServers::load(Some(data.path().join("language-servers.json")), tools.clone());
    registry.declare([declaration("yaml", &["yaml", "yml"], "yaml-ls"), declaration("toml", &["toml"], "toml-ls")]);
    fixture.session.set_state(registry);
    (fixture, tools, data)
}

fn list(fixture: &mut Fixture) -> Vec<Value> {
    fixture.dispatch("lsp.servers.list", json!({})).unwrap()["servers"].as_array().unwrap().clone()
}

fn server(fixture: &mut Fixture, id: &str) -> Value {
    list(fixture).into_iter().find(|server| server["id"] == id).unwrap()
}

fn wait_for(fixture: &mut Fixture, id: &str, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let server = server(fixture, id);
        if done(&server) || Instant::now() > deadline {
            return server;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn settings_of_a_server_no_plug_in_declares_any_more_break_nothing() {
    let mut fixture = fixture(&[("Dockerfile", "FROM alpine\n"), ("app.yaml", "a: 1\n")]);
    let data = tempfile::tempdir().unwrap();
    let file = data.path().join("language-servers.json");
    let old = json!({
        "dismissed": ["docker-langserver"],
        "servers": { "docker-langserver": { "path": "/old/bin/docker-langserver", "args": ["--stdio"] } },
        "chosen": { "dockerfile": "docker-langserver" }
    });
    std::fs::write(&file, old.to_string()).unwrap();
    std::fs::create_dir_all(data.path().join("language-servers/docker-langserver/node_modules")).unwrap();
    let registry = LanguageServers::load(Some(file), Arc::new(FakeTools::default()));
    registry.declare([declaration("yaml", &["yaml", "yml"], "yaml-ls")]);
    fixture.session.set_state(registry);
    let ids: Vec<String> = list(&mut fixture).iter().map(|server| server["id"].as_str().unwrap().to_owned()).collect();
    assert_eq!(ids, ["go", "yaml"]);
    fixture.dispatch("file.document.open", json!({ "path": fixture.path("Dockerfile") })).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let problems = fixture.dispatch("problems.file.list", json!({ "path": fixture.path("Dockerfile") })).unwrap();
    assert_ne!(problems["reason"], "serverMissing", "{problems}");
}

#[test]
fn every_server_is_listed_with_its_state() {
    let (mut fixture, tools, _data) =
        with_registry(&[("main.go", "package main\n"), ("config/app.yaml", "a: 1\n"), ("node_modules/x.toml", "")]);
    tools.found.lock().unwrap().insert("toml".to_owned(), PathBuf::from("/bin/toml-ls"));
    let servers = list(&mut fixture);
    let ids: Vec<&str> = servers.iter().map(|server| server["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["go", "yaml", "toml"]);
    let yaml = &servers[1];
    assert_eq!(yaml["state"], "missing");
    assert_eq!(yaml["name"], "yaml server");
    assert_eq!(yaml["languages"], json!(["yaml"]));
    assert_eq!(yaml["args"], json!(["--stdio"]));
    assert_eq!(yaml["installable"], false, "no installer here");
    assert_eq!(yaml["dismissed"], false);
    assert_eq!(servers[2]["state"], "notStarted");
    assert_eq!(servers[2]["path"], "/bin/toml-ls");
    assert_eq!(servers[0]["state"], "notStarted");
    let yaml = wait_for(&mut fixture, "yaml", |server| server["needed"] == true);
    assert_eq!(yaml["needed"], true, "the project has a YAML file");
    assert_eq!(server(&mut fixture, "toml")["needed"], false, "node_modules is not looked at");
    assert_eq!(server(&mut fixture, "go")["needed"], true);
    fixture.open("main.go");
    let go = wait_for(&mut fixture, "go", |server| server["state"] == "running");
    assert_eq!(go["state"], "running");
    assert_eq!(go["files"], 1);
    assert!(fixture.dispatch("lsp.server.restart", json!({ "id": "nothing" })).is_err());
}

#[test]
fn opening_a_file_makes_its_server_needed() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n")]);
    std::fs::write(fixture.path("late.toml"), "a = 1\n").unwrap();
    assert_eq!(server(&mut fixture, "toml")["needed"], false);
    let deadline = Instant::now() + Duration::from_secs(60);
    while server(&mut fixture, "toml")["needed"] == false && Instant::now() < deadline {
        fixture.dispatch("file.document.open", json!({ "path": fixture.path("late.toml") })).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }
    assert_eq!(server(&mut fixture, "toml")["needed"], true);
    assert_eq!(server(&mut fixture, "toml")["state"], "missing", "a missing server is not started");
}

#[test]
fn stop_keeps_a_server_stopped_until_restart() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n")]);
    fixture.open("main.go");
    fixture.dispatch("lsp.server.stop", json!({ "id": "go" })).unwrap();
    for _ in 0..20 {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }
    assert_eq!(server(&mut fixture, "go")["state"], "stopped");
    assert_eq!(fixture.dispatch("language.servers.status", json!({})).unwrap()["go"]["state"], "stopped");
    assert_eq!(fixture.server.launches(), 1);
    fixture.dispatch("lsp.server.restart", json!({ "id": "go" })).unwrap();
    let go = wait_for(&mut fixture, "go", |server| server["state"] == "running");
    assert_eq!(go["state"], "running");
    assert_eq!(fixture.server.launches(), 2);
}

#[test]
fn the_log_has_the_trace_and_reads_from_a_line() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n")]);
    fixture.open("main.go");
    let deadline = Instant::now() + Duration::from_secs(60);
    let log = loop {
        let log = fixture.dispatch("lsp.server.log", json!({ "id": "go" })).unwrap();
        let sent = log["lines"].as_array().unwrap().iter().any(|line| line == "--> notification textDocument/didOpen");
        if sent || Instant::now() >= deadline {
            break log;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let lines: Vec<&str> = log["lines"].as_array().unwrap().iter().map(|line| line.as_str().unwrap()).collect();
    assert!(lines.iter().any(|line| line.starts_with("--> request initialize")), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("<-- response")), "{lines:?}");
    assert!(lines.contains(&"--> notification textDocument/didOpen"), "{lines:?}");
    let next = log["next"].as_u64().unwrap();
    assert_eq!(next, lines.len() as u64);
    let later = fixture.dispatch("lsp.server.log", json!({ "id": "go", "since": next })).unwrap();
    assert!(later["lines"].as_array().unwrap().is_empty());
    assert!(fixture.dispatch("lsp.server.log", json!({ "id": "nothing" })).is_err());
}

#[test]
fn dismissal_and_configuration_are_saved() {
    let (mut fixture, _tools, data) = with_registry(&[("main.go", "package main\n")]);
    fixture.dispatch("lsp.server.dismiss", json!({ "id": "yaml" })).unwrap();
    assert_eq!(server(&mut fixture, "yaml")["dismissed"], true);
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "path": "/opt/ls/yaml", "args": ["a"] })).unwrap();
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "args": ["b", "c"] })).unwrap();
    let yaml = server(&mut fixture, "yaml");
    assert_eq!((yaml["path"].clone(), yaml["args"].clone()), (json!("/opt/ls/yaml"), json!(["b", "c"])));
    assert_eq!(yaml["state"], "missing", "the configured program does not exist");
    let file = data.path().join("language-servers.json");
    let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        saved,
        json!({ "dismissed": ["yaml"], "servers": { "yaml": { "path": "/opt/ls/yaml", "args": ["b", "c"] } } })
    );
    let again = LanguageServers::load(Some(file.clone()), Arc::new(FakeTools::default()));
    assert!(again.is_dismissed("yaml"));
    assert_eq!(again.configured("yaml").args, Some(vec!["b".to_owned(), "c".to_owned()]));
    fixture.dispatch("lsp.server.undismiss", json!({ "id": "yaml" })).unwrap();
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "path": null, "args": null })).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved, json!({ "dismissed": [], "servers": {} }));
    assert_eq!(server(&mut fixture, "yaml")["args"], json!(["--stdio"]));
    assert!(fixture.dispatch("lsp.server.configure", json!({ "id": "go", "args": [] })).is_err(), "a spec");
    assert!(fixture.dispatch("lsp.server.dismiss", json!({ "id": "nothing" })).is_err());
}

#[test]
fn install_needs_an_installer_and_never_runs_on_its_own() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n"), ("a.yaml", "")]);
    for _ in 0..3 {
        list(&mut fixture);
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    assert_eq!(registry.install_state("yaml"), None);
    let error = fixture.dispatch("lsp.server.install", json!({ "id": "yaml" })).unwrap_err();
    assert!(error.to_string().contains("no installer for yaml"), "{error}");
    assert!(fixture.dispatch("lsp.server.install", json!({ "id": "go" })).is_err(), "a spec cannot be installed");
}

#[cfg(unix)]
#[test]
fn an_installation_reports_its_progress_and_finds_the_server() {
    let (mut fixture, tools, data) = with_registry(&[("main.go", "package main\n")]);
    let program = data.path().join("bin/yaml-ls");
    *tools.install.lock().unwrap() = Some(format!(
        "echo fetching yaml-ls >&2; mkdir -p '{folder}'; touch '{program}'",
        folder = program.parent().unwrap().display(),
        program = program.display()
    ));
    let mut declared = declaration("found", &["found"], &program.to_string_lossy());
    declared.install = Some(InstallRecipe {
        kind: InstallKind::Npm,
        url: String::new(),
        sha256: String::new(),
        files: Vec::new(),
        package: "found".into(),
        hint: String::new(),
        platforms: Default::default(),
    });
    fixture.session.state::<LanguageServers>().unwrap().declare([declared]);
    assert_eq!(server(&mut fixture, "found")["installable"], true);
    assert_eq!(server(&mut fixture, "found")["state"], "missing");
    let started = fixture.dispatch("lsp.server.install", json!({ "id": "found" })).unwrap();
    assert_eq!(started, json!({ "started": true }));
    let found = wait_for(&mut fixture, "found", |server| server["installing"] == false);
    assert_eq!(found["state"], "notStarted", "{found}");
    assert_eq!(found["path"], program.to_string_lossy().as_ref());
    let log = fixture.dispatch("lsp.server.log", json!({ "id": "found" })).unwrap();
    assert!(log["lines"].as_array().unwrap().iter().any(|line| line == "fetching yaml-ls"), "{log}");
}

#[cfg(unix)]
#[test]
fn a_failed_installation_says_why() {
    let (mut fixture, tools, _data) = with_registry(&[("main.go", "package main\n")]);
    *tools.install.lock().unwrap() = Some("echo 'npm ERR! 404 not found' >&2; exit 1".to_owned());
    fixture.dispatch("lsp.server.install", json!({ "id": "yaml" })).unwrap();
    let yaml = wait_for(&mut fixture, "yaml", |server| server["installing"] == false);
    assert_eq!(yaml["state"], "missing");
    assert!(yaml["message"].is_string(), "{yaml}");
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    assert_eq!(registry.install_state("yaml"), Some(Install::Failed("npm ERR! 404 not found".to_owned())));
}

#[cfg(unix)]
#[test]
fn a_declared_server_starts_with_its_program_and_its_errors_are_logged() {
    let (mut fixture, _tools, data) = with_registry(&[("main.go", "package main\n"), ("a.yaml", "a: 1\n")]);
    let script = data.path().join("broken-ls");
    std::fs::write(&script, "#!/bin/sh\necho \"broken server started with $1\" >&2\nexit 3\n").unwrap();
    make_executable(&script);
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "path": script })).unwrap();
    fixture.dispatch("file.document.open", json!({ "path": fixture.path("a.yaml") })).unwrap();
    let yaml = wait_for(&mut fixture, "yaml", |server| server["state"] == "failed");
    assert_eq!(yaml["state"], "failed", "{yaml}");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let log = fixture.dispatch("lsp.server.log", json!({ "id": "yaml" })).unwrap();
        let lines: Vec<String> =
            log["lines"].as_array().unwrap().iter().map(|line| line.as_str().unwrap().to_owned()).collect();
        if lines.iter().any(|line| line == "broken server started with --stdio") {
            assert!(lines[0].ends_with("broken-ls --stdio"), "{lines:?}");
            break;
        }
        assert!(Instant::now() < deadline, "{lines:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn an_open_file_of_a_project_not_trusted_says_why_its_server_does_not_run() {
    let (mut fixture, _tools, data) = in_safe_mode(&[("a.yaml", "a: 1\n")]);
    let script = data.path().join("yaml-ls");
    std::fs::write(&script, "#!/bin/sh\nexit 3\n").unwrap();
    make_executable(&script);
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "path": script })).unwrap();
    fixture.dispatch("file.document.open", json!({ "path": fixture.path("a.yaml") })).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let problems = fixture.dispatch("problems.file.list", json!({ "path": fixture.path("a.yaml") })).unwrap();
    assert_eq!(problems["reason"], "notTrusted", "{problems}");
    let message = problems["message"].as_str().unwrap();
    assert!(message.contains("Trust Project") && !message.contains("open one of its files"), "{message}");
}

#[cfg(unix)]
#[test]
fn safe_mode_starts_no_language_server_and_trust_starts_it_without_reopening() {
    let (mut fixture, _tools, data) = in_safe_mode(&[("a.yaml", "a: 1\n")]);
    let marker = data.path().join("started");
    let script = data.path().join("yaml-ls");
    std::fs::write(&script, format!("#!/bin/sh\ntouch {}\nexit 3\n", marker.display())).unwrap();
    make_executable(&script);
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "path": script })).unwrap();
    fixture.dispatch("file.document.open", json!({ "path": fixture.path("a.yaml") })).unwrap();
    for _ in 0..20 {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }
    assert_ne!(server(&mut fixture, "yaml")["state"], "failed");
    assert!(!marker.exists(), "the program did not start");

    crate::trust::trust_in_tests(&crate::build_tools::BuildEnv::global(), &fixture.path(""));
    let yaml = wait_for(&mut fixture, "yaml", |server| server["state"] == "failed");
    assert_eq!(yaml["state"], "failed", "{yaml}");
    assert!(marker.exists(), "trust started it in the same session");
}

#[cfg(unix)]
#[test]
fn a_server_launched_for_an_untrusted_project_gets_gotoolchain_local_and_a_trusted_one_does_not() {
    let (fixture, _tools, data) = in_safe_mode(&[("a.yaml", "a: 1\n")]);
    let seen = data.path().join("seen");
    let script = data.path().join("yaml-ls");
    std::fs::write(&script, format!("#!/bin/sh\nprintf '%s' \"$GOTOOLCHAIN\" > {}\nexit 3\n", seen.display())).unwrap();
    make_executable(&script);
    let root = fixture.path("");
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    registry.configure("yaml", Some(Some(script)), None, None).unwrap();
    let launch = |registry: &LanguageServers| {
        let launcher = registry.launcher("yaml", &root).unwrap();
        let _ = zen_lsp::Client::start_logged(launcher, &root, Value::Null, None, Default::default());
    };
    launch(&registry);
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "local");
    crate::trust::trust_in_tests(&crate::build_tools::BuildEnv::global(), &root);
    launch(&registry);
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "");
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_project_scan_skips_hidden_and_build_folders() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["src/a.rs", ".git/x.yaml", "target/b.toml", "Dockerfile", "deep/er/c.Go"] {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }
    let files = scan(dir.path());
    let has = |server: ServerDeclaration| files.has_files_of(&server);
    assert!(has(declaration("go", &["go"], "")) && has(declaration("rust", &["rs"], "")));
    assert!(!has(declaration("yaml", &["yaml"], "")) && !has(declaration("toml", &["toml"], "")));
    let docker = ServerDeclaration { names: vec!["Dockerfile".to_owned()], ..declaration("docker", &[], "") };
    assert!(has(docker));
}

#[test]
fn a_missing_installer_is_named_with_where_to_get_it() {
    let (mut fixture, tools, _data) = with_registry(&[("main.go", "package main\n"), ("a.yaml", "")]);
    *tools.tool_missing.lock().unwrap() = true;
    let yaml = server(&mut fixture, "yaml");
    assert_eq!(yaml["installable"], false);
    assert_eq!(yaml["installer"], json!({ "tool": "go", "download": "https://go.dev/dl/" }));
    assert_eq!(yaml["message"], json!(t!("workbench-lsp-tool-missing", tool = "go", server = "yaml server")));
    let error = fixture.dispatch("lsp.server.install", json!({ "id": "yaml" })).unwrap_err();
    assert!(error.to_string().contains(yaml["message"].as_str().unwrap()), "{error}");
    *tools.tool_missing.lock().unwrap() = false;
    tools.found.lock().unwrap().insert("yaml".to_owned(), PathBuf::from("/bin/yaml-ls"));
    fixture.session.state::<LanguageServers>().unwrap().refresh();
    assert!(server(&mut fixture, "yaml").get("installer").is_none());
}

#[test]
fn a_server_turned_off_does_not_start_and_is_stopped() {
    let (mut fixture, _tools, data) = with_registry(&[("main.go", "package main\n")]);
    fixture.open("main.go");
    assert_eq!(server(&mut fixture, "go")["state"], "running");
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "enabled": false })).unwrap();
    let yaml = server(&mut fixture, "yaml");
    assert_eq!((yaml["state"].clone(), yaml["enabled"].clone()), (json!("disabled"), json!(false)));
    let saved: Value =
        serde_json::from_slice(&std::fs::read(data.path().join("language-servers.json")).unwrap()).unwrap();
    assert_eq!(saved["servers"]["yaml"], json!({ "disabled": true }));
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "enabled": true })).unwrap();
    assert_eq!(server(&mut fixture, "yaml")["state"], "missing");
    let saved: Value =
        serde_json::from_slice(&std::fs::read(data.path().join("language-servers.json")).unwrap()).unwrap();
    assert_eq!(saved["servers"], json!({}));
}

#[cfg(unix)]
#[test]
fn a_server_that_fails_to_start_is_not_started_again_until_something_changes() {
    let (mut fixture, _tools, data) = with_registry(&[("main.go", "package main\n"), ("a.yaml", "a: 1\n")]);
    let script = data.path().join("broken-ls");
    let count = data.path().join("starts");
    std::fs::write(&script, format!("#!/bin/sh\necho x >> '{}'\nexit 3\n", count.display())).unwrap();
    make_executable(&script);
    fixture.dispatch("lsp.server.configure", json!({ "id": "yaml", "path": script })).unwrap();
    fixture.dispatch("file.document.open", json!({ "path": fixture.path("a.yaml") })).unwrap();
    assert_eq!(wait_for(&mut fixture, "yaml", |server| server["state"] == "failed")["state"], "failed");
    for _ in 0..50 {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        list(&mut fixture);
    }
    let starts = || std::fs::read_to_string(&count).unwrap_or_default().lines().count();
    assert_eq!(starts(), 1, "a failed server is not retried in a loop");
    fixture.dispatch("lsp.server.configure", json!({ "id": "toml", "args": ["x"] })).unwrap();
    for _ in 0..20 {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }
    assert_eq!(starts(), 1, "another server's change does not retry it");
    assert_eq!(server(&mut fixture, "yaml")["state"], "failed");
    fixture.dispatch("lsp.server.restart", json!({ "id": "yaml" })).unwrap();
    wait_for(&mut fixture, "yaml", |server| server["state"] == "failed");
    let deadline = Instant::now() + Duration::from_secs(60);
    while starts() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(starts(), 2, "restart starts it again");
}

#[test]
fn an_empty_path_goes_back_to_the_program_found() {
    let (mut fixture, tools, _data) = with_registry(&[("main.go", "package main\n")]);
    tools.found.lock().unwrap().insert("toml".to_owned(), PathBuf::from("/bin/toml-ls"));
    fixture.dispatch("lsp.server.configure", json!({ "id": "toml", "path": "/opt/toml-ls" })).unwrap();
    assert_eq!(server(&mut fixture, "toml")["path"], "/opt/toml-ls");
    fixture.dispatch("lsp.server.configure", json!({ "id": "toml", "path": "" })).unwrap();
    assert_eq!(server(&mut fixture, "toml")["path"], "/bin/toml-ls");
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    assert_eq!(registry.configured("toml"), Configured::default());
}

#[test]
fn a_servers_work_is_a_task_with_its_percentage_and_message_cancelled_when_it_allows() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n")]);
    fixture.server.publish_on("textDocument/didOpen", "$/progress", |_| {
        json!({ "token": 7, "value": {
            "kind": "begin", "title": "Indexing", "message": "12/40 (core)", "percentage": 30, "cancellable": true,
        } })
    });
    fixture.server.publish_on(
        "window/workDoneProgress/cancel",
        "$/progress",
        |_| json!({ "token": 7, "value": { "kind": "end" } }),
    );
    fixture.open("main.go");
    let _ = wait_for(&mut fixture, "go", |server| server["busy"] == true);
    let task = loop {
        let list = fixture.dispatch("tasks.background.list", json!({})).unwrap();
        let found = list["tasks"].as_array().unwrap().iter().find(|task| task["source"] == "language-server").cloned();
        match found {
            Some(task) => break task,
            None => {
                fixture.dispatch("language.servers.sync", json!({})).unwrap();
            }
        }
    };
    assert_eq!(task["title"], "go: Indexing");
    assert_eq!((&task["detail"], task["fraction"].as_f64()), (&json!("12/40 (core)"), Some(0.3f32 as f64)));
    assert_eq!(task["cancellable"], true);
    assert!(crate::exit::running_work(&fixture.session).is_empty(), "a server's work does not hold up exiting");

    fixture.dispatch("tasks.background.cancel", json!({ "id": task["id"] })).unwrap();
    let sent = fixture.server.wait_for("window/workDoneProgress/cancel", 1, std::time::Duration::from_secs(5));
    assert_eq!(sent[0], json!({ "token": 7 }));
    let ended = loop {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let log = fixture.dispatch("tasks.log.read", json!({ "id": task["id"] })).unwrap();
        if log["task"]["state"] != "running" {
            break log;
        }
    };
    assert_eq!(ended["task"]["state"], "cancelled");
}

#[test]
fn a_servers_work_leaves_no_finished_task_behind() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n")]);
    fixture.server.publish_on(
        "textDocument/didOpen",
        "$/progress",
        |_| json!({ "token": "index", "value": { "kind": "begin", "title": "Indexing" } }),
    );
    fixture.server.publish_on(
        "textDocument/didChange",
        "$/progress",
        |_| json!({ "token": "index", "value": { "kind": "end" } }),
    );
    fixture.open("main.go");
    let _ = wait_for(&mut fixture, "go", |server| server["busy"] == true);
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let list = fixture.dispatch("tasks.background.list", json!({})).unwrap();
    let tasks = list["tasks"].as_array().unwrap();
    let server = tasks.iter().find(|task| task["source"] == "language-server").unwrap();
    assert_eq!(server.get("cancellable"), None, "the server did not allow it");
    fixture.dispatch("editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    let _ = wait_for(&mut fixture, "go", |server| server["busy"] == false);
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let list = fixture.dispatch("tasks.background.list", json!({})).unwrap();
    assert!(list["tasks"].as_array().unwrap().iter().all(|task| task["source"] != "language-server"), "{list}");
}

#[test]
fn a_server_reporting_work_is_busy_until_it_ends() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n")]);
    fixture.server.publish_on(
        "textDocument/didOpen",
        "$/progress",
        |_| json!({ "token": "index", "value": { "kind": "begin", "title": "Indexing", "percentage": 10 } }),
    );
    fixture.server.publish_on(
        "textDocument/didChange",
        "$/progress",
        |_| json!({ "token": "index", "value": { "kind": "end" } }),
    );
    assert_eq!(server(&mut fixture, "go")["busy"], false);
    fixture.open("main.go");
    let busy = wait_for(&mut fixture, "go", |server| server["busy"] == true);
    assert_eq!(busy["busy"], true);
    assert_eq!(busy["progress"], json!([{ "title": "Indexing", "percentage": 10 }]));
    assert!(busy.get("readyAfterMs").is_none());
    fixture.dispatch("editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    let idle = wait_for(&mut fixture, "go", |server| server["busy"] == false);
    assert_eq!(idle["busy"], false);
    assert!(idle.get("progress").is_none());
    assert!(idle["readyAfterMs"].is_u64());
    assert_eq!(fixture.with(|languages, _| languages.settled()), 1);
}

#[test]
fn diagnostics_are_pulled_after_each_change_beside_the_published_ones() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n\nfunc main() {}\n")]);
    let mut capabilities = json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "diagnosticProvider": { "identifier": "fake", "interFileDependencies": true, "workspaceDiagnostics": false }
    });
    fixture.server.set_capabilities(capabilities.take());
    let range =
        |line: u32| json!({ "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 4 } });
    let pulled = json!({ "range": range(2), "severity": 1, "message": "pulled", "source": "fake" });
    let published = json!({ "range": range(0), "severity": 2, "message": "published", "source": "check" });
    let report = pulled.clone();
    fixture.server.on_with("textDocument/diagnostic", move |_| json!({ "kind": "full", "items": [report] }));
    let push = published.clone();
    fixture.server.publish_on(
        "textDocument/didOpen",
        "textDocument/publishDiagnostics",
        move |params| json!({ "uri": params["textDocument"]["uri"], "diagnostics": [push] }),
    );
    let path = fixture.path("main.go");
    fixture.open("main.go");
    let messages = |fixture: &mut Fixture| -> Vec<String> {
        let list = fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap();
        let problems = list.get("problems").cloned().unwrap_or(list);
        problems.as_array().unwrap().iter().map(|problem| problem["message"].as_str().unwrap().to_owned()).collect()
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while messages(&mut fixture).len() < 2 && Instant::now() < deadline {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(messages(&mut fixture), ["published", "pulled"]);
    let asked = fixture.server.requests("textDocument/diagnostic").len();
    fixture.dispatch("editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while fixture.server.requests("textDocument/diagnostic").len() == asked && Instant::now() < deadline {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..20 {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fixture.server.requests("textDocument/diagnostic").len(), asked + 1);
}

#[test]
fn pulled_diagnostics_the_server_cancelled_are_asked_again_and_the_file_is_not_checked_meanwhile() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n\nfunc main() {}\n")]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "diagnosticProvider": { "identifier": "fake", "interFileDependencies": true, "workspaceDiagnostics": false }
    }));
    fixture.server.on("textDocument/diagnostic", json!({ "kind": "full", "items": [] }));
    let path = fixture.path("main.go");
    fixture.open("main.go");
    let list = |fixture: &mut Fixture| fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap();
    let settle = |fixture: &mut Fixture, until: &dyn Fn(&Value) -> bool| -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            fixture.dispatch("language.servers.sync", json!({})).unwrap();
            let answer = list(fixture);
            if until(&answer) || Instant::now() > deadline {
                return answer;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    assert_eq!(settle(&mut fixture, &|answer| answer["checked"] == true), json!({ "checked": true, "problems": [] }));
    fixture.server.reply(
        "textDocument/diagnostic",
        zen_lsp::fake::Reply::Error {
            code: zen_lsp::connection::SERVER_CANCELLED,
            message: "server cancelled the request".to_owned(),
        },
    );
    let asked = fixture.server.requests("textDocument/diagnostic").len();
    fixture.dispatch("editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while fixture.server.requests("textDocument/diagnostic").len() == asked && Instant::now() < deadline {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..10 {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let answer = list(&mut fixture);
    assert_eq!((answer["checked"].clone(), answer["reason"].clone()), (json!(false), json!("checking")), "{answer}");
    let range = json!({ "start": { "line": 2, "character": 0 }, "end": { "line": 2, "character": 4 } });
    let pulled = json!({ "range": range, "severity": 1, "message": "pulled", "source": "fake" });
    fixture.server.on_with("textDocument/diagnostic", move |_| json!({ "kind": "full", "items": [pulled] }));
    let answer = settle(&mut fixture, &|answer| answer.is_array());
    assert_eq!(answer[0]["message"], "pulled", "{answer}");
}

fn coded(result: Result<Value, zen_core::DispatchError>) -> (String, Value) {
    match result {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            (error.code.unwrap_or_default(), Value::Object(error.data.unwrap_or_default()))
        }
        other => panic!("not a failure: {other:?}"),
    }
}

#[test]
fn a_missing_server_says_so_instead_of_empty_answers() {
    let (mut fixture, tools, _data) = with_registry(&[("main.go", "package main\n"), ("config/app.yaml", "a: 1\n")]);
    *tools.install.lock().unwrap() = Some("true".to_owned());
    let yaml = fixture.path("config/app.yaml");
    fixture.dispatch("file.document.open", json!({ "path": yaml })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let status = fixture.dispatch("language.servers.status", json!({})).unwrap();
        if status.get("yaml").is_some() || Instant::now() > deadline {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let install = json!({ "command": "lsp.server.install", "params": { "id": "yaml" }, "runs": "sh -c true" });
    assert_eq!(status["yaml"]["state"], "missing", "{status}");
    assert_eq!(status["yaml"]["install"], install, "{status}");
    let synced = fixture.dispatch("language.servers.sync", json!({})).unwrap();
    assert_eq!(synced["servers"]["yaml"]["state"], "missing", "{synced}");

    let problems = fixture.dispatch("problems.file.list", json!({ "path": "config/app.yaml" })).unwrap();
    assert_eq!(problems["problems"], json!([]), "{problems}");
    assert_eq!((&problems["reason"], &problems["server"]), (&json!("serverMissing"), &json!("yaml")), "{problems}");
    assert_eq!(problems["install"], install, "{problems}");

    let structure = fixture.dispatch("editor.structure.get", json!({ "path": "config/app.yaml" })).unwrap();
    assert_eq!((&structure["status"], &structure["reason"]), (&json!("unavailable"), &json!("serverMissing")));
    assert_eq!(structure["server"], "yaml", "{structure}");
    assert_eq!(structure["path"], json!(yaml), "a relative path is the project's");

    for (command, params) in [
        ("navigate.usages.find", json!({ "path": yaml, "offset": 0 })),
        ("navigate.declaration.goTo", json!({ "path": yaml, "offset": 0 })),
        ("refactor.rename.prepare", json!({ "path": yaml, "offset": 0 })),
    ] {
        let (code, data) = coded(fixture.dispatch(command, params));
        assert_eq!(code, "serverMissing", "{command}");
        assert_eq!((&data["server"], &data["install"]), (&json!("yaml"), &install), "{command}: {data}");
    }

    fixture.open("main.go");
    let go = fixture.dispatch("problems.file.list", json!({ "path": "main.go" })).unwrap();
    assert_eq!((&go["problems"], &go["reason"]), (&json!([]), &json!("notChecked")), "{go}");
}

#[test]
fn a_missing_server_is_told_beside_the_answers_of_the_servers_that_run() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n"), ("config/app.yaml", "a: 1\n")]);
    fixture.server.set_capabilities(json!({ "textDocumentSync": 1, "workspaceSymbolProvider": true }));
    fixture.server.on("workspace/symbol", json!([]));
    fixture.open("main.go");
    let deadline = Instant::now() + Duration::from_secs(60);
    let found = loop {
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "Config", "waitMs": 2000 })).unwrap();
        if found["reason"] != "scanning" && found["pending"] == false || Instant::now() > deadline {
            break found;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!((&found["reason"], &found["server"]), (&json!("serverMissing"), &json!("yaml")), "not noMatch: {found}");
    assert_eq!(found["symbols"], json!([]));
    let classes = fixture.dispatch("navigate.class.goTo", json!({ "query": "Config", "waitMs": 2000 })).unwrap();
    assert_eq!(classes["reason"], "serverMissing", "{classes}");
    let project = fixture.dispatch("problems.project.list", json!({})).unwrap();
    assert_eq!((&project["problems"], &project["reason"]), (&json!([]), &json!("serverMissing")), "{project}");
    assert_eq!(project["server"], "yaml", "{project}");
}

#[test]
fn a_missing_server_is_named_only_when_nothing_was_found() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n"), ("config/app.yaml", "a: 1\n")]);
    fixture.server.set_capabilities(json!({ "textDocumentSync": 1, "workspaceSymbolProvider": true }));
    let uri = zen_lsp::position::file_uri(&fixture.path("main.go"));
    fixture.server.on_with("workspace/symbol", move |params| {
        let name = params["query"].as_str().unwrap_or_default().to_owned();
        if name.starts_with("Zebra") {
            return json!([]);
        }
        let start = json!({ "line": 0, "character": 0 });
        json!([{ "name": name, "kind": 12, "location": { "uri": uri, "range": { "start": start, "end": start } } }])
    });
    fixture.open("main.go");
    let deadline = Instant::now() + Duration::from_secs(60);
    let search = |fixture: &mut Fixture, command: &str, query: &str| {
        fixture.dispatch(command, json!({ "query": query, "waitMs": 2000 })).unwrap()
    };
    let first = loop {
        let found = search(&mut fixture, "navigate.symbol.goTo", "Run");
        if found["reason"] != "scanning" && found["pending"] == false || Instant::now() > deadline {
            break found;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(first["symbols"][0]["name"], "Run");
    assert!(first.get("reason").is_none() && first.get("server").is_none(), "{first}");
    let nothing = search(&mut fixture, "navigate.symbol.goTo", "Zebra");
    assert_eq!((&nothing["reason"], &nothing["server"]), (&json!("serverMissing"), &json!("yaml")), "{nothing}");
}

#[test]
fn a_missing_server_of_configuration_files_is_never_named_for_symbols() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n"), ("Dockerfile", "FROM x\n")]);
    let docker = ServerDeclaration {
        names: vec!["Dockerfile".to_owned()],
        language_ids: Default::default(),
        workspace_symbols: false,
        optimize_imports: Vec::new(),
        alongside: false,
        formats: false,
        fix_on_save: None,
        settings: Default::default(),
        from_project: false,
        when: None,
        java: None,
        ..declaration("docker", &[], "docker-ls")
    };
    fixture.session.state::<LanguageServers>().unwrap().declare([docker]);
    fixture.server.set_capabilities(json!({ "textDocumentSync": 1, "workspaceSymbolProvider": true }));
    fixture.server.on_with("workspace/symbol", |_| json!([]));
    fixture.open("main.go");
    let deadline = Instant::now() + Duration::from_secs(60);
    let found = loop {
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "route", "waitMs": 2000 })).unwrap();
        if found["reason"] != "scanning" && found["pending"] == false || Instant::now() > deadline {
            break found;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(found["reason"], "noMatch", "{found}");
    assert!(found.get("server").is_none(), "{found}");
    let project = fixture.dispatch("problems.project.list", json!({})).unwrap();
    assert_eq!(project["server"], "docker", "problems still name it: {project}");
}

#[test]
fn file_problems_tell_a_clean_file_from_one_not_checked_yet() {
    let mut fixture = fixture(&[("main.go", "package main\n\nfunc main() {}\n"), ("other.go", "package main\n")]);
    fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", |params| {
        let document = &params["textDocument"];
        json!({ "uri": document["uri"], "version": document["version"], "diagnostics": [] })
    });
    let problems =
        |fixture: &mut Fixture, path: &str| fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap();
    fixture.open("main.go");
    let deadline = Instant::now() + Duration::from_secs(60);
    let clean = loop {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let clean = problems(&mut fixture, "main.go");
        if clean["checked"] == true || Instant::now() > deadline {
            break clean;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(clean, json!({ "problems": [], "checked": true }), "a clean file is checked: {clean}");

    let other = problems(&mut fixture, "other.go");
    assert_eq!((&other["problems"], &other["checked"]), (&json!([]), &json!(false)), "{other}");
    assert_eq!(other["reason"], "notChecked", "never opened, never checked: {other}");
    assert!(other["message"].as_str().is_some_and(|message| message.contains("file.document.open")), "{other}");

    fixture.dispatch("editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
    let edited = problems(&mut fixture, "main.go");
    assert_eq!((&edited["checked"], &edited["reason"]), (&json!(false), &json!("checking")), "{edited}");
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let sent = problems(&mut fixture, "main.go");
    assert_eq!(sent["reason"], "checking", "the change reached the server, its check has not come: {sent}");
}

#[test]
fn symbol_search_names_the_missing_server_when_none_can_answer() {
    let (mut fixture, tools, _data) = with_registry(&[("config/app.yaml", "a: 1\n")]);
    *tools.install.lock().unwrap() = Some("true".to_owned());
    fixture.session.state_mut::<Languages>().set_specs(Vec::new());
    let deadline = Instant::now() + Duration::from_secs(60);
    let found = loop {
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "app", "waitMs": 50 })).unwrap();
        if found["reason"] == "serverMissing" || Instant::now() > deadline {
            break found;
        }
    };
    let facts = (&found["reason"], &found["server"], &found["pending"]);
    assert_eq!(facts, (&json!("serverMissing"), &json!("yaml"), &json!(false)), "{found}");
    assert_eq!(found["install"]["command"], "lsp.server.install", "{found}");
    let classes = fixture.dispatch("navigate.class.goTo", json!({ "query": "App", "waitMs": 50 })).unwrap();
    assert_eq!(classes["reason"], "serverMissing", "{classes}");
}

#[test]
fn the_servers_a_project_needs_are_found_or_missing() {
    let (fixture, tools, _data) = with_registry(&[("config/app.yaml", "a: 1\n"), ("Cargo.toml", "")]);
    tools.found.lock().unwrap().insert("toml".to_owned(), PathBuf::from("/bin/toml-ls"));
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    let needed = registry.needed_by(&fixture.path(""));
    let ids: Vec<&str> = needed.iter().map(|server| server.id.as_str()).collect();
    assert_eq!(ids, ["yaml", "toml"]);
    assert_eq!(needed[0].program, None);
    assert_eq!(needed[0].install, Some(Err("no installer for yaml".to_owned())));
    assert_eq!((needed[1].program.as_deref(), &needed[1].install), (Some(Path::new("/bin/toml-ls")), &None));
    *tools.install.lock().unwrap() = Some("true".to_owned());
    assert_eq!(registry.needed_by(&fixture.path(""))[0].install, Some(Ok("sh -c true".to_owned())));
}

#[test]
fn options_naming_an_npm_package_get_its_folder_next_to_the_server() {
    let data = tempfile::tempdir().unwrap();
    let prefix = data.path().join("prefix");
    let tsdk = prefix.join("lib/node_modules/typescript/lib");
    std::fs::create_dir_all(&tsdk).unwrap();
    std::fs::write(tsdk.join("typescript.js"), "").unwrap();
    std::fs::create_dir_all(prefix.join("bin")).unwrap();
    let program = prefix.join("bin/vue-language-server");
    std::fs::write(&program, "").unwrap();
    let tools = Arc::new(FakeTools::default());
    tools.found.lock().unwrap().insert("vue".to_owned(), program);
    let registry = LanguageServers::load(None, tools);
    let mut vue = declaration("vue", &["vue"], "vue-language-server");
    vue.initialization_options = json!({ "typescript": { "tsdk": "${nodeModulesFolder:typescript/lib/typescript.js}" }, "vue": { "hybridMode": false } });
    registry.declare([vue]);
    let project = data.path().join("app");
    let options = registry.initialization_options("vue", &project);
    assert_eq!(options["typescript"]["tsdk"], tsdk.to_string_lossy().as_ref());
    assert_eq!(options["vue"]["hybridMode"], false);
}

#[cfg(unix)]
#[test]
fn npm_servers_install_into_the_ides_own_folder_and_are_found_there_first() {
    let (mut fixture, tools, data) = with_registry(&[("main.go", "package main\n")]);
    let prefix = data.path().join("language-servers/web");
    let program = prefix.join("node_modules/.bin/web-ls");
    *tools.install.lock().unwrap() = Some(format!(
        "test -d '{prefix}' || exit 7; mkdir -p '{bin}'; touch '{program}'",
        prefix = prefix.display(),
        bin = program.parent().unwrap().display(),
        program = program.display()
    ));
    let elsewhere = data.path().join("global/web-ls");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::write(&elsewhere, "").unwrap();
    tools.found.lock().unwrap().insert("web".to_owned(), elsewhere.clone());
    fixture.session.state::<LanguageServers>().unwrap().declare([declaration("web", &["web"], "web-ls")]);
    assert_eq!(
        server(&mut fixture, "web")["path"],
        elsewhere.to_string_lossy().as_ref(),
        "a global install still works"
    );
    assert_eq!(server(&mut fixture, "web")["source"], "path");
    assert_eq!(server(&mut fixture, "yaml")["source"], "missing");
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    assert_eq!(registry.own_prefix(&registry.declaration("web").unwrap()), Some(prefix.clone()));
    let manual = ServerDeclaration { install: None, ..registry.declaration("web").unwrap() };
    assert_eq!(registry.own_prefix(&manual), None, "only npm servers have a folder of the IDE's");
    fixture.dispatch("lsp.server.install", json!({ "id": "web" })).unwrap();
    let installed = wait_for(&mut fixture, "web", |server| server["installing"] == false);
    let prefixes = tools.prefixes.lock().unwrap().clone();
    assert!(prefixes.contains(&prefix), "{prefixes:?} {installed}");
    assert!(prefixes.iter().all(|asked| asked.parent() == prefix.parent()), "one folder per server: {prefixes:?}");
    assert_eq!(registry.install_state("web"), Some(Install::Done));
    assert_eq!(registry.executable("web"), Some(program.clone()), "the IDE's own install comes before PATH");
    assert_eq!(server(&mut fixture, "web")["source"], "local", "installed locally");
    let mine = data.path().join("mine/web-ls");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, "").unwrap();
    fixture.dispatch("lsp.server.configure", json!({ "id": "web", "path": mine })).unwrap();
    assert_eq!(registry.executable("web"), Some(mine), "the user's own path wins");
    assert_eq!(server(&mut fixture, "web")["source"], "custom");
}

fn contribution(server: &str, name: &str) -> ServerPlugin {
    ServerPlugin {
        server: server.to_owned(),
        name: name.to_owned(),
        package: format!("{name}@1.0.0"),
        languages: vec!["svelte".to_owned()],
        when: None,
    }
}

#[test]
fn contributed_plugins_reach_the_servers_options_once_installed_in_its_folder() {
    let data = tempfile::tempdir().unwrap();
    let registry =
        LanguageServers::load(Some(data.path().join("language-servers.json")), Arc::new(FakeTools::default()));
    let mut typescript = declaration("ts", &["ts"], "ts-ls");
    typescript.initialization_options = json!({ "tsserver": { "fallbackPath": "x" } });
    registry.declare([typescript]);
    assert_eq!(registry.plugins_stamp("ts"), 0);
    registry.contribute("svelte", [contribution("ts", "ts-svelte"), contribution("other", "ignored")]);
    assert_eq!(registry.plugins_stamp("ts"), 1, "the server restarts for its new plug-ins");
    assert_eq!(registry.server_plugins("ts"), [contribution("ts", "ts-svelte")]);
    assert_eq!(registry.missing_plugins("ts"), [contribution("ts", "ts-svelte")]);
    let project = data.path().join("project");
    assert!(registry.initialization_options("ts", &project).get("plugins").is_none(), "not installed yet");

    let folder = data.path().join("language-servers/ts");
    std::fs::create_dir_all(folder.join("node_modules/ts-svelte")).unwrap();
    std::fs::write(folder.join("node_modules/ts-svelte/package.json"), "{}").unwrap();
    assert!(registry.missing_plugins("ts").is_empty());
    let options = registry.initialization_options("ts", &project);
    assert_eq!(options["tsserver"]["fallbackPath"], "x", "the declared options stay");
    assert_eq!(
        options["plugins"],
        json!([{ "name": "ts-svelte", "location": folder.to_string_lossy(), "languages": ["svelte"] }])
    );

    registry.contribute("vue", [contribution("ts", "ts-vue")]);
    assert_eq!(registry.server_plugins("ts").len(), 2);
    registry.withdraw("svelte");
    assert_eq!(registry.server_plugins("ts"), [contribution("ts", "ts-vue")], "uninstalled, its plug-in goes");
    assert_eq!(registry.plugins_stamp("ts"), 3);
    assert!(registry.initialization_options("ts", &project).get("plugins").is_none());
}

#[cfg(unix)]
#[test]
fn installing_a_server_installs_the_plugins_contributed_to_it_and_a_found_one_gets_only_them() {
    let (mut fixture, tools, data) = with_registry(&[("main.go", "package main\n")]);
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    registry.declare([declaration("web", &["web"], "web-ls")]);
    registry.contribute("svelte", [contribution("web", "web-svelte")]);
    let prefix = data.path().join("language-servers/web");
    let program = prefix.join("node_modules/.bin/web-ls");
    let plugin = prefix.join("node_modules/web-svelte/package.json");
    *tools.install.lock().unwrap() = Some(format!(
        "mkdir -p '{bin}' '{plugin_folder}' && touch '{program}' && echo '{{}}' > '{plugin}'",
        bin = program.parent().unwrap().display(),
        plugin_folder = plugin.parent().unwrap().display(),
        program = program.display(),
        plugin = plugin.display()
    ));
    fixture.dispatch("lsp.server.install", json!({ "id": "web" })).unwrap();
    wait_for(&mut fixture, "web", |server| server["installing"] == false);
    assert_eq!(registry.install_state("web"), Some(Install::Done));
    let asked = tools.packages.lock().unwrap().clone();
    assert!(asked.contains(&"web-server web-svelte@1.0.0".to_owned()), "the server with its plug-ins: {asked:?}");
    assert!(registry.missing_plugins("web").is_empty());

    registry.contribute("vue", [contribution("web", "web-vue")]);
    let listed = server(&mut fixture, "web");
    assert_eq!(listed["missingPlugins"], json!(["web-vue"]), "{listed}");
    tools.packages.lock().unwrap().clear();
    let vue = prefix.join("node_modules/web-vue/package.json");
    *tools.install.lock().unwrap() = Some(format!(
        "mkdir -p '{folder}' && echo '{{}}' > '{vue}'",
        folder = vue.parent().unwrap().display(),
        vue = vue.display()
    ));
    let stamp = registry.plugins_stamp("web");
    fixture.dispatch("lsp.server.install", json!({ "id": "web" })).unwrap();
    wait_for(&mut fixture, "web", |server| server["installing"] == false);
    let asked = tools.packages.lock().unwrap().clone();
    assert!(asked.contains(&"web-vue@1.0.0".to_owned()), "only the plug-in: {asked:?}");
    assert!(!asked.iter().any(|package| package.contains("web-server ")), "not the server again: {asked:?}");
    assert!(registry.missing_plugins("web").is_empty());
    assert!(registry.plugins_stamp("web") > stamp, "the server restarts to load it");
    assert!(server(&mut fixture, "web").get("missingPlugins").is_none());
}

#[test]
fn a_running_server_starts_again_when_its_plugins_change() {
    let (mut fixture, _tools, _data) = with_registry(&[("main.go", "package main\n")]);
    fixture.open("main.go");
    assert_eq!(fixture.server.launches(), 1);
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    assert_eq!(fixture.server.launches(), 1, "nothing changed");
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    registry.contribute("svelte", [contribution("go", "go-svelte")]);
    let deadline = Instant::now() + Duration::from_secs(60);
    while fixture.server.launches() < 2 {
        assert!(Instant::now() < deadline, "the server did not start again");
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    registry.contribute("other", Vec::new());
    for _ in 0..5 {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }
    assert_eq!(fixture.server.launches(), 2, "a contribution to no server of it changes nothing");
}

#[test]
fn a_contributed_plugin_is_offered_only_in_the_projects_it_is_for() {
    let data = tempfile::tempdir().unwrap();
    let registry =
        LanguageServers::load(Some(data.path().join("language-servers.json")), Arc::new(FakeTools::default()));
    registry.declare([declaration("ts", &["ts"], "ts-ls")]);
    let marker = zen_project::support::ProjectMarkers { files: Vec::new(), packages: vec!["svelte".to_owned()] };
    let svelte = ServerPlugin { when: Some(marker), ..contribution("ts", "ts-svelte") };
    registry.contribute("svelte", [svelte.clone(), contribution("ts", "ts-any")]);
    let project = |manifest: &str| {
        let folder = data.path().join(format!("p{}", manifest.len()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("package.json"), manifest).unwrap();
        folder
    };
    let names = |plugins: Vec<ServerPlugin>| plugins.into_iter().map(|plugin| plugin.name).collect::<Vec<_>>();
    let app = project(r#"{ "devDependencies": { "svelte": "^5.0.0" } }"#);
    assert_eq!(names(registry.missing_plugins_for("ts", Some(&app))), ["ts-svelte", "ts-any"]);
    let plain = project(r#"{ "dependencies": { "lodash": "^4.0.0" } }"#);
    assert_eq!(names(registry.missing_plugins_for("ts", Some(&plain))), ["ts-any"], "not a Svelte project");
    assert_eq!(names(registry.missing_plugins_for("ts", None)), ["ts-any"]);
    assert_eq!(registry.missing_plugins("ts").len(), 2, "installing installs them all");
    assert!(svelte.applies(&app) && !svelte.applies(&plain));
}

#[test]
fn of_two_servers_for_a_language_the_chosen_one_starts_and_the_first_by_default() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let data = tempfile::tempdir().unwrap();
    let registry =
        LanguageServers::load(Some(data.path().join("language-servers.json")), Arc::new(FakeTools::default()));
    let mut vendor = declaration("kotlin-lsp", &["kt"], "kotlin-lsp");
    vendor.languages = vec!["kotlin".to_owned()];
    vendor.notice = Some(zen_lsp::servers::ServerNotice {
        text: [
            ("en".to_owned(), "the vendor's software".to_owned()),
            ("ru".to_owned(), "программа поставщика".to_owned()),
        ]
        .into_iter()
        .collect(),
        url: "https://github.com/Kotlin/kotlin-lsp".to_owned(),
    });
    let mut open = declaration("kotlin-language-server", &["kt"], "kotlin-language-server");
    open.languages = vec!["kotlin".to_owned()];
    registry.declare([vendor, open, declaration("yaml", &["yaml"], "yaml-ls")]);
    assert_eq!(registry.alternatives("kotlin-language-server"), ["kotlin-lsp", "kotlin-language-server"]);
    assert!(registry.alternatives("yaml").is_empty(), "a language with one server has no choice");
    assert!(registry.is_enabled("kotlin-lsp") && !registry.is_enabled("kotlin-language-server"), "the first declared");
    registry.choose("kotlin-language-server").unwrap();
    assert!(!registry.is_enabled("kotlin-lsp") && registry.is_enabled("kotlin-language-server"));
    assert_eq!(registry.alternatives("kotlin-lsp")[0], "kotlin-language-server", "the chosen one first");
    let again = LanguageServers::load(Some(data.path().join("language-servers.json")), Arc::new(FakeTools::default()));
    again.declare(registry.declarations());
    assert!(again.is_enabled("kotlin-language-server") && !again.is_enabled("kotlin-lsp"));
    again.configure("kotlin-language-server", None, None, Some(false)).unwrap();
    assert!(!again.is_enabled("kotlin-language-server") && !again.is_enabled("kotlin-lsp"));
    let notice = registry.declaration("kotlin-lsp").unwrap().notice.unwrap();
    assert_eq!(notice.text_in("ru-RU"), "программа поставщика");
    assert_eq!(notice.text_in("de"), "the vendor's software");
}

#[test]
fn choosing_a_server_is_a_command_and_the_list_says_the_alternatives() {
    let (mut fixture, _tools, _data) = with_registry(&[("a.kt", "fun main() {}\n")]);
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    let mut vendor = declaration("kotlin-lsp", &["kt"], "kotlin-lsp");
    vendor.languages = vec!["kotlin".to_owned()];
    let mut open = declaration("kotlin-language-server", &["kt"], "kotlin-language-server");
    open.languages = vec!["kotlin".to_owned()];
    registry.declare([vendor, open]);
    let listed = fixture.dispatch("lsp.servers.list", json!({})).unwrap();
    let kotlin =
        listed["servers"].as_array().unwrap().iter().find(|server| server["id"] == "kotlin-lsp").unwrap().clone();
    assert_eq!(kotlin["alternatives"], json!(["kotlin-lsp", "kotlin-language-server"]));
    fixture.dispatch("lsp.server.choose", json!({ "id": "kotlin-language-server" })).unwrap();
    let listed = fixture.dispatch("lsp.servers.list", json!({})).unwrap();
    let open = listed["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|server| server["id"] == "kotlin-language-server")
        .unwrap()
        .clone();
    assert_eq!(open["alternatives"][0], "kotlin-language-server");
    let error = fixture.dispatch("lsp.server.choose", json!({ "id": "nothing" }));
    assert!(error.is_err());
}

fn jdtls_declaration() -> ServerDeclaration {
    let mut jdtls = declaration("jdtls", &["java"], "jdtls-is-not-here");
    jdtls.name = "Eclipse JDT Language Server".to_owned();
    jdtls.languages = vec!["java".to_owned()];
    jdtls.install = Some(InstallRecipe {
        kind: InstallKind::Archive,
        url: "https://example.org/server.tar.gz".into(),
        sha256: "00".repeat(32),
        files: Vec::new(),
        package: String::new(),
        hint: String::new(),
        platforms: Default::default(),
    });
    jdtls
}

#[test]
fn a_reload_without_the_java_server_installed_says_so_and_offers_install() {
    let (mut fixture, _tools, _data) = with_registry(&[("pom.xml", "<project><artifactId>a</artifactId></project>")]);
    fixture.session.state::<LanguageServers>().unwrap().clone().declare([jdtls_declaration()]);
    let reloaded = fixture.dispatch("maven.project.reload", json!({ "dependencies": false })).unwrap();
    assert_eq!(reloaded["reimported"], false);
    let sync = fixture.dispatch("build.sync.get", json!({})).unwrap();
    let step = sync["syncs"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["title"] == "Import into the Java language server")
        .cloned()
        .unwrap();
    let message = &step["messages"][0];
    assert_eq!(
        message["text"],
        "Eclipse JDT Language Server is not installed, so the project is not imported into it."
    );
    assert_eq!(message["severity"], "warning");
    assert_eq!(message["fix"]["title"], "Install Eclipse JDT Language Server");
    assert_eq!(message["fix"]["command"], "lsp.server.install");
    assert_eq!(message["fix"]["params"], json!({ "id": "jdtls" }));
    assert_eq!(message["fix"]["reload"], false, "the install runs in the background");
}

#[test]
fn a_new_project_jdk_is_told_once() {
    let dir = tempfile::tempdir().unwrap();
    let registry = LanguageServers::load(None, Arc::new(FakeTools::default()));
    registry.declare([ServerDeclaration { languages: vec!["java".to_owned()], ..declaration("jdtls", &["java"], "") }]);
    assert_eq!(registry.project_jdk_change("jdtls", dir.path()), None, "never launched");
    let now = crate::jdk::project_home(dir.path());
    registry.inner().launched_jdks.insert("jdtls".to_owned(), now.clone());
    assert_eq!(registry.project_jdk_change("jdtls", dir.path()), None, "the same JDK");
    registry.inner().launched_jdks.insert("jdtls".to_owned(), Some(dir.path().join("older")));
    let notice = registry.project_jdk_change("jdtls", dir.path());
    assert_eq!(notice.is_some(), now.is_some());
    assert_eq!(registry.project_jdk_change("jdtls", dir.path()), None, "told once");
}

#[test]
fn a_server_run_with_java_says_which_java_it_needs_until_a_jdk_is_found() {
    let (mut fixture, _tools, data) = with_registry(&[("App.java", "class App {}")]);
    let registry = fixture.session.state::<LanguageServers>().unwrap().clone();
    let folder = data.path().join("language-servers/jdtls/plugins");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("launcher_1.jar"), "").unwrap();
    let mut jdtls = declaration("jdtls", &["java"], "${javaHome}/bin/java");
    jdtls.args = vec!["-jar".into(), "${glob:plugins/launcher_*.jar}".into()];
    jdtls.java = Some(99);
    jdtls.install = Some(InstallRecipe {
        kind: InstallKind::Archive,
        url: "https://example.org/server.tar.gz".into(),
        sha256: "00".repeat(32),
        files: Vec::new(),
        package: String::new(),
        hint: String::new(),
        platforms: Default::default(),
    });
    registry.declare([jdtls]);
    let found = server(&mut fixture, "jdtls");
    assert_eq!(found["source"], "local", "{found}");
    assert_eq!(found["javaNeeded"], 99, "no JDK 99 anywhere: {found}");
    assert!(registry.launcher("jdtls", Path::new("/project")).is_err());

    let jdk = data.path().join("jdk");
    std::fs::create_dir_all(jdk.join("bin")).unwrap();
    std::fs::write(jdk.join("bin/java"), "").unwrap();
    std::fs::write(jdk.join("release"), "JAVA_VERSION=\"99.0.1\"\n").unwrap();
    let home = jdk.clone();
    registry.set_java_finder(Arc::new(move |_, _| Some(home.clone())));
    let found = server(&mut fixture, "jdtls");
    assert!(found.get("javaNeeded").is_none(), "{found}");
    assert!(registry.launcher("jdtls", Path::new("/project")).is_ok());
}

#[test]
fn jdt_ls_starts_with_the_projects_lombok_as_its_agent_and_restarts_when_it_changes() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("shop");
    let repository = folder.path().join("m2");
    let jar = |version: &str| {
        let jar = repository.join(format!("org/projectlombok/lombok/{version}/lombok-{version}.jar"));
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(&jar, "jar").unwrap();
        jar
    };
    let pom = |version: &str| {
        format!(
            "<project><modelVersion>4.0.0</modelVersion><groupId>a</groupId><artifactId>a</artifactId>\
             <version>1</version><dependencies><dependency><groupId>org.projectlombok</groupId>\
             <artifactId>lombok</artifactId><version>{version}</version></dependency></dependencies></project>"
        )
    };
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("pom.xml"), pom("1.18.34")).unwrap();
    let first = jar("1.18.34");
    let env = crate::build_tools::BuildEnv::default();
    crate::trust::trust_in_tests(&env, &root);
    env.set_project_settings(
        crate::maven::settings::TOOL,
        &root,
        &crate::maven::settings::MavenSettings {
            local_repository: repository.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let data = folder.path().join("data");
    let plugins = data.join("language-servers/jdtls/plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("launcher_1.jar"), "").unwrap();
    let jdk = folder.path().join("jdk");
    std::fs::create_dir_all(jdk.join("bin")).unwrap();
    std::fs::write(jdk.join("bin/java"), "").unwrap();
    std::fs::write(jdk.join("release"), "JAVA_VERSION=\"25\"\n").unwrap();
    let registry = LanguageServers::load(Some(data.join("language-servers.json")), Arc::new(FakeTools::default()));
    registry.set_java_finder(Arc::new(move |_, _| Some(jdk.clone())));
    let mut jdtls = declaration("jdtls", &["java"], "${javaHome}/bin/java");
    jdtls.languages = vec!["java".to_owned()];
    jdtls.java = Some(25);
    jdtls.args =
        vec!["-Xmx1G".into(), processing::AGENTS.into(), "-jar".into(), "${glob:plugins/launcher_*.jar}".into()];
    jdtls.install = Some(InstallRecipe {
        kind: InstallKind::Archive,
        url: "https://example.org/server.tar.gz".into(),
        sha256: "00".repeat(32),
        files: Vec::new(),
        package: String::new(),
        hint: String::new(),
        platforms: Default::default(),
    });
    registry.declare([jdtls]);
    let (sender, reads) = std::sync::mpsc::channel();
    let sender = Mutex::new(sender);
    registry.set_java_launch_reader(Some(Arc::new(move |root: &Path| {
        let found = processing::launch(&env, root);
        sender.lock().unwrap().send(std::thread::current().id()).unwrap();
        found
    })));
    let read_elsewhere = || {
        let at = reads.recv_timeout(Duration::from_secs(30)).expect("a read in the background");
        assert_ne!(at, std::thread::current().id(), "the window's thread never reads build files");
        let deadline = Instant::now() + Duration::from_secs(30);
        while registry.inner().java_launches[root.as_path()].reading {
            assert!(Instant::now() < deadline, "the background read keeps its result");
            std::thread::yield_now();
        }
    };
    let change_once_kept = || {
        let started = std::time::Instant::now();
        loop {
            if let Some(change) = registry.java_launch_change("jdtls", &root) {
                return change;
            }
            assert!(started.elapsed() < Duration::from_secs(30), "the read result is never kept");
            std::thread::yield_now();
        }
    };
    let launcher = plugins.join("launcher_1.jar").display().to_string();
    let started = std::thread::scope(|scope| scope.spawn(|| registry.launcher("jdtls", &root).map(drop)).join());
    started.unwrap().unwrap();
    read_elsewhere();
    let (lines, _) = registry.log("jdtls").since(0);
    assert!(
        lines.last().unwrap().ends_with(&format!("-Xmx1G -javaagent:{} -jar {launcher}", first.display())),
        "{lines:?}"
    );
    assert_eq!(registry.java_launch_change("jdtls", &root), None);

    std::fs::write(root.join("pom.xml"), pom("1.18.36")).unwrap();
    jar("1.18.36");
    registry.forget_java_launches();
    assert_eq!(registry.java_launch_change("jdtls", &root), None, "only the last result, until the new one is read");
    read_elsewhere();
    assert_eq!(change_once_kept(), "jdtls server restarts with Lombok 1.18.36 as its agent.");
    assert_eq!(registry.java_launch_change("jdtls", &root), None, "told once");
    assert!(reads.try_recv().is_err(), "a result read moments ago is not read again");

    std::fs::write(root.join("pom.xml"), "<project><modelVersion>4.0.0</modelVersion></project>").unwrap();
    registry.forget_java_launches();
    assert_eq!(registry.java_launch_change("jdtls", &root), None);
    read_elsewhere();
    assert!(change_once_kept().contains("without Lombok"));
    let started = std::thread::scope(|scope| scope.spawn(|| registry.launcher("jdtls", &root).map(drop)).join());
    started.unwrap().unwrap();
    read_elsewhere();
    let (lines, _) = registry.log("jdtls").since(0);
    assert!(lines.last().unwrap().ends_with(&format!("-Xmx1G -jar {launcher}")), "no empty argument: {lines:?}");
}

#[test]
fn why_a_server_failed_is_told_in_the_language_of_whoever_asks() {
    zen_i18n::register(crate::LOCALES);
    let mut languages = Languages::default();
    let reason = zen_i18n::with_language("ru", || zen_i18n::Message::new("workbench-language-not-running"));
    languages.servers.insert("toml".to_owned(), Server::Failed { reason, launch: (None, Vec::new()) });
    let told =
        |languages: &Languages, language: &str| match zen_i18n::with_language(language, || languages.started_status())
            .remove("toml")
        {
            Some(crate::language::ServerStatus::Failed { reason }) => reason,
            other => panic!("{other:?}"),
        };
    assert_eq!(told(&languages, "en"), "The language server is not running.", "an agent reads English");
    assert_eq!(told(&languages, "ru"), "Языковой сервер не запущен.", "the window reads its own language");
}

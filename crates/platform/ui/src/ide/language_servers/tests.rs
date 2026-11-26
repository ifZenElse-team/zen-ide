use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Event, PointerButton, Pos2, ViewportId, ViewportInfo};
use zen_lsp::fake::FakeServer;
use zen_workbench::language::servers::{
    InstallKind, InstallRecipe, LanguageServers as ServerRegistry, NotInstallable, ServerDeclaration, ServerTools,
};
use zen_workbench::language::{Languages, ServerSpec};

use super::*;

#[derive(Default)]
struct FakeTools {
    found: Mutex<BTreeMap<String, PathBuf>>,
    install: Mutex<Option<String>>,
    tool_missing: Mutex<bool>,
}

impl ServerTools for FakeTools {
    fn locate(&self, server: &ServerDeclaration) -> Option<PathBuf> {
        let found = self.found.lock().unwrap().get(&server.id).cloned();
        found.or_else(|| {
            let path = PathBuf::from(&server.command);
            (path.is_absolute() && path.is_file()).then_some(path)
        })
    }

    fn environment(&self, _: &ServerDeclaration) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
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
        _prefix: &Path,
    ) -> Result<std::process::Command, NotInstallable> {
        self.install_command(server)
    }
}

fn declaration(id: &str, extensions: &[&str], command: &str) -> ServerDeclaration {
    ServerDeclaration {
        id: id.to_owned(),
        name: id.to_owned(),
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
            package: id.to_owned(),
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

struct Harness {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    servers: LanguageServers,
    tools: Arc<FakeTools>,
    fake: FakeServer,
    calls: Vec<(String, Value)>,
    actions: Vec<ServerAction>,
    now: f64,
    dir: tempfile::TempDir,
}

impl Harness {
    fn new(files: &[&str]) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        for name in files {
            let path = dir.path().join("project").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "a\n").unwrap();
        }
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path().join("project") })).unwrap();
        let fake = FakeServer::new();
        let launched = fake.clone();
        session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(move |_| Ok(launched.launcher())),
            initialization_options: json!({}),
        }]);
        let tools = Arc::new(FakeTools::default());
        let servers = ServerRegistry::load(Some(dir.path().join("language-servers.json")), tools.clone());
        servers.declare([declaration("yaml", &["yaml", "yml"], "yaml-ls"), declaration("toml", &["toml"], "toml-ls")]);
        session.set_state(servers);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut harness = Self {
            ctx,
            registry,
            session,
            servers: LanguageServers::default(),
            tools,
            fake,
            calls: Vec::new(),
            actions: Vec::new(),
            now: 0.0,
            dir,
        };
        harness.poll(true);
        harness
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join("project").join(name)
    }

    fn calls(&self, id: &str) -> Vec<Value> {
        self.calls.iter().filter(|(called, _)| called == id).map(|(_, params)| params.clone()).collect()
    }

    fn poll(&mut self, focused: bool) -> bool {
        let Self { registry, session, servers, now, calls, .. } = self;
        let mut inner = dispatcher(registry, session);
        let mut dispatch = |id: &str, params: Value| {
            calls.push((id.to_owned(), params.clone()));
            inner(id, params)
        };
        servers.poll(*now, focused, &mut dispatch)
    }

    fn act(&mut self, action: ServerAction) {
        let Self { registry, session, servers, calls, .. } = self;
        let mut inner = dispatcher(registry, session);
        let mut dispatch = |id: &str, params: Value| {
            calls.push((id.to_owned(), params.clone()));
            inner(id, params)
        };
        servers.act(action, &mut dispatch).unwrap();
    }

    fn sync(&mut self) {
        self.registry.dispatch(&mut self.session, "language.servers.sync", json!({})).unwrap();
    }

    fn open(&mut self, name: &str) {
        let path = self.path(name);
        self.registry.dispatch(&mut self.session, "file.document.open", json!({ "path": path })).unwrap();
        self.sync();
    }

    fn until(&mut self, what: &str, done: impl Fn(&[ServerInfo]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            self.sync();
            self.servers.invalidate();
            self.poll(true);
            if done(self.servers.servers()) {
                return;
            }
            assert!(Instant::now() < deadline, "{what}: {:?}", self.servers.servers());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn server(&self, id: &str) -> ServerInfo {
        self.servers.server(id).cloned().unwrap_or_else(|| panic!("no server {id}"))
    }

    fn frame_with(&mut self, events: Vec<Event>) {
        let mut input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
            ..Default::default()
        };
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { focused: Some(true), ..Default::default() });
        let Self { ctx, registry, session, servers, actions, now, calls, .. } = self;
        let mut output = ctx.run_ui(input, |ui| {
            let (_, action) = servers.widget(ui, &IdePalette::zen_dark(), 880.0, 586.0);
            actions.extend(action);
            let mut inner = dispatcher(registry, session);
            let mut dispatch = |id: &str, params: Value| {
                calls.push((id.to_owned(), params.clone()));
                inner(id, params)
            };
            servers.log_window(ui, &IdePalette::zen_dark(), *now, &mut dispatch);
        });
        output.textures_delta.clear();
    }

    fn click(&mut self, at: Pos2) {
        self.frame_with(vec![Event::PointerMoved(at)]);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame_with(vec![button(true)]);
        self.frame_with(vec![button(false)]);
        self.frame_with(Vec::new());
    }

    fn click_hit(&mut self, id: &str, action: &str) {
        let rect = self.servers.hit(id, action).unwrap_or_else(|| panic!("no {action} for {id:?}"));
        self.click(rect.center());
    }
}

#[cfg(unix)]
fn installer_script(program: &Path, wait: &str) -> String {
    format!(
        "echo fetching >&2; {wait} mkdir -p '{folder}'; touch '{program}'",
        folder = program.parent().unwrap().display(),
        program = program.display()
    )
}

#[test]
fn the_list_is_read_at_most_once_a_second_and_only_while_focused() {
    let mut harness = Harness::new(&["main.go"]);
    assert_eq!(harness.calls("lsp.servers.list").len(), 1);
    harness.now = 0.5;
    assert!(!harness.poll(true));
    harness.now = 2.0;
    assert!(!harness.poll(false));
    assert_eq!(harness.calls("lsp.servers.list").len(), 1);
    harness.poll(true);
    assert_eq!(harness.calls("lsp.servers.list").len(), 2);
    harness.open("main.go");
    harness.until("go runs", |servers| servers.iter().any(|server| server.id == "go" && server.state == "running"));
    assert_eq!(harness.server("go").files, 1);
}

#[test]
fn without_the_commands_there_are_no_servers() {
    let mut servers = LanguageServers::default();
    let mut dispatch = |_: &str, _: Value| -> Result<Value, String> { Err("unknown command".to_owned()) };
    servers.poll(0.0, true, &mut dispatch);
    assert!(servers.servers().is_empty());
    assert_eq!(servers.banner("main.go", 0.0), None);
}

#[test]
fn the_widget_opens_a_popup_whose_rows_act_on_their_server() {
    let mut harness = Harness::new(&["main.go"]);
    harness.open("main.go");
    harness.until("go runs", |servers| servers.iter().any(|server| server.id == "go" && server.state == "running"));
    harness.frame_with(Vec::new());
    harness.click_hit("", "widget");
    assert!(harness.servers.popup_open());
    assert!(harness.servers.hit("go", "stop").is_some());
    assert!(harness.servers.hit("go", "install").is_none());
    harness.click_hit("go", "restart");
    assert_eq!(harness.actions, vec![ServerAction::Restart("go".to_owned())]);
    assert!(!harness.servers.popup_open());
    let action = harness.actions.pop().unwrap();
    harness.act(action);
    assert_eq!(harness.calls("lsp.server.restart"), vec![json!({ "id": "go" })]);
    harness
        .until("go runs again", |servers| servers.iter().any(|server| server.id == "go" && server.state == "running"));
    assert_eq!(harness.fake.launches(), 2);
}

#[test]
fn a_missing_server_offers_install_and_no_stop() {
    let mut harness = Harness::new(&["main.go", "config/app.yaml"]);
    *harness.tools.install.lock().unwrap() = Some("true".to_owned());
    harness.tools.found.lock().unwrap().insert("toml".to_owned(), PathBuf::from("/bin/toml-ls"));
    harness.session.state::<ServerRegistry>().unwrap().refresh();
    harness.session.state::<ServerRegistry>().unwrap().declare([declaration("other", &["other"], "other-ls")]);
    harness.until("the project scan", |servers| servers.iter().any(|server| server.id == "yaml" && server.needed));
    assert_eq!(harness.server("toml").state, "notStarted");
    harness.frame_with(Vec::new());
    harness.click_hit("", "widget");
    assert!(harness.servers.hit("yaml", "install").is_some());
    assert!(harness.servers.hit("yaml", "stop").is_none());
    assert!(harness.servers.hit("yaml", "restart").is_none());
    assert!(harness.servers.hit("toml", "log").is_none(), "a server that is not needed and not started is not listed");
    assert!(harness.servers.hit("other", "log").is_none(), "a missing server the project does not need is not listed");
    harness.click_hit("", "settings");
    assert_eq!(harness.actions, vec![ServerAction::OpenSettings]);
}

#[test]
fn an_install_asked_from_a_notification_is_told_and_brings_a_closed_banner_back() {
    let mut harness = Harness::new(&["config/app.yaml"]);
    *harness.tools.install.lock().unwrap() = Some("true".to_owned());
    harness.session.state::<ServerRegistry>().unwrap().refresh();
    harness.until("the project scan", |servers| servers.iter().any(|server| server.id == "yaml" && server.needed));
    harness.servers.closed.insert("yaml".to_owned());

    let told = harness.servers.follow_notification("lsp.server.install", "yaml").expect("says it started");
    assert!(told.contains(harness.server("yaml").name.as_str()), "{told}");
    assert!(!harness.servers.closed.contains("yaml"));

    harness.registry.dispatch(&mut harness.session, "lsp.server.install", json!({ "id": "yaml" })).unwrap();
    harness.until("it runs", |servers| servers.iter().any(|server| server.id == "yaml" && server.installing));
    assert!(harness.servers.take_finished().is_empty(), "nothing to tell yet");
    harness.tools.found.lock().unwrap().insert("yaml".to_owned(), PathBuf::from("/bin/yaml-ls"));
    harness.until("it ends", |servers| servers.iter().any(|server| server.id == "yaml" && !server.installing));
    let finished = harness.servers.take_finished();
    assert_eq!(finished.len(), 1, "{finished:?}");
    assert!(harness.servers.take_finished().is_empty(), "told once");
}

#[test]
fn dont_ask_from_a_notification_closes_the_banner_at_once() {
    let mut harness = Harness::new(&["main.go"]);
    assert_eq!(harness.servers.follow_notification("lsp.server.dismiss", "go"), None);
    assert!(harness.servers.closed.contains("go"));
}

#[test]
fn the_log_viewer_reads_new_lines_once_a_second_while_open() {
    let mut harness = Harness::new(&["main.go"]);
    harness.open("main.go");
    harness.until("go runs", |servers| servers.iter().any(|server| server.id == "go" && server.state == "running"));
    harness.frame_with(Vec::new());
    assert!(harness.calls("lsp.server.log").is_empty(), "nothing is read while the viewer is closed");
    harness.act(ServerAction::ShowLog("go".to_owned()));
    harness.frame_with(Vec::new());
    harness.frame_with(Vec::new());
    assert_eq!(harness.calls("lsp.server.log"), vec![json!({ "id": "go", "since": 0 })]);
    let (id, lines) = harness.servers.log().unwrap();
    assert_eq!(id, "go");
    assert!(lines.iter().any(|line| line.starts_with("--> request initialize")), "{lines:?}");
    let next = lines.len() as u64;
    harness.now = 1.5;
    harness.frame_with(Vec::new());
    assert_eq!(harness.calls("lsp.server.log").last(), Some(&json!({ "id": "go", "since": next })));
}

#[cfg(unix)]
#[test]
fn the_banner_offers_install_and_follows_the_installation() {
    let mut harness = Harness::new(&["main.go", "app.yaml"]);
    let program = harness.dir.path().join("bin/found-ls");
    *harness.tools.install.lock().unwrap() = Some(installer_script(&program, "sleep 0.3;"));
    let declared = ServerDeclaration { name: "Found".to_owned(), ..declaration("found", &["found"], "") };
    let declared = ServerDeclaration { command: program.to_string_lossy().into_owned(), ..declared };
    harness.session.state::<ServerRegistry>().unwrap().declare([declared]);
    harness.open("app.yaml");
    std::fs::write(harness.path("a.found"), "").unwrap();
    harness.open("a.found");
    harness.until("found is needed", |servers| servers.iter().any(|server| server.id == "found" && server.needed));
    assert_eq!(harness.servers.banner("README", 0.0), None);
    assert_eq!(harness.servers.banner("lib.rs", 0.0), None);
    let banner = harness.servers.banner("a.found", 0.0).unwrap();
    assert_eq!(banner.id, "found");
    assert_eq!(banner.text, t!("ide-lsp-missing", name = "Found"));
    assert!(banner.install && banner.dismiss && banner.download.is_none());

    harness.act(ServerAction::Install("found".to_owned()));
    assert_eq!(harness.calls("lsp.server.install"), vec![json!({ "id": "found" })]);
    harness.poll(true);
    assert!(harness.server("found").installing);
    let installing = harness.servers.banner("a.found", 0.0).unwrap();
    assert!(!installing.install && !installing.dismiss);
    assert!(installing.text.contains("Found"), "{}", installing.text);
    assert!(harness.servers.is_busy());

    harness.now = 1.0;
    harness.until("installed", |servers| servers.iter().any(|server| server.id == "found" && !server.installing));
    assert_ne!(harness.server("found").state, "missing");
    let installed = harness.servers.banner("a.found", 1.0).unwrap();
    assert!(!installed.install && !installed.dismiss);
    assert_eq!(harness.servers.banner("a.found", 8.0), None);
}

#[cfg(unix)]
#[test]
fn a_failed_installation_is_shown_and_can_be_tried_again() {
    let mut harness = Harness::new(&["app.yaml"]);
    *harness.tools.install.lock().unwrap() = Some("echo 'npm ERR! 404' >&2; exit 1".to_owned());
    harness.until("yaml is needed", |servers| servers.iter().any(|server| server.id == "yaml" && server.needed));
    harness.act(ServerAction::Install("yaml".to_owned()));
    harness.until("the installation ends", |servers| {
        servers.iter().any(|server| server.id == "yaml" && !server.installing && server.message.is_some())
    });
    let banner = harness.servers.banner("app.yaml", 0.0).unwrap();
    assert!(banner.text.contains("npm ERR! 404"), "{}", banner.text);
    assert!(banner.install && banner.dismiss);
}

#[test]
fn dont_ask_again_dismisses_and_close_only_hides() {
    let mut harness = Harness::new(&["app.yaml"]);
    harness.until("yaml is needed", |servers| servers.iter().any(|server| server.id == "yaml" && server.needed));
    assert!(harness.servers.banner("app.yaml", 0.0).is_some());
    harness.act(ServerAction::Close("yaml".to_owned()));
    assert_eq!(harness.servers.banner("app.yaml", 0.0), None);
    assert!(harness.calls("lsp.server.dismiss").is_empty());
    assert!(!harness.server("yaml").dismissed);

    let mut harness = Harness::new(&["app.yaml"]);
    harness.until("yaml is needed", |servers| servers.iter().any(|server| server.id == "yaml" && server.needed));
    harness.act(ServerAction::Dismiss("yaml".to_owned()));
    assert_eq!(harness.calls("lsp.server.dismiss"), vec![json!({ "id": "yaml" })]);
    harness.poll(true);
    assert!(harness.server("yaml").dismissed);
    assert_eq!(harness.servers.banner("app.yaml", 0.0), None);
    let saved = std::fs::read_to_string(harness.dir.path().join("language-servers.json")).unwrap();
    assert!(saved.contains("yaml"), "{saved}");
}

#[test]
fn without_its_installer_the_banner_says_what_to_get_first() {
    let mut harness = Harness::new(&["app.yaml"]);
    *harness.tools.tool_missing.lock().unwrap() = true;
    harness.until("yaml is needed", |servers| servers.iter().any(|server| server.id == "yaml" && server.needed));
    let banner = harness.servers.banner("app.yaml", 0.0).unwrap();
    assert!(banner.text.contains("Go") && !banner.text.contains("workbench-"), "{}", banner.text);
    assert!(!banner.install && banner.dismiss);
    let (label, url) = banner.download.unwrap();
    assert!(label.contains("Go") && !label.contains("ide-"), "{label}");
    assert_eq!(url, "https://go.dev/dl/");
}

#[test]
fn files_match_servers_by_extension_or_name() {
    let info = ServerInfo {
        extensions: vec!["ts".to_owned(), "vue".to_owned()],
        names: vec!["Dockerfile".to_owned(), "*.dockerfile".to_owned()],
        ..ServerInfo::default()
    };
    assert!(info.serves("app.ts"));
    assert!(info.serves("App.VUE"));
    assert!(info.serves("Dockerfile") && info.serves("web.Dockerfile"));
    assert!(!info.serves("main.go") && !info.serves("ts"));
}

#[test]
fn arguments_split_and_join_with_quotes() {
    assert_eq!(split_args("  -rpc.trace  --mode \"a b\" ''"), vec!["-rpc.trace", "--mode", "a b", ""]);
    let args = vec!["-v".to_owned(), "a b".to_owned()];
    assert_eq!(join_args(&args), "-v \"a b\"");
    assert_eq!(split_args(&join_args(&args)), args);
}

#[test]
fn the_settings_page_configures_only_what_changed() {
    let mut harness = Harness::new(&["main.go"]);
    harness.tools.found.lock().unwrap().insert("toml".to_owned(), PathBuf::from("/bin/toml-ls"));
    harness.session.state::<ServerRegistry>().unwrap().refresh();
    harness.servers.invalidate();
    harness.poll(true);
    let info = harness.server("toml");
    let mut page = ServersPage::default();
    page.select(&info);
    assert_eq!(page.configure(&info), None);
    page.args = "-rpc.trace serve".to_owned();
    assert_eq!(
        page.configure(&info),
        Some(("lsp.server.configure", json!({ "id": "toml", "args": ["-rpc.trace", "serve"] }))),
        "the path found is not pinned"
    );
    page.path = " /opt/toml-ls ".to_owned();
    let (command, params) = page.configure(&info).unwrap();
    harness.registry.dispatch(&mut harness.session, command, params).unwrap();
    harness.servers.invalidate();
    harness.poll(true);
    let info = harness.server("toml");
    assert_eq!(
        (info.path.as_deref(), info.args.clone()),
        (Some("/opt/toml-ls"), vec!["-rpc.trace".to_owned(), "serve".to_owned()])
    );
    page.select(&info);
    page.path = String::new();
    let (command, params) = page.configure(&info).unwrap();
    assert_eq!(params, json!({ "id": "toml", "path": "" }));
    page.applied = true;
    harness.registry.dispatch(&mut harness.session, command, params).unwrap();
    harness.servers.invalidate();
    harness.poll(true);
    assert_eq!(
        harness.server("toml").path.as_deref(),
        Some("/bin/toml-ls"),
        "an empty path goes back to the one found"
    );
    let info = harness.server("toml");
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        page.show(ui, &IdePalette::zen_dark(), Some(std::slice::from_ref(&info)));
    });
    output.textures_delta.clear();
    assert_eq!(page.path, "/bin/toml-ls", "the unedited field follows the program found");
    assert_eq!(page.configure(&info), None);
}

#[test]
fn a_server_turned_off_leaves_the_status_bar() {
    let mut harness = Harness::new(&["app.yaml"]);
    harness.until("yaml is needed", |servers| servers.iter().any(|server| server.id == "yaml" && server.needed));
    assert!(harness.servers.shown().iter().any(|server| server.id == "yaml"));
    harness
        .registry
        .dispatch(&mut harness.session, "lsp.server.configure", json!({ "id": "yaml", "enabled": false }))
        .unwrap();
    harness.servers.invalidate();
    harness.poll(true);
    let yaml = harness.server("yaml");
    assert_eq!((yaml.state.as_str(), yaml.enabled), ("disabled", false));
    assert!(!harness.servers.shown().iter().any(|server| server.id == "yaml"));
    assert_eq!(harness.servers.banner("app.yaml", 0.0), None);
}

#[test]
fn the_widest_columns_give_way_first_and_the_table_scrolls_only_below_the_least_widths() {
    let natural = [60.0, 90.0, 60.0, 260.0, 120.0];
    let least = [72.0, 84.0, 52.0, 90.0, 56.0];
    assert_eq!(column_widths(&natural, &least, 700.0), (natural.to_vec(), false), "all fit");
    let (widths, scroll) = column_widths(&natural, &least, 500.0);
    assert!(!scroll);
    assert!((widths.iter().sum::<f32>() - 500.0).abs() < 0.5, "{widths:?}");
    assert_eq!(&widths[..3], &natural[..3], "the name, state and version keep their width: {widths:?}");
    assert!(widths[3] < 260.0 && widths[4] <= 120.0 && widths[3] >= widths[4], "{widths:?}");
    let (widths, scroll) = column_widths(&natural, &least, 300.0);
    assert!(scroll, "below the least widths the table scrolls: {widths:?}");
    assert_eq!(widths, vec![60.0, 84.0, 52.0, 90.0, 56.0]);
}

#[test]
fn the_servers_table_stays_inside_the_settings_page() {
    let servers = vec![
        ServerInfo {
            id: "gopls".to_owned(),
            name: "gopls".to_owned(),
            state: "running".to_owned(),
            version: Some("v0.24.0".to_owned()),
            path: Some("/Users/someone-with-a-long-name/go/bin/gopls".to_owned()),
            ..ServerInfo::default()
        },
        ServerInfo {
            id: "yaml".to_owned(),
            name: "YAML Language Server".to_owned(),
            state: "missing".to_owned(),
            args: vec!["--stdio".to_owned()],
            ..ServerInfo::default()
        },
        ServerInfo {
            id: "taplo".to_owned(),
            name: "Taplo".to_owned(),
            state: "missing".to_owned(),
            args: vec!["lsp".to_owned(), "stdio".to_owned()],
            ..ServerInfo::default()
        },
    ];
    for width in [610.0, 290.0] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let page = Rect::from_min_size(pos2(270.0, 40.0), vec2(width, 500.0));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let mut page_ui = ui.new_child(egui::UiBuilder::new().max_rect(page));
            ServersPage::default().show(&mut page_ui, &IdePalette::zen_dark(), Some(&servers));
        });
        output.textures_delta.clear();
        for clipped in &output.shapes {
            let drawn = clipped.shape.visual_bounding_rect().intersect(clipped.clip_rect);
            if drawn.is_positive() && drawn.left() >= page.left() - 1.0 {
                assert!(drawn.right() <= page.right() + 0.5, "at {width}: {drawn:?} is outside {page:?}");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn a_server_without_the_plugins_other_plugins_contribute_offers_to_add_them() {
    let mut harness = Harness::new(&["main.go"]);
    let program = harness.dir.path().join("bin/found-ls");
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::write(&program, "").unwrap();
    let plugin = harness.dir.path().join("language-servers/found/node_modules/fake-plugin");
    *harness.tools.install.lock().unwrap() =
        Some(format!("mkdir -p '{plugin}' && echo '{{}}' > '{plugin}/package.json'", plugin = plugin.display()));
    let declared = ServerDeclaration {
        name: "Found".to_owned(),
        command: program.to_string_lossy().into_owned(),
        ..declaration("found", &["found"], "")
    };
    let registry = harness.session.state::<ServerRegistry>().unwrap().clone();
    registry.declare([declared]);
    registry.contribute(
        "other",
        [zen_workbench::language::servers::ServerPlugin {
            server: "found".to_owned(),
            name: "fake-plugin".to_owned(),
            package: "fake-plugin@1.0.0".to_owned(),
            languages: vec!["other".to_owned()],
            when: None,
        }],
    );
    std::fs::write(harness.path("a.found"), "").unwrap();
    harness.open("a.found");
    harness.until("found lacks its plug-in", |servers| {
        servers.iter().any(|server| server.id == "found" && server.needed && !server.missing_plugins.is_empty())
    });
    let banner = harness.servers.banner("a.found", 0.0).unwrap();
    assert_eq!(banner.text, t!("ide-lsp-plugins-missing", name = "Found", plugins = "fake-plugin"));
    assert!(banner.install && banner.dismiss);

    harness.act(ServerAction::Install("found".to_owned()));
    harness.until("the plug-in is added", |servers| {
        servers.iter().any(|server| server.id == "found" && server.missing_plugins.is_empty() && !server.installing)
    });
    assert!(plugin.join("package.json").is_file());
    let banner = harness.servers.banner("a.found", 0.0);
    assert!(banner.as_ref().is_none_or(|banner| !banner.install), "nothing more to install: {banner:?}");
}

#[cfg(unix)]
fn plugin_banner(package: &str, plugin: &str, manifest: &str) -> Option<String> {
    let files: &[&str] = if manifest.is_empty() { &["main.go"] } else { &["main.go", "package.json"] };
    let mut harness = Harness::new(files);
    if !manifest.is_empty() {
        std::fs::write(harness.path("package.json"), manifest).unwrap();
    }
    let program = harness.dir.path().join("bin/ts-ls");
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::write(&program, "").unwrap();
    let declared = ServerDeclaration {
        name: "TypeScript".to_owned(),
        command: program.to_string_lossy().into_owned(),
        ..declaration("ts", &["ts"], "")
    };
    let registry = harness.session.state::<ServerRegistry>().unwrap().clone();
    registry.declare([declared]);
    registry.contribute(
        package,
        [zen_workbench::language::servers::ServerPlugin {
            server: "ts".to_owned(),
            name: plugin.to_owned(),
            package: format!("{plugin}@1.0.0"),
            languages: vec![package.to_owned()],
            when: Some(zen_project::support::ProjectMarkers { files: Vec::new(), packages: vec![package.to_owned()] }),
        }],
    );
    std::fs::write(harness.path("main.ts"), "export {}\n").unwrap();
    harness.open("main.ts");
    harness.until("ts is needed", |servers| servers.iter().any(|server| server.id == "ts" && server.needed));
    harness.servers.banner("main.ts", 0.0).map(|banner| banner.text)
}

#[cfg(unix)]
#[test]
fn the_missing_svelte_plugin_is_offered_only_in_a_svelte_project() {
    let svelte = r#"{ "devDependencies": { "svelte": "^5.0.0", "typescript": "^5.0.0" } }"#;
    let banner = plugin_banner("svelte", "typescript-svelte-plugin", svelte);
    assert_eq!(
        banner,
        Some(t!("ide-lsp-plugins-missing", name = "TypeScript", plugins = "typescript-svelte-plugin")),
        "a Svelte project"
    );
    let plain = r#"{ "devDependencies": { "typescript": "^5.0.0" } }"#;
    assert_eq!(plugin_banner("svelte", "typescript-svelte-plugin", plain), None, "a TypeScript project");
    assert_eq!(plugin_banner("svelte", "typescript-svelte-plugin", ""), None, "no package.json");
}

#[cfg(unix)]
#[test]
fn the_missing_vue_plugin_is_offered_only_in_a_vue_project() {
    let vue = r#"{ "dependencies": { "vue": "^3.5.0" } }"#;
    let banner = plugin_banner("vue", "@vue/typescript-plugin", vue);
    assert_eq!(banner, Some(t!("ide-lsp-plugins-missing", name = "TypeScript", plugins = "@vue/typescript-plugin")));
    let plain = r#"{ "dependencies": { "react": "^19.0.0" } }"#;
    assert_eq!(plugin_banner("vue", "@vue/typescript-plugin", plain), None, "a React project");
}

#[test]
fn the_banner_offers_the_chosen_server_of_a_language_with_its_notice() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let list = |chosen: &str, other: &str| {
        let server = |id: &str, name: &str, notice: bool| {
            let mut server = json!({
                "id": id, "name": name, "state": "missing", "source": "missing", "args": [], "files": 0,
                "queued": 0, "languages": ["kotlin"], "extensions": ["kt", "kts"], "names": [],
                "enabled": id == chosen, "installable": true, "installing": false, "needed": true,
                "dismissed": false, "busy": false, "alternatives": [chosen, other],
            });
            if notice {
                server["notice"] = json!("kotlin-lsp is its vendor's software.");
                server["noticeUrl"] = json!("https://github.com/Kotlin/kotlin-lsp");
            }
            server
        };
        json!({ "servers": [
            server("kotlin-lsp", "Kotlin LSP", true),
            server("kotlin-language-server", "Kotlin Language Server (fwcd)", false),
        ] })
    };
    let mut servers = LanguageServers::default();
    let answer = list("kotlin-lsp", "kotlin-language-server");
    let mut dispatch = |_: &str, _: Value| -> Result<Value, String> { Ok(answer.clone()) };
    servers.poll(0.0, true, &mut dispatch);
    let banner = servers.banner("Main.kt", 0.0).unwrap();
    assert_eq!(banner.id, "kotlin-lsp");
    assert!(
        banner.text.contains("Kotlin LSP") && banner.text.ends_with("kotlin-lsp is its vendor's software."),
        "{}",
        banner.text
    );
    assert_eq!(banner.download, Some((t!("ide-lsp-notice-link"), "https://github.com/Kotlin/kotlin-lsp".to_owned())));

    let mut servers = LanguageServers::default();
    let answer = list("kotlin-language-server", "kotlin-lsp");
    let mut dispatch = |_: &str, _: Value| -> Result<Value, String> { Ok(answer.clone()) };
    servers.poll(0.0, true, &mut dispatch);
    let banner = servers.banner("build.gradle.kts", 0.0).unwrap();
    assert_eq!(banner.id, "kotlin-language-server", "the chosen one, not the first");
    assert!(!banner.text.contains("vendor"));
}

#[test]
fn a_server_without_the_java_it_needs_offers_to_download_that_jdk() {
    let list = json!({ "servers": [{
        "id": "jdtls", "name": "Eclipse JDT Language Server", "state": "failed", "source": "local",
        "extensions": ["java"], "needed": true, "javaNeeded": 25, "enabled": true
    }] });
    let mut calls: Vec<(String, Value)> = Vec::new();
    let mut servers = LanguageServers::default();
    let mut dispatch = |command: &str, params: Value| -> Result<Value, String> {
        calls.push((command.to_owned(), params));
        Ok(if command == "lsp.servers.list" { list.clone() } else { json!({ "task": 1 }) })
    };
    servers.poll(0.0, true, &mut dispatch);
    let banner = servers.banner("App.java", 0.0).unwrap();
    assert_eq!(banner.jdk, Some(25));
    assert!(banner.text.contains("25") && !banner.install, "{banner:?}");
    servers.act(ServerAction::DownloadJdk("jdtls".to_owned(), 25), &mut dispatch).unwrap();
    assert!(calls.contains(&("toolchain.jdk.download".to_owned(), json!({ "version": 25 }))), "{calls:?}");
    assert_eq!(servers.banner("App.java", 0.0), None, "the banner closes once the download starts");
}

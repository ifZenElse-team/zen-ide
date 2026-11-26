use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::Registry;

use super::*;

const MAIN: &str = "package main\n\nfunc greet() {}\n\nfunc main() {\n\tgreet()\n\tmissing()\n}\n";

fn product() -> IdeProduct {
    IdeProduct { name: "Test", id: "test", version: "0.0.0", mcp_port: 0, ..IdeProduct::default() }
}

fn registry() -> Registry {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    registry
}

fn range(line: u32, start: u32, end: u32) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

fn server(file: &Path) -> zen_lsp::fake::FakeServer {
    let server = zen_lsp::fake::FakeServer::new();
    server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "referencesProvider": true,
        "renameProvider": { "prepareProvider": true }
    }));
    let uri = zen_lsp::position::file_uri(file);
    server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", |params| {
        json!({
            "uri": params["textDocument"]["uri"],
            "diagnostics": [{ "range": range(6, 1, 8), "severity": 1, "source": "compiler", "message": "undefined: missing" }]
        })
    });
    let references = json!([{ "uri": uri, "range": range(2, 5, 10) }, { "uri": uri, "range": range(5, 1, 6) }]);
    server.on("textDocument/references", references);
    server.on("textDocument/prepareRename", json!({ "range": range(2, 5, 10), "placeholder": "greet" }));
    let edits = json!([
        { "range": range(2, 5, 10), "newText": "hello" },
        { "range": range(5, 1, 6), "newText": "hello" }
    ]);
    server.on("textDocument/rename", json!({ "changes": { uri: edits } }));
    server
}

fn call(url: &str, tool: &str, arguments: Value) -> Result<Value, String> {
    zen_automation::instances::call(url, tool, arguments)
}

fn command(url: &str, id: &str, params: Value) -> Result<Value, String> {
    call(url, "dispatch_command", json!({ "command": id, "params": params }))
}

#[test]
fn a_headless_ide_serves_problems_usages_and_rename_over_mcp_and_exits() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let file = project.join("main.go");
    std::fs::write(&file, MAIN).unwrap();
    let recent = Some(data.path().join("recent-projects.json"));
    let mut app = IdeApp::build(product(), registry(), recent, vec![project.clone()], true);
    app.enable_automation(0);
    let fake = server(&file);
    let launched = fake.clone();
    app.with_session(|session| {
        session.state_mut::<zen_workbench::language::Languages>().set_specs(vec![
            zen_workbench::language::ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            },
        ]);
    })
    .expect("the project is open");
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");

    let agent_file = file.clone();
    let agent = std::thread::spawn(move || {
        let file = agent_file;
        let commands = call(&url, "list_commands", json!({ "full": true })).unwrap();
        let listed = commands["commands"].as_array().map(Vec::len).unwrap_or_default();
        command(&url, "file.document.open", json!({ "path": file })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let problems = loop {
            let problems = command(&url, "problems.file.list", json!({ "path": file })).unwrap();
            if problems.as_array().is_some_and(|list| !list.is_empty()) || Instant::now() > deadline {
                break problems;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let offset = MAIN.find("greet").unwrap();
        let found =
            command(&url, "navigate.usages.find", json!({ "path": file, "offset": offset, "waitMs": 5000 })).unwrap();
        let rename = command(
            &url,
            "refactor.rename.apply",
            json!({ "path": file, "offset": offset, "newName": "hello", "waitMs": 5000 }),
        )
        .unwrap();
        let text = call(&url, "get_document_text", json!({ "path": file })).unwrap();
        let window_state = call(&url, "get_window_state", json!({}));
        let action = call(&url, "dispatch_command", json!({ "command": "view.settings.open" }));
        let keys = call(&url, "press_keys", json!({ "keys": ["⎋"] }));
        let exit = call(&url, "exit_ide", json!({})).unwrap();
        (listed, problems, found, rename, text, (window_state, action, keys), exit)
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(60);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (listed, problems, found, rename, text, (window_state, action, keys), exit) = agent.join().unwrap();

    assert_eq!(ctx.cumulative_pass_nr(), 0, "nothing was laid out or drawn");
    assert!(started.elapsed() < Duration::from_secs(30), "the loop ended with exit_ide");
    assert_eq!(exit["exiting"], true, "{exit}");
    assert!(listed > 100, "every command is listed: {listed}");
    assert_eq!(problems[0]["message"], "undefined: missing", "{problems}");
    assert_eq!(found["total"], 2, "{found}");
    assert_eq!(rename["edits"], 2, "{rename}");
    assert!(
        text["text"].as_str().is_some_and(|text| text.contains("func hello()") && !text.contains("greet")),
        "{text}"
    );
    let window_state = window_state.unwrap();
    assert!(window_state.get("toolWindows").is_some() || window_state.get("left").is_some(), "{window_state}");
    assert_eq!(window_state["headless"], true, "{window_state}");
    let overview = &window_state["session"];
    assert!(overview["project"].as_str().is_some_and(|project| !project.is_empty()), "{window_state}");
    assert!(
        overview["languageServers"].is_object() && overview["index"].is_object(),
        "one call tells the session: {overview}"
    );
    assert!(window_state.to_string().len() < 8 * 1024, "{}", window_state.to_string().len());
    let action = action.unwrap();
    assert!(action["window"]["popups"][0].as_str().is_some_and(|popup| popup.starts_with("settings:")), "{action}");
    let error = keys.expect_err("press_keys");
    assert!(error.contains("not available in a headless IDE"), "press_keys: {error}");
    assert!(std::fs::read_to_string(&file).unwrap().contains("func hello()"));
    assert!(!data.path().join("recent-projects.json").exists(), "the recent projects are the windowed IDE's");
    assert!(!data.path().join("automation.json").exists(), "automation.json is the windowed IDE's");
}

#[test]
fn an_agent_edits_reads_and_lists_in_a_few_bytes_over_mcp() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(project.join("store")).unwrap();
    let big: String =
        (1..=800).map(|line| format!("// line {line}: a comment as long as a typical Go line\n")).collect();
    std::fs::write(project.join("store/big.go"), format!("package store\n{big}")).unwrap();
    std::fs::write(project.join("main.go"), MAIN).unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    app.enable_automation(0);
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");
    let agent = std::thread::spawn(move || {
        let replace = json!({ "path": "main.go", "oldText": "\tmissing()\n", "newText": "", "save": true });
        let edit = command(&url, "editor.text.replace", replace).unwrap();
        let again = command(
            &url,
            "editor.text.replace",
            json!({ "path": "main.go", "oldText": "\tmissing()\n", "newText": "" }),
        );
        let part = call(&url, "get_document_text", json!({ "path": "store/big.go", "startLine": 300, "endLine": 339 }))
            .unwrap();
        let whole = call(&url, "get_document_text", json!({ "path": "store/big.go" }));
        let catalogue = call(&url, "list_commands", json!({})).unwrap();
        let schema = call(&url, "describe_command", json!({ "id": "editor.text.replace" })).unwrap();
        let exit = call(&url, "exit_ide", json!({})).unwrap();
        (edit, again, part, whole, catalogue, schema, exit)
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(60);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (edit, again, part, whole, catalogue, schema, exit) = agent.join().unwrap();

    assert_eq!(exit["exiting"], true);
    assert_eq!(edit, json!({ "changed": true, "line": 7, "saved": true }));
    assert!(edit.to_string().len() < 300);
    assert_eq!(std::fs::read_to_string(project.join("main.go")).unwrap(), MAIN.replace("\tmissing()\n", ""));
    let again: Value = serde_json::from_str(&again.unwrap_err()).unwrap();
    assert_eq!((again["error"].as_str(), again["count"].as_u64()), (Some("not_found"), Some(0)), "{again}");
    assert_eq!(
        (part["startLine"].as_u64(), part["endLine"].as_u64(), part["totalLines"].as_u64()),
        (Some(300), Some(339), Some(801))
    );
    assert!(part.to_string().len() < 3 * 1024, "{}", part.to_string().len());
    let whole: Value = serde_json::from_str(&whole.unwrap_err()).unwrap();
    assert_eq!(whole["error"], "too_large");
    assert!(catalogue.to_string().len() < 20 * 1024 && catalogue["groups"]["editor"].is_array());
    assert!(schema.to_string().len() < 3 * 1024 && schema["params"]["properties"]["oldText"].is_object());
}

#[test]
fn the_font_size_of_a_file_starts_from_the_settings_an_agent_changed() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("main.go"), MAIN).unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    app.enable_automation(0);
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");
    let file = project.join("main.go");
    let agent = std::thread::spawn(move || {
        command(&url, "file.document.open", json!({ "path": file })).unwrap();
        call(&url, "set_settings", json!({ "editorFont": { "size": 16 } })).unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        let state = command(&url, "editor.fontSize.get", json!({})).unwrap();
        let bigger = command(&url, "editor.fontSize.increase", json!({})).unwrap();
        call(&url, "exit_ide", json!({})).unwrap();
        (state, bigger)
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(60);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (state, bigger) = agent.join().unwrap();
    assert_eq!((state["size"].clone(), state["default"].clone()), (json!(16.0), json!(16.0)), "{state}");
    assert_eq!(bigger["size"], 17.0, "{bigger}");
}

#[test]
fn a_parent_that_ends_stops_the_ide() {
    let data = tempfile::tempdir().unwrap();
    let mut app =
        IdeApp::build(product(), registry(), Some(data.path().join("recent-projects.json")), Vec::new(), true);
    app.enable_automation(0);
    let (ctx, wake) = context();
    let started = Instant::now();
    run_loop(&mut app, &ctx, &wake, &mut || started.elapsed() > Duration::from_millis(200));
    assert!(app.finished());
}

#[cfg(unix)]
#[test]
fn a_parent_that_ends_stops_the_ide_even_when_a_file_cannot_be_saved() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    let locked = project.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(project.join("main.go"), MAIN).unwrap();
    std::fs::write(locked.join("locked.go"), "package locked\n").unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    app.with_session(|session| {
        let registry = registry();
        for file in [locked.join("locked.go"), project.join("main.go")] {
            registry.dispatch(session, "file.document.open", json!({ "path": file })).unwrap();
            registry.dispatch(session, "editor.edit.insert", json!({ "text": "// edit\n" })).unwrap();
        }
    });
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let started = Instant::now();
    run_loop(&mut app, &ctx, &wake, &mut || true);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(app.finished());
    let _ = started;
    assert_eq!(std::fs::read_to_string(project.join("main.go")).unwrap(), format!("// edit\n{MAIN}"));
}

#[test]
fn an_ide_started_for_bridges_ends_after_its_last_client() {
    let count = std::cell::Cell::new(None);
    let mut unused = Unused::new(|| count.get());
    assert!(!unused.now_unused(), "it waits for its first client");
    std::thread::sleep(MAX_WAIT);
    count.set(Some(2));
    assert!(!unused.now_unused());
    std::thread::sleep(MAX_WAIT);
    count.set(Some(1));
    assert!(!unused.now_unused(), "one client is left");
    std::thread::sleep(MAX_WAIT);
    count.set(Some(0));
    assert!(!unused.now_unused(), "it lingers for a moment");
    std::thread::sleep(LINGER + MAX_WAIT);
    assert!(unused.now_unused(), "the last client left");
}

#[test]
fn a_session_shorter_than_a_look_still_ends_the_ide_at_once() {
    let data = tempfile::tempdir().unwrap();
    let pid = 4242;
    let mut unused = Unused::new(|| zen_automation::instances::leases(data.path(), pid));
    assert!(!unused.now_unused(), "it waits for its first client");
    drop(zen_automation::instances::Lease::take(data.path(), pid).unwrap());
    std::thread::sleep(MAX_WAIT);
    assert!(!unused.now_unused(), "it lingers for a moment");
    let left = Instant::now();
    while !unused.now_unused() {
        assert!(left.elapsed() < Duration::from_secs(60), "the IDE waits for a first client that has been and gone");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(left.elapsed() >= LINGER);
}

#[test]
fn the_command_line_asks_for_a_headless_ide() {
    let args = |list: &[&str]| HeadlessOptions::from_args(list.iter().map(OsString::from));
    assert_eq!(args(&["/tmp/project"]), Ok(None), "a windowed start");
    assert_eq!(
        args(&["--headless", "--mcp-port", "55801", "--exit-with-parent", "/tmp/a", "/tmp/b"]),
        Ok(Some(HeadlessOptions {
            projects: vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
            mcp_port: Some(55801),
            exit_with_parent: true,
            exit_when_unused: false,
            accept_open: false,
            no_restore: false,
        }))
    );
    assert!(args(&["--headless", "--exit-when-unused", "/tmp/a"]).unwrap().unwrap().exit_when_unused);
    assert!(args(&["--headless", "--accept-open", "/tmp/a"]).unwrap().unwrap().accept_open);
    assert!(args(&["--headless", "--no-restore", "/tmp/a"]).unwrap().unwrap().no_restore);
    assert_eq!(args(&["--headless", "--mcp-port=55802"]).unwrap().unwrap().mcp_port, Some(55802));
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    assert_eq!(args(&["--headless", "--mcp-port", "x"]), Err("`x` is not a port".to_owned()));
    assert_eq!(args(&["--headless", "--nope"]), Err("unknown option `--nope`".to_owned()));
}

#[test]
fn a_second_headless_start_on_the_same_project_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap();
    let running = zen_automation::Server::start(
        0,
        zen_automation::ServerInfo { name: "Test".to_owned(), version: "0".to_owned(), instructions: String::new() },
        Vec::new(),
        || {},
    )
    .unwrap()
    .0;
    let instance = zen_automation::instances::Instance {
        pid: 4242,
        url: running.url(),
        product: "test".to_owned(),
        projects: vec![project.clone()],
        started: 1,
    };
    zen_automation::instances::record(data.path(), &instance).unwrap();
    let options = HeadlessOptions { projects: vec![project], ..HeadlessOptions::default() };
    let code = run_headless(product(), registry(), Some(data.path().join("recent-projects.json")), options);
    assert_eq!(code, EXIT_ALREADY_RUNNING);
}

#[test]
fn an_agent_sets_the_on_save_options_and_saves_over_mcp() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("notes.txt"), "kept  \r\nedit\r\n\r\n").unwrap();
    std::fs::write(project.join("table.csv"), "a  \nb").unwrap();
    std::fs::write(project.join(".editorconfig"), "[*.csv]\ntrim_trailing_whitespace = false\n").unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    app.enable_automation(0);
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");
    let notes = project.join("notes.txt");
    let agent = std::thread::spawn(move || {
        let defaults = call(&url, "get_settings", json!({})).unwrap();
        let settings = json!({ "removeTrailingBlankLines": true, "ensureLineBreakAtEnd": true });
        let set = call(&url, "set_settings", settings).unwrap();
        let replace = json!({ "path": "notes.txt", "oldText": "edit", "newText": "edited  ", "save": false });
        command(&url, "editor.text.replace", replace).unwrap();
        command(&url, "editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        command(&url, "file.document.save", json!({})).unwrap();
        let saved = std::fs::read_to_string(notes).unwrap();
        let undone = command(&url, "editor.edit.undo", json!({}))
            .and_then(|_| call(&url, "get_document_text", json!({ "path": "notes.txt" })));
        let csv = json!({ "path": "table.csv", "oldText": "b", "newText": "c", "save": true });
        command(&url, "editor.text.replace", csv).unwrap();
        let options = command(&url, "editor.onSave.options", json!({ "path": "table.csv" })).unwrap();
        let wrong = call(&url, "set_settings", json!({ "removeTrailingSpacesOn": "some" }));
        let exit = call(&url, "exit_ide", json!({})).unwrap();
        (defaults, set, saved, undone, options, wrong, exit)
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(60);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (defaults, set, saved, undone, options, wrong, exit) = agent.join().unwrap();

    assert_eq!(exit["exiting"], true);
    assert_eq!(
        (defaults["removeTrailingSpaces"].clone(), defaults["removeTrailingSpacesOn"].clone()),
        (json!(true), json!("modified"))
    );
    assert_eq!(defaults["keepTrailingSpacesOnCaretLine"], true);
    assert_eq!(
        (defaults["removeTrailingBlankLines"].clone(), defaults["ensureLineBreakAtEnd"].clone()),
        (json!(false), json!(false))
    );
    assert_eq!(
        (set["removeTrailingBlankLines"].clone(), set["ensureLineBreakAtEnd"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(saved, "kept  \r\nedited\r\n", "CRLF kept");
    assert_eq!(undone.unwrap()["text"], "kept  \r\nedited  \r\n\r\n", "one undo step");
    assert_eq!(std::fs::read_to_string(project.join("table.csv")).unwrap(), "a  \nc\n");
    assert_eq!(
        (options["removeTrailingSpaces"].clone(), options["trimTrailingWhitespace"].clone()),
        (json!("off"), json!(false))
    );
    assert!(wrong.is_err(), "{wrong:?}");
}

fn with_agent<T: Send + 'static>(
    recent: Option<PathBuf>,
    project: &Path,
    restore: bool,
    agent: impl FnOnce(String) -> T + Send + 'static,
) -> T {
    let mut app = IdeApp::build_restoring(product(), registry(), recent, vec![project.to_owned()], true, restore);
    app.enable_automation(0);
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");
    let agent = std::thread::spawn(move || agent(url));
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(60);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    agent.join().unwrap()
}

#[test]
fn secrets_are_kept_from_agents_and_the_window_says_which() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(".env"), "TOKEN=abc123\n").unwrap();
    std::fs::write(project.join(".env.example"), "TOKEN=\n").unwrap();
    std::fs::write(project.join("main.go"), "package main\n\n// TOKEN comes from the environment.\n").unwrap();
    let (read, sample, (found, guesses), typed, settings, notices, stats) = with_agent(None, &project, true, |url| {
        let read = call(&url, "get_document_text", json!({ "path": ".env" }));
        let sample = call(&url, "get_document_text", json!({ "path": ".env.example" }));
        for _ in 0..200 {
            if command(&url, "search.index.status", json!({})).unwrap()["ready"] == true {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let found = command(&url, "search.text.find", json!({ "query": { "text": "TOKEN" } })).unwrap();
        let guesses: Vec<Value> = ["TOKEN=abc", "TOKEN=abx", "abc123", "zzz999"]
            .iter()
            .map(|text| command(&url, "search.text.find", json!({ "query": { "text": text } })).unwrap())
            .collect();
        let found = (found, guesses);
        let typed = call(&url, "terminal_write", json!({ "text": "cat .env\n" }));
        let settings = call(&url, "set_settings", json!({ "mcpRedactFiles": [] }));
        std::thread::sleep(Duration::from_millis(300));
        let notices = command(&url, "ui.notifications.list", json!({})).unwrap();
        let stats = command(&url, "mcp.session.stats", json!({})).unwrap();
        call(&url, "exit_ide", json!({})).unwrap();
        (read, sample, found, typed, settings, notices, stats)
    });
    let read: Value = serde_json::from_str(&read.unwrap_err()).unwrap();
    assert_eq!((read["error"].as_str(), read["file"].as_str()), (Some("redacted"), Some(".env")), "{read}");
    assert!(!read.to_string().contains("abc123"));
    assert_eq!(sample.unwrap()["text"], "TOKEN=\n", "a sample stays readable");
    let texts: Vec<(&str, &str)> = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| (found["path"].as_str().unwrap(), found["text"].as_str().unwrap()))
        .collect();
    assert!(texts.iter().all(|(path, _)| *path != ".env"), "the hidden file is not searched at all: {found}");
    assert!(texts.contains(&("main.go", "// TOKEN comes from the environment.")), "{found}");
    assert_eq!(found["hiddenFiles"], 1, "only how many files are hidden: {found}");
    assert!(!found.to_string().contains("abc123"));
    for guess in &guesses {
        assert_eq!(guess, &guesses[3], "a right guess and a wrong one answer the same: {guesses:?}");
    }
    assert_eq!(guesses[0]["reason"], "noMatch");
    assert!(typed.unwrap_err().contains("redacted"));
    assert!(settings.is_err(), "only the user changes the list");
    assert!(notices.to_string().contains(".env"), "the window says which file: {notices}");
    assert_eq!(stats["tools"]["get_document_text"]["calls"], 2, "{stats}");
    assert_eq!(stats["tools"]["get_document_text"]["errors"], 1, "{stats}");
    assert!(stats["tools"]["search.text.find"]["bytesOut"].as_u64().unwrap() > 50, "{stats}");
    assert!(stats["total"]["calls"].as_u64().unwrap() >= 6, "{stats}");
}

#[test]
fn no_restore_opens_the_project_without_its_last_tabs() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("main.go"), MAIN).unwrap();
    let recent = Some(data.path().join("recent-projects.json"));
    let mut workspace = super::super::workspace::Workspace::default();
    let layout =
        super::super::workspace::ProjectLayout { documents: vec![project.join("main.go")], ..Default::default() };
    workspace.projects.insert(project.clone(), layout);
    workspace.save(&data.path().join("workspace.json"));
    let documents = |restore: bool| {
        with_agent(recent.clone(), &project, restore, |url| {
            let windows = call(&url, "list_windows", json!({})).unwrap();
            call(&url, "exit_ide", json!({})).unwrap();
            windows["windows"][0]["documents"].as_array().map_or(0, Vec::len)
        })
    };
    assert_eq!(documents(false), 0, "--no-restore");
    assert_eq!(documents(true), 1, "the last tabs come back without it");
}

#[test]
fn a_hidden_file_in_front_is_not_read_through_the_active_document() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(".env"), "TOKEN=abc123\n").unwrap();
    std::fs::write(project.join("main.go"), MAIN).unwrap();
    let mut workspace = super::super::workspace::Workspace::default();
    let layout = super::super::workspace::ProjectLayout { documents: vec![project.join(".env")], ..Default::default() };
    workspace.projects.insert(project.clone(), layout);
    workspace.save(&data.path().join("workspace.json"));
    let recent = Some(data.path().join("recent-projects.json"));
    let main = project.join("main.go");
    let (refused, windows, after) = with_agent(recent, &project, true, move |url| {
        let calls: Vec<(&str, Value)> = vec![
            ("editor.selection.get", json!({})),
            ("editor.selection.selectAll", json!({})),
            ("editor.structure.get", json!({})),
            ("editor.occurrences.list", json!({})),
            ("editor.code.reformat", json!({})),
            ("editor.edit.type", json!({ "text": "x" })),
            ("problems.file.list", json!({ "path": null })),
            ("spelling.typos.list", json!({})),
            ("navigate.usages.find", json!({ "line": 1, "column": 1 })),
            ("file.document.save", json!({})),
        ];
        let refused: Vec<(String, Result<Value, String>)> =
            calls.into_iter().map(|(id, params)| (id.to_owned(), command(&url, id, params))).collect();
        let windows = call(&url, "list_windows", json!({})).unwrap();
        command(&url, "file.document.open", json!({ "path": main })).unwrap();
        let after = command(&url, "editor.selection.get", json!({}));
        call(&url, "exit_ide", json!({})).unwrap();
        (refused, windows, after)
    });
    assert!(windows.to_string().contains("\"sensitive\":true"), "{windows}");
    for (id, answer) in refused {
        let error = answer.expect_err(&id);
        let error: Value = serde_json::from_str(&error).unwrap_or_else(|_| panic!("{id}: {error}"));
        assert_eq!(error["error"], "redacted", "{id}: {error}");
        assert!(!error.to_string().contains("abc123"), "{id}");
    }
    assert!(after.is_ok(), "{after:?}");
    assert_eq!(std::fs::read_to_string(project.join(".env")).unwrap(), "TOKEN=abc123\n", "nothing typed into it");
}

#[test]
fn a_hidden_file_is_hidden_by_its_real_path_in_any_case_or_link() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(project.join("config")).unwrap();
    std::fs::create_dir_all(project.join("sub")).unwrap();
    std::fs::write(project.join(".env"), "TOKEN=abc123\n").unwrap();
    std::fs::write(project.join(".env.example"), "TOKEN=\n").unwrap();
    std::fs::write(project.join("config/.env.local"), "LOCAL=xyz789\n").unwrap();
    std::fs::write(project.join("main.go"), MAIN).unwrap();
    std::os::unix::fs::symlink(project.join(".env"), project.join("innocent.txt")).unwrap();
    std::os::unix::fs::symlink(project.join("config"), project.join("linkdir")).unwrap();
    let mut workspace = super::super::workspace::Workspace::default();
    let layout = super::super::workspace::ProjectLayout { documents: vec![project.join(".env")], ..Default::default() };
    workspace.projects.insert(project.clone(), layout);
    workspace.save(&data.path().join("workspace.json"));
    let recent = Some(data.path().join("recent-projects.json"));
    let case_insensitive = project.join(".ENV").exists();
    let (answers, sample, found) = with_agent(recent, &project, true, move |url| {
        let mut answers: Vec<(String, Result<Value, String>)> = Vec::new();
        let mut read = |label: &str, path: &str| {
            answers.push((format!("read {label}"), call(&url, "get_document_text", json!({ "path": path }))));
        };
        let mut paths = vec!["./sub/../.env", "innocent.txt", "linkdir/.env.local"];
        if case_insensitive {
            paths.extend([".ENV", ".Env", "config/.ENV.LOCAL"]);
        }
        for path in &paths {
            read(path, path);
        }
        if case_insensitive {
            answers.push(("open .ENV".to_owned(), command(&url, "file.document.open", json!({ "path": ".ENV" }))));
            answers.push((
                "write .ENV".to_owned(),
                call(&url, "editor_text_write", json!({ "path": ".ENV", "text": "TOKEN=gone\n" })),
            ));
            answers.push(("cat .ENV".to_owned(), call(&url, "terminal_write", json!({ "text": "cat .ENV\n" }))));
        }
        answers
            .push(("cat sub/../.env".to_owned(), call(&url, "terminal_write", json!({ "text": "cat sub/../.env\n" }))));
        answers.push((
            "cat innocent.txt".to_owned(),
            call(&url, "terminal_write", json!({ "text": "cat innocent.txt\n" })),
        ));
        for (id, params) in [
            ("editor.selection.selectAll", json!({ "path": "main.go" })),
            ("editor.selection.get", json!({ "path": "main.go" })),
            ("editor.structure.get", json!({ "path": "main.go" })),
            ("markdown.preview.render", json!({ "path": "main.go" })),
        ] {
            answers.push((format!("{id} with a dummy path"), command(&url, id, params)));
        }
        let sample = call(&url, "get_document_text", json!({ "path": ".env.example" }));
        for _ in 0..200 {
            if command(&url, "search.index.status", json!({})).unwrap()["ready"] == true {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let found = command(&url, "search.text.find", json!({ "query": { "text": "abc123" } })).unwrap();
        call(&url, "exit_ide", json!({})).unwrap();
        (answers, sample, found)
    });
    for (label, answer) in &answers {
        let error = answer.as_ref().err().unwrap_or_else(|| panic!("{label} was answered: {answer:?}"));
        assert!(error.contains("redacted"), "{label}: {error}");
        assert!(!error.contains("abc123") && !error.contains("xyz789"), "{label}: {error}");
    }
    assert_eq!(sample.unwrap()["text"], "TOKEN=\n", "a sample stays readable");
    assert_eq!(found["matches"], json!([]), "not through the link either: {found}");
    assert_eq!(std::fs::read_to_string(project.join(".env")).unwrap(), "TOKEN=abc123\n");
}

fn secret_project(data: &Path) -> PathBuf {
    let project = data.canonicalize().unwrap().join("project");
    std::fs::create_dir_all(project.join("config")).unwrap();
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join(".env"), "TOKEN=abc123\n").unwrap();
    std::fs::write(project.join("config/.env.local"), "LOCAL=xyz789\n").unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(project.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(project.join(".env"), project.join("innocent.txt")).unwrap();
    project
}

fn leaks(answer: &Result<Value, String>) -> bool {
    let text = match answer {
        Ok(value) => value.to_string(),
        Err(error) => error.clone(),
    };
    text.contains("abc123") || text.contains("xyz789")
}

#[cfg(unix)]
#[test]
fn comparing_with_a_hidden_file_is_refused_on_any_side() {
    let data = tempfile::tempdir().unwrap();
    let project = secret_project(data.path());
    let case_insensitive = project.join(".ENV").exists();
    let project_root = project.clone();
    let answers = with_agent(None, &project, true, move |url| {
        let absolute = |name: &str| project_root.join(name).to_string_lossy().into_owned();
        let mut hidden: Vec<String> = [".env", "innocent.txt", "config/.env.local"].map(str::to_owned).to_vec();
        hidden.extend([absolute(".env"), absolute("innocent.txt")]);
        if case_insensitive {
            hidden.extend([".ENV".to_owned(), "config/.ENV.LOCAL".to_owned(), absolute(".ENV")]);
        }
        let mut answers = Vec::new();
        for secret in &hidden {
            let secret = secret.as_str();
            for (left, right) in [(secret, "src/main.rs"), ("src/main.rs", secret)] {
                answers.push((
                    format!("files {left} {right}"),
                    command(&url, "file.compare.files", json!({ "left": left, "right": right })),
                ));
            }
            for sides in [
                [secret, "src/main.rs", "src/lib.rs"],
                ["src/main.rs", secret, "src/lib.rs"],
                ["src/main.rs", "src/lib.rs", secret],
            ] {
                answers.push((
                    format!("three {sides:?}"),
                    command(
                        &url,
                        "file.compare.three",
                        json!({ "left": sides[0], "middle": sides[1], "right": sides[2] }),
                    ),
                ));
            }
        }
        call(&url, "exit_ide", json!({})).unwrap();
        answers
    });
    for (label, answer) in &answers {
        assert!(!leaks(answer), "{label}: {answer:?}");
        assert!(answer.as_ref().is_err_and(|error| error.contains("redacted")), "{label}: {answer:?}");
    }
}

#[cfg(unix)]
#[test]
fn no_command_hands_out_a_hidden_file_through_any_string_parameter() {
    let data = tempfile::tempdir().unwrap();
    let project = secret_project(data.path());
    let secret = project.join(".env").to_string_lossy().into_owned();
    let harmless = project.join("src/main.rs").to_string_lossy().into_owned();
    let (tried, leaked) = with_agent(None, &project, true, move |url| {
        let catalogue = call(&url, "list_commands", json!({ "full": true })).unwrap();
        let mut tried = 0;
        let mut leaked = Vec::new();
        for info in catalogue["commands"].as_array().unwrap() {
            let id = info["id"].as_str().unwrap();
            if ["run.", "debug.", "terminal.", "plugins.", "vcs.", "tools.", "tasks.", "file.fs.", "lsp.server.install"]
                .iter()
                .any(|group| id.starts_with(group))
            {
                continue;
            }
            let Some(properties) = info["params"]["properties"].as_object() else { continue };
            let kind_of = |schema: &Value| -> Option<&str> {
                let kinds = &schema["type"];
                let is =
                    |kind: &str| kinds == kind || kinds.as_array().is_some_and(|kinds| kinds.contains(&json!(kind)));
                if is("string") {
                    Some("string")
                } else if is("array") && schema["items"]["type"] == "string" {
                    Some("array")
                } else {
                    None
                }
            };
            let strings: Vec<(&String, &str)> =
                properties.iter().filter_map(|(name, schema)| Some((name, kind_of(schema)?))).collect();
            for (target, _) in &strings {
                let params: serde_json::Map<String, Value> = strings
                    .iter()
                    .map(|(name, kind)| {
                        let path = if name == target { &secret } else { &harmless };
                        let value = if *kind == "array" { json!([path]) } else { json!(path) };
                        ((*name).clone(), value)
                    })
                    .collect();
                tried += 1;
                let answer = command(&url, id, Value::Object(params));
                if leaks(&answer) {
                    leaked.push(format!("{id} {target}: {answer:?}"));
                }
            }
        }
        call(&url, "exit_ide", json!({})).unwrap();
        (tried, leaked)
    });
    assert!(tried > 100, "the catalogue was walked: {tried} parameters");
    assert!(leaked.is_empty(), "{leaked:#?}");
    assert_eq!(std::fs::read_to_string(project.join(".env")).unwrap(), "TOKEN=abc123\n");
}

fn git(folder: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(folder)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn secret_repository(data: &Path) -> PathBuf {
    let project = secret_project(data);
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "init"]);
    std::fs::write(project.join(".env"), "TOKEN=abc123\nNEXT=abc123-next\n").unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() { lib(); }\n").unwrap();
    project
}

#[cfg(unix)]
#[test]
fn patches_leave_hidden_files_out() {
    let data = tempfile::tempdir().unwrap();
    let project = secret_repository(data.path());
    let answers = with_agent(None, &project, true, |url| {
        let changes =
            command(&url, "vcs.patch.create", json!({ "source": "changes", "paths": [".env", "src/main.rs"] }));
        let active = command(&url, "vcs.patch.create", json!({ "source": "changes" }));
        let commit = command(&url, "vcs.patch.create", json!({ "source": "commit", "revision": "HEAD" }));
        let clipboard =
            command(&url, "vcs.patch.create", json!({ "source": "commit", "revision": "HEAD", "clipboard": true }));
        let written =
            command(&url, "vcs.patch.create", json!({ "source": "commit", "revision": "HEAD", "output": "out.patch" }));
        let file = call(&url, "get_document_text", json!({ "path": "out.patch" }));
        call(&url, "exit_ide", json!({})).unwrap();
        vec![
            ("changes naming it", changes),
            ("changes", active),
            ("commit", commit),
            ("clipboard", clipboard),
            ("output", written),
            ("output read", file),
        ]
    });
    let (_, named) = &answers[0];
    assert!(named.as_ref().is_err_and(|error| error.contains("redacted")), "naming it is refused: {named:?}");
    for (label, answer) in &answers[1..] {
        assert!(!leaks(answer), "{label}: {answer:?}");
    }
    let commit = answers[2].1.as_ref().unwrap();
    assert_eq!(commit["hiddenFiles"], 3, ".env, config/.env.local and the link to .env: {commit}");
    let files: Vec<&str> = commit["files"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(files.contains(&"src/main.rs") && !files.iter().any(|file| file.contains("env")), "{files:?}");
    assert!(commit["text"].as_str().unwrap().contains("fn main() {}"), "the other files stay in it");
    let active = answers[1].1.as_ref().unwrap();
    assert!(active["text"].as_str().unwrap().contains("lib();"), "{active}");
    assert_eq!(active["hiddenFiles"], 1, "{active}");
    let read = answers[5].1.as_ref().unwrap();
    assert!(read["text"].as_str().unwrap().contains("fn main() {}"), "{read}");
}

#[cfg(unix)]
#[test]
fn the_ides_own_folder_is_hidden_from_agents() {
    let data = tempfile::tempdir().unwrap();
    let project = secret_project(data.path());
    let ide = data.path().canonicalize().unwrap().join("ide");
    let copies = [
        ide.join("vcs-history/project-1/files/0a1b"),
        ide.join("vcs-shelf/project-1/1.patch"),
        ide.join("local-history/objects/3e/ac55"),
    ];
    for copy in &copies {
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::write(copy, "TOKEN=abc123\n").unwrap();
    }
    std::fs::create_dir_all(ide.join("scratches")).unwrap();
    std::fs::write(ide.join("scratches/note.txt"), "a scratch\n").unwrap();
    std::os::unix::fs::symlink(ide.join("vcs-shelf"), project.join("shelves")).unwrap();
    let paths: Vec<String> = copies.iter().map(|copy| copy.to_string_lossy().into_owned()).collect();
    let scratch = ide.join("scratches/note.txt").to_string_lossy().into_owned();
    let settings = ide.join("settings.json").to_string_lossy().into_owned();
    let answers = with_agent(Some(ide.join("recent-projects.json")), &project, true, move |url| {
        let mut answers = Vec::new();
        for path in &paths {
            answers.push((path.clone(), call(&url, "get_document_text", json!({ "path": path }))));
            answers.push((
                format!("compare {path}"),
                command(&url, "file.compare.files", json!({ "left": path, "right": "src/main.rs" })),
            ));
        }
        answers
            .push(("link".to_owned(), call(&url, "get_document_text", json!({ "path": "shelves/project-1/1.patch" }))));
        answers.push(("settings".to_owned(), call(&url, "get_document_text", json!({ "path": settings }))));
        let scratch = call(&url, "get_document_text", json!({ "path": scratch }));
        call(&url, "exit_ide", json!({})).unwrap();
        (answers, scratch)
    });
    let (answers, scratch) = answers;
    for (label, answer) in &answers {
        assert!(!leaks(answer), "{label}: {answer:?}");
        assert!(answer.as_ref().is_err_and(|error| error.contains("redacted")), "{label}: {answer:?}");
    }
    assert_eq!(scratch.unwrap()["text"], "a scratch\n", "the scratch files stay the agent's");
}

#[cfg(unix)]
#[test]
fn agents_change_no_hidden_file() {
    let data = tempfile::tempdir().unwrap();
    let project = secret_repository(data.path());
    let changed_secret = std::fs::read_to_string(project.join(".env")).unwrap();
    let answers = with_agent(None, &project, true, |url| {
        for _ in 0..200 {
            if command(&url, "search.index.status", json!({})).unwrap()["ready"] == true {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let replaced =
            command(&url, "search.text.replaceAll", json!({ "query": { "text": "abc123" }, "replacement": "gone" }));
        let deleted = command(&url, "file.fs.delete", json!({ "paths": ["config"] }));
        let renamed = command(&url, "file.fs.rename", json!({ "path": "config", "name": "settings" }));
        let rolled = command(&url, "vcs.changes.rollback", json!({ "paths": ["config"] }));
        let created = command(&url, "file.fs.newFile", json!({ "directory": ".", "name": ".env.production" }));
        let stash = command(&url, "vcs.stash.create", json!({ "message": "agent" }));
        let popped = command(&url, "vcs.stash.pop", json!({}));
        let committed = command(&url, "vcs.commit.create", json!({ "message": "all", "changelist": "Changes" }));
        call(&url, "exit_ide", json!({})).unwrap();
        vec![replaced, deleted, renamed, rolled, created, stash, popped, committed]
    });
    let [replaced, deleted, renamed, rolled, created, stash, popped, committed] = &answers[..] else { unreachable!() };
    assert!(replaced.as_ref().is_ok_and(|done| done["hiddenFiles"].as_u64() >= Some(1)), "{replaced:?}");
    for (label, refused) in [("delete", deleted), ("rename", renamed), ("rollback", rolled), ("new file", created)] {
        assert!(refused.as_ref().is_err_and(|error| error.contains("redacted")), "{label}: {refused:?}");
    }
    let stash = stash.as_ref().unwrap();
    assert_eq!(stash["hiddenFiles"], 1, "the stash leaves the secret's change where it is: {stash}");
    assert!(popped.is_ok(), "{popped:?}");
    assert!(committed.is_ok(), "{committed:?}");
    assert!(project.join("config/.env.local").exists() && !project.join(".env.production").exists());
    assert_eq!(std::fs::read_to_string(project.join(".env")).unwrap(), changed_secret, "the secret is as it was");
    assert_eq!(git(&project, &["status", "--porcelain", "--", ".env"]).trim(), "M .env", "and still not committed");
    assert!(git(&project, &["log", "-1", "--name-only", "--format="]).contains("src/main.rs"));
}

#[cfg(unix)]
#[test]
fn copies_and_revision_paths_keep_hidden_files_hidden() {
    let data = tempfile::tempdir().unwrap();
    let project = secret_project(data.path());
    std::fs::create_dir_all(project.join("deploy/vault")).unwrap();
    std::fs::write(project.join("deploy/vault/key.txt"), "VAULT=abc123\n").unwrap();
    std::fs::write(project.join("deploy/readme.md"), "deploy\n").unwrap();
    std::fs::write(project.join("notes.txt"), "notes\n").unwrap();
    let ide = data.path().canonicalize().unwrap().join("ide");
    std::fs::create_dir_all(&ide).unwrap();
    std::fs::write(ide.join("settings.json"), r#"{"mcpRedactFiles": [".env", ".env.*", "deploy/vault"]}"#).unwrap();
    let answers = with_agent(Some(ide.join("recent-projects.json")), &project, true, |url| {
        let answers = vec![
            (
                "copy a folder holding one",
                command(&url, "file.fs.copyInto", json!({ "sources": ["deploy"], "directory": "src" })),
            ),
            (
                "copy into a hidden place",
                command(&url, "file.fs.copyInto", json!({ "sources": ["notes.txt"], "directory": "deploy/vault" })),
            ),
            (
                "clipboard copy of the folder",
                command(&url, "project.file.copy", json!({ "paths": ["deploy"], "clipboard": false })),
            ),
            (
                "paste of the folder",
                command(
                    &url,
                    "project.file.paste",
                    json!({ "sources": ["deploy"], "directory": "src", "clipboard": false }),
                ),
            ),
            ("git show", call(&url, "terminal_write", json!({ "text": "git show HEAD:.env\n" }))),
            (
                "git show of a folder pattern",
                call(&url, "terminal_write", json!({ "text": "git show main:deploy/vault/key.txt\n" })),
            ),
        ];
        let copied = command(&url, "project.file.copy", json!({ "paths": ["notes.txt"], "clipboard": false }));
        let pasted = command(&url, "project.file.paste", json!({ "directory": "src", "clipboard": false }));
        call(&url, "exit_ide", json!({})).unwrap();
        (answers, copied, pasted)
    });
    let (answers, copied, pasted) = answers;
    for (label, answer) in &answers {
        assert!(!leaks(answer), "{label}: {answer:?}");
        assert!(answer.as_ref().is_err_and(|error| error.contains("redacted")), "{label}: {answer:?}");
    }
    assert!(copied.is_ok(), "a relative path is the project's: {copied:?}");
    assert!(pasted.is_ok(), "{pasted:?}");
    assert!(project.join("src/notes.txt").exists() && !project.join("src/deploy").exists());
    assert!(!project.join("deploy/vault/notes.txt").exists());
}

#[cfg(unix)]
#[test]
fn an_agents_discard_of_everything_keeps_untracked_files() {
    let data = tempfile::tempdir().unwrap();
    let project = secret_repository(data.path());
    std::fs::write(project.join("draft.txt"), "an untracked draft\n").unwrap();
    std::os::unix::fs::symlink(project.join("src/lib.rs"), project.join("lib-link.rs")).unwrap();
    let changed_secret = std::fs::read_to_string(project.join(".env")).unwrap();
    let discarded = with_agent(None, &project, true, |url| {
        let discarded = command(&url, "vcs.changes.discard", json!({ "all": true }));
        call(&url, "exit_ide", json!({})).unwrap();
        discarded
    });
    let discarded = discarded.unwrap();
    assert_eq!(discarded["untrackedKept"], 2, "{discarded}");
    assert_eq!(
        std::fs::read_to_string(project.join("src/main.rs")).unwrap(),
        "fn main() {}\n",
        "a tracked change is reverted"
    );
    assert!(project.join("draft.txt").exists(), "an untracked file stays");
    assert!(std::fs::symlink_metadata(project.join("lib-link.rs")).is_ok(), "an untracked link stays");
    assert_eq!(std::fs::read_to_string(project.join(".env")).unwrap(), changed_secret, "a hidden file is left alone");
}

fn maven_plugin() -> zen_workbench::plugin_projects::PluginProject {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/maven/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    zen_workbench::plugin_projects::PluginProject {
        id: "maven".to_owned(),
        name: "Maven".to_owned(),
        support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
    }
}

#[cfg(unix)]
#[test]
fn an_agent_opens_the_maven_tool_window_of_a_headless_ide_and_runs_a_goal_from_it() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>shop</artifactId><version>1</version></project>",
    )
    .unwrap();
    let wrapper = project.join("mvnw");
    std::fs::write(&wrapper, "#!/bin/sh\necho \"maven was asked: $*\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    app.enable_automation(0);
    app.with_session(|session| {
        session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![maven_plugin()]);
    })
    .expect("the project is open");
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");

    let agent = std::thread::spawn(move || {
        let state = || call(&url, "get_window_state", json!({})).unwrap();
        let action =
            |action: &str, params: Value| call(&url, "run_action", json!({ "action": action, "params": params }));
        let deadline = Instant::now() + Duration::from_secs(60);
        while state()["buildTools"]["maven"]["available"] != true {
            assert!(Instant::now() < deadline, "the Maven tool window never applied: {}", state());
            std::thread::sleep(Duration::from_millis(50));
        }
        let opened = action("view.toolWindow.maven", json!({})).unwrap();
        let rows = |state: &Value| state["buildTools"]["maven"]["rows"].as_array().cloned().unwrap_or_default();
        let id_of = |rows: &[Value], label: &str| {
            rows.iter().find(|row| row["label"] == label).map(|row| row["id"].as_str().unwrap().to_owned())
        };
        let lifecycle = id_of(&rows(&opened["window"]), "Lifecycle").expect("the Lifecycle row");
        let expanded = action("view.buildTool.toggle", json!({ "tool": "maven", "row": lifecycle })).unwrap();
        let compile = id_of(&rows(&expanded["window"]), "compile").expect("the compile phase under it");
        let ran = action("view.buildTool.run", json!({ "tool": "maven", "row": compile })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let output = loop {
            let runs = command(&url, "run.process.list", json!({})).unwrap();
            let run = runs["runs"].as_array().and_then(|runs| runs.last().cloned());
            if let Some(run) = run.filter(|run| run["state"] != "running") {
                break command(&url, "run.output.get", json!({ "runId": run["runId"] })).unwrap();
            }
            assert!(Instant::now() < deadline, "the goal never ran: {runs}");
            std::thread::sleep(Duration::from_millis(50));
        };
        let menu = action("view.buildTool.menu", json!({ "tool": "maven", "row": compile })).unwrap();
        let searched = action("view.buildTool.search", json!({ "tool": "maven", "text": "COMP" })).unwrap();
        let executed = action("view.maven.executeGoal", json!({ "goals": "clean install" })).unwrap();
        let run_id = executed["result"]["runId"].clone();
        let deadline = Instant::now() + Duration::from_secs(60);
        let executed_output = loop {
            let output = command(&url, "run.output.get", json!({ "runId": run_id })).unwrap();
            if output["state"] != "running" {
                break output;
            }
            assert!(Instant::now() < deadline, "Execute Maven Goal never ran: {output}");
            std::thread::sleep(Duration::from_millis(50));
        };
        let keys = call(&url, "press_keys", json!({ "keys": ["⎋"] }));
        let exit = call(&url, "exit_ide", json!({})).unwrap();
        (opened, ran, output, keys, exit, (menu, searched, executed_output))
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(120);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (opened, ran, output, keys, exit, (menu, searched, executed)) = agent.join().unwrap();
    let items = menu["result"]["items"].as_array().cloned().unwrap_or_default();
    assert!(items.iter().any(|item| item["command"] == "maven.goal.debug"), "{menu}");
    assert_eq!(searched["result"]["selected"].as_str().map(|id| id.ends_with("/compile")), Some(true), "{searched}");
    assert!(executed.to_string().contains("maven was asked: clean install"), "{executed}");

    assert_eq!(ctx.cumulative_pass_nr(), 0, "nothing was laid out or drawn");
    assert_eq!(opened["window"]["right"], "maven", "{opened}");
    assert_eq!(ran["result"]["selected"].as_str().map(|id| id.ends_with("/compile")), Some(true), "{ran}");
    assert!(output.to_string().contains("maven was asked: compile"), "{output}");
    assert!(keys.unwrap_err().contains("not available in a headless IDE"), "keys need a drawn window");
    assert_eq!(exit["exiting"], true, "{exit}");
}

#[cfg(unix)]
#[test]
fn an_agent_uses_the_gradle_tool_window_of_a_headless_ide() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("settings.gradle"), "rootProject.name = 'shop'\n").unwrap();
    let wrapper = project.join("gradlew");
    let script = r#"#!/bin/sh
for arg in "$@"; do
  case "$arg" in -Dzen.model.out=*) out="${arg#-Dzen.model.out=}" ;; esac
done
if [ -n "$out" ]; then
  mkdir -p "$out"
  echo '{"build":":","path":":","name":"shop","folder":"/p","tasks":[{"name":"assemble","group":"build"}]}' > "$out/root.json"
  exit 0
fi
echo "gradle was asked: $*"
"#;
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    app.enable_automation(0);
    let plugin = {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/gradle/plugin.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        zen_workbench::plugin_projects::PluginProject {
            id: "gradle".to_owned(),
            name: "Gradle".to_owned(),
            support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
        }
    };
    let granted = project.clone();
    app.with_session(move |session| {
        session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![plugin]);
        registry().dispatch(session, "project.trust.grant", json!({ "path": granted })).unwrap();
    })
    .expect("the project is open");
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");

    let agent = std::thread::spawn(move || {
        let state = || call(&url, "get_window_state", json!({})).unwrap();
        let action =
            |action: &str, params: Value| call(&url, "run_action", json!({ "action": action, "params": params }));
        let deadline = Instant::now() + Duration::from_secs(60);
        while state()["buildTools"]["gradle"]["available"] != true {
            assert!(Instant::now() < deadline, "the Gradle tool window never applied: {}", state());
            std::thread::sleep(Duration::from_millis(50));
        }
        action("view.toolWindow.gradle", json!({})).unwrap();
        let rows = || state()["buildTools"]["gradle"]["rows"].as_array().cloned().unwrap_or_default();
        while !rows().iter().any(|row| row["id"] == "@:/tasks") {
            assert!(Instant::now() < deadline, "the tasks never came: {}", state());
            std::thread::sleep(Duration::from_millis(50));
        }
        let menu = action("view.buildTool.menu", json!({ "tool": "gradle", "row": "@:" })).unwrap();
        let searched = action("view.buildTool.search", json!({ "tool": "gradle", "text": "TASK" })).unwrap();
        let popup = action("view.gradle.executeTask", json!({})).unwrap();
        let executed = action("view.gradle.executeTask", json!({ "tasks": "clean :assemble" })).unwrap();
        let run_id = executed["result"]["runId"].clone();
        let output = loop {
            let output = command(&url, "run.output.get", json!({ "runId": run_id })).unwrap();
            if output["state"] != "running" {
                break output;
            }
            assert!(Instant::now() < deadline, "Execute Gradle Task never ran: {output}");
            std::thread::sleep(Duration::from_millis(50));
        };
        call(&url, "exit_ide", json!({})).unwrap();
        (menu, searched, popup, output)
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(120);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (menu, searched, popup, output) = agent.join().unwrap();
    let items = menu["result"]["items"].as_array().cloned().unwrap_or_default();
    assert!(items.iter().any(|item| item["command"] == "gradle.project.reload"), "{menu}");
    assert_eq!(searched["result"]["selected"], "@:/tasks", "{searched}");
    assert_eq!(popup["result"]["executeTask"]["projectPath"], ".", "{popup}");
    assert!(output.to_string().contains("gradle was asked: clean :assemble"), "{output}");
}

fn ant_plugin() -> zen_workbench::plugin_projects::PluginProject {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/ant/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    zen_workbench::plugin_projects::PluginProject {
        id: "ant".to_owned(),
        name: "Ant".to_owned(),
        support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
    }
}

#[test]
fn an_agent_opens_the_npm_tool_window_and_reads_the_packages_of_a_headless_ide() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("package.json"),
        "{\n  \"name\": \"shop\",\n  \"scripts\": { \"dev\": \"vite\", \"build\": \"vite build\" }\n}\n",
    )
    .unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    app.enable_automation(0);
    app.with_session(|session| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/npm/plugin.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![
            zen_workbench::plugin_projects::PluginProject {
                id: "npm".to_owned(),
                name: "npm".to_owned(),
                support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
            },
        ]);
    })
    .expect("the project is open");
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");

    let agent = std::thread::spawn(move || {
        let state = || call(&url, "get_window_state", json!({})).unwrap();
        let action =
            |action: &str, params: Value| call(&url, "run_action", json!({ "action": action, "params": params }));
        let deadline = Instant::now() + Duration::from_secs(60);
        while state()["buildTools"]["npm"]["available"] != true {
            assert!(Instant::now() < deadline, "the npm tool window never applied: {}", state());
            std::thread::sleep(Duration::from_millis(50));
        }
        let opened = action("view.toolWindow.npm", json!({})).unwrap();
        let offline = command(&url, "npm.options.set", json!({ "offline": true })).unwrap();
        let selected = action("view.buildTool.select", json!({ "tool": "npm", "row": "package.json/dev" })).unwrap();
        let toggled = action("view.buildTool.toggle", json!({ "tool": "npm", "row": "package.json" })).unwrap();
        let exit = call(&url, "exit_ide", json!({})).unwrap();
        (opened, offline, selected, toggled, exit)
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(120);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (opened, offline, selected, toggled, exit) = agent.join().unwrap();

    assert_eq!(opened["window"]["right"], "npm", "{opened}");
    let rows = opened["window"]["buildTools"]["npm"]["rows"].as_array().unwrap();
    let labels: Vec<&str> = rows.iter().map(|row| row["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["shop", "dev", "build"], "{opened}");
    assert_eq!(rows[1]["runs"], "script");
    assert_eq!(rows[1]["detail"], "vite");
    assert_eq!(offline["offline"], true, "{offline}");
    assert_eq!(selected["result"]["selected"], "package.json/dev", "{selected}");
    let after = toggled["window"]["buildTools"]["npm"]["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!((after.len(), &after[0]["open"]), (1, &json!(false)), "{toggled}");
    assert_eq!(exit["exiting"], true, "{exit}");
}

#[test]
fn an_agent_lists_filters_and_opens_ant_build_files_in_a_headless_ide() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("build.xml"),
        "<project name=\"shop\" default=\"jar\">\n  <target name=\"init\"/>\n  <target name=\"compile\" description=\"Compiles\"/>\n  <target name=\"jar\"/>\n</project>\n",
    )
    .unwrap();
    let mut app = IdeApp::build(product(), registry(), None, vec![project.clone()], true);
    app.enable_automation(0);
    app.with_session(|session| {
        session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![ant_plugin()]);
    })
    .expect("the project is open");
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let url = app.automation_url().expect("the server listens");

    let agent = std::thread::spawn(move || {
        let state = || call(&url, "get_window_state", json!({})).unwrap();
        let action =
            |action: &str, params: Value| call(&url, "run_action", json!({ "action": action, "params": params }));
        let deadline = Instant::now() + Duration::from_secs(60);
        while state()["buildTools"]["ant"]["available"] != true {
            assert!(Instant::now() < deadline, "the Ant tool window never applied: {}", state());
            std::thread::sleep(Duration::from_millis(50));
        }
        let opened = action("view.toolWindow.ant", json!({})).unwrap();
        let filtered = command(&url, "ant.options.set", json!({ "filterTargets": true })).unwrap();
        let selected = action("view.buildTool.select", json!({ "tool": "ant", "row": "build.xml/jar" })).unwrap();
        let rows = state()["buildTools"]["ant"]["rows"].clone();
        let toggled = action("view.buildTool.toggle", json!({ "tool": "ant", "row": "build.xml" })).unwrap();
        let shown = action("view.buildTool.run", json!({ "tool": "ant", "row": "build.xml" })).unwrap();
        let exit = call(&url, "exit_ide", json!({})).unwrap();
        (opened, filtered, selected, rows, toggled, shown, exit)
    });
    let started = Instant::now();
    let mut gone = || agent.is_finished() || started.elapsed() > Duration::from_secs(120);
    run_loop(&mut app, &ctx, &wake, &mut gone);
    let (opened, filtered, selected, rows, toggled, shown, exit) = agent.join().unwrap();

    assert_eq!(opened["window"]["right"], "ant", "{opened}");
    let labels: Vec<&str> = opened["window"]["buildTools"]["ant"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["shop", "init", "compile", "jar"], "{opened}");
    assert_eq!(filtered["filterTargets"], true, "{filtered}");
    assert_eq!(selected["result"]["selected"], "build.xml/jar", "{selected}");
    let ids: Vec<&str> = rows.as_array().unwrap().iter().map(|row| row["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["build.xml", "build.xml/compile", "build.xml/jar"], "only described and default targets: {rows}");
    assert_eq!(rows[2]["detail"], "(default)");
    assert_eq!(rows[1]["runs"], "target");
    let after: Vec<Value> = toggled["window"]["buildTools"]["ant"]["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!((after.len(), &after[0]["open"]), (1, &json!(false)), "{toggled}");
    assert_eq!(shown["result"]["selected"], "build.xml", "{shown}");
    assert_eq!(exit["exiting"], true, "{exit}");
}

#[test]
fn zen_automation_one_means_the_default_port() {
    assert_eq!(automation_port("1", 63531), Some(63531));
    assert_eq!(automation_port("true", 63531), Some(63531));
    assert_eq!(automation_port("64000", 63531), Some(64000));
    assert_eq!(automation_port("0", 63531), None);
    assert_eq!(automation_port("", 63531), None);
}

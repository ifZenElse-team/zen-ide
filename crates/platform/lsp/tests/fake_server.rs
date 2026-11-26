use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zen_lsp::connection::{read_message, write_message};
use zen_lsp::{Client, Connection, Edit, Event, Launched, LspError};

type Log = Arc<Mutex<Vec<Vec<Value>>>>;

fn fake(log: &Log, sync_kind: u32) -> Launched {
    fake_with(log, sync_kind, true)
}

fn fake_with(log: &Log, sync_kind: u32, folders: bool) -> Launched {
    let (from_client, to_server) = std::io::pipe().unwrap();
    let (from_server, to_client) = std::io::pipe().unwrap();
    let index = {
        let mut log = log.lock().unwrap();
        log.push(Vec::new());
        log.len() - 1
    };
    let log = Arc::clone(log);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(from_client);
        let mut writer = to_client;
        let mut slow: Option<Value> = None;
        while let Ok(Some(message)) = read_message(&mut reader) {
            log.lock().unwrap()[index].push(message.clone());
            let id = message.get("id").cloned();
            let reply = |writer: &mut std::io::PipeWriter, result: Value| {
                write_message(writer, &json!({ "jsonrpc": "2.0", "id": id, "result": result })).unwrap();
            };
            match message["method"].as_str().unwrap_or_default() {
                "initialize" => reply(
                    &mut writer,
                    json!({ "capabilities": {
                        "textDocumentSync": { "openClose": true, "change": sync_kind, "save": { "includeText": true } },
                        "workspace": { "workspaceFolders": { "supported": folders, "changeNotifications": folders } }
                    } }),
                ),
                "initialized" => write_message(
                    &mut writer,
                    &json!({ "jsonrpc": "2.0", "id": "server-1", "method": "workspace/configuration", "params": { "items": [{ "section": "gopls" }] } }),
                )
                .unwrap(),
                "textDocument/didOpen" | "textDocument/didChange" => {
                    let document = &message["params"]["textDocument"];
                    write_message(
                        &mut writer,
                        &json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": document["uri"], "version": document["version"], "diagnostics": [] } }),
                    )
                    .unwrap();
                }
                "workspace/didChangeConfiguration" => write_message(
                    &mut writer,
                    &json!({ "jsonrpc": "2.0", "id": "server-1", "method": "workspace/configuration", "params": { "items": [{ "section": "gopls" }, { "section": "other" }] } }),
                )
                .unwrap(),
                "test/refresh" => write_message(
                    &mut writer,
                    &json!({ "jsonrpc": "2.0", "id": "server-refresh", "method": "workspace/inlayHint/refresh" }),
                )
                .unwrap(),
                "test/diagnosticRefresh" => write_message(
                    &mut writer,
                    &json!({ "jsonrpc": "2.0", "id": "server-diagnostics", "method": "workspace/diagnostic/refresh" }),
                )
                .unwrap(),
                "test/echo" => reply(&mut writer, message["params"].clone()),
                "test/slow" => slow = id.clone(),
                "$/cancelRequest" if slow.is_some() && message["params"]["id"] == slow.clone().unwrap() => {
                    write_message(
                        &mut writer,
                        &json!({ "jsonrpc": "2.0", "id": slow.take(), "error": { "code": -32800, "message": "cancelled" } }),
                    )
                    .unwrap();
                }
                "test/crash" | "exit" => return,
                "shutdown" => reply(&mut writer, Value::Null),
                _ => {}
            }
        }
    });
    Launched { connection: Connection::new(from_server, to_server), process: None }
}

fn client(sync_kind: u32) -> (Client, Log, Arc<AtomicUsize>) {
    client_with(sync_kind, true)
}

fn client_with(sync_kind: u32, folders: bool) -> (Client, Log, Arc<AtomicUsize>) {
    let log: Log = Arc::default();
    let launches = Arc::new(AtomicUsize::new(0));
    let (launcher_log, launcher_launches) = (Arc::clone(&log), Arc::clone(&launches));
    let client = Client::start(
        Box::new(move || {
            launcher_launches.fetch_add(1, Ordering::SeqCst);
            Ok(fake_with(&launcher_log, sync_kind, folders))
        }),
        Path::new("/projects/shop"),
        json!({}),
    )
    .unwrap();
    (client, log, launches)
}

fn received(log: &Log, method: &str, count: usize) -> Vec<Value> {
    for _ in 0..200 {
        let found: Vec<Value> = log
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .iter()
            .filter(|message| message["method"] == method || (method == "reply" && message["id"] == "server-1"))
            .cloned()
            .collect();
        if found.len() >= count {
            return found;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the server did not receive {count} × {method}");
}

#[test]
fn initialises_with_capabilities_and_answers_the_server() {
    let (mut client, log, _) = client(2);
    let initialize = &received(&log, "initialize", 1)[0];
    assert_eq!(initialize["params"]["rootUri"], "file:///projects/shop");
    assert_eq!(initialize["params"]["capabilities"]["general"]["positionEncodings"], json!(["utf-16"]));
    assert!(client.capabilities().unwrap().text_document_sync.is_some());
    received(&log, "initialized", 1);
    assert_eq!(received(&log, "reply", 1)[0]["result"], json!([null]));
    let echo = client.request("test/echo", json!({ "a": 1 })).unwrap();
    assert_eq!(echo.wait(Duration::from_secs(5)).unwrap(), json!({ "a": 1 }));
    client.shutdown();
    received(&log, "exit", 1);
}

#[test]
fn a_folder_outside_the_project_joins_the_workspace_folders_once_and_after_a_restart() {
    let (mut client, log, launches) = client(2);
    let initialize = &received(&log, "initialize", 1)[0];
    assert_eq!(initialize["params"]["workspaceFolders"], json!([{ "uri": "file:///projects/shop", "name": "shop" }]));
    client.add_workspace_folder(Path::new("/data/scratches")).unwrap();
    client.add_workspace_folder(Path::new("/data/scratches")).unwrap();
    client.add_workspace_folder(Path::new("/projects/shop")).unwrap();
    let change = &received(&log, "workspace/didChangeWorkspaceFolders", 1)[0];
    let added = json!([{ "uri": "file:///data/scratches", "name": "scratches" }]);
    assert_eq!(change["params"]["event"], json!({ "added": added, "removed": [] }));
    let _ = client.request("test/echo", json!({})).unwrap().wait(Duration::from_secs(5));
    let changes = log
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .iter()
        .filter(|m| m["method"] == "workspace/didChangeWorkspaceFolders")
        .count();
    assert_eq!(changes, 1, "each folder is added once");
    let _ = client.request("test/crash", json!({}));
    for _ in 0..200 {
        client.poll();
        if launches.load(Ordering::SeqCst) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let initialize = &received(&log, "initialize", 1)[0];
    let folders = json!([{ "uri": "file:///projects/shop", "name": "shop" }, { "uri": "file:///data/scratches", "name": "scratches" }]);
    assert_eq!(initialize["params"]["workspaceFolders"], folders, "a restarted server starts with it");
}

#[test]
fn a_server_without_workspace_folders_never_gets_one_from_outside_the_project() {
    let (mut client, log, launches) = client_with(2, false);
    received(&log, "initialize", 1);
    client.add_workspace_folder(Path::new("/data/scratches")).unwrap();
    let _ = client.request("test/echo", json!({})).unwrap().wait(Duration::from_secs(5));
    let changes = log
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .iter()
        .filter(|m| m["method"] == "workspace/didChangeWorkspaceFolders")
        .count();
    assert_eq!(changes, 0);
    let _ = client.request("test/crash", json!({}));
    for _ in 0..200 {
        client.poll();
        if launches.load(Ordering::SeqCst) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let initialize = &received(&log, "initialize", 1)[0];
    assert_eq!(initialize["params"]["workspaceFolders"], json!([{ "uri": "file:///projects/shop", "name": "shop" }]));
}

#[test]
fn the_settings_answer_the_servers_questions_and_their_changes_are_announced() {
    let (mut client, log, launches) = client(2);
    assert_eq!(received(&log, "reply", 1)[0]["result"], json!([null]));
    let settings = json!({ "hints": { "parameterNames": true } });
    client.set_configuration("gopls", settings.clone()).unwrap();
    let change = &received(&log, "workspace/didChangeConfiguration", 1)[0];
    assert_eq!(change["params"]["settings"]["gopls"], settings);
    assert_eq!(received(&log, "reply", 2)[1]["result"], json!([settings, null]));
    assert_eq!(client.configuration("gopls"), Some(&settings));
    let _ = client.request("test/crash", json!({}));
    for _ in 0..200 {
        client.poll();
        if launches.load(Ordering::SeqCst) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(received(&log, "reply", 1)[0]["result"], json!([settings]), "a restarted server is answered the same");
}

#[test]
fn settings_given_at_the_start_answer_the_first_question() {
    let log: Log = Arc::default();
    let launcher_log = Arc::clone(&log);
    let settings = json!({ "hints": { "parameterNames": true } });
    let _client = Client::start_logged(
        Box::new(move || Ok(fake(&launcher_log, 2))),
        Path::new("/projects/shop"),
        json!({}),
        None,
        [("gopls".to_owned(), settings.clone())].into(),
    )
    .unwrap();
    assert_eq!(received(&log, "reply", 1)[0]["result"], json!([settings]));
}

#[test]
fn an_inlay_hint_refresh_is_answered_and_passed_on() {
    let (mut client, log, _) = client(2);
    let _ = client.request("test/refresh", json!({})).unwrap();
    let mut events = Vec::new();
    for _ in 0..200 {
        events.extend(client.poll());
        if !events.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(events.contains(&Event::Notification {
        method: zen_lsp::connection::INLAY_HINT_REFRESH.to_owned(),
        params: Value::Null
    }));
    let answered = log.lock().unwrap().last().unwrap().iter().any(|message| message["id"] == "server-refresh");
    assert!(answered);
}

#[test]
fn a_diagnostic_refresh_is_answered_and_passed_on_and_a_reopen_keeps_the_version_going() {
    let (mut client, log, _) = client(2);
    let uri = "file:///projects/app/src/lib.rs";
    client.did_open(uri, "rust", "fn a() {}\n").unwrap();
    let _ = client.request("test/diagnosticRefresh", json!({})).unwrap();
    let mut events = Vec::new();
    for _ in 0..200 {
        events.extend(client.poll());
        if events.iter().any(|event| matches!(event, Event::Notification { method, .. } if method == zen_lsp::connection::DIAGNOSTIC_REFRESH)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(events.contains(&Event::Notification {
        method: zen_lsp::connection::DIAGNOSTIC_REFRESH.to_owned(),
        params: Value::Null
    }));
    client.reopen(uri).unwrap();
    let opened = received(&log, "textDocument/didOpen", 2);
    assert_eq!(opened[1]["params"]["textDocument"]["version"], 2);
    assert_eq!(opened[1]["params"]["textDocument"]["text"], "fn a() {}\n");
    assert_eq!(received(&log, "textDocument/didClose", 1).len(), 1);
    assert_eq!(client.version(uri), Some(2));
    let answered = log.lock().unwrap().last().unwrap().iter().any(|message| message["id"] == "server-diagnostics");
    assert!(answered);
}

#[test]
fn documents_are_synchronised_incrementally() {
    let (mut client, log, _) = client(2);
    let uri = "file:///projects/shop/main.go";
    client.did_open(uri, "go", "hello\nwörld\n").unwrap();
    let (version, _) = client
        .did_change(uri, &[Edit { range: 7..9, text: "o".to_owned() }, Edit { range: 0..5, text: "bye".to_owned() }])
        .unwrap();
    assert_eq!(version, 2);
    let change = &received(&log, "textDocument/didChange", 1)[0]["params"];
    assert_eq!(change["textDocument"]["version"], 2);
    assert_eq!(
        change["contentChanges"][0]["range"],
        json!({ "start": { "line": 1, "character": 1 }, "end": { "line": 1, "character": 2 } })
    );
    assert_eq!(
        change["contentChanges"][1]["range"],
        json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } })
    );
    client.did_save(uri).unwrap();
    assert_eq!(received(&log, "textDocument/didSave", 1)[0]["params"]["text"], "bye\nworld\n");
    std::thread::sleep(Duration::from_millis(50));
    let events = client.poll();
    assert!(events.iter().any(
        |event| matches!(event, Event::Notification { method, .. } if method == "textDocument/publishDiagnostics")
    ));
    client.did_close(uri).unwrap();
    received(&log, "textDocument/didClose", 1);
}

#[test]
fn a_server_without_incremental_sync_gets_the_whole_text() {
    let (mut client, log, _) = client(1);
    let uri = "file:///projects/shop/a.go";
    client.did_open(uri, "go", "abc").unwrap();
    client.did_change(uri, &[Edit { range: 1..2, text: "X".to_owned() }]).unwrap();
    assert_eq!(received(&log, "textDocument/didChange", 1)[0]["params"]["contentChanges"], json!([{ "text": "aXc" }]));
}

#[test]
fn outdated_requests_are_cancelled_and_recognised() {
    let (mut client, log, _) = client(2);
    let slow = client.request("test/slow", json!({})).unwrap();
    assert_eq!(slow.wait(Duration::from_millis(50)), Err(LspError::Timeout));
    client.cancel(&slow);
    assert!(matches!(slow.wait(Duration::from_secs(5)), Err(LspError::Server { code: -32800, .. })));
    received(&log, "$/cancelRequest", 1);

    let uri = "file:///projects/shop/main.go";
    client.did_open(uri, "go", "x").unwrap();
    let request = client.document_request(uri, "test/echo", json!({})).unwrap();
    assert!(!client.is_stale(&request));
    client.did_change(uri, &[Edit { range: 0..1, text: "y".to_owned() }]).unwrap();
    assert!(client.is_stale(&request), "the document changed since");
}

#[test]
fn a_crashed_server_restarts_with_the_documents_then_gives_up() {
    let (mut client, log, launches) = client(2);
    let uri = "file:///projects/shop/main.go";
    client.did_open(uri, "go", "package main\n").unwrap();
    for attempt in 1..=3 {
        let _ = client.request("test/crash", json!({}));
        let mut events = Vec::new();
        for _ in 0..200 {
            events = client.poll();
            if !events.is_empty() && events.iter().any(|event| !matches!(event, Event::Notification { .. })) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(events.contains(&Event::Restarted { attempt }), "{events:?}");
        assert_eq!(launches.load(Ordering::SeqCst), attempt + 1);
        assert_eq!(received(&log, "textDocument/didOpen", 1)[0]["params"]["textDocument"]["text"], "package main\n");
    }
    let _ = client.request("test/crash", json!({}));
    let mut gave_up = false;
    for _ in 0..200 {
        if client.poll().iter().any(|event| matches!(event, Event::GaveUp { .. })) {
            gave_up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(gave_up, "a fourth crash in the window stops the restarts");
    assert_eq!(launches.load(Ordering::SeqCst), 4);
    assert_eq!(client.request("test/echo", json!({})).err(), Some(LspError::Closed));
}

fn gopls() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let mut candidates = vec![home.join("go/bin/gopls")];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|folder| folder.join("gopls")));
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[test]
#[ignore = "needs gopls"]
fn gopls_reports_an_error() {
    let Some(gopls) = gopls() else {
        assert!(std::env::var_os("ZEN_LSP_REQUIRE_GOPLS").is_none(), "gopls is not installed");
        return;
    };
    let folder = std::env::temp_dir().join(format!("zen-lsp-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("go.mod"), "module example.com/smoke\n\ngo 1.22\n").unwrap();
    let text = "package main\n\nfunc main() {\n\tundefinedName()\n}\n";
    std::fs::write(folder.join("main.go"), text).unwrap();
    let folder = folder.canonicalize().unwrap();
    let mut client =
        Client::start(zen_lsp::process_launcher(gopls, Vec::new(), folder.clone(), Vec::new()), &folder, json!({}))
            .unwrap();
    let uri = zen_lsp::position::file_uri(&folder.join("main.go"));
    client.did_open(&uri, "go", text).unwrap();
    let mut diagnostics = Value::Null;
    for _ in 0..600 {
        for event in client.poll() {
            if let Event::Notification { method, params } = event
                && method == "textDocument/publishDiagnostics"
                && params["uri"] == uri.as_str()
                && params["diagnostics"].as_array().is_some_and(|list| !list.is_empty())
            {
                diagnostics = params["diagnostics"].clone();
            }
        }
        if !diagnostics.is_null() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    client.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
    let message = diagnostics[0]["message"].as_str().unwrap_or_default();
    assert!(message.contains("undefinedName"), "{diagnostics}");
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 3);
}

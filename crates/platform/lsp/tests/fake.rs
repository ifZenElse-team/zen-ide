use std::path::Path;
use std::time::Duration;

use serde_json::json;
use zen_lsp::fake::{FakeServer, Reply};
use zen_lsp::lsp_types::FileChangeType;
use zen_lsp::{Client, LspError};

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn answers_as_scripted_and_records_requests() {
    let server = FakeServer::new();
    server.on("textDocument/definition", json!([{ "uri": "file:///a.go", "range": null }]));
    server.on_with("test/echo", |params| params["value"].clone());
    server.reply("test/broken", Reply::Error { code: -32603, message: "broken".to_owned() });
    let mut client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    assert!(client.capabilities().unwrap().implementation_provider.is_some());
    let initialize = &server.wait_for("initialize", 1, WAIT)[0];
    let capabilities = &initialize["capabilities"]["textDocument"];
    assert_eq!(capabilities["implementation"]["linkSupport"], true);
    assert_eq!(capabilities["typeDefinition"]["linkSupport"], true);
    assert_eq!(
        capabilities["signatureHelp"]["signatureInformation"]["parameterInformation"]["labelOffsetSupport"],
        true
    );
    assert_eq!(capabilities["signatureHelp"]["signatureInformation"]["activeParameterSupport"], true);
    assert_eq!(capabilities["signatureHelp"]["contextSupport"], true);
    assert_eq!(capabilities["documentHighlight"], json!({ "dynamicRegistration": false }));
    assert!(client.document_highlights(), "the fake server highlights usages");

    let definition = client.request("textDocument/definition", json!({ "x": 1 })).unwrap();
    assert_eq!(definition.wait(WAIT).unwrap()[0]["uri"], "file:///a.go");
    assert_eq!(client.request("test/echo", json!({ "value": 7 })).unwrap().wait(WAIT).unwrap(), json!(7));
    assert!(matches!(
        client.request("test/broken", json!({})).unwrap().wait(WAIT),
        Err(LspError::Server { code: -32603, .. })
    ));
    assert_eq!(client.request("test/unknown", json!({})).unwrap().wait(WAIT).unwrap(), json!(null));
    assert_eq!(server.requests("textDocument/definition"), vec![json!({ "x": 1 })]);
    assert_eq!(server.launches(), 1);
}

#[test]
fn a_server_without_document_highlights_is_known_as_such() {
    let server = FakeServer::new();
    server.set_capabilities(json!({ "textDocumentSync": 1, "documentHighlightProvider": false }));
    let client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    assert!(!client.document_highlights());
    let server = FakeServer::new();
    server.set_capabilities(json!({ "textDocumentSync": 1 }));
    let client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    assert!(!client.document_highlights());
    let server = FakeServer::new();
    server.set_capabilities(json!({ "documentHighlightProvider": { "workDoneProgress": false } }));
    let client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    assert!(client.document_highlights(), "options count as a yes");
}

#[test]
fn the_client_declares_both_hierarchies_and_knows_which_the_server_offers() {
    let server = FakeServer::new();
    server.set_capabilities(json!({
        "callHierarchyProvider": true,
        "typeHierarchyProvider": { "workDoneProgress": false },
        "declarationProvider": false,
        "renameProvider": null
    }));
    let client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    let initialize = &server.wait_for("initialize", 1, WAIT)[0];
    let capabilities = &initialize["capabilities"]["textDocument"];
    assert_eq!(capabilities["callHierarchy"], json!({ "dynamicRegistration": false }));
    assert_eq!(capabilities["typeHierarchy"], json!({ "dynamicRegistration": false }));
    assert!(client.provides("callHierarchyProvider"));
    assert!(client.provides("typeHierarchyProvider"), "options count as a yes");
    assert!(!client.provides("declarationProvider"));
    assert!(!client.provides("renameProvider"));
    assert!(!client.provides("referencesProvider"), "absent is a no");
}

#[test]
fn delayed_and_held_answers() {
    let server = FakeServer::new();
    server.reply("test/later", Reply::Delayed(json!("late"), Duration::from_millis(100)));
    server.reply("test/never", Reply::Hold);
    let mut client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    let later = client.request("test/later", json!({})).unwrap();
    assert_eq!(later.try_answer(), None);
    assert_eq!(later.wait(WAIT).unwrap(), json!("late"));
    let never = client.request("test/never", json!({})).unwrap();
    assert_eq!(never.wait(Duration::from_millis(50)), Err(LspError::Timeout));
    client.cancel(&never);
    assert!(matches!(never.wait(WAIT), Err(LspError::Server { code: -32800, .. })));
}

#[test]
fn the_client_declares_workspace_edits_and_code_actions_and_reports_changed_files() {
    let server = FakeServer::new();
    let mut client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    let initialize = &server.wait_for("initialize", 1, WAIT)[0];
    let workspace = &initialize["capabilities"]["workspace"];
    assert_eq!(
        workspace["workspaceEdit"],
        json!({
            "documentChanges": true,
            "resourceOperations": ["create", "rename", "delete"],
            "failureHandling": "undo"
        })
    );
    assert_eq!(workspace["fileOperations"]["willRename"], true);
    assert_eq!(workspace["fileOperations"]["didDelete"], true);
    assert_eq!(workspace["fileOperations"]["dynamicRegistration"], false);
    assert_eq!(workspace["didChangeWatchedFiles"], json!({}));
    assert_eq!(workspace["applyEdit"], true);
    let document = &initialize["capabilities"]["textDocument"];
    assert_eq!(
        document["codeAction"]["codeActionLiteralSupport"]["codeActionKind"]["valueSet"],
        json!(["", "quickfix", "refactor", "source", "source.organizeImports"])
    );
    assert_eq!(document["rename"]["prepareSupport"], true);

    assert!(client.did_change_watched_files(&[]).unwrap().is_empty());
    client.did_change_watched_files(&[("file:///projects/shop/a.go".to_owned(), FileChangeType::CHANGED)]).unwrap();
    let sent = server.wait_for("workspace/didChangeWatchedFiles", 1, WAIT);
    assert_eq!(sent, vec![json!({ "changes": [{ "uri": "file:///projects/shop/a.go", "type": 2 }] })]);
}

#[test]
fn an_apply_edit_request_of_the_server_is_handed_to_the_application_and_its_answer_sent_back() {
    let server = FakeServer::new();
    let edit = json!({ "changes": {} });
    let asked = edit.clone();
    server.reply(
        "workspace/executeCommand",
        Reply::Ask {
            method: "workspace/applyEdit".to_owned(),
            params: std::sync::Arc::new(move |_| json!({ "label": "Fix", "edit": asked.clone() })),
            then: json!("done"),
        },
    );
    let mut client = Client::start(server.launcher(), Path::new("/projects/shop"), json!({})).unwrap();
    let pending = client.request("workspace/executeCommand", json!({ "command": "fix" })).unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    let request = loop {
        if let Some(request) = client.take_requests().pop() {
            break request;
        }
        assert!(std::time::Instant::now() < deadline, "no request came");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(request.method, "workspace/applyEdit");
    assert_eq!(request.params["edit"], edit);
    assert_eq!(pending.wait(Duration::from_millis(50)), Err(LspError::Timeout));
    client.respond(&request.id, json!({ "applied": true })).unwrap();
    assert_eq!(pending.wait(WAIT).unwrap(), json!("done"));
    assert_eq!(server.responses()[0]["result"], json!({ "applied": true }));
}

#[test]
fn a_server_gets_schema_content_from_the_client_and_downloads_are_refused() {
    let server = FakeServer::new();
    server.reply(
        "test/validate",
        Reply::AskAll {
            asks: vec![
                ("custom/schema/content".to_owned(), std::sync::Arc::new(|_| json!("zen-schema://yaml/compose.json"))),
                ("vscode/content".to_owned(), std::sync::Arc::new(|_| json!("https://example.com/s.json"))),
            ],
            then: json!("checked"),
        },
    );
    let content: zen_lsp::connection::ContentSource = std::sync::Arc::new(|uri: &str| {
        if uri.starts_with("zen-schema://") {
            Ok("{\"type\": \"object\"}".to_owned())
        } else {
            Err(format!("{uri}: off"))
        }
    });
    let registration = vec!["yaml/registerContentRequest".to_owned()];
    let mut client = Client::start_serving(
        server.launcher(),
        Path::new("/projects/shop"),
        json!({}),
        None,
        std::collections::BTreeMap::new(),
        Some(content),
        registration,
    )
    .unwrap();
    assert_eq!(server.wait_for("yaml/registerContentRequest", 1, WAIT).len(), 1, "registered after initialized");
    assert_eq!(client.request("test/validate", json!({})).unwrap().wait(WAIT).unwrap(), json!("checked"));
    let answers = server.wait_for_responses(2, WAIT);
    let schema = answers.iter().find(|answer| answer.get("result").is_some()).unwrap();
    assert_eq!(schema["result"], "{\"type\": \"object\"}");
    let refused = answers.iter().find(|answer| answer.get("error").is_some()).unwrap();
    assert!(refused["error"]["message"].as_str().unwrap().contains("https://example.com/s.json: off"), "{refused}");
}

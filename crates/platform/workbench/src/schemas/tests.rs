use std::sync::Arc;

use serde_json::json;
use zen_syntax::{Expansion, Feature, LineFold, OutlineItem, Outliner, SchemaDeclaration, Suggestions, TextFeatures};

use super::*;

struct Yaml;

impl Outliner for Yaml {
    fn handles(&self, _file_name: &str) -> bool {
        false
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

impl TextFeatures for Yaml {
    fn supports(&self, feature: Feature) -> bool {
        feature == Feature::Resources
    }

    fn folds(&self, _text: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, _text: &str, _caret: usize) -> Option<Suggestions> {
        None
    }

    fn expand(&self, _text: &str, _caret: usize) -> Option<Expansion> {
        None
    }

    fn plugin_id(&self) -> Option<&str> {
        Some("yaml")
    }

    fn schemas(&self) -> Vec<SchemaDeclaration> {
        vec![SchemaDeclaration {
            id: "kubernetes".to_owned(),
            name: "Kubernetes".to_owned(),
            server: "yaml-language-server".to_owned(),
            entry: "kubernetes/all.json".to_owned(),
            files: vec!["*.k8s.yaml".to_owned()],
        }]
    }

    fn resource(&self, path: &str) -> Option<Vec<u8>> {
        match path {
            "kubernetes/all.json" => Some(br#"{"oneOf": [{"$ref": "_definitions.json#/definitions/a"}]}"#.to_vec()),
            "kubernetes/_definitions.json" => Some(br#"{"definitions": {"a": {}}}"#.to_vec()),
            _ => None,
        }
    }
}

fn declaration() -> ServerDeclaration {
    let mut declaration: ServerDeclaration = serde_json::from_value(json!({
        "id": "yaml-language-server", "name": "YAML", "languages": ["yaml"], "command": "yaml-language-server",
        "schemaSupport": { "setting": "yaml.schemas", "register": "yaml/registerContentRequest" }
    }))
    .unwrap();
    declaration.settings.insert("yaml".to_owned(), json!({ "schemaStore": { "enable": false }, "validate": true }));
    declaration
}

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    zen_i18n::register(crate::LOCALES);
    set_plugins(Some(Arc::new(|| vec![Arc::new(Yaml) as Arc<dyn TextFeatures>])));
    reset_for_tests();
    guard
}

fn session() -> (Registry, Session) {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    (registry, Session::new())
}

#[test]
fn a_person_s_mapping_wins_over_a_shipped_pattern_and_names_schemas_three_ways() {
    let _one = fresh();
    let (registry, mut session) = session();
    let rows = json!({ "mappings": [
        { "schema": "yaml/kubernetes", "files": ["deploy/*.yaml"] },
        { "schema": "schemas/app.json", "files": ["*.k8s.yaml", "app.yaml"] },
        { "schema": "zen-schema://yaml/kubernetes/all.json", "files": ["cluster.yaml"] }
    ]});
    let listed =
        zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, "schemas.mappings.set", rows))
            .unwrap();
    assert_eq!(listed["mappings"].as_array().unwrap().len(), 3, "agents may map schemas: {listed}");
    assert_eq!(listed["shipped"][0]["schema"], "yaml/kubernetes");
    assert_eq!(listed["shipped"][0]["uri"], "zen-schema://yaml/kubernetes/all.json");
    let map = mapping_in("yaml-language-server", Some(Path::new("/p")));
    let shipped = map["zen-schema://yaml/kubernetes/all.json"].as_array().unwrap();
    assert!(!shipped.contains(&json!("*.k8s.yaml")), "the person's row takes the pattern: {map:?}");
    assert!(shipped.contains(&json!("deploy/*.yaml")) && shipped.contains(&json!("cluster.yaml")), "{map:?}");
    assert_eq!(map["file:///p/schemas/app.json"], json!(["*.k8s.yaml", "app.yaml"]));
    let incomplete = registry.dispatch(
        &mut session,
        "schemas.mappings.set",
        json!({ "mappings": [{ "schema": "x", "files": [] }] }),
    );
    assert!(incomplete.is_err());
}

#[test]
fn a_download_is_the_person_s_choice_never_untrusted_and_announced_before_it_starts() {
    let _one = fresh();
    let (registry, mut session) = session();
    let served = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/schema.json", served.local_addr().unwrap());
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in served.incoming().flatten() {
            use std::io::{Read, Write};
            let mut stream = stream;
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            assert!(with_state(|state| !state.told.is_empty()), "the address was named before the request came");
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body = "{\"type\": \"object\"}";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}",
                body.len()
            );
        }
    });
    assert!(content_in(&url, None).unwrap_err().contains("Schemas"), "off by default");
    let refused = zen_core::macros::as_agent(&mut session, |session| {
        registry.dispatch(session, "schemas.downloads.set", json!({ "allowed": true }))
    });
    assert!(format!("{refused:?}").contains("agentNotAllowed"), "{refused:?}");
    registry.dispatch(&mut session, "schemas.downloads.set", json!({ "allowed": true })).unwrap();
    let untrusted = tempfile::tempdir().unwrap();
    assert!(content_in(&url, Some(untrusted.path())).unwrap_err().contains("not trusted"));
    assert!(content_in("file:///etc/hosts", None).is_err(), "only http and https");
    let project = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(project.path()).unwrap();
    crate::trust::trust_in_tests(&crate::build_tools::BuildEnv::global(), &root);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let asked = url.clone();
    let download = std::thread::spawn(move || content_in(&asked, Some(&root)));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing is fetched before the person is told");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !download.is_finished() && std::time::Instant::now() < deadline {
        announce(&mut session);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(download.join().unwrap().unwrap(), "{\"type\": \"object\"}");
    let notifications = registry.dispatch(&mut session, "ui.notifications.list", json!({})).unwrap();
    assert!(notifications.to_string().contains(&url), "the address is named: {notifications}");
    assert_eq!(content_in(&url, None).unwrap(), "{\"type\": \"object\"}", "kept for the session");
    drop(project);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn shipped_schemas_reach_the_server_s_settings_and_their_content_comes_from_the_plug_in() {
    let _one = fresh();
    set_plugins(Some(Arc::new(|| vec![Arc::new(Yaml) as Arc<dyn TextFeatures>])));
    let declaration = declaration();
    let mut configuration = declaration.settings.clone();
    let (source, after) = serve(&declaration, &mut configuration, None);
    assert_eq!(after, ["yaml/registerContentRequest"]);
    let yaml = &configuration["yaml"];
    assert_eq!(yaml["schemas"]["zen-schema://yaml/kubernetes/all.json"], json!(["*.k8s.yaml"]), "{yaml}");
    assert_eq!(yaml["schemaStore"]["enable"], false, "the declared settings stay");
    let source = source.unwrap();
    assert!(source("zen-schema://yaml/kubernetes/all.json").unwrap().contains("_definitions.json"));
    assert!(source("zen-schema://yaml/kubernetes/_definitions.json#/definitions/a").unwrap().contains("definitions"));
    assert!(source("zen-schema://yaml/../secrets.json").is_err());
    assert!(source("zen-schema://other/kubernetes/all.json").is_err());
    let refused = source("https://example.com/schema.json").unwrap_err();
    assert!(refused.contains("https://example.com/schema.json") && refused.contains("Schemas"), "{refused}");
    let plain =
        serde_json::from_value::<ServerDeclaration>(json!({ "id": "x", "name": "X", "languages": [], "command": "x" }));
    let (none, nothing) = serve(&plain.unwrap(), &mut BTreeMap::new(), None);
    assert!(none.is_none() && nothing.is_empty(), "a server without schema support is left alone");
}

#[test]
fn a_server_without_a_schema_setting_still_has_its_downloads_answered() {
    let _one = fresh();
    let declaration: ServerDeclaration = serde_json::from_value(json!({
        "id": "vscode-json-language-server", "name": "JSON", "languages": ["json"], "command": "x",
        "schemaSupport": { "setting": "" }
    }))
    .unwrap();
    let mut configuration = BTreeMap::new();
    let (source, after) = serve(&declaration, &mut configuration, None);
    assert!(configuration.is_empty(), "nothing is mapped into its settings: {configuration:?}");
    assert!(after.is_empty());
    let refused = source.unwrap()("https://example.com/schema.json").unwrap_err();
    assert!(refused.contains("https://example.com/schema.json"), "{refused}");
}

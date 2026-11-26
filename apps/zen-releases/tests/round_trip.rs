use std::net::SocketAddr;

use zen_releases::publish::{Publication, current, publish, publish_changelog};
use zen_releases::server::{Config, serve};

const TOKEN: &str = "test-token";

fn start_server(data: &std::path::Path, public_key: &str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address: SocketAddr = listener.local_addr().unwrap();
    drop(listener);
    let config = Config { data: data.to_owned(), public_key: public_key.to_owned(), token: Some(TOKEN.to_owned()) };
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(serve(config, address)).unwrap();
    });
    let base = format!("http://{address}");
    for _ in 0..100 {
        if ureq::get(format!("{base}/api/v1/health")).call().is_ok() {
            return base;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the server did not start");
}

#[test]
fn publish_and_read_back() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([3; 32]);
    let base = start_server(data.path(), &public);

    assert!(current(&base, &public).unwrap().is_none(), "nothing is published yet");

    for version in ["0.1.0", "0.2.0"] {
        let file = files.path().join(format!("rustronin-{version}.tar.gz"));
        std::fs::write(&file, format!("build {version}")).unwrap();
        publish(&Publication {
            server: &base,
            token: TOKEN,
            secret_key: &secret,
            product: "rustronin",
            name: "RustRonin",
            version,
            channel: "stable",
            platform: "macos-aarch64",
            notes: "",
            file: &file,
            stage: None,
        })
        .unwrap();
    }

    let manifest = current(&base, &public).unwrap().unwrap();
    let (release, artifact) = manifest.latest("rustronin", "stable", "macos-aarch64").unwrap();
    assert_eq!(release.version, "0.2.0");
    let bytes =
        ureq::get(format!("{base}/releases/{}", artifact.url)).call().unwrap().body_mut().read_to_vec().unwrap();
    assert_eq!(bytes, b"build 0.2.0");
    zen_update::check_artifact(artifact, &bytes).unwrap();
}

#[test]
fn the_server_refuses_bad_tokens_bad_signatures_and_missing_artifacts() {
    let data = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([4; 32]);
    let base = start_server(data.path(), &public);

    let wrong_token = ureq::put(format!("{base}/api/v1/artifacts/a/0.1.0/a.tar.gz"))
        .header("Authorization", "Bearer nope")
        .send(&b"x"[..]);
    assert!(matches!(wrong_token, Err(ureq::Error::StatusCode(401))));

    let unsafe_path = ureq::put(format!("{base}/api/v1/artifacts/a/..%2F..%2Fetc/x"))
        .header("Authorization", format!("Bearer {TOKEN}"))
        .send(&b"x"[..]);
    assert!(unsafe_path.is_err());

    let mut manifest = zen_update::Manifest { schema: zen_update::SCHEMA, products: Vec::new() };
    manifest.add(
        "a",
        "A",
        zen_update::Release {
            version: "0.1.0".into(),
            channel: "stable".into(),
            date: "2026-10-16".into(),
            notes: String::new(),
            artifacts: vec![zen_update::Artifact {
                platform: "macos-aarch64".into(),
                url: "artifacts/a/0.1.0/a.tar.gz".into(),
                sha256: "00".into(),
                size: 1,
            }],
        },
    );
    let bytes = manifest.to_bytes();
    let good_signature = zen_update::sign_manifest(&bytes, &secret).unwrap();
    let missing = ureq::put(format!("{base}/api/v1/manifest"))
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("X-Zen-Signature", &good_signature)
        .send(&bytes[..]);
    assert!(matches!(missing, Err(ureq::Error::StatusCode(422))));

    let (other_secret, _) = zen_update::key_pair_from_seed([5; 32]);
    let foreign = zen_update::sign_manifest(&bytes, &other_secret).unwrap();
    let forged = ureq::put(format!("{base}/api/v1/manifest"))
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("X-Zen-Signature", &foreign)
        .send(&bytes[..]);
    assert!(matches!(forged, Err(ureq::Error::StatusCode(422))));
    assert!(current(&base, &public).unwrap().is_none(), "nothing was published");
}

#[test]
fn publish_a_plugin_and_install_it_as_an_ide_would() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([4; 32]);
    let base = start_server(data.path(), &public);
    assert!(zen_releases::publish::current_plugins(&base, &public).unwrap().is_none());

    let module = wat::parse_str(
        r#"(module
             (memory (export "memory") 1)
             (func (export "zen_abi") (result i32) i32.const 0)
             (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
             (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0))"#,
    )
    .unwrap();
    let file = files.path().join("markdown.wasm");
    std::fs::write(&file, &module).unwrap();
    let plugin = zen_plugins::CatalogPlugin {
        id: "markdown".to_owned(),
        name: "Markdown".to_owned(),
        description: "Markdown highlighting.".to_owned(),
        extensions: vec!["md".to_owned()],
        names: Vec::new(),
        hashbangs: Vec::new(),
        contents: Vec::new(),
        language_servers: Vec::new(),
        file_templates: zen_plugins::FileTemplateDeclarations::default(),
        live_templates: Default::default(),
        server_plugins: Vec::new(),
        requires: Vec::new(),
        project: zen_plugins::ProjectSupport::default(),
        database_drivers: Vec::new(),
        project_generators: Vec::new(),
        tests: Vec::new(),
        terminal_agents: Vec::new(),
        schemas: Vec::new(),
        runs: Vec::new(),
        person_only: Vec::new(),
        target_files: Vec::new(),
        tool_windows: Vec::new(),
        index: None,
        code_styles: Vec::new(),
        versions: Vec::new(),
    };
    let publication = zen_releases::publish::PluginPublication {
        server: &base,
        token: TOKEN,
        secret_key: &secret,
        plugin: plugin.clone(),
        version: "0.1.0",
        file: &file,
        notes: "### Added\n\n- Markdown highlighting.\n",
        stage: None,
    };
    zen_releases::publish::publish_plugin(&publication).unwrap();

    let importing = wat::parse_str(r#"(module (import "env" "f" (func)))"#).unwrap();
    std::fs::write(&file, importing).unwrap();
    assert!(
        zen_releases::publish::publish_plugin(&zen_releases::publish::PluginPublication {
            version: "0.2.0",
            ..publication
        })
        .is_err()
    );

    let catalog_url = format!("{base}/releases/plugins.json");
    let catalog = zen_plugins::fetch::catalog(&catalog_url, &public).unwrap();
    let found = catalog.for_file("README.md").unwrap();
    let version = found.latest().unwrap();
    assert_eq!(version.version, "0.1.0");
    assert_eq!(version.notes, "### Added\n\n- Markdown highlighting.", "the catalog carries the notes");
    let bytes = zen_plugins::fetch::module(&catalog_url, found, version).unwrap();
    let installed = zen_plugins::Installed::new(files.path().join("plugins"));
    installed.install(found, version, &bytes).unwrap();
    assert_eq!(installed.list().len(), 1);
    assert_eq!(installed.list()[0].notes, version.notes);

    let (_, other) = zen_update::key_pair_from_seed([5; 32]);
    assert!(zen_plugins::fetch::catalog(&catalog_url, &other).is_err());
}

#[test]
fn a_plugin_is_published_with_its_driver_programs_and_installed_with_them() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([6; 32]);
    let base = start_server(data.path(), &public);
    let module = wat::parse_str(
        r#"(module
             (memory (export "memory") 1)
             (func (export "zen_abi") (result i32) i32.const 0)
             (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
             (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0))"#,
    )
    .unwrap();
    let file = files.path().join("duck.wasm");
    std::fs::write(&file, &module).unwrap();
    let program = b"#!/bin/sh\nexit 0\n".to_vec();
    let name = "zen-db-duck-0.1.0-macos-aarch64";
    let url = format!("artifacts/plugin-duck/0.1.0/{name}");
    let driver = |program_entry: serde_json::Value| {
        serde_json::json!({
            "id": "duck",
            "name": "Duck",
            "capabilities": { "models": ["relational"], "language": "sql:duckdb" },
            "platforms": {
                "macos-aarch64": program_entry,
                "linux-x86_64": { "url": "https://example.org/zen-db-duck", "sha256": "0".repeat(64) },
            },
        })
    };
    let listed = serde_json::json!({
        "url": url,
        "sha256": zen_update::sha256_hex(&program),
        "size": program.len(),
        "file": "zen-db-duck",
    });
    let mut plugin = zen_plugins::CatalogPlugin {
        id: "duck".to_owned(),
        name: "Duck".to_owned(),
        description: String::new(),
        extensions: Vec::new(),
        names: Vec::new(),
        hashbangs: Vec::new(),
        contents: Vec::new(),
        language_servers: Vec::new(),
        file_templates: zen_plugins::FileTemplateDeclarations::default(),
        live_templates: Default::default(),
        server_plugins: Vec::new(),
        requires: Vec::new(),
        project: zen_plugins::ProjectSupport::default(),
        database_drivers: vec![driver(listed.clone())],
        project_generators: Vec::new(),
        tests: Vec::new(),
        terminal_agents: Vec::new(),
        schemas: Vec::new(),
        runs: Vec::new(),
        person_only: Vec::new(),
        target_files: Vec::new(),
        tool_windows: Vec::new(),
        index: None,
        code_styles: Vec::new(),
        versions: Vec::new(),
    };
    let publication = |plugin: &zen_plugins::CatalogPlugin| zen_releases::publish::PluginPublication {
        server: &base,
        token: TOKEN,
        secret_key: &secret,
        plugin: plugin.clone(),
        version: "0.1.0",
        file: &file,
        notes: "",
        stage: None,
    };

    let refused = zen_releases::publish::publish_plugin(&publication(&plugin)).unwrap_err();
    assert!(refused.contains(&url) && refused.contains("upload it first"), "{refused}");
    assert!(zen_releases::publish::current_plugins(&base, &public).unwrap().is_none(), "nothing was published");

    zen_releases::publish::upload_bytes(&base, TOKEN, "plugin-duck", "0.1.0", name, b"another program").unwrap();
    let refused = zen_releases::publish::publish_plugin(&publication(&plugin)).unwrap_err();
    assert!(refused.contains(&url), "a program stored with other bytes is refused: {refused}");

    zen_releases::publish::upload_bytes(&base, TOKEN, "plugin-duck", "0.1.0", name, &program).unwrap();
    let mut without_size = listed.clone();
    without_size.as_object_mut().unwrap().remove("size");
    plugin.database_drivers = vec![driver(without_size)];
    let refused = zen_releases::publish::publish_plugin(&publication(&plugin)).unwrap_err();
    assert!(refused.contains("size"), "{refused}");

    plugin.database_drivers = vec![driver(listed)];
    zen_releases::publish::publish_plugin(&publication(&plugin)).unwrap();

    let catalog_url = format!("{base}/releases/plugins.json");
    let catalog = zen_plugins::fetch::catalog(&catalog_url, &public).unwrap();
    let found = catalog.plugins.iter().find(|plugin| plugin.id == "duck").unwrap();
    let entry = &found.database_drivers[0]["platforms"]["macos-aarch64"];
    let downloaded = zen_plugins::fetch::file(&catalog_url, entry["url"].as_str().unwrap(), 1 << 20).unwrap();
    assert_eq!(downloaded, program);
    let installed = zen_plugins::Installed::new(files.path().join("plugins"));
    let path = installed
        .install_driver_program("duck", "duck", "zen-db-duck", entry["sha256"].as_str().unwrap(), &downloaded)
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), program);
}

#[test]
fn changelog_is_published_for_the_website() {
    let data = tempfile::tempdir().unwrap();
    let (_, public) = zen_update::key_pair_from_seed([5; 32]);
    let base = start_server(data.path(), &public);
    let url = format!("{base}/releases/changelog.json");
    assert!(ureq::get(&url).call().is_err(), "nothing is published yet");
    let json = br#"{"schema":1,"changelogs":[{"kind":"product","id":"gogarden","releases":[]}]}"#;
    assert!(publish_changelog(&base, "wrong", json).is_err(), "the token is checked");
    assert!(publish_changelog(&base, TOKEN, b"{}").is_err(), "the shape is checked");
    publish_changelog(&base, TOKEN, json).unwrap();
    assert_eq!(ureq::get(&url).call().unwrap().body_mut().read_to_vec().unwrap(), json);
}

#[test]
fn an_older_server_is_found_before_anything_is_uploaded() {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let record = std::sync::Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let mut length = 0;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            let _ = std::io::Read::read_exact(&mut reader, &mut body);
            record.lock().unwrap().push(first.trim().to_owned());
            let mut stream = stream;
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        }
    });
    let files = tempfile::tempdir().unwrap();
    let file = files.path().join("rustronin-0.1.0.tar.gz");
    std::fs::write(&file, "build").unwrap();
    let (secret, _) = zen_update::key_pair_from_seed([5; 32]);
    let error = publish(&Publication {
        server: &base,
        token: TOKEN,
        secret_key: &secret,
        product: "rustronin",
        name: "RustRonin",
        version: "0.1.0",
        channel: "stable",
        platform: "macos-aarch64",
        notes: "",
        file: &file,
        stage: None,
    })
    .unwrap_err();
    assert!(
        error.contains("older than this publisher") && error.contains("deploy the release server first"),
        "{error}"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(seen.iter().all(|request| !request.contains("/api/v1/artifacts/")), "nothing was uploaded: {seen:?}");
}

#[test]
fn a_manifest_the_server_cannot_read_is_refused_before_the_artifact_is_stored() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (_, public) = zen_update::key_pair_from_seed([3; 32]);
    let (other, _) = zen_update::key_pair_from_seed([4; 32]);
    let base = start_server(data.path(), &public);
    let file = files.path().join("rustronin-0.1.0.tar.gz");
    std::fs::write(&file, "build").unwrap();
    let error = publish(&Publication {
        server: &base,
        token: TOKEN,
        secret_key: &other,
        product: "rustronin",
        name: "RustRonin",
        version: "0.1.0",
        channel: "stable",
        platform: "macos-aarch64",
        notes: "",
        file: &file,
        stage: None,
    })
    .unwrap_err();
    assert!(error.contains("cannot read this manifest"), "{error}");
    assert!(!data.path().join("artifacts").join("rustronin").exists(), "no artifact was stored");
}

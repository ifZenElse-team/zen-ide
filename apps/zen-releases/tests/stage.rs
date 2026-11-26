use std::net::SocketAddr;
use std::path::Path;

use zen_releases::publish::{
    PluginPublication, Publication, current, publish, publish_plugin, release_stage, stage_changelog, stage_site,
    stage_summary,
};
use zen_releases::server::{Config, serve};

const TOKEN: &str = "test-token";

fn start_server(data: &Path, public_key: &str) -> String {
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

fn ide(base: &str, secret: &str, files: &Path, product: &str, version: &str, stage: Option<&str>) {
    let file = files.join(format!("{product}-{version}-macos-aarch64.tar.gz"));
    std::fs::write(&file, format!("{product} {version}")).unwrap();
    publish(&Publication {
        server: base,
        token: TOKEN,
        secret_key: secret,
        product,
        name: product,
        version,
        channel: "stable",
        platform: "macos-aarch64",
        notes: "- New.",
        file: &file,
        stage,
    })
    .unwrap();
}

fn get(url: &str) -> Option<Vec<u8>> {
    match ureq::get(url).call() {
        Ok(mut response) => Some(response.body_mut().read_to_vec().unwrap()),
        Err(ureq::Error::StatusCode(404)) => None,
        Err(error) => panic!("{url}: {error}"),
    }
}

fn versions(base: &str, public: &str, product: &str) -> Vec<String> {
    current(base, public)
        .unwrap()
        .map(|manifest| {
            manifest
                .products
                .iter()
                .filter(|entry| entry.id == product)
                .flat_map(|entry| entry.releases.iter().map(|release| release.version.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn website(dir: &Path, title: &str) {
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("index.html"), format!("<title>{title}</title>")).unwrap();
    std::fs::write(dir.join("docs/editor.html"), "editor").unwrap();
    std::fs::write(dir.join(".DS_Store"), "left out").unwrap();
}

#[test]
fn a_staged_release_is_served_only_once_it_is_released() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([7; 32]);
    let base = start_server(data.path(), &public);
    ide(&base, &secret, files.path(), "gogarden", "26.10.3", None);

    let stage = "v26.10.4-p100";
    for product in ["gogarden", "rustronin"] {
        ide(&base, &secret, files.path(), product, "26.10.4", Some(stage));
    }
    let module = files.path().join("json.wasm");
    let wasm = wat::parse_str(
        r#"(module
             (memory (export "memory") 1)
             (func (export "zen_abi") (result i32) i32.const 0)
             (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
             (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0))"#,
    )
    .unwrap();
    std::fs::write(&module, wasm).unwrap();
    let plugin = zen_plugins::CatalogPlugin {
        id: "json".to_owned(),
        name: "JSON".to_owned(),
        description: String::new(),
        extensions: vec!["json".to_owned()],
        names: Vec::new(),
        hashbangs: Vec::new(),
        contents: Vec::new(),
        language_servers: Vec::new(),
        file_templates: Default::default(),
        live_templates: Default::default(),
        server_plugins: Vec::new(),
        requires: Vec::new(),
        project: Default::default(),
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
    let staged_plugin = publish_plugin(&PluginPublication {
        server: &base,
        token: TOKEN,
        secret_key: &secret,
        plugin,
        version: "0.2.0",
        file: &module,
        notes: "- Faster.",
        stage: Some(stage),
    });
    let changelog = br#"{"schema": 1, "changelogs": [{"id": "gogarden"}]}"#;
    stage_changelog(&base, TOKEN, Some(stage), changelog).unwrap();
    let site = files.path().join("site");
    website(&site, "26.10.4");
    assert_eq!(stage_site(&base, TOKEN, stage, &site).unwrap(), 2, "hidden files are left out");

    assert_eq!(versions(&base, &public, "gogarden"), ["26.10.3"]);
    assert!(versions(&base, &public, "rustronin").is_empty());
    assert!(get(&format!("{base}/releases/changelog.json")).is_none());
    assert!(
        get(&format!("{base}/releases/artifacts/rustronin/26.10.4/rustronin-26.10.4-macos-aarch64.tar.gz")).is_some()
    );
    let staged = get(&format!("{base}/releases/stage/{stage}/manifest.json")).unwrap();
    let signature =
        String::from_utf8(get(&format!("{base}/releases/stage/{stage}/manifest.json.sig")).unwrap()).unwrap();
    let manifest = zen_update::verify_manifest(&staged, &signature, &public).unwrap();
    let gogarden = manifest.products.iter().find(|entry| entry.id == "gogarden").unwrap();
    assert_eq!(gogarden.releases.len(), 2, "the stage is built on the live manifest");
    assert_eq!(get(&format!("{base}/releases/staged-site/{stage}/docs/editor.html")).unwrap(), b"editor");
    let summary = stage_summary(&base, TOKEN, stage).unwrap();
    assert_eq!(summary["changelog"], true);
    assert_eq!(summary["site"], true);
    assert!(summary["released"].is_null());
    if let Err(error) = &staged_plugin {
        panic!("the plug-in was not staged: {error}");
    }
    assert!(get(&format!("{base}/releases/plugins.json")).is_none(), "the catalog is staged, not live");

    let released = release_stage(&base, TOKEN, stage).unwrap();
    assert!(released["released"].is_string(), "{released}");
    assert_eq!(versions(&base, &public, "rustronin"), ["26.10.4"]);
    assert_eq!(versions(&base, &public, "gogarden"), ["26.10.3", "26.10.4"]);
    assert_eq!(get(&format!("{base}/releases/changelog.json")).unwrap(), changelog);
    assert!(get(&format!("{base}/releases/plugins.json")).is_some());
    let link = std::fs::read_link(data.path().join("site/current")).unwrap();
    assert_eq!(link, Path::new(stage), "a relative link, the same in Caddy's container");
    assert_eq!(std::fs::read_to_string(data.path().join("site/current/index.html")).unwrap(), "<title>26.10.4</title>");

    let again = release_stage(&base, TOKEN, stage).unwrap();
    assert_eq!(again["already"], true);
    assert!(stage_site(&base, TOKEN, stage, &site).unwrap_err().contains("released already"));

    let restarted = start_server(data.path(), &public);
    assert_eq!(versions(&restarted, &public, "rustronin"), ["26.10.4"]);
}

#[test]
fn a_stage_made_from_an_older_live_manifest_is_not_released() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([8; 32]);
    let base = start_server(data.path(), &public);
    ide(&base, &secret, files.path(), "gogarden", "26.10.3", None);
    ide(&base, &secret, files.path(), "gogarden", "26.10.4", Some("v26.10.4-p1"));
    ide(&base, &secret, files.path(), "toolbox", "0.3.20", None);

    let file = files.path().join("rustronin.tar.gz");
    std::fs::write(&file, "r").unwrap();
    let more = publish(&Publication {
        server: &base,
        token: TOKEN,
        secret_key: &secret,
        product: "rustronin",
        name: "RustRonin",
        version: "26.10.4",
        channel: "stable",
        platform: "macos-aarch64",
        notes: "",
        file: &file,
        stage: Some("v26.10.4-p1"),
    })
    .unwrap_err();
    assert!(more.contains("changed meanwhile"), "{more}");
    let error = release_stage(&base, TOKEN, "v26.10.4-p1").unwrap_err();
    assert!(error.contains("409") && error.contains("changed since this stage was made"), "{error}");
    assert_eq!(versions(&base, &public, "gogarden"), ["26.10.3"], "nothing was released");
    assert_eq!(versions(&base, &public, "toolbox"), ["0.3.20"]);

    ide(&base, &secret, files.path(), "gogarden", "26.10.4", Some("v26.10.4-p2"));
    release_stage(&base, TOKEN, "v26.10.4-p2").unwrap();
    assert_eq!(versions(&base, &public, "gogarden"), ["26.10.3", "26.10.4"]);
    assert_eq!(versions(&base, &public, "toolbox"), ["0.3.20"]);
}

#[test]
fn staging_needs_the_token_and_safe_names() {
    let data = tempfile::tempdir().unwrap();
    let (_, public) = zen_update::key_pair_from_seed([9; 32]);
    let base = start_server(data.path(), &public);
    let status = |method: &str, path: &str, token: Option<&str>| -> u16 {
        let url = format!("{base}{path}");
        let agent = ureq::Agent::config_builder().http_status_as_error(false).build().new_agent();
        let mut response = match method {
            "POST" => {
                let mut request = agent.post(&url);
                if let Some(token) = token {
                    request = request.header("Authorization", format!("Bearer {token}"));
                }
                request.send_empty().unwrap()
            }
            _ => {
                let mut request = agent.put(&url);
                if let Some(token) = token {
                    request = request.header("Authorization", format!("Bearer {token}"));
                }
                request.send(&b"x"[..]).unwrap()
            }
        };
        let _ = response.body_mut().read_to_vec();
        response.status().as_u16()
    };
    assert_eq!(status("POST", "/api/v1/stage/v1/release", None), 401);
    assert_eq!(status("POST", "/api/v1/stage/v1/release", Some("wrong")), 401);
    assert_eq!(status("PUT", "/api/v1/stage/v1/site/index.html", None), 401);
    assert_eq!(status("POST", "/api/v1/stage/v1/release", Some(TOKEN)), 404, "nothing staged");
    assert_eq!(status("PUT", "/api/v1/stage/current/site/index.html", Some(TOKEN)), 400, "the live link's name");
    assert_eq!(status("PUT", "/api/v1/stage/v1/site/..%2Fescape", Some(TOKEN)), 400);
    assert_eq!(status("PUT", "/api/v1/stage/v1/site/a/.hidden", Some(TOKEN)), 400);
    assert_eq!(status("PUT", "/api/v1/stage/v1/manifest", Some(TOKEN)), 400, "no signature");
    assert_eq!(status("PUT", "/api/v1/stage/v1/site/docs/a.html", Some(TOKEN)), 204);
    assert!(!data.path().join("escape").exists());
}

#[test]
fn the_credentials_are_checked_without_publishing_anything() {
    let data = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([10; 32]);
    let (other, _) = zen_update::key_pair_from_seed([11; 32]);
    let base = start_server(data.path(), &public);
    zen_releases::publish::check_credentials(&base, TOKEN, &secret).unwrap();
    let token = zen_releases::publish::check_credentials(&base, "wrong", &secret).unwrap_err();
    assert!(token.contains("token") && !token.contains("wrong"), "{token}");
    let key = zen_releases::publish::check_credentials(&base, TOKEN, &other).unwrap_err();
    assert!(key.contains("does not trust this signing key") && !key.contains(&other), "{key}");
    assert!(current(&base, &public).unwrap().is_none(), "nothing was published");
}

#[test]
fn files_the_server_has_are_not_uploaded_again() {
    use zen_releases::publish::{Upload, upload_all, upload_bytes};
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (_, public) = zen_update::key_pair_from_seed([13; 32]);
    let base = start_server(data.path(), &public);
    let uploads: Vec<Upload> = (0..6)
        .map(|index| {
            let path = files.path().join(format!("file-{index}.tar.gz"));
            std::fs::write(&path, format!("content {index}")).unwrap();
            Upload {
                product: "gogarden".to_owned(),
                version: "26.10.4".to_owned(),
                name: format!("file-{index}.tar.gz"),
                path,
            }
        })
        .collect();
    assert_eq!(upload_all(&base, TOKEN, &uploads, 3).unwrap(), (6, 0), "six sent, three at a time");
    assert_eq!(upload_all(&base, TOKEN, &uploads, 3).unwrap(), (0, 6), "a retry sends nothing");
    std::fs::write(&uploads[2].path, "changed").unwrap();
    assert_eq!(upload_all(&base, TOKEN, &uploads, 2).unwrap(), (1, 5));
    let stored = data.path().join("artifacts/gogarden/26.10.4/file-2.tar.gz");
    assert_eq!(std::fs::read_to_string(stored).unwrap(), "changed");
    assert!(upload_bytes(&base, "wrong", "gogarden", "26.10.4", "x.tar.gz", b"x").unwrap_err().contains("401"));
    let anonymous = ureq::get(format!("{base}/api/v1/artifacts/gogarden/26.10.4/file-0.tar.gz")).call();
    assert!(matches!(anonymous, Err(ureq::Error::StatusCode(401))), "{anonymous:?}");
}

fn answer_to_headers_alone(base: &str, request: &str) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(base.trim_start_matches("http://")).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut answer = [0u8; 1024];
    let read = stream.read(&mut answer).expect("the server answers without waiting for the body");
    String::from_utf8_lossy(&answer[..read]).into_owned()
}

#[test]
fn a_body_without_the_token_is_refused_before_it_is_read() {
    let data = tempfile::tempdir().unwrap();
    let (_, public) = zen_update::key_pair_from_seed([20; 32]);
    let base = start_server(data.path(), &public);
    for path in ["/api/v1/artifacts/a/1/f", "/api/v1/manifest", "/api/v1/stage/s/site/a.html"] {
        let request = format!("PUT {path} HTTP/1.1\r\nHost: x\r\nContent-Length: 4000000000\r\n\r\n");
        let answer = answer_to_headers_alone(&base, &request);
        assert!(answer.starts_with("HTTP/1.1 401"), "{path}: {answer}");
    }
}

#[test]
fn documents_and_website_files_have_small_limits_and_artifacts_a_large_one() {
    let data = tempfile::tempdir().unwrap();
    let (_, public) = zen_update::key_pair_from_seed([21; 32]);
    let base = start_server(data.path(), &public);
    let agent = ureq::Agent::config_builder().http_status_as_error(false).build().new_agent();
    let status = |path: &str, length: usize| {
        let request = agent.put(format!("{base}{path}")).header("Authorization", format!("Bearer {TOKEN}"));
        let mut response = request.header("X-Zen-Signature", "00").send(&vec![b' '; length][..]).unwrap();
        let _ = response.body_mut().read_to_vec();
        response.status().as_u16()
    };
    assert_eq!(status("/api/v1/manifest", (32 << 20) + 1), 413);
    assert_eq!(status("/api/v1/stage/s/changelog", (32 << 20) + 1), 413);
    assert_eq!(status("/api/v1/stage/s/site/a.html", (64 << 20) + 1), 413);
    assert_eq!(status("/api/v1/stage/s/site/a.html", (32 << 20) + 1), 204, "a website file may be larger");
    let announced = format!(
        "PUT /api/v1/artifacts/a/1/f HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {}\r\n\r\n",
        (4u64 << 30) + 1
    );
    assert!(answer_to_headers_alone(&base, &announced).starts_with("HTTP/1.1 413"));
}

#[test]
fn a_large_artifact_is_stored_and_hashed_as_it_arrives() {
    let data = tempfile::tempdir().unwrap();
    let (_, public) = zen_update::key_pair_from_seed([22; 32]);
    let base = start_server(data.path(), &public);
    let bytes = vec![7u8; 40 << 20];
    assert!(zen_releases::publish::upload_bytes(&base, TOKEN, "toolbox", "1.0.0", "big.tar.gz", &bytes).unwrap());
    let folder = data.path().join("artifacts/toolbox/1.0.0");
    assert_eq!(std::fs::read(folder.join("big.tar.gz")).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "no partial file is left");
    assert!(!zen_releases::publish::upload_bytes(&base, TOKEN, "toolbox", "1.0.0", "big.tar.gz", &bytes).unwrap());
}

#[test]
fn an_artifact_the_live_manifest_lists_cannot_be_replaced() {
    use zen_releases::publish::upload_bytes;
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([23; 32]);
    let base = start_server(data.path(), &public);
    ide(&base, &secret, files.path(), "gogarden", "26.10.3", None);
    let name = "gogarden-26.10.3-macos-aarch64.tar.gz";
    let stored = data.path().join("artifacts/gogarden/26.10.3").join(name);
    let original = std::fs::read(&stored).unwrap();

    let refused = upload_bytes(&base, TOKEN, "gogarden", "26.10.3", name, b"other bytes").unwrap_err();

    assert!(refused.contains("409") && refused.contains("new version"), "{refused}");
    assert_eq!(std::fs::read(&stored).unwrap(), original);
    assert_eq!(std::fs::read_dir(stored.parent().unwrap()).unwrap().count(), 1, "no partial file is left");
    assert_eq!(upload_bytes(&base, TOKEN, "gogarden", "26.10.3", name, &original), Ok(false), "the same bytes pass");
    assert_eq!(upload_bytes(&base, TOKEN, "gogarden", "26.10.3", "other.tar.gz", b"x"), Ok(true), "not listed");
}

#[test]
fn a_manifest_staged_again_is_checked_by_the_hash_of_every_listed_artifact() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([24; 32]);
    let base = start_server(data.path(), &public);
    ide(&base, &secret, files.path(), "gogarden", "26.10.3", None);
    let stored = data.path().join("artifacts/gogarden/26.10.3/gogarden-26.10.3-macos-aarch64.tar.gz");
    let same_size = "x".repeat(std::fs::metadata(&stored).unwrap().len() as usize);
    std::fs::write(&stored, same_size).unwrap();

    let file = files.path().join("gogarden-26.10.4-macos-aarch64.tar.gz");
    std::fs::write(&file, "new").unwrap();
    let refused = publish(&Publication {
        server: &base,
        token: TOKEN,
        secret_key: &secret,
        product: "gogarden",
        name: "gogarden",
        version: "26.10.4",
        channel: "stable",
        platform: "macos-aarch64",
        notes: "- New.",
        file: &file,
        stage: Some("s1"),
    })
    .unwrap_err();

    assert!(refused.contains("SHA-256"), "{refused}");
}

#[test]
fn a_stage_whose_artifact_was_replaced_afterwards_is_not_released() {
    use zen_releases::publish::upload_bytes;
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([25; 32]);
    let base = start_server(data.path(), &public);
    ide(&base, &secret, files.path(), "gogarden", "26.10.4", Some("s1"));
    let name = "gogarden-26.10.4-macos-aarch64.tar.gz";
    assert_eq!(upload_bytes(&base, TOKEN, "gogarden", "26.10.4", name, b"rebuilt"), Ok(true), "only staged: allowed");

    let refused = release_stage(&base, TOKEN, "s1").unwrap_err();

    assert!(refused.contains("nothing was released"), "{refused}");
    assert!(versions(&base, &public, "gogarden").is_empty());
}

#[test]
fn the_publishing_api_is_not_served_over_plain_http() {
    let caddyfile =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/releases/Caddyfile")).unwrap();
    let plain = caddyfile.find("handle @plain_api").unwrap();
    assert!(plain < caddyfile.find("handle @releases").unwrap(), "the first matching handle wins");
    let matcher = &caddyfile[caddyfile.find("@plain_api {").unwrap()..plain];
    assert!(matcher.contains("protocol http\n") && matcher.contains("path /api/*"), "{matcher}");
}

const INSTALL_SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/releases/site/install.sh");

#[test]
fn the_install_script_trusts_the_official_key() {
    let script = std::fs::read_to_string(INSTALL_SCRIPT).unwrap();
    assert!(script.contains(zen_update::OFFICIAL_PUBLIC_KEY));
}

#[test]
fn the_python_check_of_the_install_script_verifies_the_signature() {
    let script = std::fs::read_to_string(INSTALL_SCRIPT).unwrap();
    let start = script.find("<<'VERIFY_PY'").unwrap();
    let start = start + script[start..].find('\n').unwrap() + 1;
    let check = &script[start..start + script[start..].find("VERIFY_PY").unwrap()];
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("check.py");
    let document = folder.path().join("manifest.json");
    std::fs::write(&program, check).unwrap();
    std::fs::write(&document, br#"{"schema":1}"#).unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([26; 32]);
    let (_, other) = zen_update::key_pair_from_seed([27; 32]);
    let signature = zen_update::sign_manifest(br#"{"schema":1}"#, &secret).unwrap();
    let verifies = |signature: &str, key: &str| {
        std::process::Command::new("python3")
            .args(["-I".as_ref(), program.as_os_str(), document.as_os_str(), signature.as_ref(), key.as_ref()])
            .status()
            .unwrap()
            .success()
    };
    assert!(verifies(&signature, &public));
    assert!(!verifies(&signature, &other));
    let wrong = zen_update::sign_manifest(b"another document", &secret).unwrap();
    assert!(!verifies(&wrong, &public));
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn the_install_script_stops_at_a_manifest_that_does_not_verify() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([28; 32]);
    let (_, other) = zen_update::key_pair_from_seed([29; 32]);
    let base = start_server(data.path(), &public);
    ide(&base, &secret, files.path(), "toolbox", "1.2.3", None);
    let run = |key: &str| {
        let output = std::process::Command::new("sh")
            .arg(INSTALL_SCRIPT)
            .env("ZEN_RELEASES", &base)
            .env("ZEN_PUBLIC_KEY", key)
            .env("ZEN_INSTALL_DIR", target.path())
            .env("ZEN_NO_OPEN", "1")
            .output()
            .unwrap();
        assert!(!output.status.success(), "no installer is stored, so the script stops either way");
        format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    };
    let trusted = run(&public);
    assert!(trusted.contains("Downloading Zen Toolbox 1.2.3"), "{trusted}");
    let untrusted = run(&other);
    assert!(untrusted.contains("does not verify") && !untrusted.contains("Downloading"), "{untrusted}");
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn the_install_script_installs_the_signed_archive_and_refuses_another() {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (secret, public) = zen_update::key_pair_from_seed([30; 32]);
    let base = start_server(data.path(), &public);
    let app = files.path().join("Zen Toolbox.app/Contents");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("Info.plist"), "plist").unwrap();
    let archive = files.path().join("toolbox-1.2.3-macos-aarch64.tar.gz");
    let packed = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(files.path())
        .arg("Zen Toolbox.app")
        .status()
        .unwrap();
    assert!(packed.success());
    publish(&Publication {
        server: &base,
        token: TOKEN,
        secret_key: &secret,
        product: "toolbox",
        name: "toolbox",
        version: "1.2.3",
        channel: "stable",
        platform: "macos-aarch64",
        notes: "- New.",
        file: &archive,
        stage: None,
    })
    .unwrap();
    let run = || {
        std::process::Command::new("sh")
            .arg(INSTALL_SCRIPT)
            .env("ZEN_RELEASES", &base)
            .env("ZEN_PUBLIC_KEY", &public)
            .env("ZEN_INSTALL_DIR", target.path())
            .env("ZEN_NO_OPEN", "1")
            .output()
            .unwrap()
    };
    let installed = run();
    assert!(installed.status.success(), "{}", String::from_utf8_lossy(&installed.stderr));
    let plist = target.path().join("Zen Toolbox.app/Contents/Info.plist");
    assert_eq!(std::fs::read_to_string(&plist).unwrap(), "plist");

    std::fs::remove_dir_all(target.path().join("Zen Toolbox.app")).unwrap();
    let stored = data.path().join("artifacts/toolbox/1.2.3/toolbox-1.2.3-macos-aarch64.tar.gz");
    std::fs::write(stored, "another archive").unwrap();
    let refused = run();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("does not match the signed manifest"));
    assert!(!plist.exists(), "nothing was installed");
}

#[cfg(unix)]
#[test]
fn a_data_folder_the_server_cannot_write_stops_it_at_start() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let site = data.path().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::set_permissions(&site, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::write(site.join("probe"), b"").is_ok() {
        return;
    }
    let (_, public) = zen_update::key_pair_from_seed([14; 32]);
    let config = Config { data: data.path().to_owned(), public_key: public, token: Some(TOKEN.to_owned()) };
    let Err(error) = zen_releases::server::router(config) else { panic!("an unwritable data folder was accepted") };
    assert!(error.contains("site") && error.contains("not writable") && error.contains("chown"), "{error}");
    std::fs::set_permissions(&site, std::fs::Permissions::from_mode(0o755)).unwrap();
}

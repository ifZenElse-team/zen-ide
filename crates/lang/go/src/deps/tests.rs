use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_project::inspect::{FindingSeverity, FixKind, InspectContext};

use super::*;

struct FakeServer {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    down: Arc<std::sync::atomic::AtomicBool>,
}

impl FakeServer {
    fn start(routes: HashMap<String, Vec<u8>>) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let is_down = Arc::clone(&down);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                if is_down.load(Ordering::SeqCst) {
                    continue;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                seen.lock().unwrap().push(path.clone());
                let (status, body) = match routes.get(&path) {
                    Some(body) => ("200 OK", body.clone()),
                    None => ("404 Not Found", b"not found".to_vec()),
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        FakeServer { url, requests, down }
    }

    fn go_down(&self) {
        self.down.store(true, Ordering::SeqCst);
    }

    fn requested(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn routes() -> HashMap<String, Vec<u8>> {
    let mut routes = HashMap::new();
    let mut add = |path: &str, body: Vec<u8>| {
        routes.insert(path.to_owned(), body);
    };
    add("/proxy/github.com/andybalholm/brotli/@v/list", b"v1.0.0\nv1.2.6\nv1.1.0\nv1.3.0-rc.1\n".to_vec());
    add("/proxy/golang.org/x/crypto/@v/list", b"v0.1.0\nv0.31.0\nv0.35.0\nv0.43.0\n".to_vec());
    add("/proxy/github.com/!khan/genqlient/@v/list", b"v0.8.1\n".to_vec());
    add(
        "/dl/?mode=json",
        br#"[{"version":"go1.28rc1","stable":false},{"version":"go1.27.2","stable":true},{"version":"go1.26.4","stable":true}]"#
            .to_vec(),
    );
    add(
        "/dl/?mode=json&include=all",
        br#"[{"version":"go1.27.2","stable":true},{"version":"go1.26.4","stable":true},{"version":"go1.25.7","stable":true},
            {"version":"go1.25.6","stable":true},{"version":"go1.25rc2","stable":false}]"#
            .to_vec(),
    );
    add("/proxy/github.com/hhrutter/lzw/@v/list", b"v1.0.0\nv1.0.1\n".to_vec());
    add(
        "/proxy/github.com/hhrutter/lzw/@v/v1.0.1.mod",
        b"// Deprecated: no longer maintained as a standalone module.\nmodule github.com/hhrutter/lzw\n\ngo 1.19\n"
            .to_vec(),
    );
    add("/proxy/github.com/klauspost/compress/@v/list", b"v1.18.0\nv1.18.1\nv1.18.2\nv1.20.2\nv1.20.3\n".to_vec());
    add(
        "/proxy/github.com/klauspost/compress/@v/v1.20.3.mod",
        b"module github.com/klauspost/compress\n\nretract (\n\t// https://github.com/klauspost/compress/issues/1114\n\tv1.18.1\n\n\t// Published by mistake.\n\tv1.20.3\n)\n".to_vec(),
    );
    add("/proxy/example.com/excluded/@v/list", b"v1.0.0\nv1.1.0\nv1.2.0\nv1.3.0\n".to_vec());
    add("/proxy/github.com/major/lib/@v/list", b"v1.4.0\n".to_vec());
    add("/proxy/github.com/major/lib/v2/@v/list", b"v2.0.0\nv2.3.1\n".to_vec());
    add("/proxy/github.com/major/lib/v3/@v/list", b"v3.0.0\n".to_vec());
    add("/proxy/github.com/pseudo/dep/@v/list", b"v0.1.0\nv0.2.0\n".to_vec());
    add("/proxy/github.com/old/inc/@v/list", b"v1.0.0\nv2.0.0+incompatible\nv2.1.0+incompatible\n".to_vec());
    add("/vuln/index/db.json", br#"{"modified":"2026-10-08T22:31:09Z"}"#.to_vec());
    let modules = json!([{ "path": "golang.org/x/crypto", "vulns": [
        { "id": "GO-2024-3321", "modified": "2025-02-18T20:32:01Z", "fixed": "0.31.0" },
        { "id": "GO-2025-3487", "modified": "2025-02-26T00:00:00Z", "fixed": "0.35.0" },
        { "id": "GO-2020-0012", "modified": "2024-05-20T16:03:47Z", "fixed": "0.0.0-20200220183623-bac4c82f6975" },
    ] }]);
    add("/vuln/index/modules.json.gz", gzip(modules.to_string().as_bytes()));
    let entry = |id: &str, cve: &str, ghsa: &str, fixed: &str, summary: &str| {
        json!({
            "id": id, "modified": "2025-02-18T20:32:01Z", "aliases": [cve, ghsa], "summary": summary, "details": "What happens.",
            "affected": [{ "package": { "name": "golang.org/x/crypto", "ecosystem": "Go" },
                "ranges": [{ "type": "SEMVER", "events": [{ "introduced": "0" }, { "fixed": fixed }] }] }],
            "database_specific": { "url": format!("https://pkg.go.dev/vuln/{id}") },
        })
        .to_string()
        .into_bytes()
    };
    add(
        "/vuln/ID/GO-2024-3321.json",
        entry("GO-2024-3321", "CVE-2024-45337", "GHSA-v778-237x-gjrc", "0.31.0", "Authorization bypass"),
    );
    add(
        "/vuln/ID/GO-2025-3487.json",
        entry("GO-2025-3487", "CVE-2025-22869", "GHSA-hcg3-q754-cr77", "0.35.0", "Denial of service"),
    );
    add(
        "/osv/GHSA-v778-237x-gjrc",
        json!({ "severity": [{ "type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:N" }],
                "database_specific": { "severity": "CRITICAL" } })
        .to_string()
        .into_bytes(),
    );
    routes
}

const GO_MOD: &str = "module example.com/app\n\ngo 1.25.0\n\nrequire (\n\tgithub.com/Khan/genqlient v0.8.1\n\tgithub.com/andybalholm/brotli v1.0.0 // indirect\n\tgolang.org/x/crypto v0.1.0 // indirect\n\tgitlab.corp.example/backend/kit v0.2.0\n\texample.com/forked v1.0.0\n)\n\nreplace example.com/forked => ../forked\n";

fn go_sum(text: &str, leave_out: &[&str]) -> String {
    parse_go_mod(Path::new("go.mod"), text)
        .requires
        .iter()
        .filter(|requirement| !leave_out.contains(&requirement.path.as_str()))
        .map(|requirement| {
            let (path, version) = (&requirement.path, &requirement.version);
            format!("{path} {version} h1:x=\n{path} {version}/go.mod h1:x=\n")
        })
        .collect()
}

struct Fixture {
    server: FakeServer,
    inspector: GoModInspector,
    dir: tempfile::TempDir,
    go: PathBuf,
    settings: HashMap<String, Value>,
    file: &'static str,
    text: String,
}

fn fixture(server: FakeServer, cache: Option<&Path>) -> Fixture {
    fixture_with(server, cache, "go.mod", GO_MOD, "go1.25.3")
}

fn fixture_with(server: FakeServer, cache: Option<&Path>, file: &'static str, text: &str, sdk: &str) -> Fixture {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let go = write_go(dir.path(), &format!("{}/proxy", server.url), sdk);
    let services = Services {
        vulndb: format!("{}/vuln", server.url),
        osv: format!("{}/osv", server.url),
        releases: format!("{}/dl/?mode=json", server.url),
    };
    let inspector = GoModInspector::new(Arc::new(proxy::Http), services);
    if let Some(cache) = cache {
        inspector.set_cache_dir(cache.to_owned());
    }
    std::fs::write(dir.path().join(file), text).unwrap();
    if file == "go.mod" {
        std::fs::write(dir.path().join("go.sum"), go_sum(text, &[])).unwrap();
    }
    Fixture { server, inspector, dir, go, settings: HashMap::new(), file, text: text.to_owned() }
}

#[cfg(unix)]
fn write_go(dir: &Path, proxy: &str, sdk: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let go = dir.join("go");
    let env = json!({ "GOPROXY": format!("{proxy},direct"), "GOPRIVATE": "gitlab.corp.example", "GONOPROXY": "", "GONOSUMDB": "", "GOFLAGS": "", "GOVERSION": sdk });
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nenv) echo '{env}' ;;\nlist) cat list.json 2>/dev/null ;;\nmod) if [ -f tidy.diff ]; then cat tidy.diff; exit 1; fi ;;\nesac\n"
    );
    std::fs::write(&go, script).unwrap();
    std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
    go
}

#[cfg(not(unix))]
fn write_go(_dir: &Path, _proxy: &str, _sdk: &str) -> PathBuf {
    PathBuf::new()
}

impl Fixture {
    fn inspect(&self) -> Vec<Finding> {
        let path = self.dir.path().join(self.file);
        let setting = |key: &str| self.settings.get(key).cloned();
        let context = InspectContext {
            root: self.dir.path(),
            path: &path,
            text: &self.text,
            tool: &self.go,
            env: &[],
            setting: &setting,
        };
        self.inspector.inspect(&context)
    }

    fn at<'a>(&self, findings: &'a [Finding], code: &str, text: &str) -> Option<&'a Finding> {
        findings.iter().find(|finding| finding.code == code && self.text[finding.start..finding.end].contains(text))
    }

    fn settle(&self) -> Vec<Finding> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let findings = self.inspect();
            if !self.inspector.busy() {
                let again = self.inspect();
                if !self.inspector.busy() {
                    return again;
                }
            }
            assert!(
                Instant::now() < deadline,
                "the checks did not finish: {findings:?} {:?}",
                lock(&self.inspector.shared.state).queued
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn at<'a>(findings: &'a [Finding], code: &str, text: &str) -> Option<&'a Finding> {
    findings.iter().find(|finding| finding.code == code && GO_MOD[finding.start..finding.end].contains(text))
}

#[cfg(unix)]
#[test]
fn outdated_and_vulnerable_modules_are_found_and_private_ones_never_asked_about() {
    let fixture = fixture(FakeServer::start(routes()), None);
    let findings = fixture.settle();
    let brotli = at(&findings, "newerVersion", "v1.0.0").expect("brotli is outdated");
    assert_eq!(&GO_MOD[brotli.start..brotli.end], "v1.0.0");
    assert_eq!(brotli.severity, FindingSeverity::WeakWarning);
    assert_eq!(brotli.message, "Newer version is available: v1.2.6");
    assert_eq!(brotli.fixes[0].kind, FixKind::Batch);
    assert_eq!(brotli.fixes[0].title, "Update dependencies…");
    assert_eq!(brotli.fixes[1].id, "go.update:github.com/andybalholm/brotli@v1.2.6");
    assert_eq!(brotli.fixes[1].label.as_deref(), Some("github.com/andybalholm/brotli: v1.0.0 → v1.2.6"));
    assert_eq!(brotli.fixes[2].args["tidy"], true);
    assert!(at(&findings, "newerVersion", "v0.8.1").is_none(), "genqlient is up to date");

    let crypto = at(&findings, "vulnerability", "golang.org/x/crypto").expect("crypto is vulnerable");
    assert_eq!(&GO_MOD[crypto.start..crypto.end], "golang.org/x/crypto v0.1.0 // indirect", "the whole line");
    assert_eq!(crypto.severity, FindingSeverity::Warning);
    assert_eq!(crypto.message, "2 known vulnerabilities in this dependency:");
    let rows: Vec<(&str, Option<&str>, &str)> =
        crypto.rows.iter().map(|row| (row.label.as_str(), row.badge.as_deref(), row.text.as_str())).collect();
    assert_eq!(
        rows,
        [("CVE-2024-45337", Some("9.1"), "Authorization bypass"), ("CVE-2025-22869", None, "Denial of service")]
    );
    assert_eq!(crypto.rows[0].url.as_deref(), Some("https://pkg.go.dev/vuln/GO-2024-3321"));
    assert_eq!(crypto.fixes[0].title, "Update to unaffected version v0.35.0");
    assert_eq!(crypto.fixes[0].args, json!({ "module": "golang.org/x/crypto", "version": "v0.35.0", "tidy": false }));
    let details = crypto.details.as_deref().unwrap();
    assert!(
        details.contains("GO-2024-3321") && details.contains("CVE-2024-45337") && details.contains("9.1"),
        "{details}"
    );
    assert!(details.contains("vuln.go.dev), licensed under CC BY 4.0"), "the data's licence is named: {details}");
    assert!(at(&findings, "newerVersion", "v0.1.0").is_some(), "crypto is outdated too");

    let requested = fixture.server.requested();
    assert!(!requested.iter().any(|path| path.contains("gitlab.corp.example")), "{requested:?}");
    assert!(
        !requested.iter().any(|path| path.contains("forked")),
        "a replaced module is not asked about: {requested:?}"
    );
    assert!(
        !requested.iter().any(|path| path.contains("GO-2020-0012")),
        "a vulnerability fixed long ago is not fetched"
    );
    let count = requested.len();
    fixture.settle();
    assert_eq!(fixture.server.requested().len(), count);
}

#[cfg(unix)]
#[test]
fn turned_off_checks_ask_nothing() {
    let mut fixture = fixture(FakeServer::start(routes()), None);
    fixture.settings.insert(CHECK_UPDATES.to_owned(), json!(false));
    fixture.settings.insert(CHECK_VULNERABILITIES.to_owned(), json!(false));
    assert!(fixture.settle().is_empty());
    assert!(fixture.server.requested().is_empty());
    fixture.settings.insert(CHECK_VULNERABILITIES.to_owned(), json!(true));
    let findings = fixture.settle();
    assert!(findings.iter().all(|finding| finding.code == "vulnerability"), "{findings:?}");
    assert!(at(&findings, "vulnerability", "golang.org/x/crypto").is_some());
}

#[cfg(unix)]
#[test]
fn offline_finds_nothing_without_errors_and_the_disk_cache_comes_back() {
    let cache = tempfile::tempdir().unwrap();
    let online = fixture(FakeServer::start(routes()), Some(cache.path()));
    assert!(at(&online.settle(), "vulnerability", "golang.org/x/crypto").is_some());
    let server = online.server;
    server.go_down();
    let shared = |server: &FakeServer| FakeServer {
        url: server.url.clone(),
        requests: Arc::clone(&server.requests),
        down: Arc::clone(&server.down),
    };

    let fresh = fixture(shared(&server), None);
    assert!(fresh.settle().is_empty(), "offline: nothing, and no error");

    let again = fixture(shared(&server), Some(cache.path()));
    let findings = again.settle();
    assert!(at(&findings, "newerVersion", "v1.0.0").is_some(), "{findings:?}");
    assert!(at(&findings, "vulnerability", "golang.org/x/crypto").is_some(), "{findings:?}");
}

#[test]
fn a_requirement_is_found_on_its_line() {
    let place = place(GO_MOD, 8, "golang.org/x/crypto", "v0.1.0").unwrap();
    assert_eq!(&GO_MOD[place.0..place.1], "golang.org/x/crypto");
    assert_eq!(&GO_MOD[place.2..place.3], "v0.1.0");
    assert_eq!(&GO_MOD[place.0..place.4], "golang.org/x/crypto v0.1.0 // indirect");
}

#[test]
fn fixes_of_one_module_take_the_newest_version() {
    let inspector = GoModInspector::new(Arc::new(proxy::Http), Services::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("go.mod");
    let setting = |_: &str| None;
    let context =
        InspectContext { root: dir.path(), path: &path, text: "", tool: Path::new(""), env: &[], setting: &setting };
    let fix = |version: &str| update_fix("a.example/b", "v1.0.0", version, String::new(), false, true);
    assert!(inspector.apply(&context, &[fix("v1.1.0"), fix("v1.2.0")]).is_err(), "no Go SDK: no update");
    let broken = InspectionFix { args: json!({}), ..fix("v1.0.0") };
    assert!(inspector.apply(&context, &[broken]).is_err());
}

const VARIATIONS: &str = "module example.com/app\n\ngo 1.25.0\n\ntoolchain go1.25.0\n\nrequire (\n\tgithub.com/major/lib v1.4.0\n\tgithub.com/klauspost/compress v1.18.1\n\texample.com/excluded v1.1.0\n\tgithub.com/pseudo/dep v0.0.0-20190101000000-abcdef123456\n\tgithub.com/old/inc v2.0.0+incompatible\n\texample.com/moved v1.0.0\n\texample.com/unknown v1.99.0\n\tgitlab.corp.example/backend/kit v0.2.0\n)\n\nrequire (\n\tgithub.com/hhrutter/lzw v1.0.0 // indirect\n\texample.com/unused v1.0.0 // indirect\n)\n\nexclude (\n\texample.com/excluded v1.1.0\n\texample.com/excluded v1.3.0\n)\n\nreplace example.com/moved => example.com/other v1.2.0\n";

#[cfg(unix)]
#[test]
fn every_variation_of_a_requirement_is_found() {
    let fixture = fixture_with(FakeServer::start(routes()), None, "go.mod", VARIATIONS, "go1.25.3");
    let dir = fixture.dir.path();
    std::fs::write(dir.join("go.sum"), go_sum(VARIATIONS, &["github.com/pseudo/dep"])).unwrap();
    std::fs::write(
        dir.join("list.json"),
        r#"{"Path":"example.com/app","Main":true}
{"Path":"example.com/unknown","Version":"v1.99.0","Error":{"Err":"example.com/unknown@v1.99.0: invalid version: unknown revision v1.99.0"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("tidy.diff"),
        "diff current/go.mod tidy/go.mod\n--- current/go.mod\n+++ tidy/go.mod\n@@ -1 +1 @@\n-\texample.com/unused v1.0.0 // indirect\n",
    )
    .unwrap();
    let findings = fixture.settle();
    let codes: Vec<(&str, &str)> =
        findings.iter().map(|finding| (finding.code.as_str(), &VARIATIONS[finding.start..finding.end])).collect();

    let lzw = fixture.at(&findings, "deprecatedModule", "github.com/hhrutter/lzw").expect("lzw is deprecated");
    assert_eq!(&VARIATIONS[lzw.start..lzw.end], "github.com/hhrutter/lzw");
    assert!(lzw.deprecated);
    assert_eq!(lzw.message, "Deprecated module github.com/hhrutter/lzw.");
    assert_eq!(lzw.note.as_deref(), Some("Deprecation notice: no longer maintained as a standalone module."));
    assert_eq!(lzw.fixes[0].title, "Show github.com/hhrutter/lzw deprecation notice");
    assert_eq!(lzw.fixes[0].kind, FixKind::Details);
    assert!(lzw.details.as_deref().unwrap().contains("no longer maintained"));

    let compress = fixture.at(&findings, "retractedVersion", "v1.18.1").expect("v1.18.1 is retracted");
    assert_eq!(compress.severity, FindingSeverity::Warning);
    assert_eq!(compress.message, "Version v1.18.1 is retracted by its authors");
    assert_eq!(compress.note.as_deref(), Some("Reason: https://github.com/klauspost/compress/issues/1114"));
    assert_eq!(compress.fixes[0].args["version"], "v1.20.2", "v1.20.3 is retracted too");
    assert!(fixture.at(&findings, "newerVersion", "v1.18.1").is_none(), "one finding per version");

    let excluded = fixture.at(&findings, "excludedVersion", "v1.1.0").expect("v1.1.0 is excluded");
    assert_eq!(excluded.message, "Version v1.1.0 is excluded in this go.mod");
    assert_eq!(excluded.fixes[0].args["version"], "v1.2.0", "v1.3.0 is excluded as well");

    let major = fixture.at(&findings, "newerMajorVersion", "github.com/major/lib").expect("a newer major");
    assert_eq!(major.severity, FindingSeverity::Information);
    assert_eq!(major.message, "Newer major version is available: github.com/major/lib/v3 v3.0.0");
    assert!(major.fixes.is_empty(), "taking a major version needs changes in the code");
    assert!(fixture.at(&findings, "newerVersion", "v1.4.0").is_none(), "v1.4.0 is the newest v1");

    assert_eq!(fixture.at(&findings, "newerVersion", "v0.0.0-2019").unwrap().fixes[1].args["version"], "v0.2.0");
    assert_eq!(
        fixture.at(&findings, "newerVersion", "v2.0.0+incompatible").unwrap().fixes[1].args["version"],
        "v2.1.0+incompatible"
    );

    let unknown = fixture.at(&findings, "goListError", "v1.99.0").expect("go list's error");
    assert_eq!(
        (unknown.severity, unknown.message.as_str()),
        (FindingSeverity::Error, "Cannot load example.com/unknown v1.99.0: invalid version: unknown revision v1.99.0")
    );
    let missing = fixture.at(&findings, "missingGoSum", "v0.0.0-2019").expect("go.sum has no entry");
    assert_eq!(missing.message, "Missing go.sum entry for github.com/pseudo/dep v0.0.0-20190101000000-abcdef123456");
    assert_eq!(missing.fixes[0].args, json!({ "tidy": true }));
    let unused = fixture.at(&findings, "unusedRequirement", "example.com/unused").expect("unused");
    assert_eq!(unused.severity, FindingSeverity::WeakWarning);
    assert!(unused.unused && !unused.deprecated, "greyed out, not underlined");
    assert!(findings.iter().filter(|finding| finding.unused).all(|finding| finding.code == "unusedRequirement"));
    assert_eq!(unused.fixes[0].title, "Run go mod tidy");

    let go = fixture.at(&findings, "newerGoVersion", "1.25.0").expect("a newer Go");
    assert_eq!(go.message, "Newer Go version is available: 1.27.2");
    assert_eq!(go.fixes[0].title, "Update go directive to 1.27.2");
    assert_eq!(go.fixes[0].args, json!({ "run": ["mod", "edit", "-go=1.27.2"] }));
    let toolchain = fixture.at(&findings, "newerToolchain", "go1.25.0").expect("a newer toolchain");
    assert_eq!(toolchain.fixes[0].args, json!({ "run": ["mod", "edit", "-toolchain=go1.27.2"] }));
    assert!(fixture.at(&findings, "goNewerThanSdk", "1.25.0").is_none(), "the SDK is newer");

    let requested = fixture.server.requested();
    for never in ["moved", "other", "gitlab.corp.example", "lzw/v2"] {
        assert!(!requested.iter().any(|path| path.contains(never)), "{never}: {requested:?}");
    }
    assert!(!codes.iter().any(|(_, text)| text.contains("moved") || text.contains("gitlab.corp")), "{codes:?}");
}

#[cfg(unix)]
#[test]
fn a_go_directive_newer_than_the_sdk_points_to_its_settings() {
    let text = "module example.com/app\n\ngo 1.26.1\n";
    let fixture = fixture_with(FakeServer::start(routes()), None, "go.mod", text, "go1.24.0");
    let findings = fixture.settle();
    let sdk = fixture.at(&findings, "goNewerThanSdk", "1.26.1").expect("the SDK is older");
    assert_eq!(sdk.message, "This module needs Go 1.26.1, newer than the Go SDK in use (1.24.0)");
    assert_eq!(sdk.fixes[0].kind, FixKind::Action);
    assert_eq!(sdk.fixes[0].title, "Configure Go SDK…");
    assert_eq!(sdk.fixes[0].args, json!({ "action": "view.settings.open", "params": { "page": "go" } }));
    assert!(fixture.at(&findings, "newerGoVersion", "1.26.1").is_some());
}

#[cfg(unix)]
#[test]
fn the_go_directive_offers_the_newest_release_and_the_newest_patch_of_its_line() {
    let fixes = |text: &str, code: &str, token: &str| -> Vec<(String, String, Value, bool)> {
        let text = format!("module example.com/app\n\n{text}\n");
        let fixture = fixture_with(FakeServer::start(routes()), None, "go.mod", &text, "go1.27.2");
        let findings = fixture.settle();
        fixture
            .at(&findings, code, token)
            .map(|finding| {
                finding
                    .fixes
                    .iter()
                    .map(|fix| (fix.id.clone(), fix.title.clone(), fix.args.clone(), fix.inline))
                    .collect()
            })
            .unwrap_or_default()
    };
    let edit = |flag: &str| json!({ "run": ["mod", "edit", flag] });
    assert_eq!(
        fixes("go 1.26", "newerGoVersion", "1.26"),
        [
            ("go.goDirective:1.27.2".to_owned(), "Update go directive to 1.27.2".to_owned(), edit("-go=1.27.2"), true),
            ("go.goDirectivePatch:1.26.4".to_owned(), "Update to 1.26.4".to_owned(), edit("-go=1.26.4"), true),
        ]
    );
    let titles = |text: &str, code: &str, token: &str| -> Vec<String> {
        fixes(text, code, token).into_iter().map(|(_, title, _, _)| title).collect()
    };
    assert_eq!(titles("go 1.26.0", "newerGoVersion", "1.26.0"), ["Update go directive to 1.27.2", "Update to 1.26.4"]);
    assert_eq!(titles("go 1.26.4", "newerGoVersion", "1.26.4"), ["Update go directive to 1.27.2"]);
    assert_eq!(titles("go 1.27", "newerGoVersion", "1.27"), ["Update to 1.27.2"]);
    assert_eq!(titles("go 1.27.0", "newerGoVersion", "1.27.0"), ["Update to 1.27.2"]);
    assert!(titles("go 1.27.2", "newerGoVersion", "1.27.2").is_empty());
    assert_eq!(titles("go 1.25.0", "newerGoVersion", "1.25.0"), ["Update go directive to 1.27.2", "Update to 1.25.7"]);
    assert_eq!(
        fixes("go 1.26.0\n\ntoolchain go1.26.1", "newerToolchain", "go1.26.1"),
        [
            (
                "go.toolchain:go1.27.2".to_owned(),
                "Update toolchain to go1.27.2".to_owned(),
                edit("-toolchain=go1.27.2"),
                true
            ),
            (
                "go.toolchainPatch:go1.26.4".to_owned(),
                "Update to go1.26.4".to_owned(),
                edit("-toolchain=go1.26.4"),
                true
            ),
        ]
    );
}

#[test]
fn a_raised_go_directive_is_explained_by_the_modules_that_require_it() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let cache = tempfile::tempdir().unwrap();
    let put = |module: &str, version: &str, go: &str| {
        let dir = cache.path().join("cache/download").join(module).join("@v");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{version}.mod")), format!("module {module}\n\ngo {go}\n")).unwrap();
    };
    put("github.com/gofiber/fiber/v3", "v3.6.1", "1.26.0");
    put("github.com/nats-io/nats.go", "v1.54.0", "1.26.0");
    put("github.com/!burnt!sushi/toml", "v1.6.0", "1.26.0");
    put("github.com/old/lib", "v1.1.0", "1.22");
    let parse = |text: &str| parse_go_mod(Path::new("go.mod"), text);
    let before = parse(
        "module m\n\ngo 1.26\n\nrequire (\n\tgithub.com/gofiber/fiber/v3 v3.5.0\n\tgithub.com/nats-io/nats.go v1.53.0\n\tgithub.com/BurntSushi/toml v1.5.0\n\tgithub.com/old/lib v1.0.0\n\tgithub.com/kept/lib v1.0.0\n)\n",
    );
    let after = parse(
        "module m\n\ngo 1.26.0\n\nrequire (\n\tgithub.com/gofiber/fiber/v3 v3.6.1\n\tgithub.com/nats-io/nats.go v1.54.0\n\tgithub.com/BurntSushi/toml v1.6.0\n\tgithub.com/old/lib v1.1.0\n\tgithub.com/kept/lib v1.0.0\n)\n",
    );
    let notice = update::raise_notice(&before, &after, || Some(cache.path().to_owned())).expect("raised");
    assert_eq!(notice.title, "go directive raised to 1.26.0");
    assert_eq!(
        notice.text,
        "The go directive was raised from 1.26 to 1.26.0: github.com/gofiber/fiber/v3 v3.6.1 and 2 more require go 1.26.0"
    );
    let one = parse("module m\n\ngo 1.26.0\n\nrequire github.com/gofiber/fiber/v3 v3.6.1\n");
    let notice = update::raise_notice(&before, &one, || Some(cache.path().to_owned())).unwrap();
    assert_eq!(
        notice.text,
        "The go directive was raised from 1.26 to 1.26.0: github.com/gofiber/fiber/v3 v3.6.1 requires go 1.26.0"
    );
    let notice = update::raise_notice(&before, &after, || None).unwrap();
    assert_eq!(notice.text, "The go directive was raised from 1.26 to 1.26.0");
    assert!(update::raise_notice(&after, &after, || Some(cache.path().to_owned())).is_none());
    assert!(update::raise_notice(&after, &before, || Some(cache.path().to_owned())).is_none());
}

#[cfg(unix)]
#[test]
fn a_go_work_file_has_its_go_version_and_used_folders_checked() {
    let text = "go 1.24\n\nuse (\n\t./app\n\t./missing\n\t./plain\n)\n";
    let fixture = fixture_with(FakeServer::start(routes()), None, "go.work", text, "go1.25.3");
    std::fs::create_dir_all(fixture.dir.path().join("app")).unwrap();
    std::fs::write(fixture.dir.path().join("app/go.mod"), "module example.com/app\n").unwrap();
    std::fs::create_dir_all(fixture.dir.path().join("plain")).unwrap();
    let findings = fixture.settle();
    let mut uses: Vec<(&str, &str)> = findings
        .iter()
        .filter(|finding| finding.code == "useDirectory")
        .map(|finding| (&text[finding.start..finding.end], finding.message.as_str()))
        .collect();
    uses.sort();
    assert_eq!(
        uses,
        [("./missing", "Directory ./missing does not exist"), ("./plain", "Directory ./plain has no go.mod")]
    );
    let go = fixture.at(&findings, "newerGoVersion", "1.24").expect("a newer Go");
    assert_eq!(go.fixes[0].args, json!({ "run": ["work", "edit", "-go=1.27.2"] }));
}

#[test]
fn newer_major_versions_follow_the_module_path_rules() {
    assert_eq!(next_major("github.com/a/b").as_deref(), Some("github.com/a/b/v2"));
    assert_eq!(next_major("github.com/a/b/v3").as_deref(), Some("github.com/a/b/v4"));
    assert_eq!(next_major("github.com/a/v1x").as_deref(), Some("github.com/a/v1x/v2"));
    assert_eq!(next_major("gopkg.in/yaml.v3"), None);
}

#[test]
fn a_latest_go_mod_tells_deprecation_and_retractions() {
    let info = ModInfo::parse(
        b"// Deprecated: use b instead\nmodule a\n\nretract [v1.0.0, v1.2.0] // broken\nretract v1.5.0\n",
    );
    assert_eq!(info.deprecated.as_deref(), Some("use b instead"));
    assert_eq!(info.retraction("v1.1.0").as_deref(), Some("broken"));
    assert_eq!(info.retraction("v1.5.0").as_deref(), Some(""));
    assert_eq!(info.retraction("v1.3.0"), None);
}

#[cfg(unix)]
#[test]
fn fixes_run_their_go_commands_in_order() {
    use std::os::unix::fs::PermissionsExt;
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let inspector = GoModInspector::new(Arc::new(proxy::Http), Services::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("go.mod");
    std::fs::write(&path, "module m\n").unwrap();
    let go = dir.path().join("go");
    std::fs::write(&go, "#!/bin/sh\necho \"go $*\"\n").unwrap();
    std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
    let setting = |_: &str| None;
    let context = InspectContext { root: dir.path(), path: &path, text: "", tool: &go, env: &[], setting: &setting };
    let edit = InspectionFix {
        id: "go.goDirective:1.27.2".to_owned(),
        title: String::new(),
        kind: FixKind::Apply,
        batch: None,
        label: None,
        inline: false,
        args: json!({ "run": ["mod", "edit", "-go=1.27.2"] }),
    };
    let update = update_fix("a.example/b", "v1.0.0", "v1.1.0", String::new(), true, false);
    let mut job = inspector.apply(&context, &[edit, update]).unwrap();
    assert_eq!(job.title().as_deref(), Some("Updating 1 dependency"));
    let deadline = Instant::now() + Duration::from_secs(60);
    while matches!(job.poll(), zen_project::inspect::FixState::Running { .. }) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        job.drain_output(),
        [
            "$ go mod edit -go=1.27.2",
            "go mod edit -go=1.27.2",
            "$ go get a.example/b@v1.1.0",
            "go get a.example/b@v1.1.0",
            "$ go mod tidy",
            "go mod tidy"
        ]
    );
    assert!(job.drain_output().is_empty(), "each line once");
}

#[test]
fn the_findings_of_a_go_mod_depend_on_the_module_files_on_disk() {
    let inspector = GoModInspector::new(Arc::new(proxy::Http), Services::default());
    let inputs = inspector.inputs(Path::new("/work/app/go.mod"));
    for name in ["go.mod", "go.sum", "go.work"] {
        assert!(inputs.contains(&Path::new("/work/app").join(name)), "{name}: {inputs:?}");
    }
}

fn find_go() -> Option<PathBuf> {
    let candidates = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).map(|folder| folder.join("go")).collect::<Vec<_>>())
        .unwrap_or_default();
    candidates
        .into_iter()
        .chain(["/opt/homebrew/bin/go", "/usr/local/go/bin/go", "/usr/local/bin/go"].map(PathBuf::from))
        .find(|candidate| candidate.is_file())
}

fn stored_zip(files: &[(&str, &str)]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    let u16le = |value: usize| (value as u16).to_le_bytes();
    let u32le = |value: u32| value.to_le_bytes();
    for (name, contents) in files {
        let mut crc = flate2::Crc::new();
        crc.update(contents.as_bytes());
        let (crc, size, offset) = (crc.sum(), contents.len() as u32, out.len() as u32);
        let common =
            [&u16le(20)[..], &u16le(0), &u16le(0), &u16le(0), &u16le(0x21), &u32le(crc), &u32le(size)].concat();
        out.extend_from_slice(&u32le(0x0403_4b50));
        out.extend_from_slice(&common);
        out.extend_from_slice(&u32le(size));
        out.extend_from_slice(&u16le(name.len()));
        out.extend_from_slice(&u16le(0));
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(contents.as_bytes());
        central.extend_from_slice(&u32le(0x0201_4b50));
        central.extend_from_slice(&u16le(20));
        central.extend_from_slice(&common);
        central.extend_from_slice(&u32le(size));
        central.extend_from_slice(&u16le(name.len()));
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&u32le(0));
        central.extend_from_slice(&u32le(offset));
        central.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&u32le(0x0605_4b50));
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&u16le(files.len()));
    out.extend_from_slice(&u16le(files.len()));
    out.extend_from_slice(&u32le(central.len() as u32));
    out.extend_from_slice(&u32le(start));
    out.extend_from_slice(&u16le(0));
    out
}

#[cfg(unix)]
#[test]
fn run_go_mod_tidy_restores_a_missing_go_sum_entry_and_the_mark_goes() {
    let Some(go) = find_go() else { return };
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let temp = tempfile::tempdir().unwrap();
    let versions = temp.path().join("proxy/example.com/dep/@v");
    std::fs::create_dir_all(&versions).unwrap();
    std::fs::write(versions.join("list"), "v1.0.0\n").unwrap();
    std::fs::write(versions.join("v1.0.0.info"), r#"{"Version":"v1.0.0","Time":"2024-01-01T00:00:00Z"}"#).unwrap();
    std::fs::write(versions.join("v1.0.0.mod"), "module example.com/dep\n\ngo 1.21\n").unwrap();
    let zip = stored_zip(&[
        ("example.com/dep@v1.0.0/go.mod", "module example.com/dep\n\ngo 1.21\n"),
        ("example.com/dep@v1.0.0/dep.go", "package dep\n\n// Answer is the answer.\nconst Answer = 42\n"),
    ]);
    std::fs::write(versions.join("v1.0.0.zip"), zip).unwrap();
    let module = temp.path().join("app");
    std::fs::create_dir_all(&module).unwrap();
    let text = "module example.com/app\n\ngo 1.21\n\nrequire example.com/dep v1.0.0\n";
    let path = module.join("go.mod");
    std::fs::write(&path, text).unwrap();
    std::fs::write(
        module.join("main.go"),
        "package main\n\nimport \"example.com/dep\"\n\nfunc main() { println(dep.Answer) }\n",
    )
    .unwrap();
    std::fs::write(module.join("go.sum"), "").unwrap();
    let cache = temp.path().join("gopath");
    let env: Vec<(String, String)> = [
        ("GOPROXY", format!("file://{}", temp.path().join("proxy").display())),
        ("GOSUMDB", "off".to_owned()),
        ("GOPATH", cache.display().to_string()),
        ("GOMODCACHE", cache.join("pkg/mod").display().to_string()),
        ("GOFLAGS", "-modcacherw".to_owned()),
        ("GOTOOLCHAIN", "local".to_owned()),
        ("GOWORK", "off".to_owned()),
    ]
    .map(|(key, value)| (key.to_owned(), value))
    .into();
    let inspector = GoModInspector::new(Arc::new(proxy::Http), Services::default());
    let setting = |key: &str| (key == CHECK_UPDATES || key == CHECK_VULNERABILITIES).then_some(json!(false));
    let inspect = || {
        let context = InspectContext { root: &module, path: &path, text, tool: &go, env: &env, setting: &setting };
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let findings = inspector.inspect(&context);
            if !inspector.busy() && !lock(&inspector.shared.state).envs.is_empty() {
                return inspector.inspect(&context);
            }
            assert!(Instant::now() < deadline, "the checks did not finish: {findings:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let findings = inspect();
    let missing = findings.iter().find(|finding| finding.code == "missingGoSum").expect("go.sum lacks the entry");
    assert_eq!(missing.message, "Missing go.sum entry for example.com/dep v1.0.0");
    let tidy = missing.fixes[0].clone();
    assert_eq!(tidy.title, "Run go mod tidy");

    let context = InspectContext { root: &module, path: &path, text, tool: &go, env: &env, setting: &setting };
    let mut job = inspector.apply(&context, &[tidy]).unwrap();
    assert_eq!(job.title().as_deref(), Some("Running go mod tidy"));
    let deadline = Instant::now() + Duration::from_secs(60);
    let state = loop {
        match job.poll() {
            zen_project::inspect::FixState::Running { .. } => {}
            state => break state,
        }
        assert!(Instant::now() < deadline, "go mod tidy did not end");
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = job.drain_output().join("\n");
    assert!(matches!(state, zen_project::inspect::FixState::Done { .. }), "{state:?}\n{output}");
    assert!(output.starts_with("$ go mod tidy"), "the log says what ran: {output}");
    let go_sum = std::fs::read_to_string(module.join("go.sum")).unwrap();
    assert!(go_sum.contains("example.com/dep v1.0.0/go.mod h1:"), "tidy restored the entry: {go_sum}");
    let findings = inspect();
    assert!(!findings.iter().any(|finding| finding.code == "missingGoSum"), "the mark went: {findings:?}");

    std::fs::write(&path, "module example.com/app\n\ngo 1.21\n\nrequire example.com/gone v1.0.0\n").unwrap();
    std::fs::write(module.join("main.go"), "package main\n\nimport \"example.com/gone\"\n\nfunc main() { gone.F() }\n")
        .unwrap();
    let mut job = inspector.apply(&context, &[missing.fixes[0].clone()]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let state = loop {
        match job.poll() {
            zen_project::inspect::FixState::Running { .. } => {}
            state => break state,
        }
        assert!(Instant::now() < deadline, "go mod tidy did not end");
        std::thread::sleep(Duration::from_millis(20));
    };
    match state {
        zen_project::inspect::FixState::Failed { output } => {
            assert!(output.starts_with("go mod tidy") && output.contains("example.com/gone"), "{output}")
        }
        state => panic!("tidy should fail: {state:?}"),
    }
}

#[test]
fn gopls_unused_requirements_are_known_as_unused() {
    let inspector = GoModInspector::new(Arc::new(proxy::Http), Services::default());
    let unused = |source: Option<&str>, message: &str| inspector.unused_diagnostic(source, message);
    assert!(unused(Some("go mod tidy"), "golang.org/x/sync is not used in this module"));
    assert!(!unused(Some("go mod tidy"), "golang.org/x/sync is not in your go.mod file"));
    assert!(!unused(Some("go mod tidy"), "go.sum is out of sync with go.mod"));
    assert!(!unused(Some("compiler"), "golang.org/x/sync is not used in this module"));
    assert!(!unused(None, "golang.org/x/sync is not used in this module"));
}

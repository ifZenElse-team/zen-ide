use std::time::Duration;

use crate::test_proxy::tls_target;
use crate::trust;
use crate::trust::parse_certificates;
use crate::{ProxyMode, ProxySettings};

const CA: &str = include_str!("../../tests/fixtures/ca.pem");
const LEAF: &str = include_str!("../../tests/fixtures/leaf.pem");
const KEY: &str = include_str!("../../tests/fixtures/leaf.pk8.pem");

fn fetch(url: &str) -> Result<u16, crate::CheckError> {
    let settings = ProxySettings { mode: ProxyMode::NoProxy, ..ProxySettings::default() };
    trust::asking_the_person(|| crate::check(&settings, None, url, Duration::from_secs(20)))
}

fn fetch_unasked(url: &str) -> Result<u16, crate::CheckError> {
    let settings = ProxySettings { mode: ProxyMode::NoProxy, ..ProxySettings::default() };
    crate::check(&settings, None, url, Duration::from_secs(20))
}

fn answer_when_asked(accept: bool) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        loop {
            if let Some(waiting) = trust::pending().first() {
                let fingerprint = waiting.certificate.fingerprint.clone();
                assert!(trust::answer(&fingerprint, accept).unwrap());
                return fingerprint;
            }
            std::thread::yield_now();
        }
    })
}

static ONE_STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_trust_store_decides_who_is_connected_to() {
    let _store = ONE_STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let folder = tempfile::tempdir().unwrap();
    trust::open(folder.path());
    let url = format!("https://localhost:{}/", tls_target(LEAF, KEY));
    let leaf = parse_certificates(LEAF.as_bytes()).remove(0);
    let root = parse_certificates(CA.as_bytes()).remove(0);

    let refused = fetch(&url).unwrap_err();
    assert!(format!("{refused:?}").to_lowercase().contains("issuer"), "nothing accepts it: {refused:?}");
    assert!(trust::pending().is_empty(), "without a window nobody is asked");

    trust::set_automatic(true);
    assert_eq!(fetch(&url), Ok(200), "Accept non-trusted certificates automatically");
    trust::set_automatic(false);
    assert!(trust::accepted().is_empty(), "automatic acceptance adds nothing to the list");

    trust::set_accepted(vec![root.clone()]).unwrap();
    assert_eq!(fetch(&url), Ok(200), "an accepted issuer vouches for its servers");
    assert!(std::fs::read_to_string(folder.path().join("accepted.pem")).unwrap().contains("BEGIN CERTIFICATE"));
    assert!(trust::remove(&trust::fingerprint(&root)).unwrap());
    assert!(fetch(&url).is_err());

    trust::enable_prompts(|| {});
    let rejecting = answer_when_asked(false);
    assert!(fetch(&url).is_err(), "Reject");
    let fingerprint = rejecting.join().unwrap();
    assert_eq!(fingerprint, trust::fingerprint(&leaf));
    assert!(fetch(&url).is_err() && trust::pending().is_empty(), "rejected once: not asked again this session");

    let other = format!("https://localhost:{}/", tls_target(CA_SIGNED_AGAIN, KEY_AGAIN));
    let accepting = answer_when_asked(true);
    assert_eq!(fetch(&other), Ok(200), "Accept");
    let fingerprint = accepting.join().unwrap();
    let pinned = trust::pinned();
    assert_eq!(
        pinned.iter().map(|pin| (pin.host.as_str(), trust::fingerprint(&pin.der))).collect::<Vec<_>>(),
        [("localhost", fingerprint)]
    );
    assert!(trust::accepted().is_empty(), "accepting adds no issuer");
    trust::open(folder.path());
    assert_eq!(fetch(&other), Ok(200), "the list outlives a restart");
}

const CA_SIGNED_AGAIN: &str = include_str!("../../tests/fixtures/other.pem");
const KEY_AGAIN: &str = include_str!("../../tests/fixtures/other.pk8.pem");

#[test]
fn an_accepted_leaf_holds_for_its_host_only() {
    let _store = ONE_STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let folder = tempfile::tempdir().unwrap();
    trust::open(folder.path());
    let port = tls_target(LEAF, KEY);
    let leaf = parse_certificates(LEAF.as_bytes()).remove(0);
    trust::accept(leaf, "localhost").unwrap();
    assert_eq!(fetch_unasked(&format!("https://localhost:{port}/")), Ok(200));
    assert!(
        fetch_unasked(&format!("https://127.0.0.1:{port}/")).is_err(),
        "another host: the same certificate proves nothing"
    );
}

#[test]
fn an_accepted_certificate_cannot_sign_for_anyone() {
    let _store = ONE_STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let folder = tempfile::tempdir().unwrap();
    trust::open(folder.path());
    let port = tls_target(LEAF, KEY);
    let root = parse_certificates(CA.as_bytes()).remove(0);
    trust::accept(root, "localhost").unwrap();
    assert!(
        fetch_unasked(&format!("https://localhost:{port}/")).is_err(),
        "a certificate accepted at a prompt is not a root"
    );
}

#[test]
fn git_is_never_told_to_stop_verifying_and_keeps_the_person_s_roots() {
    let _store = ONE_STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let folder = tempfile::tempdir().unwrap();
    trust::open(folder.path());
    trust::set_automatic(true);
    let automatic = trust::git_environment(None);
    trust::set_automatic(false);
    assert_eq!(automatic, [], "automatic acceptance says nothing to Git");
    let (leaf, root, own) = (
        parse_certificates(LEAF.as_bytes()).remove(0),
        parse_certificates(CA.as_bytes()).remove(0),
        parse_certificates(CA_SIGNED_AGAIN.as_bytes()).remove(0),
    );
    trust::accept(leaf.clone(), "localhost").unwrap();
    trust::set_accepted(vec![root.clone()]).unwrap();
    let person = folder.path().join("mine.pem");
    std::fs::write(&person, CA_SIGNED_AGAIN).unwrap();
    let environment = trust::git_environment(Some(&person));
    trust::set_accepted(Vec::new()).unwrap();
    assert!(
        environment.iter().all(|(name, _)| name != "GIT_SSL_NO_VERIFY" && name != "GIT_SSL_CAINFO"),
        "{environment:?}"
    );
    let answer = |url: &str| {
        let mut git = std::process::Command::new("git");
        git.args(["config", "--get-urlmatch", "http.sslCAInfo", url])
            .current_dir(folder.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        git.envs(environment.iter().map(|(name, value)| (name.as_str(), value.as_str())));
        let output = git.output().unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    let for_host = std::fs::read(answer("https://localhost/")).unwrap();
    assert_eq!(parse_certificates(&for_host), [leaf], "the pinned host trusts that certificate and nothing else");
    let elsewhere = std::fs::read(answer("https://a.example/")).unwrap();
    let elsewhere = parse_certificates(&elsewhere);
    assert!(elsewhere.contains(&root), "what the person added");
    assert!(elsewhere.contains(&own), "the person's own http.sslCAInfo stays");
    if std::path::Path::new("/etc/ssl/cert.pem").exists() {
        assert!(elsewhere.len() > 10, "the public roots stay: {}", elsewhere.len());
    }
}

#[test]
fn only_an_unknown_issuer_can_be_accepted_and_only_for_the_right_name_and_dates() {
    use rustls::pki_types::{CertificateDer, ServerName};
    let leaf = parse_certificates(LEAF.as_bytes()).remove(0);
    assert!(!trust::expired_at(&leaf, 1_800_000_000));
    assert!(trust::expired_at(&leaf, 9_000_000_000), "past its end");
    assert!(trust::expired_at(&leaf, 1_000), "before its start");
    let der = CertificateDer::from(leaf);
    assert!(super::names_the_host(&der, &ServerName::try_from("localhost").unwrap()));
    assert!(!super::names_the_host(&der, &ServerName::try_from("a.example").unwrap()));
}

#[test]
fn a_connection_nobody_asked_for_raises_no_prompt() {
    let _store = ONE_STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let folder = tempfile::tempdir().unwrap();
    trust::open(folder.path());
    trust::enable_prompts(|| {});
    let port = tls_target(CA_SIGNED_AGAIN, KEY_AGAIN);
    let url = format!("https://localhost:{port}/");
    let background = std::thread::spawn(move || {
        crate::agent_for(&url).get(&url).call().map(|_| ()).map_err(|error| error.to_string())
    });
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(4) && !background.is_finished() {
        std::thread::sleep(Duration::from_millis(20));
    }
    let waiting = trust::pending();
    for untrusted in &waiting {
        trust::answer(&untrusted.certificate.fingerprint, false).unwrap();
    }
    assert!(waiting.is_empty(), "it would wait two minutes for a dialog nobody expects: {waiting:?}");
    assert!(background.join().unwrap().is_err());
}

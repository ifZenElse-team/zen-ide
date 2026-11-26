use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError, RwLock};
use std::time::{Duration, Instant};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use schemars::JsonSchema;
use serde::Serialize;
use sha2::{Digest, Sha256};
use x509_cert::Certificate;
use x509_cert::der::{Decode, Encode, EncodePem, pem::LineEnding};

const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

/// A certificate as Settings → Server Certificates and the prompt show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CertificateInfo {
    /// The name the certificate was issued to.
    pub subject: String,
    /// The name of its issuer.
    pub issuer: String,
    /// The start of its validity.
    pub not_before: String,
    /// The end of its validity.
    pub not_after: String,
    /// The SHA-256 fingerprint in lowercase hex; names the certificate in commands.
    pub fingerprint: String,
    /// The one host it is accepted for; none for a certificate added as an issuer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

pub fn fingerprint(der: &[u8]) -> String {
    hex::encode(Sha256::digest(der))
}

pub fn describe(der: &[u8]) -> Option<CertificateInfo> {
    let certificate = Certificate::from_der(der).ok()?;
    let tbs = &certificate.tbs_certificate;
    Some(CertificateInfo {
        subject: tbs.subject.to_string(),
        issuer: tbs.issuer.to_string(),
        not_before: tbs.validity.not_before.to_string(),
        not_after: tbs.validity.not_after.to_string(),
        fingerprint: fingerprint(der),
        host: None,
    })
}

fn within_validity(der: &[u8], now: UnixTime) -> bool {
    let Ok(certificate) = Certificate::from_der(der) else { return false };
    let validity = certificate.tbs_certificate.validity;
    let now = Duration::from_secs(now.as_secs());
    validity.not_before.to_unix_duration() <= now && now <= validity.not_after.to_unix_duration()
}

fn names_the_host(end_entity: &CertificateDer<'_>, server_name: &ServerName<'_>) -> bool {
    rustls::server::ParsedCertificate::try_from(end_entity)
        .is_ok_and(|parsed| rustls::client::verify_server_name(&parsed, server_name).is_ok())
}

pub fn expired_at(der: &[u8], seconds: u64) -> bool {
    !within_validity(der, UnixTime::since_unix_epoch(Duration::from_secs(seconds)))
}

pub fn parse_certificates(bytes: &[u8]) -> Vec<Vec<u8>> {
    if bytes.windows(10).any(|window| window == b"-----BEGIN") {
        return pem_certificates(bytes);
    }
    if Certificate::from_der(bytes).is_ok() { vec![bytes.to_vec()] } else { Vec::new() }
}

fn pem_certificates(bytes: &[u8]) -> Vec<Vec<u8>> {
    let Ok(text) = std::str::from_utf8(bytes) else { return Vec::new() };
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
        let Some(end) = rest[start..].find("-----END CERTIFICATE-----") else { break };
        let end = start + end + "-----END CERTIFICATE-----".len();
        if let Ok(certificate) = Certificate::load_pem_chain(&rest.as_bytes()[start..end])
            && let Some(der) = certificate.first().and_then(|one| one.to_der().ok())
        {
            found.push(der);
        }
        rest = &rest[end..];
    }
    found
}

pub fn to_pem(certificates: &[Vec<u8>]) -> String {
    certificates.iter().filter_map(|der| Certificate::from_der(der).ok()?.to_pem(LineEnding::LF).ok()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub host: String,
    pub der: Vec<u8>,
}

const PIN_HOST: &str = "host: ";

fn pins_from(text: &str) -> Vec<Pin> {
    let mut pins = Vec::new();
    let mut parts = text.split(PIN_HOST).skip(1);
    for part in parts.by_ref() {
        let Some((host, pem)) = part.split_once('\n') else { continue };
        if let Some(der) = parse_certificates(pem.as_bytes()).into_iter().next() {
            pins.push(Pin { host: host.trim().to_ascii_lowercase(), der });
        }
    }
    pins
}

fn pins_to(pins: &[Pin]) -> String {
    pins.iter().map(|pin| format!("{PIN_HOST}{}\n{}", pin.host, to_pem(std::slice::from_ref(&pin.der)))).collect()
}

#[derive(Default)]
struct State {
    automatic: bool,
    accepted: Vec<Vec<u8>>,
    pinned: Vec<Pin>,
    file: Option<PathBuf>,
    pins_file: Option<PathBuf>,
    folder: Option<PathBuf>,
    config: Option<Arc<ClientConfig>>,
    bundle: Option<PathBuf>,
}

fn state() -> &'static RwLock<State> {
    static STATE: OnceLock<RwLock<State>> = OnceLock::new();
    STATE.get_or_init(RwLock::default)
}

fn write_state() -> std::sync::RwLockWriteGuard<'static, State> {
    state().write().unwrap_or_else(PoisonError::into_inner)
}

fn read_state() -> std::sync::RwLockReadGuard<'static, State> {
    state().read().unwrap_or_else(PoisonError::into_inner)
}

pub fn open(folder: &Path) {
    let file = folder.join("accepted.pem");
    let accepted = std::fs::read(&file).map(|bytes| parse_certificates(&bytes)).unwrap_or_default();
    let bundle = folder.join("ca-bundle.pem");
    let pins_file = folder.join("accepted-hosts.pem");
    let pinned = std::fs::read_to_string(&pins_file).map(|text| pins_from(&text)).unwrap_or_default();
    let mut state = write_state();
    state.file = Some(file);
    state.pins_file = Some(pins_file);
    state.folder = Some(folder.to_owned());
    state.bundle = Some(bundle);
    state.accepted = accepted;
    state.pinned = pinned;
    state.config = None;
}

pub fn set_automatic(automatic: bool) {
    write_state().automatic = automatic;
}

pub fn automatic() -> bool {
    read_state().automatic
}

pub fn accepted() -> Vec<Vec<u8>> {
    read_state().accepted.clone()
}

pub fn set_accepted(accepted: Vec<Vec<u8>>) -> std::io::Result<()> {
    let mut state = write_state();
    state.accepted = accepted;
    state.config = None;
    if let Some(file) = &state.file {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(file, to_pem(&state.accepted))?;
    }
    Ok(())
}

pub fn pinned() -> Vec<Pin> {
    read_state().pinned.clone()
}

fn set_pinned(pinned: Vec<Pin>) -> std::io::Result<()> {
    let mut state = write_state();
    state.pinned = pinned;
    if let Some(file) = &state.pins_file {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(file, pins_to(&state.pinned))?;
    }
    Ok(())
}

pub fn accept(der: Vec<u8>, host: &str) -> std::io::Result<()> {
    let host = host.to_ascii_lowercase();
    let mut pinned = pinned();
    if !pinned.iter().any(|pin| pin.host == host && pin.der == der) {
        pinned.push(Pin { host, der });
    }
    set_pinned(pinned)
}

fn is_pinned(der: &[u8], host: &str) -> bool {
    read_state().pinned.iter().any(|pin| pin.der == der && pin.host.eq_ignore_ascii_case(host))
}

pub fn remove(fingerprint_to_remove: &str) -> std::io::Result<bool> {
    let mut accepted = accepted();
    let before = accepted.len();
    accepted.retain(|der| fingerprint(der) != fingerprint_to_remove);
    let mut pinned = pinned();
    let pins_before = pinned.len();
    pinned.retain(|pin| fingerprint(&pin.der) != fingerprint_to_remove);
    let removed = accepted.len() != before || pinned.len() != pins_before;
    if accepted.len() != before {
        set_accepted(accepted)?;
    }
    if pinned.len() != pins_before {
        set_pinned(pinned)?;
    }
    Ok(removed)
}

fn system_roots() -> &'static Vec<Vec<u8>> {
    static ROOTS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    ROOTS.get_or_init(crate::keychain::roots)
}

fn verifier_roots(accepted: &[Vec<u8>]) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let extra = system_roots().iter().chain(accepted).map(|der| CertificateDer::from(der.clone()));
    roots.add_parsable_certificates(extra);
    roots
}

pub(crate) fn client_config() -> Result<Arc<ClientConfig>, Error> {
    crate::install_tls();
    if let Some(config) = read_state().config.clone() {
        return Ok(config);
    }
    let accepted = accepted();
    let provider = Arc::new(rustls_rustcrypto::provider());
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(verifier_roots(&accepted)), Arc::clone(&provider))
        .build()
        .map_err(|error| Error::General(error.to_string()))?;
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Verifier { inner }))
        .with_no_client_auth();
    let config = Arc::new(config);
    write_state().config = Some(Arc::clone(&config));
    Ok(config)
}

const DEFAULT_BUNDLES: [&str; 3] =
    ["/etc/ssl/cert.pem", "/etc/ssl/certs/ca-certificates.crt", "/etc/pki/tls/certs/ca-bundle.crt"];

fn base_bundle(person_ca: Option<&Path>) -> Vec<Vec<u8>> {
    let mut found = system_roots().clone();
    let listed = person_ca.map(Path::to_path_buf).into_iter().chain(DEFAULT_BUNDLES.iter().map(PathBuf::from));
    for path in listed {
        if let Ok(bytes) = std::fs::read(path) {
            found.extend(parse_certificates(&bytes));
        }
    }
    found
}

pub fn git_environment(person_ca: Option<&Path>) -> Vec<(String, String)> {
    let (accepted, pinned, bundle, folder) = {
        let state = read_state();
        (state.accepted.clone(), state.pinned.clone(), state.bundle.clone(), state.folder.clone())
    };
    let mut config = Vec::new();
    if let Some(bundle) = bundle.filter(|_| !accepted.is_empty()) {
        let base = base_bundle(person_ca);
        if !base.is_empty() {
            let all: Vec<Vec<u8>> = base.into_iter().chain(accepted).collect();
            if std::fs::write(&bundle, to_pem(&all)).is_ok() {
                config.push(("http.sslCAInfo".to_owned(), bundle.display().to_string()));
            }
        }
    }
    for pin in &pinned {
        let Some(file) = folder.as_ref().map(|folder| folder.join(format!("host-{}.pem", fingerprint(&pin.der))))
        else {
            continue;
        };
        if std::fs::write(&file, to_pem(std::slice::from_ref(&pin.der))).is_ok() {
            config.push((format!("http.https://{}/.sslCAInfo", pin.host), file.display().to_string()));
        }
    }
    if config.is_empty() {
        return Vec::new();
    }
    let first = std::env::var("GIT_CONFIG_COUNT").ok().and_then(|count| count.parse::<usize>().ok()).unwrap_or(0);
    let mut environment = vec![("GIT_CONFIG_COUNT".to_owned(), (first + config.len()).to_string())];
    for (index, (key, value)) in config.into_iter().enumerate() {
        environment.push((format!("GIT_CONFIG_KEY_{}", first + index), key));
        environment.push((format!("GIT_CONFIG_VALUE_{}", first + index), value));
    }
    environment
}

#[derive(Debug)]
struct Verifier {
    inner: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.inner.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now) {
            Err(error @ Error::InvalidCertificate(CertificateError::UnknownIssuer)) => {
                let der = end_entity.as_ref();
                if !within_validity(der, now) || !names_the_host(end_entity, server_name) {
                    return Err(error);
                }
                let host = server_name.to_str();
                if automatic() || is_pinned(der, &host) || ask(der, &host) {
                    return Ok(ServerCertVerified::assertion());
                }
                Err(error)
            }
            answer => answer,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// A server's certificate waiting for Accept or Reject.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Untrusted {
    /// The host that presented it.
    pub host: String,
    /// The certificate.
    pub certificate: CertificateInfo,
}

#[derive(Default)]
struct Prompts {
    enabled: bool,
    waiting: Vec<(Untrusted, Vec<u8>)>,
    rejected: HashSet<String>,
    accepted_now: HashSet<String>,
}

thread_local! {
    static ASKED_FOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn asking_the_person<T>(work: impl FnOnce() -> T) -> T {
    let before = ASKED_FOR.replace(true);
    let result = work();
    ASKED_FOR.set(before);
    result
}

fn answer_key(host: &str, fingerprint: &str) -> String {
    format!("{host} {fingerprint}")
}

type Notifier = Box<dyn Fn() + Send + Sync>;

fn prompts() -> &'static (Mutex<Prompts>, Condvar) {
    static PROMPTS: OnceLock<(Mutex<Prompts>, Condvar)> = OnceLock::new();
    PROMPTS.get_or_init(Default::default)
}

fn notifier() -> &'static Mutex<Option<Notifier>> {
    static NOTIFIER: Mutex<Option<Notifier>> = Mutex::new(None);
    &NOTIFIER
}

pub fn enable_prompts(notify: impl Fn() + Send + Sync + 'static) {
    prompts().0.lock().unwrap_or_else(PoisonError::into_inner).enabled = true;
    *notifier().lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(notify));
}

pub fn pending() -> Vec<Untrusted> {
    let prompts = prompts().0.lock().unwrap_or_else(PoisonError::into_inner);
    prompts.waiting.iter().map(|(untrusted, _)| untrusted.clone()).collect()
}

pub fn answer(fingerprint_of: &str, accept_it: bool) -> std::io::Result<bool> {
    let (lock, ready) = prompts();
    let mut prompts = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(at) =
        prompts.waiting.iter().position(|(untrusted, _)| untrusted.certificate.fingerprint == fingerprint_of)
    else {
        return Ok(false);
    };
    let (untrusted, der) = prompts.waiting.remove(at);
    let key = answer_key(&untrusted.host, fingerprint_of);
    if accept_it {
        prompts.accepted_now.insert(key);
    } else {
        prompts.rejected.insert(key);
    }
    drop(prompts);
    let stored = if accept_it { accept(der, &untrusted.host) } else { Ok(()) };
    ready.notify_all();
    stored.map(|()| true)
}

fn ask(der: &[u8], host: &str) -> bool {
    let Some(certificate) = describe(der) else { return false };
    let id = certificate.fingerprint.clone();
    let key = answer_key(host, &id);
    if !ASKED_FOR.get() {
        return false;
    }
    let (lock, ready) = prompts();
    let mut prompts = lock.lock().unwrap_or_else(PoisonError::into_inner);
    if !prompts.enabled || prompts.rejected.contains(&key) {
        return false;
    }
    if !prompts.waiting.iter().any(|(untrusted, _)| untrusted.certificate.fingerprint == id) {
        prompts.accepted_now.remove(&key);
        prompts.waiting.push((Untrusted { host: host.to_owned(), certificate }, der.to_vec()));
        if let Some(notify) = &*notifier().lock().unwrap_or_else(PoisonError::into_inner) {
            notify();
        }
    }
    let deadline = Instant::now() + ANSWER_TIMEOUT;
    loop {
        if prompts.accepted_now.contains(&key) {
            return true;
        }
        if prompts.rejected.contains(&key) {
            return false;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            prompts.waiting.retain(|(untrusted, _)| untrusted.certificate.fingerprint != id);
            return false;
        }
        prompts = ready.wait_timeout(prompts, left).unwrap_or_else(PoisonError::into_inner).0;
    }
}

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_net::trust::{self, CertificateInfo, Untrusted};

pub fn open_store(data: &Path) {
    trust::open(&data.join("ssl"));
    refresh_git();
}

pub fn apply(automatic: bool) {
    if trust::automatic() != automatic {
        trust::set_automatic(automatic);
        refresh_git();
    }
}

pub fn refresh_git() {
    let _ = std::thread::Builder::new().name("zen-git-trust".to_owned()).spawn(|| {
        let person_ca = zen_vcs::git::person_ca_file();
        let environment = trust::git_environment(person_ca.as_deref());
        let variables: Vec<(&str, &str)> =
            environment.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        zen_vcs::git::set_tls_environment(&variables);
    });
}

pub fn set_accepted(accepted: Vec<Vec<u8>>) -> std::io::Result<()> {
    trust::set_accepted(accepted)?;
    refresh_git();
    Ok(())
}

pub fn read_file(path: &Path) -> Result<Vec<Vec<u8>>, String> {
    let name = path.display().to_string();
    let bytes = std::fs::read(path)
        .map_err(|error| t!("network-certificate-unreadable", path = name.as_str(), error = error.to_string()))?;
    let found = trust::parse_certificates(&bytes);
    if found.is_empty() {
        return Err(t!("network-certificate-none", path = name));
    }
    Ok(found)
}

pub fn remove(fingerprint: &str) -> Option<String> {
    match trust::remove(fingerprint) {
        Ok(_) => {
            refresh_git();
            None
        }
        Err(error) => Some(error.to_string()),
    }
}

pub fn answer(fingerprint: &str, accept: bool) -> std::io::Result<bool> {
    let answered = trust::answer(fingerprint, accept)?;
    if answered && accept {
        refresh_git();
    }
    Ok(answered)
}

fn listed() -> Vec<CertificateInfo> {
    let added = trust::accepted().into_iter().filter_map(|der| trust::describe(&der));
    let pinned = trust::pinned()
        .into_iter()
        .filter_map(|pin| trust::describe(&pin.der).map(|info| CertificateInfo { host: Some(pin.host), ..info }));
    added.chain(pinned).collect()
}

/// The accepted certificates of Settings → Server Certificates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CertificateList {
    /// The accepted certificates.
    pub accepted: Vec<CertificateInfo>,
    /// Accept non-trusted certificates automatically is on.
    pub automatic: bool,
}

pub struct ListCertificates;

impl Command for ListCertificates {
    type Params = NoParams;
    type Output = CertificateList;
    const ID: &'static str = "settings.certificates.list";
    const TITLE: &'static str = "Server Certificates";
    const DESCRIPTION: &'static str = "Lists the certificates accepted in Settings → Server Certificates, and whether \
        non-trusted certificates are accepted automatically (acceptUntrustedCertificates in set_settings).";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<CertificateList, CommandError> {
        Ok(CertificateList { accepted: listed(), automatic: trust::automatic() })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddParams {
    /// A .crt, .cer or .pem file, PEM or DER.
    pub path: PathBuf,
}

pub struct AddCertificate;

impl Command for AddCertificate {
    type Params = AddParams;
    type Output = CertificateList;
    const ID: &'static str = "settings.certificates.add";
    const TITLE: &'static str = "Add Server Certificate";
    const DESCRIPTION: &'static str = "Adds the certificates of a .crt, .cer or .pem file to the accepted list, so \
        that the IDE's connections trust them: a server's own certificate, or the certificate of an issuer such as a \
        corporate root.";

    fn run(&self, session: &mut Session, params: AddParams) -> Result<CertificateList, CommandError> {
        super::person_only(session)?;
        let found = read_file(&params.path).map_err(CommandError::new)?;
        let mut accepted = trust::accepted();
        for der in found {
            if !accepted.contains(&der) {
                accepted.push(der);
            }
        }
        set_accepted(accepted).map_err(|error| CommandError::new(error.to_string()))?;
        Ok(CertificateList { accepted: listed(), automatic: trust::automatic() })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintParams {
    /// The SHA-256 fingerprint in lowercase hex.
    pub fingerprint: String,
}

pub struct RemoveCertificate;

impl Command for RemoveCertificate {
    type Params = FingerprintParams;
    type Output = CertificateList;
    const ID: &'static str = "settings.certificates.remove";
    const TITLE: &'static str = "Remove Server Certificate";
    const DESCRIPTION: &'static str = "Removes one certificate from the accepted list by its fingerprint.";

    fn run(&self, _: &mut Session, params: FingerprintParams) -> Result<CertificateList, CommandError> {
        match trust::remove(&params.fingerprint) {
            Ok(true) => refresh_git(),
            Ok(false) => return Err(CommandError::new(t!("network-certificate-unknown"))),
            Err(error) => return Err(CommandError::new(error.to_string())),
        }
        Ok(CertificateList { accepted: listed(), automatic: trust::automatic() })
    }
}

pub struct PendingCertificates;

impl Command for PendingCertificates {
    type Params = NoParams;
    type Output = Vec<Untrusted>;
    const ID: &'static str = "settings.certificates.pending";
    const TITLE: &'static str = "Untrusted Server Certificates";
    const DESCRIPTION: &'static str = "Lists the servers' certificates waiting for an answer in the Untrusted \
        Server's Certificate dialog; their connections wait until settings.certificates.answer or the dialog \
        decides, two minutes at most.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<Vec<Untrusted>, CommandError> {
        Ok(trust::pending())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnswerParams {
    /// The SHA-256 fingerprint in lowercase hex.
    pub fingerprint: String,
    /// Accept adds the certificate to the list; otherwise it is rejected for this session.
    pub accept: bool,
}

pub struct AnswerCertificate;

impl Command for AnswerCertificate {
    type Params = AnswerParams;
    type Output = ();
    const ID: &'static str = "settings.certificates.answer";
    const TITLE: &'static str = "Accept or Reject Server Certificate";
    const DESCRIPTION: &'static str = "Answers the Untrusted Server's Certificate dialog for one waiting certificate: \
        Accept adds it to the accepted list, Reject refuses it until the IDE restarts.";

    fn run(&self, session: &mut Session, params: AnswerParams) -> Result<(), CommandError> {
        super::person_only(session)?;
        match answer(&params.fingerprint, params.accept) {
            Ok(true) => Ok(()),
            Ok(false) => Err(CommandError::new(t!("network-certificate-unknown"))),
            Err(error) => Err(CommandError::new(error.to_string())),
        }
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListCertificates)?;
    registry.register(AddCertificate)?;
    registry.register(RemoveCertificate)?;
    registry.register(PendingCertificates)?;
    registry.register(AnswerCertificate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_a_certificate_is_refused_and_a_pem_one_is_read() {
        zen_i18n::register(crate::LOCALES);
        let folder = tempfile::tempdir().unwrap();
        let wrong = folder.path().join("wrong.pem");
        std::fs::write(&wrong, "not a certificate").unwrap();
        assert!(read_file(&wrong).unwrap_err().contains("wrong.pem"));
        let good = folder.path().join("root.cer");
        std::fs::write(&good, include_str!("../../../net/tests/fixtures/ca.pem")).unwrap();
        let found = read_file(&good).unwrap();
        assert_eq!(found.len(), 1);
        let info = trust::describe(&found[0]).unwrap();
        assert!(info.subject.contains("Zen Test Root"), "{info:?}");
    }
}

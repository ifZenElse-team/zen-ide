use std::sync::Once;

use mysql_async::{ClientIdentity, SslOpts};

use crate::registry::{TlsMode, TlsSettings};

pub(super) fn install_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::CryptoProvider::install_default(rustls_rustcrypto::provider());
    });
}

pub(super) fn options(settings: &TlsSettings, host: &str) -> Option<SslOpts> {
    let base = SslOpts::default().with_danger_tls_hostname_override(Some(host.to_owned()));
    let opts = match settings.mode {
        TlsMode::Disable => return None,
        TlsMode::Prefer | TlsMode::Require => base.with_danger_accept_invalid_certs(true),
        TlsMode::VerifyCa => base.with_danger_skip_domain_validation(true),
        TlsMode::VerifyFull => base,
    };
    let opts = match &settings.ca_file {
        Some(file) if matches!(settings.mode, TlsMode::VerifyCa | TlsMode::VerifyFull) => {
            opts.with_root_certs(vec![file.clone().into()]).with_disable_built_in_roots(true)
        }
        _ => opts,
    };
    Some(match (&settings.client_cert, &settings.client_key) {
        (Some(cert), Some(key)) => {
            opts.with_client_identity(Some(ClientIdentity::new(cert.clone().into(), key.clone().into())))
        }
        _ => opts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_choose_what_is_checked() {
        let mut settings = TlsSettings { mode: TlsMode::Disable, ..TlsSettings::default() };
        assert!(options(&settings, "db").is_none());
        settings.mode = TlsMode::Require;
        let required = options(&settings, "db").unwrap();
        assert!(required.accept_invalid_certs());
        settings.mode = TlsMode::VerifyCa;
        settings.ca_file = Some("/tmp/ca.pem".into());
        let verified = options(&settings, "db").unwrap();
        assert!(!verified.accept_invalid_certs() && verified.skip_domain_validation());
        assert!(verified.disable_built_in_roots() && verified.root_certs().len() == 1);
        settings.mode = TlsMode::VerifyFull;
        let full = options(&settings, "db").unwrap();
        assert!(!full.skip_domain_validation());
        assert_eq!(full.tls_hostname_override(), Some("db"));
    }
}

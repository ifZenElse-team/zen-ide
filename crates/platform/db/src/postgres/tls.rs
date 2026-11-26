use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::driver::{DbError, ErrorKind};
use crate::registry::{TlsMode, TlsSettings};

pub(super) fn connector(settings: &TlsSettings) -> Result<MakeRustlsConnect, DbError> {
    let provider = Arc::new(rustls_rustcrypto::provider());
    let failed = |error: &dyn std::fmt::Display| DbError::new(ErrorKind::Tls, error.to_string());
    let verifier: Arc<dyn ServerCertVerifier> = match settings.mode {
        TlsMode::Disable | TlsMode::Prefer | TlsMode::Require => Arc::new(AnyCertificate(Arc::clone(&provider))),
        TlsMode::VerifyCa | TlsMode::VerifyFull => {
            let mut roots = RootCertStore::empty();
            match &settings.ca_file {
                Some(file) => {
                    let certificates = CertificateDer::pem_file_iter(file).map_err(|error| failed(&error))?;
                    for certificate in certificates {
                        roots.add(certificate.map_err(|error| failed(&error))?).map_err(|error| failed(&error))?;
                    }
                }
                None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
            }
            let chain = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
                .build()
                .map_err(|error| failed(&error))?;
            if settings.mode == TlsMode::VerifyFull { chain } else { Arc::new(ChainOnly(chain)) }
        }
    };
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| failed(&error))?
        .dangerous()
        .with_custom_certificate_verifier(verifier);
    let mut config = match (&settings.client_cert, &settings.client_key) {
        (Some(cert), Some(key)) => {
            let certificates = CertificateDer::pem_file_iter(cert)
                .map_err(|error| failed(&error))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| failed(&error))?;
            let key = PrivateKeyDer::from_pem_file(key).map_err(|error| failed(&error))?;
            builder.with_client_auth_cert(certificates, key).map_err(|error| failed(&error))?
        }
        _ => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![b"postgresql".to_vec()];
    Ok(MakeRustlsConnect::new(config))
}

#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct ChainOnly(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for ChainOnly {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.0.verify_server_cert(end_entity, intermediates, server_name, ocsp, now) {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

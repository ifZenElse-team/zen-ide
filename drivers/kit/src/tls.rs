use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use serde_json::Value as Json;

use crate::Failure;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl Mode {
    pub fn of(tls: &Json) -> Self {
        match tls.get("mode").and_then(Json::as_str).unwrap_or_default() {
            "disable" => Mode::Disable,
            "require" => Mode::Require,
            "verify-ca" => Mode::VerifyCa,
            "verify-full" => Mode::VerifyFull,
            _ => Mode::Prefer,
        }
    }

    pub fn required(self) -> bool {
        matches!(self, Mode::Require | Mode::VerifyCa | Mode::VerifyFull)
    }
}

pub fn direct(tls: &Json) -> bool {
    tls.get("direct").and_then(Json::as_bool).unwrap_or(false)
}

pub fn client_config(tls: &Json, alpn: &[&[u8]]) -> Result<Arc<ClientConfig>, Failure> {
    config_with(tls, alpn, rustls::DEFAULT_VERSIONS)
}

pub fn client_config_tls12(tls: &Json) -> Result<Arc<ClientConfig>, Failure> {
    config_with(tls, &[], &[&rustls::version::TLS12])
}

fn config_with(
    tls: &Json,
    alpn: &[&[u8]],
    versions: &[&'static rustls::SupportedProtocolVersion],
) -> Result<Arc<ClientConfig>, Failure> {
    let provider = Arc::new(rustls_rustcrypto::provider());
    let failed = |error: &dyn std::fmt::Display| Failure::database("tls", error.to_string());
    let file = |key: &str| tls.get(key).and_then(Json::as_str).filter(|path| !path.is_empty());
    let mode = Mode::of(tls);
    let verifier: Arc<dyn ServerCertVerifier> = match mode {
        Mode::Disable | Mode::Prefer | Mode::Require => Arc::new(AnyCertificate(Arc::clone(&provider))),
        Mode::VerifyCa | Mode::VerifyFull => {
            let mut roots = RootCertStore::empty();
            match file("caFile") {
                Some(path) => {
                    let certificates = CertificateDer::pem_file_iter(path).map_err(|error| failed(&error))?;
                    for certificate in certificates {
                        roots.add(certificate.map_err(|error| failed(&error))?).map_err(|error| failed(&error))?;
                    }
                }
                None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
            }
            let chain = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
                .build()
                .map_err(|error| failed(&error))?;
            if mode == Mode::VerifyFull { chain } else { Arc::new(ChainOnly(chain)) }
        }
    };
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(versions)
        .map_err(|error| failed(&error))?
        .dangerous()
        .with_custom_certificate_verifier(verifier);
    let mut config = match (file("clientCert"), file("clientKey")) {
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
    config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
    Ok(Arc::new(config))
}

pub fn server_name(host: &str) -> Result<ServerName<'static>, Failure> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    ServerName::try_from(host.to_owned())
        .map_err(|_| Failure::database("tls", format!("`{host}` is not a host name TLS can check")))
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
        let algorithms = &self.0.signature_verification_algorithms;
        match verify_tls12_signature(message, cert, dss, algorithms) {
            Err(error) if unreadable(&error) => signed_by_key(message, cert, dss, algorithms),
            other => other,
        }
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algorithms = &self.0.signature_verification_algorithms;
        match verify_tls13_signature(message, cert, dss, algorithms) {
            Err(error) if unreadable(&error) && !legacy(dss.scheme) => signed_by_key(message, cert, dss, algorithms),
            other => other,
        }
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn legacy(scheme: SignatureScheme) -> bool {
    matches!(
        scheme,
        SignatureScheme::RSA_PKCS1_SHA1
            | SignatureScheme::ECDSA_SHA1_Legacy
            | SignatureScheme::RSA_PKCS1_SHA256
            | SignatureScheme::RSA_PKCS1_SHA384
            | SignatureScheme::RSA_PKCS1_SHA512
    )
}

fn unreadable(error: &rustls::Error) -> bool {
    matches!(
        error,
        rustls::Error::InvalidCertificate(
            CertificateError::Other(_) | CertificateError::BadEncoding | CertificateError::UnknownIssuer
        )
    )
}

fn signed_by_key(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
    algorithms: &WebPkiSupportedAlgorithms,
) -> Result<HandshakeSignatureValid, rustls::Error> {
    let bad = || rustls::Error::InvalidCertificate(CertificateError::BadSignature);
    let (algorithm, key) =
        public_key(cert.as_ref()).ok_or(rustls::Error::InvalidCertificate(CertificateError::BadEncoding))?;
    let candidates = algorithms
        .mapping
        .iter()
        .find(|(scheme, _)| *scheme == dss.scheme)
        .map(|(_, candidates)| *candidates)
        .ok_or_else(bad)?;
    candidates
        .iter()
        .filter(|candidate| candidate.public_key_alg_id().as_ref() == algorithm)
        .find(|candidate| candidate.verify_signature(key, message, dss.signature()).is_ok())
        .map(|_| HandshakeSignatureValid::assertion())
        .ok_or_else(bad)
}

pub(crate) fn public_key(der: &[u8]) -> Option<(&[u8], &[u8])> {
    let (_, certificate, _) = element(der)?;
    let (_, mut tbs, _) = element(certificate)?;
    let mut fields = Vec::new();
    while !tbs.is_empty() {
        let (tag, content, rest) = element(tbs)?;
        fields.push((tag, content));
        tbs = rest;
    }
    let skip = usize::from(fields.first()?.0 == 0xa0);
    let (tag, spki) = *fields.get(skip + 5)?;
    if tag != 0x30 {
        return None;
    }
    let (algorithm_tag, algorithm, rest) = element(spki)?;
    let (bits_tag, bits, _) = element(rest)?;
    if algorithm_tag != 0x30 || bits_tag != 0x03 || bits.first() != Some(&0) {
        return None;
    }
    Some((algorithm, &bits[1..]))
}

fn element(der: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *der.first()?;
    let first = *der.get(1)?;
    let (length, start) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 {
            return None;
        }
        let length = der.get(2..2 + count)?.iter().fold(0usize, |length, byte| (length << 8) | usize::from(*byte));
        (length, 2 + count)
    };
    let end = start.checked_add(length)?;
    Some((tag, der.get(start..end)?, der.get(end..)?))
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

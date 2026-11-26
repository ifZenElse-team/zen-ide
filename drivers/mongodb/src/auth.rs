use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bson::spec::BinarySubtype;
use bson::{Binary, Bson, Document, doc};
use hmac::{KeyInit, Mac};
use md5::Digest;
use tokio::io::{AsyncRead, AsyncWrite};
use zen_db_driver_kit::Failure;

use crate::wire::{Wire, WireError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mechanism {
    Sha1,
    Sha256,
}

impl Mechanism {
    pub(crate) fn named(name: &str) -> Result<Option<Self>, Failure> {
        match name.to_ascii_uppercase().as_str() {
            "" | "DEFAULT" => Ok(None),
            "SCRAM-SHA-256" => Ok(Some(Mechanism::Sha256)),
            "SCRAM-SHA-1" => Ok(Some(Mechanism::Sha1)),
            other => Err(Failure::unsupported(format!("the authentication mechanism {other} is not supported"))),
        }
    }

    pub(crate) fn offered(hello: &Document) -> Self {
        let offered = |name: &str| {
            hello
                .get_array("saslSupportedMechs")
                .is_ok_and(|mechanisms| mechanisms.iter().any(|mechanism| mechanism.as_str() == Some(name)))
        };
        if offered("SCRAM-SHA-256") || !offered("SCRAM-SHA-1") { Mechanism::Sha256 } else { Mechanism::Sha1 }
    }

    fn name(self) -> &'static str {
        match self {
            Mechanism::Sha1 => "SCRAM-SHA-1",
            Mechanism::Sha256 => "SCRAM-SHA-256",
        }
    }

    fn hmac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        match self {
            Mechanism::Sha1 => {
                let mut mac = <hmac::Hmac<sha1::Sha1> as KeyInit>::new_from_slice(key).expect("HMAC takes any key");
                mac.update(data);
                mac.finalize().into_bytes().to_vec()
            }
            Mechanism::Sha256 => {
                let mut mac = <hmac::Hmac<sha2::Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key");
                mac.update(data);
                mac.finalize().into_bytes().to_vec()
            }
        }
    }

    fn hash(self, data: &[u8]) -> Vec<u8> {
        match self {
            Mechanism::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Mechanism::Sha256 => sha2::Sha256::digest(data).to_vec(),
        }
    }

    fn password(self, user: &str, password: &str) -> Result<String, Failure> {
        match self {
            Mechanism::Sha1 => {
                let digest = md5::Md5::digest(format!("{user}:mongo:{password}").as_bytes());
                Ok(zen_db_driver_kit::value::hex(&digest))
            }
            Mechanism::Sha256 => stringprep::saslprep(password)
                .map(|prepared| prepared.into_owned())
                .map_err(|_| Failure::database("authentication", "the password has characters SASLprep refuses")),
        }
    }

    fn salted(self, password: &[u8], salt: &[u8], iterations: u32) -> Vec<u8> {
        let mut block = salt.to_vec();
        block.extend_from_slice(&1u32.to_be_bytes());
        let mut previous = self.hmac(password, &block);
        let mut result = previous.clone();
        for _ in 1..iterations {
            previous = self.hmac(password, &previous);
            for (out, byte) in result.iter_mut().zip(&previous) {
                *out ^= byte;
            }
        }
        result
    }
}

pub(crate) struct Scram {
    mechanism: Mechanism,
    user: String,
    password: String,
    nonce: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Proof {
    pub(crate) message: String,
    pub(crate) server_signature: Vec<u8>,
}

impl Scram {
    pub(crate) fn new(mechanism: Mechanism, user: &str, password: &str, nonce: String) -> Result<Self, Failure> {
        Ok(Self { mechanism, user: user.to_owned(), password: mechanism.password(user, password)?, nonce })
    }

    fn bare(&self) -> String {
        format!("n={},r={}", self.user.replace('=', "=3D").replace(',', "=2C"), self.nonce)
    }

    pub(crate) fn first(&self) -> String {
        format!("n,,{}", self.bare())
    }

    pub(crate) fn answer(&self, server_first: &str) -> Result<Proof, Failure> {
        let refused = |what: &str| Failure::database("authentication", format!("SCRAM: {what}"));
        let field = |name: char| {
            server_first.split(',').find_map(|part| part.strip_prefix(name).and_then(|rest| rest.strip_prefix('=')))
        };
        let nonce = field('r').ok_or_else(|| refused("the server sent no nonce"))?;
        if !nonce.starts_with(&self.nonce) || nonce.len() == self.nonce.len() {
            return Err(refused("the server's nonce does not extend ours"));
        }
        let salt = STANDARD.decode(field('s').unwrap_or_default()).map_err(|_| refused("the salt is not Base64"))?;
        let iterations: u32 =
            field('i').and_then(|count| count.parse().ok()).ok_or_else(|| refused("no iteration count"))?;
        if iterations < 4096 {
            return Err(refused("the server asks for too few iterations"));
        }
        let salted = self.mechanism.salted(self.password.as_bytes(), &salt, iterations);
        let client_key = self.mechanism.hmac(&salted, b"Client Key");
        let stored_key = self.mechanism.hash(&client_key);
        let without_proof = format!("c=biws,r={nonce}");
        let auth_message = format!("{},{server_first},{without_proof}", self.bare());
        let signature = self.mechanism.hmac(&stored_key, auth_message.as_bytes());
        let proof: Vec<u8> = client_key.iter().zip(&signature).map(|(key, sign)| key ^ sign).collect();
        let server_key = self.mechanism.hmac(&salted, b"Server Key");
        Ok(Proof {
            message: format!("{without_proof},p={}", STANDARD.encode(proof)),
            server_signature: self.mechanism.hmac(&server_key, auth_message.as_bytes()),
        })
    }
}

pub(crate) fn nonce() -> Result<String, Failure> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|error| Failure::database("authentication", error.to_string()))?;
    Ok(STANDARD.encode(bytes))
}

fn payload(text: &str) -> Bson {
    Bson::Binary(Binary { subtype: BinarySubtype::Generic, bytes: text.as_bytes().to_vec() })
}

fn text_of(reply: &Document) -> Result<String, Failure> {
    match reply.get("payload") {
        Some(Bson::Binary(binary)) => Ok(String::from_utf8_lossy(&binary.bytes).into_owned()),
        _ => Err(Failure::database("authentication", "SCRAM: the server's answer has no payload")),
    }
}

fn refused(error: WireError) -> Failure {
    match error {
        WireError::Server { message, .. } => Failure::database("authentication", message),
        lost => lost.failure(),
    }
}

pub(crate) async fn x509<S: AsyncRead + AsyncWrite + Unpin>(
    wire: &mut Wire<S>,
    user: Option<&str>,
) -> Result<(), Failure> {
    let mut command = doc! { "authenticate": 1, "mechanism": "MONGODB-X509" };
    if let Some(user) = user.filter(|user| !user.is_empty()) {
        command.insert("user", user);
    }
    wire.command("$external", command).await.map_err(refused)?;
    Ok(())
}

pub(crate) async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
    wire: &mut Wire<S>,
    mechanism: Mechanism,
    source: &str,
    user: &str,
    password: &str,
) -> Result<(), Failure> {
    let scram = Scram::new(mechanism, user, password, nonce()?)?;
    let started = wire
        .command(
            source,
            doc! {
                "saslStart": 1, "mechanism": mechanism.name(), "payload": payload(&scram.first()),
                "autoAuthorize": 1, "options": { "skipEmptyExchange": true },
            },
        )
        .await
        .map_err(refused)?;
    let conversation = started.get("conversationId").cloned().unwrap_or(Bson::Int32(1));
    let proof = scram.answer(&text_of(&started)?)?;
    let mut reply = wire
        .command(
            source,
            doc! { "saslContinue": 1, "conversationId": conversation.clone(), "payload": payload(&proof.message) },
        )
        .await
        .map_err(refused)?;
    let server_final = text_of(&reply)?;
    let verifier = server_final
        .split(',')
        .find_map(|part| part.strip_prefix("v="))
        .and_then(|signature| STANDARD.decode(signature).ok());
    if verifier.as_deref() != Some(proof.server_signature.as_slice()) {
        return Err(Failure::database("authentication", "SCRAM: the server could not prove it knows the password"));
    }
    while !reply.get_bool("done").unwrap_or(true) {
        reply = wire
            .command(source, doc! { "saslContinue": 1, "conversationId": conversation.clone(), "payload": payload("") })
            .await
            .map_err(refused)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

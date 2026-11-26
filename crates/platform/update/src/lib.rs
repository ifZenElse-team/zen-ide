use std::fmt;

#[cfg(not(target_arch = "wasm32"))]
pub mod fetch;
pub mod install;
pub mod licences;
pub mod scripts;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const OFFICIAL_MANIFEST_URL: &str = "https://ide.ifzenelse.space/releases/manifest.json";

pub const OFFICIAL_PLUGINS_URL: &str = "https://ide.ifzenelse.space/releases/plugins.json";

#[cfg(not(target_arch = "wasm32"))]
pub fn install_tls() {
    zen_net::install_tls();
}

pub const OFFICIAL_PUBLIC_KEY: &str = "464c3ceb2e14dfdf571c14dbb4114f21b3ee9014a0cacca158933ed6dab9bf33";

pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub products: Vec<Product>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Product {
    pub id: String,
    pub name: String,
    pub releases: Vec<Release>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub version: String,
    pub channel: String,
    pub date: String,
    #[serde(default)]
    pub notes: String,
    pub artifacts: Vec<Artifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub platform: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateError {
    BadEncoding(&'static str),
    BadSignature,
    BadManifest(String),
    UnsupportedSchema(u32),
    ArtifactMismatch { url: String },
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::BadEncoding(what) => write!(f, "the {what} is not valid"),
            UpdateError::BadSignature => f.write_str("the release manifest's signature does not verify"),
            UpdateError::BadManifest(reason) => write!(f, "the release manifest is not valid: {reason}"),
            UpdateError::UnsupportedSchema(version) => {
                write!(f, "the release manifest uses schema {version}; this build reads schema {SCHEMA}")
            }
            UpdateError::ArtifactMismatch { url } => write!(f, "the download of {url} does not match the manifest"),
        }
    }
}

impl std::error::Error for UpdateError {}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Manifest, UpdateError> {
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|error| UpdateError::BadManifest(error.to_string()))?;
        if manifest.schema != SCHEMA {
            return Err(UpdateError::UnsupportedSchema(manifest.schema));
        }
        Ok(manifest)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }

    pub fn product(&self, id: &str) -> Option<&Product> {
        self.products.iter().find(|product| product.id == id)
    }

    pub fn latest(&self, product: &str, channel: &str, platform: &str) -> Option<(&Release, &Artifact)> {
        self.product(product)?
            .releases
            .iter()
            .filter(|release| release.channel == channel)
            .filter_map(|release| {
                let version = semver::Version::parse(&release.version).ok()?;
                let artifact = release.artifacts.iter().find(|artifact| artifact.platform == platform)?;
                Some((version, release, artifact))
            })
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, release, artifact)| (release, artifact))
    }

    pub fn versions(&self, product: &str, channel: &str, platform: &str) -> Vec<(&Release, &Artifact)> {
        let Some(product) = self.product(product) else { return Vec::new() };
        let mut found: Vec<(semver::Version, &Release, &Artifact)> = product
            .releases
            .iter()
            .filter(|release| release.channel == channel)
            .filter_map(|release| {
                let version = semver::Version::parse(&release.version).ok()?;
                let artifact = release.artifacts.iter().find(|artifact| artifact.platform == platform)?;
                Some((version, release, artifact))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        found.into_iter().map(|(_, release, artifact)| (release, artifact)).collect()
    }

    pub fn add(&mut self, product_id: &str, product_name: &str, release: Release) {
        let product = match self.products.iter_mut().position(|product| product.id == product_id) {
            Some(index) => &mut self.products[index],
            None => {
                self.products.push(Product {
                    id: product_id.to_owned(),
                    name: product_name.to_owned(),
                    releases: Vec::new(),
                });
                self.products.last_mut().expect("just pushed")
            }
        };
        match product
            .releases
            .iter_mut()
            .find(|existing| existing.version == release.version && existing.channel == release.channel)
        {
            Some(existing) => {
                for artifact in release.artifacts {
                    existing.artifacts.retain(|other| other.platform != artifact.platform);
                    existing.artifacts.push(artifact);
                }
                existing.date = release.date;
                if !release.notes.is_empty() {
                    existing.notes = release.notes;
                }
            }
            None => product.releases.push(release),
        }
    }

    pub fn withdraw(&mut self, product_id: &str, version: Option<&str>) -> usize {
        let mut withdrawn = 0;
        for product in self.products.iter_mut().filter(|product| product.id == product_id) {
            let before = product.releases.len();
            product.releases.retain(|release| version.is_some_and(|version| release.version != version));
            withdrawn += before - product.releases.len();
        }
        self.products.retain(|product| !product.releases.is_empty());
        withdrawn
    }

    pub fn artifacts(&self) -> impl Iterator<Item = &Artifact> {
        self.products.iter().flat_map(|product| product.releases.iter()).flat_map(|release| release.artifacts.iter())
    }
}

pub fn verify_manifest(bytes: &[u8], signature_hex: &str, public_key_hex: &str) -> Result<Manifest, UpdateError> {
    verify_signature(bytes, signature_hex, public_key_hex)?;
    Manifest::parse(bytes)
}

pub fn verify_signature(bytes: &[u8], signature_hex: &str, public_key_hex: &str) -> Result<(), UpdateError> {
    let key = public_key(public_key_hex)?;
    let signature = signature(signature_hex)?;
    key.verify_strict(bytes, &signature).map_err(|_| UpdateError::BadSignature)
}

pub fn sign_manifest(bytes: &[u8], secret_key_hex: &str) -> Result<String, UpdateError> {
    Ok(hex::encode(secret_key(secret_key_hex)?.sign(bytes).to_bytes()))
}

pub fn public_key_of(secret_key_hex: &str) -> Result<String, UpdateError> {
    Ok(hex::encode(secret_key(secret_key_hex)?.verifying_key().to_bytes()))
}

pub fn key_pair_from_seed(seed: [u8; 32]) -> (String, String) {
    let key = SigningKey::from_bytes(&seed);
    (hex::encode(key.to_bytes()), hex::encode(key.verifying_key().to_bytes()))
}

pub fn check_public_key(public_key_hex: &str) -> Result<(), UpdateError> {
    public_key(public_key_hex).map(|_| ())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn check_artifact(artifact: &Artifact, bytes: &[u8]) -> Result<(), UpdateError> {
    let size_matches = artifact.size == 0 || bytes.len() as u64 == artifact.size;
    if size_matches && sha256_hex(bytes) == artifact.sha256 {
        Ok(())
    } else {
        Err(UpdateError::ArtifactMismatch { url: artifact.url.clone() })
    }
}

pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn decode<const N: usize>(text: &str, what: &'static str) -> Result<[u8; N], UpdateError> {
    let bytes = hex::decode(text.trim()).map_err(|_| UpdateError::BadEncoding(what))?;
    bytes.try_into().map_err(|_| UpdateError::BadEncoding(what))
}

fn public_key(text: &str) -> Result<VerifyingKey, UpdateError> {
    VerifyingKey::from_bytes(&decode(text, "public key")?).map_err(|_| UpdateError::BadEncoding("public key"))
}

fn secret_key(text: &str) -> Result<SigningKey, UpdateError> {
    Ok(SigningKey::from_bytes(&decode(text, "secret key")?))
}

fn signature(text: &str) -> Result<Signature, UpdateError> {
    Ok(Signature::from_bytes(&decode(text, "signature")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_newest_first_for_the_channel_and_platform() {
        let release = |version: &str, channel: &str, platform: &str| Release {
            version: version.into(),
            channel: channel.into(),
            date: "2026-10-17".into(),
            notes: String::new(),
            artifacts: vec![Artifact {
                platform: platform.into(),
                url: "artifacts/x".into(),
                sha256: String::new(),
                size: 0,
            }],
        };
        let mut manifest = Manifest { schema: SCHEMA, products: Vec::new() };
        for (version, channel, platform) in [
            ("0.9.0", "stable", "macos-aarch64"),
            ("0.10.0", "stable", "macos-aarch64"),
            ("0.11.0", "nightly", "macos-aarch64"),
            ("0.12.0", "stable", "linux-x86_64"),
        ] {
            manifest.add("gogarden", "GoGarden", release(version, channel, platform));
        }
        let listed: Vec<&str> = manifest
            .versions("gogarden", "stable", "macos-aarch64")
            .into_iter()
            .map(|(release, _)| release.version.as_str())
            .collect();
        assert_eq!(listed, ["0.10.0", "0.9.0"]);
        assert!(manifest.versions("rustronin", "stable", "macos-aarch64").is_empty());
    }

    fn release(version: &str, channel: &str, platform: &str) -> Release {
        Release {
            version: version.to_owned(),
            channel: channel.to_owned(),
            date: "2026-10-16".to_owned(),
            notes: String::new(),
            artifacts: vec![Artifact {
                platform: platform.to_owned(),
                url: format!("artifacts/rustronin/{version}/rustronin.tar.gz"),
                sha256: sha256_hex(version.as_bytes()),
                size: version.len() as u64,
            }],
        }
    }

    fn sample() -> Manifest {
        let mut manifest = Manifest { schema: SCHEMA, products: Vec::new() };
        manifest.add("rustronin", "RustRonin", release("0.9.0", "stable", "macos-aarch64"));
        manifest.add("rustronin", "RustRonin", release("0.10.0", "stable", "macos-aarch64"));
        manifest.add("rustronin", "RustRonin", release("0.11.0", "nightly", "macos-aarch64"));
        manifest.add("rustronin", "RustRonin", release("0.12.0", "stable", "linux-x86_64"));
        manifest
    }

    #[test]
    fn signed_manifests_verify_and_tampered_ones_do_not() {
        let (secret, public) = key_pair_from_seed([7; 32]);
        let bytes = sample().to_bytes();
        let signature = sign_manifest(&bytes, &secret).unwrap();
        assert_eq!(verify_manifest(&bytes, &signature, &public).unwrap(), sample());

        let mut tampered = bytes.clone();
        let index = tampered.iter().position(|&b| b == b'9').unwrap();
        tampered[index] = b'8';
        assert_eq!(verify_manifest(&tampered, &signature, &public), Err(UpdateError::BadSignature));

        let (_, other_public) = key_pair_from_seed([8; 32]);
        assert_eq!(verify_manifest(&bytes, &signature, &other_public), Err(UpdateError::BadSignature));
        assert_eq!(public_key_of(&secret).unwrap(), public);
        assert_eq!(verify_manifest(&bytes, "zz", &public), Err(UpdateError::BadEncoding("signature")));
    }

    #[test]
    fn latest_compares_semantic_versions_per_channel_and_platform() {
        let manifest = sample();
        let (release, _) = manifest.latest("rustronin", "stable", "macos-aarch64").unwrap();
        assert_eq!(release.version, "0.10.0", "0.10.0 is newer than 0.9.0");
        assert_eq!(manifest.latest("rustronin", "nightly", "macos-aarch64").unwrap().0.version, "0.11.0");
        assert!(manifest.latest("rustronin", "stable", "windows-x86_64").is_none());
        assert!(manifest.latest("gogarden", "stable", "macos-aarch64").is_none());
    }

    #[test]
    fn adding_the_same_platform_replaces_its_artifact() {
        let mut manifest = sample();
        let mut again = release("0.10.0", "stable", "macos-aarch64");
        again.artifacts[0].size = 99;
        manifest.add("rustronin", "RustRonin", again);
        let (release, artifact) = manifest.latest("rustronin", "stable", "macos-aarch64").unwrap();
        assert_eq!(release.artifacts.len(), 1);
        assert_eq!(artifact.size, 99);
    }

    #[test]
    fn withdrawing_releases() {
        let mut manifest = sample();
        assert_eq!(manifest.withdraw("rustronin", Some("0.10.0")), 1);
        assert_eq!(manifest.latest("rustronin", "stable", "macos-aarch64").unwrap().0.version, "0.9.0");
        assert_eq!(manifest.withdraw("rustronin", None), 3);
        assert!(manifest.product("rustronin").is_none(), "a product without releases disappears");
        assert_eq!(manifest.withdraw("nothing", None), 0);
    }

    #[test]
    fn unknown_schemas_and_fields_are_rejected() {
        assert_eq!(Manifest::parse(br#"{"schema":2,"products":[]}"#), Err(UpdateError::UnsupportedSchema(2)));
        assert!(matches!(
            Manifest::parse(br#"{"schema":1,"products":[],"extra":1}"#),
            Err(UpdateError::BadManifest(_))
        ));
    }

    #[test]
    fn artifacts_are_checked_by_size_and_hash() {
        let manifest = sample();
        let (_, artifact) = manifest.latest("rustronin", "stable", "macos-aarch64").unwrap();
        assert_eq!(check_artifact(artifact, b"0.10.0"), Ok(()));
        assert!(check_artifact(artifact, b"0.10.1").is_err());
    }

    #[test]
    fn artifact_sizes_are_read_and_an_old_manifest_without_them_still_parses() {
        let manifest = |size: &str| {
            format!(
                r#"{{"schema":1,"products":[{{"id":"gogarden","name":"GoGarden","releases":[{{"version":"0.38.7",
                "channel":"stable","date":"2026-10-19","artifacts":[{{"platform":"macos-aarch64",
                "url":"artifacts/gogarden/0.38.7/gogarden.tar.gz","sha256":"00"{size}}}]}}]}}]}}"#
            )
        };
        let size = |manifest: &Manifest| manifest.latest("gogarden", "stable", "macos-aarch64").unwrap().1.size;
        assert_eq!(size(&Manifest::parse(manifest(r#","size":41234567"#).as_bytes()).unwrap()), 41_234_567);
        assert_eq!(size(&Manifest::parse(manifest("").as_bytes()).unwrap()), 0, "no size reads as unknown");
        assert!(Manifest::parse(manifest(r#","size":"41 MB""#).as_bytes()).is_err());
    }

    #[test]
    fn an_artifact_of_unknown_size_is_checked_by_its_hash() {
        let artifact = Artifact { platform: String::new(), url: "a".into(), sha256: sha256_hex(b"bytes"), size: 0 };
        assert_eq!(check_artifact(&artifact, b"bytes"), Ok(()));
        assert!(check_artifact(&artifact, b"other").is_err());
    }

    #[test]
    fn the_current_platform_has_an_os_and_an_arch() {
        assert!(current_platform().contains('-'));
    }
}

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const API: &str = "https://api.adoptium.net/v3";

/// The feature releases one can download, as the API lists them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Releases {
    /// Every feature release still available, e.g. `[8, 11, 17, 21, 25]`, newest first.
    pub versions: Vec<u32>,
    /// The long-term support ones among them, newest first.
    pub lts: Vec<u32>,
}

/// One download: the newest build of a feature release for this computer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    /// The release's name, e.g. `jdk-21.0.5+11`.
    pub release: String,
    /// Its Java version, e.g. `21.0.5+11`.
    pub version: String,
    /// The archive's address.
    pub link: String,
    /// The archive's file name.
    pub name: String,
    /// Its SHA-256, in hex.
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
}

pub fn parse_releases(json: &[u8]) -> Result<Releases, String> {
    #[derive(Deserialize)]
    struct Answer {
        available_releases: Vec<u32>,
        available_lts_releases: Vec<u32>,
    }
    let answer: Answer = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let newest_first = |mut list: Vec<u32>| {
        list.sort_unstable_by(|a, b| b.cmp(a));
        list
    };
    Ok(Releases { versions: newest_first(answer.available_releases), lts: newest_first(answer.available_lts_releases) })
}

pub fn package_url(feature: u32, os: &str, arch: &str) -> String {
    format!("{API}/assets/latest/{feature}/hotspot?architecture={arch}&image_type=jdk&os={os}&vendor=eclipse")
}

pub fn parse_package(json: &[u8]) -> Result<Package, String> {
    #[derive(Deserialize)]
    struct Asset {
        binary: Binary,
        release_name: String,
        version: Version,
    }
    #[derive(Deserialize)]
    struct Binary {
        image_type: String,
        package: Archive,
    }
    #[derive(Deserialize)]
    struct Archive {
        link: String,
        name: String,
        checksum: String,
        size: u64,
    }
    #[derive(Deserialize)]
    struct Version {
        openjdk_version: String,
    }
    let assets: Vec<Asset> = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let asset =
        assets.into_iter().find(|asset| asset.binary.image_type == "jdk").ok_or("no JDK build for this computer")?;
    let archive = asset.binary.package;
    if !archive.link.starts_with("https://") {
        return Err(format!("not an HTTPS download: {}", archive.link));
    }
    Ok(Package {
        release: asset.release_name,
        version: asset.version.openjdk_version,
        link: archive.link,
        name: archive.name,
        sha256: archive.checksum.to_ascii_lowercase(),
        size: archive.size,
    })
}

pub fn platform() -> Result<(&'static str, &'static str), String> {
    let os = match std::env::consts::OS {
        "macos" => "mac",
        "linux" => "linux",
        other => return Err(format!("downloading a JDK is not supported on {other} yet")),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x64",
        other => return Err(format!("downloading a JDK is not supported on {other} yet")),
    };
    Ok((os, arch))
}

pub fn fetch_releases() -> Result<Releases, String> {
    let url = format!("{API}/info/available_releases");
    let bytes = document(&url, 1 << 20)?.ok_or_else(|| format!("{url}: not found"))?;
    parse_releases(&bytes)
}

pub fn fetch_package(feature: u32) -> Result<Package, String> {
    let (os, arch) = platform()?;
    let url = package_url(feature, os, arch);
    let bytes = document(&url, 4 << 20)?.ok_or_else(|| format!("no JDK {feature} for this computer"))?;
    parse_package(&bytes)
}

pub fn install(package: &Package, folder: &Path, progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let bytes = download(package, progress)?;
    unpack(&bytes, folder)
}

#[cfg(not(target_arch = "wasm32"))]
fn document(url: &str, limit: u64) -> Result<Option<Vec<u8>>, String> {
    zen_update::fetch::document(url, limit, std::time::Duration::from_secs(20)).map_err(|error| error.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn download(package: &Package, progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, String> {
    zen_update::fetch::checked_with_progress(&package.link, package.size, &package.sha256, progress)
        .map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
fn document(_: &str, _: u64) -> Result<Option<Vec<u8>>, String> {
    Err("downloads are not available here".to_owned())
}

#[cfg(target_arch = "wasm32")]
fn download(_: &Package, _: impl FnMut(u64, u64)) -> Result<Vec<u8>, String> {
    Err("downloads are not available here".to_owned())
}

pub fn unpack(archive: &[u8], folder: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let temporary = folder.join(format!(".unpacking-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temporary);
    std::fs::create_dir_all(&temporary).map_err(|error| format!("{}: {error}", temporary.display()))?;
    let result = (|| {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        tar.set_preserve_permissions(true);
        for entry in tar.entries().map_err(|error| error.to_string())? {
            let mut entry = entry.map_err(|error| error.to_string())?;
            entry.unpack_in(&temporary).map_err(|error| error.to_string())?;
        }
        let tops: Vec<PathBuf> = std::fs::read_dir(&temporary)
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        let [top] = tops.as_slice() else { return Err("the archive is not one JDK folder".to_owned()) };
        let name = top.file_name().ok_or("the archive has no folder")?;
        let target = folder.join(name);
        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(|error| format!("{}: {error}", target.display()))?;
        }
        std::fs::rename(top, &target).map_err(|error| format!("{}: {error}", target.display()))?;
        Ok(target)
    })();
    let _ = std::fs::remove_dir_all(&temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_answers_are_read() {
        let releases = parse_releases(
            br#"{"available_lts_releases":[8,11,17,21,25],"available_releases":[8,11,17,21,22,23,24,25],"most_recent_feature_release":25,"most_recent_lts":25,"tip_version":26}"#,
        )
        .unwrap();
        assert_eq!(releases.versions[..2], [25, 24]);
        assert_eq!(releases.lts, [25, 21, 17, 11, 8]);

        let asset = br#"[{"binary":{"architecture":"aarch64","image_type":"jdk","os":"mac","package":{"checksum":"ABCDEF","link":"https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.5%2B11/OpenJDK21U-jdk_aarch64_mac_hotspot_21.0.5_11.tar.gz","name":"OpenJDK21U-jdk_aarch64_mac_hotspot_21.0.5_11.tar.gz","size":190000000}},"release_name":"jdk-21.0.5+11","vendor":"eclipse","version":{"major":21,"openjdk_version":"21.0.5+11","semver":"21.0.5+11"}}]"#;
        let package = parse_package(asset).unwrap();
        assert_eq!(package.release, "jdk-21.0.5+11");
        assert_eq!(package.sha256, "abcdef");
        assert_eq!(package.size, 190_000_000);
        assert!(parse_package(b"[]").is_err());
        let plain = String::from_utf8_lossy(asset).replace("https://", "http://");
        assert!(parse_package(plain.as_bytes()).unwrap_err().contains("HTTPS"));
        assert_eq!(
            package_url(21, "mac", "aarch64"),
            "https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=aarch64&image_type=jdk&os=mac&vendor=eclipse"
        );
    }

    #[test]
    fn an_archive_unpacks_into_its_own_folder() {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut add = |path: &str, text: &str| {
            let mut header = tar::Header::new_gnu();
            header.set_size(text.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, text.as_bytes()).unwrap();
        };
        add("jdk-21.0.5+11/Contents/Home/bin/java", "");
        add("jdk-21.0.5+11/Contents/Home/release", "JAVA_VERSION=\"21.0.5\"\n");
        let archive = builder.into_inner().unwrap().finish().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let unpacked = unpack(&archive, dir.path()).unwrap();
        assert_eq!(unpacked, dir.path().join("jdk-21.0.5+11"));
        let jdk = crate::jdk::detect::read(&unpacked, crate::jdk::JdkSource::Downloaded).unwrap();
        assert_eq!(jdk.major, 21);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temporary folder is left");
        assert!(unpack(b"not an archive", dir.path()).is_err());
    }
}

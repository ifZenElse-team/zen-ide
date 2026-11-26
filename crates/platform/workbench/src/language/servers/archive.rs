use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const STAMP: &str = ".zen-archive";

pub const MAX_ARCHIVE: u64 = 1 << 30;

#[cfg(not(target_arch = "wasm32"))]
pub fn download(url: &str, progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, String> {
    zen_update::fetch::download(url, MAX_ARCHIVE, progress).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn download(url: &str, _progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, String> {
    Err(format!("cannot download {url} here"))
}

pub fn unpack(bytes: &[u8], sha256: &str, url: &str, folder: &Path) -> Result<(), String> {
    let found = hex(&Sha256::digest(bytes));
    if !found.eq_ignore_ascii_case(sha256.trim()) {
        return Err(format!("the SHA-256 of {url} is {found}, not {sha256}"));
    }
    let parent = folder.parent().ok_or("no parent folder")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let staging = parent.join(format!(".{name}.partial"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let unpacked = (|| {
        if bytes.starts_with(b"PK\x03\x04") {
            unzip(bytes, &staging, url)?;
        } else {
            tar::Archive::new(flate2::read::GzDecoder::new(bytes))
                .unpack(&staging)
                .map_err(|error| format!("cannot unpack {url}: {error}"))?;
        }
        let entries: Vec<_> = std::fs::read_dir(&staging).map_err(|error| error.to_string())?.flatten().collect();
        let root = match entries.as_slice() {
            [only] if only.path().is_dir() => only.path(),
            _ => staging.clone(),
        };
        std::fs::write(root.join(STAMP), url).map_err(|error| error.to_string())?;
        let _ = std::fs::remove_dir_all(folder);
        std::fs::rename(&root, folder).map_err(|error| error.to_string())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    unpacked
}

fn unzip(bytes: &[u8], staging: &Path, url: &str) -> Result<(), String> {
    let archive = staging.with_extension("zip");
    std::fs::write(&archive, bytes).map_err(|error| format!("{}: {error}", archive.display()))?;
    let output = std::process::Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(staging).output();
    let _ = std::fs::remove_file(&archive);
    let output = output.map_err(|error| format!("cannot unpack {url}: tar: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("cannot unpack {url}: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

pub fn inside(folder: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty() || !path.components().all(|part| matches!(part, std::path::Component::Normal(_))) {
        return Err(format!("{relative} is not a path inside the server's folder"));
    }
    Ok(folder.join(path))
}

pub fn place_file(bytes: &[u8], sha256: &str, url: &str, path: &Path) -> Result<(), String> {
    let found = hex(&Sha256::digest(bytes));
    if !found.eq_ignore_ascii_case(sha256.trim()) {
        return Err(format!("the SHA-256 of {url} is {found}, not {sha256}"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn installed_version(folder: &Path) -> Option<String> {
    let url = std::fs::read_to_string(folder.join(STAMP)).ok()?;
    let is_version = |part: &str| {
        part.contains('.')
            && part.split('.').all(|piece| !piece.is_empty() && piece.bytes().all(|b| b.is_ascii_digit()))
    };
    let mut segments = url.trim().rsplit('/');
    let name = segments.next()?;
    let name = [".tar.gz", ".tgz", ".zip", ".sit"].iter().find_map(|suffix| name.strip_suffix(suffix)).unwrap_or(name);
    name.split(['-', '_'])
        .find(|part| is_version(part))
        .or_else(|| segments.take(2).find(|segment| is_version(segment)))
        .map(str::to_owned)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn java_version(home: &Path) -> Option<u32> {
    let program = home.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
    if !program.is_file() {
        return None;
    }
    let release = std::fs::read_to_string(home.join("release")).ok()?;
    let value = release.lines().find_map(|line| line.strip_prefix("JAVA_VERSION="))?.trim().trim_matches('"');
    let mut parts = value.split(['.', '_', '-', '+']);
    match parts.next()?.parse().ok()? {
        1 => parts.next()?.parse().ok(),
        major => Some(major),
    }
}

pub fn newest_java(homes: impl IntoIterator<Item = PathBuf>, version: u32) -> Option<PathBuf> {
    homes
        .into_iter()
        .filter_map(|home| java_version(&home).filter(|&found| found >= version).map(|found| (found, home)))
        .max_by_key(|(found, _)| *found)
        .map(|(_, home)| home)
}

pub fn workspace_data(caches: &Path, id: &str, root: &Path) -> PathBuf {
    let hash = hex(&Sha256::digest(root.to_string_lossy().as_bytes()));
    let name: String = root
        .file_name()
        .map(|name| {
            name.to_string_lossy().chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect()
        })
        .unwrap_or_default();
    caches.join("language-servers").join(id).join(format!("{name}-{}", &hash[..16]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn an_archive_is_checked_then_unpacked_in_place_of_the_old_one() {
        let data = tempfile::tempdir().unwrap();
        let folder = data.path().join("language-servers/jdtls");
        let bytes =
            tar_gz(&[("plugins/org.eclipse.equinox.launcher_1.8.0.jar", b"jar"), ("config_mac/config.ini", b"x")]);
        let sha = hex(&Sha256::digest(&bytes));
        let url = "https://download.example/jdtls/jdt-language-server-1.63.0-202610191322.tar.gz";

        let wrong = unpack(&bytes, &"0".repeat(64), url, &folder).unwrap_err();
        assert!(wrong.contains("SHA-256"), "{wrong}");
        assert!(!folder.exists(), "nothing unpacked from an archive that does not match");

        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("old.txt"), "old").unwrap();
        unpack(&bytes, &sha.to_uppercase(), url, &folder).unwrap();
        assert!(folder.join("plugins/org.eclipse.equinox.launcher_1.8.0.jar").is_file());
        assert!(!folder.join("old.txt").exists(), "the old server is replaced");
        assert_eq!(installed_version(&folder).as_deref(), Some("1.63.0"));
        let left: Vec<_> =
            std::fs::read_dir(folder.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["jdtls"], "no staging folder is left");
    }

    #[cfg(unix)]
    #[test]
    fn a_zip_archive_is_unpacked_too_and_its_version_read_from_the_url() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src/server/bin");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("kotlin-language-server"), "#!/bin/sh\n").unwrap();
        let zip = dir.path().join("server.zip");
        let made = std::process::Command::new("zip")
            .current_dir(dir.path().join("src"))
            .args(["-qr", zip.to_str().unwrap(), "server"])
            .status();
        if !made.is_ok_and(|status| status.success()) {
            return;
        }
        let bytes = std::fs::read(&zip).unwrap();
        let sha = hex(&Sha256::digest(&bytes));
        let folder = dir.path().join("language-servers/kotlin-language-server");
        let url = "https://github.com/fwcd/kotlin-language-server/releases/download/1.3.13/server.zip";
        unpack(&bytes, &sha, url, &folder).unwrap();
        assert!(folder.join("bin/kotlin-language-server").is_file(), "its one top folder is unwrapped");
        assert_eq!(installed_version(&folder).as_deref(), Some("1.3.13"));
        std::fs::write(folder.join(STAMP), "https://x/kotlin-server-263.7084.0-aarch64.sit").unwrap();
        assert_eq!(installed_version(&folder).as_deref(), Some("263.7084.0"));
    }

    #[test]
    fn one_top_folder_is_unwrapped_and_nothing_escapes_the_folder() {
        let data = tempfile::tempdir().unwrap();
        let folder = data.path().join("server");
        let bytes = tar_gz(&[("server-2.0/bin/run", b"#!"), ("server-2.0/lib/a.jar", b"a")]);
        unpack(&bytes, &hex(&Sha256::digest(&bytes)), "https://x/server-2.0.tar.gz", &folder).unwrap();
        assert!(folder.join("bin/run").is_file() && folder.join("lib/a.jar").is_file());
        assert_eq!(installed_version(&folder).as_deref(), Some("2.0"));

        let mut raw = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.as_gnu_mut().unwrap().name[..12].copy_from_slice(b"../escape.tx");
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        raw.append(&header, &b"x"[..]).unwrap();
        let bytes = raw.into_inner().unwrap().finish().unwrap();
        let _ = unpack(&bytes, &hex(&Sha256::digest(&bytes)), "https://x/evil.tar.gz", &data.path().join("evil"));
        assert!(!data.path().join("escape.tx").exists());
    }

    fn jdk(folder: &Path, release: &str) -> PathBuf {
        std::fs::create_dir_all(folder.join("bin")).unwrap();
        std::fs::write(folder.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" }), "").unwrap();
        std::fs::write(folder.join("release"), release).unwrap();
        folder.to_owned()
    }

    #[test]
    fn the_newest_jdk_new_enough_is_chosen() {
        let homes = tempfile::tempdir().unwrap();
        let old = jdk(&homes.path().join("8"), "JAVA_VERSION=\"1.8.0_402\"\n");
        let lts = jdk(&homes.path().join("21"), "IMPLEMENTOR=\"x\"\nJAVA_VERSION=\"21.0.13\"\n");
        let new = jdk(&homes.path().join("25"), "JAVA_VERSION=\"25\"\n");
        assert_eq!(java_version(&old), Some(8));
        assert_eq!(java_version(&lts), Some(21));
        assert_eq!(java_version(&homes.path().join("none")), None);
        assert_eq!(newest_java([old.clone(), lts.clone(), new.clone()], 21), Some(new));
        assert_eq!(newest_java([old.clone(), lts.clone()], 25), None);
        assert_eq!(newest_java([old, lts.clone()], 17), Some(lts));
    }

    #[test]
    fn workspace_data_is_per_project_and_outside_it() {
        let caches = Path::new("/data/caches");
        let one = workspace_data(caches, "jdtls", Path::new("/work/shop"));
        let two = workspace_data(caches, "jdtls", Path::new("/other/shop"));
        assert_ne!(one, two);
        assert!(one.starts_with("/data/caches/language-servers/jdtls"));
        assert!(one.file_name().unwrap().to_string_lossy().starts_with("shop-"));
    }
}

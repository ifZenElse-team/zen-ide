use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Kind, PROGRAM, only_mine, private_folder};

pub(crate) fn bin_folder(dir: &Path) -> PathBuf {
    dir.join("bin")
}

pub(crate) struct Starting(File);

impl Starting {
    pub(crate) fn lock(dir: &Path) -> io::Result<Starting> {
        private_folder(dir)?;
        let file = OpenOptions::new().create(true).append(true).mode(0o600).open(dir.join("start.lock"))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)?;
        Ok(Starting(file))
    }
}

impl Drop for Starting {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(&self.0, rustix::fs::FlockOperation::Unlock);
    }
}

pub(crate) fn build_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)[..16].iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn runnable(dir: &Path, protocol: u32, source: &Path) -> io::Result<PathBuf> {
    let bytes = std::fs::read(source)?;
    let build = build_of(&bytes);
    let bin = bin_folder(dir);
    let versions = bin.join(protocol.to_string());
    let folder = versions.join(&build);
    for level in [dir, bin.as_path(), versions.as_path(), folder.as_path()] {
        private_folder(level)?;
    }
    let copy = folder.join(PROGRAM);
    if trusted(&copy, &build).is_err() {
        if std::fs::symlink_metadata(&copy).is_ok() {
            std::fs::remove_file(&copy)?;
        }
        write_copy(&folder, &copy, &bytes)?;
    }
    trusted(&copy, &build)?;
    remove_unused(dir, &folder);
    Ok(copy)
}

pub(crate) fn trusted(copy: &Path, build: &str) -> io::Result<()> {
    only_mine(copy, Kind::File)?;
    if build_of(&std::fs::read(copy)?) == build {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} changed after it was copied and is not run", copy.display()),
        ))
    }
}

fn write_copy(folder: &Path, copy: &Path, bytes: &[u8]) -> io::Result<()> {
    let part = folder.join(format!(".{PROGRAM}-{}.part", std::process::id()));
    let _ = std::fs::remove_file(&part);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o700).open(&part)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o500))?;
    std::fs::rename(&part, copy)
}

pub(crate) struct Running {
    pub(crate) program: Option<PathBuf>,
}

pub(crate) fn running(dir: &Path, protocol: u32) -> Option<Running> {
    let text = std::fs::read_to_string(dir.join(format!("host-{protocol}.json"))).ok()?;
    let info: serde_json::Value = serde_json::from_str(&text).ok()?;
    let pid = rustix::process::Pid::from_raw(i32::try_from(info["pid"].as_u64()?).ok()?)?;
    rustix::process::test_kill_process(pid).ok()?;
    Some(Running { program: info["program"].as_str().map(PathBuf::from) })
}

fn remove_unused(dir: &Path, keep: &Path) {
    let in_use: HashSet<PathBuf> = protocols(dir)
        .into_iter()
        .filter_map(|protocol| running(dir, protocol)?.program?.parent().map(Path::to_path_buf))
        .collect();
    let Ok(versions) = std::fs::read_dir(bin_folder(dir)) else { return };
    for versions in versions.flatten().map(|entry| entry.path()) {
        let Ok(builds) = std::fs::read_dir(&versions) else { continue };
        for build in builds.flatten().map(|entry| entry.path()) {
            if build != keep && !in_use.contains(&build) {
                let _ = std::fs::remove_dir_all(&build);
            }
        }
        let _ = std::fs::remove_dir(&versions);
    }
}

fn protocols(dir: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            name.to_str()?.strip_prefix("host-")?.strip_suffix(".json")?.parse().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    fn bundle(dir: &Path, bytes: &[u8]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let program = dir.join(PROGRAM);
        let _ = std::fs::remove_file(&program);
        std::fs::write(&program, bytes).unwrap();
        program
    }

    #[test]
    fn the_copy_is_outside_the_bundle_private_and_named_by_its_build() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        let source = bundle(&temp.path().join("Old.app"), b"build one");
        let copy = runnable(&dir, 4, &source).unwrap();
        assert_eq!(copy, dir.join("bin").join("4").join(build_of(b"build one")).join(PROGRAM));
        assert_eq!(std::fs::read(&copy).unwrap(), b"build one");
        assert_eq!(std::fs::metadata(&copy).unwrap().mode() & 0o777, 0o500);
        for folder in [dir.join("bin"), dir.join("bin").join("4"), copy.parent().unwrap().to_owned()] {
            assert_eq!(std::fs::metadata(&folder).unwrap().mode() & 0o777, 0o700, "{}", folder.display());
        }
        std::fs::remove_dir_all(temp.path().join("Old.app")).unwrap();
        assert!(copy.is_file(), "removing the bundle leaves the copy");
    }

    #[test]
    fn a_new_build_gets_its_own_copy_and_unused_copies_go() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        let first = runnable(&dir, 1, &bundle(&temp.path().join("app"), b"build one")).unwrap();
        let second = runnable(&dir, 1, &bundle(&temp.path().join("app"), b"build two")).unwrap();
        assert_ne!(first.parent(), second.parent());
        assert!(!first.exists(), "no host runs the first copy any more");
        assert!(second.is_file());
    }

    #[test]
    fn a_copy_a_host_runs_is_kept() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        let first = runnable(&dir, 1, &bundle(&temp.path().join("app"), b"build one")).unwrap();
        let info = serde_json::json!({ "pid": std::process::id(), "program": first, "protocol": 1 });
        std::fs::write(dir.join("host-1.json"), info.to_string()).unwrap();
        let second = runnable(&dir, 2, &bundle(&temp.path().join("app"), b"build two")).unwrap();
        assert!(first.is_file(), "a running host's copy stays");
        assert!(second.is_file());
    }

    #[test]
    fn a_changed_copy_is_never_run_and_is_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        let source = bundle(&temp.path().join("app"), b"build one");
        let copy = runnable(&dir, 1, &source).unwrap();
        let build = build_of(b"build one");
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&copy, b"something else").unwrap();
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(trusted(&copy, &build).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(runnable(&dir, 1, &source).unwrap(), copy);
        assert_eq!(std::fs::read(&copy).unwrap(), b"build one", "copied again from the bundle");
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(trusted(&copy, &build).unwrap_err().kind(), io::ErrorKind::PermissionDenied, "others can read it");
    }

    #[test]
    fn a_link_in_the_copys_place_is_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        let source = bundle(&temp.path().join("app"), b"build one");
        let folder = dir.join("bin").join("1").join(build_of(b"build one"));
        for level in [dir.as_path(), &dir.join("bin"), &dir.join("bin").join("1"), &folder] {
            private_folder(level).unwrap();
        }
        let elsewhere = bundle(&temp.path().join("other"), b"build one");
        std::os::unix::fs::symlink(&elsewhere, folder.join(PROGRAM)).unwrap();
        let copy = runnable(&dir, 1, &source).unwrap();
        assert!(!std::fs::symlink_metadata(&copy).unwrap().file_type().is_symlink(), "a real file now");
        assert!(elsewhere.is_file(), "the link's target is untouched");
    }
}

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{Kind, only_mine, private_folder};
use crate::HostedSession;

pub const MAX_LINES: usize = 2_000;

pub const MAX_TEXT: usize = 1 << 20;

pub const MAX_COMMAND: usize = 4 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub cwd: PathBuf,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<Vec<String>>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub saved: u64,
}

pub(crate) fn folder(dir: &Path) -> PathBuf {
    dir.join("sessions")
}

fn path(dir: &Path, instance: u64, id: u64) -> PathBuf {
    folder(dir).join(format!("{instance}-{id}.json"))
}

pub fn write(dir: &Path, instance: u64, id: u64, record: &Record) -> io::Result<()> {
    let folder = folder(dir);
    private_folder(&folder)?;
    let target = path(dir, instance, id);
    let part = folder.join(format!(".{instance}-{id}.part"));
    let _ = std::fs::remove_file(&part);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&part)?;
    file.write_all(&serde_json::to_vec(record).map_err(io::Error::other)?)?;
    drop(file);
    std::fs::rename(&part, target)
}

pub(crate) fn remove(dir: &Path, instance: u64, id: u64) {
    let _ = std::fs::remove_file(path(dir, instance, id));
}

pub(crate) fn read(dir: &Path, hosted: HostedSession) -> Option<Record> {
    only_mine(&folder(dir), Kind::Folder).ok()?;
    let path = path(dir, hosted.host, hosted.id);
    only_mine(&path, Kind::File).ok()?;
    let text = std::fs::read(&path).ok()?;
    if text.len() > 2 * MAX_TEXT {
        return None;
    }
    serde_json::from_slice(&text).ok()
}

pub(crate) fn forget(dir: &Path, hosted: HostedSession) {
    remove(dir, hosted.host, hosted.id);
}

pub(crate) fn forget_all(dir: &Path) {
    if only_mine(&folder(dir), Kind::Folder).is_ok() {
        let _ = std::fs::remove_dir_all(folder(dir));
    }
}

pub(crate) fn forget_older_than(dir: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(folder(dir)) else { return };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|since| since > age));
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub fn booted_at() -> Option<u64> {
    if cfg!(target_os = "linux") {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        return stat.lines().find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok());
    }
    let output = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "kern.boottime"]).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    text.split("sec =").nth(1)?.split(',').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_private_and_comes_back() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        private_folder(&dir).unwrap();
        let record = Record {
            cwd: "/tmp".into(),
            command: Some("make".to_owned()),
            text: "out".to_owned(),
            ..Record::default()
        };
        write(&dir, 7, 3, &record).unwrap();
        let hosted = HostedSession { id: 3, protocol: 1, host: 7 };
        assert_eq!(read(&dir, hosted), Some(record));
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(path(&dir, 7, 3)).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(folder(&dir)).unwrap().mode() & 0o777, 0o700);
        forget(&dir, hosted);
        assert_eq!(read(&dir, hosted), None);
    }

    #[test]
    fn a_record_others_can_read_is_not_used() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        private_folder(&dir).unwrap();
        write(&dir, 7, 3, &Record::default()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path(&dir, 7, 3), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read(&dir, HostedSession { id: 3, protocol: 1, host: 7 }), None);
    }

    #[test]
    fn the_boot_time_is_known_and_in_the_past() {
        let booted = booted_at().expect("the system's boot time");
        assert!(booted > 0 && booted <= now(), "{booted}");
    }
}

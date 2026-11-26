use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{Kind, only_mine, private_folder};

pub const MAX_REPORT: u64 = 64 * 1024;

pub fn folder(dir: &Path) -> PathBuf {
    dir.join("agents")
}

pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len()) && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn path(dir: &Path, terminal: &str, agent: &str) -> Option<PathBuf> {
    (valid_name(terminal) && valid_name(agent)).then(|| folder(dir).join(format!("{terminal}-{agent}.json")))
}

pub(crate) fn ensure(dir: &Path) -> io::Result<PathBuf> {
    private_folder(dir)?;
    let folder = folder(dir);
    private_folder(&folder)?;
    Ok(folder)
}

pub(crate) fn read(dir: &Path, terminal: &str, agent: &str) -> Option<serde_json::Value> {
    only_mine(&folder(dir), Kind::Folder).ok()?;
    let path = path(dir, terminal, agent)?;
    let meta = std::fs::symlink_metadata(&path).ok()?;
    let mine = meta.uid() == rustix::process::getuid().as_raw() && meta.file_type().is_file();
    if !mine || meta.len() > MAX_REPORT {
        return None;
    }
    serde_json::from_slice(&std::fs::read(&path).ok()?).ok()
}

pub(crate) fn write(dir: &Path, terminal: &str, agent: &str, report: &serde_json::Value) -> io::Result<()> {
    let target = path(dir, terminal, agent).ok_or_else(|| io::Error::other("not a terminal or agent name"))?;
    let folder = ensure(dir)?;
    let part = folder.join(format!(".{terminal}-{agent}.part"));
    let _ = std::fs::remove_file(&part);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&part)?;
    file.write_all(report.to_string().as_bytes())?;
    drop(file);
    std::fs::rename(part, target)
}

pub(crate) fn forget(dir: &Path, terminal: &str) {
    if !valid_name(terminal) || only_mine(&folder(dir), Kind::Folder).is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder(dir)) else { return };
    let prefix = format!("{terminal}-");
    for entry in entries.flatten() {
        if entry.file_name().to_str().is_some_and(|name| name.starts_with(&prefix)) {
            let _ = std::fs::remove_file(entry.path());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_comes_back_and_names_cannot_leave_the_folder() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        let report = serde_json::json!({ "session_id": "abc" });
        write(&dir, "t1", "claude-code", &report).unwrap();
        assert_eq!(read(&dir, "t1", "claude-code"), Some(report));
        assert!(write(&dir, "../t1", "claude-code", &serde_json::json!({})).is_err());
        assert!(!valid_name("a/b") && !valid_name("") && !valid_name(".."));
        forget(&dir, "t1");
        assert_eq!(read(&dir, "t1", "claude-code"), None);
    }

    #[test]
    fn a_hooks_file_with_the_default_mode_is_read_in_the_private_folder() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal");
        let folder = ensure(&dir).unwrap();
        std::fs::write(folder.join("t2-codex.json"), r#"{"session_id":"x-1"}"#).unwrap();
        assert_eq!(read(&dir, "t2", "codex"), Some(serde_json::json!({ "session_id": "x-1" })));
        std::fs::write(folder.join("t3-codex.json"), "x".repeat(MAX_REPORT as usize + 1)).unwrap();
        assert_eq!(read(&dir, "t3", "codex"), None, "too large");
    }
}

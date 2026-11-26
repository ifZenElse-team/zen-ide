use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub const SYNC_EVERY: Duration = Duration::from_secs(1);

pub fn encode<T: Serialize>(record: &T) -> Vec<u8> {
    let Ok(json) = serde_json::to_vec(record) else { return Vec::new() };
    let mut line = Vec::with_capacity(json.len() + 10);
    line.extend_from_slice(format!("{:08x} ", crc32fast::hash(&json)).as_bytes());
    line.extend_from_slice(&json);
    line.push(b'\n');
    line
}

#[derive(Debug)]
pub struct Decoded<T> {
    pub records: Vec<T>,
    pub damaged: usize,
    pub unchecked: usize,
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Decoded<T> {
    let mut decoded = Decoded { records: Vec::new(), damaged: 0, unchecked: 0 };
    let mut rest = bytes;
    while !rest.is_empty() {
        let (line, ended) = match rest.iter().position(|byte| *byte == b'\n') {
            Some(end) => {
                let line = &rest[..end];
                rest = &rest[end + 1..];
                (line, true)
            }
            None => {
                let line = rest;
                rest = &[];
                (line, false)
            }
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if !ended {
            decoded.damaged += 1;
            continue;
        }
        match record(line) {
            Some((record, checked)) => {
                decoded.records.push(record);
                if !checked {
                    decoded.unchecked += 1;
                }
            }
            None => decoded.damaged += 1,
        }
    }
    decoded
}

fn record<T: DeserializeOwned>(line: &[u8]) -> Option<(T, bool)> {
    if line.first() == Some(&b'{') {
        return serde_json::from_slice(line).ok().map(|record| (record, false));
    }
    let (crc, json) = (line.get(..8)?, line.get(9..)?);
    if line.get(8) != Some(&b' ') {
        return None;
    }
    let crc = u32::from_str_radix(std::str::from_utf8(crc).ok()?, 16).ok()?;
    if crc32fast::hash(json) != crc {
        return None;
    }
    serde_json::from_slice(json).ok().map(|record| (record, true))
}

// WHY: a rename survives a power loss only once its folder is synced, and no test can cut the power.
pub fn sync_folder(folder: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(folder)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = folder;
        Ok(())
    }
}

fn private(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600)
    }
    #[cfg(not(unix))]
    {
        options
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(folder)?;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let temporary = folder.join(format!(".{name}.{}.tmp", std::process::id()));
    let written = private(OpenOptions::new().write(true).create(true).truncate(true))
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    sync_folder(folder)
}

pub fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => sync_folder(path.parent().unwrap_or(Path::new("."))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
pub struct Appender {
    path: PathBuf,
    file: Option<File>,
    unsynced: bool,
    synced: Option<Instant>,
}

impl Appender {
    pub fn new(path: PathBuf) -> Self {
        Self { path, file: None, unsynced: false, synced: None }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if self.file.is_none() {
            let folder = self.path.parent().unwrap_or(Path::new("."));
            std::fs::create_dir_all(folder)?;
            let created = !self.path.exists();
            let file = private(OpenOptions::new().create(true).append(true)).open(&self.path)?;
            if created {
                file.sync_all()?;
                sync_folder(folder)?;
            }
            self.file = Some(file);
        }
        let Some(file) = self.file.as_mut() else { return Ok(()) };
        if let Err(error) = file.write_all(bytes) {
            self.file = None;
            return Err(error);
        }
        self.unsynced = true;
        Ok(())
    }

    pub fn sync_due(&self, now: Instant) -> Option<Duration> {
        if !self.unsynced {
            return None;
        }
        Some(self.synced.map_or(Duration::ZERO, |synced| (synced + SYNC_EVERY).saturating_duration_since(now)))
    }

    pub fn sync(&mut self) -> std::io::Result<()> {
        if !self.unsynced {
            return Ok(());
        }
        if let Some(file) = self.file.as_mut() {
            file.sync_all()?;
        }
        self.unsynced = false;
        self.synced = Some(Instant::now());
        Ok(())
    }

    pub fn sync_if_due(&mut self, now: Instant) -> std::io::Result<()> {
        match self.sync_due(now) {
            Some(left) if left.is_zero() => self.sync(),
            _ => Ok(()),
        }
    }

    pub fn close(&mut self) {
        self.file = None;
        self.unsynced = false;
    }
}

#[cfg(test)]
mod tests;

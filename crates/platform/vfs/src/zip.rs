use std::fmt;
use std::io::{Read, Write};

use flate2::Crc;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const END_SIZE: usize = 22;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
const UTF8_NAMES: u16 = 0x0800;
const ENCRYPTED: u16 = 0x0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub archive: u64,
    pub entries: usize,
    pub entry: u64,
    pub total: u64,
}

impl Limits {
    pub const SETTINGS: Limits =
        Limits { archive: 64 * 1024 * 1024, entries: 4096, entry: 8 * 1024 * 1024, total: 64 * 1024 * 1024 };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZipError {
    NotZip,
    Zip64,
    TooLarge(u64),
    TooManyEntries(usize),
    EntryTooLarge { name: String, limit: u64 },
    Unsafe(String),
    Damaged(String),
    Encrypted,
    Method(u16),
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::NotZip => f.write_str("not a ZIP archive"),
            ZipError::Zip64 => f.write_str("a ZIP64 archive"),
            ZipError::TooLarge(limit) => write!(f, "larger than {limit} bytes"),
            ZipError::TooManyEntries(limit) => write!(f, "more than {limit} entries"),
            ZipError::EntryTooLarge { name, limit } => write!(f, "{name} unpacks to more than {limit} bytes"),
            ZipError::Unsafe(name) => write!(f, "the entry name {name:?} is not a safe relative path"),
            ZipError::Damaged(why) => write!(f, "damaged: {why}"),
            ZipError::Encrypted => f.write_str("encrypted"),
            ZipError::Method(method) => write!(f, "packed with method {method}"),
        }
    }
}

impl std::error::Error for ZipError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub folder: bool,
    pub link: bool,
    pub executable: bool,
    flags: u16,
    method: u16,
    crc: u32,
    packed: u64,
    offset: u64,
}

#[derive(Debug)]
pub struct Archive {
    bytes: Vec<u8>,
    entries: Vec<Entry>,
    central: u64,
    limits: Limits,
}

pub fn safe_name(name: &str) -> bool {
    let name = name.strip_suffix('/').unwrap_or(name);
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', '\0'])
        && name.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ZipError> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| ZipError::Damaged("truncated".to_owned()))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ZipError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| ZipError::Damaged("truncated".to_owned()))
}

fn find_end(bytes: &[u8]) -> Result<usize, ZipError> {
    if bytes.len() < END_SIZE {
        return Err(ZipError::NotZip);
    }
    let last = bytes.len() - END_SIZE;
    let first = last.saturating_sub(usize::from(u16::MAX));
    (first..=last).rev().find(|&at| u32_at(bytes, at).is_ok_and(|sig| sig == END)).ok_or(ZipError::NotZip)
}

impl Archive {
    pub fn parse(bytes: Vec<u8>, limits: Limits) -> Result<Archive, ZipError> {
        if bytes.len() as u64 > limits.archive {
            return Err(ZipError::TooLarge(limits.archive));
        }
        let end = find_end(&bytes)?;
        let count = u16_at(&bytes, end + 10)?;
        let size = u32_at(&bytes, end + 12)?;
        let start = u32_at(&bytes, end + 16)?;
        if count == u16::MAX || size == u32::MAX || start == u32::MAX {
            return Err(ZipError::Zip64);
        }
        let count = usize::from(count);
        if count > limits.entries {
            return Err(ZipError::TooManyEntries(limits.entries));
        }
        let (start, size) = (start as usize, size as usize);
        if start.checked_add(size).is_none_or(|stop| stop > end) {
            return Err(ZipError::Damaged("the central directory is outside the archive".to_owned()));
        }
        let mut entries = Vec::with_capacity(count);
        let mut at = start;
        let mut total = 0u64;
        for _ in 0..count {
            if u32_at(&bytes, at)? != CENTRAL {
                return Err(ZipError::Damaged("a central directory record is missing".to_owned()));
            }
            let host = u16_at(&bytes, at + 4)? >> 8;
            let flags = u16_at(&bytes, at + 8)?;
            let method = u16_at(&bytes, at + 10)?;
            let crc = u32_at(&bytes, at + 16)?;
            let packed = u32_at(&bytes, at + 20)?;
            let unpacked = u32_at(&bytes, at + 24)?;
            let name_len = usize::from(u16_at(&bytes, at + 28)?);
            let extra_len = usize::from(u16_at(&bytes, at + 30)?);
            let comment_len = usize::from(u16_at(&bytes, at + 32)?);
            let external = u32_at(&bytes, at + 38)?;
            let offset = u32_at(&bytes, at + 42)?;
            if packed == u32::MAX || unpacked == u32::MAX || offset == u32::MAX {
                return Err(ZipError::Zip64);
            }
            let raw = bytes
                .get(at + 46..at + 46 + name_len)
                .ok_or_else(|| ZipError::Damaged("an entry name is cut off".to_owned()))?;
            let name = String::from_utf8_lossy(raw).into_owned();
            if !safe_name(&name) {
                return Err(ZipError::Unsafe(name));
            }
            let folder = name.ends_with('/');
            let link = host == 3 && (external >> 16) & 0o170_000 == 0o120_000;
            let executable = host == 3 && (external >> 16) & 0o111 != 0;
            let size = u64::from(unpacked);
            if size > limits.entry {
                return Err(ZipError::EntryTooLarge { name, limit: limits.entry });
            }
            total += size;
            if total > limits.total {
                return Err(ZipError::EntryTooLarge { name, limit: limits.total });
            }
            entries.push(Entry {
                name: name.trim_end_matches('/').to_owned(),
                size,
                folder,
                link,
                executable,
                flags,
                method,
                crc,
                packed: u64::from(packed),
                offset: u64::from(offset),
            });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Archive { bytes, entries, central: start as u64, limits })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>, ZipError> {
        if entry.flags & ENCRYPTED != 0 {
            return Err(ZipError::Encrypted);
        }
        let at = usize::try_from(entry.offset).map_err(|_| ZipError::Damaged("offset".to_owned()))?;
        if u32_at(&self.bytes, at)? != LOCAL {
            return Err(ZipError::Damaged(format!("{}: no local header", entry.name)));
        }
        let name_len = usize::from(u16_at(&self.bytes, at + 26)?);
        let extra_len = usize::from(u16_at(&self.bytes, at + 28)?);
        let start = at + 30 + name_len + extra_len;
        let stop = start
            .checked_add(usize::try_from(entry.packed).map_err(|_| ZipError::Damaged("size".to_owned()))?)
            .filter(|&stop| stop as u64 <= self.central)
            .ok_or_else(|| ZipError::Damaged(format!("{}: the data is outside the archive", entry.name)))?;
        let packed = &self.bytes[start..stop];
        let limit = self.limits.entry.min(entry.size);
        let data = match entry.method {
            STORED => packed.to_vec(),
            DEFLATED => {
                let mut data = Vec::new();
                DeflateDecoder::new(packed)
                    .take(limit + 1)
                    .read_to_end(&mut data)
                    .map_err(|error| ZipError::Damaged(format!("{}: {error}", entry.name)))?;
                data
            }
            other => return Err(ZipError::Method(other)),
        };
        if data.len() as u64 != entry.size {
            return Err(ZipError::Damaged(format!("{}: the size does not match", entry.name)));
        }
        let mut crc = Crc::new();
        crc.update(&data);
        if crc.sum() != entry.crc {
            return Err(ZipError::Damaged(format!("{}: the checksum does not match", entry.name)));
        }
        Ok(data)
    }
}

#[derive(Debug, Default)]
pub struct ZipWriter {
    bytes: Vec<u8>,
    central: Vec<u8>,
    count: u16,
    time: (u16, u16),
}

impl ZipWriter {
    pub fn new(seconds: u64) -> ZipWriter {
        ZipWriter { time: dos_time(seconds), ..ZipWriter::default() }
    }

    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), ZipError> {
        if !safe_name(name) || name.ends_with('/') {
            return Err(ZipError::Unsafe(name.to_owned()));
        }
        if self.count == u16::MAX - 1 || data.len() as u64 >= u64::from(u32::MAX) {
            return Err(ZipError::Zip64);
        }
        let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).map_err(|error| ZipError::Damaged(error.to_string()))?;
        let packed = encoder.finish().map_err(|error| ZipError::Damaged(error.to_string()))?;
        let mut crc = Crc::new();
        crc.update(data);
        let offset = u32::try_from(self.bytes.len()).map_err(|_| ZipError::Zip64)?;
        let header = |out: &mut Vec<u8>, central: bool| {
            out.extend_from_slice(&if central { CENTRAL } else { LOCAL }.to_le_bytes());
            if central {
                out.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes());
            }
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&UTF8_NAMES.to_le_bytes());
            out.extend_from_slice(&DEFLATED.to_le_bytes());
            out.extend_from_slice(&self.time.1.to_le_bytes());
            out.extend_from_slice(&self.time.0.to_le_bytes());
            out.extend_from_slice(&crc.sum().to_le_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            if central {
                out.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&(0o100_644u32 << 16).to_le_bytes());
                out.extend_from_slice(&offset.to_le_bytes());
            }
            out.extend_from_slice(name.as_bytes());
        };
        let mut local = Vec::new();
        header(&mut local, false);
        let mut central = Vec::new();
        header(&mut central, true);
        self.bytes.extend_from_slice(&local);
        self.bytes.extend_from_slice(&packed);
        self.central.extend_from_slice(&central);
        self.count += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Vec<u8> {
        let start = self.bytes.len() as u32;
        let size = self.central.len() as u32;
        self.bytes.append(&mut self.central);
        self.bytes.extend_from_slice(&END.to_le_bytes());
        self.bytes.extend_from_slice(&[0; 4]);
        self.bytes.extend_from_slice(&self.count.to_le_bytes());
        self.bytes.extend_from_slice(&self.count.to_le_bytes());
        self.bytes.extend_from_slice(&size.to_le_bytes());
        self.bytes.extend_from_slice(&start.to_le_bytes());
        self.bytes.extend_from_slice(&0u16.to_le_bytes());
        self.bytes
    }
}

pub fn utc(seconds: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year as u64, month as u64, day as u64, rest / 3600, rest % 3600 / 60, rest % 60)
}

fn dos_time(seconds: u64) -> (u16, u16) {
    let (year, month, day, hour, minute, second) = utc(seconds);
    let year = year.clamp(1980, 2107);
    let date = ((year - 1980) << 9) as u16 | (month << 5) as u16 | day as u16;
    let time = (hour << 11) as u16 | (minute << 5) as u16 | (second / 2) as u16;
    (date, time)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(1_792_000_000);
        for (name, data) in files {
            writer.add(name, data).unwrap();
        }
        writer.finish()
    }

    fn raw_entry(name: &str, data: &[u8]) -> Vec<u8> {
        let mut crc = Crc::new();
        crc.update(data);
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (sig, central_record) in [(LOCAL, false), (CENTRAL, true)] {
            let target = if central_record { &mut central } else { &mut out };
            target.extend_from_slice(&sig.to_le_bytes());
            if central_record {
                target.extend_from_slice(&20u16.to_le_bytes());
            }
            target.extend_from_slice(&20u16.to_le_bytes());
            target.extend_from_slice(&0u16.to_le_bytes());
            target.extend_from_slice(&STORED.to_le_bytes());
            target.extend_from_slice(&[0; 4]);
            target.extend_from_slice(&crc.sum().to_le_bytes());
            target.extend_from_slice(&(data.len() as u32).to_le_bytes());
            target.extend_from_slice(&(data.len() as u32).to_le_bytes());
            target.extend_from_slice(&(name.len() as u16).to_le_bytes());
            target.extend_from_slice(&0u16.to_le_bytes());
            if central_record {
                target.extend_from_slice(&[0; 6]);
                target.extend_from_slice(&[0; 4]);
                target.extend_from_slice(&0u32.to_le_bytes());
            }
            target.extend_from_slice(name.as_bytes());
            if !central_record {
                target.extend_from_slice(data);
            }
        }
        let start = out.len() as u32;
        let size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&END.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn what_is_written_reads_back() {
        let big = "x".repeat(100_000);
        let bytes = archive(&[("settings.json", b"{}"), ("keymaps/a b.json", big.as_bytes())]);
        let read = Archive::parse(bytes, Limits::SETTINGS).unwrap();
        let names: Vec<&str> = read.entries().iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["settings.json", "keymaps/a b.json"]);
        assert_eq!(read.read(&read.entries()[0]).unwrap(), b"{}");
        assert_eq!(read.read(&read.entries()[1]).unwrap(), big.as_bytes());
    }

    #[test]
    fn stored_entries_read_and_damage_is_found() {
        let bytes = raw_entry("colors/Dusk.icls", b"<scheme/>");
        let read = Archive::parse(bytes.clone(), Limits::SETTINGS).unwrap();
        assert_eq!(read.read(&read.entries()[0]).unwrap(), b"<scheme/>");
        let mut broken = bytes;
        let at = broken.windows(9).position(|window| window == b"<scheme/>").unwrap();
        broken[at] = b'(';
        let read = Archive::parse(broken, Limits::SETTINGS).unwrap();
        assert!(matches!(read.read(&read.entries()[0]), Err(ZipError::Damaged(_))));
    }

    #[test]
    fn unsafe_names_refuse_the_archive() {
        for name in ["../evil.json", "/etc/passwd", "a/../../b", "C:/x", "a\\..\\b", "./a", "a//b"] {
            assert!(!safe_name(name), "{name}");
            assert_eq!(
                Archive::parse(raw_entry(name, b"x"), Limits::SETTINGS).unwrap_err(),
                ZipError::Unsafe(name.to_owned())
            );
        }
        assert!(safe_name("keymaps/My keymap.xml"));
        assert!(safe_name("options/"));
        assert!(ZipWriter::new(0).add("../x", b"").is_err());
    }

    #[test]
    fn limits_refuse_large_and_crowded_archives() {
        let small = Limits { archive: 1024 * 1024, entries: 2, entry: 1000, total: 1500 };
        let bytes = archive(&[("a.json", &[b' '; 1001])]);
        assert!(matches!(Archive::parse(bytes, small), Err(ZipError::EntryTooLarge { .. })));
        let bytes = archive(&[("a.json", &[b' '; 800]), ("b.json", &[b' '; 800])]);
        assert!(matches!(Archive::parse(bytes, small), Err(ZipError::EntryTooLarge { limit: 1500, .. })));
        let bytes = archive(&[("a", b""), ("b", b""), ("c", b"")]);
        assert_eq!(Archive::parse(bytes, small).unwrap_err(), ZipError::TooManyEntries(2));
        let bytes = archive(&[("a", &vec![0; 4096])]);
        assert_eq!(Archive::parse(bytes, Limits { archive: 100, ..small }).unwrap_err(), ZipError::TooLarge(100));
        assert_eq!(Archive::parse(b"PK not really".to_vec(), small).unwrap_err(), ZipError::NotZip);
    }

    #[test]
    fn a_bomb_stops_at_its_declared_size() {
        let mut bytes = archive(&[("a.json", &vec![0; 2_000_000])]);
        let central = bytes.windows(4).rposition(|window| window == CENTRAL.to_le_bytes()).unwrap();
        bytes[central + 24..central + 28].copy_from_slice(&10u32.to_le_bytes());
        let read = Archive::parse(bytes, Limits::SETTINGS).unwrap();
        assert!(matches!(read.read(&read.entries()[0]), Err(ZipError::Damaged(_))));
    }
}

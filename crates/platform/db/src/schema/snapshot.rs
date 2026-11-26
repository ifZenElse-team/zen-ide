use std::io::Write;
use std::path::Path;

use super::Snapshot;

pub const SNAPSHOT_VERSION: u32 = 5;

const MAGIC: &[u8; 4] = b"ZDBS";

const HEADER: usize = 4 + 4 + 8 + 4;

pub fn read_snapshot(file: &Path) -> Option<Snapshot> {
    let bytes = std::fs::read(file).ok()?;
    if bytes.len() < HEADER || &bytes[..4] != MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let length = usize::try_from(u64::from_le_bytes(bytes[8..16].try_into().ok()?)).ok()?;
    let crc = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    let body = bytes.get(HEADER..)?;
    if version != SNAPSHOT_VERSION || body.len() != length || crc32fast::hash(body) != crc {
        return None;
    }
    postcard::from_bytes(body).ok()
}

pub fn write_snapshot(file: &Path, snapshot: &Snapshot) -> std::io::Result<()> {
    let body = postcard::to_stdvec(snapshot).map_err(std::io::Error::other)?;
    let mut bytes = Vec::with_capacity(HEADER + body.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
    bytes.extend_from_slice(&body);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = file.with_extension("snapshot.tmp");
    {
        let mut out = std::fs::File::create(&temporary)?;
        out.write_all(&bytes)?;
        out.sync_all()?;
    }
    std::fs::rename(&temporary, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Catalog, Level, Object, ObjectKind, Schema};

    fn sample() -> Snapshot {
        let mut table = Object::new(ObjectKind::Table, "orders");
        table.rows = Some(12);
        table.comment = Some("Orders".to_owned());
        Snapshot {
            catalogs: vec![Catalog {
                name: "shop".to_owned(),
                current: true,
                schemas_read: true,
                schemas: vec![Schema {
                    name: "public".to_owned(),
                    level: Level::Details,
                    fingerprint: Some("1:2".to_owned()),
                    objects: vec![table],
                    ..Schema::default()
                }],
                ..Catalog::default()
            }],
            ..Snapshot::new("pg", "postgresql")
        }
    }

    #[test]
    fn a_snapshot_comes_back_as_written_and_a_damaged_one_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("caches/pg.snapshot");
        write_snapshot(&file, &sample()).unwrap();
        assert_eq!(read_snapshot(&file), Some(sample()));
        assert!(!file.with_extension("snapshot.tmp").exists());
        let mut bytes = std::fs::read(&file).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(read_snapshot(&file), None);
        bytes[last] ^= 0xff;
        bytes[4] = 9;
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(read_snapshot(&file), None, "another version is read again from the server");
        assert_eq!(read_snapshot(&dir.path().join("missing")), None);
    }
}

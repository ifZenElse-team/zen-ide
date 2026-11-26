use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Note {
    id: u32,
    text: String,
}

fn note(id: u32, text: &str) -> Note {
    Note { id, text: text.to_owned() }
}

#[test]
fn records_carry_a_crc_and_damaged_or_torn_lines_are_dropped() {
    let mut bytes = encode(&note(1, "first"));
    assert!(bytes.starts_with(format!("{:08x} {{", crc32fast::hash(br#"{"id":1,"text":"first"}"#)).as_bytes()));
    let mut second = encode(&note(2, "second"));
    let at = second.iter().position(|byte| *byte == b's').unwrap();
    second[at] = b'S';
    bytes.extend_from_slice(&second);
    bytes.extend_from_slice(br#"{"id":3,"text":"before checks"}"#);
    bytes.push(b'\n');
    bytes.extend_from_slice(&encode(&note(4, "fourth")));
    let torn = encode(&note(5, "torn by a crash"));
    bytes.extend_from_slice(&torn[..torn.len() - 6]);

    let decoded = decode::<Note>(&bytes);
    assert_eq!(decoded.records, [note(1, "first"), note(3, "before checks"), note(4, "fourth")]);
    assert_eq!((decoded.damaged, decoded.unchecked), (2, 1), "a changed record and a torn one; one without a check");
}

#[test]
fn an_atomic_write_keeps_the_old_copy_until_the_new_one_is_whole() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("deep/state.json");
    write_atomic(&file, b"old").unwrap();
    write_atomic(&file, b"new").unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"new");
    let names: Vec<String> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["state.json"], "no temporary file is left");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let blocked = dir.path().join("blocked");
    std::fs::create_dir_all(blocked.join("state.json")).unwrap();
    std::fs::write(blocked.join("keep.txt"), "kept").unwrap();
    assert!(write_atomic(&blocked.join("state.json"), b"x").is_err(), "a folder in the way is not replaced");
    assert!(blocked.join("state.json").is_dir());
    assert_eq!(std::fs::read_to_string(blocked.join("keep.txt")).unwrap(), "kept");
    assert_eq!(std::fs::read_dir(&blocked).unwrap().count(), 2, "the temporary file is removed");

    remove(&file).unwrap();
    assert!(!file.exists());
    remove(&file).unwrap();
}

#[test]
fn appended_records_are_synced_at_most_once_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Appender::new(dir.path().join("journal.jsonl"));
    let start = Instant::now();
    assert_eq!(journal.sync_due(start), None, "nothing to sync");
    journal.append(&encode(&note(1, "a"))).unwrap();
    assert_eq!(journal.sync_due(start), Some(Duration::ZERO), "the first record syncs at once");
    journal.sync_if_due(start).unwrap();
    assert_eq!(journal.sync_due(start), None);
    journal.append(&encode(&note(2, "b"))).unwrap();
    let left = journal.sync_due(Instant::now()).unwrap();
    assert!(left > Duration::from_millis(500) && left <= SYNC_EVERY, "{left:?}");
    journal.sync_if_due(Instant::now()).unwrap();
    assert!(journal.sync_due(Instant::now()).is_some(), "not synced again within the second");
    journal.sync_if_due(Instant::now() + SYNC_EVERY).unwrap();
    assert_eq!(journal.sync_due(Instant::now()), None);
    let decoded = decode::<Note>(&std::fs::read(journal.path()).unwrap());
    assert_eq!(decoded.records, [note(1, "a"), note(2, "b")]);
}

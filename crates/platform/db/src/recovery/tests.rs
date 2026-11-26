use std::time::Duration;

use super::*;
use crate::edit::{EditColumn, KeyKind, RowKey, Staged, Updated};
use crate::value::Value;

const WAIT: Duration = Duration::from_secs(10);

fn console(path: &str, text: &str) -> Item {
    Item::Console(ConsoleText { path: PathBuf::from(path), text: text.to_owned(), at: 1 })
}

fn edits() -> Item {
    let column = |index: usize, name: &str, type_name: &str| EditColumn {
        index,
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        editable: true,
        nullable: index > 0,
        default: None,
        sourced: true,
    };
    let editing = Editing {
        editable: true,
        reason: None,
        table: Some("public.customers".to_owned()),
        key: Some(RowKey { kind: KeyKind::PrimaryKey, columns: vec![0], names: vec!["id".to_owned()] }),
        columns: vec![column(0, "id", "bigint"), column(1, "name", "text")],
        dialect: SqlDialect::Postgres,
        documents: false,
        target: None,
    };
    let changes = Changes {
        updated: vec![Updated {
            row: 0,
            original: vec![Value::Integer(1), Value::Text("Ann".to_owned())],
            cells: vec![(1, Staged::Value(Value::Text("Anna".to_owned())))],
        }],
        ..Changes::default()
    };
    Item::Edits(Box::new(StagedEdits {
        connection: "shop".to_owned(),
        connection_name: "Shop".to_owned(),
        dialect: dialect_name(SqlDialect::Postgres).to_owned(),
        editing,
        changes,
        statement: "SELECT * FROM public.customers".to_owned(),
        place: Some("customers".to_owned()),
        at: 2,
    }))
}

#[test]
fn what_is_kept_comes_back_after_a_restart_and_goes_when_dealt_with() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = Recovery::open(dir.path());
    assert!(recovery.flush(WAIT));
    assert!(recovery.loaded());
    assert!(!recovery.file().exists(), "nothing kept, no file");
    recovery.put("console:a", console("/consoles/a.sql", "select 1"));
    recovery.put("console:a", console("/consoles/a.sql", "select 12"));
    recovery.put("edits:1", edits());
    recovery.put("console:b", console("/consoles/b.sql", "select 2"));
    recovery.remove("console:b");
    assert!(recovery.flush(WAIT));
    assert!(recovery.recovered().is_empty(), "this run's items are not offered back to it");
    drop(recovery);

    let restarted = Recovery::open(dir.path());
    assert!(restarted.flush(WAIT));
    let recovered = restarted.recovered();
    let keys: Vec<&str> = recovered.iter().map(|(_, key, _)| key.as_str()).collect();
    assert_eq!(keys, ["console:a", "edits:1"]);
    assert_eq!(*recovered[0].2, console("/consoles/a.sql", "select 12"), "the last text kept");
    let Item::Edits(staged) = &*recovered[1].2 else { panic!("edits") };
    let planned = crate::edit::plan(&staged.editing(), &staged.changes);
    assert_eq!(
        crate::edit::preview(&planned),
        "UPDATE public.customers SET \"name\" = 'Anna' WHERE \"id\" = 1 AND \"name\" IS NOT DISTINCT FROM 'Ann' RETURNING \"id\", \"name\";"
    );
    let text = std::fs::read_to_string(restarted.file()).unwrap();
    assert_eq!(text.lines().count(), 2, "compacted when read: {text}");

    assert!(restarted.announce());
    assert!(!restarted.announce(), "announced once");
    restarted.adopt(recovered[0].0);
    assert_eq!(restarted.recovered().len(), 1, "an adopted item is this run's own");
    assert!(restarted.get("console:a").is_some());
    assert!(restarted.discard(recovered[1].0));
    assert!(!restarted.discard(recovered[1].0));
    restarted.remove("console:a");
    assert!(restarted.flush(WAIT));
    assert!(!restarted.file().exists(), "the file goes when nothing is kept");
    assert_eq!(restarted.error(), None);
}

#[test]
fn a_damaged_record_is_dropped_and_older_text_is_compacted_away() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = Recovery::open(dir.path());
    let long = "x".repeat(64 * 1024);
    for round in 0..40 {
        recovery.put("console:a", console("/consoles/a.sql", &format!("{round}{long}")));
        assert!(recovery.flush(WAIT));
    }
    recovery.put("console:b", console("/consoles/b.sql", "kept"));
    assert!(recovery.flush(WAIT));
    let size = std::fs::metadata(recovery.file()).unwrap().len();
    assert!(size < 2 * 1024 * 1024 + 200 * 1024, "dead records are compacted: {size}");
    let file = recovery.file();
    drop(recovery);

    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, text.replace("kept", "kepT")).unwrap();
    let restarted = Recovery::open(dir.path());
    assert!(restarted.flush(WAIT));
    let keys: Vec<String> = restarted.recovered().into_iter().map(|(_, key, _)| key).collect();
    assert_eq!(keys, ["console:a"], "a record whose check fails is never read");
    let Item::Console(kept) = &*restarted.get("console:a").unwrap() else { panic!("console") };
    assert!(kept.text.starts_with("39"), "the newest text");
}

#[test]
fn a_journal_another_running_ide_holds_is_left_alone_and_taken_over_once_it_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let running = Recovery::open(dir.path());
    running.put("console:a", console("/consoles/a.sql", "select 1"));
    assert!(running.flush(WAIT));

    let headless = Recovery::open(dir.path());
    assert!(headless.flush(WAIT));
    assert!(headless.recovered().is_empty(), "the running IDE's items are not offered");
    assert!(!headless.announce());
    assert_ne!(headless.file(), running.file());
    headless.put("console:b", console("/consoles/b.sql", "select 2"));
    headless.remove("console:b");
    assert!(headless.flush(WAIT));
    running.put("console:a", console("/consoles/a.sql", "select 12"));
    assert!(running.flush(WAIT));
    assert!(running.file().exists(), "still written where it was");

    let file = running.file();
    drop(running);
    drop(headless);
    assert!(file.exists(), "kept items stay for the next run");
    let next = Recovery::open(dir.path());
    assert!(next.flush(WAIT));
    let recovered = next.recovered();
    assert_eq!(recovered.len(), 1);
    assert_eq!(*recovered[0].2, console("/consoles/a.sql", "select 12"));
    assert!(!file.exists(), "taken over: the old journal goes");
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "this run's journal and lock only: {names:?}");
}

#[test]
fn the_journal_of_a_build_before_per_process_journals_is_read_once() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join(FILE);
    let record = journal::encode(&Written { key: "console:a", item: Some(&console("/consoles/a.sql", "old")) });
    std::fs::write(&old, record).unwrap();
    let recovery = Recovery::open(dir.path());
    assert!(recovery.flush(WAIT));
    assert_eq!(recovery.recovered().len(), 1);
    assert!(!old.exists(), "its items are this run's journal's now");
}

#[test]
fn a_journal_that_closes_has_written_all_it_was_given() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = Recovery::open(dir.path());
    for round in 0..200 {
        recovery.put("console:a", console("/consoles/a.sql", &format!("round {round}.")));
    }
    let file = recovery.file();
    drop(recovery);
    assert!(std::fs::read_to_string(file).unwrap().contains("round 199."));
}

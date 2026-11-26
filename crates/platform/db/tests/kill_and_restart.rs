use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use zen_db::dialect::SqlDialect;
use zen_db::driver::ContextKind;
use zen_db::edit::{Changes, Deleted, EditColumn, Editing, Inserted, KeyKind, RowKey, Staged, Updated};
use zen_db::history::{HistorySink, Journal, Provenance, Query, Ran, Source};
use zen_db::recovery::{ConsoleText, Item, OpenTransaction, Recovery, StagedEdits, dialect_name};
use zen_db::value::Value;

const CHILD: &str = "ZEN_DB_KILL_CHILD";
const KEPT: &str = "zen-db-child-kept-everything";
const WAIT: Duration = Duration::from_secs(10);

fn editing() -> Editing {
    let column = |index: usize, name: &str, type_name: &str| EditColumn {
        index,
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        editable: true,
        nullable: index > 0,
        default: None,
        sourced: true,
    };
    Editing {
        editable: true,
        reason: None,
        table: Some("public.customers".to_owned()),
        key: Some(RowKey { kind: KeyKind::PrimaryKey, columns: vec![0], names: vec!["id".to_owned()] }),
        columns: vec![column(0, "id", "bigint"), column(1, "name", "text")],
        dialect: SqlDialect::Postgres,
        documents: false,
        target: None,
    }
}

fn changes() -> Changes {
    Changes {
        deleted: vec![Deleted { row: 1, original: vec![Value::Integer(2), Value::Text("Bob".to_owned())] }],
        inserted: vec![Inserted { row: 3, cells: vec![(1, Staged::Value(Value::Text("Eve".to_owned())))] }],
        updated: vec![Updated {
            row: 0,
            original: vec![Value::Integer(1), Value::Text("Ann".to_owned())],
            cells: vec![(1, Staged::Value(Value::Text("Anna".to_owned())))],
        }],
    }
}

fn keep(folder: &Path) {
    let recovery = Recovery::open(folder);
    let journal = Journal::open(folder);
    recovery.put(
        "console:/data/consoles/p/local/console.sql",
        Item::Console(ConsoleText {
            path: PathBuf::from("/data/consoles/p/local/console.sql"),
            text: "select * from customers where id = 1;\n-- typed just now".to_owned(),
            at: 1,
        }),
    );
    recovery.put(
        "edits:7",
        Item::Edits(Box::new(StagedEdits {
            connection: "shop".to_owned(),
            connection_name: "Shop".to_owned(),
            dialect: dialect_name(SqlDialect::Postgres).to_owned(),
            editing: editing(),
            changes: changes(),
            statement: "SELECT * FROM public.customers".to_owned(),
            place: Some("customers".to_owned()),
            at: 2,
        })),
    );
    recovery.put(
        "transaction:shop:console",
        Item::Transaction(OpenTransaction {
            connection: "shop".to_owned(),
            connection_name: "Shop".to_owned(),
            context: ContextKind::Console("console.sql".to_owned()),
            statements: 2,
            modifying: 1,
            log: vec!["UPDATE orders SET status = 'paid' WHERE id = 7".to_owned()],
            at: 3,
        }),
    );
    journal.record(Ran {
        connection: "shop".to_owned(),
        context: ContextKind::Console("console.sql".to_owned()),
        schema: None,
        text: "UPDATE orders SET status = 'paid' WHERE id = 7".to_owned(),
        started: SystemTime::now(),
        duration: Duration::from_millis(3),
        rows: None,
        affected: Some(1),
        error: None,
        provenance: Provenance::of(Source::Statement),
        params: Vec::new(),
    });
    assert!(recovery.flush(WAIT) && journal.flush(WAIT));
    let mut out = std::io::stdout();
    writeln!(out, "\n{KEPT}").unwrap();
    out.flush().unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn a_killed_process_loses_nothing_it_kept() {
    if let Some(folder) = std::env::var_os(CHILD) {
        keep(Path::new(&folder));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("databases");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["a_killed_process_loses_nothing_it_kept", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, &folder)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (said, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let kept = BufReader::new(stdout).lines().map_while(Result::ok).any(|line| line.contains(KEPT));
        let _ = said.send(kept);
    });
    let ready = heard.recv_timeout(Duration::from_secs(120)).unwrap_or(false);
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(ready, "the child kept its items");
    assert!(!status.success(), "killed, not ended: {status}");

    let journals: Vec<PathBuf> = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with("recovery-") && name.ends_with(".jsonl")
        })
        .collect();
    assert_eq!(journals.len(), 1, "the child's journal: {journals:?}");
    let child_journal = &journals[0];
    let mut file = std::fs::OpenOptions::new().append(true).open(child_journal).unwrap();
    file.write_all(b"0badc0de {\"key\":\"console:torn\",\"item\":{\"kind\":\"cons").unwrap();
    drop(file);

    let recovery = Recovery::open(&folder);
    let journal = Journal::open(&folder);
    assert!(recovery.flush(WAIT) && journal.flush(WAIT));
    let recovered = recovery.recovered();
    let keys: Vec<&str> = recovered.iter().map(|(_, key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        ["console:/data/consoles/p/local/console.sql", "edits:7", "transaction:shop:console"],
        "a torn record is dropped, the rest is whole"
    );
    let Item::Console(console) = &*recovered[0].2 else { panic!("a console") };
    assert!(console.text.ends_with("-- typed just now"));
    let Item::Edits(staged) = &*recovered[1].2 else { panic!("edits") };
    assert_eq!(staged.changes, changes(), "every change with the values its row was read with");
    let sql = zen_db::edit::preview(&zen_db::edit::plan(&staged.editing(), &staged.changes));
    assert!(sql.starts_with("DELETE FROM public.customers WHERE \"id\" = 2;"), "{sql}");
    assert!(sql.contains("WHERE \"id\" = 1 AND \"name\" IS NOT DISTINCT FROM 'Ann'"), "{sql}");
    let Item::Transaction(transaction) = &*recovered[2].2 else { panic!("a transaction") };
    assert_eq!((transaction.statements, transaction.modifying), (2, 1));
    let history = journal.search(&Query { limit: 10, ..Query::default() });
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].text, "UPDATE orders SET status = 'paid' WHERE id = 7");
    let kept = std::fs::read_to_string(recovery.file()).unwrap();
    assert!(!kept.contains("console:torn"), "the torn record is gone from the file");
    assert_ne!(&recovery.file(), child_journal);
    assert!(!child_journal.exists(), "the child's journal is taken over and removed");
}

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::dialect::SqlDialect;
use crate::export::{Format, JsonStyle, Settings};
use crate::memory::{ID, MemoryDriver};
use crate::registry::AuthKind;
use crate::types::DEVELOPMENT;

const WAIT: Duration = Duration::from_secs(10);

fn engine() -> Engine {
    let driver = Arc::new(MemoryDriver::new(false));
    let engine = Engine::new(vec![driver as Arc<dyn Driver>], "Zen Test");
    let source = DataSource {
        id: "kv".to_owned(),
        name: "kv".to_owned(),
        driver: ID.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        auth: AuthKind::Password,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let request =
        ConnectRequest { source, kind, password: Some(Secret::new("pw".to_owned())), stored: None, ssh_secret: None };
    let op = engine.connect(request).unwrap();
    assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    engine
}

fn item(statement: &str, output: Output) -> ExportItem {
    ExportItem {
        connection: "kv".to_owned(),
        context: None,
        statement: statement.to_owned(),
        name: "rows".to_owned(),
        table: "rows".to_owned(),
        dialect: SqlDialect::Postgres,
        output,
    }
}

fn finished(engine: &Engine, id: u64) -> ExportState {
    engine.wait(id, WAIT).unwrap();
    let state = engine.export_state(id).unwrap();
    assert_ne!(state.status, OperationStatus::Running, "{state:?}");
    state
}

#[test]
fn rows_stream_into_a_file_and_into_text_with_counters_and_the_session_closes() {
    let engine = engine();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rows.csv");
    let id = engine
        .export(
            vec![
                item("ROWS 5000", Output::File { path: path.clone(), overwrite: false }),
                item("ROWS 2", Output::Text),
            ],
            Settings::default(),
        )
        .unwrap();
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.items[0].rows, state.items[1].rows, state.rows), (5000, 2, 5002));
    assert!(!state.items[0].server_copy);
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(state.items[0].bytes, written.len() as u64);
    assert_eq!(written.lines().count(), 5001);
    assert!(written.starts_with("id,name,even,nothing,doc\n0,row 0,true,,\"{n: 0, tag: x}\"\n"), "{written}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no part file is left");
    assert!(state.text);
    let text = engine.take_export_text(id).unwrap();
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(engine.take_export_text(id).is_none(), "taken once");
    assert!(
        engine
            .status("kv")
            .contexts
            .iter()
            .all(|context| !matches!(&context.context, ContextKind::Console(name) if name.starts_with("export"))),
        "the export's own session closes: {:?}",
        engine.status("kv").contexts
    );
    let json = serde_json::to_string(&engine.export_state(id).unwrap()).unwrap();
    assert!(!json.contains("row 0"), "answers never hold rows: {json}");
}

#[test]
fn an_existing_file_is_kept_unless_overwrite_and_json_lines_are_written() {
    let engine = engine();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rows.jsonl");
    std::fs::write(&path, "keep me").unwrap();
    let settings = Settings { format: Format::Json, json: JsonStyle::Lines, ..Settings::default() };
    let id = engine
        .export(vec![item("ROWS 3", Output::File { path: path.clone(), overwrite: false })], settings.clone())
        .unwrap();
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Failed);
    assert_eq!(state.items[0].error.as_ref().and_then(|error| error.code.as_deref()), Some("exists"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
    let id =
        engine.export(vec![item("ROWS 3", Output::File { path: path.clone(), overwrite: true })], settings).unwrap();
    assert_eq!(finished(&engine, id).status, OperationStatus::Done);
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written.lines().count(), 3);
    assert!(
        written.starts_with(
            "{\"id\":0,\"name\":\"row 0\",\"even\":true,\"nothing\":null,\"doc\":{\"n\":0,\"tag\":\"x\"}}\n"
        ),
        "{written}"
    );
}

#[test]
fn a_statement_without_rows_fails_and_cancel_leaves_nothing_behind() {
    let engine = engine();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("none.csv");
    let id = engine
        .export(vec![item("SET a 1", Output::File { path: path.clone(), overwrite: false })], Settings::default())
        .unwrap();
    let state = finished(&engine, id);
    assert_eq!(
        state.items[0].error.as_ref().map(|error| error.detail.as_str()),
        Some("READONLY"),
        "an export's own session only reads"
    );
    let main = ExportItem {
        context: Some(ContextKind::Main),
        ..item("SET a 1", Output::File { path: path.clone(), overwrite: false })
    };
    let id = engine.export(vec![main], Settings::default()).unwrap();
    let state = finished(&engine, id);
    assert_eq!(state.items[0].error.as_ref().and_then(|error| error.code.as_deref()), Some("noRows"), "{state:?}");
    assert!(!path.exists());
    let id = engine
        .export(vec![item("SLEEP 3000", Output::File { path: path.clone(), overwrite: false })], Settings::default())
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(engine.cancel(id));
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Cancelled, "{state:?}");
    assert_eq!(state.items[0].status, ItemStatus::Cancelled);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing is left behind");
    assert!(engine.export(Vec::new(), Settings::default()).is_err());
    let bad = Settings { delimiter: "\"".to_owned(), ..Settings::default() };
    assert_eq!(
        engine.export(vec![item("ROWS 1", Output::Text)], bad).unwrap_err().code.as_deref(),
        Some("delimiterQuote")
    );
}

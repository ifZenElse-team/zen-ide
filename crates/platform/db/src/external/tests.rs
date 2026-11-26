use super::protocol::{self, RpcError};
use super::*;

fn manifest() -> serde_json::Value {
    serde_json::json!([{
        "id": "duckdb",
        "name": "DuckDB",
        "process": "shared",
        "capabilities": { "models": ["relational"], "language": "sql:duckdb" },
        "readVerbs": ["select"],
        "platforms": {
            "macos-aarch64": { "url": "https://example.invalid/zen-duckdb-macos-aarch64?x=1", "sha256": "ab".repeat(32) },
            "linux-x86_64": { "url": "drivers/zen-duckdb", "sha256": "cd".repeat(32), "file": "zen-duckdb" }
        }
    }])
}

#[test]
fn a_manifest_declares_drivers_with_a_program_per_platform() {
    let declared = DriverDeclaration::parse_list(&manifest()).unwrap();
    assert_eq!(declared[0].protocol, protocol::VERSION, "protocol 1 when absent");
    assert_eq!(declared[0].process, ProcessModel::Shared);
    assert_eq!(declared[0].platforms["macos-aarch64"].file_name().as_deref(), Some("zen-duckdb-macos-aarch64"));
    assert_eq!(declared[0].platforms["linux-x86_64"].file_name().as_deref(), Some("zen-duckdb"));
}

#[test]
fn the_sql_server_plugin_declares_a_driver_of_its_dialect() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/sqlserver/plugin.json")).unwrap();
    let declared = DriverDeclaration::parse_list(&manifest["databaseDrivers"]).unwrap();
    assert_eq!(declared[0].id, "sqlserver");
    assert_eq!(declared[0].default_port, Some(1433));
    let language = &declared[0].capabilities.language;
    assert_eq!(crate::dialect::SqlDialect::of_language(language), Some(crate::dialect::SqlDialect::SqlServer));
    assert_eq!(crate::sql::Dialect::of_language(language), crate::sql::Dialect::SqlServer);
}

#[test]
fn the_mongodb_plugin_declares_a_document_driver_whose_shell_reads_count_as_reads() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/mongodb/plugin.json")).unwrap();
    let declaration = DriverDeclaration::parse_list(&manifest["databaseDrivers"]).unwrap().remove(0);
    assert_eq!((declaration.id.as_str(), declaration.default_port), ("mongodb", Some(27017)));
    assert!(declaration.guards_read_only);
    let driver =
        ExternalDriver::new(ExternalSpec { declaration, program: PathBuf::from("/none"), sha256: None, plugin: None });
    assert_eq!(driver.classify("db.orders.find({ total: { $gt: 10 } }).sort({ total: -1 })"), StatementKind::Read);
    assert_eq!(driver.classify("show collections"), StatementKind::Read);
    assert_eq!(driver.classify("db.orders.insertOne({ total: 1 })"), StatementKind::Write);
    assert_eq!(driver.classify("db.orders.aggregate([{ $out: \"copy\" }])"), StatementKind::Write);
}

#[test]
fn the_duckdb_plugin_declares_a_sql_driver_whose_tables_open_as_a_select() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/duckdb/plugin.json")).unwrap();
    let declaration = DriverDeclaration::parse_list(&manifest["databaseDrivers"]).unwrap().remove(0);
    assert_eq!((declaration.id.as_str(), declaration.default_port), ("duckdb", None));
    let driver =
        ExternalDriver::new(ExternalSpec { declaration, program: PathBuf::from("/none"), sha256: None, plugin: None });
    assert_eq!(driver.classify("SELECT * FROM 'data.parquet'"), StatementKind::Read);
    assert_eq!(driver.classify("INSERT INTO t VALUES (1)"), StatementKind::Write);
    let path = ["shop".to_owned(), "main".to_owned(), "my \"t\"".to_owned()];
    assert_eq!(
        driver.open_text(&path, &crate::model::NodeKind::Table).as_deref(),
        Some("SELECT * FROM \"shop\".\"main\".\"my \"\"t\"\"\"")
    );
    let mongo: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/mongodb/plugin.json")).unwrap();
    let declaration = DriverDeclaration::parse_list(&mongo["databaseDrivers"]).unwrap().remove(0);
    let driver =
        ExternalDriver::new(ExternalSpec { declaration, program: PathBuf::from("/none"), sha256: None, plugin: None });
    let collection = ["shop".to_owned(), "orders".to_owned()];
    assert_eq!(
        driver.open_text(&collection, &crate::model::NodeKind::Collection).as_deref(),
        Some("db.getSiblingDB(\"shop\").getCollection(\"orders\").find({})")
    );
}

#[test]
fn a_manifest_with_a_bad_id_protocol_model_file_or_checksum_is_refused() {
    let broken = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut json = manifest();
        change(&mut json[0]);
        DriverDeclaration::parse_list(&json).unwrap_err()
    };
    assert!(broken(&|driver| driver["id"] = "Duck DB".into()).contains("not a valid driver id"));
    assert!(broken(&|driver| driver["protocol"] = 2.into()).contains("protocol 2"));
    assert!(broken(&|driver| driver["capabilities"]["models"] = serde_json::json!([])).contains("no data model"));
    assert!(
        broken(&|driver| driver["platforms"]["linux-x86_64"]["file"] = "../evil".into()).contains("plain file name")
    );
    assert!(broken(&|driver| driver["platforms"]["linux-x86_64"]["sha256"] = "00".into()).contains("SHA-256"));
}

#[test]
fn errors_of_a_driver_map_to_the_engines_kinds() {
    let error = |code, data: Option<serde_json::Value>| RpcError { code, message: "said".into(), data }.to_db_error();
    assert_eq!(error(protocol::METHOD_NOT_FOUND, None).kind, ErrorKind::Unsupported);
    assert_eq!(error(protocol::UNKNOWN_SESSION, None).kind, ErrorKind::NotConnected);
    assert_eq!(error(protocol::INTERNAL_ERROR, None).kind, ErrorKind::Server);
    let detailed = error(
        protocol::DATABASE_ERROR,
        Some(serde_json::json!({ "kind": "authentication", "code": "28P01", "position": 3 })),
    );
    assert_eq!(
        (detailed.kind, detailed.code.as_deref(), detailed.position),
        (ErrorKind::Authentication, Some("28P01"), Some(3))
    );
}

#[test]
fn statements_of_a_language_that_is_not_sql_read_only_by_their_declared_verbs() {
    let mut declaration = DriverDeclaration::parse_list(&manifest()).unwrap().remove(0);
    declaration.capabilities.language = "mongo".into();
    declaration.read_verbs = vec!["find".into()];
    let driver =
        ExternalDriver::new(ExternalSpec { declaration, program: PathBuf::from("/none"), sha256: None, plugin: None });
    assert_eq!(driver.classify("FIND users"), StatementKind::Read);
    assert_eq!(driver.classify("drop users"), StatementKind::Write);
    assert_eq!(driver.classify(""), StatementKind::Write);
    assert_eq!(driver.classify("db.users.find({ age: { $gt: 3 } }).limit(5)"), StatementKind::Read);
    assert_eq!(driver.classify("db['users'].find()"), StatementKind::Read);
    assert_eq!(driver.classify("db.users.deleteMany({ find: 1 })"), StatementKind::Write);
    assert_eq!(command_verb("show collections"), "show");
    assert_eq!(command_verb("  find (x)"), "find");
    assert!(!driver.changes_structure("drop users"), "only SQL is judged for structure");
}

#[test]
fn the_log_keeps_its_newest_lines_and_cuts_long_ones() {
    let log = DriverLog::default();
    for line in 0..600 {
        log.push(format!("line {line}"));
    }
    let (tail, truncated) = log.tail(2);
    assert_eq!((tail, truncated), (vec!["line 598".to_owned(), "line 599".to_owned()], true));
    log.push("é".repeat(3000));
    assert!(log.tail(1).0[0].ends_with('…'));
}

#[test]
fn declaring_keeps_a_driver_whose_spec_did_not_change() {
    let declaration = DriverDeclaration::parse_list(&manifest()).unwrap().remove(0);
    let spec = ExternalSpec { declaration, program: PathBuf::from("/none"), sha256: None, plugin: Some("duck".into()) };
    let declared = Declared::default();
    declared.set(vec![spec.clone(), spec.clone()]);
    let first = declared.drivers();
    assert_eq!(first.len(), 1, "an id is declared once");
    declared.clone().set(vec![spec]);
    assert!(Arc::ptr_eq(&first[0], &declared.drivers()[0]));
    declared.set(Vec::new());
    assert!(declared.as_drivers().is_empty());
}

#[test]
fn a_drivers_rows_are_checked_against_its_columns_and_cut_to_what_was_asked() {
    use super::session::checked_rows;
    let rows = vec![vec![1, 2], vec![3, 4], vec![5, 6]];
    assert_eq!(checked_rows(2, rows.clone(), Some(5)).unwrap(), (rows.clone(), false));
    assert_eq!(checked_rows(2, rows.clone(), Some(2)).unwrap(), (rows[..2].to_vec(), true), "past maxRows");
    assert_eq!(checked_rows(2, rows.clone(), Some(0)).unwrap().0.len(), 0);
    let ragged = vec![vec![1, 2], vec![3]];
    let error = checked_rows(2, ragged, None).unwrap_err();
    assert!(error.detail.contains("row of 1 values for 2 columns (row 2)"), "{}", error.detail);
}

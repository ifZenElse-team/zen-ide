use super::*;

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

#[test]
fn a_query_reads_its_columns_typed_and_keeps_repeated_names_by_place() {
    let printed = read(
        &lines(concat!(
            r#"{"results":[{"columns":[{"name":"ID","type":"NUMBER"},{"name":"ID","type":"VARCHAR2"},{"name":"BORN","type":"DATE"}],"items":"#,
            "\n",
            r#"[{"id":1,"id":"a","born":"2026-10-31 00:00:00"},{"id":2,"id":"","born":""},{"id":3,"id":"c","born":""}]}]}"#,
            "\n3 rows selected. \n"
        )),
        "SELECT * FROM t",
        2,
    );
    assert!(printed.has_result && printed.truncated && printed.failure.is_none());
    assert_eq!(printed.columns[1], Column { name: "ID".into(), type_name: "VARCHAR2".into() });
    assert_eq!(printed.rows.len(), 2);
    assert_eq!(
        printed.rows[0],
        vec![value::integer(1), value::text("a"), value::temporal("2026-10-31 00:00:00", "date")]
    );
    assert_eq!(printed.rows[1][1], value::null());
    assert!(printed.notes.is_empty(), "{:?}", printed.notes);
}

#[test]
fn fields_are_found_by_name_when_some_are_left_out() {
    let printed = read(
        &lines(
            r#"{"results":[{"columns":[{"name":"A","type":"NUMBER"},{"name":"B","type":"VARCHAR2"}],"items":[{"b":"x"}]}]}"#,
        ),
        "SELECT a, b FROM t",
        10,
    );
    assert_eq!(printed.rows[0], vec![value::null(), value::text("x")]);
}

#[test]
fn feedback_counts_changes_and_keeps_other_lines() {
    let printed = read(&lines("\n2 rows updated.\n"), "UPDATE t SET a = 1", 10);
    assert_eq!(printed.affected, Some(2));
    assert!(!printed.has_result);
    let created = read(&lines("Table T created.\n"), "CREATE TABLE t (a NUMBER)", 10);
    assert_eq!(created.notes, ["Table T created."]);
    assert_eq!(read(&lines("1 row inserted."), "INSERT INTO t VALUES (1)", 10).affected, Some(1));
}

#[test]
fn an_error_report_names_its_code_and_the_place() {
    let report = lines(
        "\nError starting at line : 1 in command -\nSELECT *\n  FROM nope\nError at Command Line : 2 Column : 8\nError report -\nSQL Error: ORA-00942: table or view \"HR\".\"NOPE\" does not exist\n\nhttps://docs.oracle.com/error-help/db/ora-00942/\n00942. 00000 -  \"table or view %s does not exist\"\n*Cause:    The table or view does not exist.\n",
    );
    let failure = failure(&report, "SELECT *\n  FROM nope").unwrap();
    assert_eq!(failure.message, "ORA-00942: table or view \"HR\".\"NOPE\" does not exist");
    let data = failure.data.unwrap();
    assert_eq!(
        (data["kind"].as_str(), data["code"].as_str(), data["position"].as_u64()),
        (Some("server"), Some("ORA-00942"), Some(17))
    );
}

#[test]
fn plsql_errors_point_at_the_line_and_column_of_the_block() {
    let report = lines(
        "Error starting at line : 1 in command -\nBEGIN\n  x := 1;\nEND;\nError report -\nORA-06550: line 2, column 3:\nPLS-00201: identifier 'X' must be declared\nORA-06550: line 2, column 3:\nPL/SQL: Statement ignored\n",
    );
    let failure = failure(&report, "BEGIN\n  x := 1;\nEND;").unwrap();
    assert_eq!(failure.message, "PLS-00201: identifier 'X' must be declared");
    assert_eq!(failure.data.unwrap()["position"], 9);
    let compiled = lines(
        "Procedure P compiled\n\nLINE/COL  ERROR\n--------- -------------------------------------------------------------\n3/1       PLS-00103: Encountered the symbol \"END\"\nErrors: check compiler log\n",
    );
    let failure = super::failure(&compiled, "CREATE PROCEDURE p IS\nBEGIN\nEND;").unwrap();
    assert!(failure.message.starts_with("PLS-00103"), "{}", failure.message);
    assert_eq!(failure.data.unwrap()["position"], 29);
}

#[test]
fn errors_are_sorted_into_kinds() {
    let kind = |text: &str| failure(&lines(text), "x").unwrap().kind().map(str::to_owned);
    assert_eq!(kind("ORA-01013: user requested cancel of current operation").as_deref(), Some("cancelled"));
    assert_eq!(
        kind("  Error Message = ORA-01017: invalid credential or not authorized; logon denied").as_deref(),
        Some("authentication")
    );
    assert_eq!(
        kind("  Error Message = IO Error: The Network Adapter could not establish the connection").as_deref(),
        Some("connection")
    );
    assert_eq!(
        kind("ORA-12514: Cannot connect to database. Service x is not registered").as_deref(),
        Some("connection")
    );
    assert_eq!(
        kind("IO Error: PKIX path building failed: unable to find valid certification path").as_deref(),
        Some("tls")
    );
    assert_eq!(kind("SP2-0640: Not connected").as_deref(), Some("connection"));
    assert!(failure(&lines("PL/SQL procedure successfully completed."), "x").is_none());
}

#[test]
fn positions_count_characters_of_earlier_lines() {
    assert_eq!(position("ab\ncdé", 2, 3), Some(6));
    assert_eq!(position("ab", 1, 40), Some(2));
    assert_eq!(position("ab", 3, 1), None);
}

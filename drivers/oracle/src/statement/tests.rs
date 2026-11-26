use super::*;

#[test]
fn sql_loses_its_final_semicolon_and_blocks_keep_theirs() {
    let select = check("  SELECT 1 FROM dual ;  ").unwrap();
    assert_eq!(
        (select.text.as_str(), select.verb.as_str(), select.kind),
        ("  SELECT 1 FROM dual", "SELECT", Kind::Query)
    );
    assert!(select.reads());
    let block = check("BEGIN\n  NULL;\nEND;\n").unwrap();
    assert_eq!((block.text.as_str(), block.kind), ("BEGIN\n  NULL;\nEND;", Kind::Block));
    let unit = check("create or replace editionable procedure p is begin null; end;").unwrap();
    assert_eq!(unit.kind, Kind::Block);
    assert_eq!(check("-- note\nINSERT INTO t VALUES (1)").unwrap().kind, Kind::Change);
    assert_eq!(check("/* x */ CREATE TABLE t (a NUMBER)").unwrap().kind, Kind::Definition);
    assert_eq!(check("ROLLBACK TO SAVEPOINT a").unwrap().kind, Kind::Other);
    assert_eq!(check("rollback").unwrap().kind, Kind::Rollback);
    assert_eq!(check("SET TRANSACTION READ ONLY").unwrap().kind, Kind::Other);
    assert!(!check("SELECT * FROM t FOR UPDATE").unwrap().reads());
}

#[test]
fn commands_of_sqlcl_are_refused_where_they_start() {
    for (text, at) in
        [("host ls", 1), ("  SET ECHO ON", 3), ("@script.sql", 1), ("!ls", 1), ("CONNECT a/b", 1), ("SPOOL x", 1)]
    {
        let failure = check(text).unwrap_err();
        assert_eq!(failure.kind(), Some("invalid"), "{text}");
        assert_eq!(failure.data.unwrap()["position"], at, "{text}");
    }
    assert_eq!(check("   ").unwrap_err().kind(), Some("invalid"));
}

#[test]
fn lines_sqlcl_would_act_on_are_refused() {
    let slash = check("SELECT 1\n/\nFROM dual").unwrap_err();
    assert_eq!(slash.data.unwrap()["position"], 10);
    assert!(check("SELECT 1 FROM dual\n  #host ls").is_err());
    assert!(check("BEGIN\n  NULL;\n.\nEND;").is_err());
    assert!(check("SELECT 10/2, '#' FROM dual").is_ok());
}

#[test]
fn names_are_quoted_and_texts_doubled() {
    assert_eq!(quote("HR"), "\"HR\"");
    assert_eq!(quote("a\"b"), "\"a\"\"b\"");
    assert_eq!(literal("it's"), "'it''s'");
}

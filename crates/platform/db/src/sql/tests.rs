use super::*;

use Dialect::{Lines as L, MySql as M, Oracle as O, Postgres as P, Script as J, SqlServer as S, Standard as Q};

fn texts(text: &str, dialect: Dialect) -> Vec<String> {
    split(text, dialect).iter().map(|statement| statement.text(text).to_owned()).collect()
}

const SPLITS: &[(Dialect, &str, &[&str])] = &[
    (Q, "", &[]),
    (Q, "   \n\n  ", &[]),
    (Q, "SELECT 1", &["SELECT 1"]),
    (Q, "SELECT 1;", &["SELECT 1"]),
    (Q, "SELECT 1; SELECT 2;", &["SELECT 1", "SELECT 2"]),
    (Q, "SELECT 1;SELECT 2", &["SELECT 1", "SELECT 2"]),
    (Q, "SELECT 1;\nSELECT 2;\n", &["SELECT 1", "SELECT 2"]),
    (Q, "SELECT 1;;;SELECT 2", &["SELECT 1", "SELECT 2"]),
    (Q, ";;;", &[]),
    (Q, "  SELECT 1  ;  ", &["SELECT 1"]),
    (Q, "SELECT 'a;b';", &["SELECT 'a;b'"]),
    (Q, "SELECT 'it''s; fine'; SELECT 2", &["SELECT 'it''s; fine'", "SELECT 2"]),
    (Q, "SELECT \"a;b\" FROM t; SELECT 2", &["SELECT \"a;b\" FROM t", "SELECT 2"]),
    (Q, "SELECT \"a\"\";b\"; SELECT 2", &["SELECT \"a\"\";b\"", "SELECT 2"]),
    (Q, "SELECT 1 -- no; split\n; SELECT 2", &["SELECT 1 -- no; split", "SELECT 2"]),
    (Q, "SELECT /* ; */ 1; SELECT 2", &["SELECT /* ; */ 1", "SELECT 2"]),
    (Q, "SELECT /* a\n; b\n*/ 1; SELECT 2", &["SELECT /* a\n; b\n*/ 1", "SELECT 2"]),
    (Q, "-- only a comment", &[]),
    (Q, "/* only\n a comment */", &[]),
    (Q, "-- heading\nSELECT 1;", &["-- heading\nSELECT 1"]),
    (Q, "SELECT 1; -- trailing\nSELECT 2;", &["SELECT 1", "SELECT 2"]),
    (Q, "SELECT 1; /* after */\nSELECT 2;", &["SELECT 1", "SELECT 2"]),
    (Q, "SELECT 1;\n-- about two\nSELECT 2;", &["SELECT 1", "-- about two\nSELECT 2"]),
    (Q, "SELECT 1;\n-- trailing comment only", &["SELECT 1"]),
    (Q, "SELECT f(1; 2); SELECT 3", &["SELECT f(1; 2)", "SELECT 3"]),
    (Q, "SELECT (\n1;\n); SELECT 3", &["SELECT (\n1;\n)", "SELECT 3"]),
    (Q, "SELECT a[1;2]; SELECT 3", &["SELECT a[1;2]", "SELECT 3"]),
    (Q, "SELECT CASE WHEN a THEN 1 ELSE 2 END; SELECT 3", &["SELECT CASE WHEN a THEN 1 ELSE 2 END", "SELECT 3"]),
    (Q, "SELECT CASE WHEN a; SELECT 3", &["SELECT CASE WHEN a", "SELECT 3"]),
    (Q, "BEGIN; SELECT 1; COMMIT;", &["BEGIN", "SELECT 1", "COMMIT"]),
    (Q, "begin;\nselect 1;\nend;", &["begin", "select 1", "end"]),
    (Q, "BEGIN TRANSACTION; SELECT 1; COMMIT;", &["BEGIN TRANSACTION", "SELECT 1", "COMMIT"]),
    (Q, "BEGIN WORK; ROLLBACK;", &["BEGIN WORK", "ROLLBACK"]),
    (Q, "BEGIN ISOLATION LEVEL SERIALIZABLE; SELECT 1;", &["BEGIN ISOLATION LEVEL SERIALIZABLE", "SELECT 1"]),
    (Q, "BEGIN READ ONLY; SELECT 1;", &["BEGIN READ ONLY", "SELECT 1"]),
    (Q, "BEGIN NOT DEFERRABLE; SELECT 1;", &["BEGIN NOT DEFERRABLE", "SELECT 1"]),
    (Q, "BEGIN DEFERRED TRANSACTION; SELECT 1;", &["BEGIN DEFERRED TRANSACTION", "SELECT 1"]),
    (Q, "BEGIN IMMEDIATE; SELECT 1;", &["BEGIN IMMEDIATE", "SELECT 1"]),
    (Q, "BEGIN\n;\nSELECT 1;", &["BEGIN", "SELECT 1"]),
    (
        Q,
        "CREATE TRIGGER t AFTER INSERT ON a BEGIN INSERT INTO b VALUES (1); UPDATE c SET x = 1; END; SELECT 1;",
        &["CREATE TRIGGER t AFTER INSERT ON a BEGIN INSERT INTO b VALUES (1); UPDATE c SET x = 1; END", "SELECT 1"],
    ),
    (
        Q,
        "CREATE TRIGGER t BEGIN\n  SELECT CASE WHEN 1 THEN 2 END;\n  SELECT 3;\nEND;\nSELECT 4;",
        &["CREATE TRIGGER t BEGIN\n  SELECT CASE WHEN 1 THEN 2 END;\n  SELECT 3;\nEND", "SELECT 4"],
    ),
    (
        Q,
        "CREATE PROCEDURE p BEGIN IF x THEN SELECT 1; END IF; SELECT 2; END; SELECT 3",
        &["CREATE PROCEDURE p BEGIN IF x THEN SELECT 1; END IF; SELECT 2; END", "SELECT 3"],
    ),
    (
        Q,
        "CREATE PROCEDURE p BEGIN LOOP SELECT 1; END LOOP; END; SELECT 3",
        &["CREATE PROCEDURE p BEGIN LOOP SELECT 1; END LOOP; END", "SELECT 3"],
    ),
    (
        Q,
        "CREATE PROCEDURE p BEGIN WHILE x DO SELECT 1; END WHILE; END; SELECT 3",
        &["CREATE PROCEDURE p BEGIN WHILE x DO SELECT 1; END WHILE; END", "SELECT 3"],
    ),
    (
        Q,
        "CREATE PROCEDURE p BEGIN REPEAT SELECT 1; UNTIL x END REPEAT; END; SELECT 3",
        &["CREATE PROCEDURE p BEGIN REPEAT SELECT 1; UNTIL x END REPEAT; END", "SELECT 3"],
    ),
    (
        Q,
        "CREATE PROCEDURE p BEGIN CASE x WHEN 1 THEN SELECT 1; END CASE; END; SELECT 3",
        &["CREATE PROCEDURE p BEGIN CASE x WHEN 1 THEN SELECT 1; END CASE; END", "SELECT 3"],
    ),
    (
        Q,
        "CREATE PROCEDURE p BEGIN BEGIN SELECT 1; END; SELECT 2; END; SELECT 3",
        &["CREATE PROCEDURE p BEGIN BEGIN SELECT 1; END; SELECT 2; END", "SELECT 3"],
    ),
    (Q, "SELECT 1\nSELECT 2", &["SELECT 1\nSELECT 2"]),
    (Q, "SELECT 1\n\nSELECT 2", &["SELECT 1\n\nSELECT 2"]),
    (Q, "SELECT 'a\n;\nb'; SELECT 2", &["SELECT 'a\n;\nb'", "SELECT 2"]),
    (Q, "SELECT 'a\\'; SELECT 2", &["SELECT 'a\\'", "SELECT 2"]),
    (Q, "SELECT $$a;b$$; SELECT 2", &["SELECT $$a", "b$$", "SELECT 2"]),
    (Q, "SELECT 'кириллица; да'; SELECT 2", &["SELECT 'кириллица; да'", "SELECT 2"]),
    (Q, "SELECT \"ключ\" FROM т; SELECT 2", &["SELECT \"ключ\" FROM т", "SELECT 2"]),
    (Q, "SELECT 1 /* unterminated ; ", &["SELECT 1 /* unterminated ;"]),
    (Q, "SELECT 'unterminated ; ", &["SELECT 'unterminated ;"]),
    (Q, "SELECT 1\r\n;\r\nSELECT 2\r\n", &["SELECT 1", "SELECT 2"]),
    (Q, "\\set x 1\nSELECT 1;", &["\\set x 1\nSELECT 1"]),
    (P, "SELECT $$a;b$$; SELECT 2", &["SELECT $$a;b$$", "SELECT 2"]),
    (P, "SELECT $tag$ a; $$ ; b $tag$; SELECT 2", &["SELECT $tag$ a; $$ ; b $tag$", "SELECT 2"]),
    (
        P,
        "CREATE FUNCTION f() RETURNS int AS $$\nBEGIN\n  RETURN 1;\nEND;\n$$ LANGUAGE plpgsql;\nSELECT f();",
        &["CREATE FUNCTION f() RETURNS int AS $$\nBEGIN\n  RETURN 1;\nEND;\n$$ LANGUAGE plpgsql", "SELECT f()"],
    ),
    (
        P,
        "DO $do$ BEGIN RAISE NOTICE 'x;'; END $do$; SELECT 1",
        &["DO $do$ BEGIN RAISE NOTICE 'x;'; END $do$", "SELECT 1"],
    ),
    (P, "SELECT $1; SELECT $2", &["SELECT $1", "SELECT $2"]),
    (P, "SELECT a$b$c; SELECT 2", &["SELECT a$b$c", "SELECT 2"]),
    (P, "SELECT $_x$;$_x$; SELECT 2", &["SELECT $_x$;$_x$", "SELECT 2"]),
    (P, "SELECT E'a\\';b'; SELECT 2", &["SELECT E'a\\';b'", "SELECT 2"]),
    (P, "SELECT e'\\\\'; SELECT 2", &["SELECT e'\\\\'", "SELECT 2"]),
    (P, "SELECT 'a\\'; SELECT 2", &["SELECT 'a\\'", "SELECT 2"]),
    (P, "SELECT U&'d\\0061t'; SELECT 2", &["SELECT U&'d\\0061t'", "SELECT 2"]),
    (P, "SELECT /* a /* nested; */ still; */ 1; SELECT 2", &["SELECT /* a /* nested; */ still; */ 1", "SELECT 2"]),
    (P, "\\c otherdb\nSELECT 1;", &["\\c otherdb", "SELECT 1"]),
    (P, "SELECT 1;\n\\timing on\nSELECT 2;", &["SELECT 1", "\\timing on", "SELECT 2"]),
    (P, "  \\x\nSELECT 1", &["\\x", "SELECT 1"]),
    (P, "SELECT '\\x';", &["SELECT '\\x'"]),
    (P, "BEGIN; SELECT 1; END;", &["BEGIN", "SELECT 1", "END"]),
    (
        P,
        "CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END; SELECT 3;",
        &["CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END", "SELECT 3"],
    ),
    (P, "SELECT ARRAY[1;2]; SELECT 3", &["SELECT ARRAY[1;2]", "SELECT 3"]),
    (P, "SELECT \"we;ird\"\"q\"; SELECT 2", &["SELECT \"we;ird\"\"q\"", "SELECT 2"]),
    (P, "SELECT 1 -- x\n-- y;\n; SELECT 2", &["SELECT 1 -- x\n-- y;", "SELECT 2"]),
    (P, "COPY t FROM STDIN;", &["COPY t FROM STDIN"]),
    (P, "SELECT '$$'; SELECT 2", &["SELECT '$$'", "SELECT 2"]),
    (P, "SELECT 1::text; SELECT 2", &["SELECT 1::text", "SELECT 2"]),
    (M, "SELECT 'a\\';b'; SELECT 2", &["SELECT 'a\\';b'", "SELECT 2"]),
    (M, "SELECT \"a\\\";b\"; SELECT 2", &["SELECT \"a\\\";b\"", "SELECT 2"]),
    (M, "SELECT `a;b` FROM t; SELECT 2", &["SELECT `a;b` FROM t", "SELECT 2"]),
    (M, "SELECT 1 # comment; still\n; SELECT 2", &["SELECT 1 # comment; still", "SELECT 2"]),
    (M, "SELECT 1 --; SELECT 2", &["SELECT 1 --", "SELECT 2"]),
    (M, "SELECT 1 -- c; d\n; SELECT 2", &["SELECT 1 -- c; d", "SELECT 2"]),
    (M, "SELECT 1 /*! c; */; SELECT 2", &["SELECT 1 /*! c; */", "SELECT 2"]),
    (
        M,
        "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END//\nDELIMITER ;\nSELECT 3;",
        &["DELIMITER //", "CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END", "DELIMITER ;", "SELECT 3"],
    ),
    (M, "DELIMITER $$\nSELECT 1; SELECT 2$$\nSELECT 3$$", &["DELIMITER $$", "SELECT 1; SELECT 2", "SELECT 3"]),
    (M, "delimiter ;;\nSELECT 1;;\nSELECT 2;;", &["delimiter ;;", "SELECT 1", "SELECT 2"]),
    (M, "SELECT 'a//b';\nDELIMITER //\nSELECT '//'//", &["SELECT 'a//b'", "DELIMITER //", "SELECT '//'"]),
    (M, "SELECT delimiter FROM t; SELECT 2", &["SELECT delimiter FROM t", "SELECT 2"]),
    (M, "BEGIN; SELECT 1; COMMIT;", &["BEGIN", "SELECT 1", "COMMIT"]),
    (M, "START TRANSACTION; SELECT 1; COMMIT;", &["START TRANSACTION", "SELECT 1", "COMMIT"]),
    (M, "SELECT $$a;b$$; SELECT 2", &["SELECT $$a", "b$$", "SELECT 2"]),
    (S, "SELECT 1\nGO\nSELECT 2\nGO", &["SELECT 1", "SELECT 2"]),
    (S, "SELECT 1\ngo 5\nSELECT 2", &["SELECT 1", "SELECT 2"]),
    (S, "SELECT 1\n  GO  \nSELECT 2", &["SELECT 1", "SELECT 2"]),
    (S, "SELECT 1; SELECT 2\nGO", &["SELECT 1", "SELECT 2"]),
    (S, "SELECT go FROM t\nGO", &["SELECT go FROM t"]),
    (S, "SELECT 'x\nGO\ny'\nGO", &["SELECT 'x\nGO\ny'"]),
    (S, "SELECT [a;b] FROM t; SELECT 2", &["SELECT [a;b] FROM t", "SELECT 2"]),
    (S, "SELECT [a]]b;] FROM t; SELECT 2", &["SELECT [a]]b;] FROM t", "SELECT 2"]),
    (S, "SELECT N'a;b'; SELECT 2", &["SELECT N'a;b'", "SELECT 2"]),
    (S, "SELECT /* /* ; */ ; */ 1; SELECT 2", &["SELECT /* /* ; */ ; */ 1", "SELECT 2"]),
    (S, "BEGIN TRAN; SELECT 1; COMMIT;", &["BEGIN TRAN", "SELECT 1", "COMMIT"]),
    (S, "BEGIN TRANSACTION t1; SELECT 1;", &["BEGIN TRANSACTION t1", "SELECT 1"]),
    (S, "BEGIN DISTRIBUTED TRANSACTION; SELECT 1;", &["BEGIN DISTRIBUTED TRANSACTION", "SELECT 1"]),
    (S, "BEGIN\n  SELECT 1;\n  SELECT 2;\nEND;\nSELECT 3;", &["BEGIN\n  SELECT 1;\n  SELECT 2;\nEND", "SELECT 3"]),
    (
        S,
        "BEGIN TRY\n  SELECT 1/0;\nEND TRY\nBEGIN CATCH\n  SELECT 2;\nEND CATCH;\nSELECT 3;",
        &["BEGIN TRY\n  SELECT 1/0;\nEND TRY\nBEGIN CATCH\n  SELECT 2;\nEND CATCH", "SELECT 3"],
    ),
    (
        S,
        "IF 1 = 1\nBEGIN\n  SELECT 1;\nEND\nELSE\nBEGIN\n  SELECT 2;\nEND;\nSELECT 3;",
        &["IF 1 = 1\nBEGIN\n  SELECT 1;\nEND\nELSE\nBEGIN\n  SELECT 2;\nEND", "SELECT 3"],
    ),
    (
        S,
        "CREATE PROC p AS\nBEGIN\n  SELECT 1;\nEND\nGO\nEXEC p",
        &["CREATE PROC p AS\nBEGIN\n  SELECT 1;\nEND", "EXEC p"],
    ),
    (O, "SELECT 1 FROM dual; SELECT 2 FROM dual", &["SELECT 1 FROM dual", "SELECT 2 FROM dual"]),
    (O, "SELECT 1 FROM dual\n/\nSELECT 2 FROM dual\n/\n", &["SELECT 1 FROM dual", "SELECT 2 FROM dual"]),
    (O, "SELECT q'[a;b']' FROM dual; SELECT 2", &["SELECT q'[a;b']' FROM dual", "SELECT 2"]),
    (O, "SELECT q'{x\n;y}' FROM dual; SELECT 2", &["SELECT q'{x\n;y}' FROM dual", "SELECT 2"]),
    (O, "SELECT 10/2 FROM dual; SELECT 2", &["SELECT 10/2 FROM dual", "SELECT 2"]),
    (
        O,
        "BEGIN\n  NULL;\n\n  NULL;\nEND;\n/\nSELECT 1 FROM dual;",
        &["BEGIN\n  NULL;\n\n  NULL;\nEND;", "SELECT 1 FROM dual"],
    ),
    (
        O,
        "DECLARE\n  n NUMBER;\nBEGIN\n  n := 1;\nEND;\n/\nSELECT 1 FROM dual",
        &["DECLARE\n  n NUMBER;\nBEGIN\n  n := 1;\nEND;", "SELECT 1 FROM dual"],
    ),
    (
        O,
        "CREATE OR REPLACE PROCEDURE p IS\n  n NUMBER;\nBEGIN\n  n := 1;\nEND p;\n/\nCREATE TABLE t (a NUMBER);",
        &["CREATE OR REPLACE PROCEDURE p IS\n  n NUMBER;\nBEGIN\n  n := 1;\nEND p;", "CREATE TABLE t (a NUMBER)"],
    ),
    (
        O,
        "CREATE OR REPLACE PACKAGE BODY k AS\n  PROCEDURE p IS BEGIN NULL; END;\nEND k;\n/",
        &["CREATE OR REPLACE PACKAGE BODY k AS\n  PROCEDURE p IS BEGIN NULL; END;\nEND k;"],
    ),
    (
        O,
        "CREATE TABLE t (a NUMBER); CREATE INDEX i ON t (a);",
        &["CREATE TABLE t (a NUMBER)", "CREATE INDEX i ON t (a)"],
    ),
    (L, "SET a 1\nGET a\n\nDEL a", &["SET a 1", "GET a", "DEL a"]),
    (L, "  PING  \n", &["PING"]),
    (L, "SET a 'x;y'\n", &["SET a 'x;y'"]),
    (
        J,
        "db.a.find({\n  b: 'x;\\'y'\n})\n  .sort({ c: 1 });\ndb.d.drop()",
        &["db.a.find({\n  b: 'x;\\'y'\n})\n  .sort({ c: 1 })", "db.d.drop()"],
    ),
    (J, "db.a.find() // no; split\n;show dbs", &["db.a.find() // no; split", "show dbs"]),
];

#[test]
fn scripts_split_into_their_statements() {
    for (dialect, text, expected) in SPLITS {
        assert_eq!(texts(text, *dialect), *expected, "{dialect:?}: {text:?}");
    }
}

#[test]
fn client_commands_are_marked() {
    let text = "\\set x 1\nSELECT 1;";
    let found = split(text, Dialect::Postgres);
    assert!(found[0].client);
    assert!(!found[1].client);
    let text = "DELIMITER //\nSELECT 1//";
    let found = split(text, Dialect::MySql);
    assert_eq!(found.iter().map(|statement| statement.client).collect::<Vec<_>>(), [true, false]);
    let text = "@set id = 5\nSELECT :id;\n  @include lib.sql\nSELECT 2";
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Standard] {
        let found = split(text, dialect);
        let shown: Vec<(&str, bool)> = found.iter().map(|statement| (statement.text(text), statement.client)).collect();
        assert_eq!(
            shown,
            [("@set id = 5", true), ("SELECT :id", false), ("@include lib.sql", true), ("SELECT 2", false)]
        );
    }
}

#[test]
fn delimiters_end_where_they_end() {
    let text = "SELECT 1 ;  SELECT 2";
    let found = split(text, Dialect::Postgres);
    assert_eq!(found[0], Statement { start: 0, end: 8, delimiter_end: 10, client: false });
    assert_eq!(found[1], Statement { start: 12, end: 20, delimiter_end: 20, client: false });
}

fn at(text: &str, dialect: Dialect, mode: BlankLines) -> Option<String> {
    let caret = text.find('|').expect("a caret");
    let clean = text.replacen('|', "", 1);
    let script = Script::new(&clean, dialect);
    script.statement_at(&clean, caret, mode).map(|statement| statement.text(&clean).to_owned())
}

const AT: &[(Dialect, BlankLines, &str, Option<&str>)] = &[
    (P, BlankLines::Always, "SELECT |1; SELECT 2;", Some("SELECT 1")),
    (P, BlankLines::Always, "|SELECT 1; SELECT 2;", Some("SELECT 1")),
    (P, BlankLines::Always, "SELECT 1;| SELECT 2;", Some("SELECT 1")),
    (P, BlankLines::Always, "SELECT 1; |SELECT 2;", Some("SELECT 2")),
    (P, BlankLines::Always, "SELECT 1;SELECT 2|;", Some("SELECT 2")),
    (P, BlankLines::Always, "SELECT 1;SELECT 2;|", Some("SELECT 2")),
    (P, BlankLines::Always, "SELECT 1;   |\nSELECT 2;", Some("SELECT 1")),
    (P, BlankLines::Always, "SELECT 1;\n|SELECT 2;", Some("SELECT 2")),
    (P, BlankLines::Always, "SELECT 1;\n   |   SELECT 2;", Some("SELECT 2")),
    (P, BlankLines::Always, "SELECT 1;\n\n|\nSELECT 2;", Some("SELECT 1")),
    (P, BlankLines::Always, "SELECT 1\n\n|\nSELECT 2", Some("SELECT 1")),
    (P, BlankLines::Always, "SELECT 1\n\nSELECT |2", Some("SELECT 2")),
    (P, BlankLines::Always, "SELECT |1\n\nSELECT 2", Some("SELECT 1")),
    (P, BlankLines::Never, "SELECT |1\n\nSELECT 2", Some("SELECT 1\n\nSELECT 2")),
    (P, BlankLines::Always, "SELECT *\nFROM t\n\nWHERE |x", Some("WHERE x")),
    (P, BlankLines::Smart, "SELECT *\nFROM t\n\nWHERE |x", Some("SELECT *\nFROM t\n\nWHERE x")),
    (P, BlankLines::Smart, "SELECT *\nFROM\n\nt|", Some("SELECT *\nFROM\n\nt")),
    (P, BlankLines::Smart, "SELECT a,\n\nb|", Some("SELECT a,\n\nb")),
    (P, BlankLines::Smart, "SELECT (1\n\n+ 2|)", Some("SELECT (1\n\n+ 2)")),
    (P, BlankLines::Always, "SELECT (1\n\n+ 2|)", Some("+ 2)")),
    (P, BlankLines::Smart, "SELECT 1\n\nSELECT |2", Some("SELECT 2")),
    (P, BlankLines::Always, "SELECT 'a\n\n|b'", Some("SELECT 'a\n\nb'")),
    (P, BlankLines::Always, "SELECT $$a\n\n|b$$", Some("SELECT $$a\n\nb$$")),
    (P, BlankLines::Always, "SELECT /* a\n\n|b */ 1", Some("SELECT /* a\n\nb */ 1")),
    (
        Q,
        BlankLines::Always,
        "CREATE TRIGGER t BEGIN\n  SELECT 1;\n\n  SELECT |2;\nEND;",
        Some("CREATE TRIGGER t BEGIN\n  SELECT 1;\n\n  SELECT 2;\nEND"),
    ),
    (P, BlankLines::Always, "-- note\n\nSELECT |1", Some("SELECT 1")),
    (P, BlankLines::Always, "-- note\nSELECT |1", Some("-- note\nSELECT 1")),
    (P, BlankLines::Always, "SELECT 1;\n-- only |comment", Some("SELECT 1")),
    (P, BlankLines::Always, "|", None),
    (P, BlankLines::Always, "\n\n|\n", None),
    (P, BlankLines::Always, "SELECT 1;\n\n\n\n|", Some("SELECT 1")),
    (P, BlankLines::Always, "  |  \nSELECT 1", Some("SELECT 1")),
    (M, BlankLines::Always, "DELIMITER //\nSELECT 1; SELECT |2//", Some("SELECT 1; SELECT 2")),
    (S, BlankLines::Always, "SELECT 1\nGO\nSELECT |2\nGO", Some("SELECT 2")),
    (L, BlankLines::Always, "GET a\nSET |b 1\n", Some("SET b 1")),
    (J, BlankLines::Smart, "db.a.find({\n\n  b: |1 })", Some("db.a.find({\n\n  b: 1 })")),
    (J, BlankLines::Always, "db.a.find()\n\ndb.b.|find()", Some("db.b.find()")),
];

#[test]
fn the_statement_at_the_caret_follows_the_rules() {
    for (dialect, mode, text, expected) in AT {
        assert_eq!(at(text, *dialect, *mode).as_deref(), *expected, "{dialect:?} {mode:?}: {text:?}");
    }
}

#[test]
fn the_table_has_more_than_a_hundred_cases() {
    assert!(SPLITS.len() + AT.len() >= 100, "{}", SPLITS.len() + AT.len());
}

#[test]
fn execute_from_the_caret_takes_the_rest() {
    let text = "SELECT 1; SELECT 2;\nSELECT 3;";
    let script = Script::new(text, Dialect::Postgres);
    let rest: Vec<&str> = script.statements_from(text, 11).iter().map(|statement| statement.text(text)).collect();
    assert_eq!(rest, ["SELECT 2", "SELECT 3"]);
}

#[test]
fn next_and_previous_statements_step_over_blank_lines() {
    let text = "SELECT 1;\nSELECT 2\n\nSELECT 3;";
    let script = Script::new(text, Dialect::Postgres);
    let next = script.neighbour(text, 0, true, BlankLines::Always).unwrap();
    assert_eq!(next.text(text), "SELECT 2");
    let next = script.neighbour(text, next.start, true, BlankLines::Always).unwrap();
    assert_eq!(next.text(text), "SELECT 3");
    assert!(script.neighbour(text, next.start, true, BlankLines::Always).is_none());
    let back = script.neighbour(text, next.start, false, BlankLines::Always).unwrap();
    assert_eq!(back.text(text), "SELECT 2");
}

#[test]
fn queries_that_only_read_are_pageable() {
    assert!(pageable("SELECT * FROM t"));
    assert!(pageable("  -- x\n select 1"));
    assert!(pageable("WITH a AS (SELECT 1) SELECT * FROM a"));
    assert!(pageable("VALUES (1), (2)"));
    assert!(pageable("TABLE t"));
    assert!(pageable("SELECT 'insert into' FROM t"));
    assert!(pageable("SELECT \"update\" FROM t"));
    assert!(!pageable("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"));
    assert!(!pageable("INSERT INTO t VALUES (1) RETURNING id"));
    assert!(!pageable("UPDATE t SET a = 1 RETURNING *"));
    assert!(!pageable("SELECT * INTO c FROM t"));
    assert!(!pageable("SELECT * FROM t FOR UPDATE"));
    assert!(!pageable("EXPLAIN SELECT 1"));
    assert!(!pageable("SHOW search_path"));
}

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

#[test]
fn edits_split_as_a_fresh_split_does() {
    let pieces = [
        "SELECT 1;",
        "\n",
        "'",
        "$$",
        "$t$",
        "/*",
        "*/",
        "--",
        "BEGIN",
        "END",
        " ",
        ";",
        "(",
        ")",
        "CASE",
        "\n\n",
        "E'",
        "\\",
        "x",
    ];
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::SqlServer, Dialect::Standard] {
        let mut seed = 7_u64;
        let mut text = String::from("SELECT 1;\nSELECT 2;\n");
        let mut script = Script::new(&text, dialect);
        for _ in 0..3000 {
            let at = (lcg(&mut seed) as usize) % (text.len() + 1);
            let at = (0..=at).rev().find(|at| text.is_char_boundary(*at)).unwrap_or(0);
            if lcg(&mut seed).is_multiple_of(3) && at < text.len() {
                let mut end = (at + 1 + (lcg(&mut seed) as usize) % 6).min(text.len());
                while !text.is_char_boundary(end) {
                    end += 1;
                }
                text.replace_range(at..end, "");
                script.edit(&text, at, end, at);
            } else {
                let piece = pieces[(lcg(&mut seed) as usize) % pieces.len()];
                text.insert_str(at, piece);
                script.edit(&text, at, at, at + piece.len());
            }
            assert_eq!(script.statements(&text), split(&text, dialect), "{dialect:?}: {text:?}");
        }
    }
}

fn large_script() -> String {
    let mut text = String::with_capacity(10 * 1024 * 1024 + 256);
    let mut index = 0;
    while text.len() < 10 * 1024 * 1024 {
        text.push_str(&format!(
            "-- statement {index}\nSELECT id, name, 'text; with a delimiter'\nFROM table_{index}\nWHERE id > {index};\n\n"
        ));
        index += 1;
    }
    text
}

#[test]
fn an_edit_in_a_large_script_lexes_a_few_lines() {
    let mut text = large_script();
    let mut script = Script::new(&text, Dialect::Postgres);
    let middle = text.len() / 2;
    let line = text[middle..].find('\n').map_or(middle, |at| middle + at + 1);
    text.insert(line, 'x');
    assert!(script.edit(&text, line, line, line + 1) <= 6);
    text.insert(line, '\'');
    let lexed = script.edit(&text, line, line, line + 1);
    assert!(lexed > 100, "a quote opened changes the lines after it");
    text.replace_range(line..line + 1, "");
    let lexed = script.edit(&text, line, line + 1, line);
    assert!(lexed > 100);
    let statement = script.statement_at(&text, line + 3, BlankLines::Always).unwrap();
    assert!(statement.text(&text).contains("SELECT"));
}

#[test]
fn splitting_ten_megabytes_is_fast() {
    if std::env::var_os("ZEN_TIMING_CHECKS").is_none() {
        return;
    }
    let mut text = large_script();
    let started = std::time::Instant::now();
    let mut script = Script::new(&text, Dialect::Postgres);
    let whole = started.elapsed();
    let middle = text.len() / 2;
    let started = std::time::Instant::now();
    text.insert(middle, ' ');
    script.edit(&text, middle, middle, middle + 1);
    let found = script.statement_at(&text, middle, BlankLines::Always);
    let edit = started.elapsed();
    assert!(found.is_some());
    assert!(whole < std::time::Duration::from_secs(2), "{whole:?}");
    assert!(edit < std::time::Duration::from_millis(20), "{edit:?}");
}

#[test]
fn smart_commit_switches_on_the_first_statement_that_may_change_data() {
    for reads in
        ["SELECT 1", "  -- c\n select * from t", "/* x */ SHOW search_path", "SET x = 1", "use db", "EXPLAIN SELECT 1"]
    {
        assert!(!smart_commit_switches(reads), "{reads}");
    }
    for writes in [
        "UPDATE t SET a = 1",
        "insert into t values (1)",
        "DELETE FROM t",
        "WITH d AS (DELETE FROM t) SELECT 1",
        "CREATE TABLE t (a int)",
    ] {
        assert!(smart_commit_switches(writes), "{writes}");
    }
    for control in ["BEGIN", "commit", "ROLLBACK", "savepoint a", "", "-- only a comment"] {
        assert!(!smart_commit_switches(control), "{control}");
    }
    assert!(controls_transaction("start transaction"));
    assert_eq!(first_keyword("/* a */ -- b\n  update t"), Some("UPDATE".to_owned()));
}

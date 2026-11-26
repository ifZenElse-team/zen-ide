use super::*;

fn kinds(text: &str) -> Vec<(&str, Kind)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

#[test]
fn highlights_a_query() {
    let text =
        "SELECT u.id, count(*) AS n FROM users u WHERE u.name = 'O''Brien' AND age > 18.5 -- adults\nLIMIT :limit;";
    assert_eq!(
        kinds(text),
        [
            ("SELECT", Kind::Keyword),
            ("count", Kind::Function),
            ("AS", Kind::Keyword),
            ("FROM", Kind::Keyword),
            ("WHERE", Kind::Keyword),
            ("'O''Brien'", Kind::String),
            ("AND", Kind::Keyword),
            ("18.5", Kind::Number),
            ("-- adults", Kind::Comment),
            ("LIMIT", Kind::Keyword),
            (":limit", Kind::Constant),
        ]
    );
}

#[test]
fn highlights_ddl_types_and_dialects() {
    let text = "create table t (id bigserial primary key, data jsonb not null default '{}'::jsonb, n int, at timestamptz default now()); insert into t(a) values (1); /* x\ny */ \
                $$ body $$ $fn$ a $fn$ $1 @var ? `q` E'a\\'b' # mysql";
    let found = kinds(text);
    for expected in [
        ("create", Kind::Keyword),
        ("bigserial", Kind::Keyword),
        ("primary", Kind::Keyword),
        ("jsonb", Kind::Keyword),
        ("'{}'", Kind::String),
        ("int", Kind::Keyword),
        ("now", Kind::Function),
        ("/* x\ny */", Kind::Comment),
        ("$$ body $$", Kind::String),
        ("$fn$ a $fn$", Kind::String),
        ("$1", Kind::Constant),
        ("@var", Kind::Constant),
        ("?", Kind::Constant),
        ("E'a\\'b'", Kind::String),
        ("# mysql", Kind::Comment),
    ] {
        assert!(found.contains(&expected), "{expected:?} in {found:?}");
    }
    assert!(!found.iter().any(|(word, _)| *word == "q" || *word == "t"), "identifiers stay plain");
}

#[test]
fn survives_any_input() {
    let pieces =
        ["'", "\"", "`", "$", "$$", "$a$", "--", "/*", "*/", "#", "\n", "1", ".", "e", "x", "(", ":", "é", "日", "\\"];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                let text = format!("{a}{b}{c}");
                let mut previous = 0;
                for span in highlight(&text) {
                    assert!(
                        span.start >= previous
                            && span.end > span.start
                            && span.end <= text.len()
                            && text.is_char_boundary(span.start)
                            && text.is_char_boundary(span.end),
                        "{text:?}"
                    );
                    previous = span.end;
                }
            }
        }
    }
}

#[test]
fn encodes_records() {
    assert_eq!(encode(&[Span { start: 1, end: 3, kind: Kind::Type }]), [1, 0, 0, 0, 3, 0, 0, 0, 6, 0, 0, 0]);
}

fn folds(text: &str, dialect: Dialect) -> Vec<(u32, u32)> {
    fold(text, dialect).into_iter().map(|fold| (fold.start_line, fold.end_line)).collect()
}

fn labels(text: &str, dialect: Dialect) -> Vec<String> {
    let caret = text.find('|').unwrap();
    let text = text.replace('|', "");
    complete(&text, caret, dialect)
        .map(|found| found.items.into_iter().map(|item| item.label).collect())
        .unwrap_or_default()
}

fn messages(text: &str, dialect: Dialect) -> Vec<String> {
    problems(text, dialect).into_iter().map(|problem| problem.message).collect()
}

#[test]
fn folds_statements_brackets_blocks_comments_and_regions() {
    let text = "-- region Setup\n\
                -- one\n\
                -- two\n\
                CREATE TABLE users (\n\
                  id int,\n\
                  name text\n\
                );\n\
                /* a\n\
                b */\n\
                SELECT CASE\n\
                  WHEN a THEN 1\n\
                END FROM t;\n\
                -- endregion\n\
                SELECT 1;";
    let found = folds(text, Dialect::Generic);
    for expected in [(0, 12), (1, 2), (3, 6), (7, 8), (9, 11)] {
        assert!(found.contains(&expected), "{expected:?} in {found:?}");
    }
    assert!(found.windows(2).all(|pair| pair[0] <= pair[1] || pair[0].0 == pair[1].0), "{found:?}");
    assert!(found.iter().all(|(start, end)| end > start));
}

#[test]
fn folds_a_postgres_function_body_and_a_mysql_procedure() {
    let postgres = "CREATE FUNCTION f() RETURNS int AS $$\nBEGIN\n  RETURN 1;\nEND;\n$$ LANGUAGE plpgsql;\nSELECT 1;";
    let found = folds(postgres, Dialect::Postgres);
    assert!(found.contains(&(0, 4)), "{found:?}");
    let mysql = "DELIMITER $$\nCREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\n  SELECT 2;\nEND$$\nDELIMITER ;\nSELECT 3;";
    let found = folds(mysql, Dialect::MySql);
    assert!(found.contains(&(1, 5)), "the procedure is one statement: {found:?}");
    assert!(found.contains(&(2, 5)), "its BEGIN … END block: {found:?}");
}

#[test]
fn outlines_created_objects_with_columns_and_queries_by_their_target() {
    let text = "CREATE TABLE IF NOT EXISTS public.users (id bigint PRIMARY KEY, \"Name\" text, CONSTRAINT u UNIQUE (id));\n\
                CREATE OR REPLACE VIEW active AS SELECT * FROM users;\n\
                CREATE INDEX ON users (id);\n\
                WITH recent AS (SELECT 1), old (x) AS MATERIALIZED (SELECT 2) SELECT * FROM recent;\n\
                INSERT INTO orders VALUES (1);\n\
                UPDATE users SET a = 1;\n\
                DELETE FROM users WHERE id = (SELECT 1 FROM t);\n\
                SELECT count(*) FROM shop.orders o;\n\
                DROP TABLE IF EXISTS users;\n\
                USE shop;";
    let found = outline(text, Dialect::Generic);
    let names: Vec<(&str, &str)> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.kind)).collect();
    assert_eq!(
        names,
        [
            ("public.users", "class"),
            ("active", "interface"),
            ("INDEX", "key"),
            ("SELECT … FROM recent", "key"),
            ("INSERT INTO orders", "key"),
            ("UPDATE users", "key"),
            ("DELETE FROM users", "key"),
            ("SELECT … FROM shop.orders", "key"),
            ("DROP TABLE users", "class"),
            ("USE shop", "key"),
        ]
    );
    let columns: Vec<&str> = found[0].children.iter().map(|column| column.name.as_str()).collect();
    assert_eq!(columns, ["id", "\"Name\""]);
    assert_eq!(&text[found[0].children[0].start..found[0].children[0].end], "id bigint PRIMARY KEY");
    let ctes: Vec<&str> = found[3].children.iter().map(|cte| cte.name.as_str()).collect();
    assert_eq!(ctes, ["recent", "old"]);
    for symbol in &found {
        assert!(symbol.children.iter().all(|child| child.start >= symbol.start && child.end <= symbol.end));
    }
    assert!(text[found[0].start..found[0].end].ends_with(");"));
}

#[test]
fn completes_keywords_of_the_dialect_in_the_case_typed() {
    assert_eq!(labels("SELECT * FROM t ORDER |", Dialect::Generic)[0], "BY");
    assert_eq!(labels("select * from t order b|", Dialect::Generic)[0], "by");
    let start = labels("|", Dialect::Postgres);
    assert_eq!(start[0], "ALTER", "statement starters first, by name");
    assert!(start.iter().position(|label| label == "SELECT") < start.iter().position(|label| label == "AND"));
    assert!(labels("SELECT * FROM t WHERE a IL|", Dialect::Postgres).contains(&"ILIKE".to_owned()));
    assert!(!labels("SELECT * FROM t WHERE a IL|", Dialect::Sqlite).contains(&"ILIKE".to_owned()));
    assert!(labels("PRA|", Dialect::Sqlite).contains(&"PRAGMA".to_owned()));
    assert!(labels("PRA|", Dialect::Postgres).is_empty());
    assert!(labels("SELECT * FROM t WHERE a IL|", Dialect::Generic).contains(&"ILIKE".to_owned()));
}

#[test]
fn completes_types_where_a_type_goes_and_functions_with_brackets() {
    assert_eq!(labels("CREATE TABLE t (id |", Dialect::Postgres)[0], "BIGINT");
    assert_eq!(labels("SELECT x::|", Dialect::Postgres)[0], "BIGINT");
    assert_eq!(labels("SELECT CAST(x AS |", Dialect::Postgres)[0], "BIGINT");
    assert!(labels("CREATE TABLE t (id Date|", Dialect::ClickHouse).contains(&"DateTime64".to_owned()));
    let text = "SELECT coa";
    let found = complete(text, text.len(), Dialect::Generic).unwrap();
    assert_eq!(found.from, 7);
    let coalesce = found.items.iter().find(|item| item.label == "coalesce").unwrap();
    assert_eq!((coalesce.kind, coalesce.insert.as_str()), ("function", "coalesce($0)"));
}

#[test]
fn completes_nothing_inside_literals_comments_names_or_after_a_dot() {
    for text in [
        "SELECT 'sel|",
        "SELECT 'a' |x",
        "-- sel|",
        "/* sel|",
        "SELECT t.|",
        "SELECT t.na|",
        "SELECT \"sel|",
        "SELECT 12|",
        "SELECT :na|",
        "SELECT $$ sel|",
    ] {
        let caret = text.find('|').unwrap();
        let clean = text.replace('|', "");
        if text == "SELECT 'a' |x" {
            assert!(complete(&clean, caret, Dialect::Generic).is_some(), "after a closed string");
            continue;
        }
        assert_eq!(complete(&clean, caret, Dialect::Generic), None, "{text}");
    }
    assert!(complete("SELECT `sel", 11, Dialect::Postgres).is_some(), "backticks quote nothing in PostgreSQL");
}

#[test]
fn finds_syntax_problems_in_the_dialect() {
    assert_eq!(messages("SELECT 'abc FROM t;", Dialect::Generic), ["Unclosed string literal"]);
    assert_eq!(messages("SELECT 'it\\'s';", Dialect::MySql), Vec::<String>::new());
    assert_eq!(messages("SELECT 'it\\'s';", Dialect::Postgres), ["Unclosed string literal"]);
    assert_eq!(messages("SELECT 'it\\'s';", Dialect::Generic), Vec::<String>::new(), "dialects disagree");
    assert_eq!(messages("SELECT (1 + 2;\nSELECT 3);", Dialect::Postgres), ["'(' is not closed", "Unmatched ')'"]);
    assert_eq!(messages("SELECT CASE WHEN a THEN 1 FROM t", Dialect::Postgres), ["CASE without END"]);
    assert_eq!(messages("SELECT a, FROM t;", Dialect::Postgres), ["Unexpected ',' before FROM"]);
    assert_eq!(messages("SELECT a, FROM t;", Dialect::ClickHouse), Vec::<String>::new());
    let generic = problems("INSERT INTO t (a,) VALUES (1);", Dialect::Generic);
    assert_eq!(generic.len(), 1);
    assert!(!generic[0].error, "a warning without a dialect");
    assert_eq!(messages("/* open", Dialect::Generic), ["Unclosed comment"]);
    assert_eq!(messages("SELECT $body$ x", Dialect::Postgres), ["Unclosed dollar-quoted string"]);
    assert_eq!(messages("SELECT \"a", Dialect::Postgres), ["Unclosed quoted name"]);
    assert_eq!(messages("SELECT 1 # (", Dialect::MySql), Vec::<String>::new(), "a MySQL comment");
    assert_eq!(messages("SELECT a[1] FROM t;", Dialect::Postgres), Vec::<String>::new());
    assert_eq!(messages("SELECT [a] FROM t;", Dialect::SqlServer), Vec::<String>::new());
}

#[test]
fn reads_procedures_scripts_and_client_commands_without_false_problems() {
    let postgres = "CREATE FUNCTION f() RETURNS int AS $$\nBEGIN\n  IF x THEN RETURN 'a''s'; END IF;\nEND;\n$$ LANGUAGE plpgsql;\nBEGIN;\nCOMMIT;\n\\set x 1\n";
    assert_eq!(messages(postgres, Dialect::Postgres), Vec::<String>::new());
    let mysql = "DELIMITER $$\nCREATE PROCEDURE p()\nBEGIN\n  CASE WHEN 1 THEN SELECT 1; END CASE;\n  WHILE x DO SET x = x - 1; END WHILE;\nEND$$\nDELIMITER ;\nSELECT \"a\\\"b\";";
    assert_eq!(messages(mysql, Dialect::MySql), Vec::<String>::new());
    assert_eq!(outline(mysql, Dialect::MySql).len(), 2);
    let sqlserver =
        "SELECT TOP 1 [order] FROM t\nGO\nBEGIN TRY\n  SELECT 1;\nEND TRY\nBEGIN CATCH\n  SELECT 2;\nEND CATCH\nGO 2\n";
    assert_eq!(messages(sqlserver, Dialect::SqlServer), Vec::<String>::new());
    let sqlite = "CREATE TRIGGER t AFTER INSERT ON a BEGIN\n  UPDATE b SET c = 1;\n  DELETE FROM d;\nEND;\nSELECT 1;";
    assert_eq!(outline(sqlite, Dialect::Sqlite).len(), 2);
    assert!(folds(sqlite, Dialect::Sqlite).contains(&(0, 3)));
}

#[test]
fn the_dialect_is_set_by_name_for_the_calls_that_follow() {
    assert!(dialect_request(b"mariadb").is_empty());
    assert_eq!(current_dialect(), Dialect::MySql);
    assert_eq!(problems_request(b"SELECT 'it\\'s';"), b"[]");
    dialect_request(b"postgres");
    assert_eq!(current_dialect(), Dialect::Postgres);
    assert!(String::from_utf8(problems_request(b"SELECT 'it\\'s';")).unwrap().contains("Unclosed string literal"));
    dialect_request(b"");
    assert_eq!(current_dialect(), Dialect::Generic);
    let mut input = 3u32.to_le_bytes().to_vec();
    input.extend_from_slice(b"sel");
    let answer = String::from_utf8(complete_request(&input)).unwrap();
    assert!(
        answer.starts_with("{\"from\":0,\"items\":[{\"label\":\"select\",\"kind\":\"keyword\",\"insert\":\"select\"}"),
        "{answer}"
    );
}

#[test]
fn every_feature_survives_any_input() {
    let pieces = [
        "'",
        "\"",
        "`",
        "[",
        "]",
        "$",
        "$$",
        "$a$",
        "--",
        "/*",
        "*/",
        "#",
        "\n",
        "1",
        ".",
        "e",
        "(",
        ")",
        ":",
        "é",
        "日",
        "\\",
        ";",
        ",",
        "BEGIN",
        "CASE",
        "END",
        "\nDELIMITER $$\n",
        "\nGO\n",
        "CREATE TABLE t",
        "WITH a AS",
        " FROM ",
    ];
    let dialects =
        [Dialect::Generic, Dialect::Postgres, Dialect::MySql, Dialect::Sqlite, Dialect::ClickHouse, Dialect::SqlServer];
    let mut round = 0;
    for a in pieces {
        for b in pieces {
            for c in pieces {
                let text = format!("{a}{b}{c}");
                round += 1;
                {
                    let dialect = dialects[round % dialects.len()];
                    let lines = text.split('\n').count() as u32;
                    for fold in fold(&text, dialect) {
                        assert!(fold.end_line > fold.start_line && fold.end_line < lines, "{text:?}");
                    }
                    fn check(text: &str, symbols: &[Symbol], parent: std::ops::Range<usize>) {
                        for symbol in symbols {
                            assert!(!symbol.name.is_empty(), "{text:?}");
                            assert!(
                                symbol.start >= parent.start && symbol.end <= parent.end && symbol.start < symbol.end,
                                "{text:?}"
                            );
                            assert!(
                                text.is_char_boundary(symbol.start) && text.is_char_boundary(symbol.end),
                                "{text:?}"
                            );
                            check(text, &symbol.children, symbol.start..symbol.end);
                        }
                    }
                    check(&text, &outline(&text, dialect), 0..text.len());
                    for problem in problems(&text, dialect) {
                        assert!(problem.start < problem.end && problem.end <= text.len(), "{text:?}");
                        assert!(text.is_char_boundary(problem.start) && text.is_char_boundary(problem.end), "{text:?}");
                    }
                    let carets = [0, text.len() / 2, text.len()];
                    for caret in carets.into_iter().filter(|at| text.is_char_boundary(*at)) {
                        if let Some(found) = complete(&text, caret, dialect) {
                            assert!(found.from <= caret && text.is_char_boundary(found.from), "{text:?}");
                        }
                    }
                }
            }
        }
    }
}

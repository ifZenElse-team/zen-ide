use super::*;

fn kinds(code: &str) -> Vec<(&str, HighlightKind)> {
    highlight(code, SqlDialect::Postgres).into_iter().map(|span| (&code[span.range], span.kind)).collect()
}

fn literal_of(text: &str) -> Literal {
    let start = text.find(['"', '`']).unwrap();
    let end = text.rfind(['"', '`']).unwrap() + 1;
    Literal { range: start..end, content: start + 1..end - 1, quote: text.as_bytes()[start], escapes: true }
}

#[test]
fn statements_read_as_sql() {
    for code in [
        "SELECT id, name FROM users WHERE id = $1",
        "SELECT 1",
        "SELECT *",
        "  select id from users",
        "INSERT INTO t (a) VALUES (?)",
        "insert or replace into t values (1)",
        "UPDATE users SET name = :name",
        "update users set name = ?",
        "DELETE FROM sessions WHERE expires < now()",
        "WITH recent AS (SELECT * FROM o) SELECT * FROM recent",
        "CREATE TABLE IF NOT EXISTS t (id int)",
        "create unique index i on t (a)",
        "DROP VIEW v",
        "ALTER TABLE t ADD COLUMN c int",
        "TRUNCATE orders",
        "GRANT SELECT ON t TO reader",
        "EXPLAIN ANALYZE SELECT 1",
        "(SELECT 1) UNION (SELECT 2)",
        "MERGE INTO t USING s ON t.id = s.id",
    ] {
        assert!(looks_like_sql(code), "{code}");
    }
}

#[test]
fn words_that_only_start_like_sql_do_not() {
    for code in [
        "",
        "DELETE",
        "UPDATE",
        "SELECT",
        "Select a file",
        "Select a file from the list",
        "select an item",
        "Update available",
        "update the list",
        "delete it",
        "Create a new project",
        "create a table",
        "with love",
        "drop it",
        "insert coin",
        "Truncated",
        "selected",
        "SELECTED rows",
        "explain this",
    ] {
        assert!(!looks_like_sql(code), "{code}");
    }
}

#[test]
fn directives_and_tags_decide_before_the_text() {
    let injector = SqlStrings::new(None);
    let marked = "// language=SQL\nq := \"pick everything\"";
    assert!(injector.accepts(marked, &literal_of(marked), "pick everything"));
    let same_line = "q := /* language=PostgreSQL */ \"pick everything\"";
    assert!(injector.accepts(same_line, &literal_of(same_line), "pick everything"));
    let other = "// language=JSON\nq := \"SELECT 1\"";
    assert!(!injector.accepts(other, &literal_of(other), "SELECT 1"), "another language wins over the text");
    let far = "// language=SQL\n\nx := 1\nq := \"pick everything\"";
    assert!(!injector.accepts(far, &literal_of(far), "pick everything"), "only the line before counts");
    let tagged_template = "const rows = await sql`pick ${id}`";
    assert!(injector.accepts(tagged_template, &literal_of(tagged_template), "pick ${id}"));
    let generic = "db.sql<User>`pick`";
    assert!(injector.accepts(generic, &literal_of(generic), "pick"));
    let other_tag = "html`<b>`";
    assert!(!injector.accepts(other_tag, &literal_of(other_tag), "<b>"));
    let plain = "q := \"SELECT 1\"";
    assert!(injector.accepts(plain, &literal_of(plain), "SELECT 1"));
}

#[test]
fn may_hold_is_false_for_text_without_sql_words() {
    let injector = SqlStrings::new(None);
    assert!(!injector.may_hold("fn main() { let x = 1; println!(\"{x}\"); }"));
    assert!(injector.may_hold("q := \"SELECT 1\""));
    assert!(injector.may_hold("// language=sql"));
    assert!(injector.may_hold("sql`x`"));
}

#[test]
fn files_of_code_languages_are_hosts() {
    for name in ["main.go", "lib.rs", "app.py", "index.ts", "view.tsx", "a.mjs", "Repo.java", "Dao.kt"] {
        assert!(hosts(name), "{name}");
    }
    for name in ["query.sql", "README.md", "Cargo.toml", "go"] {
        assert!(!hosts(name), "{name}");
    }
}

#[test]
fn own_highlighting_marks_keywords_functions_literals_and_parameters() {
    assert_eq!(
        kinds("SELECT count(*), 'a' -- n\nFROM t WHERE id = $1 AND x = :name AND y = ? AND z::int > 2"),
        [
            ("SELECT", HighlightKind::Keyword),
            ("count", HighlightKind::Function),
            ("'a'", HighlightKind::String),
            ("-- n", HighlightKind::Comment),
            ("FROM", HighlightKind::Keyword),
            ("WHERE", HighlightKind::Keyword),
            ("$", HighlightKind::Constant),
            ("1", HighlightKind::Constant),
            ("AND", HighlightKind::Keyword),
            (":", HighlightKind::Constant),
            ("name", HighlightKind::Constant),
            ("AND", HighlightKind::Keyword),
            ("?", HighlightKind::Constant),
            ("AND", HighlightKind::Keyword),
            ("2", HighlightKind::Number),
        ]
    );
    assert_eq!(kinds("select `a` from b")[0], ("select", HighlightKind::Keyword));
}

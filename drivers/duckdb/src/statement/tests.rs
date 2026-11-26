use super::*;

fn position(failure: &Failure) -> u64 {
    failure.data.as_ref().and_then(|data| data["position"].as_u64()).unwrap_or_default()
}

#[test]
fn one_statement_is_read_with_its_verb_and_without_its_final_semicolon() {
    let checked = check("  select 'a;b', \"c\"\"d\" -- x;\n /* ; */ from t ;  ").unwrap();
    assert_eq!(checked.verb, "SELECT");
    assert_eq!(checked.text, "  select 'a;b', \"c\"\"d\" -- x;\n /* ; */ from t");
    assert!(checked.describable() && checked.reads());
    let insert = check("INSERT INTO t VALUES ($$it's$$)").unwrap();
    assert_eq!(insert.verb, "INSERT");
    assert!(!insert.reads());
}

#[test]
fn dot_commands_second_statements_and_open_quotes_are_refused() {
    assert_eq!(position(&check(".shell ls").unwrap_err()), 1);
    assert_eq!(position(&check("SELECT 1;\n.shell ls").unwrap_err()), 11);
    assert_eq!(position(&check("SELECT 1; SELECT 2").unwrap_err()), 9);
    assert_eq!(position(&check("SELECT 'open").unwrap_err()), 8);
    assert_eq!(position(&check("SELECT /* open").unwrap_err()), 8);
    assert!(check("  ;  ").is_err());
    assert!(check("-- only a comment").is_err());
}

#[test]
fn lines_the_duckdb_program_would_read_as_its_own_are_refused() {
    assert_eq!(position(&check("SELECT 1\n   .shell ls").unwrap_err()), 13);
    assert_eq!(position(&check("SELECT 1 -- c\n\t.system ls").unwrap_err()), 16);
    assert_eq!(position(&check("SELECT 1\r\n.shell ls").unwrap_err()), 11);
    assert_eq!(position(&check("SELECT 1\nGO\n.shell ls").unwrap_err()), 10);
    assert_eq!(position(&check("SELECT 1\n  go  -- end\nSELECT 2").unwrap_err()), 12);
    assert_eq!(position(&check("SELECT 1\n/\n.shell ls").unwrap_err()), 10);
    assert_eq!(position(&check("SELECT 1\n/ /* c */").unwrap_err()), 10);
    assert!(check("SELECT $$\n;\n.shell ls\n$$").is_ok_and(|checked| checked.verb == "SELECT"));
    assert!(check("SELECT 'a\n.shell ls'").is_ok());
    assert!(check("SELECT 1 /* x\n.shell ls */").is_ok());
    assert!(check("SELECT 4\n/2 AS half").is_ok());
    assert!(check("SELECT 1 AS\ngoal").is_ok());
    assert!(check("SELECT 1\n/* note */").is_ok());
}

#[test]
fn names_and_text_are_quoted() {
    assert_eq!(quote("a\"b"), "\"a\"\"b\"");
    assert_eq!(literal("it's"), "'it''s'");
}

use super::{Token, tokenize};
use crate::sql::{Dialect, strip_in};

const FIRST: &[&str] = &["install", "load", "force", "attach", "copy", "export", "import"];

const SETTINGS: &[&str] = &[
    "enable_external_access",
    "allowed_paths",
    "allowed_directories",
    "lock_configuration",
    "secret_directory",
    "home_directory",
    "temp_directory",
    "enable_http_metadata_cache",
    "http_proxy",
];

const CALLS: &[&str] = &[
    "glob",
    "sniff_csv",
    "getenv",
    "query",
    "query_table",
    "load_extension",
    "json_execute_serialized_sql",
    "postgres_query",
    "postgres_execute",
    "mysql_query",
    "mysql_execute",
    "st_read",
    "st_read_meta",
    "st_drivers",
];

const CALL_PREFIXES: &[&str] = &["read_", "parquet_", "iceberg_", "delta_", "ducklake_", "sqlite_", "http_"];

const TABLE_BEFORE: &[&str] = &["from", "join", "summarize", "describe", "show", "pivot", "unpivot", "table"];

const FROM_ENDS: &[&str] = &[
    "where",
    "group",
    "having",
    "order",
    "limit",
    "offset",
    "select",
    "union",
    "except",
    "intersect",
    "qualify",
    "window",
    "on",
    "using",
    "returning",
    "set",
    "values",
];

fn name(token: &Token) -> Option<String> {
    match token {
        Token::Word { lower, .. } => Some(lower.clone()),
        Token::Quoted(raw) if raw.starts_with('"') => {
            Some(raw.trim_start_matches('"').trim_end_matches('"').replace("\"\"", "\"").to_lowercase())
        }
        _ => None,
    }
}

fn outside_call(name: &str) -> bool {
    CALLS.contains(&name)
        || name.ends_with("_scan")
        || name.ends_with("_attach")
        || CALL_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

pub fn reaches_outside(statement: &str) -> bool {
    let tokens = tokenize(&strip_in(statement, Dialect::Postgres));
    let words: Vec<Option<String>> = tokens.iter().map(name).collect();
    let first = words.first().cloned().flatten().unwrap_or_default();
    if FIRST.contains(&first.as_str()) {
        return true;
    }
    if first == "update" && words.get(1).cloned().flatten().as_deref() == Some("extensions") {
        return true;
    }
    if first == "create" && words.iter().take(6).flatten().any(|word| word == "secret") {
        return true;
    }
    if matches!(first.as_str(), "set" | "reset" | "pragma")
        && words.iter().flatten().any(|word| word.contains("extension") || SETTINGS.contains(&word.as_str()))
    {
        return true;
    }
    let mut in_from = false;
    for (at, token) in tokens.iter().enumerate() {
        let next = tokens.get(at + 1);
        if next == Some(&Token::Punct('(')) && words[at].as_deref().is_some_and(outside_call) {
            return true;
        }
        if let Some(word) = &words[at] {
            if TABLE_BEFORE.contains(&word.as_str()) {
                in_from = true;
                if next == Some(&Token::Literal) {
                    return true;
                }
                continue;
            }
            if FROM_ENDS.contains(&word.as_str()) {
                in_from = false;
            }
        }
        if in_from && *token == Token::Punct(',') && next == Some(&Token::Literal) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::reaches_outside;

    #[test]
    fn files_extensions_and_other_servers_reach_outside() {
        for statement in [
            "SELECT * FROM read_csv('orders.csv')",
            "select * from READ_PARQUET(['a.parquet'])",
            "SELECT * FROM 'orders.parquet'",
            "FROM '/etc/passwd'",
            "SELECT * FROM t JOIN 'x.csv' USING (id)",
            "SELECT * FROM t, 'x.csv'",
            "SUMMARIZE 'x.parquet'",
            "DESCRIBE 'x.json'",
            "SELECT * FROM \"read_csv\"('x')",
            "SELECT * FROM main.read_text('x')",
            "SELECT * FROM glob('*')",
            "SELECT * FROM sqlite_scan('a.db', 't')",
            "SELECT * FROM postgres_scan('host=x', 'public', 't')",
            "SELECT * FROM query('SELECT 1')",
            "SELECT getenv('HOME')",
            "COPY t TO 'out.csv'",
            "COPY t FROM 'in.csv'",
            "EXPORT DATABASE 'dir'",
            "IMPORT DATABASE 'dir'",
            "ATTACH 'other.duckdb' AS other",
            "INSTALL httpfs",
            "FORCE INSTALL spatial FROM community",
            "LOAD spatial",
            "UPDATE EXTENSIONS",
            "SET custom_extension_repository = 'http://x'",
            "SET GLOBAL autoinstall_known_extensions = true",
            "RESET autoload_known_extensions",
            "SET enable_external_access = true",
            "CREATE SECRET (TYPE s3, KEY_ID 'k')",
            "CREATE OR REPLACE PERSISTENT SECRET s (TYPE http)",
            "CREATE MACRO m() AS TABLE SELECT * FROM read_json('x')",
            "WITH f AS (SELECT * FROM 'x.csv') SELECT * FROM f",
        ] {
            assert!(reaches_outside(statement), "{statement}");
        }
    }

    #[test]
    fn statements_on_the_database_stay_inside() {
        for statement in [
            "SELECT * FROM orders WHERE note = 'read_csv(x)'",
            "SELECT 'a', 'b' FROM t",
            "SELECT * FROM t WHERE a IN ('x', 'y')",
            "SELECT $$read_csv('x')$$",
            "SELECT 1 -- FROM 'x.csv'",
            "INSERT INTO t VALUES ('a', 'b')",
            "UPDATE t SET reader = 'x'",
            "SET search_path = 'shop'",
            "CREATE TABLE copy_of AS SELECT * FROM t",
            "SELECT count(*) FROM range(10)",
            "DETACH other",
            "SHOW TABLES",
            "DESCRIBE orders",
        ] {
            assert!(!reaches_outside(statement), "{statement}");
        }
    }
}

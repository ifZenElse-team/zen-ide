use crate::lexer::{dollar_tag, number_length};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    Type = 6,
    Function = 7,
    Constant = 10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

const KEYWORDS: &[&str] = &[
    "ABORT",
    "ACTION",
    "ADD",
    "AFTER",
    "ALL",
    "ALTER",
    "ALWAYS",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ATTACH",
    "AUTHORIZATION",
    "AUTOINCREMENT",
    "AUTO_INCREMENT",
    "BEFORE",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CALL",
    "CASCADE",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "COMMENT",
    "COMMIT",
    "CONCURRENTLY",
    "CONFLICT",
    "CONSTRAINT",
    "CONTINUE",
    "COPY",
    "CREATE",
    "CROSS",
    "CURRENT",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "CURSOR",
    "DATABASE",
    "DECLARE",
    "DEFAULT",
    "DEFERRABLE",
    "DEFERRED",
    "DELETE",
    "DESC",
    "DETACH",
    "DISTINCT",
    "DO",
    "DOMAIN",
    "DROP",
    "EACH",
    "ELSE",
    "ELSIF",
    "END",
    "ENGINE",
    "ENUM",
    "ESCAPE",
    "EXCEPT",
    "EXCEPTION",
    "EXCLUDE",
    "EXECUTE",
    "EXISTS",
    "EXPLAIN",
    "EXTENSION",
    "FALSE",
    "FETCH",
    "FILTER",
    "FIRST",
    "FOLLOWING",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "FUNCTION",
    "GENERATED",
    "GLOB",
    "GRANT",
    "GROUP",
    "GROUPS",
    "HAVING",
    "IF",
    "IGNORE",
    "ILIKE",
    "IMMEDIATE",
    "IN",
    "INDEX",
    "INHERITS",
    "INITIALLY",
    "INNER",
    "INSERT",
    "INSTEAD",
    "INTERSECT",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "KEY",
    "LANGUAGE",
    "LAST",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "LOCK",
    "LOOP",
    "MATERIALIZED",
    "MATCH",
    "MERGE",
    "NATURAL",
    "NO",
    "NOT",
    "NOTHING",
    "NOTNULL",
    "NULL",
    "NULLS",
    "OF",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "OTHERS",
    "OUTER",
    "OVER",
    "OWNER",
    "PARTITION",
    "PERFORM",
    "PRAGMA",
    "PRECEDING",
    "PRIMARY",
    "PROCEDURE",
    "RAISE",
    "RANGE",
    "RECURSIVE",
    "REFERENCES",
    "REFRESH",
    "REINDEX",
    "RELEASE",
    "RENAME",
    "REPLACE",
    "RESTRICT",
    "RETURN",
    "RETURNING",
    "RETURNS",
    "REVOKE",
    "RIGHT",
    "ROLE",
    "ROLLBACK",
    "ROW",
    "ROWS",
    "SAVEPOINT",
    "SCHEMA",
    "SELECT",
    "SEQUENCE",
    "SET",
    "SHOW",
    "SIMILAR",
    "SOME",
    "TABLE",
    "TABLESPACE",
    "TEMP",
    "TEMPORARY",
    "THEN",
    "TIES",
    "TO",
    "TRANSACTION",
    "TRIGGER",
    "TRUE",
    "TRUNCATE",
    "TYPE",
    "UNBOUNDED",
    "UNION",
    "UNIQUE",
    "UNLOGGED",
    "UPDATE",
    "USE",
    "USING",
    "VACUUM",
    "VALUES",
    "VIEW",
    "VOLATILE",
    "STABLE",
    "IMMUTABLE",
    "WHEN",
    "WHERE",
    "WHILE",
    "WINDOW",
    "WITH",
    "WITHOUT",
    "ZONE",
    "STRICT",
    "SECURITY",
    "DEFINER",
    "INVOKER",
    "LOCAL",
    "GLOBAL",
    "SESSION",
    "SHARE",
    "NOWAIT",
    "SKIP",
    "LOCKED",
    "UPSERT",
    "DUPLICATE",
];

const TYPES: &[&str] = &[
    "BIGINT",
    "BIGSERIAL",
    "BINARY",
    "BIT",
    "BLOB",
    "BOOL",
    "BOOLEAN",
    "BYTEA",
    "CHAR",
    "CHARACTER",
    "CIDR",
    "CLOB",
    "DATE",
    "DATETIME",
    "DEC",
    "DECIMAL",
    "DOUBLE",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
    "INET",
    "INT",
    "INT2",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERVAL",
    "JSON",
    "JSONB",
    "LONGTEXT",
    "MACADDR",
    "MEDIUMINT",
    "MEDIUMTEXT",
    "MONEY",
    "NCHAR",
    "NUMERIC",
    "NVARCHAR",
    "PRECISION",
    "REAL",
    "SERIAL",
    "SMALLINT",
    "SMALLSERIAL",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "TIMETZ",
    "TINYINT",
    "TINYTEXT",
    "UUID",
    "VARBINARY",
    "VARCHAR",
    "VARYING",
    "XML",
    "YEAR",
];

pub fn highlight(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut at = 0;
    let push = |spans: &mut Vec<Span>, start: usize, end: usize, kind: Kind| {
        if end > start {
            spans.push(Span { start, end, kind });
        }
    };
    while at < bytes.len() {
        let rest = &text[at..];
        let c = bytes[at];
        if rest.starts_with("--") || c == b'#' {
            let end = rest.find('\n').map_or(text.len(), |newline| at + newline);
            push(&mut spans, at, end, Kind::Comment);
            at = end;
            continue;
        }
        if let Some(body) = rest.strip_prefix("/*") {
            let end = body.find("*/").map_or(text.len(), |close| at + 2 + close + 2);
            push(&mut spans, at, end, Kind::Comment);
            at = end;
            continue;
        }
        if c == b'\'' || ((c == b'E' || c == b'e') && bytes.get(at + 1) == Some(&b'\'') && !preceded_by_word(bytes, at))
        {
            let quote = if c == b'\'' { at } else { at + 1 };
            let backslashes = quote != at;
            let mut index = quote + 1;
            let end = loop {
                match bytes.get(index) {
                    None => break text.len(),
                    Some(b'\\') if backslashes => index += 2,
                    Some(b'\'') if bytes.get(index + 1) == Some(&b'\'') => index += 2,
                    Some(b'\'') => break index + 1,
                    Some(_) => index += 1,
                }
            };
            let end = end.min(text.len());
            push(&mut spans, at, end, Kind::String);
            at = end;
            continue;
        }
        if c == b'$' {
            if let Some(tag_end) = dollar_tag(rest) {
                let tag = &rest[..tag_end];
                let end = rest[tag_end..].find(tag).map_or(text.len(), |close| at + tag_end + close + tag_end);
                push(&mut spans, at, end, Kind::String);
                at = end;
                continue;
            }
            let digits = rest[1..].bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                push(&mut spans, at, at + 1 + digits, Kind::Constant);
                at += 1 + digits;
                continue;
            }
        }
        if (c == b':'
            && bytes.get(at + 1).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            && (at == 0 || bytes[at - 1] != b':'))
            || c == b'@'
        {
            let length =
                1 + rest[1..].bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'@').count();
            push(&mut spans, at, at + length, Kind::Constant);
            at += length;
            continue;
        }
        if c == b'?' {
            push(&mut spans, at, at + 1, Kind::Constant);
            at += 1;
            continue;
        }
        if c == b'"' || c == b'`' {
            let end = rest[1..].find(c as char).map_or(text.len(), |close| at + 1 + close + 1);
            at = end;
            continue;
        }
        if (c.is_ascii_digit() || (c == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)))
            && !preceded_by_word(bytes, at)
        {
            let length = number_length(rest);
            push(&mut spans, at, at + length, Kind::Number);
            at += length;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let length = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'$').count();
            let word = &rest[..length];
            let upper = word.to_ascii_uppercase();
            let after = rest[length..].trim_start_matches([' ', '\t']);
            let qualified = at > 0 && bytes[at - 1] == b'.';
            if after.starts_with('(')
                && !KEYWORDS.contains(&upper.as_str())
                && !TYPES.contains(&upper.as_str())
                && !names_a_relation(&text[..at])
            {
                push(&mut spans, at, at + length, Kind::Function);
            } else if !qualified && (TYPES.contains(&upper.as_str()) || KEYWORDS.contains(&upper.as_str())) {
                push(&mut spans, at, at + length, Kind::Keyword);
            }
            at += length;
            continue;
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    spans
}

fn names_a_relation(before: &str) -> bool {
    let before = before.trim_end();
    let word: String = before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    let word: String = word.chars().rev().collect::<String>().to_ascii_uppercase();
    if before.ends_with('.') {
        return true;
    }
    matches!(
        word.as_str(),
        "TABLE" | "INTO" | "INDEX" | "VIEW" | "REFERENCES" | "TYPE" | "ON" | "EXISTS" | "JOIN" | "UPDATE"
    )
}

fn preceded_by_word(bytes: &[u8], at: usize) -> bool {
    at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_')
}

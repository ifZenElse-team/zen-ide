use crate::driver::{DbError, ErrorKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Prepared {
    pub(super) sql: String,
    pub(super) words: Vec<String>,
}

fn refuse(detail: &str) -> DbError {
    DbError::new(ErrorKind::Server, detail)
}

fn closing(bytes: &[u8], from: usize, quote: u8) -> Option<usize> {
    let mut at = from;
    while at < bytes.len() {
        if bytes[at] == quote {
            if quote != b']' && bytes.get(at + 1) == Some(&quote) {
                at += 2;
                continue;
            }
            return Some(at);
        }
        at += 1;
    }
    None
}

fn ends_or_escapes(line: &str) -> bool {
    if line.starts_with('.') {
        return true;
    }
    let line = line.trim_start();
    let rest = if let Some(rest) = line.strip_prefix('/') {
        rest
    } else if line.get(..2).is_some_and(|word| word.eq_ignore_ascii_case("go")) {
        &line[2..]
    } else {
        return false;
    };
    let rest = rest.trim_start();
    rest.is_empty() || rest.starts_with("--") || rest.starts_with("/*")
}

pub(super) fn prepare(text: &str) -> Result<Prepared, DbError> {
    if text.contains('\0') {
        return Err(refuse("the statement holds a NUL character"));
    }
    let bytes = text.as_bytes();
    let mut words: Vec<String> = Vec::new();
    let mut depth = 0_usize;
    let mut opened = false;
    let mut first: Option<usize> = None;
    let mut end: Option<usize> = None;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if first.is_some() && bytes[at - 1] == b'\n' && ends_or_escapes(text[at..].split('\n').next().unwrap_or("")) {
            return Err(DbError::new(
                ErrorKind::Unsupported,
                "a line of only / or GO, or one starting with a dot, is a command of sqlite3 and is not run",
            ));
        }
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if text[at..].starts_with("--") {
            at = text[at..].find('\n').map_or(bytes.len(), |line| at + line + 1);
            continue;
        }
        if text[at..].starts_with("/*") {
            at = text[at + 2..]
                .find("*/")
                .map(|close| at + 2 + close + 2)
                .ok_or_else(|| refuse("unterminated comment"))?;
            continue;
        }
        if end.is_some() {
            return Err(refuse("one statement at a time"));
        }
        if first.is_none() {
            if byte == b'.' {
                return Err(DbError::new(ErrorKind::Unsupported, "dot-commands of sqlite3 are not run"));
            }
            first = Some(at);
        }
        match byte {
            b'\'' | b'"' | b'`' | b'[' => {
                let quote = if byte == b'[' { b']' } else { byte };
                at = closing(bytes, at + 1, quote).ok_or_else(|| refuse("unterminated quote"))? + 1;
            }
            b';' => {
                let trigger = matches!(
                    words.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
                    ["create", "trigger", ..] | ["create", "temp" | "temporary", "trigger", ..]
                );
                if !trigger || (opened && depth == 0) {
                    end = Some(at);
                }
                at += 1;
            }
            _ if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80 => {
                let start = at;
                while at < bytes.len()
                    && (bytes[at].is_ascii_alphanumeric()
                        || bytes[at] == b'_'
                        || bytes[at] == b'$'
                        || bytes[at] >= 0x80)
                {
                    at += 1;
                }
                let word = text[start..at].to_ascii_lowercase();
                match word.as_str() {
                    "begin" | "case" => {
                        depth += 1;
                        opened = true;
                    }
                    "end" => depth = depth.saturating_sub(1),
                    _ => {}
                }
                if words.len() < 3 {
                    words.push(word);
                }
            }
            _ => {
                at += 1;
            }
        }
    }
    let Some(first) = first else { return Ok(Prepared { sql: String::new(), words }) };
    let trigger = matches!(
        words.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
        ["create", "trigger", ..] | ["create", "temp" | "temporary", "trigger", ..]
    );
    if words.first().map(String::as_str) == Some("attach")
        || (words.first().map(String::as_str) == Some("vacuum") && words.iter().any(|word| word == "into"))
    {
        return Err(DbError::new(ErrorKind::Unsupported, "ATTACH and VACUUM INTO open files outside the database"));
    }
    if trigger && !(opened && depth == 0) {
        return Err(refuse("a trigger ends with END"));
    }
    let sql = text[first..end.unwrap_or(bytes.len())].trim_end().to_owned();
    Ok(Prepared { sql, words })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_statement_passes_without_its_semicolon() {
        let prepared = prepare("  -- note\n SELECT 'a;b', \"c;\" FROM t; -- done\n").unwrap();
        assert_eq!(prepared.sql, "SELECT 'a;b', \"c;\" FROM t");
        assert_eq!(prepared.words, ["select", "from", "t"].map(str::to_owned));
        assert_eq!(prepare("/* only */ -- comments").unwrap().sql, "");
        let trigger = "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET x = 1; DELETE FROM c; END;";
        assert_eq!(prepare(trigger).unwrap().sql, trigger.trim_end_matches(';'));
        assert!(prepare("create temp trigger t after insert on a begin select 1; end").is_ok());
    }

    #[test]
    fn what_could_run_something_else_is_refused() {
        for text in [
            "SELECT 1; SELECT 2",
            "SELECT 1;\n.shell ls",
            ".shell ls",
            "-- c\n.output /tmp/x",
            "SELECT 'open",
            "SELECT 1 /* open",
            "CREATE TRIGGER t AFTER INSERT ON a BEGIN SELECT 1;",
            "SELECT [open",
            "SELECT 1\0",
        ] {
            assert!(prepare(text).is_err(), "{text}");
        }
        assert_eq!(prepare(".tables").unwrap_err().kind, ErrorKind::Unsupported);
    }

    #[test]
    fn a_line_sqlite3_ends_a_statement_on_or_runs_as_a_dot_command_is_refused() {
        for text in [
            "SELECT 1\n/\n.shell touch /tmp/x",
            "SELECT 1\n  /  \n.system touch /tmp/x",
            "SELECT 1\nGO\n.shell touch /tmp/x",
            "SELECT 1\ngo -- done\n.shell touch /tmp/x",
            "SELECT 1\n/ /* done */\n.system touch /tmp/x",
            "SELECT 1 -- note\n/\nSELECT 2",
            "SELECT 1\r\nGo\r\n",
            "SELECT 1\n/",
            "SELECT\n.shell touch /tmp/x",
        ] {
            assert!(prepare(text).is_err(), "{text:?}");
        }
        for text in [
            "SELECT 4\n/ 2",
            "SELECT 'a\n/\ngo\n.shell b'",
            "SELECT 1 /*\n/\nGO\n*/",
            "SELECT 1\n/* note */ FROM t",
            "SELECT 1 AS\ngoal",
            "SELECT\n  .5",
        ] {
            assert!(prepare(text).is_ok(), "{text:?}");
        }
    }

    #[test]
    fn statements_that_open_other_files_are_refused() {
        for text in [
            "ATTACH '/tmp/other.db' AS other",
            "attach database ':memory:' as m",
            "VACUUM INTO '/tmp/copy.db'",
            "vacuum main into '/tmp/copy.db'",
        ] {
            assert_eq!(prepare(text).map_err(|error| error.kind), Err(ErrorKind::Unsupported), "{text}");
        }
        assert!(prepare("VACUUM").is_ok());
        assert!(prepare("DETACH other").is_ok());
    }
}

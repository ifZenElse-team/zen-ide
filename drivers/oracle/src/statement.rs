use zen_db_driver_kit::Failure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Query,
    Change,
    Definition,
    Block,
    Commit,
    Rollback,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) text: String,
    pub(crate) verb: String,
    pub(crate) kind: Kind,
}

impl Checked {
    pub(crate) fn reads(&self) -> bool {
        self.kind == Kind::Query && !self.text.to_ascii_uppercase().contains("FOR UPDATE")
    }
}

const DEFINITIONS: &[&str] = &[
    "CREATE",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "RENAME",
    "COMMENT",
    "GRANT",
    "REVOKE",
    "ANALYZE",
    "AUDIT",
    "NOAUDIT",
    "PURGE",
    "FLASHBACK",
    "ASSOCIATE",
    "DISASSOCIATE",
];

const CHANGES: &[&str] = &["INSERT", "UPDATE", "DELETE", "MERGE", "LOCK"];

const OTHERS: &[&str] = &["SAVEPOINT", "EXPLAIN", "CALL"];

fn words(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() && found.len() < 6 {
        let c = chars[at];
        if c == '-' && chars.get(at + 1) == Some(&'-') {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if c == '/' && chars.get(at + 1) == Some(&'*') {
            at += 2;
            while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                at += 1;
            }
            at += 2;
        } else if c.is_alphabetic() {
            let start = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '$' | '#')) {
                at += 1;
            }
            found.push(chars[start..at].iter().collect::<String>().to_ascii_uppercase());
        } else if c.is_whitespace() {
            at += 1;
        } else {
            found.push(c.to_string());
            break;
        }
    }
    found
}

fn is_block(words: &[String]) -> bool {
    let mut rest = words.iter().map(String::as_str);
    match rest.next() {
        Some("DECLARE" | "BEGIN") => true,
        Some("CREATE") => {
            for word in rest {
                match word {
                    "OR" | "REPLACE" | "EDITIONABLE" | "NONEDITIONABLE" | "AND" | "COMPILE" | "RESOLVE" | "NOFORCE" => {
                    }
                    "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE" | "LIBRARY" | "JAVA" => return true,
                    _ => return false,
                }
            }
            false
        }
        _ => false,
    }
}

fn kind_of(words: &[String]) -> Option<Kind> {
    let first = words.first()?.as_str();
    let second = words.get(1).map(String::as_str);
    if is_block(words) {
        return Some(Kind::Block);
    }
    Some(match first {
        "SELECT" | "WITH" | "(" => Kind::Query,
        "COMMIT" => Kind::Commit,
        "ROLLBACK" if words.iter().any(|word| word == "TO") => Kind::Other,
        "ROLLBACK" => Kind::Rollback,
        "SET" if matches!(second, Some("TRANSACTION" | "ROLE" | "CONSTRAINT" | "CONSTRAINTS")) => Kind::Other,
        first if CHANGES.contains(&first) => Kind::Change,
        first if DEFINITIONS.contains(&first) => Kind::Definition,
        first if OTHERS.contains(&first) => Kind::Other,
        _ => return None,
    })
}

fn line_refused(line: &str) -> Option<&'static str> {
    let trimmed = line.trim();
    if trimmed == "/" || trimmed == "." {
        return Some("a line with only / or . ends a statement in SQLcl: write it on a line with other text");
    }
    if line.trim_start().starts_with('#') {
        return Some("a line that starts with # runs a SQLcl command: move the # away from the start of the line");
    }
    None
}

pub(crate) fn check(statement: &str) -> Result<Checked, Failure> {
    let words = words(statement);
    let Some(first) = words.first() else {
        return Err(Failure::syntax("there is no statement", 1));
    };
    let Some(kind) = kind_of(&words) else {
        let at = statement.find(|c: char| !c.is_whitespace()).map_or(1, |at| statement[..at].chars().count() + 1);
        return Err(Failure::syntax(
            format!("{first} is not a SQL statement or PL/SQL block: commands of SQLcl are not run"),
            at,
        ));
    };
    let body = statement.trim_end();
    let text = if kind == Kind::Block {
        body.to_owned()
    } else {
        body.strip_suffix(';').unwrap_or(body).trim_end().to_owned()
    };
    let mut before = 0;
    for line in text.split('\n') {
        if let Some(reason) = line_refused(line) {
            return Err(Failure::syntax(reason, before + 1));
        }
        before += line.chars().count() + 1;
    }
    Ok(Checked { text, verb: first.clone(), kind })
}

pub(crate) fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub(crate) fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests;

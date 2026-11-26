pub(crate) fn words(statement: &str) -> Vec<String> {
    let chars: Vec<char> = statement.chars().collect();
    let mut words = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        if c == '-' && next == Some('-') {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 1;
            at += 2;
            while at < chars.len() && depth > 0 {
                match (chars[at], chars.get(at + 1).copied()) {
                    ('/', Some('*')) => {
                        depth += 1;
                        at += 2;
                    }
                    ('*', Some('/')) => {
                        depth -= 1;
                        at += 2;
                    }
                    _ => at += 1,
                }
            }
        } else if matches!(c, '\'' | '"' | '[') {
            let close = if c == '[' { ']' } else { c };
            at += 1;
            while at < chars.len() {
                if chars[at] == close {
                    if chars.get(at + 1) == Some(&close) {
                        at += 2;
                        continue;
                    }
                    break;
                }
                at += 1;
            }
            at += 1;
        } else if c.is_alphabetic() || c == '_' || c == '@' || c == '#' {
            let start = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '@' | '#' | '$')) {
                at += 1;
            }
            words.push(chars[start..at].iter().collect::<String>().to_lowercase());
        } else {
            at += 1;
        }
    }
    words
}

pub(crate) fn counts_rows(statement: &str) -> bool {
    let words = words(statement);
    let changes = |word: &str| matches!(word, "insert" | "update" | "delete" | "merge");
    let first = words.first().map(String::as_str).unwrap_or_default();
    let changing = changes(first) || (first == "with" && words.iter().any(|word| changes(word)));
    changing && !words.iter().any(|word| word == "output")
}

pub(crate) fn selects(statement: &str) -> bool {
    matches!(words(statement).first().map(String::as_str), Some("select" | "with"))
}

pub(crate) fn bracket(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

pub(crate) fn literal(text: &str) -> String {
    format!("N'{}'", text.replace('\'', "''"))
}

pub(crate) fn line_start(statement: &str, line: u32) -> Option<usize> {
    if line <= 1 {
        return Some(1);
    }
    let mut seen = 1;
    for (index, c) in statement.chars().enumerate() {
        if c == '\n' {
            seen += 1;
            if seen == line {
                return Some(index + 2);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_skip_strings_names_and_comments() {
        assert_eq!(
            words("SELECT [update], 'delete' -- insert\n/* merge /* nested */ */ FROM t"),
            ["select", "from", "t"]
        );
        assert_eq!(words("EXEC @r = dbo.p #tmp"), ["exec", "@r", "dbo", "p", "#tmp"]);
    }

    #[test]
    fn changes_without_output_count_rows() {
        assert!(counts_rows("UPDATE t SET a = 1"));
        assert!(counts_rows("  -- note\nINSERT INTO t VALUES (1)"));
        assert!(counts_rows("WITH x AS (SELECT 1 AS a) DELETE FROM t WHERE a IN (SELECT a FROM x)"));
        assert!(!counts_rows("INSERT INTO t OUTPUT inserted.id VALUES (1)"));
        assert!(!counts_rows("SELECT * FROM [delete]"));
        assert!(!counts_rows("EXEC dbo.purge"));
        assert!(selects("WITH x AS (SELECT 1) SELECT * FROM x"));
        assert!(!selects("EXEC sp_who"));
    }

    #[test]
    fn names_and_texts_are_quoted() {
        assert_eq!(bracket("a]b"), "[a]]b]");
        assert_eq!(literal("it's"), "N'it''s'");
    }

    #[test]
    fn error_lines_become_positions() {
        assert_eq!(line_start("SELECT\nFROM", 2), Some(8));
        assert_eq!(line_start("SELECT", 1), Some(1));
        assert_eq!(line_start("SELECT", 3), None);
    }
}

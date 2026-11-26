use zen_db_driver_kit::Failure;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) text: String,
    pub(crate) verb: String,
}

const QUERIES: &[&str] =
    &["SELECT", "WITH", "FROM", "VALUES", "TABLE", "SHOW", "DESCRIBE", "SUMMARIZE", "PIVOT", "UNPIVOT", "("];

impl Checked {
    pub(crate) fn describable(&self) -> bool {
        QUERIES.contains(&self.verb.as_str())
    }

    pub(crate) fn reads(&self) -> bool {
        self.describable() || matches!(self.verb.as_str(), "EXPLAIN" | "PRAGMA" | "CALL")
    }
}

pub(crate) fn check(statement: &str) -> Result<Checked, Failure> {
    let chars: Vec<char> = statement.chars().collect();
    let mut at = 0;
    let mut end = None;
    let mut first_word: Option<(usize, usize)> = None;
    while at < chars.len() {
        if at == 0 || chars[at - 1] == '\n' {
            shell_line(&chars, at)?;
        }
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        match c {
            '\'' | '"' => {
                let start = at;
                at += 1;
                loop {
                    match chars.get(at) {
                        None => return Err(Failure::syntax("the quote is not closed", start + 1)),
                        Some(found) if *found == c && chars.get(at + 1) == Some(&c) => at += 2,
                        Some(found) if *found == c => break,
                        Some(_) => at += 1,
                    }
                }
            }
            '$' if next == Some('$') => {
                let start = at;
                at += 2;
                loop {
                    match chars.get(at) {
                        None => return Err(Failure::syntax("the $$ string is not closed", start + 1)),
                        Some('$') if chars.get(at + 1) == Some(&'$') => {
                            at += 1;
                            break;
                        }
                        Some(_) => at += 1,
                    }
                }
            }
            '-' if next == Some('-') => {
                while chars.get(at + 1).is_some_and(|c| *c != '\n') {
                    at += 1;
                }
            }
            '/' if next == Some('*') => {
                let start = at;
                at += 2;
                while !(chars.get(at) == Some(&'*') && chars.get(at + 1) == Some(&'/')) {
                    if at >= chars.len() {
                        return Err(Failure::syntax("the comment is not closed", start + 1));
                    }
                    at += 1;
                }
                at += 1;
            }
            ';' => {
                if end.is_none() {
                    end = Some(at);
                }
            }
            c if c.is_whitespace() => {}
            _ => {
                if let Some(stop) = end {
                    return Err(Failure::syntax("run one statement at a time", stop + 1));
                }
                if first_word.is_none() {
                    if c == '.' {
                        return Err(Failure::syntax("dot-commands of the duckdb program are not run", at + 1));
                    }
                    let start = at;
                    let mut stop = at + 1;
                    if c.is_alphabetic() {
                        while chars.get(stop).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                            stop += 1;
                        }
                    }
                    first_word = Some((start, stop));
                    at = stop;
                    continue;
                }
            }
        }
        at += 1;
    }
    let text: String = chars[..end.unwrap_or(chars.len())].iter().collect();
    let verb = first_word.map(|(start, stop)| chars[start..stop].iter().collect::<String>().to_ascii_uppercase());
    match verb {
        Some(verb) => Ok(Checked { text: text.trim_end().to_owned(), verb }),
        None => Err(Failure::syntax("there is no statement", 1)),
    }
}

fn shell_line(chars: &[char], start: usize) -> Result<(), Failure> {
    let blank = |c: &char| *c == ' ' || *c == '\t' || *c == '\r';
    let mut at = start;
    while chars.get(at).is_some_and(blank) {
        at += 1;
    }
    if chars.get(at) == Some(&'.') {
        return Err(Failure::syntax("dot-commands of the duckdb program are not run", at + 1));
    }
    let word = match (chars.get(at), chars.get(at + 1)) {
        (Some('/'), next) if next != Some(&'*') => 1,
        (Some(g), Some(o)) if g.eq_ignore_ascii_case(&'g') && o.eq_ignore_ascii_case(&'o') => 2,
        _ => return Ok(()),
    };
    let mut end = at + word;
    if word == 2 && chars.get(end).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
        return Ok(());
    }
    while chars.get(end).is_some_and(blank) {
        end += 1;
    }
    let rest = (chars.get(end), chars.get(end + 1));
    let alone = matches!(rest, (None | Some('\n'), _) | (Some('-'), Some('-')) | (Some('/'), Some('*')));
    if alone {
        return Err(Failure::syntax("a line with only / or GO ends a statement in the duckdb program", at + 1));
    }
    Ok(())
}

pub(crate) fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub(crate) fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests;

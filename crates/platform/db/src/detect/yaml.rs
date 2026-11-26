#[derive(Clone, Debug, PartialEq)]
pub enum Yaml {
    Null,
    Scalar(String),
    List(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

impl Yaml {
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(entries) => entries.iter().find(|(known, _)| known == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn scalar(&self) -> Option<&str> {
        match self {
            Yaml::Scalar(text) => Some(text),
            _ => None,
        }
    }

    pub fn items(&self) -> &[Yaml] {
        match self {
            Yaml::List(items) => items,
            _ => &[],
        }
    }

    pub fn map_scalars(&self, change: &dyn Fn(&str) -> String) -> Yaml {
        match self {
            Yaml::Null => Yaml::Null,
            Yaml::Scalar(text) => Yaml::Scalar(change(text)),
            Yaml::List(items) => Yaml::List(items.iter().map(|item| item.map_scalars(change)).collect()),
            Yaml::Map(entries) => {
                Yaml::Map(entries.iter().map(|(key, value)| (key.clone(), value.map_scalars(change))).collect())
            }
        }
    }
}

struct Line {
    indent: usize,
    text: String,
}

pub fn parse(text: &str) -> Yaml {
    let mut lines = prepare(text);
    let mut at = 0;
    let Some(first) = lines.first() else { return Yaml::Null };
    let indent = first.indent;
    block(&mut lines, &mut at, indent)
}

fn prepare(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        let trimmed = raw.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            if lines.is_empty() {
                continue;
            }
            break;
        }
        let indent = raw.len() - trimmed.len();
        let text = strip_comment(trimmed).trim_end().to_owned();
        lines.push(Line { indent, text });
    }
    lines
}

fn strip_comment(text: &str) -> &str {
    let mut quote = None;
    let mut previous = ' ';
    for (at, c) in text.char_indices() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && previous.is_whitespace() => return &text[..at],
            None => {}
        }
        previous = c;
    }
    text
}

fn is_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn block(lines: &mut Vec<Line>, at: &mut usize, indent: usize) -> Yaml {
    let Some(line) = lines.get(*at) else { return Yaml::Null };
    if line.indent < indent {
        return Yaml::Null;
    }
    let indent = line.indent;
    if is_item(&line.text) {
        list(lines, at, indent)
    } else if split_key(&line.text).is_some() {
        map(lines, at, indent)
    } else {
        let value = scalar(&line.text);
        *at += 1;
        skip_deeper(lines, at, indent);
        value
    }
}

fn skip_deeper(lines: &[Line], at: &mut usize, indent: usize) {
    while lines.get(*at).is_some_and(|line| line.indent > indent) {
        *at += 1;
    }
}

fn list(lines: &mut Vec<Line>, at: &mut usize, indent: usize) -> Yaml {
    let mut items = Vec::new();
    while let Some(line) = lines.get(*at) {
        if line.indent != indent || !is_item(&line.text) {
            break;
        }
        let rest = line.text[1..].trim_start().to_owned();
        if rest.is_empty() {
            *at += 1;
            let nested = lines.get(*at).filter(|line| line.indent > indent).map(|line| line.indent);
            items.push(nested.map_or(Yaml::Null, |nested| block(lines, at, nested)));
        } else {
            let column = indent + (line.text.len() - rest.len());
            lines[*at] = Line { indent: column, text: rest };
            items.push(block(lines, at, column));
        }
        skip_deeper(lines, at, indent);
    }
    Yaml::List(items)
}

fn map(lines: &mut Vec<Line>, at: &mut usize, indent: usize) -> Yaml {
    let mut entries = Vec::new();
    while let Some(line) = lines.get(*at) {
        if line.indent != indent || is_item(&line.text) {
            break;
        }
        let Some((key, rest)) = split_key(&line.text) else { break };
        let mut rest = rest.trim();
        if rest.starts_with('&') {
            rest = rest.split_once(char::is_whitespace).map_or("", |(_, value)| value.trim());
        }
        let block_scalar = (rest.starts_with('|') || rest.starts_with('>'))
            && rest[1..].chars().all(|c| c.is_ascii_digit() || c == '-' || c == '+');
        *at += 1;
        let value = if rest.is_empty() {
            match lines.get(*at) {
                Some(next) if next.indent > indent => {
                    let nested = next.indent;
                    block(lines, at, nested)
                }
                Some(next) if next.indent == indent && is_item(&next.text) => list(lines, at, indent),
                _ => Yaml::Null,
            }
        } else if block_scalar {
            let mut text = Vec::new();
            while let Some(next) = lines.get(*at).filter(|next| next.indent > indent) {
                text.push(next.text.clone());
                *at += 1;
            }
            Yaml::Scalar(text.join("\n"))
        } else {
            scalar(rest)
        };
        skip_deeper(lines, at, indent);
        entries.push((key, value));
    }
    Yaml::Map(entries)
}

fn split_key(text: &str) -> Option<(String, &str)> {
    if text.starts_with('[') || text.starts_with('{') {
        return None;
    }
    let mut quote = None;
    let mut depth = 0usize;
    let bytes = text.as_bytes();
    for (at, c) in text.char_indices() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '[' | '{' => depth += 1,
                ']' | '}' => depth = depth.saturating_sub(1),
                ':' if depth == 0 && bytes.get(at + 1).is_none_or(|next| next.is_ascii_whitespace()) => {
                    let key = text[..at].trim();
                    let key = unquote(key);
                    return Some((key, &text[at + 1..]));
                }
                _ => {}
            },
        }
    }
    None
}

fn unquote(text: &str) -> String {
    if let Some(inner) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return inner.replace("\\\"", "\"").replace("\\\\", "\\");
    }
    if let Some(inner) = text.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    text.to_owned()
}

fn split_flow(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut quote, mut depth, mut start) = (None, 0usize, 0);
    for (at, c) in inner.char_indices() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '[' | '{' => depth += 1,
                ']' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    parts.push(inner[start..at].trim());
                    start = at + 1;
                }
                _ => {}
            },
        }
    }
    parts.push(inner[start..].trim());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

fn scalar(text: &str) -> Yaml {
    let text = text.trim();
    let text = if text.starts_with('&') {
        text.split_once(char::is_whitespace).map_or("", |(_, value)| value.trim())
    } else {
        text
    };
    if text.is_empty() || text == "~" || text == "null" || text.starts_with('*') {
        return Yaml::Null;
    }
    if let Some(inner) = text.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return Yaml::List(split_flow(inner).into_iter().map(scalar).collect());
    }
    if let Some(inner) = text.strip_prefix('{').and_then(|rest| rest.strip_suffix('}')) {
        let entries = split_flow(inner)
            .into_iter()
            .filter_map(|part| split_key(part).map(|(key, value)| (key, scalar(value))))
            .collect();
        return Yaml::Map(entries);
    }
    Yaml::Scalar(unquote(text))
}

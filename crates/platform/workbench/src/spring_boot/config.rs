use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Properties,
    Yaml,
}

pub fn format_of(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let (stem, extension) = name.rsplit_once('.')?;
    if stem != "application" && !stem.strip_prefix("application-").is_some_and(|profile| !profile.is_empty()) {
        return None;
    }
    match extension {
        "properties" => Some(Format::Properties),
        "yml" | "yaml" => Some(Format::Yaml),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigKey {
    pub name: String,
    pub range: Range<usize>,
    pub value: Option<Range<usize>>,
    pub indent: usize,
    pub leaf: bool,
    pub listed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Key { from: usize, parent: String, indent: usize },
    Value { from: usize, key: String },
}

struct Line<'a> {
    start: usize,
    indent: usize,
    content: &'a str,
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut found = Vec::new();
    let mut start = 0;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let content = line.trim_start_matches(' ');
        found.push(Line { start, indent: line.len() - content.len(), content });
        start += raw.len();
    }
    found
}

fn without_comment(content: &str) -> &str {
    let mut quote = None;
    let mut previous = ' ';
    for (at, c) in content.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), c) if c == open => quote = None,
            (None, '#') if previous == ' ' || previous == '\t' || at == 0 => return content[..at].trim_end(),
            _ => {}
        }
        previous = c;
    }
    content.trim_end()
}

fn key_end(content: &str) -> Option<usize> {
    if content.starts_with(['#', '[', '{', '|', '>', '&', '*', '!', '%', '-']) || content.trim_end() == "..." {
        return None;
    }
    if let Some(quote) = content.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let close = content[1..].find(quote)? + 2;
        return content[close..].starts_with(':').then_some(close);
    }
    let colon = content
        .match_indices(':')
        .map(|(at, _)| at)
        .find(|at| content[at + 1..].is_empty() || content[at + 1..].starts_with([' ', '\t']))?;
    let key = content[..colon].trim_end();
    (!key.is_empty()).then_some(key.len())
}

fn unquoted(key: &str) -> &str {
    let key = key.trim();
    if key.len() >= 2 && (key.starts_with('"') && key.ends_with('"') || key.starts_with('\'') && key.ends_with('\'')) {
        &key[1..key.len() - 1]
    } else {
        key
    }
}

fn joined(parent: &str, key: &str) -> String {
    if parent.is_empty() { key.to_owned() } else { format!("{parent}.{key}") }
}

fn yaml_keys(text: &str) -> Vec<ConfigKey> {
    let mut keys: Vec<ConfigKey> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut block: Option<usize> = None;
    let mut item: Option<usize> = None;
    for line in lines(text) {
        let content = without_comment(line.content);
        if content.is_empty() {
            continue;
        }
        if let Some(column) = block {
            if line.indent > column {
                continue;
            }
            block = None;
        }
        if content == "---" {
            stack.clear();
            item = None;
            continue;
        }
        let (indent, content, listed) = match content.strip_prefix("- ").or_else(|| (content == "-").then_some("")) {
            Some(item) => {
                let inner = item.trim_start();
                (line.indent + 2 + (item.len() - inner.len()), inner, true)
            }
            None => (line.indent, content, false),
        };
        if item.is_some_and(|column| indent < column || (listed && indent == column)) {
            item = None;
        }
        while stack.last().is_some_and(|(at, _)| *at >= indent) {
            stack.pop();
        }
        let Some(end) = key_end(content) else {
            continue;
        };
        let parent = stack.last().map(|(_, index)| *index);
        let parent_name = parent.map_or("", |index| keys[index].name.as_str());
        let key_start = line.start + indent;
        let key_text = &content[..end];
        let name = joined(parent_name, unquoted(key_text));
        let rest = content[end..].trim_start_matches(':');
        let value_text = rest.trim_start();
        let value_start = key_start + end + (rest.len() - value_text.len()) + 1;
        if value_text.starts_with(['|', '>']) {
            block = Some(indent);
        }
        if let Some(index) = parent {
            keys[index].leaf = false;
        }
        if listed {
            item = Some(indent);
        }
        let listed = listed || item.is_some() || parent.is_some_and(|index| keys[index].listed);
        let value = (!value_text.is_empty()).then(|| value_start..value_start + value_text.len());
        stack.push((indent, keys.len()));
        keys.push(ConfigKey { name, range: key_start..key_start + end, value, indent, leaf: true, listed });
    }
    keys
}

fn properties_keys(text: &str) -> Vec<ConfigKey> {
    crate::resource_bundles::format::read(text)
        .into_iter()
        .map(|entry| ConfigKey {
            name: entry.key,
            range: entry.key_range,
            value: Some(entry.value_range).filter(|range| !range.is_empty()),
            indent: 0,
            leaf: true,
            listed: false,
        })
        .collect()
}

pub fn keys(format: Format, text: &str) -> Vec<ConfigKey> {
    match format {
        Format::Properties => properties_keys(text),
        Format::Yaml => yaml_keys(text),
    }
}

pub fn indent_unit(keys: &[ConfigKey]) -> usize {
    keys.iter().map(|key| key.indent).filter(|indent| *indent > 0).min().unwrap_or(2)
}

fn line_start(text: &str, caret: usize) -> usize {
    text[..caret].rfind('\n').map_or(0, |at| at + 1)
}

fn properties_place(text: &str, caret: usize) -> Option<Place> {
    let start = line_start(text, caret);
    if start > 0 && text[..start - 1].trim_end_matches('\r').ends_with('\\') {
        return None;
    }
    let line = &text[start..caret];
    let typed = line.trim_start_matches([' ', '\t', '\x0c']);
    if typed.starts_with(['#', '!']) {
        return None;
    }
    let key_start = caret - typed.len();
    let mut escaped = false;
    for (at, c) in typed.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '=' | ':' | ' ' | '\t' if !escaped => {
                let key = typed[..at].to_owned();
                let after = typed[at..].trim_start_matches([' ', '\t']);
                let after = after.strip_prefix(['=', ':']).unwrap_or(after);
                let value = after.trim_start_matches([' ', '\t']);
                return Some(Place::Value { from: caret - value.len(), key });
            }
            _ => escaped = false,
        }
    }
    Some(Place::Key { from: key_start, parent: String::new(), indent: 0 })
}

fn yaml_place(text: &str, caret: usize) -> Option<Place> {
    let start = line_start(text, caret);
    let line = &text[start..caret];
    let content = line.trim_start_matches(' ');
    let indent = line.len() - content.len();
    if content.starts_with('#') || content.contains(" #") {
        return None;
    }
    let before: Vec<ConfigKey> = yaml_keys(&text[..start]);
    let parent_at = |indent: usize| {
        let mut stack: Vec<&ConfigKey> = Vec::new();
        for key in &before {
            while stack.last().is_some_and(|top| top.indent >= key.indent) {
                stack.pop();
            }
            stack.push(key);
        }
        while stack.last().is_some_and(|top| top.indent >= indent) {
            stack.pop();
        }
        stack.last().map(|key| key.name.clone()).unwrap_or_default()
    };
    if let Some(item) = content.strip_prefix("- ") {
        let value = item.trim_start();
        return Some(Place::Value { from: caret - value.len(), key: parent_at(indent) });
    }
    if let Some(end) = key_end(content) {
        let rest = content[end..].trim_start_matches(':');
        let value = rest.trim_start();
        let key = joined(&parent_at(indent), unquoted(&content[..end]));
        return Some(Place::Value { from: caret - value.len(), key });
    }
    if content.contains([':', ' ', '\t']) {
        return None;
    }
    Some(Place::Key { from: caret - content.len(), parent: parent_at(indent), indent })
}

pub fn place_at(format: Format, text: &str, caret: usize) -> Option<Place> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    match format {
        Format::Properties => properties_place(text, caret),
        Format::Yaml => yaml_place(text, caret),
    }
}

pub fn yaml_insert(relative: &str, indent: usize, unit: usize, line_break: &str) -> String {
    let parts = split_name(relative);
    let mut out = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push_str(line_break);
            out.push_str(&" ".repeat(indent + unit * index));
        }
        out.push_str(part);
        out.push(':');
    }
    out.push(' ');
    out
}

fn split_name(name: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (at, c) in name.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '.' if depth == 0 => {
                parts.push(&name[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&name[start..]);
    parts
}

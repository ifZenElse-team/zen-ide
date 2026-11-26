#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Map(Vec<Entry>),
    Seq(Vec<Node>),
    Str(String),
    Null,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub key_start: usize,
    pub key_end: usize,
    pub value: Node,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub value: Value,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub root: Node,
    pub start: usize,
    pub end: usize,
}

impl Node {
    fn null(at: usize) -> Node {
        Node { value: Value::Null, start: at, end: at }
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        match &self.value {
            Value::Map(entries) => entries.iter().find(|entry| entry.key == key).map(|entry| &entry.value),
            _ => None,
        }
    }

    pub fn at(&self, keys: &[&str]) -> Option<&Node> {
        keys.iter().try_fold(self, |node, key| node.get(key))
    }

    pub fn str(&self) -> Option<&str> {
        match &self.value {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn items(&self) -> &[Node] {
        match &self.value {
            Value::Seq(items) => items,
            _ => &[],
        }
    }
}

struct Line {
    start: usize,
    text: String,
}

fn indent(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
}

fn strip_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let (mut single, mut double) = (false, false);
    for (at, byte) in bytes.iter().enumerate() {
        match byte {
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b'#' if !single && !double && (at == 0 || bytes[at - 1] == b' ' || bytes[at - 1] == b'\t') => {
                return text[..at].trim_end();
            }
            _ => {}
        }
    }
    text.trim_end()
}

fn blank_comment(text: &str) -> String {
    let kept = strip_comment(text).len();
    let mut out = text[..kept].to_owned();
    out.push_str(&" ".repeat(text.len() - kept));
    out
}

fn blank(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

fn key_of(content: &str) -> Option<(String, usize, usize, usize)> {
    let bytes = content.as_bytes();
    let first = *bytes.first()?;
    if first == b'"' || first == b'\'' {
        let close = content[1..].find(first as char)? + 1;
        let rest = &content[close + 1..];
        let colon = rest.len() - rest.trim_start().len();
        if !rest[colon..].starts_with(':') {
            return None;
        }
        let after = close + 1 + colon + 1;
        if after < content.len() && !content[after..].starts_with([' ', '\t']) {
            return None;
        }
        return Some((content[1..close].to_owned(), 0, close + 1, after));
    }
    if matches!(first, b'-' | b'[' | b'{' | b'#' | b'|' | b'>' | b'&' | b'*' | b'!' | b'%' | b'@' | b'`' | b'?') {
        return None;
    }
    let mut at = 0;
    while let Some(found) = content[at..].find(':') {
        let colon = at + found;
        let after = colon + 1;
        if after == content.len() || content[after..].starts_with([' ', '\t']) {
            let key = content[..colon].trim_end();
            if key.is_empty() {
                return None;
            }
            return Some((key.to_owned(), 0, key.len(), after));
        }
        at = after;
    }
    None
}

fn scalar(text: &str, start: usize) -> Node {
    let trimmed_start = text.len() - text.trim_start().len();
    let mut body = text.trim();
    let mut offset = start + trimmed_start;
    loop {
        let skip =
            if body.starts_with('&') || body.starts_with('!') { body.find(' ').map(|space| space + 1) } else { None };
        match skip {
            Some(skip) => {
                offset += skip;
                body = body[skip..].trim_start();
            }
            None => break,
        }
    }
    let end = offset + body.len();
    if body.is_empty() || body == "~" || body == "null" {
        return Node { value: Value::Null, start: offset, end };
    }
    let unquoted = if body.len() >= 2 && body.starts_with('"') && body.ends_with('"') {
        body[1..body.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else if body.len() >= 2 && body.starts_with('\'') && body.ends_with('\'') {
        body[1..body.len() - 1].replace("''", "'")
    } else {
        body.to_owned()
    };
    Node { value: Value::Str(unquoted), start: offset, end }
}

struct Flow<'a> {
    text: &'a str,
    base: usize,
    at: usize,
}

impl Flow<'_> {
    fn space(&mut self) {
        while self.text[self.at..].starts_with([' ', '\t', '\n', '\r']) {
            self.at += 1;
        }
    }

    fn node(&mut self, depth: usize) -> Node {
        self.space();
        let start = self.base + self.at;
        if depth > 64 {
            self.at = self.text.len();
            return Node::null(start);
        }
        match self.text[self.at..].chars().next() {
            Some('[') => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.space();
                    match self.text[self.at..].chars().next() {
                        None => break,
                        Some(']') => {
                            self.at += 1;
                            break;
                        }
                        Some(',') => self.at += 1,
                        _ => {
                            let before = self.at;
                            items.push(self.node(depth + 1));
                            if self.at == before {
                                self.at += 1;
                            }
                        }
                    }
                }
                Node { value: Value::Seq(items), start, end: self.base + self.at }
            }
            Some('{') => {
                self.at += 1;
                let mut entries = Vec::new();
                loop {
                    self.space();
                    match self.text[self.at..].chars().next() {
                        None => break,
                        Some('}') => {
                            self.at += 1;
                            break;
                        }
                        Some(',') => self.at += 1,
                        _ => {
                            let key_start = self.at;
                            let key_end = self.text[self.at..]
                                .find([':', ',', '}'])
                                .map_or(self.text.len(), |found| self.at + found);
                            let key = scalar(&self.text[key_start..key_end], self.base + key_start);
                            self.at = key_end;
                            let value = if self.text[self.at..].starts_with(':') {
                                self.at += 1;
                                self.node(depth + 1)
                            } else {
                                Node::null(self.base + self.at)
                            };
                            if let Value::Str(name) = key.value {
                                entries.push(Entry { key: name, key_start: key.start, key_end: key.end, value });
                            }
                        }
                    }
                }
                Node { value: Value::Map(entries), start, end: self.base + self.at }
            }
            Some(quote @ ('"' | '\'')) => {
                let close =
                    self.text[self.at + 1..].find(quote).map_or(self.text.len(), |found| self.at + 1 + found + 1);
                let node = scalar(&self.text[self.at..close.min(self.text.len())], start);
                self.at = close.min(self.text.len());
                node
            }
            _ => {
                let end = self.text[self.at..].find([',', ']', '}']).map_or(self.text.len(), |found| self.at + found);
                let node = scalar(&self.text[self.at..end], start);
                self.at = end;
                node
            }
        }
    }
}

struct Parser {
    lines: Vec<Line>,
    at: usize,
}

impl Parser {
    fn skip_blank(&mut self) {
        while self.at < self.lines.len() && blank(&self.lines[self.at].text) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<(usize, String)> {
        self.skip_blank();
        let line = self.lines.get(self.at)?;
        let ind = indent(&line.text);
        Some((ind, strip_comment(&line.text[ind..]).to_owned()))
    }

    fn node(&mut self, depth: usize) -> Node {
        let Some((ind, content)) = self.peek() else {
            let end = self.lines.last().map_or(0, |line| line.start + line.text.len());
            return Node::null(end);
        };
        if depth > 64 {
            self.at = self.lines.len();
            return Node::null(self.lines.last().map_or(0, |line| line.start));
        }
        if content == "-" || content.starts_with("- ") {
            self.seq(ind, depth)
        } else if key_of(&content).is_some() {
            self.map(ind, depth)
        } else {
            let line = &self.lines[self.at];
            let start = line.start + ind;
            self.at += 1;
            self.flow_or_scalar(&content, start)
        }
    }

    fn flow_or_scalar(&mut self, content: &str, start: usize) -> Node {
        if !(content.starts_with('[') || content.starts_with('{')) {
            return scalar(content, start);
        }
        let open = |text: &str| {
            text.chars().filter(|c| matches!(c, '[' | '{')).count()
                > text.chars().filter(|c| matches!(c, ']' | '}')).count()
        };
        let first = self
            .lines
            .get(self.at.saturating_sub(1))
            .filter(|line| start >= line.start && start <= line.start + line.text.len());
        let mut text = match first {
            Some(line) => blank_comment(&line.text)[start - line.start..].to_owned(),
            None => content.to_owned(),
        };
        let mut previous_end = first.map_or(start + content.len(), |line| line.start + line.text.len());
        while open(&text) && self.at < self.lines.len() {
            let line = &self.lines[self.at];
            let gap = line.start.saturating_sub(previous_end).max(1);
            text.push_str(&" ".repeat(gap.saturating_sub(1)));
            text.push('\n');
            text.push_str(&blank_comment(&line.text));
            previous_end = line.start + line.text.len();
            self.at += 1;
        }
        Flow { text: &text, base: start, at: 0 }.node(0)
    }

    fn block_scalar(&mut self, ind: usize, start: usize) -> Node {
        let mut parts = Vec::new();
        let mut end = start;
        while self.at < self.lines.len() {
            let line = &self.lines[self.at];
            if !line.text.trim().is_empty() && indent(&line.text) <= ind {
                break;
            }
            parts.push(line.text.trim().to_owned());
            end = line.start + line.text.len();
            self.at += 1;
        }
        Node { value: Value::Str(parts.join("\n").trim().to_owned()), start, end }
    }

    fn map(&mut self, ind: usize, depth: usize) -> Node {
        let mut entries = Vec::new();
        let mut start = None;
        let mut end = 0;
        while let Some((here, content)) = self.peek() {
            if here != ind || content.starts_with("- ") || content == "-" {
                break;
            }
            let Some((key, key_from, key_to, after)) = key_of(&content) else { break };
            let line_start = self.lines[self.at].start + here;
            start.get_or_insert(line_start);
            self.at += 1;
            let rest = &content[after..];
            let rest_start = line_start + after + (rest.len() - rest.trim_start().len());
            let rest = rest.trim();
            let value = if rest.starts_with('|') || rest.starts_with('>') {
                self.block_scalar(ind, rest_start)
            } else if !rest.is_empty() {
                let rest = rest.to_owned();
                self.flow_or_scalar(&rest, rest_start)
            } else {
                match self.peek() {
                    Some((next, _)) if next > ind => self.node(depth + 1),
                    Some((next, next_content))
                        if next == ind && (next_content.starts_with("- ") || next_content == "-") =>
                    {
                        self.seq(ind, depth + 1)
                    }
                    _ => Node::null(line_start + after),
                }
            };
            end = end.max(value.end).max(line_start + key_to);
            entries.push(Entry { key, key_start: line_start + key_from, key_end: line_start + key_to, value });
        }
        let start = start.unwrap_or(end);
        Node { value: Value::Map(entries), start, end }
    }

    fn seq(&mut self, ind: usize, depth: usize) -> Node {
        let mut items = Vec::new();
        let mut start = None;
        let mut end = 0;
        while let Some((here, content)) = self.peek() {
            if here != ind || !(content.starts_with("- ") || content == "-") {
                break;
            }
            let line = self.lines[self.at].start;
            start.get_or_insert(line + here);
            let rest = &content[1..];
            let item = if rest.trim().is_empty() {
                self.at += 1;
                match self.peek() {
                    Some((next, _)) if next > ind => self.node(depth + 1),
                    _ => Node::null(line + here + 1),
                }
            } else {
                let text = &mut self.lines[self.at].text;
                let replaced: String =
                    text.chars().enumerate().map(|(at, c)| if at == here { ' ' } else { c }).collect();
                *text = replaced;
                self.node(depth + 1)
            };
            end = end.max(item.end);
            items.push(item);
        }
        Node { value: Value::Seq(items), start: start.unwrap_or(end), end }
    }
}

pub fn documents(text: &str) -> Vec<Document> {
    let mut docs = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    let mut doc_start = 0;
    let mut offset = 0;
    let finish = |lines: Vec<Line>, docs: &mut Vec<Document>, start: usize, end: usize| {
        if lines.iter().all(|line| blank(&line.text)) {
            return;
        }
        let mut parser = Parser { lines, at: 0 };
        let root = parser.node(0);
        docs.push(Document { root, start, end });
    };
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let marker = line.starts_with("---") && (line.len() == 3 || line[3..].starts_with([' ', '\t', '#']));
        if marker || line == "..." {
            finish(std::mem::take(&mut lines), &mut docs, doc_start, offset);
            doc_start = offset + raw.len();
        } else if !line.starts_with('%') || !lines.is_empty() {
            lines.push(Line { start: offset, text: line.replace('\t', " ") });
        }
        offset += raw.len();
    }
    finish(lines, &mut docs, doc_start, offset);
    docs
}

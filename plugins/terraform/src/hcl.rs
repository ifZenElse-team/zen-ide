#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    Function = 7,
    Constant = 10,
    Punctuation = 11,
    Property = 17,
    Global = 18,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Entry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Comment,
    Text,
    Escape,
    Open,
    Close,
    HeredocOpen,
    HeredocClose,
    Ident,
    Number,
    Punct(u8),
    Operator,
}

#[derive(Clone, Copy, Debug)]
struct Tok {
    start: usize,
    end: usize,
    token: Token,
}

enum Mode {
    Expr { braces: u32 },
    Quoted,
    Heredoc { marker: (usize, usize), line_start: bool },
}

const GLOBALS: [&str; 9] = ["var", "local", "module", "data", "each", "count", "self", "path", "terraform"];

const KEYWORDS: [&str; 3] = ["for", "in", "if"];

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn char_len(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(1, char::len_utf8)
}

fn line_end(bytes: &[u8], at: usize) -> usize {
    bytes[at..].iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |found| at + found)
}

fn heredoc_marker(bytes: &[u8], at: usize) -> Option<(usize, usize, usize)> {
    let mut name = at + 2;
    if bytes.get(name) == Some(&b'-') {
        name += 1;
    }
    if !bytes.get(name).copied().is_some_and(is_ident_start) {
        return None;
    }
    let end = name + bytes[name..].iter().take_while(|byte| is_ident(**byte)).count();
    let after = bytes[end..].iter().position(|byte| *byte != b' ' && *byte != b'\t' && *byte != b'\r');
    match after.map(|found| bytes[end + found]) {
        Some(b'\n') | None => Some((name, end, line_end(bytes, end))),
        Some(_) => None,
    }
}

fn template_start(bytes: &[u8], at: usize) -> Option<usize> {
    let opens = matches!(bytes.get(at), Some(b'$' | b'%')) && bytes.get(at + 1) == Some(&b'{');
    if !opens {
        return None;
    }
    Some(if bytes.get(at + 2) == Some(&b'~') { at + 3 } else { at + 2 })
}

fn is_escaped_template(bytes: &[u8], at: usize) -> bool {
    matches!(bytes.get(at), Some(b'$' | b'%')) && bytes.get(at + 1) == bytes.get(at) && bytes.get(at + 2) == Some(&b'{')
}

fn set_braces(stack: &mut [Mode], depth: u32) {
    if let Some(Mode::Expr { braces }) = stack.last_mut() {
        *braces = depth;
    }
}

fn tokens(text: &str) -> Vec<Tok> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut stack = vec![Mode::Expr { braces: 0 }];
    let mut at = 0;
    let mut text_start: Option<usize> = None;
    let flush = |out: &mut Vec<Tok>, from: &mut Option<usize>, to: usize| {
        if let Some(start) = from.take()
            && to > start
        {
            out.push(Tok { start, end: to, token: Token::Text });
        }
    };
    while at < bytes.len() {
        match stack.last_mut() {
            Some(Mode::Expr { braces }) => {
                let depth = *braces;
                let nested = stack.len() > 1;
                let c = bytes[at];
                match c {
                    b' ' | b'\t' | b'\r' | b'\n' => at += 1,
                    b'#' => {
                        let end = line_end(bytes, at);
                        out.push(Tok { start: at, end, token: Token::Comment });
                        at = end;
                    }
                    b'/' if bytes.get(at + 1) == Some(&b'/') => {
                        let end = line_end(bytes, at);
                        out.push(Tok { start: at, end, token: Token::Comment });
                        at = end;
                    }
                    b'/' if bytes.get(at + 1) == Some(&b'*') => {
                        let end = text[at + 2..].find("*/").map_or(bytes.len(), |found| at + 2 + found + 2);
                        out.push(Tok { start: at, end, token: Token::Comment });
                        at = end;
                    }
                    b'"' => {
                        text_start = Some(at);
                        at += 1;
                        stack.push(Mode::Quoted);
                    }
                    b'<' if bytes.get(at + 1) == Some(&b'<') && heredoc_marker(bytes, at).is_some() => {
                        let (name, end, line) = heredoc_marker(bytes, at).unwrap_or((at, at, at));
                        out.push(Tok { start: at, end, token: Token::HeredocOpen });
                        at = (line + 1).min(bytes.len());
                        stack.push(Mode::Heredoc { marker: (name, end), line_start: true });
                    }
                    b'{' | b'[' | b'(' => {
                        if c == b'{' {
                            set_braces(&mut stack, depth + 1);
                        }
                        out.push(Tok { start: at, end: at + 1, token: Token::Punct(c) });
                        at += 1;
                    }
                    b'}' if depth == 0 && nested => {
                        out.push(Tok { start: at, end: at + 1, token: Token::Close });
                        at += 1;
                        stack.pop();
                        if matches!(stack.last(), Some(Mode::Quoted | Mode::Heredoc { .. })) {
                            text_start = Some(at);
                        }
                    }
                    b'}' | b']' | b')' | b',' | b'.' | b':' | b'?' => {
                        if c == b'}' {
                            set_braces(&mut stack, depth.saturating_sub(1));
                        }
                        out.push(Tok { start: at, end: at + 1, token: Token::Punct(c) });
                        at += 1;
                    }
                    b'~' if bytes.get(at + 1) == Some(&b'}') && depth == 0 && nested => {
                        out.push(Tok { start: at, end: at + 2, token: Token::Close });
                        at += 2;
                        stack.pop();
                        if matches!(stack.last(), Some(Mode::Quoted | Mode::Heredoc { .. })) {
                            text_start = Some(at);
                        }
                    }
                    b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'+' | b'-' | b'*' | b'/' | b'%' => {
                        let two = bytes.get(at + 1).is_some_and(|next| {
                            matches!(
                                (c, next),
                                (b'=' | b'!' | b'<' | b'>', b'=') | (b'&', b'&') | (b'|', b'|') | (b'=', b'>')
                            )
                        });
                        let end = at + if two { 2 } else { 1 };
                        let token = if c == b'=' && !two { Token::Punct(b'=') } else { Token::Operator };
                        out.push(Tok { start: at, end, token });
                        at = end;
                    }
                    _ if c.is_ascii_digit() => {
                        let mut end = at + bytes[at..].iter().take_while(|byte| byte.is_ascii_digit()).count();
                        if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
                            end += 1 + bytes[end + 1..].iter().take_while(|byte| byte.is_ascii_digit()).count();
                        }
                        if matches!(bytes.get(end), Some(b'e' | b'E')) {
                            let digits =
                                if matches!(bytes.get(end + 1), Some(b'+' | b'-')) { end + 2 } else { end + 1 };
                            if bytes.get(digits).is_some_and(u8::is_ascii_digit) {
                                end = digits + bytes[digits..].iter().take_while(|byte| byte.is_ascii_digit()).count();
                            }
                        }
                        out.push(Tok { start: at, end, token: Token::Number });
                        at = end;
                    }
                    _ if is_ident_start(c) => {
                        let end = at + bytes[at..].iter().take_while(|byte| is_ident(**byte)).count();
                        out.push(Tok { start: at, end, token: Token::Ident });
                        at = end;
                    }
                    _ => at += char_len(text, at),
                }
            }
            Some(Mode::Quoted) => {
                let c = bytes[at];
                match c {
                    b'"' => {
                        at += 1;
                        flush(&mut out, &mut text_start.or(Some(at - 1)), at);
                        text_start = None;
                        stack.pop();
                    }
                    b'\n' => {
                        flush(&mut out, &mut text_start, at);
                        stack.pop();
                    }
                    b'\\' => {
                        flush(&mut out, &mut text_start, at);
                        let len = match bytes.get(at + 1) {
                            Some(b'u') => 6,
                            Some(b'U') => 10,
                            Some(_) => 2,
                            None => 1,
                        };
                        let mut end = (at + len).min(bytes.len());
                        while !text.is_char_boundary(end) {
                            end += 1;
                        }
                        out.push(Tok { start: at, end, token: Token::Escape });
                        at = end;
                        text_start = Some(at);
                    }
                    _ if is_escaped_template(bytes, at) => at += 3,
                    _ => match template_start(bytes, at) {
                        Some(end) => {
                            flush(&mut out, &mut text_start, at);
                            out.push(Tok { start: at, end, token: Token::Open });
                            at = end;
                            stack.push(Mode::Expr { braces: 0 });
                        }
                        None => {
                            text_start.get_or_insert(at);
                            at += char_len(text, at);
                        }
                    },
                }
            }
            Some(Mode::Heredoc { marker, line_start }) => {
                if *line_start {
                    *line_start = false;
                    let end = line_end(bytes, at);
                    let line = &text[at..end];
                    let name = &text[marker.0..marker.1];
                    if line.trim() == name {
                        flush(&mut out, &mut text_start, at);
                        let start = at + (line.len() - line.trim_start().len());
                        out.push(Tok { start, end: start + name.len(), token: Token::HeredocClose });
                        at = start + name.len();
                        stack.pop();
                        continue;
                    }
                }
                let c = bytes[at];
                if c == b'\n' {
                    flush(&mut out, &mut text_start, at);
                    at += 1;
                    if let Some(Mode::Heredoc { line_start, .. }) = stack.last_mut() {
                        *line_start = true;
                    }
                } else if is_escaped_template(bytes, at) {
                    text_start.get_or_insert(at);
                    at += 3;
                } else if let Some(end) = template_start(bytes, at) {
                    flush(&mut out, &mut text_start, at);
                    out.push(Tok { start: at, end, token: Token::Open });
                    at = end;
                    stack.push(Mode::Expr { braces: 0 });
                } else {
                    text_start.get_or_insert(at);
                    at += char_len(text, at);
                }
            }
            None => break,
        }
    }
    flush(&mut out, &mut text_start, bytes.len());
    out.sort_by_key(|tok| tok.start);
    out
}

fn first_on_line(text: &str, at: usize) -> bool {
    text[..at].rsplit('\n').next().is_some_and(|before| before.trim().is_empty())
}

fn same_line(text: &str, from: usize, to: usize) -> bool {
    !text[from..to].contains('\n')
}

fn block_header(text: &str, toks: &[Tok], index: usize) -> Option<usize> {
    let tok = toks[index];
    if tok.token != Token::Ident || !first_on_line(text, tok.start) {
        return None;
    }
    for (offset, next) in toks[index + 1..].iter().enumerate() {
        if !same_line(text, tok.start, next.start) {
            return None;
        }
        match next.token {
            Token::Punct(b'{') => return Some(index + 1 + offset),
            Token::Text | Token::Escape | Token::Ident => {}
            _ => return None,
        }
    }
    None
}

fn label_text(text: &str, toks: &[Tok]) -> String {
    let mut name = String::new();
    let mut last_end = None;
    for tok in toks {
        if last_end.is_some_and(|end| end < tok.start) && !name.ends_with(' ') {
            name.push(' ');
        }
        name.push_str(&text[tok.start..tok.end]);
        last_end = Some(tok.end);
    }
    name
}

pub fn highlight(text: &str) -> Vec<Span> {
    let toks = tokens(text);
    let mut spans = Vec::with_capacity(toks.len());
    let headers: Vec<bool> = (0..toks.len()).map(|index| block_header(text, &toks, index).is_some()).collect();
    for (index, tok) in toks.iter().enumerate() {
        let word = &text[tok.start..tok.end];
        let kind = match tok.token {
            Token::Comment => Some(Kind::Comment),
            Token::Text => Some(Kind::String),
            Token::Escape => Some(Kind::Constant),
            Token::Open | Token::Close => Some(Kind::Punctuation),
            Token::HeredocOpen | Token::HeredocClose => Some(Kind::Keyword),
            Token::Number => Some(Kind::Number),
            Token::Punct(_) | Token::Operator => Some(Kind::Punctuation),
            Token::Ident => {
                let next = toks.get(index + 1);
                let previous = index.checked_sub(1).map(|at| toks[at]);
                let after_dot = previous.is_some_and(|tok| tok.token == Token::Punct(b'.'));
                if headers[index] {
                    Some(Kind::Keyword)
                } else if next.is_some_and(|next| next.token == Token::Punct(b'('))
                    && same_line(text, tok.end, next.map_or(tok.end, |next| next.start))
                {
                    Some(Kind::Function)
                } else if !after_dot && matches!(word, "true" | "false" | "null") {
                    Some(Kind::Constant)
                } else if !after_dot && next.is_some_and(|next| next.token == Token::Punct(b'=')) {
                    Some(Kind::Property)
                } else if !after_dot
                    && GLOBALS.contains(&word)
                    && next.is_some_and(|next| next.token == Token::Punct(b'.'))
                {
                    Some(Kind::Global)
                } else if !after_dot && KEYWORDS.contains(&word) {
                    Some(Kind::Keyword)
                } else {
                    None
                }
            }
        };
        if let Some(kind) = kind
            && tok.end > tok.start
        {
            spans.push(Span { start: tok.start, end: tok.end, kind });
        }
    }
    spans
}

fn line_of(starts: &[usize], offset: usize) -> u32 {
    (starts.partition_point(|start| *start <= offset) - 1) as u32
}

pub fn fold(text: &str) -> Vec<Fold> {
    let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
    let mut folds = Vec::new();
    let mut open: Vec<(u8, u32)> = Vec::new();
    let mut heredocs: Vec<u32> = Vec::new();
    let add = |folds: &mut Vec<Fold>, start_line: u32, end_line: u32| {
        if end_line > start_line {
            folds.push(Fold { start_line, end_line });
        }
    };
    for tok in tokens(text) {
        let line = line_of(&starts, tok.start);
        match tok.token {
            Token::Punct(c @ (b'{' | b'[' | b'(')) => open.push((c, line)),
            Token::Punct(c @ (b'}' | b']' | b')')) => {
                let wanted = match c {
                    b'}' => b'{',
                    b']' => b'[',
                    _ => b'(',
                };
                if let Some(at) = open.iter().rposition(|(opened, _)| *opened == wanted) {
                    let (_, start) = open[at];
                    open.truncate(at);
                    add(&mut folds, start, line);
                }
            }
            Token::HeredocOpen => heredocs.push(line),
            Token::HeredocClose => {
                if let Some(start) = heredocs.pop() {
                    add(&mut folds, start, line);
                }
            }
            Token::Comment if text[tok.start..].starts_with("/*") => {
                add(&mut folds, line, line_of(&starts, tok.end.saturating_sub(1)));
            }
            _ => {}
        }
    }
    folds.sort_by_key(|fold| (fold.start_line, fold.end_line));
    folds.dedup();
    folds
}

fn kind_of(block: &str, top: bool) -> &'static str {
    if !top {
        return "key";
    }
    match block {
        "resource" | "data" | "ephemeral" => "class",
        "module" | "terraform" | "locals" => "module",
        "variable" => "variable",
        "output" => "field",
        "provider" => "interface",
        _ => "key",
    }
}

pub fn outline(text: &str) -> Vec<Entry> {
    let toks = tokens(text);
    let mut root: Vec<Entry> = Vec::new();
    let mut stack: Vec<Option<Entry>> = Vec::new();
    let mut index = 0;
    while index < toks.len() {
        let tok = toks[index];
        if let Some(brace) = block_header(text, &toks, index)
            && stack.len() < 32
        {
            let block = &text[tok.start..tok.end];
            let top = !stack.iter().any(Option::is_some);
            let name = label_text(text, &toks[index..brace]);
            stack.push(Some(Entry {
                name,
                kind: kind_of(block, top),
                start: tok.start,
                end: tok.end,
                children: Vec::new(),
            }));
            index = brace + 1;
            continue;
        }
        match tok.token {
            Token::Punct(b'{') => stack.push(None),
            Token::Punct(b'}') => {
                if let Some(Some(mut entry)) = stack.pop() {
                    entry.end = tok.end;
                    match stack.iter_mut().rev().find_map(Option::as_mut) {
                        Some(parent) => parent.children.push(entry),
                        None => root.push(entry),
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    while let Some(open) = stack.pop() {
        if let Some(mut entry) = open {
            entry.end = text.len();
            match stack.iter_mut().rev().find_map(Option::as_mut) {
                Some(parent) => parent.children.push(entry),
                None => root.push(entry),
            }
        }
    }
    root.truncate(20_000);
    root
}

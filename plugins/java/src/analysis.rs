use crate::lexer::{self, Class, KEYWORDS, MODULE_WORDS, Raw, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    Doc = 4,
    Type = 6,
    Function = 7,
    Attribute = 9,
    Constant = 10,
    Punctuation = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub raw: Raw,
    pub class: Class,
}

pub fn tokens(text: &str) -> (Vec<Token>, Vec<(u32, u32)>) {
    let module = lexer::is_module_declaration(text);
    let mut all = Vec::new();
    let mut spans = Vec::new();
    let mut state = State::Code;
    let mut opened: Option<u32> = None;
    let mut offset = 0;
    for (number, line) in text.split('\n').enumerate() {
        let number = u32::try_from(number).unwrap_or(u32::MAX);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let (found, next) = lexer::lex_line(line, state);
        let classes = lexer::classify(line, &found, module);
        for (token, class) in found.iter().zip(classes) {
            all.push(Token {
                start: offset + token.start,
                end: offset + token.end,
                line: number,
                raw: token.raw,
                class,
            });
        }
        match (state == State::Code, next == State::Code) {
            (true, false) => opened = Some(number),
            (false, true) => {
                if let Some(first) = opened.take().filter(|first| number > *first) {
                    spans.push((first, number));
                }
            }
            _ => {}
        }
        state = next;
        offset += text[offset..].find('\n').map_or(text.len() - offset, |at| at + 1);
    }
    (all, spans)
}

pub fn highlight(text: &str) -> Vec<Span> {
    let (tokens, _) = tokens(text);
    tokens
        .iter()
        .filter_map(|token| {
            let kind = match token.raw {
                Raw::Number => Kind::Number,
                Raw::String => Kind::String,
                Raw::Comment => Kind::Comment,
                Raw::Doc => Kind::Doc,
                Raw::At => Kind::Attribute,
                Raw::Punct => Kind::Punctuation,
                Raw::Word => match token.class {
                    Class::Keyword => Kind::Keyword,
                    Class::Type => Kind::Type,
                    Class::Function => Kind::Function,
                    Class::Constant => Kind::Constant,
                    Class::Annotation => Kind::Attribute,
                    Class::Plain => return None,
                },
            };
            Some(Span { start: token.start, end: token.end, kind })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

pub fn fold(text: &str) -> Vec<Fold> {
    let (tokens, spans) = tokens(text);
    let mut folds: Vec<Fold> = spans.into_iter().map(|(start_line, end_line)| Fold { start_line, end_line }).collect();
    let mut open = Vec::new();
    for token in tokens.iter().filter(|token| token.raw == Raw::Punct) {
        match &text[token.start..token.end] {
            "{" => open.push(token.line),
            "}" => {
                if let Some(start_line) = open.pop().filter(|start| token.line > *start) {
                    folds.push(Fold { start_line, end_line: token.line });
                }
            }
            _ => {}
        }
    }
    let mut run: Option<(u32, u32)> = None;
    let mut last_code_line = None;
    for token in tokens.iter().filter(|token| !matches!(token.raw, Raw::Comment | Raw::Doc)) {
        if last_code_line == Some(token.line) {
            continue;
        }
        last_code_line = Some(token.line);
        let word = &text[token.start..token.end];
        if word == "import" && token.class == Class::Keyword {
            run = Some(run.map_or((token.line, token.line), |(first, _)| (first, token.line)));
        } else if let Some((first, last)) = run.take()
            && last > first
        {
            folds.push(Fold { start_line: first, end_line: last });
        }
    }
    if let Some((first, last)) = run
        && last > first
    {
        folds.push(Fold { start_line: first, end_line: last });
    }
    folds.sort();
    folds.dedup();
    folds
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Interface,
    Enum,
    Method,
    Field,
    Constant,
    Module,
}

impl SymbolKind {
    fn name(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Enum => "type",
            Self::Method => "method",
            Self::Field => "field",
            Self::Constant => "constant",
            Self::Module => "module",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

const MAX_DEPTH: usize = 32;

struct Parser<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    count: usize,
}

impl<'a> Parser<'a> {
    fn word(&self, at: usize) -> &'a str {
        self.tokens.get(at).map_or("", |token| &self.text[token.start..token.end])
    }

    fn is_word(&self, at: usize) -> bool {
        self.tokens.get(at).is_some_and(|token| token.raw == Raw::Word)
    }

    fn closing(&self, at: usize) -> usize {
        let (open, close) = match self.word(at) {
            "{" => ("{", "}"),
            "(" => ("(", ")"),
            "[" => ("[", "]"),
            _ => return at,
        };
        let mut depth = 0;
        for index in at..self.tokens.len() {
            let word = self.word(index);
            if word == open {
                depth += 1;
            } else if word == close {
                depth -= 1;
                if depth == 0 {
                    return index;
                }
            }
        }
        self.tokens.len().saturating_sub(1)
    }

    fn end_of(&self, at: usize) -> usize {
        self.tokens.get(at).map_or(self.text.len(), |token| token.end)
    }

    fn body(&mut self, from: usize, to: usize, depth: usize) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut at = from;
        while at < to && self.count < 20_000 {
            let head = at;
            let mut end = at;
            let mut paren_at = None;
            while end < to {
                match self.word(end) {
                    "{" | ";" | "=" => break,
                    "(" | "[" => {
                        if self.word(end) == "(" && paren_at.is_none() && end > head && self.is_word(end - 1) {
                            paren_at = Some(end);
                        }
                        end = self.closing(end) + 1;
                    }
                    _ => end += 1,
                }
            }
            if end >= to {
                break;
            }
            let terminator = self.word(end);
            let words: Vec<usize> = (head..end).filter(|&index| self.is_word(index)).collect();
            let first = words.first().map_or("", |&index| self.word(index));
            if matches!(first, "package" | "import") {
                at = self.skip_to_semicolon(end, to) + 1;
                continue;
            }
            if let Some(symbol) = self.type_declaration(head, end, depth) {
                at = self.after(&symbol, to);
                symbols.push(symbol);
                self.count += 1;
                continue;
            }
            if self.module_declaration(head, end).is_some() && terminator == "{" {
                let close = self.closing(end);
                let name = (head..end)
                    .skip_while(|&index| self.word(index) != "module")
                    .skip(1)
                    .map(|index| self.word(index))
                    .collect::<String>();
                symbols.push(self.symbol(name, SymbolKind::Module, head, close, Vec::new()));
                self.count += 1;
                at = close + 1;
                continue;
            }
            match (terminator, paren_at) {
                ("{", Some(paren)) => {
                    let close = self.closing(end);
                    symbols.push(self.symbol(
                        self.word(paren - 1).to_owned(),
                        SymbolKind::Method,
                        head,
                        close,
                        Vec::new(),
                    ));
                    self.count += 1;
                    at = close + 1;
                }
                ("{", None) => at = self.closing(end) + 1,
                (";", Some(paren)) => {
                    symbols.push(self.symbol(
                        self.word(paren - 1).to_owned(),
                        SymbolKind::Method,
                        head,
                        end,
                        Vec::new(),
                    ));
                    self.count += 1;
                    at = end + 1;
                }
                _ => {
                    let statement_end = self.statement_end(head, to);
                    let mut segment = head;
                    let mut cursor = head;
                    let mut first = true;
                    while cursor <= statement_end {
                        let word = self.word(cursor);
                        if word == "<" {
                            cursor = self.generic_end(cursor) + 1;
                            continue;
                        }
                        if matches!(word, "=" | "," | ";") || cursor == statement_end {
                            let name_at = cursor.checked_sub(1).filter(|&name| name >= segment && self.is_word(name));
                            let words_in = (segment..cursor).filter(|&index| self.is_word(index)).count();
                            if let Some(name) = name_at
                                && (!first || words_in >= 2)
                            {
                                symbols.push(self.symbol(
                                    self.word(name).to_owned(),
                                    SymbolKind::Field,
                                    head,
                                    name,
                                    Vec::new(),
                                ));
                                self.count += 1;
                            }
                            first = false;
                            if word == "=" {
                                cursor = self.skip_expression(cursor + 1, to);
                            }
                            if self.word(cursor) == "," {
                                segment = cursor + 1;
                                cursor += 1;
                                continue;
                            }
                            break;
                        }
                        match word {
                            "(" | "[" | "{" => cursor = self.closing(cursor) + 1,
                            _ => cursor += 1,
                        }
                    }
                    at = statement_end + 1;
                }
            }
        }
        symbols
    }

    fn after(&self, symbol: &Symbol, to: usize) -> usize {
        (0..to).find(|&index| self.tokens[index].start >= symbol.end).unwrap_or(to)
    }

    fn statement_end(&self, from: usize, to: usize) -> usize {
        let mut at = from;
        while at < to {
            match self.word(at) {
                ";" => return at,
                "{" | "(" | "[" => at = self.closing(at) + 1,
                _ => at += 1,
            }
        }
        to
    }

    fn skip_to_semicolon(&self, from: usize, to: usize) -> usize {
        (from..to).find(|&index| self.word(index) == ";").unwrap_or(to)
    }

    fn skip_expression(&self, from: usize, to: usize) -> usize {
        let mut at = from;
        while at < to {
            match self.word(at) {
                "," | ";" => return at,
                "{" | "(" | "[" => at = self.closing(at) + 1,
                "<" if at > 0 && self.word(at - 1).chars().next().is_some_and(char::is_uppercase) => {
                    at = self.generic_end(at) + 1;
                }
                _ => at += 1,
            }
        }
        to
    }

    fn generic_end(&self, at: usize) -> usize {
        let mut depth = 0;
        for index in at..self.tokens.len() {
            match self.word(index) {
                "<" => depth += 1,
                ">" => {
                    depth -= 1;
                    if depth == 0 {
                        return index;
                    }
                }
                ";" | "{" | "(" | "=" | ")" => return at,
                _ => {}
            }
        }
        at
    }

    fn module_declaration(&self, head: usize, end: usize) -> Option<usize> {
        (head..end).find(|&index| self.word(index) == "module" && self.tokens[index].class == Class::Keyword)
    }

    fn type_declaration(&mut self, head: usize, end: usize, depth: usize) -> Option<Symbol> {
        let keyword = (head..end).find(|&index| {
            let word = self.word(index);
            let keyword = self.tokens[index].class == Class::Keyword;
            keyword
                && matches!(word, "class" | "interface" | "enum" | "record")
                && (index == 0 || self.word(index - 1) != ".")
        })?;
        let name = self.word(keyword + 1).to_owned();
        if !self.is_word(keyword + 1) || self.word(end) != "{" {
            return None;
        }
        let annotation = keyword > 0 && self.word(keyword - 1) == "@";
        let kind = match self.word(keyword) {
            "interface" => SymbolKind::Interface,
            "enum" => SymbolKind::Enum,
            _ if annotation => SymbolKind::Interface,
            _ => SymbolKind::Class,
        };
        let close = self.closing(end);
        let mut children = Vec::new();
        if self.word(keyword) == "record"
            && let Some(paren) = (keyword..end).find(|&index| self.word(index) == "(")
        {
            children.extend(self.components(paren));
        }
        if depth < MAX_DEPTH {
            if kind == SymbolKind::Enum {
                let (constants, rest) = self.enum_constants(end + 1, close);
                children.extend(constants);
                children.extend(self.body(rest, close, depth + 1));
            } else {
                children.extend(self.body(end + 1, close, depth + 1));
            }
        }
        Some(self.symbol(name, kind, head, close, children))
    }

    fn components(&mut self, paren: usize) -> Vec<Symbol> {
        let close = self.closing(paren);
        let mut found = Vec::new();
        let mut at = paren + 1;
        while at < close {
            match self.word(at) {
                "(" | "[" => at = self.closing(at),
                "<" => {
                    let mut depth = 0;
                    while at < close {
                        match self.word(at) {
                            "<" => depth += 1,
                            ">" => depth -= 1,
                            _ => {}
                        }
                        if depth == 0 {
                            break;
                        }
                        at += 1;
                    }
                }
                "," if self.is_word(at - 1) => {
                    found.push(self.symbol(
                        self.word(at - 1).to_owned(),
                        SymbolKind::Field,
                        at - 1,
                        at - 1,
                        Vec::new(),
                    ));
                }
                _ => {}
            }
            at += 1;
        }
        if close > paren + 1 && self.is_word(close - 1) {
            found.push(self.symbol(
                self.word(close - 1).to_owned(),
                SymbolKind::Field,
                close - 1,
                close - 1,
                Vec::new(),
            ));
        }
        found
    }

    fn enum_constants(&mut self, from: usize, to: usize) -> (Vec<Symbol>, usize) {
        let mut constants = Vec::new();
        let mut at = from;
        while at < to {
            match self.word(at) {
                ";" => return (constants, at + 1),
                "," => at += 1,
                "@" => at += 2,
                _ if self.is_word(at) => {
                    let start = at;
                    let mut end = at;
                    at += 1;
                    if self.word(at) == "(" {
                        end = self.closing(at);
                        at = end + 1;
                    }
                    if self.word(at) == "{" {
                        end = self.closing(at);
                        at = end + 1;
                    }
                    constants.push(self.symbol(
                        self.word(start).to_owned(),
                        SymbolKind::Constant,
                        start,
                        end,
                        Vec::new(),
                    ));
                    self.count += 1;
                }
                _ => at += 1,
            }
        }
        (constants, to)
    }

    fn symbol(&self, name: String, kind: SymbolKind, first: usize, last: usize, children: Vec<Symbol>) -> Symbol {
        let start = self.tokens.get(first).map_or(0, |token| token.start);
        Symbol { name, kind, start, end: self.end_of(last).max(start), children }
    }
}

pub fn outline(text: &str) -> Vec<Symbol> {
    let (tokens, _) = tokens(text);
    let tokens: Vec<Token> = tokens.into_iter().filter(|token| !matches!(token.raw, Raw::Comment | Raw::Doc)).collect();
    let mut parser = Parser { text, count: 0, tokens };
    let length = parser.tokens.len();
    parser.body(0, length, 0)
}

pub fn outline_json(symbols: &[Symbol]) -> String {
    let mut out = String::from("[");
    write_symbols(&mut out, symbols);
    out.push(']');
    out
}

fn write_symbols(out: &mut String, symbols: &[Symbol]) {
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        quote(out, &symbol.name);
        out.push_str(&format!(
            ",\"kind\":\"{}\",\"start\":{},\"end\":{},\"children\":[",
            symbol.kind.name(),
            symbol.start,
            symbol.end
        ));
        write_symbols(out, &symbol.children);
        out.push_str("]}");
    }
}

pub fn quote(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
    pub detail: Option<String>,
}

pub fn complete(text: &str, caret: usize) -> (usize, Vec<Item>) {
    let caret = text.floor_char_boundary(caret);
    let mut from = caret;
    while from > 0 {
        let Some(c) = text[..from].chars().next_back() else { break };
        if c.is_alphanumeric() || c == '_' || c == '$' {
            from -= c.len_utf8();
        } else {
            break;
        }
    }
    let typed = &text[from..caret];
    if typed.is_empty() {
        return (caret, Vec::new());
    }
    let after_dot = text[..from].trim_end().ends_with('.');
    let lower = typed.to_lowercase();
    let matches = |word: &str| word.len() > typed.len() && word.to_lowercase().starts_with(&lower);
    let mut items: Vec<Item> = Vec::new();
    if !after_dot {
        let module = lexer::is_module_declaration(text);
        let words = KEYWORDS
            .iter()
            .chain(["record", "sealed", "non-sealed", "permits", "var", "yield", "when"].iter())
            .chain(if module { MODULE_WORDS } else { &[] }.iter());
        for keyword in words.filter(|keyword| matches(keyword)) {
            items.push(Item {
                label: (*keyword).to_owned(),
                kind: "keyword",
                insert: (*keyword).to_owned(),
                detail: None,
            });
        }
    }
    let (tokens, _) = tokens(text);
    let mut names: Vec<(&str, &'static str)> = Vec::new();
    for token in tokens.iter().filter(|token| token.raw == Raw::Word && (token.end < from || token.start > caret)) {
        let word = &text[token.start..token.end];
        let kind = match token.class {
            Class::Keyword => continue,
            Class::Type | Class::Annotation => "class",
            Class::Function => "method",
            Class::Constant => "constant",
            Class::Plain => "variable",
        };
        if matches(word) && !names.iter().any(|(known, _)| *known == word) {
            names.push((word, kind));
        }
    }
    for (name, kind) in names.into_iter().take(2_000 - items.len().min(2_000)) {
        items.push(Item { label: name.to_owned(), kind, insert: name.to_owned(), detail: None });
    }
    items.truncate(2_000);
    (from, items)
}

pub fn complete_json(from: usize, items: &[Item]) -> String {
    let mut out = format!("{{\"from\":{from},\"items\":[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        quote(&mut out, &item.label);
        out.push_str(&format!(",\"kind\":\"{}\",\"insert\":", item.kind));
        quote(&mut out, &item.insert);
        if let Some(detail) = &item.detail {
            out.push_str(",\"detail\":");
            quote(&mut out, detail);
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

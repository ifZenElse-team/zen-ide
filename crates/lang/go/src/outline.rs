use zen_syntax::{OutlineItem, Outliner, SuppressedLines, SymbolKind};

use crate::{Token, tokenize};

#[derive(Clone, Copy, Debug, Default)]
pub struct GoOutliner;

impl Outliner for GoOutliner {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn outline(&self, text: &str) -> Vec<OutlineItem> {
        let mut items = Parser::new(text).parse();
        if !text.is_ascii() {
            let mut chars = vec![0; text.len() + 1];
            let mut count = 0;
            for (at, c) in text.char_indices() {
                chars[at..at + c.len_utf8()].fill(count);
                count += 1;
            }
            chars[text.len()] = count;
            to_chars(&mut items, &chars);
        }
        items
    }

    fn reads_suppressions(&self) -> bool {
        true
    }

    fn suppressions(&self, text: &str) -> Vec<SuppressedLines> {
        crate::nolint::suppressions(text)
    }
}

fn to_chars(items: &mut [OutlineItem], chars: &[usize]) {
    for item in items {
        item.start = chars[item.start];
        item.end = chars[item.end];
        item.selection_start = chars[item.selection_start];
        to_chars(&mut item.children, chars);
    }
}

const DECLARATIONS: &[&str] = &["func", "type", "const", "var", "import", "package"];

#[derive(Clone, Copy, Debug)]
struct Tok {
    token: Token,
    start: usize,
    end: usize,
    line_start: bool,
    column_zero: bool,
}

struct Method {
    receiver: String,
    item: OutlineItem,
}

enum Found {
    Item(OutlineItem),
    Method(Method),
}

struct Parser<'a> {
    text: &'a str,
    toks: Vec<Tok>,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        let mut toks = Vec::new();
        let mut line_start = true;
        for (token, start, end) in tokenize(text) {
            match token {
                Token::Whitespace => line_start |= text[start..end].contains('\n'),
                Token::Comment => line_start |= text[start..end].contains('\n') || text[start..end].starts_with("//"),
                _ => {
                    let column_zero = start == 0 || text.as_bytes()[start - 1] == b'\n';
                    toks.push(Tok { token, start, end, line_start, column_zero });
                    line_start = false;
                }
            }
        }
        Self { text, toks }
    }

    fn word(&self, at: usize) -> &'a str {
        self.toks.get(at).map_or("", |tok| &self.text[tok.start..tok.end])
    }

    fn is(&self, at: usize, punct: char) -> bool {
        self.toks.get(at).is_some_and(|tok| tok.token == Token::Punct(punct))
    }

    fn is_ident(&self, at: usize) -> bool {
        self.toks.get(at).is_some_and(|tok| tok.token == Token::Ident)
    }

    fn is_open(&self, at: usize) -> bool {
        self.is(at, '(') || self.is(at, '[') || self.is(at, '{')
    }

    fn is_declaration_start(&self, at: usize) -> bool {
        self.toks.get(at).is_some_and(|tok| tok.column_zero) && DECLARATIONS.contains(&self.word(at))
    }

    fn end_of(&self, at: usize) -> usize {
        self.toks.get(at).map_or(self.text.len(), |tok| tok.end)
    }

    fn start_of(&self, at: usize) -> usize {
        self.toks.get(at).map_or(self.text.len(), |tok| tok.start)
    }

    fn skip_balanced(&self, open: usize) -> usize {
        let mut depth = 0usize;
        let mut at = open;
        while at < self.toks.len() {
            if depth > 0 && self.is_declaration_start(at) {
                return at;
            }
            match self.toks[at].token {
                Token::Punct('(' | '[' | '{') => depth += 1,
                Token::Punct(')' | ']' | '}') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return at + 1;
                    }
                }
                _ => {}
            }
            at += 1;
        }
        at
    }

    fn ends_statement(&self, at: usize) -> bool {
        self.toks.get(at).is_some_and(|tok| {
            matches!(
                tok.token,
                Token::Ident | Token::Number | Token::String | Token::Rune | Token::Punct(')' | ']' | '}')
            )
        })
    }

    fn breaks_before(&self, at: usize) -> bool {
        at >= self.toks.len()
            || self.is(at, ';')
            || self.is(at, ')')
            || self.is(at, '}')
            || self.is_declaration_start(at)
            || (at > 0 && self.toks[at].line_start && self.ends_statement(at - 1))
    }

    fn rest_end(&self, at: usize) -> usize {
        if self.breaks_before(at) { at } else { self.statement_end(at, true) }
    }

    fn statement_end(&self, from: usize, stop_at_close: bool) -> usize {
        let mut at = from;
        while at < self.toks.len() {
            if at > from && (self.is_declaration_start(at) || (self.toks[at].line_start && self.ends_statement(at - 1)))
            {
                break;
            }
            if self.is(at, ';') || (stop_at_close && (self.is(at, ')') || self.is(at, '}'))) {
                break;
            }
            at = if self.is_open(at) { self.skip_balanced(at) } else { at + 1 };
        }
        at
    }

    fn source(&self, from: usize, to: usize) -> Option<String> {
        if from >= to {
            return None;
        }
        let text = &self.text[self.start_of(from)..self.end_of(to - 1)];
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!collapsed.is_empty()).then_some(collapsed)
    }

    fn parse(&self) -> Vec<OutlineItem> {
        let mut found = Vec::new();
        let mut at = 0;
        while at < self.toks.len() {
            at = match self.word(at) {
                "func" if self.toks[at].line_start => self.func(at, &mut found),
                "type" if self.toks[at].line_start => self.declaration(at, &mut found, Self::type_spec),
                "const" if self.toks[at].line_start => self.declaration(at, &mut found, |parser, at, start, found| {
                    parser.value_spec(at, start, SymbolKind::Constant, found)
                }),
                "var" if self.toks[at].line_start => self.declaration(at, &mut found, |parser, at, start, found| {
                    parser.value_spec(at, start, SymbolKind::Variable, found)
                }),
                _ if self.is_open(at) => self.skip_balanced(at),
                _ => at + 1,
            };
        }
        nest(found)
    }

    fn func(&self, at: usize, found: &mut Vec<Found>) -> usize {
        let start = self.toks[at].start;
        let mut next = at + 1;
        let mut receiver = None;
        if self.is(next, '(') {
            let close = self.skip_balanced(next);
            receiver = Some(self.receiver(next + 1, close.saturating_sub(1).max(next + 1)));
            next = close;
        }
        if !self.is_ident(next) {
            return next.max(at + 1);
        }
        let name = next;
        let mut body = None;
        let mut end = name + 1;
        while end < self.toks.len() {
            if self.is(end, '{') {
                body = Some(end);
                break;
            }
            if self.toks[end].line_start || self.is(end, ';') {
                break;
            }
            end = if self.is(end, '(') || self.is(end, '[') { self.skip_balanced(end) } else { end + 1 };
        }
        let signature = self.source(name + 1, body.unwrap_or(end));
        let after = body.map_or(end, |open| self.skip_balanced(open));
        let mut item = OutlineItem {
            name: self.word(name).to_owned(),
            detail: signature,
            kind: SymbolKind::Function,
            start,
            end: self.end_of(after.max(name + 1) - 1),
            selection_start: self.toks[name].start,
            children: Vec::new(),
        };
        match receiver {
            Some((type_text, type_name)) => {
                item.kind = SymbolKind::Method;
                item.detail = type_text;
                found.push(Found::Method(Method { receiver: type_name, item }));
            }
            None => found.push(Found::Item(item)),
        }
        after
    }

    fn receiver(&self, from: usize, to: usize) -> (Option<String>, String) {
        let to = to.min(self.toks.len());
        let named = self.is_ident(from) && from + 1 < to && !self.is(from + 1, '[') && !self.is(from + 1, '.');
        let type_from = if named { from + 1 } else { from };
        let mut type_name = String::new();
        let mut at = type_from;
        while at < to && !self.is(at, '[') {
            if self.is_ident(at) {
                type_name = self.word(at).to_owned();
            }
            at += 1;
        }
        (self.source(type_from, to), type_name)
    }

    fn declaration(
        &self,
        at: usize,
        found: &mut Vec<Found>,
        spec: impl Fn(&Self, usize, usize, &mut Vec<Found>) -> usize,
    ) -> usize {
        let keyword_start = self.toks[at].start;
        let mut next = at + 1;
        if !self.is(next, '(') {
            return if self.is_ident(next) { spec(self, next, keyword_start, found) } else { next };
        }
        next += 1;
        while next < self.toks.len() {
            if self.is(next, ')') {
                return next + 1;
            }
            if self.is_declaration_start(next) {
                return next;
            }
            if self.is_ident(next) {
                let start = self.toks[next].start;
                next = spec(self, next, start, found).max(next + 1);
            } else {
                next += 1;
            }
        }
        next
    }

    fn type_spec(&self, name: usize, start: usize, found: &mut Vec<Found>) -> usize {
        let mut at = name + 1;
        if self.is(at, '[') && self.is_ident(at + 1) && !self.is(at + 2, ']') {
            at = self.skip_balanced(at);
        }
        if self.is(at, '=') {
            at += 1;
        }
        let mut item = OutlineItem {
            name: self.word(name).to_owned(),
            detail: None,
            kind: SymbolKind::Type,
            start,
            end: self.toks[name].end,
            selection_start: self.toks[name].start,
            children: Vec::new(),
        };
        let compound = match self.word(at) {
            "struct" if self.is(at + 1, '{') => Some(SymbolKind::Struct),
            "interface" if self.is(at + 1, '{') => Some(SymbolKind::Interface),
            _ => None,
        };
        let after = if let Some(kind) = compound {
            item.kind = kind;
            let close = self.skip_balanced(at + 1);
            item.children = if kind == SymbolKind::Struct {
                self.fields(at + 2, close)
            } else {
                self.interface_methods(at + 2, close)
            };
            close
        } else {
            let end = self.rest_end(at);
            item.detail = self.source(at, end);
            end
        };
        item.end = self.end_of(after.max(name + 1) - 1);
        found.push(Found::Item(item));
        after
    }

    fn members(&self, from: usize, close: usize) -> Vec<(usize, usize)> {
        let limit = if self.is(close.saturating_sub(1), '}') { close - 1 } else { close };
        let mut members = Vec::new();
        let mut at = from;
        while at < limit {
            if self.is(at, ';') {
                at += 1;
                continue;
            }
            let end = self.statement_end(at, true).min(limit).max(at + 1);
            members.push((at, end));
            at = end;
        }
        members
    }

    fn fields(&self, from: usize, close: usize) -> Vec<OutlineItem> {
        let mut fields = Vec::new();
        for (first, end) in self.members(from, close) {
            let field_end = self.end_of(end - 1);
            let mut names = Vec::new();
            let mut at = first;
            while self.is_ident(at) && at < end {
                names.push(at);
                if self.is(at + 1, ',') {
                    at += 2;
                } else {
                    at += 1;
                    break;
                }
            }
            let embedded = names.len() == 1
                && (at >= end || self.is(at, '.') || self.toks.get(at).is_some_and(|tok| tok.token == Token::String));
            if names.is_empty() || embedded {
                let type_end = self.type_end(first, end);
                let name = (first..type_end).rev().find(|&at| self.is_ident(at));
                if let Some(name) = name {
                    fields.push(OutlineItem {
                        name: self.word(name).to_owned(),
                        detail: None,
                        kind: SymbolKind::Field,
                        start: self.toks[first].start,
                        end: field_end,
                        selection_start: self.toks[name].start,
                        children: Vec::new(),
                    });
                }
                continue;
            }
            let detail = self.source(at, self.type_end(at, end));
            for name in names {
                fields.push(OutlineItem {
                    name: self.word(name).to_owned(),
                    detail: detail.clone(),
                    kind: SymbolKind::Field,
                    start: self.toks[name].start,
                    end: field_end,
                    selection_start: self.toks[name].start,
                    children: Vec::new(),
                });
            }
        }
        fields
    }

    fn type_end(&self, from: usize, end: usize) -> usize {
        (from..end).find(|&at| self.toks[at].token == Token::String).unwrap_or(end)
    }

    fn interface_methods(&self, from: usize, close: usize) -> Vec<OutlineItem> {
        self.members(from, close)
            .into_iter()
            .filter(|&(first, _)| self.is_ident(first) && self.is(first + 1, '('))
            .map(|(first, end)| OutlineItem {
                name: self.word(first).to_owned(),
                detail: self.source(first + 1, end),
                kind: SymbolKind::Method,
                start: self.toks[first].start,
                end: self.end_of(end - 1),
                selection_start: self.toks[first].start,
                children: Vec::new(),
            })
            .collect()
    }

    fn value_spec(&self, first: usize, start: usize, kind: SymbolKind, found: &mut Vec<Found>) -> usize {
        let mut names = Vec::new();
        let mut at = first;
        while self.is_ident(at) {
            names.push(at);
            if self.is(at + 1, ',') {
                at += 2;
            } else {
                at += 1;
                break;
            }
        }
        let end = self.rest_end(at);
        let type_end = (at..end).find(|&at| self.is(at, '=')).unwrap_or(end);
        let detail = self.source(at, type_end);
        let item_end = self.end_of(end.max(first + 1) - 1);
        for (index, name) in names.into_iter().enumerate() {
            if self.word(name) == "_" {
                continue;
            }
            found.push(Found::Item(OutlineItem {
                name: self.word(name).to_owned(),
                detail: detail.clone(),
                kind,
                start: if index == 0 { start } else { self.toks[name].start },
                end: item_end,
                selection_start: self.toks[name].start,
                children: Vec::new(),
            }));
        }
        end
    }
}

fn nest(found: Vec<Found>) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    let mut methods = Vec::new();
    for entry in found {
        match entry {
            Found::Item(item) => items.push(Some(item)),
            Found::Method(method) => {
                methods.push((items.len(), method));
                items.push(None);
            }
        }
    }
    for (slot, method) in methods {
        let owner = items.iter_mut().flatten().find(|item| {
            item.name == method.receiver
                && matches!(item.kind, SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Type)
        });
        match owner {
            Some(owner) => owner.children.push(method.item),
            None => items[slot] = Some(method.item),
        }
    }
    items.into_iter().flatten().collect()
}

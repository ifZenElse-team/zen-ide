use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
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

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "case", "esac", "for", "select", "while", "until", "do", "done", "in",
    "function", "time", "coproc", "[[", "]]",
];

const BUILTINS: &[&str] = &[
    "echo", "printf", "cd", "export", "local", "readonly", "declare", "typeset", "unset", "set", "shift", "source",
    ".", "eval", "exec", "exit", "return", "trap", "read", "test", "true", "false", "alias",
];

const DECLARATIONS: &[&str] = &["export", "local", "readonly", "declare", "typeset"];

const PREFIXES: &[&str] = &["sudo", "command", "builtin", "nohup", "exec", "!", "{"];

pub fn highlight(text: &str) -> Vec<Span> {
    let mut lexer = Lexer {
        text,
        bytes: text.as_bytes(),
        spans: Vec::new(),
        stack: Vec::new(),
        heredocs: Vec::new(),
        command: true,
        in_word: false,
        declaration: false,
        loop_variable: false,
        expect_in: false,
        function_name: false,
    };
    lexer.run();
    lexer.spans
}

#[derive(Clone, Copy, Debug)]
enum Context {
    Quote,
    Substitution { command: bool },
    Backtick { command: bool },
    Subshell,
    Array,
    Arithmetic { marked: bool, depth: usize, command: bool },
    Case { awaiting_in: bool, pattern: bool },
}

struct Heredoc {
    delimiter: String,
    strip_tabs: bool,
    quoted: bool,
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    spans: Vec<Span>,
    stack: Vec<Context>,
    heredocs: Vec<Heredoc>,
    command: bool,
    in_word: bool,
    declaration: bool,
    loop_variable: bool,
    expect_in: bool,
    function_name: bool,
}

impl Lexer<'_> {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        let end = end.min(self.text.len());
        if end <= start {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if start < last.end {
                return;
            }
            if last.end == start && last.kind == kind {
                last.end = end;
                return;
            }
        }
        self.spans.push(Span { start, end, kind });
    }

    fn next_char(&self, at: usize) -> usize {
        self.text[at..].chars().next().map_or(self.text.len(), |c| at + c.len_utf8())
    }

    fn after_escape(&self, at: usize) -> usize {
        if at + 1 >= self.text.len() { self.text.len() } else { self.next_char(at + 1) }
    }

    fn begin_word(&mut self) -> Option<bool> {
        if self.in_word {
            return None;
        }
        self.in_word = true;
        let command = self.command;
        self.command = false;
        Some(command)
    }

    fn end_command(&mut self) {
        self.command = true;
        self.in_word = false;
        self.declaration = false;
        self.loop_variable = false;
        self.function_name = false;
        if !matches!(self.stack.last(), Some(Context::Case { awaiting_in: true, .. })) {
            self.expect_in = false;
        }
    }

    fn run(&mut self) {
        let mut at = 0;
        while at < self.bytes.len() {
            let next = match self.stack.last().copied() {
                Some(Context::Quote) => self.quoted(at),
                Some(Context::Arithmetic { marked, depth, command }) => self.arithmetic(at, marked, depth, command),
                _ => self.token(at),
            };
            at = if next > at { next } else { self.next_char(at) };
        }
    }

    fn quoted(&mut self, at: usize) -> usize {
        let mut index = at;
        while index < self.bytes.len() {
            match self.bytes[index] {
                b'"' => {
                    self.push(at, index + 1, Kind::String);
                    self.stack.pop();
                    return index + 1;
                }
                b'\\' => index = self.after_escape(index),
                b'$' if self.dollar_form(index, true).is_some() => {
                    self.push(at, index, Kind::String);
                    return self.dollar(index, true).unwrap_or(index + 1);
                }
                b'`' => {
                    self.push(at, index, Kind::String);
                    self.push(index, index + 1, Kind::Punctuation);
                    self.stack.push(Context::Backtick { command: self.command });
                    self.command = true;
                    self.in_word = false;
                    return index + 1;
                }
                _ => index += 1,
            }
        }
        self.push(at, self.text.len(), Kind::String);
        self.text.len()
    }

    fn arithmetic(&mut self, at: usize, marked: bool, depth: usize, command: bool) -> usize {
        let c = self.bytes[at];
        let set_depth = |lexer: &mut Self, depth: usize| {
            if let Some(Context::Arithmetic { depth: current, .. }) = lexer.stack.last_mut() {
                *current = depth;
            }
        };
        match c {
            b'(' => {
                set_depth(self, depth + 1);
                at + 1
            }
            b')' if depth > 0 => {
                set_depth(self, depth - 1);
                at + 1
            }
            b')' => {
                self.stack.pop();
                self.command = command;
                let end = if self.bytes.get(at + 1) == Some(&b')') { at + 2 } else { at + 1 };
                if marked {
                    self.push(at, end, Kind::Punctuation);
                }
                self.in_word = marked;
                end
            }
            b'0'..=b'9' => {
                let length = self.bytes[at..].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'#').count();
                self.push(at, at + length, Kind::Number);
                at + length
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let length = name_length(&self.bytes[at..]);
                self.push(at, at + length, Kind::Constant);
                at + length
            }
            b'$' => self.dollar(at, false).unwrap_or(at + 1),
            b'\\' => self.after_escape(at),
            _ => self.next_char(at),
        }
    }

    fn token(&mut self, at: usize) -> usize {
        let c = self.bytes[at];
        match c {
            b'\n' => {
                self.end_command();
                if self.heredocs.is_empty() { at + 1 } else { self.heredoc_bodies(at + 1) }
            }
            b' ' | b'\t' | b'\r' => {
                self.in_word = false;
                at + 1
            }
            b'\\' if self.bytes.get(at + 1) == Some(&b'\n') => at + 2,
            b'#' if !self.in_word => {
                let end = self.text[at..].find('\n').map_or(self.text.len(), |newline| at + newline);
                self.push(at, end, Kind::Comment);
                end
            }
            b'"' => {
                self.begin_word();
                self.push(at, at + 1, Kind::String);
                self.stack.push(Context::Quote);
                at + 1
            }
            b'\'' => {
                self.begin_word();
                let end = self.text[at + 1..].find('\'').map_or(self.text.len(), |close| at + 1 + close + 1);
                self.push(at, end, Kind::String);
                end
            }
            b'`' => {
                if let Some(Context::Backtick { command }) = self.stack.last().copied() {
                    self.stack.pop();
                    self.push(at, at + 1, Kind::Punctuation);
                    self.command = command;
                    self.in_word = true;
                } else {
                    self.begin_word();
                    self.push(at, at + 1, Kind::Punctuation);
                    self.stack.push(Context::Backtick { command: self.command });
                    self.command = true;
                    self.in_word = false;
                }
                at + 1
            }
            b'$' if self.dollar_form(at, false).is_some() => {
                self.begin_word();
                self.dollar(at, false).unwrap_or(at + 1)
            }
            b'(' => self.open_paren(at),
            b')' => self.close_paren(at),
            b';' | b'&' | b'|' | b'<' | b'>' => self.operator(at, 0),
            b'0'..=b'9' if !self.in_word && self.fd_redirect(at).is_some() => {
                let digits = self.fd_redirect(at).unwrap_or(0);
                self.operator(at, digits)
            }
            _ => self.word(at),
        }
    }

    fn dollar_form(&self, at: usize, quoted: bool) -> Option<usize> {
        match self.bytes.get(at + 1)? {
            b'(' | b'{' => Some(2),
            b'\'' | b'"' if !quoted => Some(2),
            b if b.is_ascii_alphanumeric() || b"_@?#$!*-".contains(b) => Some(1),
            _ => None,
        }
    }

    fn dollar(&mut self, at: usize, quoted: bool) -> Option<usize> {
        self.dollar_form(at, quoted)?;
        let rest = &self.text[at..];
        if rest.starts_with("$((") {
            self.push(at, at + 3, Kind::Punctuation);
            self.stack.push(Context::Arithmetic { marked: true, depth: 0, command: self.command });
            return Some(at + 3);
        }
        if rest.starts_with("$(") {
            self.push(at, at + 2, Kind::Punctuation);
            self.stack.push(Context::Substitution { command: self.command });
            self.command = true;
            self.in_word = false;
            return Some(at + 2);
        }
        if rest.starts_with("$'") {
            let mut index = at + 2;
            let end = loop {
                match self.bytes.get(index) {
                    None => break self.text.len(),
                    Some(b'\\') => index = self.after_escape(index),
                    Some(b'\'') => break index + 1,
                    Some(_) => index += 1,
                }
            };
            self.push(at, end, Kind::String);
            return Some(end);
        }
        if rest.starts_with("$\"") {
            self.push(at, at + 2, Kind::String);
            self.stack.push(Context::Quote);
            return Some(at + 2);
        }
        let length = variable_length(rest)?;
        self.push(at, at + length, Kind::Constant);
        Some(at + length)
    }

    fn fd_redirect(&self, at: usize) -> Option<usize> {
        let digits = self.bytes[at..].iter().take_while(|b| b.is_ascii_digit()).count();
        matches!(self.bytes.get(at + digits), Some(b'<' | b'>')).then_some(digits)
    }

    fn open_paren(&mut self, at: usize) -> usize {
        if let Some(Context::Case { pattern: true, .. }) = self.stack.last() {
            return at + 1;
        }
        if self.in_word && at > 0 && self.bytes[at - 1] == b'=' {
            self.stack.push(Context::Array);
            self.command = false;
            self.in_word = false;
            return at + 1;
        }
        if !self.in_word && self.bytes.get(at + 1) == Some(&b'(') {
            self.begin_word();
            self.stack.push(Context::Arithmetic { marked: false, depth: 0, command: false });
            return at + 2;
        }
        self.stack.push(Context::Subshell);
        self.command = true;
        self.in_word = false;
        at + 1
    }

    fn close_paren(&mut self, at: usize) -> usize {
        match self.stack.last().copied() {
            Some(Context::Substitution { command }) => {
                self.stack.pop();
                self.push(at, at + 1, Kind::Punctuation);
                self.command = command;
                self.in_word = true;
            }
            Some(Context::Case { pattern: true, .. }) => {
                if let Some(Context::Case { pattern, .. }) = self.stack.last_mut() {
                    *pattern = false;
                }
                self.push(at, at + 1, Kind::Punctuation);
                self.end_command();
            }
            Some(Context::Array) => {
                self.stack.pop();
                self.command = true;
                self.in_word = true;
            }
            Some(Context::Subshell) => {
                self.stack.pop();
                self.command = false;
                self.in_word = false;
            }
            _ => self.in_word = false,
        }
        at + 1
    }

    fn operator(&mut self, at: usize, digits: usize) -> usize {
        let start = at;
        let at = at + digits;
        let rest = &self.text[at..];
        if digits == 0 && (rest.starts_with("<(") || rest.starts_with(">(")) {
            self.begin_word();
            self.push(at, at + 2, Kind::Punctuation);
            self.stack.push(Context::Substitution { command: self.command });
            self.command = true;
            self.in_word = false;
            return at + 2;
        }
        const OPERATORS: &[&str] = &[
            ";;&", ";;", ";&", ";", "||", "|&", "|", "&&", "&>>", "&>", "&", "<<<", "<<-", "<<", "<>", "<&", "<", ">>",
            ">&", ">|", ">",
        ];
        let operator = OPERATORS.iter().find(|operator| rest.starts_with(**operator)).copied().unwrap_or(">");
        let mut end = at + operator.len().min(rest.len().max(1));
        match operator {
            ";;&" | ";;" | ";&" => {
                if let Some(Context::Case { pattern, awaiting_in: false }) = self.stack.last_mut() {
                    *pattern = true;
                }
                self.end_command();
            }
            ";" | "||" | "|&" | "|" | "&&" | "&" => self.end_command(),
            ">&" | "<&" => {
                let target = self.bytes[end..].iter().take_while(|b| b.is_ascii_digit()).count();
                end += if target > 0 { target } else { usize::from(self.bytes.get(end) == Some(&b'-')) };
                self.in_word = false;
            }
            _ => self.in_word = false,
        }
        self.push(start, end, Kind::Punctuation);
        if operator == "<<" || operator == "<<-" {
            return self.heredoc_start(end, operator == "<<-");
        }
        end
    }

    fn heredoc_start(&mut self, at: usize, strip_tabs: bool) -> usize {
        let mut index = at;
        while matches!(self.bytes.get(index), Some(b' ' | b'\t')) {
            index += 1;
        }
        let start = index;
        let mut delimiter = String::new();
        let mut quoted = false;
        let mut quote: Option<char> = None;
        while index < self.text.len() {
            let c = self.text[index..].chars().next().unwrap_or(' ');
            if let Some(open) = quote {
                if c == '\n' {
                    break;
                }
                index += c.len_utf8();
                if c == open {
                    quote = None;
                } else {
                    delimiter.push(c);
                }
                continue;
            }
            match c {
                '\'' | '"' => {
                    quoted = true;
                    quote = Some(c);
                    index += 1;
                }
                '\\' => {
                    quoted = true;
                    index += 1;
                    if let Some(next) = self.text[index..].chars().next().filter(|next| *next != '\n') {
                        delimiter.push(next);
                        index += next.len_utf8();
                    }
                }
                ' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | '<' | '>' | '(' | ')' => break,
                _ => {
                    delimiter.push(c);
                    index += c.len_utf8();
                }
            }
        }
        if index > start {
            self.push(start, index, Kind::String);
            self.heredocs.push(Heredoc { delimiter, strip_tabs, quoted });
        }
        self.in_word = false;
        index
    }

    fn heredoc_bodies(&mut self, mut at: usize) -> usize {
        for heredoc in std::mem::take(&mut self.heredocs) {
            let body = at;
            loop {
                if at >= self.text.len() {
                    self.heredoc_body(body, self.text.len(), heredoc.quoted);
                    return self.text.len();
                }
                let line_end = self.text[at..].find('\n').map_or(self.text.len(), |newline| at + newline);
                let mut line = &self.text[at..line_end];
                if heredoc.strip_tabs {
                    line = line.trim_start_matches('\t');
                }
                if line.strip_suffix('\r').unwrap_or(line) == heredoc.delimiter {
                    self.heredoc_body(body, at, heredoc.quoted);
                    self.push(at, line_end, Kind::String);
                    at = (line_end + 1).min(self.text.len());
                    break;
                }
                at = (line_end + 1).min(self.text.len());
                if line_end == self.text.len() {
                    at = self.text.len();
                }
            }
        }
        at
    }

    fn heredoc_body(&mut self, start: usize, end: usize, quoted: bool) {
        if quoted {
            self.push(start, end, Kind::String);
            return;
        }
        let mut chunk = start;
        let mut index = start;
        while index < end {
            match self.bytes[index] {
                b'\\' => index = self.after_escape(index).min(end),
                b'$' => {
                    let length = variable_length(&self.text[index..end]);
                    if let Some(length) = length {
                        self.push(chunk, index, Kind::String);
                        self.push(index, index + length, Kind::Constant);
                        index += length;
                        chunk = index;
                    } else {
                        index += 1;
                    }
                }
                _ => index += 1,
            }
        }
        self.push(chunk, end, Kind::String);
    }

    fn word(&mut self, at: usize) -> usize {
        let mut end = self.next_char(at).max(if self.bytes[at] == b'\\' { self.after_escape(at) } else { 0 });
        while end < self.bytes.len() {
            match self.bytes[end] {
                b' ' | b'\t' | b'\r' | b'\n' | b';' | b'&' | b'|' | b'<' | b'>' | b'(' | b')' | b'"' | b'\'' | b'`'
                | b'$' => break,
                b'\\' => end = self.after_escape(end),
                _ => end += 1,
            }
        }
        let end = end.min(self.text.len());
        let Some(command) = self.begin_word() else {
            return end;
        };
        let word = &self.text[at..end];
        let whole = self.bytes.get(end).is_none_or(|b| b" \t\r\n;&|<>()".contains(b));

        if let Some(Context::Case { pattern: true, .. }) = self.stack.last() {
            if whole && word == "esac" {
                self.stack.pop();
                self.push(at, end, Kind::Keyword);
            }
            return end;
        }
        if self.function_name {
            self.function_name = false;
            self.push(at, end, Kind::Function);
            return self.function_parens(end);
        }
        if self.loop_variable && whole {
            self.loop_variable = false;
            self.expect_in = true;
            self.push(at, end, Kind::Constant);
            return end;
        }
        if whole && word == "in" && self.expect_in {
            self.expect_in = false;
            if let Some(Context::Case { awaiting_in, pattern }) = self.stack.last_mut()
                && *awaiting_in
            {
                *awaiting_in = false;
                *pattern = true;
            }
            self.push(at, end, Kind::Keyword);
            return end;
        }
        if (command || self.declaration)
            && let Some(name) = assignment_name(word)
        {
            self.push(at, at + name, Kind::Constant);
            self.command = command;
            return at + name;
        }
        if word.starts_with('-') && word[1..].trim_start_matches('-').starts_with(|c: char| c.is_ascii_alphabetic()) {
            let option = word.find('=').unwrap_or(word.len());
            self.push(at, at + option, Kind::Attribute);
            self.command = command;
            return at + option;
        }
        if whole && word == "]]" {
            self.push(at, end, Kind::Keyword);
            return end;
        }
        if command && whole {
            if KEYWORDS.contains(&word) {
                self.push(at, end, Kind::Keyword);
                match word {
                    "if" | "then" | "else" | "elif" | "while" | "until" | "do" | "time" | "coproc" => {
                        self.command = true;
                    }
                    "case" => {
                        self.stack.push(Context::Case { awaiting_in: true, pattern: false });
                        self.expect_in = true;
                    }
                    "for" | "select" => self.loop_variable = true,
                    "function" => self.function_name = true,
                    "esac" => {
                        if let Some(Context::Case { .. }) = self.stack.last() {
                            self.stack.pop();
                        }
                    }
                    _ => {}
                }
                return end;
            }
            if self.parens_end(end).is_some() {
                self.push(at, end, Kind::Function);
                return self.function_parens(end);
            }
            if BUILTINS.contains(&word) {
                self.push(at, end, Kind::Function);
                self.declaration = DECLARATIONS.contains(&word);
                self.command = word == "exec";
                return end;
            }
            if PREFIXES.contains(&word) {
                self.command = true;
                return end;
            }
        }
        if whole && is_number(word) {
            self.push(at, end, Kind::Number);
        }
        end
    }

    fn parens_end(&self, at: usize) -> Option<usize> {
        let inner = self.text[at..].trim_start_matches([' ', '\t']).strip_prefix('(')?;
        let closed = inner.trim_start_matches([' ', '\t']);
        closed.starts_with(')').then(|| self.text.len() - closed.len() + 1)
    }

    fn function_parens(&mut self, at: usize) -> usize {
        self.command = true;
        match self.parens_end(at) {
            Some(end) => {
                self.in_word = false;
                end
            }
            None => at,
        }
    }
}

fn name_length(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count()
}

fn variable_length(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    match bytes.get(1)? {
        b'{' => {
            let mut depth = 0usize;
            for (index, b) in bytes.iter().enumerate().skip(1) {
                match b {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(index + 1);
                        }
                    }
                    b'\n' => return Some(index),
                    _ => {}
                }
            }
            Some(text.len())
        }
        b if b.is_ascii_alphabetic() || *b == b'_' => Some(1 + name_length(&bytes[1..])),
        b if b.is_ascii_digit() || b"@?#$!*-".contains(b) => Some(2),
        _ => None,
    }
}

fn assignment_name(word: &str) -> Option<usize> {
    let bytes = word.as_bytes();
    if !bytes.first().is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_') {
        return None;
    }
    let name = name_length(bytes);
    let mut at = name;
    if bytes.get(at) == Some(&b'[') {
        at += word[at..].find(']')? + 1;
    }
    if bytes.get(at) == Some(&b'+') {
        at += 1;
    }
    (bytes.get(at) == Some(&b'=')).then_some(name)
}

fn is_number(word: &str) -> bool {
    if let Some(hex) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let (whole, fraction) = word.split_once('.').unwrap_or((word, ""));
    !whole.is_empty() && whole.bytes().all(|b| b.is_ascii_digit()) && fraction.bytes().all(|b| b.is_ascii_digit())
}

pub const SHELLS: &[&str] = &["sh", "bash", "zsh", "ksh"];

const INTERPRETER_ARGS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn from_hashbang(line: &str) -> Option<Option<(String, Vec<String>)>> {
    let line = line.strip_prefix("#!")?;
    let mut words = line.split_whitespace();
    let Some(first) = words.next() else { return Some(None) };
    let (program, rest): (&str, Vec<&str>) = if base(first) == "env" {
        let mut rest = words.skip_while(|word| word.starts_with('-') || word.contains('='));
        match rest.next() {
            Some(program) => (base(program), rest.collect()),
            None => return Some(None),
        }
    } else {
        (base(first), words.collect())
    };
    if !SHELLS.contains(&program) || rest.len() > INTERPRETER_ARGS || rest.iter().any(|arg| arg.starts_with('#')) {
        return Some(None);
    }
    Some(Some((program.to_owned(), rest.into_iter().map(str::to_owned).collect())))
}

fn from_extension(name: &str) -> Option<&'static str> {
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    match extension {
        "sh" | "command" => Some("sh"),
        "bash" => Some("bash"),
        "zsh" => Some("zsh"),
        "ksh" => Some("ksh"),
        _ => None,
    }
}

pub fn run_target(path: &str, text: &str) -> Option<Target> {
    let name = base(path);
    if name.is_empty() {
        return None;
    }
    let first = text.lines().next().unwrap_or("");
    let (program, mut args) = match from_hashbang(first) {
        Some(found) => found?,
        None => (from_extension(name)?.to_owned(), Vec::new()),
    };
    args.push("${file}".to_owned());
    Some(Target { name: name.to_owned(), program, args })
}

fn json_string(out: &mut String, text: &str) {
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

pub fn run_targets_json(target: Option<&Target>) -> String {
    let Some(target) = target else { return "[]".to_owned() };
    let mut out = String::from("[{\"line\":0,\"id\":\"script\",\"name\":");
    json_string(&mut out, &target.name);
    out.push_str(",\"program\":");
    json_string(&mut out, &target.program);
    out.push_str(",\"cwd\":\"${fileDir}\",\"kind\":\"main\",\"args\":[");
    for (at, arg) in target.args.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        json_string(&mut out, arg);
    }
    out.push_str("]}]");
    out
}

pub fn run_targets_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return b"[]".to_vec() };
    let path = std::str::from_utf8(&input[..split]).unwrap_or("");
    let text = std::str::from_utf8(&input[split + 1..]).unwrap_or("");
    run_targets_json(run_target(path, text).as_ref()).into_bytes()
}

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn encode(spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::with_capacity(spans.len() * 12);
    for span in spans {
        for value in [span.start as u32, span.end as u32, span.kind as u32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_abi() -> i32 {
    1
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_alloc(len: i32) -> i32 {
    INPUT.with_borrow_mut(|input| {
        input.clear();
        input.resize(usize::try_from(len).unwrap_or(0), 0);
        input.as_mut_ptr() as usize as i32
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_highlight(ptr: i32, len: i32) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        let text = if valid { std::str::from_utf8(&input[..len as usize]).unwrap_or("") } else { "" };
        encode(&highlight(text))
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_run_targets(ptr: i32, len: i32) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        if valid { run_targets_request(&input[..len as usize]) } else { b"[]".to_vec() }
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: &str, text: &str) -> Option<(String, Vec<String>)> {
        run_target(path, text).map(|target| (target.program, target.args))
    }

    fn owned(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_hashbang_names_the_shell_that_runs_the_script() {
        assert_eq!(target("bin/deploy", "#!/bin/bash\necho\n"), Some(("bash".into(), owned(&["${file}"]))));
        assert_eq!(target("deploy", "#!/usr/bin/env zsh\n"), Some(("zsh".into(), owned(&["${file}"]))));
        assert_eq!(target("a.sh", "#!/bin/bash -eu\n"), Some(("bash".into(), owned(&["-eu", "${file}"]))));
        assert_eq!(target("a.sh", "#!/usr/bin/env -S bash -e\n"), Some(("bash".into(), owned(&["-e", "${file}"]))));
        assert_eq!(target("a.sh", "#!/usr/bin/env LANG=C ksh\n"), Some(("ksh".into(), owned(&["${file}"]))));
        assert_eq!(target("a.sh", "#!/usr/bin/env python3\n"), None, "another interpreter is not a shell's");
        assert_eq!(target("a.sh", "#!/bin/sh # note\n"), None);
        assert_eq!(target("a.sh", "#!\n"), None);
    }

    #[test]
    fn without_a_hashbang_the_extension_names_it_and_rc_files_have_none() {
        assert_eq!(target("x/build.sh", "echo\n"), Some(("sh".into(), owned(&["${file}"]))));
        assert_eq!(target("go.bash", ""), Some(("bash".into(), owned(&["${file}"]))));
        assert_eq!(target("start.command", "echo\n"), Some(("sh".into(), owned(&["${file}"]))));
        assert_eq!(target("y.zsh", "echo\n").unwrap().0, "zsh");
        assert_eq!(target(".bashrc", "alias l=ls\n"), None);
        assert_eq!(target(".zshrc", ""), None);
        assert_eq!(target("deploy", "echo\n"), None);
    }

    #[test]
    fn the_module_answers_abi_1_so_that_the_host_asks_for_its_run_targets() {
        assert_eq!(zen_abi(), 1);
    }

    #[test]
    fn the_run_target_is_json_for_the_host_on_the_first_line() {
        let answer = run_targets_request(b"tools/de\"ploy.sh\0#!/bin/bash -e\necho\n");
        let json: serde_json::Value = serde_json::from_slice(&answer).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{
                "line": 0, "id": "script", "name": "de\"ploy.sh", "program": "bash",
                "cwd": "${fileDir}", "kind": "main", "args": ["-e", "${file}"],
            }])
        );
        assert_eq!(run_targets_request(b".bashrc\0alias l=ls\n"), b"[]");
        assert_eq!(run_targets_request(b"no separator"), b"[]");
        for shell in SHELLS {
            let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
            assert!(manifest["runs"].as_array().unwrap().contains(&serde_json::json!(shell)), "{shell} is declared");
        }
    }

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    fn contains_all(text: &str, expected: &[(&str, Kind)]) {
        let found = kinds(text);
        for expected in expected {
            assert!(found.contains(expected), "{expected:?} in {found:?}");
        }
    }

    #[test]
    fn highlights_a_script() {
        let text = "#!/bin/bash\n# Build it.\nset -eu\nNAME=\"zen $USER\" # who\nif [[ -f \"$1\" ]]; then\n    echo 'ok' >> log 2>&1\nfi\n";
        assert_eq!(
            kinds(text),
            [
                ("#!/bin/bash", Kind::Comment),
                ("# Build it.", Kind::Comment),
                ("set", Kind::Function),
                ("-eu", Kind::Attribute),
                ("NAME", Kind::Constant),
                ("\"zen ", Kind::String),
                ("$USER", Kind::Constant),
                ("\"", Kind::String),
                ("# who", Kind::Comment),
                ("if", Kind::Keyword),
                ("[[", Kind::Keyword),
                ("-f", Kind::Attribute),
                ("\"", Kind::String),
                ("$1", Kind::Constant),
                ("\"", Kind::String),
                ("]]", Kind::Keyword),
                (";", Kind::Punctuation),
                ("then", Kind::Keyword),
                ("echo", Kind::Function),
                ("'ok'", Kind::String),
                (">>", Kind::Punctuation),
                ("2>&1", Kind::Punctuation),
                ("fi", Kind::Keyword),
            ]
        );
    }

    #[test]
    fn keywords_and_builtins_count_in_command_position_only() {
        let text = "echo if then done; sudo make install && cd /tmp || exit 1";
        assert_eq!(
            kinds(text),
            [
                ("echo", Kind::Function),
                (";", Kind::Punctuation),
                ("&&", Kind::Punctuation),
                ("cd", Kind::Function),
                ("||", Kind::Punctuation),
                ("exit", Kind::Function),
                ("1", Kind::Number),
            ]
        );
    }

    #[test]
    fn highlights_loops_and_case() {
        let text = "for f in *.txt; do\n  wc -l \"$f\"\ndone\nwhile read -r line; do :; done < input\ncase \"$x\" in\n  a|b) echo one ;;\n  *) exit 2 ;;\nesac\nselect s in a b; do break; done";
        contains_all(
            text,
            &[
                ("for", Kind::Keyword),
                ("f", Kind::Constant),
                ("in", Kind::Keyword),
                ("do", Kind::Keyword),
                ("-l", Kind::Attribute),
                ("$f", Kind::Constant),
                ("done", Kind::Keyword),
                ("while", Kind::Keyword),
                ("read", Kind::Function),
                ("-r", Kind::Attribute),
                ("<", Kind::Punctuation),
                ("case", Kind::Keyword),
                ("$x", Kind::Constant),
                ("|", Kind::Punctuation),
                (")", Kind::Punctuation),
                (";;", Kind::Punctuation),
                ("esac", Kind::Keyword),
                ("select", Kind::Keyword),
                ("s", Kind::Constant),
            ],
        );
        let found = kinds(text);
        assert!(!found.iter().any(|(word, _)| *word == "a" || *word == "b" || *word == "wc"), "{found:?}");
    }

    #[test]
    fn highlights_function_definitions() {
        let text = "say() { printf '%s\\n' \"$*\"; }\nfunction cleanup {\n  rm -rf \"$WORK\"\n}\nfunction other() { :; }\nsay hello";
        contains_all(
            text,
            &[
                ("say", Kind::Function),
                ("printf", Kind::Function),
                ("$*", Kind::Constant),
                ("function", Kind::Keyword),
                ("cleanup", Kind::Function),
                ("-rf", Kind::Attribute),
                ("other", Kind::Function),
            ],
        );
        assert_eq!(kinds("say hello"), []);
    }

    #[test]
    fn highlights_variables_and_assignments() {
        let text = "export PATH=\"$HOME/bin:${PATH:-/usr/bin}\" A=1 B+=x C[0]=y\necho $# $? $$ $@ $0 ${#arr[@]} \\$NOT\nlocal x=$(( x + 1 ))";
        contains_all(
            text,
            &[
                ("export", Kind::Function),
                ("PATH", Kind::Constant),
                ("$HOME", Kind::Constant),
                ("${PATH:-/usr/bin}", Kind::Constant),
                ("A", Kind::Constant),
                ("B", Kind::Constant),
                ("C", Kind::Constant),
                ("$#", Kind::Constant),
                ("$?", Kind::Constant),
                ("$$", Kind::Constant),
                ("$@", Kind::Constant),
                ("$0", Kind::Constant),
                ("${#arr[@]}", Kind::Constant),
                ("x", Kind::Constant),
                ("$((", Kind::Punctuation),
                ("1", Kind::Number),
                ("))", Kind::Punctuation),
            ],
        );
        assert!(!kinds(text).iter().any(|(word, _)| word.contains("NOT")));
        assert_eq!(kinds("grep a=b f"), []);
    }

    #[test]
    fn highlights_command_substitution() {
        let text = "DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nn=`wc -l < f` ; x=$(if true; then :; fi) <(ls)";
        contains_all(
            text,
            &[
                ("DIR", Kind::Constant),
                ("$(", Kind::Punctuation),
                ("cd", Kind::Function),
                ("$0", Kind::Constant),
                (")", Kind::Punctuation),
                ("&&", Kind::Punctuation),
                ("`", Kind::Punctuation),
                ("-l", Kind::Attribute),
                ("<(", Kind::Punctuation),
            ],
        );
        let found = kinds(text);
        assert!(found.contains(&("if", Kind::Keyword)), "`if` is in command position inside $(…): {found:?}");
        assert!(found.contains(&("fi", Kind::Keyword)), "{found:?}");
        assert!(!found.iter().any(|(word, _)| *word == "dirname" || *word == "pwd" || *word == "ls"), "{found:?}");
    }

    #[test]
    fn highlights_strings() {
        let text = r##"echo "a \"b\" $x" 'it''s' $'tab\t\'q' "x # not a comment"# also"##;
        contains_all(
            text,
            &[
                (r#""a \"b\" "#, Kind::String),
                ("$x", Kind::Constant),
                ("'it''s'", Kind::String),
                (r"$'tab\t\'q'", Kind::String),
                (r#""x # not a comment""#, Kind::String),
            ],
        );
        assert!(!kinds(text).iter().any(|(_, kind)| *kind == Kind::Comment), "`#` inside a word");
        assert_eq!(kinds("a#b ${x#y}"), [("${x#y}", Kind::Constant)]);
    }

    #[test]
    fn highlights_heredocs() {
        let text = "cat <<EOF > out\nHello $USER, ${HOME}\n  EOF\nEOF\ncat <<-'END'\n\tliteral $x\n\tEND\necho done\nread x <<< \"$y\"";
        contains_all(
            text,
            &[
                ("<<", Kind::Punctuation),
                ("EOF", Kind::String),
                (">", Kind::Punctuation),
                ("Hello ", Kind::String),
                ("$USER", Kind::Constant),
                (", ", Kind::String),
                ("${HOME}", Kind::Constant),
                ("\n  EOF\nEOF", Kind::String),
                ("<<-", Kind::Punctuation),
                ("'END'", Kind::String),
                ("\tliteral $x\n\tEND", Kind::String),
                ("echo", Kind::Function),
                ("<<<", Kind::Punctuation),
                ("$y", Kind::Constant),
            ],
        );
    }

    #[test]
    fn highlights_options_numbers_and_operators() {
        let text = "ls -la --color=auto -- file 42 3.14 0x1F x1 & wait; a |& b; c &> /dev/null; d 2>/dev/null >&2";
        contains_all(
            text,
            &[
                ("-la", Kind::Attribute),
                ("--color", Kind::Attribute),
                ("42", Kind::Number),
                ("3.14", Kind::Number),
                ("0x1F", Kind::Number),
                ("&", Kind::Punctuation),
                ("|&", Kind::Punctuation),
                ("&>", Kind::Punctuation),
                ("2>", Kind::Punctuation),
                (">&2", Kind::Punctuation),
            ],
        );
        let found = kinds(text);
        assert!(!found.iter().any(|(word, _)| *word == "--" || *word == "x1" || *word == "auto"), "{found:?}");
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "'", "\"", "`", "$", "$(", "$((", "${", "}", ")", "(", "#", "\n", "<<", "<<-", "EOF", "\\", "1", "-", "=",
            "case", "in", ";;", "é", "日", " ",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    for d in ["", "\n", "é"] {
                        let text = format!("{a}{b}{c}{d}");
                        let mut previous = 0;
                        for span in highlight(&text) {
                            assert!(
                                span.start >= previous
                                    && span.end > span.start
                                    && span.end <= text.len()
                                    && text.is_char_boundary(span.start)
                                    && text.is_char_boundary(span.end),
                                "{text:?}"
                            );
                            previous = span.end;
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn encodes_records() {
        assert_eq!(encode(&[Span { start: 1, end: 3, kind: Kind::Function }]), [1, 0, 0, 0, 3, 0, 0, 0, 7, 0, 0, 0]);
    }
}

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_i18n::t;

const MAX_DEPTH: usize = 8;

const MAX_PARSES: usize = 64;

const MAX_NESTING: usize = 64;

const MAX_PROBLEMS: usize = 100;

const MAX_OUTPUT: usize = 1 << 20;

const UNSUPPORTED: &[&str] = &["foreach", "macro", "include", "define", "evaluate", "break", "stop"];

/// Something a template has that cannot be rendered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The include it is in, `None` for the template itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<String>,
    /// The line, from 1.
    pub line: usize,
    /// The column, from 1.
    pub column: usize,
    /// What is wrong, in words for the user.
    pub message: String,
}

#[derive(Clone, Debug)]
enum Node {
    Text(String),
    Ref { name: String, quiet: bool, source: String },
    Set { name: String, value: Expr },
    If { branches: Vec<(Cond, Vec<Node>)>, otherwise: Vec<Node> },
    Parse { target: Expr, line: usize, column: usize },
}

#[derive(Clone, Debug)]
enum Expr {
    Literal(String),
    Interpolated(Vec<Node>),
    Ref(String),
}

#[derive(Clone, Debug)]
enum Cond {
    Value(Expr),
    Not(Box<Cond>),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Equal(Expr, Expr, bool),
}

enum Stop {
    Eof,
    ElseIf(Cond),
    Else,
    End,
}

#[derive(Clone, Debug)]
pub struct Parsed {
    nodes: Vec<Node>,
    pub problems: Vec<Problem>,
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    problems: Vec<(usize, String)>,
    include: Option<&'a str>,
    nesting: usize,
    terms: usize,
    absent: std::cell::RefCell<Vec<(&'static str, usize)>>,
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Parser<'_> {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn starts_with(&self, text: &str) -> bool {
        let wanted: Vec<char> = text.chars().collect();
        self.chars[self.pos.min(self.chars.len())..].starts_with(&wanted)
    }

    fn find(&self, text: &'static str, from: usize) -> Option<usize> {
        if self.absent.borrow().iter().any(|(known, after)| *known == text && *after <= from) {
            return None;
        }
        let needle: Vec<char> = text.chars().collect();
        let found = (from..self.chars.len()).find(|&at| self.chars[at..].starts_with(&needle));
        if found.is_none() {
            self.absent.borrow_mut().push((text, from));
        }
        found
    }

    fn position(&self, at: usize) -> (usize, usize) {
        position(&self.chars, at)
    }

    fn problem(&mut self, at: usize, message: String) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push((at, message));
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn name_at(&self, at: usize) -> Option<(String, usize)> {
        let first = *self.chars.get(at)?;
        if !is_name_start(first) {
            return None;
        }
        let end = (at..self.chars.len()).find(|&i| !is_name_char(self.chars[i])).unwrap_or(self.chars.len());
        Some((self.chars[at..end].iter().collect(), end))
    }

    fn directive_at(&self) -> Option<(String, usize)> {
        let braced = self.peek(1) == Some('{');
        let start = self.pos + 1 + usize::from(braced);
        let end = (start..self.chars.len()).find(|&i| !self.chars[i].is_ascii_alphabetic()).unwrap_or(self.chars.len());
        if end == start {
            return None;
        }
        let name: String = self.chars[start..end].iter().collect();
        if braced {
            return (self.chars.get(end) == Some(&'}')).then_some((name, end + 1));
        }
        Some((name, end))
    }

    fn line_start_before(&self, at: usize) -> Option<usize> {
        let mut start = at;
        while start > 0 {
            match self.chars[start - 1] {
                ' ' | '\t' => start -= 1,
                '\n' => return Some(start),
                _ => return None,
            }
        }
        Some(0)
    }

    fn gobble(&mut self, start: usize, text: &mut String) {
        let Some(line_start) = self.line_start_before(start) else { return };
        let end = (self.pos..self.chars.len()).find(|&i| self.chars[i] != ' ' && self.chars[i] != '\t');
        let after = match end {
            None => self.chars.len(),
            Some(i) if self.chars[i] == '\n' => i + 1,
            Some(i) if self.chars[i] == '\r' && self.chars.get(i + 1) == Some(&'\n') => i + 2,
            Some(_) => return,
        };
        let spaces = start - line_start;
        if text.chars().rev().take(spaces).filter(|c| *c == ' ' || *c == '\t').count() == spaces {
            for _ in 0..spaces {
                text.pop();
            }
        }
        self.pos = after;
    }

    fn block(&mut self, inside_if: bool) -> (Vec<Node>, Stop) {
        let mut nodes = Vec::new();
        let mut text = String::new();
        let flush = |nodes: &mut Vec<Node>, text: &mut String| {
            if !text.is_empty() {
                nodes.push(Node::Text(std::mem::take(text)));
            }
        };
        while let Some(c) = self.peek(0) {
            let start = self.pos;
            match c {
                '#' if self.starts_with("##") => {
                    let end = self.find("\n", self.pos).map_or(self.chars.len(), |i| i + 1);
                    if let Some(line_start) = self.line_start_before(start) {
                        for _ in 0..start - line_start {
                            if !text.ends_with([' ', '\t']) {
                                break;
                            }
                            text.pop();
                        }
                    } else if end <= self.chars.len() && self.chars.get(end - 1) == Some(&'\n') {
                        text.push('\n');
                    }
                    self.pos = end;
                }
                '#' if self.starts_with("#*") => match self.find("*#", self.pos + 2) {
                    Some(end) => {
                        self.pos = end + 2;
                        self.gobble(start, &mut text);
                    }
                    None => {
                        self.problem(start, t!("workbench-file-templates-unclosed-comment"));
                        self.pos = self.chars.len();
                    }
                },
                '#' if self.starts_with("#[[") => match self.find("]]#", self.pos + 3) {
                    Some(end) => {
                        text.extend(&self.chars[self.pos + 3..end]);
                        self.pos = end + 3;
                    }
                    None => {
                        self.problem(start, t!("workbench-file-templates-unclosed-raw"));
                        self.pos = self.chars.len();
                    }
                },
                '#' => match self.directive_at() {
                    Some((name, end))
                        if matches!(name.as_str(), "set" | "if" | "elseif" | "else" | "end" | "parse") =>
                    {
                        self.pos = end;
                        match name.as_str() {
                            "set" => {
                                if let Some(node) = self.set(start) {
                                    self.gobble(start, &mut text);
                                    flush(&mut nodes, &mut text);
                                    nodes.push(node);
                                }
                            }
                            "if" => {
                                if self.nesting >= MAX_NESTING {
                                    self.problem(start, t!("workbench-file-templates-nested-deep"));
                                    self.pos = self.chars.len();
                                    continue;
                                }
                                let Some(cond) = self.parenthesised_condition(start) else { continue };
                                self.gobble(start, &mut text);
                                flush(&mut nodes, &mut text);
                                self.nesting += 1;
                                let node = self.if_rest(cond, start);
                                self.nesting -= 1;
                                nodes.push(node);
                            }
                            "parse" => {
                                self.skip_spaces();
                                if self.peek(0) != Some('(') {
                                    self.problem(start, t!("workbench-file-templates-expected", what = "("));
                                    continue;
                                }
                                self.pos += 1;
                                self.skip_spaces();
                                let Some(target) = self.expr() else {
                                    self.problem(start, t!("workbench-file-templates-bad-parse"));
                                    self.skip_to_close();
                                    continue;
                                };
                                self.skip_spaces();
                                if self.peek(0) == Some(')') {
                                    self.pos += 1;
                                } else {
                                    self.problem(self.pos, t!("workbench-file-templates-expected", what = ")"));
                                    self.skip_to_close();
                                }
                                let (line, column) = self.position(start);
                                self.gobble(start, &mut text);
                                flush(&mut nodes, &mut text);
                                nodes.push(Node::Parse { target, line, column });
                            }
                            other if inside_if => {
                                let stop = match other {
                                    "elseif" => match self.parenthesised_condition(start) {
                                        Some(cond) => Stop::ElseIf(cond),
                                        None => continue,
                                    },
                                    "else" => Stop::Else,
                                    _ => Stop::End,
                                };
                                self.gobble(start, &mut text);
                                flush(&mut nodes, &mut text);
                                return (nodes, stop);
                            }
                            other => {
                                self.problem(start, t!("workbench-file-templates-stray", directive = other));
                            }
                        }
                    }
                    Some((name, end)) if UNSUPPORTED.contains(&name.as_str()) && self.call_follows(end) => {
                        self.problem(start, t!("workbench-file-templates-unsupported", directive = name.as_str()));
                        text.extend(&self.chars[start..end]);
                        self.pos = end;
                    }
                    _ => {
                        text.push('#');
                        self.pos += 1;
                    }
                },
                '$' => match self.reference(start) {
                    Some(node) => {
                        flush(&mut nodes, &mut text);
                        nodes.push(node);
                    }
                    None => {
                        text.push('$');
                        self.pos = start + 1;
                    }
                },
                '\\' if self.escapes() => {
                    self.pos += 1;
                    let end = match self.peek(0) {
                        Some('$') => self.reference_end().unwrap_or(self.pos + 1),
                        _ => self.directive_at().map_or(self.pos + 1, |(_, end)| end),
                    };
                    text.extend(&self.chars[self.pos..end]);
                    self.pos = end;
                }
                c => {
                    text.push(c);
                    self.pos += 1;
                }
            }
        }
        flush(&mut nodes, &mut text);
        (nodes, Stop::Eof)
    }

    fn call_follows(&self, end: usize) -> bool {
        self.chars[end.min(self.chars.len())..].iter().find(|c| **c != ' ' && **c != '\t') == Some(&'(')
    }

    fn escapes(&self) -> bool {
        match self.peek(1) {
            Some('$') => {
                let at = self.pos + 2 + usize::from(self.peek(2) == Some('!'));
                self.chars.get(at) == Some(&'{') || self.name_at(at).is_some()
            }
            Some('#') => {
                let at = self.pos + 1;
                let braced = self.chars.get(at + 1) == Some(&'{');
                let name_at = at + 1 + usize::from(braced);
                let end = (name_at..self.chars.len())
                    .find(|&i| !self.chars[i].is_ascii_alphabetic())
                    .unwrap_or(self.chars.len());
                let name: String = self.chars[name_at..end].iter().collect();
                matches!(name.as_str(), "set" | "if" | "elseif" | "else" | "end" | "parse")
                    || (UNSUPPORTED.contains(&name.as_str()) && self.call_follows(end))
            }
            _ => false,
        }
    }

    fn reference_end(&self) -> Option<usize> {
        let mut at = self.pos + 1;
        if self.chars.get(at) == Some(&'!') {
            at += 1;
        }
        if self.chars.get(at) == Some(&'{') {
            return self.find("}", at).map(|end| end + 1);
        }
        self.name_at(at).map(|(_, end)| end)
    }

    fn skip_to_close(&mut self) {
        let mut depth = 0usize;
        while let Some(c) = self.peek(0) {
            self.pos += 1;
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => return,
                ')' => depth -= 1,
                '\n' => return,
                _ => {}
            }
        }
    }

    fn reference(&mut self, start: usize) -> Option<Node> {
        let mut at = start + 1;
        let quiet = self.chars.get(at) == Some(&'!');
        if quiet {
            at += 1;
        }
        if self.chars.get(at) == Some(&'{') {
            let (name, end) = self.name_at(at + 1)?;
            if self.chars.get(end) == Some(&'}') {
                self.pos = end + 1;
                let source = self.chars[start..self.pos].iter().collect();
                return Some(Node::Ref { name, quiet, source });
            }
            self.problem(start, t!("workbench-file-templates-unsupported-reference"));
            self.pos = self.find("}", end).map_or(end, |close| close + 1);
            let source: String = self.chars[start..self.pos].iter().collect();
            return Some(Node::Text(source));
        }
        let (name, end) = self.name_at(at)?;
        if self.chars.get(end) == Some(&'.')
            && let Some((_, after)) = self.name_at(end + 1)
            && self.chars.get(after) == Some(&'(')
        {
            self.problem(start, t!("workbench-file-templates-unsupported-reference"));
            self.pos = after;
            self.skip_to_close();
            let source: String = self.chars[start..self.pos].iter().collect();
            return Some(Node::Text(source));
        }
        self.pos = end;
        let source = self.chars[start..end].iter().collect();
        Some(Node::Ref { name, quiet, source })
    }

    fn set(&mut self, start: usize) -> Option<Node> {
        self.skip_spaces();
        if self.peek(0) != Some('(') {
            self.problem(start, t!("workbench-file-templates-expected", what = "("));
            return None;
        }
        self.pos += 1;
        self.skip_spaces();
        let target = match self.expr() {
            Some(Expr::Ref(name)) => name,
            _ => {
                self.problem(start, t!("workbench-file-templates-bad-set"));
                self.skip_to_close();
                return None;
            }
        };
        self.skip_spaces();
        if self.peek(0) != Some('=') {
            self.problem(self.pos, t!("workbench-file-templates-expected", what = "="));
            self.skip_to_close();
            return None;
        }
        self.pos += 1;
        self.skip_spaces();
        let value_at = self.pos;
        let Some(value) = self.expr() else {
            self.problem(value_at, t!("workbench-file-templates-bad-value"));
            self.skip_to_close();
            return None;
        };
        self.skip_spaces();
        if self.peek(0) != Some(')') {
            self.problem(self.pos, t!("workbench-file-templates-unsupported-expression"));
            self.skip_to_close();
            return None;
        }
        self.pos += 1;
        Some(Node::Set { name: target, value })
    }

    fn expr(&mut self) -> Option<Expr> {
        let start = self.pos;
        match self.peek(0)? {
            '"' | '\'' => {
                let quote = self.peek(0)?;
                let mut at = start + 1;
                let mut raw = String::new();
                loop {
                    match self.chars.get(at) {
                        None => {
                            self.problem(start, t!("workbench-file-templates-unclosed-string"));
                            self.pos = self.chars.len();
                            return None;
                        }
                        Some('\\') if self.chars.get(at + 1) == Some(&quote) => {
                            raw.push(quote);
                            at += 2;
                        }
                        Some(c) if *c == quote => break,
                        Some(c) => {
                            raw.push(*c);
                            at += 1;
                        }
                    }
                }
                self.pos = at + 1;
                if quote == '\'' {
                    return Some(Expr::Literal(raw));
                }
                let mut inner = Parser {
                    chars: raw.chars().collect(),
                    pos: 0,
                    problems: Vec::new(),
                    include: self.include,
                    nesting: 0,
                    terms: 0,
                    absent: std::cell::RefCell::default(),
                };
                let mut nodes = Vec::new();
                let mut text = String::new();
                while let Some(c) = inner.peek(0) {
                    let at = inner.pos;
                    if c == '$'
                        && let Some(node) = inner.reference(at)
                    {
                        if !text.is_empty() {
                            nodes.push(Node::Text(std::mem::take(&mut text)));
                        }
                        nodes.push(node);
                        continue;
                    }
                    text.push(c);
                    inner.pos = at + 1;
                }
                if !text.is_empty() {
                    nodes.push(Node::Text(text));
                }
                for (_, message) in inner.problems {
                    self.problem(start, message);
                }
                Some(Expr::Interpolated(nodes))
            }
            '$' => match self.reference(start)? {
                Node::Ref { name, .. } => Some(Expr::Ref(name)),
                _ => None,
            },
            c if c.is_ascii_digit() || c == '-' => {
                let end = (start + 1..self.chars.len())
                    .find(|&i| !self.chars[i].is_ascii_digit())
                    .unwrap_or(self.chars.len());
                let number: String = self.chars[start..end].iter().collect();
                if number == "-" {
                    return None;
                }
                self.pos = end;
                Some(Expr::Literal(number))
            }
            _ => {
                let (word, end) = self.name_at(start)?;
                if word == "true" || word == "false" {
                    self.pos = end;
                    return Some(Expr::Literal(word));
                }
                None
            }
        }
    }

    fn parenthesised_condition(&mut self, start: usize) -> Option<Cond> {
        self.skip_spaces();
        if self.peek(0) != Some('(') {
            self.problem(start, t!("workbench-file-templates-expected", what = "("));
            return None;
        }
        self.pos += 1;
        self.terms = 0;
        let Some(cond) = self.or() else {
            self.problem(self.pos, t!("workbench-file-templates-bad-condition"));
            self.skip_to_close();
            return None;
        };
        self.skip_spaces();
        if self.peek(0) != Some(')') {
            self.problem(self.pos, t!("workbench-file-templates-bad-condition"));
            self.skip_to_close();
            return None;
        }
        self.pos += 1;
        Some(cond)
    }

    fn keyword(&mut self, symbol: &str, word: &str) -> bool {
        self.skip_spaces();
        if self.starts_with(symbol) {
            self.pos += symbol.chars().count();
            return true;
        }
        if let Some((found, end)) = self.name_at(self.pos)
            && found == word
        {
            self.pos = end;
            return true;
        }
        false
    }

    fn or(&mut self) -> Option<Cond> {
        let mut left = self.and()?;
        while self.keyword("||", "or") {
            left = Cond::Or(Box::new(left), Box::new(self.and()?));
        }
        Some(left)
    }

    fn and(&mut self) -> Option<Cond> {
        let mut left = self.unary()?;
        while self.keyword("&&", "and") {
            left = Cond::And(Box::new(left), Box::new(self.unary()?));
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<Cond> {
        self.terms += 1;
        if self.terms > MAX_NESTING {
            return None;
        }
        self.skip_spaces();
        if self.peek(0) == Some('!') && self.peek(1) != Some('=') {
            self.pos += 1;
            return Some(Cond::Not(Box::new(self.unary()?)));
        }
        if let Some((word, end)) = self.name_at(self.pos)
            && word == "not"
        {
            self.pos = end;
            return Some(Cond::Not(Box::new(self.unary()?)));
        }
        if self.peek(0) == Some('(') {
            self.pos += 1;
            let inner = self.or()?;
            self.skip_spaces();
            if self.peek(0) != Some(')') {
                return None;
            }
            self.pos += 1;
            return Some(inner);
        }
        let left = self.expr()?;
        let at = self.pos;
        if self.keyword("==", "eq") {
            self.skip_spaces();
            return Some(Cond::Equal(left, self.expr()?, false));
        }
        if self.keyword("!=", "ne") {
            self.skip_spaces();
            return Some(Cond::Equal(left, self.expr()?, true));
        }
        self.pos = at;
        Some(Cond::Value(left))
    }

    fn if_rest(&mut self, first: Cond, start: usize) -> Node {
        let mut branches = Vec::new();
        let mut condition = first;
        loop {
            let (nodes, stop) = self.block(true);
            branches.push((condition, nodes));
            match stop {
                Stop::ElseIf(next) => condition = next,
                Stop::Else => {
                    let (otherwise, stop) = self.block(true);
                    match stop {
                        Stop::End => {}
                        Stop::Eof => self.problem(start, t!("workbench-file-templates-unclosed-if")),
                        Stop::Else | Stop::ElseIf(_) => {
                            self.problem(start, t!("workbench-file-templates-else-after-else"));
                            let _ = self.block(true);
                        }
                    }
                    return Node::If { branches, otherwise };
                }
                Stop::End => return Node::If { branches, otherwise: Vec::new() },
                Stop::Eof => {
                    self.problem(start, t!("workbench-file-templates-unclosed-if"));
                    return Node::If { branches, otherwise: Vec::new() };
                }
            }
        }
    }
}

pub fn parse(text: &str) -> Parsed {
    parse_in(text, None)
}

fn position(chars: &[char], at: usize) -> (usize, usize) {
    let at = at.min(chars.len());
    let line = chars[..at].iter().filter(|c| **c == '\n').count() + 1;
    let column = at - chars[..at].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1) + 1;
    (line, column)
}

fn parse_in(text: &str, include: Option<&str>) -> Parsed {
    let mut parser = Parser {
        chars: text.chars().collect(),
        pos: 0,
        problems: Vec::new(),
        include,
        nesting: 0,
        terms: 0,
        absent: std::cell::RefCell::default(),
    };
    let (nodes, _) = parser.block(false);
    let chars = parser.chars;
    let problems = parser
        .problems
        .into_iter()
        .map(|(at, message)| {
            let (line, column) = position(&chars, at);
            Problem { include: include.map(str::to_owned), line, column, message }
        })
        .collect();
    Parsed { nodes, problems }
}

pub type Includes<'a> = &'a dyn Fn(&str) -> Option<String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub missing: Vec<String>,
    pub problems: Vec<Problem>,
}

struct Renderer<'a> {
    values: HashMap<String, String>,
    includes: Includes<'a>,
    out: Rendered,
    depth: usize,
    parses: usize,
    stopped: bool,
}

impl Renderer<'_> {
    fn report(&mut self, problem: Problem) {
        if self.out.problems.len() < MAX_PROBLEMS {
            self.out.problems.push(problem);
        }
    }

    fn too_long(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.report(Problem {
                include: None,
                line: 1,
                column: 1,
                message: t!("workbench-file-templates-too-long", size = MAX_OUTPUT / 1024),
            });
        }
    }

    fn value(&mut self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Literal(text) => Some(text.clone()),
            Expr::Ref(name) => {
                let found = self.values.get(name).cloned();
                if found.is_none() && !self.out.missing.contains(name) {
                    self.out.missing.push(name.clone());
                }
                found
            }
            Expr::Interpolated(nodes) => {
                let mut text = String::new();
                for node in nodes {
                    match node {
                        Node::Text(part) => text.push_str(part),
                        Node::Ref { name, quiet, source } => match self.value(&Expr::Ref(name.clone())) {
                            Some(value) => text.push_str(&value),
                            None if *quiet => {}
                            None => text.push_str(source),
                        },
                        _ => {}
                    }
                }
                if text.len() > MAX_OUTPUT {
                    self.too_long();
                    return Some(String::new());
                }
                Some(text)
            }
        }
    }

    fn truth(&mut self, cond: &Cond) -> bool {
        match cond {
            Cond::Value(expr) => self.value(expr).is_some_and(|value| !value.is_empty() && value != "false"),
            Cond::Not(inner) => !self.truth(inner),
            Cond::And(left, right) => {
                let left = self.truth(left);
                let right = self.truth(right);
                left && right
            }
            Cond::Or(left, right) => {
                let left = self.truth(left);
                let right = self.truth(right);
                left || right
            }
            Cond::Equal(left, right, negated) => {
                let equal = self.value(left) == self.value(right);
                equal != *negated
            }
        }
    }

    fn nodes(&mut self, nodes: &[Node], include: Option<&str>) {
        for node in nodes {
            if self.stopped {
                return;
            }
            if self.out.text.len() > MAX_OUTPUT {
                self.out.text.truncate(self.out.text.floor_char_boundary(MAX_OUTPUT));
                self.too_long();
                return;
            }
            match node {
                Node::Text(text) => self.out.text.push_str(text),
                Node::Ref { name, quiet, source } => match self.value(&Expr::Ref(name.clone())) {
                    Some(value) => self.out.text.push_str(&value),
                    None if *quiet => {}
                    None => self.out.text.push_str(source),
                },
                Node::Set { name, value } => {
                    let value = self.value(value).unwrap_or_default();
                    self.values.insert(name.clone(), value);
                }
                Node::If { branches, otherwise } => {
                    let chosen = branches.iter().find(|(cond, _)| self.truth(cond)).map(|(_, nodes)| nodes);
                    self.nodes(chosen.unwrap_or(otherwise), include);
                }
                Node::Parse { target, line, column } => {
                    let name = self.value(target).unwrap_or_default();
                    let problem = |message: String, include: Option<&str>| Problem {
                        include: include.map(str::to_owned),
                        line: *line,
                        column: *column,
                        message,
                    };
                    if self.depth >= MAX_DEPTH {
                        self.report(problem(t!("workbench-file-templates-parse-deep"), include));
                        continue;
                    }
                    if self.parses >= MAX_PARSES {
                        self.report(problem(t!("workbench-file-templates-parse-many", count = MAX_PARSES), include));
                        continue;
                    }
                    let Some(text) = (self.includes)(&name) else {
                        self.report(problem(t!("workbench-file-templates-no-include", name = name.as_str()), include));
                        continue;
                    };
                    self.parses += 1;
                    let parsed = parse_in(&text, Some(&name));
                    for found in parsed.problems {
                        self.report(found);
                    }
                    self.depth += 1;
                    self.nodes(&parsed.nodes, Some(&name));
                    self.depth -= 1;
                }
            }
        }
    }
}

pub fn render(text: &str, values: &HashMap<String, String>, includes: Includes<'_>) -> Rendered {
    let parsed = parse(text);
    let mut renderer = Renderer {
        values: values.clone(),
        includes,
        out: Rendered { problems: parsed.problems, ..Rendered::default() },
        depth: 0,
        parses: 0,
        stopped: false,
    };
    renderer.nodes(&parsed.nodes, None);
    if renderer.out.text.len() > MAX_OUTPUT {
        renderer.out.text.truncate(renderer.out.text.floor_char_boundary(MAX_OUTPUT));
        renderer.too_long();
    }
    renderer.out
}

pub fn references(text: &str, includes: Includes<'_>) -> Vec<String> {
    let mut found = Vec::new();
    let mut set = Vec::new();
    let mut parses = 0;
    walk(&parse(text).nodes, includes, &mut found, &mut set, 0, &mut parses);
    found
}

fn walk_expr(expr: &Expr, found: &mut Vec<String>, set: &[String]) {
    match expr {
        Expr::Literal(_) => {}
        Expr::Ref(name) => {
            if !set.contains(name) && !found.contains(name) {
                found.push(name.clone());
            }
        }
        Expr::Interpolated(nodes) => {
            for node in nodes {
                if let Node::Ref { name, .. } = node {
                    walk_expr(&Expr::Ref(name.clone()), found, set);
                }
            }
        }
    }
}

fn walk_cond(cond: &Cond, found: &mut Vec<String>, set: &[String]) {
    match cond {
        Cond::Value(expr) => walk_expr(expr, found, set),
        Cond::Not(inner) => walk_cond(inner, found, set),
        Cond::And(left, right) | Cond::Or(left, right) => {
            walk_cond(left, found, set);
            walk_cond(right, found, set);
        }
        Cond::Equal(left, right, _) => {
            walk_expr(left, found, set);
            walk_expr(right, found, set);
        }
    }
}

fn walk(
    nodes: &[Node],
    includes: Includes<'_>,
    found: &mut Vec<String>,
    set: &mut Vec<String>,
    depth: usize,
    parses: &mut usize,
) {
    for node in nodes {
        match node {
            Node::Text(_) => {}
            Node::Ref { name, .. } => walk_expr(&Expr::Ref(name.clone()), found, set),
            Node::Set { name, value } => {
                walk_expr(value, found, set);
                set.push(name.clone());
            }
            Node::If { branches, otherwise } => {
                for (cond, nodes) in branches {
                    walk_cond(cond, found, set);
                    walk(nodes, includes, found, set, depth, parses);
                }
                walk(otherwise, includes, found, set, depth, parses);
            }
            Node::Parse { target, .. } => {
                walk_expr(target, found, set);
                let name = match target {
                    Expr::Literal(name) => Some(name.clone()),
                    Expr::Interpolated(nodes) if nodes.iter().all(|node| matches!(node, Node::Text(_))) => Some(
                        nodes.iter().map(|node| if let Node::Text(text) = node { text.as_str() } else { "" }).collect(),
                    ),
                    _ => None,
                };
                if depth < MAX_DEPTH
                    && *parses < MAX_PARSES
                    && let Some(text) = name.and_then(|name| includes(&name))
                {
                    *parses += 1;
                    walk(&parse(&text).nodes, includes, found, set, depth + 1, parses);
                }
            }
        }
    }
}

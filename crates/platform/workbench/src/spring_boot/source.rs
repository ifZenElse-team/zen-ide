use std::path::Path;

const STEREOTYPES: &[&str] =
    &["Component", "Service", "Repository", "Controller", "RestController", "Configuration", "SpringBootApplication"];

const MAPPINGS: &[(&str, &str)] = &[
    ("GetMapping", "GET"),
    ("PostMapping", "POST"),
    ("PutMapping", "PUT"),
    ("DeleteMapping", "DELETE"),
    ("PatchMapping", "PATCH"),
    ("RequestMapping", ""),
];

const MODIFIERS: &[&str] = &[
    "public",
    "protected",
    "private",
    "static",
    "final",
    "abstract",
    "synchronized",
    "native",
    "default",
    "open",
    "override",
    "internal",
    "suspend",
    "inline",
    "operator",
    "data",
    "sealed",
    "lateinit",
    "const",
    "transient",
    "volatile",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BeanKind {
    Component,
    Method,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bean {
    pub kind: BeanKind,
    pub type_name: String,
    pub supertypes: Vec<String>,
    pub name: String,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Injection {
    pub type_name: String,
    pub name: String,
    pub owner: String,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub method: String,
    pub path: String,
    pub handler: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub beans: Vec<Bean>,
    pub injections: Vec<Injection>,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Word(String),
    Text(String),
    Mark(char),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    kind: Kind,
    line: u32,
}

fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut line = 0u32;
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        match c {
            '\n' => {
                line += 1;
                at += 1;
            }
            c if c.is_whitespace() => at += 1,
            '/' if chars.get(at + 1) == Some(&'/') => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            '/' if chars.get(at + 1) == Some(&'*') => {
                at += 2;
                while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                    if chars[at] == '\n' {
                        line += 1;
                    }
                    at += 1;
                }
                at += 2;
            }
            '"' | '\'' => {
                let start_line = line;
                let triple = c == '"' && chars.get(at + 1) == Some(&'"') && chars.get(at + 2) == Some(&'"');
                let mut value = String::new();
                if triple {
                    at += 3;
                    while at < chars.len()
                        && !(chars[at] == '"' && chars.get(at + 1) == Some(&'"') && chars.get(at + 2) == Some(&'"'))
                    {
                        if chars[at] == '\n' {
                            line += 1;
                        }
                        value.push(chars[at]);
                        at += 1;
                    }
                    at += 3;
                } else {
                    at += 1;
                    while at < chars.len() && chars[at] != c && chars[at] != '\n' {
                        if chars[at] == '\\' {
                            at += 1;
                        }
                        if let Some(next) = chars.get(at) {
                            value.push(*next);
                        }
                        at += 1;
                    }
                    at += 1;
                }
                if c == '"' {
                    found.push(Token { kind: Kind::Text(value), line: start_line });
                }
            }
            c if c.is_alphanumeric() || c == '_' || c == '$' => {
                let start = at;
                while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_' || chars[at] == '$') {
                    at += 1;
                }
                found.push(Token { kind: Kind::Word(chars[start..at].iter().collect()), line });
            }
            '`' => {
                let start = at + 1;
                at = start;
                while at < chars.len() && chars[at] != '`' && chars[at] != '\n' {
                    at += 1;
                }
                found.push(Token { kind: Kind::Word(chars[start..at.min(chars.len())].iter().collect()), line });
                at += 1;
            }
            c => {
                found.push(Token { kind: Kind::Mark(c), line });
                at += 1;
            }
        }
    }
    found
}

#[derive(Clone, Debug)]
struct Annotation {
    name: String,
    args: Vec<Token>,
}

impl Annotation {
    fn texts(&self, keys: &[&str]) -> Vec<String> {
        let mut found = Vec::new();
        let mut depth = 0usize;
        let mut key: Option<String> = None;
        let mut index = 0;
        while index < self.args.len() {
            match &self.args[index].kind {
                Kind::Mark('(' | '{' | '[') => depth += 1,
                Kind::Mark(')' | '}' | ']') => depth = depth.saturating_sub(1),
                Kind::Mark(',') if depth == 0 => key = None,
                Kind::Word(word) if matches!(self.args.get(index + 1).map(|t| &t.kind), Some(Kind::Mark('='))) => {
                    key = Some(word.clone());
                    index += 1;
                }
                Kind::Text(text) if key.as_deref().is_none_or(|key| keys.contains(&key)) => found.push(text.clone()),
                _ => {}
            }
            index += 1;
        }
        found
    }

    fn methods(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut in_method = false;
        for (index, token) in self.args.iter().enumerate() {
            let Kind::Word(word) = &token.kind else { continue };
            if matches!(self.args.get(index + 1).map(|next| &next.kind), Some(Kind::Mark('='))) {
                in_method = word == "method";
            } else if in_method
                && matches!(word.as_str(), "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" | "TRACE")
            {
                found.push(word.clone());
            }
        }
        found
    }
}

struct Class {
    name: String,
    depth: usize,
    bean: bool,
    prefixes: Vec<String>,
}

struct Reader {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
    classes: Vec<Class>,
    pending: Vec<Annotation>,
    facts: Facts,
    kotlin: bool,
}

fn simple(type_name: &str) -> String {
    type_name.rsplit('.').next().unwrap_or(type_name).trim_end_matches('?').to_owned()
}

fn join(prefix: &str, path: &str) -> String {
    let joined = format!("/{}/{}", prefix.trim_matches('/'), path.trim_matches('/'));
    let mut out = String::new();
    for part in joined.split('/').filter(|part| !part.is_empty()) {
        out.push('/');
        out.push_str(part);
    }
    if out.is_empty() { "/".to_owned() } else { out }
}

impl Reader {
    fn word(&self, at: usize) -> Option<&str> {
        match &self.tokens.get(at)?.kind {
            Kind::Word(word) => Some(word),
            _ => None,
        }
    }

    fn mark(&self, at: usize) -> Option<char> {
        match self.tokens.get(at)?.kind {
            Kind::Mark(c) => Some(c),
            _ => None,
        }
    }

    fn line(&self, at: usize) -> u32 {
        self.tokens.get(at).map_or(0, |token| token.line)
    }

    fn closing(&self, open: usize) -> usize {
        let (start, end) = match self.mark(open) {
            Some('(') => ('(', ')'),
            Some('<') => ('<', '>'),
            Some('{') => ('{', '}'),
            Some('[') => ('[', ']'),
            _ => return open,
        };
        let mut depth = 0usize;
        let mut at = open;
        while at < self.tokens.len() {
            match self.mark(at) {
                Some(c) if c == start => depth += 1,
                Some(c) if c == end => {
                    depth -= 1;
                    if depth == 0 {
                        return at;
                    }
                }
                Some(';') if start == '<' => return at,
                _ => {}
            }
            at += 1;
        }
        self.tokens.len().saturating_sub(1)
    }

    fn annotation(&mut self) {
        self.at += 1;
        let mut name = String::new();
        while let Some(word) = self.word(self.at) {
            name = word.to_owned();
            self.at += 1;
            if self.mark(self.at) == Some('.') && self.word(self.at + 1).is_some() {
                self.at += 1;
            } else {
                break;
            }
        }
        if name == "interface" {
            return;
        }
        let mut args = Vec::new();
        if self.mark(self.at) == Some('(') {
            let close = self.closing(self.at);
            args = self.tokens[self.at + 1..close].to_vec();
            self.at = close + 1;
        }
        self.pending.push(Annotation { name, args });
    }

    fn type_at(&self, at: usize) -> (String, usize) {
        let mut name = String::new();
        let mut at = at;
        while let Some(word) = self.word(at) {
            name.push_str(word);
            at += 1;
            if self.mark(at) == Some('.') && self.word(at + 1).is_some() {
                name.push('.');
                at += 1;
            } else {
                break;
            }
        }
        if self.mark(at) == Some('<') {
            at = self.closing(at) + 1;
        }
        while self.mark(at) == Some('?') || (self.mark(at) == Some('[') && self.mark(at + 1) == Some(']')) {
            at += if self.mark(at) == Some('?') { 1 } else { 2 };
        }
        (simple(&name), at)
    }

    fn parameters(&self, open: usize) -> Vec<(String, String, u32)> {
        let close = self.closing(open);
        let mut found = Vec::new();
        let mut start = open + 1;
        let mut at = open + 1;
        let mut depth = 0usize;
        while at <= close {
            match self.mark(at) {
                Some('(' | '<' | '[' | '{') => depth += 1,
                Some(')' | '>' | ']' | '}') if at != close => depth = depth.saturating_sub(1),
                _ => {}
            }
            if (self.mark(at) == Some(',') && depth == 0) || at == close {
                if let Some(parameter) = self.parameter(start, at) {
                    found.push(parameter);
                }
                start = at + 1;
            }
            at += 1;
        }
        found
    }

    fn parameter(&self, start: usize, end: usize) -> Option<(String, String, u32)> {
        let mut at = start;
        while at < end {
            if self.mark(at) == Some('@') {
                at += 1;
                while self.word(at).is_some() && self.mark(at + 1) == Some('.') {
                    at += 2;
                }
                at += 1;
                if self.mark(at) == Some('(') {
                    at = self.closing(at) + 1;
                }
                continue;
            }
            match self.word(at) {
                Some(word) if MODIFIERS.contains(&word) || matches!(word, "val" | "var" | "vararg") => at += 1,
                _ => break,
            }
        }
        if self.kotlin {
            let name = self.word(at)?.to_owned();
            if self.mark(at + 1) != Some(':') {
                return None;
            }
            let (type_name, _) = self.type_at(at + 2);
            return (!type_name.is_empty()).then(|| (type_name, name, self.line(at)));
        }
        let (type_name, after) = self.type_at(at);
        let mut name_at = after;
        while self.mark(name_at) == Some('.') {
            name_at += 1;
        }
        let name = self.word(name_at).filter(|_| name_at < end)?.to_owned();
        (!type_name.is_empty()).then(|| (type_name, name, self.line(at)))
    }

    fn class(&mut self, keyword_at: usize) {
        let Some(name) = self.word(keyword_at + 1).map(str::to_owned) else {
            self.at = keyword_at + 1;
            return;
        };
        let line = self.line(keyword_at + 1);
        let annotations = std::mem::take(&mut self.pending);
        let bean = annotations.iter().any(|annotation| STEREOTYPES.contains(&annotation.name.as_str()));
        let prefixes: Vec<String> = annotations
            .iter()
            .filter(|annotation| annotation.name == "RequestMapping")
            .flat_map(|annotation| annotation.texts(&["value", "path"]))
            .collect();
        let mut at = keyword_at + 2;
        if self.mark(at) == Some('<') {
            at = self.closing(at) + 1;
        }
        let mut supertypes = Vec::new();
        let mut injections = Vec::new();
        let mut body = None;
        let mut collecting = false;
        while at < self.tokens.len() {
            match &self.tokens[at].kind {
                Kind::Mark('{') => {
                    body = Some(at);
                    break;
                }
                Kind::Mark(';') => break,
                Kind::Mark('@') if self.kotlin && self.line(at) > line => break,
                Kind::Mark('@') => {
                    at += 1;
                    while self.word(at).is_some() && self.mark(at + 1) == Some('.') {
                        at += 2;
                    }
                    at += 1;
                    if self.mark(at) == Some('(') {
                        at = self.closing(at) + 1;
                    }
                    continue;
                }
                Kind::Mark('(') => {
                    if self.kotlin && !collecting && bean {
                        injections.extend(self.parameters(at));
                    }
                    at = self.closing(at);
                }
                Kind::Mark(':') => collecting = true,
                Kind::Word(word)
                    if self.kotlin
                        && matches!(word.as_str(), "fun" | "class" | "object" | "val" | "var" | "interface") =>
                {
                    break;
                }
                Kind::Word(word) if matches!(word.as_str(), "extends" | "implements") => collecting = true,
                Kind::Word(word) if word == "constructor" || MODIFIERS.contains(&word.as_str()) => {}
                Kind::Word(_) if collecting => {
                    let (type_name, after) = self.type_at(at);
                    supertypes.push(type_name);
                    at = after;
                    continue;
                }
                _ => {}
            }
            at += 1;
        }
        if bean {
            self.facts.beans.push(Bean {
                kind: BeanKind::Component,
                type_name: name.clone(),
                supertypes,
                name: name.clone(),
                line,
            });
            for (type_name, parameter, line) in injections {
                self.facts.injections.push(Injection { type_name, name: parameter, owner: name.clone(), line });
            }
        }
        match body {
            Some(open) => {
                self.depth += 1;
                self.classes.push(Class { name, depth: self.depth, bean, prefixes });
                self.at = open + 1;
            }
            None => self.at = at.max(keyword_at + 2),
        }
    }

    fn member(&mut self) {
        let Some(class) = self.classes.last() else {
            self.pending.clear();
            self.at += 1;
            return;
        };
        let (owner, bean, prefixes) = (class.name.clone(), class.bean, class.prefixes.clone());
        let start = self.at;
        let mut at = start;
        let mut words: Vec<usize> = Vec::new();
        let first_line = self.line(start);
        while at < self.tokens.len() {
            if self.kotlin && self.line(at) > first_line {
                break;
            }
            match self.mark(at) {
                Some('(') => break,
                Some(';' | '{' | '}' | '=') => break,
                Some('<') => at = self.closing(at),
                Some('@') => break,
                _ => {}
            }
            if self.word(at).is_some() {
                words.push(at);
            }
            at += 1;
        }
        let annotations = std::mem::take(&mut self.pending);
        let has = |name: &str| annotations.iter().any(|annotation| annotation.name == name);
        if self.mark(at) == Some('(') && !words.is_empty() {
            let name_at = *words.last().unwrap_or(&start);
            let name = self.word(name_at).unwrap_or_default().to_owned();
            let line = self.line(name_at);
            let constructor = name == owner || name == "constructor";
            let parameters = self.parameters(at);
            let close = self.closing(at);
            if has("Bean") {
                let type_name = if self.kotlin {
                    if self.mark(close + 1) == Some(':') { self.type_at(close + 2).0 } else { String::new() }
                } else {
                    words.iter().rev().nth(1).and_then(|&type_at| self.word(type_at)).map(simple).unwrap_or_default()
                };
                if !type_name.is_empty() {
                    let named = annotations
                        .iter()
                        .find(|annotation| annotation.name == "Bean")
                        .and_then(|annotation| annotation.texts(&["value", "name"]).into_iter().next())
                        .unwrap_or_else(|| name.clone());
                    self.facts.beans.push(Bean {
                        kind: BeanKind::Method,
                        type_name,
                        supertypes: Vec::new(),
                        name: named,
                        line,
                    });
                }
                for (type_name, parameter, line) in &parameters {
                    self.facts.injections.push(Injection {
                        type_name: type_name.clone(),
                        name: parameter.clone(),
                        owner: format!("{owner}.{name}"),
                        line: *line,
                    });
                }
            } else if (bean && constructor) || has("Autowired") || has("Inject") {
                for (type_name, parameter, line) in parameters {
                    self.facts.injections.push(Injection { type_name, name: parameter, owner: owner.clone(), line });
                }
            }
            for annotation in &annotations {
                let Some((_, method)) = MAPPINGS.iter().find(|(mapping, _)| *mapping == annotation.name) else {
                    continue;
                };
                let mut paths = annotation.texts(&["value", "path"]);
                if paths.is_empty() {
                    paths.push(String::new());
                }
                let methods = if method.is_empty() { annotation.methods() } else { vec![(*method).to_owned()] };
                let methods = if methods.is_empty() { vec![String::new()] } else { methods };
                let prefixes = if prefixes.is_empty() { vec![String::new()] } else { prefixes.clone() };
                for prefix in &prefixes {
                    for path in &paths {
                        for method in &methods {
                            self.facts.endpoints.push(Endpoint {
                                method: method.clone(),
                                path: join(prefix, path),
                                handler: format!("{owner}.{name}"),
                                line,
                            });
                        }
                    }
                }
            }
            self.at = close + 1;
            return;
        }
        if (has("Autowired") || has("Inject")) && !words.is_empty() {
            let (type_name, name, line) = if self.kotlin {
                let colon = (start..at).find(|&index| self.mark(index) == Some(':'));
                match colon {
                    Some(colon) => (
                        self.type_at(colon + 1).0,
                        colon.checked_sub(1).and_then(|before| self.word(before)).unwrap_or_default().to_owned(),
                        self.line(colon),
                    ),
                    None => (String::new(), String::new(), 0),
                }
            } else {
                let name_at = *words.last().unwrap_or(&start);
                let type_at = words.iter().rev().nth(1).copied().unwrap_or(start);
                (
                    self.word(type_at).map(simple).unwrap_or_default(),
                    self.word(name_at).unwrap_or_default().to_owned(),
                    self.line(name_at),
                )
            };
            if !type_name.is_empty() {
                self.facts.injections.push(Injection { type_name, name, owner, line });
            }
        }
        self.at = at.max(start + 1);
    }

    fn read(mut self) -> Facts {
        while self.at < self.tokens.len() {
            match &self.tokens[self.at].kind {
                Kind::Mark('@') => {
                    if self.word(self.at + 1) == Some("interface") {
                        self.class(self.at + 1);
                    } else {
                        self.annotation();
                    }
                }
                Kind::Mark('{') => {
                    self.depth += 1;
                    self.pending.clear();
                    self.at += 1;
                }
                Kind::Mark('}') => {
                    if self.classes.last().is_some_and(|class| class.depth == self.depth) {
                        self.classes.pop();
                    }
                    self.depth = self.depth.saturating_sub(1);
                    self.pending.clear();
                    self.at += 1;
                }
                Kind::Mark(';') => {
                    self.pending.clear();
                    self.at += 1;
                }
                Kind::Word(word)
                    if matches!(word.as_str(), "class" | "interface" | "enum" | "record" | "object")
                        && self.mark(self.at.wrapping_sub(1)) != Some('.')
                        && self.mark(self.at.wrapping_sub(1)) != Some(':') =>
                {
                    self.class(self.at);
                }
                Kind::Word(word) if MODIFIERS.contains(&word.as_str()) || word == "companion" => self.at += 1,
                Kind::Word(_) if self.classes.last().is_some_and(|class| class.depth == self.depth) => {
                    self.member();
                }
                _ => {
                    if self.classes.last().is_none_or(|class| class.depth != self.depth) {
                        self.pending.clear();
                    }
                    self.at += 1;
                }
            }
        }
        self.facts
    }
}

pub fn is_source(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "java" || extension == "kt")
}

pub fn read(path: &Path, text: &str) -> Facts {
    let kotlin = path.extension().is_some_and(|extension| extension == "kt");
    let reader = Reader {
        tokens: tokens(text),
        at: 0,
        depth: 0,
        classes: Vec::new(),
        pending: Vec::new(),
        facts: Facts::default(),
        kotlin,
    };
    reader.read()
}

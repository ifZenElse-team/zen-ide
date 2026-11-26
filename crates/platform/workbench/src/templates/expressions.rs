use zen_i18n::t;

use crate::file_templates::variables::{Moment, user};

pub const FUNCTIONS: &[&str] = &[
    "camelCase",
    "capitalize",
    "concat",
    "date",
    "decapitalize",
    "fileName",
    "fileNameWithoutExtension",
    "lineNumber",
    "lowercase",
    "snakeCase",
    "time",
    "uppercase",
    "user",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub file_name: String,
    pub line: usize,
    pub now: Moment,
}

impl Default for Place {
    fn default() -> Self {
        Self { file_name: String::new(), line: 0, now: Moment::at(std::time::UNIX_EPOCH, 0) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Text(String),
    Variable(String),
    Call(String, Vec<Node>),
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    source: &'a str,
}

impl Parser<'_> {
    fn skip_spaces(&mut self) {
        while self.chars.next_if(|(_, c)| c.is_whitespace()).is_some() {}
    }

    fn node(&mut self) -> Result<Node, String> {
        self.skip_spaces();
        match self.chars.peek().copied() {
            Some((_, '"')) => {
                self.chars.next();
                let mut text = String::new();
                loop {
                    match self.chars.next() {
                        Some((_, '\\')) => text.extend(self.chars.next().map(|(_, c)| c)),
                        Some((_, '"')) => return Ok(Node::Text(text)),
                        Some((_, c)) => text.push(c),
                        None => return Err(t!("workbench-templates-expression-quote")),
                    }
                }
            }
            Some((start, c)) if c.is_ascii_alphabetic() || c == '_' => {
                let mut end = start;
                while let Some((at, c)) = self.chars.next_if(|(_, c)| c.is_ascii_alphanumeric() || *c == '_') {
                    end = at + c.len_utf8();
                }
                let name = self.source[start..end].to_owned();
                self.skip_spaces();
                if self.chars.next_if(|(_, c)| *c == '(').is_none() {
                    return Ok(Node::Variable(name));
                }
                if !FUNCTIONS.contains(&name.as_str()) {
                    return Err(t!("workbench-templates-expression-function", name = name.as_str()));
                }
                let mut arguments = Vec::new();
                self.skip_spaces();
                if self.chars.next_if(|(_, c)| *c == ')').is_some() {
                    return Ok(Node::Call(name, arguments));
                }
                loop {
                    arguments.push(self.node()?);
                    self.skip_spaces();
                    match self.chars.next() {
                        Some((_, ',')) => {}
                        Some((_, ')')) => return Ok(Node::Call(name, arguments)),
                        _ => return Err(t!("workbench-templates-expression-syntax")),
                    }
                }
            }
            _ => Err(t!("workbench-templates-expression-syntax")),
        }
    }
}

fn parse(expression: &str) -> Result<Node, String> {
    let mut parser = Parser { chars: expression.char_indices().peekable(), source: expression };
    let node = parser.node()?;
    parser.skip_spaces();
    match parser.chars.next() {
        None => Ok(node),
        Some(_) => Err(t!("workbench-templates-expression-syntax")),
    }
}

pub fn check(expression: &str) -> Result<(), String> {
    if expression.trim().is_empty() { Ok(()) } else { parse(expression).map(|_| ()) }
}

pub fn references(expression: &str) -> Vec<String> {
    fn visit(node: &Node, found: &mut Vec<String>) {
        match node {
            Node::Text(_) => {}
            Node::Variable(name) => {
                if !found.contains(name) {
                    found.push(name.clone());
                }
            }
            Node::Call(_, arguments) => arguments.iter().for_each(|argument| visit(argument, found)),
        }
    }
    let mut found = Vec::new();
    if let Ok(node) = parse(expression) {
        visit(&node, &mut found);
    }
    found
}

fn words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    for c in text.chars() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            if c.is_uppercase() && previous.is_some_and(|before| before.is_lowercase() || before.is_ascii_digit()) {
                words.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
        previous = Some(c);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words.retain(|word| !word.is_empty());
    words
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn decapitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_lowercase().chain(chars).collect()).unwrap_or_default()
}

fn call(name: &str, arguments: &[String], place: &Place) -> String {
    let first = arguments.first().map_or("", String::as_str);
    match name {
        "camelCase" => {
            let words = words(first);
            words
                .iter()
                .enumerate()
                .map(|(index, word)| {
                    let lower = word.to_lowercase();
                    if index == 0 { lower } else { capitalize(&lower) }
                })
                .collect()
        }
        "snakeCase" => words(first).iter().map(|word| word.to_lowercase()).collect::<Vec<_>>().join("_"),
        "capitalize" => capitalize(first),
        "decapitalize" => decapitalize(first),
        "lowercase" => first.to_lowercase(),
        "uppercase" => first.to_uppercase(),
        "concat" => arguments.concat(),
        "fileName" => place.file_name.clone(),
        "fileNameWithoutExtension" => {
            place.file_name.rsplit_once('.').map_or(place.file_name.as_str(), |(stem, _)| stem).to_owned()
        }
        "lineNumber" => (place.line + 1).to_string(),
        "date" => format!("{:04}-{:02}-{:02}", place.now.year, place.now.month, place.now.day),
        "time" => format!("{:02}:{:02}", place.now.hour, place.now.minute),
        "user" => user(),
        _ => String::new(),
    }
}

fn run(node: &Node, values: &dyn Fn(&str) -> Option<String>, place: &Place) -> String {
    match node {
        Node::Text(text) => text.clone(),
        Node::Variable(name) => values(name).unwrap_or_default(),
        Node::Call(name, arguments) => {
            let arguments: Vec<String> = arguments.iter().map(|argument| run(argument, values, place)).collect();
            call(name, &arguments, place)
        }
    }
}

pub fn evaluate(expression: &str, values: &dyn Fn(&str) -> Option<String>, place: &Place) -> String {
    parse(expression).map(|node| run(&node, values, place)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> Place {
        let now = Moment { year: 2026, month: 3, day: 7, weekday: 5, hour: 9, minute: 4, second: 0 };
        Place { file_name: "user_store.go".to_owned(), line: 41, now }
    }

    fn eval(expression: &str) -> String {
        let values = |name: &str| (name == "TYPE").then(|| "HttpServer".to_owned());
        evaluate(expression, &values, &place())
    }

    #[test]
    fn functions_change_the_case_and_read_the_place() {
        assert_eq!(eval("decapitalize(TYPE)"), "httpServer");
        assert_eq!(eval("snakeCase(TYPE)"), "http_server");
        assert_eq!(eval("camelCase(fileNameWithoutExtension())"), "userStore");
        assert_eq!(eval("capitalize(\"x\")"), "X");
        assert_eq!(eval("concat(\"new\", TYPE, \"\\\"\")"), "newHttpServer\"");
        assert_eq!(eval("uppercase(lowercase(TYPE))"), "HTTPSERVER");
        assert_eq!(eval("fileName()"), "user_store.go");
        assert_eq!(eval("lineNumber()"), "42");
        assert_eq!((eval("date()"), eval("time()")), ("2026-03-07".to_owned(), "09:04".to_owned()));
        assert_eq!(eval("OTHER"), "", "a variable without a value gives nothing");
    }

    #[test]
    fn a_broken_expression_is_refused_and_gives_nothing() {
        assert!(check("").is_ok() && check("capitalize( TYPE )").is_ok());
        for broken in ["capitalize(", "nope()", "\"open", "a b", "concat(a,)"] {
            assert!(check(broken).is_err(), "{broken}");
            assert_eq!(eval(broken), "", "{broken}");
        }
        assert_eq!(references("concat(A, capitalize(B), A, \"C\")"), ["A", "B"]);
    }
}

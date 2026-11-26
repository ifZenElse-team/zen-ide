use crate::dialect::Dialect;
use crate::lexer::{Token, TokenKind};
use crate::script::Script;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

pub const MAX_SYMBOLS: usize = 20_000;

const MAX_NAME: usize = 120;

const OBJECTS: &[(&str, &str)] = &[
    ("TABLE", "class"),
    ("VIEW", "interface"),
    ("FUNCTION", "function"),
    ("PROCEDURE", "function"),
    ("TRIGGER", "method"),
    ("INDEX", "key"),
    ("SCHEMA", "module"),
    ("DATABASE", "module"),
    ("SEQUENCE", "constant"),
    ("TYPE", "type"),
    ("DOMAIN", "type"),
    ("EXTENSION", "module"),
    ("ROLE", "variable"),
    ("USER", "variable"),
    ("EVENT", "method"),
    ("POLICY", "rule"),
    ("RULE", "rule"),
    ("DICTIONARY", "class"),
];

const NOT_COLUMNS: &[&str] = &[
    "CONSTRAINT",
    "PRIMARY",
    "FOREIGN",
    "UNIQUE",
    "CHECK",
    "KEY",
    "INDEX",
    "EXCLUDE",
    "LIKE",
    "FULLTEXT",
    "SPATIAL",
    "PERIOD",
    "PROJECTION",
];

const NAMED_AFTER: &[&str] =
    &["USE", "CALL", "EXEC", "EXECUTE", "DESCRIBE", "DESC", "PRAGMA", "SHOW", "SET", "VACUUM", "ANALYZE", "OPTIMIZE"];

pub fn outline(text: &str, dialect: Dialect) -> Vec<Symbol> {
    let script = Script::read(text, dialect);
    let mut symbols = Vec::new();
    let mut count = 0;
    for statement in &script.statements {
        let tokens: Vec<Token> = script.significant(statement).map(|(_, token)| token).collect();
        let span = script.span(statement);
        let Some(mut symbol) = describe(text, &tokens) else { continue };
        symbol.start = span.start;
        symbol.end = span.end;
        symbol.children.retain(|child| child.start >= span.start && child.end <= span.end);
        count += 1 + symbol.children.len();
        if count > MAX_SYMBOLS {
            break;
        }
        symbols.push(symbol);
    }
    symbols
}

fn symbol(name: String, kind: &'static str) -> Symbol {
    let name = if name.chars().count() > MAX_NAME { name.chars().take(MAX_NAME).collect() } else { name };
    Symbol { name, kind, start: 0, end: 0, children: Vec::new() }
}

fn upper(text: &str, token: &Token) -> String {
    if token.kind == TokenKind::Word { token.text(text).to_ascii_uppercase() } else { String::new() }
}

fn describe(text: &str, tokens: &[Token]) -> Option<Symbol> {
    let at = tokens.iter().position(|token| token.kind == TokenKind::Word)?;
    let verb = upper(text, &tokens[at]);
    let rest = &tokens[at + 1..];
    Some(match verb.as_str() {
        "CREATE" => create(text, rest).unwrap_or_else(|| symbol("CREATE".to_owned(), "key")),
        "ALTER" | "DROP" | "TRUNCATE" | "COMMENT" | "RENAME" | "REFRESH" | "REINDEX" => {
            let (object, kind, name) = object_and_name(text, rest);
            let phrase = [verb.as_str(), object.as_str(), name.as_deref().unwrap_or("")]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            symbol(phrase, kind)
        }
        "WITH" => with(text, rest),
        _ => symbol(phrase(text, &tokens[at..]), "key"),
    })
}

fn create(text: &str, rest: &[Token]) -> Option<Symbol> {
    let (object_at, kind) = rest.iter().take(16).enumerate().find_map(|(index, token)| {
        let word = upper(text, token);
        OBJECTS.iter().find(|(object, _)| *object == word).map(|(_, kind)| (index, *kind))
    })?;
    let object = upper(text, &rest[object_at]);
    let mut at = object_at + 1;
    while rest
        .get(at)
        .is_some_and(|token| ["IF", "NOT", "EXISTS", "CONCURRENTLY"].iter().any(|word| token.is(text, word)))
    {
        at += 1;
    }
    let Some((name, after)) = qualified_name(text, rest, at) else {
        return Some(symbol(object, kind));
    };
    if rest.get(at).is_some_and(|token| token.is(text, "ON")) {
        return Some(symbol(object, kind));
    }
    let mut created = symbol(name, kind);
    if object == "TABLE" {
        created.children = columns(text, &rest[after..]);
    }
    Some(created)
}

fn object_and_name(text: &str, rest: &[Token]) -> (String, &'static str, Option<String>) {
    let Some((index, kind)) = rest.iter().take(4).enumerate().find_map(|(index, token)| {
        let word = upper(text, token);
        OBJECTS.iter().find(|(object, _)| *object == word).map(|(_, kind)| (index, *kind))
    }) else {
        return (rest.first().map(|token| upper(text, token)).unwrap_or_default(), "key", None);
    };
    let mut at = index + 1;
    while rest.get(at).is_some_and(|token| ["IF", "EXISTS", "CONCURRENTLY"].iter().any(|word| token.is(text, word))) {
        at += 1;
    }
    let words: Vec<String> = rest[..=index].iter().map(|token| upper(text, token)).collect();
    (words.join(" "), kind, qualified_name(text, rest, at).map(|(name, _)| name))
}

fn qualified_name(text: &str, tokens: &[Token], at: usize) -> Option<(String, usize)> {
    let named = |token: &Token| matches!(token.kind, TokenKind::Word | TokenKind::Quoted);
    let first = tokens.get(at).filter(|token| named(token))?;
    let mut name = first.text(text).to_owned();
    let mut index = at + 1;
    while tokens.get(index).is_some_and(|token| token.is_symbol(text, '.'))
        && let Some(part) = tokens.get(index + 1).filter(|token| named(token))
    {
        name.push('.');
        name.push_str(part.text(text));
        index += 2;
    }
    Some((name, index))
}

fn columns(text: &str, tokens: &[Token]) -> Vec<Symbol> {
    let Some(open) = tokens.iter().position(|token| token.is_symbol(text, '(')) else { return Vec::new() };
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut element: Option<(Token, Token)> = None;
    for token in &tokens[open..] {
        if token.is_symbol(text, '(') {
            depth += 1;
            if depth == 1 {
                continue;
            }
        } else if token.is_symbol(text, ')') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                found.extend(element.take());
                break;
            }
        } else if depth == 1 && token.is_symbol(text, ',') {
            found.extend(element.take());
            continue;
        }
        match &mut element {
            Some((_, last)) => *last = *token,
            None => element = Some((*token, *token)),
        }
    }
    found
        .into_iter()
        .filter(|(first, _)| {
            matches!(first.kind, TokenKind::Word | TokenKind::Quoted)
                && !NOT_COLUMNS.iter().any(|word| first.is(text, word))
        })
        .map(|(first, last)| {
            let mut column = symbol(first.text(text).to_owned(), "field");
            column.start = first.start;
            column.end = last.end;
            column
        })
        .collect()
}

fn with(text: &str, rest: &[Token]) -> Symbol {
    let mut at = usize::from(rest.first().is_some_and(|token| token.is(text, "RECURSIVE")));
    let mut ctes = Vec::new();
    while let Some(name) = rest.get(at).filter(|token| matches!(token.kind, TokenKind::Word | TokenKind::Quoted)) {
        let mut index = at + 1;
        if rest.get(index).is_some_and(|token| token.is_symbol(text, '(')) {
            index = closing(text, rest, index) + 1;
        }
        if !rest.get(index).is_some_and(|token| token.is(text, "AS")) {
            break;
        }
        index += 1;
        while rest.get(index).is_some_and(|token| token.is(text, "NOT") || token.is(text, "MATERIALIZED")) {
            index += 1;
        }
        if !rest.get(index).is_some_and(|token| token.is_symbol(text, '(')) {
            break;
        }
        let close = closing(text, rest, index);
        let mut cte = symbol(name.text(text).to_owned(), "variable");
        cte.start = name.start;
        cte.end = rest.get(close).map_or(name.end, |token| token.end);
        ctes.push(cte);
        at = close + 1;
        if !rest.get(at).is_some_and(|token| token.is_symbol(text, ',')) {
            break;
        }
        at += 1;
    }
    let main = rest.get(at..).filter(|main| !main.is_empty());
    let mut statement = symbol(main.map_or_else(|| "WITH".to_owned(), |main| phrase(text, main)), "key");
    statement.children = ctes;
    statement
}

fn closing(text: &str, tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is_symbol(text, '(') {
            depth += 1;
        } else if token.is_symbol(text, ')') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return index;
            }
        }
    }
    tokens.len().saturating_sub(1)
}

fn phrase(text: &str, tokens: &[Token]) -> String {
    let Some(at) = tokens.iter().position(|token| token.kind == TokenKind::Word) else { return String::new() };
    let verb = upper(text, &tokens[at]);
    let rest = &tokens[at + 1..];
    let target = |after: &[&str]| -> Option<String> {
        let mut depth = 0usize;
        for (index, token) in rest.iter().enumerate() {
            if token.is_symbol(text, '(') {
                depth += 1;
            } else if token.is_symbol(text, ')') {
                depth = depth.saturating_sub(1);
            } else if depth == 0 && after.iter().any(|word| token.is(text, word)) {
                let mut name_at = index + 1;
                while rest.get(name_at).is_some_and(|token| {
                    ["INTO", "FROM", "ONLY", "IGNORE", "LOW_PRIORITY", "TABLE"].iter().any(|word| token.is(text, word))
                }) {
                    name_at += 1;
                }
                return qualified_name(text, rest, name_at).map(|(name, _)| name);
            }
        }
        None
    };
    let with_target = |words: &str, target: Option<String>| match target {
        Some(name) => format!("{words} {name}"),
        None => words.to_owned(),
    };
    match verb.as_str() {
        "SELECT" => target(&["FROM"]).map_or_else(|| "SELECT".to_owned(), |name| format!("SELECT … FROM {name}")),
        "INSERT" | "REPLACE" => with_target(
            &format!("{verb} INTO"),
            qualified_after(text, rest, &["INTO", "IGNORE", "LOW_PRIORITY", "OR", "ROLLBACK", "ABORT", "FAIL"]),
        ),
        "UPDATE" => with_target(
            "UPDATE",
            qualified_after(
                text,
                rest,
                &["ONLY", "LOW_PRIORITY", "IGNORE", "OR", "REPLACE", "ROLLBACK", "ABORT", "FAIL"],
            ),
        ),
        "DELETE" => with_target("DELETE FROM", target(&["FROM"]).or_else(|| qualified_after(text, rest, &[]))),
        "MERGE" => with_target("MERGE INTO", qualified_after(text, rest, &["INTO"])),
        _ if NAMED_AFTER.contains(&verb.as_str()) => {
            with_target(&verb, qualified_name(text, rest, 0).map(|(name, _)| name))
        }
        _ => verb,
    }
}

fn qualified_after(text: &str, rest: &[Token], skipped: &[&str]) -> Option<String> {
    let at = rest.iter().take_while(|token| skipped.iter().any(|word| token.is(text, word))).count();
    qualified_name(text, rest, at).map(|(name, _)| name)
}

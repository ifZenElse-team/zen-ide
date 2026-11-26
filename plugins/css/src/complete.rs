use crate::data::{self, Property};
use crate::token::{Tok, Token, is_name_byte, scss, tokenize};
use crate::tree::{Node, NodeKind, parse};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completions {
    pub from: usize,
    pub items: Vec<Item>,
}

enum Context {
    Selector,
    Property,
    Value { property: String, value_start: usize },
    Other,
}

pub fn complete(text: &str, caret: usize) -> Option<Completions> {
    let mut caret = caret.min(text.len());
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    let bytes = text.as_bytes();
    let tokens = tokenize(text);
    if tokens.iter().any(|token| inside_literal(text, token, caret)) {
        return None;
    }
    let word_start = caret - bytes[..caret].iter().rev().take_while(|b| is_word_byte(**b)).count();
    let prefix = &text[word_start..caret];
    let before = |n: usize| word_start.checked_sub(n).map(|i| bytes[i]);
    if before(1) == Some(b'@') {
        let scss_rules = if scss() { data::SCSS_AT_RULES } else { &[] };
        let items = data::AT_RULES
            .iter()
            .chain(scss_rules)
            .map(|(name, rest)| item(format!("@{name}"), "keyword", format!("@{name}{rest}")))
            .collect();
        return finish(word_start - 1, &text[word_start - 1..caret], items);
    }
    if scss() && before(1) == Some(b'$') {
        let items =
            variables(text, &tokens, caret).into_iter().map(|name| item(name.clone(), "variable", name)).collect();
        return finish(word_start - 1, &text[word_start - 1..caret], items);
    }
    if scss() && text[..word_start].trim_end().ends_with("@include") {
        let items = mixins(text, &tokens).into_iter().map(|name| item(name.clone(), "function", name)).collect();
        return finish(word_start, prefix, items);
    }
    let nodes = parse(text, &tokens);
    let context = locate(text, &nodes, &tokens, caret, false);
    match context {
        Context::Selector => selector(text, word_start, prefix),
        Context::Property => {
            if prefix.starts_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
            let after = &text[caret..];
            let rest = after.trim_start_matches(|c: char| c.is_ascii() && is_word_byte(c as u8)).trim_start();
            let has_colon = rest.starts_with(':');
            let insert = |name: &str| if has_colon { name.to_owned() } else { format!("{name}: $0;") };
            let mut items: Vec<Item> = data::PROPERTIES
                .iter()
                .map(|property| item(property.name.to_owned(), "property", insert(property.name)))
                .collect();
            items.extend(
                custom_properties(text, &tokens, caret)
                    .into_iter()
                    .map(|name| item(name.clone(), "variable", insert(&name))),
            );
            finish(word_start, prefix, items)
        }
        Context::Value { property, value_start } => {
            let known = data::property(&property);
            if known.is_none()
                && !property.starts_with("--")
                && before(1) == Some(b':')
                && before(2).is_some_and(|b| b != b' ')
            {
                return selector(text, word_start, prefix);
            }
            value(text, &tokens, caret, word_start, value_start, known)
        }
        Context::Other => None,
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

fn item(label: String, kind: &'static str, insert: String) -> Item {
    Item { label, kind, insert }
}

const MAX_ITEMS: usize = 2000;

fn finish(from: usize, prefix: &str, items: Vec<Item>) -> Option<Completions> {
    let mut seen = std::collections::HashSet::<String>::new();
    let items: Vec<Item> = items
        .into_iter()
        .filter(|item| {
            item.label.len() >= prefix.len()
                && item.label.is_char_boundary(prefix.len())
                && item.label[..prefix.len()].eq_ignore_ascii_case(prefix)
                && seen.insert(item.label.clone())
        })
        .take(MAX_ITEMS)
        .collect();
    (!items.is_empty()).then_some(Completions { from, items })
}

fn inside_literal(text: &str, token: &Token, caret: usize) -> bool {
    let body = &text[token.start..token.end];
    match token.tok {
        Tok::Comment => {
            let closed = body.len() >= 4 && body.ends_with("*/");
            caret > token.start + 1 && (caret < token.end || (caret == token.end && !closed))
        }
        Tok::String => {
            let closed = body.len() >= 2 && body.as_bytes()[body.len() - 1] == body.as_bytes()[0];
            caret > token.start && (caret < token.end || (caret == token.end && !closed))
        }
        Tok::Url => caret > token.start + 3 && (caret < token.end || (caret == token.end && !body.ends_with(')'))),
        _ => false,
    }
}

fn locate(text: &str, nodes: &[Node], tokens: &[Token], caret: usize, mixed: bool) -> Context {
    for node in nodes {
        if caret < tokens[node.first].start {
            break;
        }
        if let Some(block) = &node.block {
            if caret <= tokens[block.open].start {
                return prelude_context(node);
            }
            let inside = block.close.is_none_or(|close| caret <= tokens[close].start);
            if inside {
                return locate(text, &block.children, tokens, caret, block.mixed);
            }
            continue;
        }
        let end = match node.semicolon {
            Some(semicolon) => tokens[semicolon].start,
            None => tokens[node.end - 1].end,
        };
        if caret > end {
            continue;
        }
        if node.kind != NodeKind::Declaration {
            return prelude_context(node);
        }
        return match node.colon {
            Some(colon) if caret >= tokens[colon].end => {
                let name = &tokens[node.first];
                Context::Value { property: text[name.start..name.end].to_owned(), value_start: tokens[colon].end }
            }
            _ => Context::Property,
        };
    }
    if mixed { Context::Property } else { Context::Selector }
}

fn prelude_context(node: &Node) -> Context {
    match node.kind {
        NodeKind::Rule => Context::Selector,
        _ => Context::Other,
    }
}

fn selector(text: &str, word_start: usize, prefix: &str) -> Option<Completions> {
    let head = &text[..word_start];
    let names = if head.ends_with("::") {
        data::PSEUDO_ELEMENTS
    } else if head.ends_with(':') {
        data::PSEUDO_CLASSES
    } else {
        return None;
    };
    let items = names
        .iter()
        .map(|name| match name.strip_suffix("()") {
            Some(bare) => item((*name).to_owned(), "keyword", format!("{bare}($0)")),
            None => item((*name).to_owned(), "keyword", (*name).to_owned()),
        })
        .collect();
    finish(word_start, prefix, items)
}

fn value(
    text: &str,
    tokens: &[Token],
    caret: usize,
    word_start: usize,
    value_start: usize,
    property: Option<&Property>,
) -> Option<Completions> {
    let bytes = text.as_bytes();
    let letters = caret - bytes[..caret].iter().rev().take_while(|b| b.is_ascii_alphabetic() || **b == b'%').count();
    let digits = letters - bytes[..letters].iter().rev().take_while(|b| b.is_ascii_digit() || **b == b'.').count();
    let number = digits < letters && bytes[letters - 1].is_ascii_digit();
    let standalone = digits == 0
        || matches!(bytes[digits - 1], b'-' | b'+')
        || !is_name_byte(bytes[digits - 1]) && bytes[digits - 1] != b'#';
    if number && standalone {
        let items = data::UNITS.iter().map(|unit| item((*unit).to_owned(), "value", (*unit).to_owned())).collect();
        return finish(letters, &text[letters..caret], items);
    }
    let prefix = &text[word_start..caret];
    match word_start.checked_sub(1).map(|i| bytes[i]) {
        Some(b'!') => {
            return finish(word_start, prefix, vec![item("important".to_owned(), "keyword", "important".to_owned())]);
        }
        Some(b'#') => return None,
        _ => {}
    }
    let variables = custom_properties(text, tokens, caret);
    if innermost_function(text, tokens, value_start, caret).is_some_and(|name| name.eq_ignore_ascii_case("var")) {
        let items = variables.into_iter().map(|name| item(name.clone(), "variable", name)).collect();
        return finish(word_start, prefix, items);
    }
    let mut items = Vec::new();
    if let Some(property) = property {
        items.extend(property.values.iter().map(|value| item((*value).to_owned(), "value", (*value).to_owned())));
        if property.colour {
            items.extend(data::COLOURS.iter().map(|colour| item((*colour).to_owned(), "value", (*colour).to_owned())));
        }
    }
    items.extend(data::GLOBAL_VALUES.iter().map(|value| item((*value).to_owned(), "keyword", (*value).to_owned())));
    items.extend(
        data::functions(property).into_iter().map(|name| item(format!("{name}()"), "function", format!("{name}($0)"))),
    );
    if prefix.starts_with("--") {
        items = variables.into_iter().map(|name| item(name.clone(), "variable", format!("var({name})"))).collect();
    }
    finish(word_start, prefix, items)
}

fn innermost_function<'a>(text: &'a str, tokens: &[Token], from: usize, caret: usize) -> Option<&'a str> {
    let mut stack: Vec<Option<&str>> = Vec::new();
    let mut pending = None;
    for token in tokens.iter().filter(|t| t.start >= from && t.end <= caret) {
        match token.tok {
            Tok::Function => pending = Some(&text[token.start..token.end]),
            Tok::OpenParen => stack.push(pending.take()),
            Tok::CloseParen => {
                stack.pop();
            }
            _ => pending = None,
        }
    }
    stack.last().copied().flatten()
}

fn variables(text: &str, tokens: &[Token], caret: usize) -> Vec<String> {
    let mut names: Vec<String> = tokens
        .iter()
        .filter(|t| t.tok == Tok::Variable && !(t.start <= caret && caret <= t.end))
        .map(|t| text[t.start..t.end].to_owned())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

fn mixins(text: &str, tokens: &[Token]) -> Vec<String> {
    let mut names: Vec<String> = tokens
        .windows(3)
        .filter(|w| w[0].tok == Tok::AtKeyword && text[w[0].start..w[0].end].eq_ignore_ascii_case("@mixin"))
        .filter(|w| w[1].tok == Tok::Space && matches!(w[2].tok, Tok::Ident | Tok::Function))
        .map(|w| text[w[2].start..w[2].end].to_owned())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

fn custom_properties(text: &str, tokens: &[Token], caret: usize) -> Vec<String> {
    let mut names: Vec<String> = tokens
        .iter()
        .filter(|t| t.tok == Tok::Ident && !(t.start <= caret && caret <= t.end))
        .map(|t| &text[t.start..t.end])
        .filter(|name| name.starts_with("--") && name.len() > 2)
        .map(str::to_owned)
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_custom_properties_stay_fast_and_bounded() {
        let variables: String = (0..60_000).map(|n| format!("--v{n}:1;")).collect();
        let found = at(&format!(":root{{{variables}}}.a{{ | }}")).expect("items");
        assert_eq!(found.items.len(), MAX_ITEMS);
    }

    fn at(text_with_caret: &str) -> Option<Completions> {
        let caret = text_with_caret.find('|').unwrap_or(text_with_caret.len());
        let text = text_with_caret.replacen('|', "", 1);
        complete(&text, caret)
    }

    fn labels(text_with_caret: &str) -> Vec<String> {
        at(text_with_caret).map(|c| c.items.into_iter().map(|item| item.label).collect()).unwrap_or_default()
    }

    fn find(text_with_caret: &str, label: &str) -> Option<Item> {
        at(text_with_caret)?.items.into_iter().find(|item| item.label == label)
    }

    #[test]
    fn completes_property_names() {
        let completions = at(".a { dis| }").unwrap_or_else(|| panic!("no completions"));
        assert_eq!(completions.from, 5);
        assert_eq!(completions.items[0], item("display".to_owned(), "property", "display: $0;".to_owned()));
        assert_eq!(labels(".a {\n  color: red;\n  ju|\n}"), ["justify-content", "justify-items", "justify-self"]);
        assert_eq!(find(".a { |", "z-index").map(|i| i.insert), Some("z-index: $0;".to_owned()));
        assert_eq!(find(".a { fl|ex: 1 }", "flex").map(|i| i.insert), Some("flex".to_owned()));
        assert!(labels(":root { --main: 1px } .a { --m| }").contains(&"--main".to_owned()));
        assert!(labels("@font-face { font-d| }").contains(&"font-display".to_owned()));
        assert!(labels("@media print { .a { tr| } }").contains(&"transform".to_owned()));
    }

    #[test]
    fn completes_values_of_the_property() {
        let display = labels(".a { display: | }");
        for value in ["block", "flex", "inline-grid", "none", "inherit", "var()"] {
            assert!(display.contains(&value.to_owned()), "{value}");
        }
        assert!(!display.contains(&"red".to_owned()));
        assert_eq!(labels(".a { position: st| }"), ["static", "sticky"]);
        assert_eq!(labels(".a { flex-direction: row-| }"), ["row-reverse"]);
        assert_eq!(labels(".a { justify-content: space-| }"), ["space-between", "space-around", "space-evenly"]);
        assert!(labels(".a { color: rebecca| }").contains(&"rebeccapurple".to_owned()));
        assert!(labels(".a { border: 1px solid tom| }").contains(&"tomato".to_owned()));
        assert!(labels(".a { color: ok|").contains(&"oklch()".to_owned()));
        assert_eq!(find(".a { width: cal| }", "calc()").map(|i| i.insert), Some("calc($0)".to_owned()));
        assert_eq!(labels(".a { color: red !imp| }"), ["important"]);
        assert_eq!(
            labels(".a { -webkit-transform: rot| }"),
            ["rotate()", "rotateX()", "rotateY()", "rotateZ()", "rotate3d()"]
        );
        assert_eq!(labels(".a { color:bl| }"), ["black", "blanchedalmond", "blue", "blueviolet"]);
    }

    #[test]
    fn completes_units_and_variables() {
        let completions = at(".a { margin: 0 12| }").unwrap_or_else(|| panic!("no completions"));
        assert_eq!(completions.from, 17);
        assert!(completions.items.iter().any(|item| item.label == "px"));
        assert_eq!(labels(".a { width: 1.5r| }"), ["rem", "rlh", "rad"]);
        assert_eq!(labels(".a { width: calc(100% - 2v| }"), ["vw", "vh", "vmin", "vmax", "vi", "vb"]);
        assert!(labels(".a { color: #12| }").is_empty());
        let text = ":root { --gap: 4px; --fg: #000 }\n.a { margin: var(--| }";
        assert_eq!(labels(text), ["--fg", "--gap"]);
        assert_eq!(labels(":root { --gap: 4px }\n.a { margin: var(| }"), ["--gap"]);
        assert_eq!(
            find(":root { --gap: 4px }\n.a { margin: --g| }", "--gap").map(|i| i.insert),
            Some("var(--gap)".to_owned())
        );
    }

    #[test]
    fn completes_at_rules_and_pseudo_classes() {
        let completions = at("@me|").unwrap_or_else(|| panic!("no completions"));
        assert_eq!(completions.from, 0);
        assert_eq!(completions.items, [item("@media".to_owned(), "keyword", "@media $0".to_owned())]);
        assert_eq!(find("a { @|", "@media").map(|i| i.insert), Some("@media $0".to_owned()));
        assert_eq!(find("@font-f|", "@font-face").map(|i| i.insert), Some("@font-face {\n\t$0\n}".to_owned()));
        assert_eq!(labels("a:ho|"), ["host", "host()", "host-context()", "hover"]);
        let not = find("li:n| {}", "not()").unwrap_or_else(|| panic!("no :not"));
        assert_eq!(not.insert, "not($0)");
        assert_eq!(labels("p::be|"), ["before"]);
        assert_eq!(labels(".a { &:foc| }"), ["focus", "focus-visible", "focus-within"]);
        assert!(labels("@media screen { a:fi| }").contains(&"first-child".to_owned()));
    }

    #[test]
    fn completes_scss() {
        if !scss() {
            assert!(!labels("$a: 1; .b { c: $| }").contains(&"$a".to_owned()));
            assert!(!labels("@inc|").contains(&"@include".to_owned()));
            return;
        }
        assert_eq!(labels("$gap: 1px; $grid: 2; .b { margin: $g| }"), ["$gap", "$grid"]);
        let completions = at("$gap: 1px; .b { margin: $| }").unwrap_or_else(|| panic!("no completions"));
        assert_eq!(completions.from, 24);
        assert_eq!(find("@inc|", "@include").map(|i| i.insert), Some("@include $0;".to_owned()));
        assert!(labels(".a { @|").contains(&"@extend".to_owned()));
        assert_eq!(labels("@mixin pad($x) {}\n@mixin round {}\n.a { @include |"), ["pad", "round"]);
        assert!(labels(".a { // dis|\n }").is_empty());
        assert!(labels(".a { dis| }").contains(&"display".to_owned()));
    }

    #[test]
    fn stays_quiet_where_nothing_fits() {
        assert!(at(".a { content: \"dis|\" }").is_none());
        assert!(at("/* dis| */").is_none());
        assert!(at("/* dis|").is_none());
        assert!(at(".a { background: url(a|.png) }").is_none());
        assert!(at("@media (min-w|) {}").is_none());
        assert!(at("di|").is_none());
        assert!(at(".a { display: zzz| }").is_none());
        assert!(complete("é", 1).is_none());
        assert!(complete("a { b", 99).is_some());
    }

    #[test]
    fn survives_any_caret() {
        let text = "@media (x: 1) { a:hover::before, b[c=\"d\"] { color: var(--x, #fff) !important; w: 1.5e3px } }\n/* é */ @fo";
        for caret in 0..=text.len() + 1 {
            if let Some(completions) = complete(text, caret) {
                assert!(completions.from <= caret.min(text.len()) && !completions.items.is_empty());
            }
        }
    }
}

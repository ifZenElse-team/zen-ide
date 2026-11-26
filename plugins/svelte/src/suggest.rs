use crate::lexer::{End, Tok, lex};
use crate::markup::{Markup, NodeKind};
use crate::sfc::{Content, unquote};
use crate::spans::DIRECTIVES;
use crate::{color, script, with_style_dialect};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub from: usize,
    pub items: Vec<Item>,
}

const RUNES: &[(&str, &str)] = &[
    ("$state", "$state($0)"),
    ("$state.raw", "$state.raw($0)"),
    ("$derived", "$derived($0)"),
    ("$derived.by", "$derived.by(() => $0)"),
    ("$effect", "$effect(() => {\n\t$0\n})"),
    ("$effect.pre", "$effect.pre(() => {\n\t$0\n})"),
    ("$props", "$props()"),
    ("$bindable", "$bindable($0)"),
    ("$inspect", "$inspect($0)"),
    ("$host", "$host()"),
];

const OPENERS: &[(&str, &str)] = &[
    ("if", "if $0}"),
    ("each", "each $0 as item}"),
    ("await", "await $0}"),
    ("key", "key $0}"),
    ("snippet", "snippet $0()}"),
];
const BRANCHES: &[(&str, &str)] =
    &[("else", "else}"), ("else if", "else if $0}"), ("then", "then $0}"), ("catch", "catch $0}")];
const TAGS: &[(&str, &str)] =
    &[("html", "html $0}"), ("const", "const $0}"), ("render", "render $0()}"), ("debug", "debug $0}")];

const BLOCKS: &[(&str, &str)] = &[
    ("script lang=\"ts\"", "script lang=\"ts\">\n\t$0\n</script>"),
    ("script", "script>\n\t$0\n</script>"),
    ("script module", "script module>\n\t$0\n</script>"),
    ("style", "style>\n\t$0\n</style>"),
    ("style lang=\"scss\"", "style lang=\"scss\">\n\t$0\n</style>"),
];

fn item(label: &str, kind: &'static str, insert: String) -> Item {
    Item { label: label.to_owned(), kind, insert, detail: None }
}

pub fn complete(text: &str, caret: usize) -> Option<Completion> {
    let mut caret = caret.min(text.len());
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    let markup = Markup::read(text);
    if let Some(block) = markup.block_at(caret).filter(|block| text[block.start..block.inner.0].ends_with('>')) {
        let base = block.inner.0;
        let inner = &text[base..block.inner.1];
        return match block.content {
            Content::JavaScript | Content::TypeScript => {
                let dialect = if block.content == Content::TypeScript {
                    script::Dialect::TypeScript
                } else {
                    script::Dialect::JavaScript
                };
                script_completion(inner, caret - base, dialect)
                    .map(|found| Completion { from: base + found.from, ..found })
            }
            Content::Css | Content::Scss => {
                let found = with_style_dialect(block.content, || crate::complete::complete(inner, caret - base))?;
                let items = found
                    .items
                    .into_iter()
                    .map(|item| Item { label: item.label, kind: item.kind, insert: item.insert, detail: None })
                    .collect::<Vec<_>>();
                (!items.is_empty()).then_some(Completion { from: base + found.from, items })
            }
            Content::Template | Content::Other => None,
        };
    }
    block_completion(text, &markup, caret)
        .or_else(|| directive_completion(&markup.masked, caret))
        .or_else(|| top_level(&markup, caret))
}

fn script_completion(source: &str, caret: usize, dialect: script::Dialect) -> Option<Completion> {
    let found = script::complete(source, caret, dialect);
    let mut items: Vec<Item> = found
        .items
        .into_iter()
        .map(|item| Item { label: item.label, kind: item.kind, insert: item.insert, detail: item.detail })
        .collect();
    let word_start = source[..caret]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '$' | '.'))
        .last()
        .map_or(caret, |(at, _)| at);
    let typed = &source[word_start..caret];
    let mut from = found.from;
    if typed.starts_with('$') {
        let runes: Vec<Item> = RUNES
            .iter()
            .filter(|(rune, _)| rune.starts_with(typed))
            .map(|(rune, insert)| item(rune, "function", (*insert).to_owned()))
            .collect();
        if !runes.is_empty() {
            from = word_start;
            items.retain(|item| item.label.starts_with('$'));
            items.splice(0..0, runes);
        }
    }
    (!items.is_empty()).then_some(Completion { from, items })
}

fn block_completion(text: &str, markup: &Markup, caret: usize) -> Option<Completion> {
    let line_start = text[..caret].rfind(['\n', '}']).map_or(0, |at| at + 1);
    let open = text[line_start..caret].rfind('{').map(|at| line_start + at)?;
    let after = &text[open + 1..caret];
    let sigil = after.chars().next().filter(char::is_ascii)?;
    let typed = &after[1..];
    if !typed.chars().all(|c| c.is_ascii_alphabetic() || c == ' ') {
        return None;
    }
    let from = open + 2;
    let offered: Vec<Item> = match sigil {
        '#' => OPENERS.iter().map(|(label, insert)| item(label, "keyword", (*insert).to_owned())).collect(),
        ':' => BRANCHES.iter().map(|(label, insert)| item(label, "keyword", (*insert).to_owned())).collect(),
        '@' => TAGS.iter().map(|(label, insert)| item(label, "keyword", (*insert).to_owned())).collect(),
        '/' => {
            let innermost = markup
                .nodes
                .iter()
                .filter(|node| {
                    node.kind == NodeKind::Block && node.open_end <= open && (!node.closed || node.close_start >= open)
                })
                .max_by_key(|node| node.start)?;
            vec![item(&innermost.name, "keyword", format!("{}}}", innermost.name))]
        }
        _ => return None,
    };
    let items: Vec<Item> = offered.into_iter().filter(|item| item.label.starts_with(typed)).collect();
    (!items.is_empty()).then_some(Completion { from, items })
}

fn directive_completion(masked: &str, caret: usize) -> Option<Completion> {
    let (_, end) = lex(&masked[..caret]);
    let End::AttrName { attr, .. } = end else { return None };
    let typed = &masked[attr..caret];
    if typed.is_empty() || typed.contains(':') {
        return None;
    }
    let items: Vec<Item> = DIRECTIVES
        .iter()
        .filter(|name| name.starts_with(typed))
        .map(|name| item(&format!("{name}:"), "attribute", format!("{name}:$0")))
        .collect();
    (!items.is_empty()).then_some(Completion { from: attr, items })
}

fn top_level(markup: &Markup, caret: usize) -> Option<Completion> {
    let before = &markup.masked[..caret];
    let (_, end) = lex(before);
    let End::TagName { lt, closing: false } = end else { return None };
    let inside = markup.nodes.iter().any(|node| node.open_end <= lt && lt < node.close_start);
    if inside {
        return None;
    }
    let typed = &before[lt + 1..];
    let items: Vec<Item> = BLOCKS
        .iter()
        .filter(|(label, _)| label.starts_with(typed))
        .map(|(label, insert)| item(label, "snippet", (*insert).to_owned()))
        .collect();
    (!items.is_empty()).then_some(Completion { from: lt + 1, items })
}

pub(crate) fn colors(text: &str) -> Vec<color::Color> {
    let markup = Markup::read(text);
    let mut found = Vec::new();
    for block in &markup.blocks {
        if matches!(block.content, Content::Css | Content::Scss) {
            let syntax = color::Syntax { scss: block.content == Content::Scss, in_block: false };
            color::colors(text, block.inner.0..block.inner.1, syntax, &mut found);
        }
    }
    let tokens = &markup.tokens;
    for (at, token) in tokens.iter().enumerate() {
        let is_style = at >= 2
            && token.kind == Tok::AttrValue
            && tokens[at - 1].kind == Tok::Eq
            && tokens[at - 2].kind == Tok::AttrName
            && text[tokens[at - 2].start..tokens[at - 2].end].eq_ignore_ascii_case("style")
            && !markup.in_code(token.start);
        if is_style {
            let value = &text[token.start..token.end];
            if value.contains('{') {
                continue;
            }
            let start = token.start + usize::from(value.starts_with(['"', '\'']));
            let end = start + unquote(value).len();
            color::colors(text, start..end, color::Syntax { scss: false, in_block: true }, &mut found);
        }
    }
    found.sort_by_key(|color| color.start);
    found
}

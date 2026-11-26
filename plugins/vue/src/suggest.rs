use crate::lexer::{End, Tok, lex};
use crate::sfc::{Content, blocks, unquote};
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

const DIRECTIVES: &[(&str, bool)] = &[
    ("v-if", true),
    ("v-else-if", true),
    ("v-else", false),
    ("v-for", true),
    ("v-show", true),
    ("v-model", true),
    ("v-bind", true),
    ("v-on", true),
    ("v-slot", true),
    ("v-html", true),
    ("v-text", true),
    ("v-memo", true),
    ("v-once", false),
    ("v-pre", false),
    ("v-cloak", false),
];

const BLOCKS: &[(&str, &str)] = &[
    ("template", "template>\n  $0\n</template>"),
    ("script setup", "script setup>\n$0\n</script>"),
    ("script setup lang=\"ts\"", "script setup lang=\"ts\">\n$0\n</script>"),
    ("script", "script>\n$0\n</script>"),
    ("style scoped", "style scoped>\n$0\n</style>"),
    ("style scoped lang=\"scss\"", "style scoped lang=\"scss\">\n$0\n</style>"),
    ("style", "style>\n$0\n</style>"),
];

pub fn complete(text: &str, caret: usize) -> Option<Completion> {
    let mut caret = caret.min(text.len());
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    let (tokens, _) = lex(text);
    let blocks = blocks(text, &tokens);
    let block = blocks.iter().find(|block| {
        block.start < caret
            && block.inner.0 <= caret
            && caret <= block.inner.1
            && text[block.start..block.inner.0].ends_with('>')
    });
    let Some(block) = block else { return top_level(text, caret) };
    let base = block.inner.0;
    let inner = &text[base..block.inner.1];
    match block.content {
        Content::JavaScript | Content::TypeScript => {
            let dialect = if block.content == Content::TypeScript {
                script::Dialect::TypeScript
            } else {
                script::Dialect::JavaScript
            };
            let found = script::complete(inner, caret - base, dialect);
            let items = found
                .items
                .into_iter()
                .map(|item| Item { label: item.label, kind: item.kind, insert: item.insert, detail: item.detail })
                .collect::<Vec<_>>();
            (!items.is_empty()).then_some(Completion { from: base + found.from, items })
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
        Content::Template => directives(text, base, caret),
        Content::Other => None,
    }
}

fn directives(text: &str, base: usize, caret: usize) -> Option<Completion> {
    let (_, end) = lex(&text[base..caret]);
    let End::AttrName { attr, .. } = end else { return None };
    let typed = &text[base + attr..caret];
    if !typed.starts_with('v') {
        return None;
    }
    let items: Vec<Item> = DIRECTIVES
        .iter()
        .filter(|(name, _)| name.starts_with(typed))
        .map(|&(name, valued)| Item {
            label: name.to_owned(),
            kind: "attribute",
            insert: if valued { format!("{name}=\"$0\"") } else { name.to_owned() },
            detail: None,
        })
        .collect();
    (!items.is_empty()).then_some(Completion { from: base + attr, items })
}

fn top_level(text: &str, caret: usize) -> Option<Completion> {
    let before = &text[..caret];
    let (_, end) = lex(before);
    let End::TagName { lt, closing: false } = end else { return None };
    let (head, _) = lex(&before[..lt]);
    if blocks(&before[..lt], &head).iter().any(|block| !block.closed) {
        return None;
    }
    let typed = &before[lt + 1..];
    let items: Vec<Item> = BLOCKS
        .iter()
        .filter(|(label, _)| label.starts_with(typed))
        .map(|&(label, insert)| Item {
            label: label.to_owned(),
            kind: "snippet",
            insert: insert.to_owned(),
            detail: None,
        })
        .collect();
    (!items.is_empty()).then_some(Completion { from: lt + 1, items })
}

pub(crate) fn colors(text: &str) -> Vec<color::Color> {
    let (tokens, _) = lex(text);
    let mut found = Vec::new();
    for block in blocks(text, &tokens) {
        match block.content {
            Content::Css | Content::Scss => {
                let syntax = color::Syntax { scss: block.content == Content::Scss, in_block: false };
                color::colors(text, block.inner.0..block.inner.1, syntax, &mut found);
            }
            Content::Template => {
                let tokens = &tokens[block.tokens.0..block.tokens.1];
                for (at, token) in tokens.iter().enumerate() {
                    let is_style = at >= 2
                        && token.kind == Tok::AttrValue
                        && tokens[at - 1].kind == Tok::Eq
                        && tokens[at - 2].kind == Tok::AttrName
                        && text[tokens[at - 2].start..tokens[at - 2].end].eq_ignore_ascii_case("style");
                    if is_style {
                        let value = &text[token.start..token.end];
                        let start = token.start + usize::from(value.starts_with(['"', '\'']));
                        let end = start + unquote(value).len();
                        let syntax = color::Syntax { scss: false, in_block: true };
                        color::colors(text, start..end, syntax, &mut found);
                    }
                }
            }
            _ => {}
        }
    }
    found
}

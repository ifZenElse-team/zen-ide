use crate::data::{BOOLEAN_ATTRIBUTES, ELEMENT_ATTRIBUTES, ELEMENTS, ENTITIES, GLOBAL_ATTRIBUTES, values};
use crate::lexer::{End, Tok, lex};
use crate::tree::build;

const MAX_END_TAGS: usize = 2000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub from: usize,
    pub items: Vec<Item>,
}

pub(crate) fn clamp(text: &str, caret: usize) -> usize {
    let mut caret = caret.min(text.len());
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    caret
}

fn starts_with_ci(word: &str, typed: &str) -> bool {
    word.len() >= typed.len() && word.as_bytes()[..typed.len()].eq_ignore_ascii_case(typed.as_bytes())
}

fn item(label: &str, kind: &'static str, insert: String) -> Item {
    Item { label: label.to_owned(), kind, insert }
}

pub fn complete(text: &str, caret: usize) -> Option<Completion> {
    let caret = clamp(text, caret);
    let prefix = &text[..caret];
    let (tokens, end) = lex(prefix);
    let (from, items) = match end {
        End::TagName { lt, closing: false } => {
            let typed = &prefix[lt + 1..];
            let items = ELEMENTS
                .iter()
                .filter(|name| starts_with_ci(name, typed))
                .map(|name| item(name, "tag", (*name).to_owned()))
                .collect();
            (lt + 1, items)
        }
        End::TagName { lt, closing: true } => {
            let typed = &prefix[lt + 2..];
            let before: Vec<_> = tokens.iter().copied().filter(|token| token.end <= lt).collect();
            let tree = build(prefix, &before);
            let mut items: Vec<Item> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for &open in tree.open.iter().rev() {
                let name = &tree.elements[open].name;
                if starts_with_ci(name, typed) && seen.insert(name.as_str()) {
                    items.push(item(name, "tag", format!("{name}>")));
                    if items.len() == MAX_END_TAGS {
                        break;
                    }
                }
            }
            (lt + 2, items)
        }
        End::Tag { lt, .. } | End::AttrName { lt, .. } if prefix.as_bytes().get(lt + 1) == Some(&b'/') => {
            (caret, Vec::new())
        }
        End::Tag { lt, name } => (caret, attributes(prefix, &tokens, lt, name, caret, "")),
        End::AttrName { lt, name, attr } => (attr, attributes(prefix, &tokens, lt, name, attr, &prefix[attr..])),
        End::Value { name, attr, start, quote, .. } => {
            let element = prefix[name.0..name.1].to_ascii_lowercase();
            let attribute = prefix[attr.0..attr.1].to_ascii_lowercase();
            let typed = &prefix[start..];
            let items = values(&element, &attribute)
                .iter()
                .filter(|value| starts_with_ci(value, typed))
                .map(|value| {
                    item(value, "value", if quote.is_some() { (*value).to_owned() } else { format!("\"{value}\"") })
                })
                .collect();
            (start, items)
        }
        End::Data => {
            let word = prefix.bytes().rev().take_while(u8::is_ascii_alphanumeric).count();
            let amp = caret - word;
            if amp == 0 || prefix.as_bytes()[amp - 1] != b'&' {
                return None;
            }
            let typed = &prefix[amp..];
            let items = ENTITIES
                .iter()
                .filter(|name| starts_with_ci(name, typed))
                .map(|name| {
                    let entity = format!("&{name};");
                    item(&entity, "constant", entity.clone())
                })
                .collect();
            (amp - 1, items)
        }
        End::Opaque => return None,
    };
    (!items.is_empty()).then_some(Completion { from, items })
}

fn attributes(
    text: &str,
    tokens: &[crate::lexer::Token],
    lt: usize,
    name: (usize, usize),
    typed_at: usize,
    typed: &str,
) -> Vec<Item> {
    let element = text[name.0..name.1].to_ascii_lowercase();
    let present: Vec<String> = tokens
        .iter()
        .filter(|token| token.kind == Tok::AttrName && token.start > lt && token.start != typed_at)
        .map(|token| text[token.start..token.end].to_ascii_lowercase())
        .collect();
    let own = ELEMENT_ATTRIBUTES.iter().find(|(owner, _)| *owner == element).map_or(&[][..], |(_, list)| *list);
    let mut items: Vec<Item> = Vec::new();
    for attribute in own.iter().chain(GLOBAL_ATTRIBUTES) {
        if !starts_with_ci(attribute, typed)
            || present.iter().any(|present| present == attribute)
            || items.iter().any(|item| item.label == *attribute)
        {
            continue;
        }
        let insert = if BOOLEAN_ATTRIBUTES.contains(attribute) {
            (*attribute).to_owned()
        } else {
            format!("{attribute}=\"$0\"")
        };
        items.push(item(attribute, "attribute", insert));
    }
    items
}

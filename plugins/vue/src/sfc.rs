use crate::lexer::{Tok, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Content {
    Template,
    JavaScript,
    TypeScript,
    Css,
    Scss,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) name: String,
    pub(crate) content: Content,
    pub(crate) lang: Option<String>,
    pub(crate) setup: bool,
    pub(crate) scoped: bool,
    pub(crate) module: bool,
    pub(crate) start: usize,
    pub(crate) inner: (usize, usize),
    pub(crate) end: usize,
    pub(crate) closed: bool,
    pub(crate) tokens: (usize, usize),
}

impl Block {
    pub(crate) fn label(&self) -> String {
        let mut label = self.name.clone();
        for (on, word) in [(self.setup, "setup"), (self.scoped, "scoped"), (self.module, "module")] {
            if on {
                label.push(' ');
                label.push_str(word);
            }
        }
        if let Some(lang) = &self.lang {
            label.push_str(&format!(" lang=\"{lang}\""));
        }
        label
    }
}

pub(crate) fn unquote(value: &str) -> &str {
    let quoted = value.starts_with(['"', '\'']);
    let inner = if quoted { &value[1..] } else { value };
    if quoted && inner.ends_with(&value[..1]) { &inner[..inner.len() - 1] } else { inner }
}

pub(crate) fn blocks(text: &str, tokens: &[Token]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].kind != Tok::Lt || tokens.get(at + 1).is_none_or(|token| token.kind != Tok::TagName) {
            at += 1;
            continue;
        }
        let start = tokens[at].start;
        let name = text[tokens[at + 1].start..tokens[at + 1].end].to_owned();
        let mut attributes: Vec<(String, Option<String>)> = Vec::new();
        let mut index = at + 2;
        let mut closed = None;
        while index < tokens.len() {
            let token = tokens[index];
            match token.kind {
                Tok::AttrName => attributes.push((text[token.start..token.end].to_ascii_lowercase(), None)),
                Tok::AttrValue => {
                    if let Some((_, value)) = attributes.last_mut() {
                        *value = Some(unquote(&text[token.start..token.end]).to_owned());
                    }
                }
                Tok::Gt => {
                    closed = Some((token.end, false));
                    break;
                }
                Tok::SlashGt => {
                    closed = Some((token.end, true));
                    break;
                }
                Tok::Eq => {}
                _ => break,
            }
            index += 1;
        }
        let lang = attributes
            .iter()
            .find(|(attribute, _)| attribute == "lang")
            .and_then(|(_, value)| value.clone())
            .map(|lang| lang.trim().to_ascii_lowercase())
            .filter(|lang| !lang.is_empty());
        let has = |word: &str| attributes.iter().any(|(attribute, _)| attribute == word);
        let content = content_of(&name, lang.as_deref());
        let (setup, scoped, module) = (has("setup"), has("scoped"), has("module"));
        let Some((open_end, self_closing)) = closed else {
            let end = text.len();
            blocks.push(Block {
                name,
                content,
                lang,
                setup,
                scoped,
                module,
                start,
                inner: (end, end),
                end,
                closed: false,
                tokens: (tokens.len(), tokens.len()),
            });
            break;
        };
        let first = index + 1;
        if self_closing {
            blocks.push(Block {
                name,
                content,
                lang,
                setup,
                scoped,
                module,
                start,
                inner: (open_end, open_end),
                end: open_end,
                closed: true,
                tokens: (first, first),
            });
            at = first;
            continue;
        }
        let (inner_end, last, end, next) = match closing(text, tokens, first, &name) {
            Some(close) => {
                let gt = tokens.get(close + 2).filter(|token| token.kind == Tok::Gt).map(|_| close + 2);
                let end = gt.map_or(tokens[close + 1].end, |gt| tokens[gt].end);
                (tokens[close].start, close, end, gt.map_or(close + 2, |gt| gt + 1))
            }
            None => (text.len(), tokens.len(), text.len(), tokens.len()),
        };
        let closed = last < tokens.len();
        blocks.push(Block {
            name,
            content,
            lang,
            setup,
            scoped,
            module,
            start,
            inner: (open_end, inner_end),
            end,
            closed,
            tokens: (first, last),
        });
        at = next;
    }
    blocks
}

fn closing(text: &str, tokens: &[Token], first: usize, name: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = first;
    while index + 1 < tokens.len() {
        let (token, next) = (tokens[index], tokens[index + 1]);
        let named = next.kind == Tok::TagName && text[next.start..next.end].eq_ignore_ascii_case(name);
        match token.kind {
            Tok::Lt if named => {
                let self_closing = tokens[index + 2..]
                    .iter()
                    .find(|token| matches!(token.kind, Tok::Gt | Tok::SlashGt | Tok::Lt | Tok::LtSlash))
                    .is_some_and(|token| token.kind == Tok::SlashGt);
                if !self_closing {
                    depth += 1;
                }
            }
            Tok::LtSlash if named => {
                if depth == 0 {
                    return Some(index);
                }
                depth -= 1;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn content_of(name: &str, lang: Option<&str>) -> Content {
    match (name.to_ascii_lowercase().as_str(), lang) {
        ("template", None | Some("html")) => Content::Template,
        ("script", None | Some("js" | "jsx" | "mjs" | "cjs" | "javascript")) => Content::JavaScript,
        ("script", Some("ts" | "tsx" | "mts" | "cts" | "typescript")) => Content::TypeScript,
        ("style", None | Some("css" | "postcss")) => Content::Css,
        ("style", Some("scss")) => Content::Scss,
        _ => Content::Other,
    }
}

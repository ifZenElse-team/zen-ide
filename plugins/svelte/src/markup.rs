use crate::lexer::{Tok, Token, lex};
use crate::sfc::{Block, Content, blocks};

const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

pub(crate) const BLOCKS: &[&str] = &["if", "each", "await", "key", "snippet"];

pub(crate) fn mask(text: &str) -> (String, Vec<(usize, usize)>) {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut regions = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if bytes[i..].starts_with(b"<!--") {
                i = find(bytes, i + 4, b"-->").map_or(bytes.len(), |end| end + 3);
                continue;
            }
            if let Some(element) = ["script", "style"].into_iter().find(|name| starts_tag(bytes, i, name)) {
                let close = format!("</{element}");
                let open_end = find(bytes, i, b">").map_or(bytes.len(), |end| end + 1);
                i = find_ci(bytes, open_end, close.as_bytes()).unwrap_or(bytes.len());
                continue;
            }
        }
        if bytes[i] == b'{'
            && let Some(close) = matching_brace(bytes, i)
        {
            for byte in &mut out[i + 1..close] {
                *byte = b'x';
            }
            regions.push((i, close + 1));
            i = close + 1;
            continue;
        }
        i += 1;
    }
    (String::from_utf8(out).unwrap_or_else(|_| text.to_owned()), regions)
}

fn starts_tag(bytes: &[u8], at: usize, name: &str) -> bool {
    let end = at + 1 + name.len();
    bytes.get(at + 1..end).is_some_and(|word| word.eq_ignore_ascii_case(name.as_bytes()))
        && bytes.get(end).is_none_or(|&byte| byte.is_ascii_whitespace() || byte == b'>' || byte == b'/')
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|window| window == needle).map(|at| from + at)
}

fn find_ci(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|window| window.eq_ignore_ascii_case(needle)).map(|at| from + at)
}

fn matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            quote @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BlockTag {
    pub(crate) sigil: u8,
    pub(crate) word: String,
    pub(crate) word_range: (usize, usize),
}

pub(crate) fn block_tag(text: &str, region: (usize, usize)) -> Option<BlockTag> {
    let inner = &text[region.0 + 1..region.1.saturating_sub(1).max(region.0 + 1)];
    let lead = inner.len() - inner.trim_start().len();
    let start = region.0 + 1 + lead;
    let sigil = *text.as_bytes().get(start)?;
    if !matches!(sigil, b'#' | b':' | b'/' | b'@') {
        return None;
    }
    let word: String = text[start + 1..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    let mut end = start + 1 + word.len();
    if sigil == b':' && word == "else" {
        let rest = &text[end..region.1];
        let spaces = rest.len() - rest.trim_start().len();
        if rest.trim_start().starts_with("if") && rest.trim_start()[2..].starts_with(|c: char| !c.is_alphanumeric()) {
            end += spaces + 2;
        }
    }
    Some(BlockTag { sigil, word, word_range: (start, end) })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Element,
    Block,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) start: usize,
    pub(crate) open_end: usize,
    pub(crate) close_start: usize,
    pub(crate) end: usize,
    pub(crate) closed: bool,
    pub(crate) parent: Option<usize>,
}

pub(crate) struct Markup {
    pub(crate) masked: String,
    pub(crate) regions: Vec<(usize, usize)>,
    pub(crate) tokens: Vec<Token>,
    pub(crate) blocks: Vec<Block>,
    pub(crate) nodes: Vec<Node>,
}

impl Markup {
    pub(crate) fn read(text: &str) -> Self {
        let (masked, regions) = mask(text);
        let (tokens, _) = lex(&masked);
        let blocks: Vec<Block> = blocks(&masked, &tokens)
            .into_iter()
            .filter(|block| !matches!(block.content, Content::Other | Content::Template))
            .collect();
        let nodes = tree(text, &masked, &tokens, &regions, &blocks);
        Self { masked, regions, tokens, blocks, nodes }
    }

    pub(crate) fn block_at(&self, offset: usize) -> Option<&Block> {
        self.blocks.iter().find(|block| block.inner.0 <= offset && offset <= block.inner.1)
    }

    pub(crate) fn in_code(&self, offset: usize) -> bool {
        self.blocks.iter().any(|block| block.inner.0 < offset && offset < block.inner.1)
    }
}

fn inside(blocks: &[Block], at: usize) -> bool {
    blocks.iter().any(|block| block.inner.0 <= at && at < block.inner.1)
}

fn tree(text: &str, masked: &str, tokens: &[Token], regions: &[(usize, usize)], code: &[Block]) -> Vec<Node> {
    enum Event {
        Open { name: String, label: String, start: usize, open_end: usize, leaf: bool },
        Close { name: String, start: usize, end: usize },
        OpenBlock { name: String, label: String, start: usize, end: usize },
        CloseBlock { name: String, start: usize, end: usize },
    }
    let mut events: Vec<(usize, Event)> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        let named = tokens.get(index + 1).filter(|next| next.kind == Tok::TagName);
        match (token.kind, named) {
            (Tok::Lt, Some(name_token)) if !inside(code, token.start) && !is_code_tag(code, token.start) => {
                let tag = &masked[name_token.start..name_token.end];
                let mut label = tag.to_owned();
                let mut close = index + 2;
                let mut attribute = "";
                while close < tokens.len() && matches!(tokens[close].kind, Tok::AttrName | Tok::Eq | Tok::AttrValue) {
                    let current = tokens[close];
                    match current.kind {
                        Tok::AttrName => attribute = &masked[current.start..current.end],
                        Tok::AttrValue if attribute == "id" => {
                            label.push('#');
                            label.push_str(crate::sfc::unquote(&text[current.start..current.end]));
                        }
                        _ => {}
                    }
                    close += 1;
                }
                let end_of_tag = tokens.get(close).filter(|token| matches!(token.kind, Tok::Gt | Tok::SlashGt));
                let open_end = end_of_tag.map_or(name_token.end, |token| token.end);
                let leaf = end_of_tag.is_none_or(|token| token.kind == Tok::SlashGt)
                    || VOID.iter().any(|void| tag.eq_ignore_ascii_case(void));
                events.push((
                    token.start,
                    Event::Open { name: tag.to_owned(), label, start: token.start, open_end, leaf },
                ));
                index = close;
            }
            (Tok::LtSlash, Some(name_token))
                if !inside(code, token.start) && !code.iter().any(|block| block.inner.1 == token.start) =>
            {
                let gt = tokens.get(index + 2).filter(|token| token.kind == Tok::Gt);
                let end = gt.map_or(name_token.end, |token| token.end);
                let name = masked[name_token.start..name_token.end].to_owned();
                events.push((token.start, Event::Close { name, start: token.start, end }));
                index += 2;
            }
            _ => index += 1,
        }
    }
    for &region in regions {
        let in_attribute = tokens.iter().any(|token| {
            matches!(token.kind, Tok::AttrValue | Tok::AttrName) && token.start <= region.0 && region.1 <= token.end
        });
        if in_attribute {
            continue;
        }
        let Some(tag) = block_tag(text, region) else { continue };
        if !BLOCKS.contains(&tag.word.as_str()) {
            continue;
        }
        match tag.sigil {
            b'#' => {
                let label = shorten(text[region.0..region.1].split_whitespace().collect::<Vec<_>>().join(" "));
                events.push((region.0, Event::OpenBlock { name: tag.word, label, start: region.0, end: region.1 }));
            }
            b'/' => events.push((region.0, Event::CloseBlock { name: tag.word, start: region.0, end: region.1 })),
            _ => {}
        }
    }
    events.sort_by_key(|(at, _)| *at);
    let mut nodes: Vec<Node> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let close_to =
        |nodes: &mut Vec<Node>, open: &mut Vec<usize>, kind: NodeKind, name: &str, start: usize, end: usize| {
            let found = open.iter().rposition(|&node| {
                nodes[node].kind == kind
                    && match kind {
                        NodeKind::Element => nodes[node].name.eq_ignore_ascii_case(name),
                        NodeKind::Block => nodes[node].name == name,
                    }
            });
            if let Some(depth) = found {
                for &unclosed in &open[depth + 1..] {
                    nodes[unclosed].close_start = start;
                    nodes[unclosed].end = start;
                }
                let node = open[depth];
                nodes[node].close_start = start;
                nodes[node].end = end;
                nodes[node].closed = true;
                open.truncate(depth);
            }
        };
    for (_, event) in events {
        match event {
            Event::Open { name, label, start, open_end, leaf } => {
                nodes.push(Node {
                    kind: NodeKind::Element,
                    name,
                    label,
                    start,
                    open_end,
                    close_start: open_end,
                    end: open_end,
                    closed: leaf,
                    parent: open.last().copied(),
                });
                if !leaf {
                    open.push(nodes.len() - 1);
                }
            }
            Event::OpenBlock { name, label, start, end } => {
                nodes.push(Node {
                    kind: NodeKind::Block,
                    name,
                    label,
                    start,
                    open_end: end,
                    close_start: end,
                    end,
                    closed: false,
                    parent: open.last().copied(),
                });
                open.push(nodes.len() - 1);
            }
            Event::Close { name, start, end } => close_to(&mut nodes, &mut open, NodeKind::Element, &name, start, end),
            Event::CloseBlock { name, start, end } => {
                close_to(&mut nodes, &mut open, NodeKind::Block, &name, start, end)
            }
        }
    }
    let last = text.len();
    for unclosed in open {
        nodes[unclosed].close_start = last;
        nodes[unclosed].end = last;
    }
    nodes
}

fn is_code_tag(code: &[Block], at: usize) -> bool {
    code.iter().any(|block| block.start == at)
}

fn shorten(label: String) -> String {
    const MAX: usize = 60;
    if label.chars().count() <= MAX {
        return label;
    }
    let mut short: String = label.chars().take(MAX - 1).collect();
    short.push('…');
    short
}

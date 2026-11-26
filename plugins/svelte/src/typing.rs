use crate::markup::{BLOCKS, Markup, NodeKind, block_tag};

pub const TRIGGERS: &str = ">/}";

pub fn on_type(text: &str, caret: usize, typed: char) -> Option<String> {
    if caret == 0 || caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let markup = Markup::read(text);
    if markup.in_code(caret - 1) {
        return None;
    }
    match typed {
        '>' => {
            let node = markup.nodes.iter().find(|node| {
                node.kind == NodeKind::Element && node.open_end == caret && text[..caret].ends_with('>')
            })?;
            (!node.closed).then(|| format!("$0</{}>", node.name))
        }
        '/' => {
            if !text[..caret].ends_with("</") {
                return None;
            }
            let lt = caret - 2;
            let open = markup
                .nodes
                .iter()
                .filter(|node| node.kind == NodeKind::Element && node.open_end <= lt && lt <= node.close_start)
                .filter(|node| !node.closed || node.close_start == lt)
                .max_by_key(|node| node.start)?;
            let rest = &text[caret..];
            (!rest.starts_with(open.name.as_str())).then(|| format!("{}>", open.name))
        }
        '}' => {
            let region = *markup.regions.iter().find(|region| region.1 == caret)?;
            let tag = block_tag(text, region).filter(|tag| tag.sigil == b'#' && BLOCKS.contains(&tag.word.as_str()))?;
            let node = markup.nodes.iter().find(|node| node.kind == NodeKind::Block && node.start == region.0)?;
            (!node.closed).then(|| format!("$0{{/{}}}", tag.word))
        }
        _ => None,
    }
}

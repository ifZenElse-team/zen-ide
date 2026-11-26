#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading { level: u8, spans: Vec<Span> },
    Paragraph(Vec<Span>),
    Item { depth: u8, marker: Marker, spans: Vec<Span> },
    Code(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Bullet,
    Number(u32),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }
}

const MAX_DEPTH: u8 = 3;

pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut pending: Option<(Pending, String)> = None;
    let mut code: Option<String> = None;

    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(body) = &mut code {
            if line.trim_start().starts_with("```") {
                blocks.push(Block::Code(code.take().unwrap_or_default()));
            } else {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(line);
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut pending, &mut blocks);
            continue;
        }
        if trimmed.starts_with("```") {
            flush(&mut pending, &mut blocks);
            code = Some(String::new());
            continue;
        }
        if let Some((level, rest)) = heading(trimmed) {
            flush(&mut pending, &mut blocks);
            blocks.push(Block::Heading { level, spans: inline(rest) });
            continue;
        }
        let indent = indentation(line);
        if let Some((marker, rest)) = item(trimmed) {
            flush(&mut pending, &mut blocks);
            let depth = u8::try_from(indent / 2).unwrap_or(MAX_DEPTH).min(MAX_DEPTH);
            pending = Some((Pending::Item { depth, marker }, rest.to_owned()));
            continue;
        }
        match &mut pending {
            Some((_, text)) => {
                text.push(' ');
                text.push_str(trimmed);
            }
            None => pending = Some((Pending::Paragraph, trimmed.to_owned())),
        }
    }
    flush(&mut pending, &mut blocks);
    if let Some(body) = code {
        blocks.push(Block::Code(body));
    }
    blocks
}

#[derive(Clone, Copy)]
enum Pending {
    Paragraph,
    Item { depth: u8, marker: Marker },
}

fn flush(pending: &mut Option<(Pending, String)>, blocks: &mut Vec<Block>) {
    let Some((kind, text)) = pending.take() else { return };
    let spans = inline(&text);
    blocks.push(match kind {
        Pending::Paragraph => Block::Paragraph(spans),
        Pending::Item { depth, marker } => Block::Item { depth, marker, spans },
    });
}

fn indentation(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some((u8::try_from(hashes).unwrap_or(6), text))
}

fn item(line: &str) -> Option<(Marker, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return Some((Marker::Bullet, rest.trim_start()));
        }
    }
    if matches!(line, "-" | "*" | "+") {
        return Some((Marker::Bullet, ""));
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &line[digits..];
    let rest = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))?;
    let number = line[..digits].parse().ok()?;
    Some((Marker::Number(number), rest.trim_start()))
}

pub fn inline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut current = String::new();
    let (mut bold, mut italic) = (false, false);
    let mut i = 0;

    let push = |spans: &mut Vec<Span>, text: &mut String, bold: bool, italic: bool, code: bool| {
        if text.is_empty() {
            return;
        }
        let text = std::mem::take(text);
        match spans.last_mut() {
            Some(last) if last.bold == bold && last.italic == italic && last.code == code => last.text.push_str(&text),
            _ => spans.push(Span { text, bold, italic, code }),
        }
    };

    while let Some(&c) = chars.get(i) {
        let next = chars.get(i + 1).copied();
        match c {
            '\\' if next.is_some_and(|n| n.is_ascii_punctuation()) => {
                current.extend(next);
                i += 2;
            }
            '`' => match find(&chars, i + 1, |at| chars.get(at) == Some(&'`')) {
                Some(end) if end > i + 1 => {
                    push(&mut spans, &mut current, bold, italic, false);
                    let mut code: String = chars[i + 1..end].iter().collect();
                    push(&mut spans, &mut code, bold, italic, true);
                    i = end + 1;
                }
                _ => {
                    current.push(c);
                    i += 1;
                }
            },
            '*' | '_' if next == Some(c) => {
                let closes = bold
                    || find(&chars, i + 2, |at| chars.get(at) == Some(&c) && chars.get(at + 1) == Some(&c)).is_some();
                if closes {
                    push(&mut spans, &mut current, bold, italic, false);
                    bold = !bold;
                } else {
                    current.push(c);
                    current.push(c);
                }
                i += 2;
            }
            '*' => {
                let opens = !italic
                    && next.is_some_and(|n| !n.is_whitespace())
                    && find(&chars, i + 1, |at| chars.get(at) == Some(&'*') && chars.get(at + 1) != Some(&'*'))
                        .is_some();
                if italic || opens {
                    push(&mut spans, &mut current, bold, italic, false);
                    italic = !italic;
                } else {
                    current.push(c);
                }
                i += 1;
            }
            '[' => match link(&chars, i) {
                Some((label, end)) => {
                    current.push_str(&label);
                    i = end;
                }
                None => {
                    current.push(c);
                    i += 1;
                }
            },
            _ => {
                current.push(c);
                i += 1;
            }
        }
    }
    push(&mut spans, &mut current, bold, italic, false);
    spans
}

fn find(chars: &[char], from: usize, found: impl Fn(usize) -> bool) -> Option<usize> {
    (from..chars.len()).find(|&at| found(at))
}

fn link(chars: &[char], start: usize) -> Option<(String, usize)> {
    let close = find(chars, start + 1, |at| chars.get(at) == Some(&']'))?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = find(chars, close + 2, |at| chars.get(at) == Some(&')'))?;
    Some((chars[start + 1..close].iter().collect(), end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold(text: &str) -> Span {
        Span { text: text.into(), bold: true, ..Span::default() }
    }

    fn code(text: &str) -> Span {
        Span { text: text.into(), code: true, ..Span::default() }
    }

    #[test]
    fn headings_paragraphs_and_lists() {
        let blocks = parse(
            "# Zen\n\n### Added\n\n- One\n- Two\n  continued\n  - Nested\n1. First\n\nA paragraph\non two lines.",
        );
        assert_eq!(
            blocks,
            vec![
                Block::Heading { level: 1, spans: vec![Span::plain("Zen")] },
                Block::Heading { level: 3, spans: vec![Span::plain("Added")] },
                Block::Item { depth: 0, marker: Marker::Bullet, spans: vec![Span::plain("One")] },
                Block::Item { depth: 0, marker: Marker::Bullet, spans: vec![Span::plain("Two continued")] },
                Block::Item { depth: 1, marker: Marker::Bullet, spans: vec![Span::plain("Nested")] },
                Block::Item { depth: 0, marker: Marker::Number(1), spans: vec![Span::plain("First")] },
                Block::Paragraph(vec![Span::plain("A paragraph on two lines.")]),
            ]
        );
    }

    #[test]
    fn inline_styles() {
        assert_eq!(
            inline("Run `cargo xtask ci` **before** every *commit*."),
            vec![
                Span::plain("Run "),
                code("cargo xtask ci"),
                Span::plain(" "),
                bold("before"),
                Span::plain(" every "),
                Span { text: "commit".into(), italic: true, ..Span::default() },
                Span::plain("."),
            ]
        );
        assert_eq!(inline("__strong__"), vec![bold("strong")]);
        assert_eq!(inline("See [the site](https://ide.ifzenelse.space)."), vec![Span::plain("See the site.")]);
        assert_eq!(inline(r"\*not italic\*"), vec![Span::plain("*not italic*")]);
    }

    #[test]
    fn unmatched_markers_stay_literal() {
        assert_eq!(inline("2 * 3 = 6"), vec![Span::plain("2 * 3 = 6")]);
        assert_eq!(inline("**open"), vec![Span::plain("**open")]);
        assert_eq!(inline("a ` b"), vec![Span::plain("a ` b")]);
        assert_eq!(inline("snake_case_name"), vec![Span::plain("snake_case_name")]);
        assert_eq!(inline("[label] (x)"), vec![Span::plain("[label] (x)")]);
        assert_eq!(parse("#hashtag"), vec![Block::Paragraph(vec![Span::plain("#hashtag")])]);
    }

    #[test]
    fn code_blocks_keep_their_lines() {
        let blocks = parse("Before\n```sh\ncargo xtask ci\n  indented\n```\nAfter");
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(vec![Span::plain("Before")]),
                Block::Code("cargo xtask ci\n  indented".into()),
                Block::Paragraph(vec![Span::plain("After")]),
            ]
        );
        assert_eq!(parse("```\nunclosed"), vec![Block::Code("unclosed".into())]);
    }

    #[test]
    fn non_ascii_text_and_crlf() {
        assert_eq!(
            parse("## Что нового\r\n- **Быстрее** 🚀\r\n"),
            vec![
                Block::Heading { level: 2, spans: vec![Span::plain("Что нового")] },
                Block::Item {
                    depth: 0, marker: Marker::Bullet, spans: vec![bold("Быстрее"), Span::plain(" 🚀")]
                },
            ]
        );
    }

    #[test]
    fn never_panics_on_any_short_input() {
        let alphabet: Vec<char> = "*_`#-+1.)[]( \\\n\té".chars().collect();
        let mut stack = vec![String::new()];
        while let Some(prefix) = stack.pop() {
            let _ = parse(&prefix);
            if prefix.chars().count() < 4 {
                for &c in &alphabet {
                    stack.push(format!("{prefix}{c}"));
                }
            }
        }
        for input in ["", "\n\n\n", "#######", "- ", "99999999999. x", "**`**`**", "[](", "``````", "*\u{0}*"] {
            let _ = parse(input);
        }
    }
}

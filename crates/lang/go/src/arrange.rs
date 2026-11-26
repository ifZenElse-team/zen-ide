use zen_syntax::Arranger;

pub const ORDER: &[&str] = &["const", "var", "type", "func"];

pub struct GoArranger;

impl Arranger for GoArranger {
    fn language(&self) -> &str {
        "go"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn arrange(&self, text: &str, order: &[String]) -> Option<String> {
        arrange(text, order)
    }

    fn before_reformat(&self, text: &str, option: &dyn Fn(&str) -> bool) -> Option<String> {
        option(crate::tags::ALIGN_OPTION).then(|| crate::tags::align(text, false)).flatten()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line<'a> {
    Blank,
    Comment,
    Code(&'a str),
    Inner,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Code,
    LineComment,
    BlockComment,
    Text,
    Raw,
    Rune,
}

fn classify(text: &str) -> Option<Vec<(usize, Line<'_>)>> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut state = State::Code;
    let mut depth: i64 = 0;
    let mut at = 0;
    let mut line_start = true;
    while at <= bytes.len() {
        if line_start {
            let end = text[at..].find('\n').map_or(text.len(), |end| at + end);
            let content = text[at..end].trim();
            let line = match state {
                State::BlockComment if depth == 0 => Line::Comment,
                State::Code if depth == 0 && content.is_empty() => Line::Blank,
                State::Code if depth == 0 && (content.starts_with("//") || content.starts_with("/*")) => Line::Comment,
                State::Code if depth == 0 => {
                    let word = content.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').next().unwrap_or("");
                    Line::Code(word)
                }
                _ => Line::Inner,
            };
            if at < bytes.len() || !content.is_empty() {
                lines.push((at, line));
            }
            line_start = false;
        }
        let Some(&byte) = bytes.get(at) else { break };
        let next = bytes.get(at + 1).copied();
        match state {
            State::Code => match byte {
                b'/' if next == Some(b'/') => {
                    state = State::LineComment;
                    at += 1;
                }
                b'/' if next == Some(b'*') => {
                    state = State::BlockComment;
                    at += 1;
                }
                b'"' => state = State::Text,
                b'`' => state = State::Raw,
                b'\'' => state = State::Rune,
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth -= 1,
                _ => {}
            },
            State::LineComment if byte == b'\n' => state = State::Code,
            State::BlockComment if byte == b'*' && next == Some(b'/') => {
                state = State::Code;
                at += 1;
            }
            State::Text | State::Rune if byte == b'\\' => at += 1,
            State::Text if byte == b'"' || byte == b'\n' => state = State::Code,
            State::Rune if byte == b'\'' || byte == b'\n' => state = State::Code,
            State::Raw if byte == b'`' => state = State::Code,
            _ => {}
        }
        if depth < 0 {
            return None;
        }
        if bytes.get(at) == Some(&b'\n') {
            line_start = true;
        }
        at += 1;
    }
    (state == State::Code || state == State::LineComment).then_some(())?;
    (depth == 0).then_some(lines)
}

struct Chunk<'a> {
    kind: &'a str,
    body: &'a str,
    blank_after: bool,
}

fn kind_of(word: &str) -> Option<&str> {
    ORDER.contains(&word).then_some(word)
}

pub fn arrange(text: &str, order: &[String]) -> Option<String> {
    let lines = classify(text)?;
    let mut starts = Vec::new();
    for (index, (_, line)) in lines.iter().enumerate() {
        match line {
            Line::Code("package") | Line::Code("import") if starts.is_empty() => {}
            Line::Code(word) => {
                let kind = kind_of(word)?;
                let mut first = index;
                while first > 0 && lines[first - 1].1 == Line::Comment {
                    first -= 1;
                }
                starts.push((first, kind));
            }
            _ => {}
        }
    }
    if starts.len() < 2 {
        return None;
    }
    let offset = |index: usize| lines.get(index).map_or(text.len(), |(at, _)| *at);
    let header = &text[..offset(starts[0].0)];
    let chunks: Vec<Chunk<'_>> = starts
        .iter()
        .enumerate()
        .map(|(position, (first, kind))| {
            let end = starts.get(position + 1).map_or(text.len(), |(next, _)| offset(*next));
            let raw = &text[offset(*first)..end];
            let body = raw.trim_end();
            Chunk { kind, body, blank_after: raw[body.len()..].matches('\n').count() > 1 }
        })
        .collect();
    let rank = |kind: &str| order.iter().position(|known| known == kind).unwrap_or(order.len());
    let mut sorted: Vec<usize> = (0..chunks.len()).collect();
    sorted.sort_by_key(|index| rank(chunks[*index].kind));
    if sorted.iter().enumerate().all(|(position, index)| position == *index) {
        return None;
    }
    let chunks: Vec<&Chunk<'_>> = sorted.iter().map(|index| &chunks[*index]).collect();
    let mut arranged = header.to_owned();
    let last = chunks.len() - 1;
    for (position, chunk) in chunks.iter().enumerate() {
        arranged.push_str(chunk.body);
        arranged.push('\n');
        if position < last && (chunk.blank_after || chunks[position + 1].kind != chunk.kind) {
            arranged.push('\n');
        }
    }
    Some(arranged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order() -> Vec<String> {
        ORDER.iter().map(|kind| (*kind).to_owned()).collect()
    }

    #[test]
    fn top_level_declarations_go_by_kind_and_keep_their_order_within_a_kind() {
        let text = "//go:build linux\n\npackage main\n\nimport (\n\t\"fmt\"\n)\n\nfunc init() { a() }\n\n// T is a type.\ntype T struct {\n\tx int\n}\n\nvar b = 2\nvar a = 1\n\nconst (\n\tC = iota\n)\n\nfunc init() { b() }\n";
        let arranged = arrange(text, &order()).unwrap();
        assert_eq!(
            arranged,
            "//go:build linux\n\npackage main\n\nimport (\n\t\"fmt\"\n)\n\nconst (\n\tC = iota\n)\n\nvar b = 2\nvar a = 1\n\n// T is a type.\ntype T struct {\n\tx int\n}\n\nfunc init() { a() }\n\nfunc init() { b() }\n"
        );
        assert_eq!(arrange(&arranged, &order()), None, "arranged code stays as it is");
    }

    #[test]
    fn braces_strings_and_comments_inside_declarations_do_not_split_them() {
        let text = "package p\n\nfunc f() {\n\ts := `\nvar x = 1\n`\n\t_ = \"}\" + s\n\t/*\nconst y = 2\n*/\n}\n\nconst z = 3\n";
        let arranged = arrange(text, &order()).unwrap();
        assert!(arranged.starts_with("package p\n\nconst z = 3\n\nfunc f() {\n\ts := `\nvar x = 1\n`"), "{arranged}");
    }

    #[test]
    fn code_it_does_not_understand_is_left_as_it_is() {
        assert_eq!(arrange("package p\n\nfunc f() {\n", &order()), None, "an unclosed brace");
        assert_eq!(arrange("package p\n\nfunc f() {}\nfoo bar\nconst c = 1\n", &order()), None);
        assert_eq!(arrange("package p\n\nconst c = 1\n", &order()), None, "one declaration");
    }

    #[test]
    fn the_order_comes_from_the_settings() {
        let text = "package p\n\nconst c = 1\n\nfunc f() {}\n";
        let functions_first = vec!["func".to_owned(), "const".to_owned()];
        assert_eq!(arrange(text, &functions_first).unwrap(), "package p\n\nfunc f() {}\n\nconst c = 1\n");
    }
}

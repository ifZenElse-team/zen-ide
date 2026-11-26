use std::sync::Arc;

use super::*;
use crate::{Highlighter, Highlighting, Incremental, LineLexer, Lines};

struct Strings;

impl LineLexer for Strings {
    type State = bool;
    type Line = Vec<Range<usize>>;

    fn lex(&self, line: &str, raw: &mut bool) -> Vec<Range<usize>> {
        let bytes = line.as_bytes();
        let mut pieces = Vec::new();
        let mut at = 0;
        if *raw {
            match line.find('`') {
                Some(end) => {
                    pieces.push(0..end + 1);
                    *raw = false;
                    at = end + 1;
                }
                None => return std::iter::once(0..line.len()).collect(),
            }
        }
        while at < bytes.len() {
            match bytes[at] {
                b'"' => {
                    let mut end = at + 1;
                    while end < bytes.len() && bytes[end] != b'"' && bytes[end] != b'\n' {
                        end += if bytes[end] == b'\\' { 2 } else { 1 };
                    }
                    let end = (end + 1).min(bytes.len());
                    pieces.push(at..end);
                    at = end;
                }
                b'`' => match line[at + 1..].find('`') {
                    Some(close) => {
                        pieces.push(at..at + close + 2);
                        at += close + 2;
                    }
                    None => {
                        pieces.push(at..line.len());
                        *raw = true;
                        return pieces;
                    }
                },
                _ => at += 1,
            }
        }
        pieces
    }
}

struct StringLines(Lines<Strings>);

impl Incremental for StringLines {
    fn edit(&mut self, text: &str, change: TextChange) -> usize {
        self.0.edit(&Strings, text, change, |_| {}).len()
    }

    fn spans(&self, _: &str, range: Range<usize>) -> Vec<Span> {
        let first = self.0.line_of(range.start);
        let last = self.0.line_of(range.end.saturating_sub(1).max(range.start));
        let mut spans = Vec::new();
        for index in first..=last.min(self.0.len() - 1) {
            let start = self.0.start(index);
            for piece in self.0.line(index) {
                let (from, to) = (start + piece.start, start + piece.end);
                if to <= range.start || (from >= range.end && range.end > range.start) {
                    continue;
                }
                spans.push(Span { range: from..to, kind: HighlightKind::String });
            }
        }
        spans
    }

    fn lines(&self) -> usize {
        self.0.len()
    }
}

struct Host {
    incremental: bool,
}

impl Highlighter for Host {
    fn handles(&self, _: &str) -> bool {
        true
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let lines = Lines::new(&Strings, text);
        StringLines(lines).spans(text, 0..text.len())
    }

    fn incremental(&self, text: &str) -> Option<Box<dyn Incremental>> {
        self.incremental.then(|| Box::new(StringLines(Lines::new(&Strings, text))) as Box<dyn Incremental>)
    }
}

struct Upper;

impl Injector for Upper {
    fn language(&self) -> &str {
        "sql"
    }

    fn may_hold(&self, text: &str) -> bool {
        text.contains("SELECT")
    }

    fn accepts(&self, _: &str, _: &Literal, code: &str) -> bool {
        code.split_whitespace().next() == Some("SELECT")
    }

    fn highlight(&self, code: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut at = 0;
        for word in code.split(|c: char| !c.is_ascii_alphanumeric()) {
            let start = code[at..].find(word).map_or(at, |found| at + found);
            if !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_uppercase()) {
                spans.push(Span { range: start..start + word.len(), kind: HighlightKind::Keyword });
            }
            at = start + word.len();
        }
        spans
    }
}

fn injected(text: &str, incremental: bool) -> Highlighting {
    let mut highlighting = Highlighting::new(Arc::new(Host { incremental }), text);
    highlighting.inject(text, vec![Arc::new(Upper)]);
    highlighting
}

fn found(highlighting: &Highlighting) -> Vec<(Range<usize>, Vec<Span>)> {
    highlighting
        .fragments()
        .iter()
        .map(|fragment| (fragment.literal.range.clone(), fragment.spans().collect()))
        .collect()
}

#[test]
fn literals_take_prefixes_quotes_and_pieces_on_several_lines() {
    let text = "a(r#\"x\"#)";
    let rust = literal(text, 2..8).unwrap();
    assert_eq!(&text[rust.content.clone()], "x");
    assert!(!rust.escapes, "a raw literal reads no escapes");
    let python = literal("'''y'''", 0..7).unwrap();
    assert_eq!(python.content, 3..4);
    let open = literal("\"open", 0..5).unwrap();
    assert_eq!(open.content, 1..5, "an unclosed literal runs to its end");
    let text = "q(`one\ntwo`)";
    let lines = Lines::new(&Strings, text);
    let spans = StringLines(lines).spans(text, 0..text.len());
    assert_eq!(spans.len(), 2, "one piece per line");
    let joined = literals(text, &spans);
    assert_eq!(joined.len(), 1);
    assert_eq!(&text[joined[0].content.clone()], "one\ntwo");
    assert_eq!(literal("(x)", 0..3), None);
}

#[test]
fn code_reads_escapes_and_maps_back() {
    let text = "s := \"SELECT \\\"Name\\\"\\nFROM t\"";
    let literal = literal(text, 5..text.len()).unwrap();
    let code = Code::new(text, &literal);
    assert_eq!(code.text, "SELECT \"Name\"\nFROM t");
    let name = code.text.find("Name").unwrap();
    assert_eq!(&text[code.to_text(name)..code.to_text(name + 4)], "Name");
    let from = code.text.find("FROM").unwrap();
    assert_eq!(code.from_text(code.to_text(from)), from);
    assert_eq!(code.to_text(code.text.len()), literal.content.end);
    let raw = Literal { escapes: false, ..literal.clone() };
    assert_eq!(Code::new(text, &raw).text, &text[literal.content]);
}

#[test]
fn fragments_are_found_and_laid_over_the_string() {
    for incremental in [false, true] {
        let text = "x = \"SELECT a FROM t\"\ny = \"select nothing\"\nz = `SELECT\n  b FROM u`\n";
        let highlighting = injected(text, incremental);
        let fragments = highlighting.fragments();
        assert_eq!(fragments.len(), 2, "{incremental}");
        assert_eq!(&*fragments[0].language, "sql");
        assert_eq!(&text[fragments[1].literal.content.clone()], "SELECT\n  b FROM u");
        let spans = highlighting.spans_with_injections(text, 0..21);
        let kinds: Vec<(&str, HighlightKind)> =
            spans.iter().map(|span| (&text[span.range.clone()], span.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("\"", HighlightKind::String),
                ("SELECT", HighlightKind::Keyword),
                ("FROM", HighlightKind::Keyword),
                ("\"", HighlightKind::String)
            ]
        );
        let second = highlighting.spans_with_injections(text, 22..42);
        assert!(second.iter().all(|span| span.kind == HighlightKind::String), "not SQL: left a string");
        let at = text.find("b FROM").unwrap();
        assert_eq!(highlighting.fragment_at(at).map(|fragment| fragment.literal.range.start), text.find('`'));
        assert!(highlighting.fragment_at(2).is_none());
        assert_eq!(highlighting.spans(text, 0..21).len(), 1, "the plain spans stay the host's");
        let mut line = 0;
        for piece in text.split_inclusive('\n') {
            let range = line..line + piece.len();
            for span in highlighting.spans_with_injections(text, range.clone()) {
                assert!(span.range.end > range.start && span.range.start < range.end, "{span:?} touches {range:?}");
            }
            line = range.end;
        }
    }
}

#[test]
fn no_injectors_no_fragments() {
    let text = "x = \"SELECT a FROM t\"";
    let mut highlighting = Highlighting::new(Arc::new(Host { incremental: true }), text);
    highlighting.inject(text, Vec::new());
    assert!(highlighting.fragments().is_empty());
    assert_eq!(highlighting.spans_with_injections(text, 0..text.len()), highlighting.spans(text, 0..text.len()));
}

#[test]
fn random_edits_find_what_a_fresh_scan_finds() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move |below: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % below.max(1) as u64) as usize
    };
    let pieces = ["\"", "`", "\n", "SELECT ", "x", " FROM t", "", "\\\"", "\"SELECT 1\"\n"];
    for incremental in [true, false] {
        let mut text = String::from("a \"SELECT 1\"\nb `SELECT\nx` c\n\"no\"\n");
        let mut highlighting = injected(&text, incremental);
        for _ in 0..1_500 {
            let mut start = next(text.len() + 1);
            while !text.is_char_boundary(start) {
                start -= 1;
            }
            let end = (start + next(5)).min(text.len());
            let insert = pieces[next(pieces.len())];
            text.replace_range(start..end, insert);
            let change = TextChange { start, old_end: end, new_end: start + insert.len() };
            highlighting.update(&text, Some(&[change]));
            assert_eq!(found(&highlighting), found(&injected(&text, incremental)), "{text:?}");
        }
    }
}

#[test]
fn an_edit_far_from_strings_looks_at_its_lines_only() {
    let mut text = String::from("a \"SELECT 1\"\n");
    text.push_str(&"plain line\n".repeat(200));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    struct Counting(Arc<std::sync::atomic::AtomicUsize>);
    impl Injector for Counting {
        fn language(&self) -> &str {
            "sql"
        }
        fn may_hold(&self, text: &str) -> bool {
            self.0.fetch_add(text.len(), std::sync::atomic::Ordering::Relaxed);
            text.contains("SELECT")
        }
        fn accepts(&self, _: &str, _: &Literal, code: &str) -> bool {
            code.starts_with("SELECT")
        }
        fn highlight(&self, _: &str) -> Vec<Span> {
            Vec::new()
        }
    }
    let mut highlighting = Highlighting::new(Arc::new(Host { incremental: true }), &text);
    highlighting.inject(&text, vec![Arc::new(Counting(Arc::clone(&calls)))]);
    calls.store(0, std::sync::atomic::Ordering::Relaxed);
    let at = text.len() - 5;
    text.insert(at, 'z');
    highlighting.update(&text, Some(&[TextChange { start: at, old_end: at, new_end: at + 1 }]));
    assert!(calls.load(std::sync::atomic::Ordering::Relaxed) < 64, "only the lines around the edit");
    assert_eq!(highlighting.fragments().len(), 1);
}

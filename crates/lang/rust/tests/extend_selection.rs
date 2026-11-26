use zen_editor::{Structure, StructureOptions, Token, TokenKind};
use zen_lang_rust::RustHighlighter;
use zen_syntax::Highlighter;

fn steps(text: &str, mark: &str) -> Vec<String> {
    let chars = |byte: usize| text[..byte].chars().count();
    let tokens = RustHighlighter
        .highlight(text)
        .into_iter()
        .filter_map(|span| {
            let kind = if span.kind.is_comment() {
                TokenKind::Comment
            } else if span.kind.is_string() {
                TokenKind::String
            } else {
                return None;
            };
            Some(Token { range: chars(span.range.start)..chars(span.range.end), kind })
        })
        .collect();
    let structure = Structure::new(text, tokens, StructureOptions::default());
    let at = chars(text.find(mark).unwrap());
    let mut range = at..at;
    let mut seen = Vec::new();
    while let Some(next) = structure.extend(range.clone(), &[]) {
        seen.push(text.chars().skip(next.start).take(next.len()).collect());
        range = next;
    }
    seen
}

const SOURCE: &str = r##"use std::collections::HashMap;

/// Counts the words.
#[must_use]
pub fn count(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split(r#" "#) {
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
}
"##;

#[test]
fn a_call_in_a_loop_grows_out_to_the_documented_function() {
    let found = steps(SOURCE, "insert");
    assert_eq!(
        found[..6],
        [
            "or_insert".to_owned(),
            "counts.entry(word).or_insert".into(),
            "counts.entry(word).or_insert(0)".into(),
            "*counts.entry(word).or_insert(0) += 1;".into(),
            "{\n        *counts.entry(word).or_insert(0) += 1;\n    }".into(),
            "for word in text.split(r#\" \"#) {\n        *counts.entry(word).or_insert(0) += 1;\n    }".into(),
        ]
    );
    let function = &SOURCE[SOURCE.find("pub fn").unwrap()..SOURCE.len() - 1];
    let at = found.iter().position(|step| step == function).expect("the function");
    assert_eq!(found[at + 1], SOURCE[SOURCE.find("/// Counts").unwrap()..SOURCE.len() - 1]);
    assert_eq!(found[at + 2], SOURCE);
}

#[test]
fn a_raw_string_loses_its_hashes_and_quotes_first() {
    let found = steps(SOURCE, "\"#)");
    assert_eq!(found[..4], [" ".to_owned(), "r#\" \"#".into(), "(r#\" \"#)".into(), "text.split(r#\" \"#)".into()]);
}

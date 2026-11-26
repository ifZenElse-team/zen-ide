use zen_editor::{Structure, StructureOptions, Token, TokenKind};
use zen_lang_go::GoHighlighter;
use zen_syntax::Highlighter;

fn structure(text: &str) -> Structure {
    let chars = |byte: usize| text[..byte].chars().count();
    let tokens = GoHighlighter
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
    Structure::new(text, tokens, StructureOptions::default())
}

fn steps(text: &str, mark: &str) -> Vec<String> {
    let structure = structure(text);
    let at = text[..text.find(mark).unwrap()].chars().count();
    let mut range = at..at;
    let mut seen = Vec::new();
    while let Some(next) = structure.extend(range.clone(), &[]) {
        seen.push(text.chars().skip(next.start).take(next.len()).collect());
        range = next;
    }
    seen
}

const SOURCE: &str = r#"package main

import "fmt"

// Greet prints a greeting.
// It is called from main.
func Greet(names []string) {
	for _, name := range names {
		if name != "" {
			fmt.Printf("hello, %s\n", name)
		}
	}
}
"#;

#[test]
fn a_string_in_a_call_in_an_if_grows_out_to_the_file() {
    let printf = "fmt.Printf(\"hello, %s\\n\", name)";
    let block = format!("{{\n\t\t\t{printf}\n\t\t}}");
    let statement = format!("if name != \"\" {block}");
    let range_block = format!("{{\n\t\t{statement}\n\t}}");
    let range_loop = format!("for _, name := range names {range_block}");
    let body = format!("{{\n\t{range_loop}\n}}");
    let function = format!("func Greet(names []string) {body}");
    assert_eq!(
        steps(SOURCE, "hello"),
        [
            "hello".to_owned(),
            "hello, %s\\n".into(),
            "\"hello, %s\\n\"".into(),
            "\"hello, %s\\n\", name".into(),
            "(\"hello, %s\\n\", name)".into(),
            printf.into(),
            block,
            statement,
            range_block,
            range_loop,
            body,
            function.clone(),
            format!("// Greet prints a greeting.\n// It is called from main.\n{function}"),
            SOURCE.into(),
        ]
    );
}

#[test]
fn a_doc_comment_grows_by_sentence_then_to_its_declaration() {
    let found = steps(SOURCE, "called");
    assert_eq!(
        found[..3],
        [
            "called".to_owned(),
            "It is called from main.".into(),
            "Greet prints a greeting.\n// It is called from main.".into()
        ]
    );
    assert_eq!(found[3], "// Greet prints a greeting.\n// It is called from main.");
    assert!(found[4].starts_with("// Greet prints a greeting.\n// It is called from main.\nfunc Greet("));
}

#[test]
fn a_raw_string_and_an_import_path() {
    let text = "package main\n\nvar query = `select 1`\n";
    assert_eq!(steps(text, "select")[..3], ["select".to_owned(), "select 1".into(), "`select 1`".into()]);
    assert_eq!(steps(SOURCE, "fmt\"")[..3], ["fmt".to_owned(), "\"fmt\"".into(), "import \"fmt\"".into()]);
}

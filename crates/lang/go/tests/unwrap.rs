use zen_editor::{UnwrapOption, unwrap_options};
use zen_lang_go::GoHighlighter;
use zen_syntax::Highlighter;

fn options(text: &str, mark: &str) -> Vec<UnwrapOption> {
    let chars = |byte: usize| text[..byte].chars().count();
    let skipped: Vec<_> = GoHighlighter
        .highlight(text)
        .into_iter()
        .filter(|span| span.kind.is_comment() || span.kind.is_string())
        .map(|span| chars(span.range.start)..chars(span.range.end))
        .collect();
    unwrap_options(text, &skipped, text[..text.find(mark).unwrap()].chars().count())
}

fn apply(text: &str, option: &UnwrapOption) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: String = chars[..option.range.start].iter().collect();
    out.push_str(&option.replacement);
    out.extend(&chars[option.range.end..]);
    out
}

const SOURCE: &str = r#"package main

func handle(items []string) {
	for _, item := range items {
		switch item {
		case "{":
			println(`}` + item) // }
		default:
			if r := '}'; item != "" {
				println(r)
			}
		}
	}
}
"#;

#[test]
fn braces_in_strings_runes_and_comments_do_not_count() {
    let found = options(SOURCE, "println(`");
    let names: Vec<_> = found.iter().map(|option| option.construct.clone().unwrap_or_default()).collect();
    assert_eq!(names, ["println(…)", "case…", "case…", "for…"]);
    assert_eq!(
        apply(SOURCE, &found[1]),
        "package main\n\nfunc handle(items []string) {\n\tfor _, item := range items {\n\t\tprintln(`}` + item) // }\n\t}\n}\n"
    );
    let found = options(SOURCE, "println(r)");
    let names: Vec<_> = found.iter().map(|option| option.construct.clone().unwrap_or_default()).collect();
    assert_eq!(names, ["println(…)", "if…", "default…", "default…", "for…"]);
    assert!(apply(SOURCE, &found[1]).contains("\t\tdefault:\n\t\t\tprintln(r)\n\t\t}\n"));
}

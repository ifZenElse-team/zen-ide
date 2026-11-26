use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use zen_core::{Registry, Session};
use zen_syntax::{HighlightKind, Highlighter, Span};
use zen_workbench::Workbench;
use zen_workbench::brackets::Highlighters;
use zen_workbench::templates::surround::{Surrounder, wrap};
use zen_workbench::templates::{Around, Builtin, LiveTemplates, Template, expand_around};

const SURROUNDERS: &str = r#"[
    { "language": "go", "id": "if", "title": "if", "text": "if $CONDITION$ {\n\t$SELECTION$\n}" },
    { "language": "go", "id": "ifElse", "title": "if / else", "text": "if $CONDITION$ {\n\t$SELECTION$\n} else {\n\t$END$\n}" },
    { "language": "go", "id": "for", "title": "for", "text": "for $CONDITION$ {\n\t$SELECTION$\n}" },
    { "language": "go", "id": "func", "title": "func", "text": "func() {\n\t$SELECTION$\n}()" },
    { "language": "go", "id": "region", "title": "region", "text": "// region $DESCRIPTION$\n$SELECTION$\n// endregion" },
    { "language": "go", "id": "parentheses", "title": "(expr)", "text": "($SELECTION$)", "expression": true },
    { "language": "go", "id": "not", "title": "!(expr)", "text": "!($SELECTION$)", "expression": true }
]"#;

fn surrounders() -> Vec<Surrounder> {
    Surrounder::parse_list(SURROUNDERS).unwrap()
}

fn template(surrounder: &Surrounder) -> Template {
    Template {
        language: surrounder.language.clone(),
        abbreviation: surrounder.id.clone(),
        description: String::new(),
        text: surrounder.text.clone(),
        variables: surrounder.variables.clone(),
        imports: Vec::new(),
        contexts: Vec::new(),
        enabled: true,
    }
}

fn skipped(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let end = if rest.starts_with("//") {
            rest.find('\n').map_or(text.len(), |found| at + found)
        } else if let Some(body) = rest.strip_prefix("/*") {
            body.find("*/").map_or(text.len(), |found| at + found + 4)
        } else if let Some(body) = rest.strip_prefix("r#\"") {
            body.find("\"#").map_or(text.len(), |found| at + found + 5)
        } else if rest.starts_with('"') || rest.starts_with('`') {
            let quote = rest.as_bytes()[0];
            let mut end = at + 1;
            while end < text.len() && text.as_bytes()[end] != quote {
                end += if text.as_bytes()[end] == b'\\' && quote != b'`' { 2 } else { 1 };
            }
            (end + 1).min(text.len())
        } else if rest.starts_with('\'') && rest.chars().nth(2) == Some('\'') {
            at + 1 + rest.chars().nth(1).map_or(0, char::len_utf8) + 1
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        };
        ranges.push(at..end);
        at = end;
    }
    ranges
}

fn hidden(skipped: &[Range<usize>], at: usize) -> bool {
    skipped.iter().any(|range| range.contains(&at))
}

fn balanced(text: &str, range: Range<usize>, skipped: &[Range<usize>]) -> bool {
    let mut stack = Vec::new();
    for (at, c) in text[range.clone()].char_indices() {
        if hidden(skipped, range.start + at) {
            continue;
        }
        match c {
            '(' | '[' | '{' => stack.push(c),
            ')' | ']' | '}' => {
                let open = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if stack.pop() != Some(open) {
                    return false;
                }
            }
            _ => {}
        }
    }
    stack.is_empty()
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |found| found + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |found| at + found)
}

fn leading(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn unit_of(text: &str) -> &'static str {
    if text.contains("\n\t") {
        "\t"
    } else if text.contains("\n    ") {
        "    "
    } else {
        "  "
    }
}

fn wrapped_code<'a>(template: &Template, indent: &str, unit: &str, crlf: bool, expanded: &'a str) -> &'a str {
    let empty = Around { crlf, ..Around::default() };
    let cut = template.text.find("$SELECTION$").unwrap();
    let mut before = template.clone();
    before.text.truncate(cut);
    let before = expand_around(&before, indent, unit, &empty).text;
    let without = expand_around(template, indent, unit, &empty).text;
    let after = without.len() - before.len();
    assert!(expanded.starts_with(&before), "{expanded:?} does not start with {before:?}");
    &expanded[before.len()..expanded.len() - after]
}

fn check(text: &str, selection: Range<usize>, surrounder: &Surrounder) {
    let skipped = skipped(text);
    let unit = unit_of(text);
    let template = template(surrounder);
    let crlf = text.contains("\r\n");
    let wrapped = wrap(text, &skipped, selection.clone(), &template, surrounder.expression, unit);
    let range = wrapped.range.clone();
    let result = format!("{}{}{}", &text[..range.start], wrapped.expanded.text, &text[range.end..]);
    let what = format!("{} over {selection:?} wrapped {range:?} of\n{text}\ngiving\n{result}", surrounder.id);
    let picked = &text[selection.clone()];
    let code = !picked.trim().is_empty();

    if surrounder.expression && code {
        assert_eq!(range, selection, "an expression is taken as it is: {what}");
    } else if range.is_empty() {
        assert!(!code, "{what}");
        assert!(text[line_start(text, selection.end)..line_end(text, selection.end)].trim().is_empty(), "{what}");
    } else {
        assert!(text[line_start(text, range.start)..range.start].trim().is_empty(), "starts mid-line: {what}");
        assert!(text[range.end..line_end(text, range.end)].trim().is_empty(), "ends mid-line: {what}");
        assert!(balanced(text, range.clone(), &skipped), "not a whole statement: {what}");
        if code {
            let first = selection.start + picked.len() - picked.trim_start().len();
            let last = selection.start + picked.trim_end().len();
            assert!(range.start <= first && last <= range.end, "the selection is not all wrapped: {what}");
        } else {
            let caret_line = line_start(text, selection.end)..line_end(text, selection.end);
            let content = caret_line.start + leading(&text[caret_line.clone()]).len();
            assert!(range.start <= content && range.end >= content, "the caret's line is not wrapped: {what}");
        }
        assert!(balanced(&result, 0..result.len(), &skipped_of_result(&result)), "unbalanced: {what}");
    }

    let indent = leading(&text[line_start(text, range.start)..range.start]);
    let inside = wrapped_code(&template, indent, unit, crlf, &wrapped.expanded.text);
    let original: Vec<&str> = text[range.clone()].split_inclusive('\n').collect();
    let mut changed: Vec<&str> = inside.split_inclusive('\n').collect();
    if original.last().is_some_and(|last| last.trim().is_empty() && !last.ends_with('\n'))
        && changed.len() < original.len()
    {
        changed.push("");
    }
    assert_eq!(original.len(), changed.len(), "the lines: {what}");
    let mut at = range.start;
    for (index, (before, after)) in original.iter().zip(&changed).enumerate() {
        let starts = at;
        at += before.len();
        if index == 0 || skipped.iter().any(|range| range.start < starts && starts < range.end) {
            assert_eq!(before, after, "line {index} changes: {what}");
        } else if before.trim().is_empty() {
            assert_eq!(after.trim_matches([' ', '\t']), before.trim_matches([' ', '\t']), "line {index}: {what}");
        } else {
            assert_eq!(
                after.trim_start_matches([' ', '\t']),
                before.trim_start_matches([' ', '\t']),
                "line {index} changes but for its indentation: {what}"
            );
        }
    }
    for range in skipped.iter().filter(|skip| skip.start >= range.start && skip.end <= range.end) {
        assert!(result.contains(&text[range.clone()]), "a string or comment changes: {what}");
    }
    if crlf {
        assert_eq!(result.matches('\n').count(), result.matches("\r\n").count(), "a bare line feed: {what}");
    }
    let kinds: String = text.lines().flat_map(|line| leading(line).chars()).collect();
    let mut at = 0;
    for line in result.split_inclusive('\n') {
        let starts = at;
        at += line.len();
        if hidden(&skipped_of_result(&result), starts) && starts > 0 {
            continue;
        }
        for c in leading(line).chars() {
            assert!(kinds.contains(c) || kinds.is_empty(), "indentation of another kind: {what}");
        }
    }
}

fn skipped_of_result(result: &str) -> Vec<Range<usize>> {
    skipped(result)
}

fn boundaries(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()))
        .filter(|&at| !(text[..at].ends_with('\r') && text[at..].starts_with('\n')))
        .collect()
}

fn every_caret_and_selection(snippets: &[&str]) {
    let surrounders = surrounders();
    for text in snippets {
        let offsets = boundaries(text);
        for (index, &start) in offsets.iter().enumerate() {
            for &end in &offsets[index..] {
                for surrounder in &surrounders {
                    if surrounder.expression && text[start..end].trim().is_empty() {
                        continue;
                    }
                    check(text, start..end, surrounder);
                }
            }
        }
    }
}

const GO: &[&str] = &[
    "package main\n\nfunc f(x int) {\n\tif x > 0 {\n\t\ta()\n\t} else if x < 0 {\n\t\tb()\n\t} else {\n\t\td()\n\t}\n}\n",
    "func f() {\n\tfor i := 0; i < n; i++ {\n\t\tif err := g(i); err != nil { // }\n\t\t\treturn err\n\t\t}\n\t}\n}\n",
    "func f() {\n\tswitch x {\n\tcase '}':\n\t\ta(\"{\")\n\tcase 2, 3:\n\t\t// {\n\t\tb()\n\t}\n}\n",
    "func f() {\n\tgo func() {\n\t\twork()\n\t}()\n\tdefer func() { recover() }()\n}\n",
    "func T(t *testing.T) {\n\tt.Run(\"a\", func(t *testing.T) {\n\t\tcheck(t)\n\t})\n\tt.Run(\"b\",\n\t\tfunc(t *testing.T) {\n\t\t\tcheck(t)\n\t\t})\n}\n",
    "func f() {\n\tq := `\n{\n\t}`\n\tif ok {\n\t\ts := `line\n\t\tkeep\n}`\n\t\tuse(s, q)\n\t}\n}\n",
    "func f() {\n\tif ok {\n\t\t/* {\n\t\t   block\n\t\t*/\n\t\tg()\n\t}\n}\n",
    "func f() {\n\t{\n\t\ta := 1\n\n\t\tuse(a)\n\t}\n\tx := T{\n\t\ta: 1,\n\t}\n}\n",
    "func f() {\n\tv := strings.TrimSpace(read(f, 1))\n\tw := wrap(\n\t\tinner(x),\n\t)\n\tz := m[k](y)\n}\n",
    "func f() {\n\tif a { if b { c() } }\n\tx := 1; if ok { a() }; y()\n}\n",
    "func f() {\n  if ok {\n      deep()\n    shallow()\n  }\n}\n",
    "func f() {\r\n\tif ok {\r\n\t\ta()\r\n\t} else {\r\n\t\tb()\r\n\t}\r\n\ts := `x\r\n  y`\r\n}\r\n",
    "func f() {\n\tv := a +\n\t\tb\n\tw := c.\n\t\td()\n\ti++\n}\n",
    "func f() {\nlabel:\n\tfor {\n\t\tbreak label\n\t}\n}\n",
    "func f() {\n\t \n\ta()\n\t\t\n\tb()\n}\n",
];

const RUST: &[&str] = &[
    "fn f() {\n    if a {\n        b();\n    } else if c {\n        d();\n    }\n    while go() {\n        step(\"}\");\n    }\n}\n",
    "fn f() -> u8 {\n    for i in 0..n {\n        let s = r#\"\n{ not code\n\"#;\n        println!(\"{}\", s);\n    }\n    match x {\n        Some(v) => { v }\n        None => 0,\n    }\n}\n",
    "fn f<'a>(x: &'a str) {\n    {\n        let a = '{';\n        g(a, x);\n    }\n    let v = items\n        .iter()\n        .map(|x| x + 1);\n}\n",
];

const JS: &[&str] = &[
    "function f() {\n  if (a == b) {\n    c();\n  } else {\n    d(`}${e}\n  }`);\n  }\n  for (let i = 0; i < n; i++) {\n    g(i);\n  }\n}\n",
    "const f = function (x) {\n  setTimeout(function () {\n    run(x);\n  }, 10);\n  const o = {\n    a: 1,\n    b: [2, 3],\n  };\n  return ok\n    && wrap(x);\n};\n",
    "if (a) {\r\n  b('}');\r\n} else {\r\n  /* ( */ c();\r\n}\r\n",
];

#[test]
fn go() {
    every_caret_and_selection(GO);
}

#[test]
fn rust() {
    every_caret_and_selection(RUST);
}

#[test]
fn javascript() {
    every_caret_and_selection(JS);
}

struct ByHand;

impl Highlighter for ByHand {
    fn handles(&self, file_name: &str) -> bool {
        file_name.starts_with("snippet.")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        for range in skipped(text) {
            let mut start = range.start;
            for piece in text[range.clone()].split_inclusive('\n') {
                spans.push(Span { range: start..start + piece.len(), kind: HighlightKind::String });
                start += piece.len();
            }
        }
        spans
    }
}

fn text_of(session: &Session) -> String {
    session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
}

fn char_offset(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

#[test]
fn the_commands_change_what_wrap_says_and_one_undo_gives_the_file_back() {
    zen_i18n::register(zen_workbench::LOCALES);
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let mut session = Session::new();
    let highlighter: Arc<dyn Highlighter> = Arc::new(ByHand);
    session
        .state_mut::<Highlighters>()
        .set_source(Arc::new(move |name| ByHand.handles(name).then(|| Arc::clone(&highlighter))));
    let templates = LiveTemplates::default();
    for language in ["go", "rust", "javascript"] {
        let surrounders = surrounders()
            .into_iter()
            .filter(|surrounder| surrounder.id != "region")
            .map(|surrounder| Surrounder { language: language.to_owned(), ..surrounder })
            .collect();
        templates.declare_language(Builtin {
            language: language.to_owned(),
            templates: Vec::new(),
            add_import: None,
            surrounders,
            ..Builtin::default()
        });
    }
    session.set_state(templates);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
    let run = |session: &mut Session, id: &str, params: serde_json::Value| {
        registry.dispatch(session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    };
    for (extension, snippets) in [("go", GO), ("rs", RUST), ("js", JS)] {
        for (number, text) in snippets.iter().enumerate() {
            let path = folder.path().join(format!("snippet.{number}.{extension}"));
            std::fs::write(&path, text).unwrap();
            run(&mut session, "file.document.open", json!({ "path": path }));
            let skipped = skipped(text);
            let offsets = boundaries(text);
            let lines: Vec<usize> = offsets.iter().copied().filter(|&at| at == line_start(text, at)).collect();
            let selections = offsets.iter().map(|&at| at..at).chain(
                lines
                    .iter()
                    .flat_map(|&start| lines.iter().filter(move |&&end| end > start).map(move |&end| start..end)),
            );
            for selection in selections {
                let (anchor, head) = (char_offset(text, selection.start), char_offset(text, selection.end));
                run(
                    &mut session,
                    "editor.selection.set",
                    json!({ "selections": [{ "anchor": anchor, "head": head }] }),
                );
                let listed = run(&mut session, "editor.surround.list", json!({}));
                for item in listed["items"].as_array().unwrap() {
                    let name = item["name"].as_str().unwrap();
                    let Some(surrounder) =
                        surrounders().into_iter().find(|surrounder| surrounder.id == name && surrounder.id != "region")
                    else {
                        continue;
                    };
                    let unit = {
                        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
                        zen_workbench::project_settings::indent_unit_of(&session, document)
                    };
                    let wrapped =
                        wrap(text, &skipped, selection.clone(), &template(&surrounder), surrounder.expression, &unit);
                    let expected = format!(
                        "{}{}{}",
                        &text[..wrapped.range.start],
                        wrapped.expanded.text,
                        &text[wrapped.range.end..]
                    );
                    run(&mut session, "editor.surround.apply", json!({ "name": name }));
                    assert_eq!(text_of(&session), expected, "{name} over {selection:?} of {}", path.display());
                    run(&mut session, "editor.edit.undo", json!({}));
                    assert_eq!(text_of(&session), *text, "one undo after {name} over {selection:?} of\n{text}");
                    run(
                        &mut session,
                        "editor.selection.set",
                        json!({ "selections": [{ "anchor": anchor, "head": head }] }),
                    );
                }
            }
            run(&mut session, "file.document.close", json!({ "path": path }));
            assert!(Path::new(&path).exists());
        }
    }
}

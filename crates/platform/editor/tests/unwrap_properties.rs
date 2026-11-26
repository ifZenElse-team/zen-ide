use std::ops::Range;

use zen_editor::{UnwrapKind, UnwrapOption, unwrap_options};

fn skipped(text: &str) -> Vec<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let start = at;
        let rest = |at: usize, s: &str| chars[at..].iter().take(s.chars().count()).copied().eq(s.chars());
        if rest(at, "//") {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if rest(at, "/*") {
            at += 2;
            while at < chars.len() && !rest(at, "*/") {
                at += 1;
            }
            at = (at + 2).min(chars.len());
        } else if rest(at, "r#\"") {
            at += 3;
            while at < chars.len() && !rest(at, "\"#") {
                at += 1;
            }
            at = (at + 2).min(chars.len());
        } else if matches!(chars[at], '"' | '`') || (chars[at] == '\'' && chars.get(at + 2) == Some(&'\'')) {
            let quote = chars[at];
            at += 1;
            while at < chars.len() && chars[at] != quote {
                if chars[at] == '\\' && quote != '`' {
                    at += 1;
                }
                at += 1;
            }
            at = (at + 1).min(chars.len());
        } else {
            at += 1;
            continue;
        }
        ranges.push(start..at);
    }
    ranges
}

fn slice(chars: &[char], range: Range<usize>) -> String {
    chars[range].iter().collect()
}

fn apply(text: &str, option: &UnwrapOption) -> String {
    let chars: Vec<char> = text.chars().collect();
    slice(&chars, 0..option.range.start) + &option.replacement + &slice(&chars, option.range.end..chars.len())
}

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn balanced(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let hidden = skipped(text);
    let mut stack = Vec::new();
    for (at, &c) in chars.iter().enumerate() {
        if hidden.iter().any(|range| range.contains(&at)) {
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

fn check(text: &str) {
    let hidden = skipped(text);
    let chars: Vec<char> = text.chars().collect();
    let indents: String = text.lines().flat_map(|line| line.chars().take_while(|c| *c == ' ' || *c == '\t')).collect();
    for caret in 0..=chars.len() {
        for option in unwrap_options(text, &hidden, caret) {
            let result = apply(text, &option);
            let what = format!("{option:?} at {caret} of\n{text}\ngave\n{result}");
            let mut removed = String::new();
            let mut from = option.range.start;
            let mut kept_text = String::new();
            for kept in
                option.kept.iter().filter(|kept| kept.start >= option.range.start && kept.end <= option.range.end)
            {
                removed += &slice(&chars, from..kept.start);
                kept_text += &slice(&chars, kept.clone());
                from = kept.end;
            }
            removed += &slice(&chars, from..option.range.end);
            if option.kind == UnwrapKind::Unwrap {
                assert_eq!(squeeze(&option.replacement), squeeze(&kept_text), "the kept code changes: {what}");
            } else {
                assert!(option.replacement.is_empty(), "{what}");
            }
            assert!(balanced(&removed), "what goes is not whole: {removed:?} in {what}");
            assert!(balanced(&result), "unbalanced: {what}");
            for range in &hidden {
                if range.start >= option.range.start && range.end <= option.range.end {
                    let literal = slice(&chars, range.clone());
                    let inside = option.kept.iter().any(|kept| kept.start <= range.start && range.end <= kept.end);
                    assert!(!inside || result.contains(&literal), "a string or comment changes: {literal:?} in {what}");
                }
            }
            if text.contains("\r\n") {
                assert_eq!(result.matches('\n').count(), result.matches("\r\n").count(), "a bare line feed: {what}");
            }
            for line in result.lines() {
                for c in line.chars().take_while(|c| *c == ' ' || *c == '\t') {
                    assert!(indents.contains(c), "indentation of another kind: {what}");
                }
            }
            assert!(option.caret <= result.chars().count(), "{what}");
        }
    }
}

const GO: &[&str] = &[
    "package main\n\nfunc f(x int) {\n\tif x > 0 {\n\t\ta()\n\t} else if x < 0 {\n\t\tb()\n\t} else if x == 9 {\n\t\tc()\n\t} else {\n\t\td()\n\t}\n}\n",
    "func f() {\n\tfor i := 0; i < n; i++ {\n\t\tif err := g(i); err != nil { // }\n\t\t\treturn err\n\t\t}\n\t}\n}\n",
    "func f() {\n\tswitch x {\n\tcase '}':\n\t\ta(\"{\")\n\tcase 2, 3:\n\t\t// {\n\t\tb()\n\tdefault:\n\t}\n}\n",
    "func f() {\n\tselect {\n\tcase v := <-ch:\n\t\tuse(v)\n\tcase <-done:\n\t\treturn\n\t}\n}\n",
    "func f() {\n\tgo func() {\n\t\twork()\n\t}()\n\tdefer func() { recover() }()\n}\n",
    "func T(t *testing.T) {\n\tt.Run(\"a\", func(t *testing.T) {\n\t\tcheck(t)\n\t})\n\tt.Run(\"b\",\n\t\tfunc(t *testing.T) {\n\t\t\tcheck(t)\n\t\t})\n}\n",
    "func f() error {\n\terr := retry(func() error {\n\t\treturn g()\n\t}, 3)\n\treturn err\n}\n",
    "func f() {\n\tq := `\n{\n\t}`\n\tif ok {\n\t\ts := `line\n\t\tkeep\n}`\n\t\tuse(s, q)\n\t}\n}\n",
    "func f() {\n\tif ok {\n\t\t/* {\n\t\t   block\n\t\t*/\n\t\tg()\n\t}\n}\n",
    "func f() {\n\t{\n\t\ta := 1\n\n\t\tuse(a)\n\t}\n\tx := T{\n\t\ta: 1,\n\t}\n}\n",
    "func f() {\n\tv := strings.TrimSpace(read(f, 1))\n\tw := wrap(\n\t\tinner(x),\n\t)\n\tz := m[k](y)\n}\n",
    "func f() {\n\tif a { if b { c() } }\n\tx := 1; if ok { a() }; y()\n}\n",
    "func f() {\n\tif ok {\n\t}\n\tfor {\n\t\tif done {\n\t\t\tbreak\n\t\t} else {\n\t\t\tcontinue\n\t\t}\n\t}\n}\n",
    "func f() {\n  if ok {\n      deep()\n    shallow()\n  }\n}\n",
    "func f() {\r\n\tif ok {\r\n\t\ta()\r\n\t} else {\r\n\t\tb()\r\n\t}\r\n}\r\n",
    "func f() {\n\tif x {\n\t\ta()\n\t} // tail\n\telse {\n\t}\n}\n",
    "func f() {\n\tif x { a()\n\t\tb() }\n\tfor { c() }\n}\n",
    "func f() {\n\tfn := func(a, b int) (int, error) {\n\t\treturn a + b, nil\n\t}\n\t_ = fn\n}\n",
    "func f() {\nlabel:\n\tfor {\n\t\tswitch {\n\t\tcase a:\n\t\t\tbreak label\n\t\t}\n\t}\n}\n",
];

const RUST: &[&str] = &[
    "fn f() {\n    if a {\n        b();\n    } else if c {\n        d();\n    } else {\n        e();\n    }\n    while go() {\n        step(\"}\");\n    }\n    loop { break; }\n}\n",
    "fn f() -> u8 {\n    let x = Some(compute(1));\n    for i in 0..n {\n        let s = r#\"\n{ not code\n\"#;\n        println!(\"{}\", s);\n    }\n    match x {\n        Some(v) => { v }\n        None => 0,\n    }\n}\n",
    "fn f() {\n    {\n        let a = '{';\n        g(a);\n    }\n}\n",
];

const JS: &[&str] = &[
    "function f() {\n  if (a == b) {\n    c();\n  } else {\n    d(`}${e}`);\n  }\n  for (let i = 0; i < n; i++) {\n    g(i);\n  }\n  switch (x) {\n    case 1:\n      h();\n      break;\n    default:\n      k();\n  }\n}\n",
    "const f = function (x) {\n  setTimeout(function () {\n    run(x);\n  }, 10);\n  return wrap(x);\n};\n",
    "while (x) {\n  // }\n  y('}');\n}\n",
    "if (a) {\r\n  b();\r\n} else {\r\n  c();\r\n}\r\n",
];

#[test]
fn go() {
    GO.iter().for_each(|text| check(text));
}

#[test]
fn rust() {
    RUST.iter().for_each(|text| check(text));
}

#[test]
fn javascript() {
    JS.iter().for_each(|text| check(text));
}

fn titles(text: &str, mark: &str) -> Vec<String> {
    let caret = text[..text.find(mark).unwrap()].chars().count();
    unwrap_options(text, &skipped(text), caret).into_iter().filter_map(|option| option.construct).collect()
}

#[test]
fn patterns_attributes_and_go_statements_are_not_calls() {
    assert_eq!(titles("match x {\n    Some(v) => v,\n}\n", "v)"), Vec::<String>::new());
    assert_eq!(titles("if let Some(v) = x {\n    use_it(v);\n}\n", "v)"), ["if…"]);
    assert_eq!(titles("#[derive(Debug)]\nstruct A(u8);\n", "Debug"), Vec::<String>::new());
    assert_eq!(titles("struct A(u8);\n", "u8"), Vec::<String>::new());
    assert_eq!(titles("func f() {\n\tdefer close(ch)\n\tgo run(x)\n}\n", "ch)"), Vec::<String>::new());
    assert_eq!(titles("func f() {\n\tgo run(x)\n}\n", "x)"), Vec::<String>::new());
    assert_eq!(titles("v := len(x)\n", "x)"), ["len(…)"]);
}

#[test]
fn a_comment_between_the_branches_keeps_the_chain() {
    let text = "if (a) {\n  b();\n} // why\nelse {\n  c();\n}\n";
    let options = unwrap_options(text, &skipped(text), text.find("b()").unwrap());
    assert_eq!(apply(text, &options[0]), "b();\n");
}

#[test]
fn a_function_literal_split_from_its_call_is_not_offered() {
    let text = "func T(t *testing.T) {\n\tt.Run(\"b\",\n\t\tfunc(t *testing.T) {\n\t\t\tcheck(t)\n\t\t})\n}\n";
    assert_eq!(titles(text, "t)\n\t\t})"), ["check(…)"]);
    let text = "func f() {\n\ta(); go func() {\n\t\twork()\n\t}()\n}\n";
    assert_eq!(titles(text, "work"), Vec::<String>::new());
}

#[test]
fn a_raw_string_keeps_its_lines() {
    let text = "func f() {\n\tif ok {\n\t\ts := `a\n\t\t  b \n`\n\t\tuse(s)\n\t}\n}\n";
    let options = unwrap_options(text, &skipped(text), text.find("use").unwrap() - 1);
    let unwrapped = options.iter().find(|option| option.construct.as_deref() == Some("if…")).unwrap();
    assert_eq!(apply(text, unwrapped), "func f() {\n\ts := `a\n\t\t  b \n`\n\tuse(s)\n}\n");
}

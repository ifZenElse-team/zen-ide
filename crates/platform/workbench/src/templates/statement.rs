use std::ops::Range;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};

use super::{LiveTemplates, document_at};
use crate::editing::SelectionInfo;
use crate::language::{byte_offset, char_offset};
use crate::{code_style, project_settings};

/// How Complete Current Statement finishes a statement of a language.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementRules {
    /// The language id, e.g. `rust`.
    pub language: String,
    /// What ends a statement that has no block, e.g. `;`; nothing when empty.
    #[serde(default)]
    pub terminator: String,
    /// The words that start a statement or declaration with a block, e.g. `if`, `for`, `fn`, `struct`.
    #[serde(default)]
    pub blocks: Vec<String>,
    /// The words whose condition goes in parentheses, e.g. `if` and `while` in Java.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parenthesized: Vec<String>,
    /// Whether `Type name(parameters)`, two words or more before the first parenthesis, declares a method with a block.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub methods: bool,
    /// The words that start such a line without declaring a method, e.g. `return`, `throw`, `new`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_declarations: Vec<String>,
}

pub struct Completed {
    pub range: Range<usize>,
    pub text: String,
    pub caret: usize,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct Scan {
    closers: String,
    opens_block: bool,
    words: Vec<String>,
}

fn scan(body: &str) -> Scan {
    let mut stack: Vec<char> = Vec::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut head = true;
    for c in body.chars() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' && open == '"' {
                escaped = true;
            } else if c == open {
                quote = None;
            }
            continue;
        }
        if is_word(c) {
            word.push(c);
            continue;
        }
        if head && stack.is_empty() && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.clear();
        match c {
            '"' | '`' => quote = Some(c),
            '(' | '[' | '{' => {
                if c == '{' && stack.is_empty() {
                    head = false;
                }
                stack.push(c);
            }
            ')' | ']' | '}' => {
                stack.pop();
            }
            '=' | ';' if stack.is_empty() => head = false,
            _ => {}
        }
    }
    if head && stack.is_empty() && !word.is_empty() {
        words.push(word);
    }
    let mut closers: String = quote.map(String::from).unwrap_or_default();
    let opens_block = stack.contains(&'{');
    for open in stack.iter().rev().take_while(|open| **open != '{') {
        closers.push(if *open == '(' { ')' } else { ']' });
    }
    Scan { closers, opens_block, words }
}

fn declares_method(body: &str, rules: &StatementRules) -> bool {
    let Some((before, _)) = body.split_once('(') else { return false };
    let mut depth = 0i32;
    let mut tokens = Vec::new();
    let mut token = String::new();
    for c in before.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                continue;
            }
            c if !(is_word(c) || "[]?@$,. ".contains(c)) => return false,
            _ => {}
        }
        token.push(c);
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    rules.methods
        && tokens.len() >= 2
        && !tokens.last().is_some_and(|name| name.contains('.'))
        && !rules.not_declarations.contains(&tokens[0])
}

pub fn complete(text: &str, caret: usize, rules: &StatementRules, unit: &str) -> Completed {
    let line_start = text[..caret].rfind('\n').map_or(0, |at| at + 1);
    let line_end = text[caret..].find('\n').map_or(text.len(), |at| caret + at);
    let line = text[line_start..line_end].trim_end_matches('\r');
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let body = line.trim_end();
    let range = line_start..line_start + line.len();
    if body.trim().is_empty() {
        let text = format!("{body}\n{indent}");
        return Completed { caret: text.len(), text, range };
    }
    let scan = scan(body);
    let mut completed = format!("{body}{}", scan.closers);
    let leading = body.trim_start().trim_start_matches('}').trim_start();
    let is_comment = ["//", "#", "/*", "*"].iter().any(|prefix| leading.starts_with(prefix));
    let block =
        !is_comment && (scan.words.iter().any(|word| rules.blocks.contains(word)) || declares_method(leading, rules));
    if block {
        let inner = format!("{indent}{unit}");
        if scan.opens_block {
            let next = text[line_end..].trim_start_matches(['\r', '\n']);
            let next_line = next.lines().next().unwrap_or_default();
            let next_indent = next_line.len() - next_line.trim_start().len();
            let deeper = next_indent > indent.len() && !next_line.trim().is_empty();
            let closed = next_indent == indent.len() && next_line.trim_start().starts_with('}');
            let closing = if deeper || closed { String::new() } else { format!("\n{indent}}}") };
            let text = format!("{completed}\n{inner}{closing}");
            return Completed { caret: completed.len() + 1 + inner.len(), text, range };
        }
        if let Some(open) = completed.strip_suffix("{}") {
            completed = open.trim_end().to_owned();
        }
        if let Some(keyword) = scan.words.first().filter(|word| rules.parenthesized.contains(word)) {
            let at = completed.find(keyword.as_str()).map_or(0, |at| at + keyword.len());
            let condition = completed[at..].trim();
            if !condition.is_empty() && !condition.starts_with('(') {
                completed = format!("{} ({condition})", completed[..at].trim_end());
            }
        }
        let head = format!("{completed} {{\n{inner}");
        let text = format!("{head}\n{indent}}}");
        return Completed { caret: head.len(), text, range };
    }
    let terminated = rules.terminator.is_empty()
        || is_comment
        || [rules.terminator.as_str(), "{", "}", ",", ":"].iter().any(|end| completed.ends_with(end));
    if !terminated {
        completed.push_str(&rules.terminator);
    }
    if completed != body {
        return Completed { caret: completed.len(), text: completed, range };
    }
    let text = format!("{completed}\n{indent}");
    Completed { caret: text.len(), text, range }
}

/// The parameters of `editor.statement.complete`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CompleteParams {
    /// The file, open in the editor; the active one when absent.
    pub path: Option<PathBuf>,
}

pub struct CompleteStatement;

impl Command for CompleteStatement {
    type Params = CompleteParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.statement.complete";
    const TITLE: &'static str = "Complete Current Statement";
    const DESCRIPTION: &'static str = "Finishes the statement at the primary caret as the file's language declares it \
         (⇧⌘⏎): closes its brackets and string, adds the block of an `if`, a loop or a declaration with the caret in \
         it, or the language's terminator such as `;`; a statement that is complete gets a new line below.";

    fn is_available(&self, session: &Session) -> bool {
        session
            .state::<crate::Workbench>()
            .and_then(crate::Workbench::active_document)
            .is_some_and(|document| document.image().is_none())
    }

    fn run(&self, session: &mut Session, params: CompleteParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document_at(session, params.path.as_deref())?;
        let path = document.path().to_owned();
        let rules = session
            .state::<LiveTemplates>()
            .and_then(|templates| templates.statements_for(&code_style::language_of(&path)))
            .unwrap_or_default();
        let unit = session
            .state::<crate::Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .map_or_else(|| "    ".to_owned(), |document| project_settings::indent_unit_of(session, document));
        let document = document_at(session, Some(&path))?;
        document.template = None;
        let text = document.text().to_owned();
        let caret = byte_offset(&text, document.selections().primary().head);
        let completed = complete(&text, caret, &rules, &unit);
        let range = char_offset(&text, completed.range.start)..char_offset(&text, completed.range.end);
        let at =
            range.start + document.with_line_separator(completed.text[..completed.caret].to_owned()).chars().count();
        if document.replace_ranges(vec![(range, completed.text)]) {
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(at)));
        }
        Ok(crate::editing::describe_selections(document))
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CompleteStatement)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust() -> StatementRules {
        StatementRules {
            language: "rust".to_owned(),
            terminator: ";".to_owned(),
            blocks: ["if", "else", "for", "while", "loop", "match", "fn", "struct", "impl"].map(str::to_owned).to_vec(),
            ..StatementRules::default()
        }
    }

    fn run(text: &str, rules: &StatementRules) -> String {
        let caret = text.find('|').unwrap();
        let text = text.replace('|', "");
        let completed = complete(&text, caret, rules, "    ");
        let mut out = text.clone();
        let insert = format!("{}|{}", &completed.text[..completed.caret], &completed.text[completed.caret..]);
        out.replace_range(completed.range, &insert);
        out
    }

    #[test]
    fn brackets_close_and_the_terminator_follows() {
        assert_eq!(run("    let x = f(a, [1|\n", &rust()), "    let x = f(a, [1]);|\n");
        assert_eq!(run("    print!(\"a|\n", &rust()), "    print!(\"a\");|\n");
        assert_eq!(run("    x.go();|\nnext\n", &rust()), "    x.go();\n    |\nnext\n", "complete: a new line");
        assert_eq!(run("    // a note|\n", &rust()), "    // a note\n    |\n", "a comment gets no terminator");
    }

    #[test]
    fn a_block_statement_gets_its_block_with_the_caret_inside() {
        assert_eq!(run("    if ready(x|\n", &rust()), "    if ready(x) {\n        |\n    }\n");
        assert_eq!(run("pub(crate) fn f() -> u8|", &rust()), "pub(crate) fn f() -> u8 {\n    |\n}");
        assert_eq!(run("    } else|\n", &rust()), "    } else {\n        |\n    }\n");
        assert_eq!(run("match x {|\n}\n", &rust()), "match x {\n    |\n}\n", "an open block gets a line");
        assert_eq!(run("fn f() {|\n    a();\n}\n", &rust()), "fn f() {\n    |\n    a();\n}\n");
        assert_eq!(run("let x = if a|", &rust()), "let x = if a;|", "an assignment is a statement");
        let java = StatementRules {
            language: "java".to_owned(),
            terminator: ";".to_owned(),
            blocks: vec!["if".to_owned()],
            parenthesized: vec!["if".to_owned()],
            methods: true,
            not_declarations: ["return", "throw", "new"].map(str::to_owned).to_vec(),
        };
        assert_eq!(run("if x > 0|", &java), "if (x > 0) {\n    |\n}");
        assert_eq!(run("if (ok(|", &java), "if (ok()) {\n    |\n}");
        assert_eq!(
            run("    public Map<String, Integer> counts(int x|", &java),
            "    public Map<String, Integer> counts(int x) {\n        |\n    }"
        );
        assert_eq!(run("    return count(x|", &java), "    return count(x);|");
        assert_eq!(run("    int n = count(x|", &java), "    int n = count(x);|");
        assert_eq!(run("    list.add(x|", &java), "    list.add(x);|");
        let go = StatementRules { blocks: vec!["for".to_owned()], ..StatementRules::default() };
        assert_eq!(run("\tfor i := 0; i < n; i++|", &go), "\tfor i := 0; i < n; i++ {\n\t    |\n\t}");
        assert_eq!(run("\tfmt.Println(x|", &go), "\tfmt.Println(x)|", "no terminator in Go");
    }
}

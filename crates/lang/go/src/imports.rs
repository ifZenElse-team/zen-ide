use std::ops::Range;

use crate::{Token, tokenize};

struct Spec {
    start: usize,
    end: usize,
    path: String,
}

fn unquote(literal: &str) -> String {
    literal.trim_matches(|c| c == '"' || c == '`').to_owned()
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |newline| newline + 1)
}

fn blank(text: &str) -> bool {
    text.chars().all(|c| c == ' ' || c == '\t')
}

pub(crate) fn imported_paths(text: &str) -> Vec<String> {
    let tokens: Vec<(Token, usize, usize)> =
        tokenize(text).into_iter().filter(|(token, ..)| !matches!(token, Token::Whitespace | Token::Comment)).collect();
    let word = |index: usize| tokens.get(index).map(|&(_, start, end)| &text[start..end]);
    let mut paths = Vec::new();
    let mut index = 2;
    while word(index) == Some("import") {
        index += 1;
        if word(index) == Some("(") {
            while let Some(&(token, from, to)) = tokens.get(index).filter(|token| token.0 != Token::Punct(')')) {
                if token == Token::String {
                    paths.push(unquote(&text[from..to]));
                }
                index += 1;
            }
            index += 1;
        } else {
            while let Some(&(token, from, to)) = tokens.get(index) {
                index += 1;
                if token == Token::String {
                    paths.push(unquote(&text[from..to]));
                    break;
                }
            }
        }
        if word(index) == Some(";") {
            index += 1;
        }
    }
    paths
}

pub fn add_import(text: &str, path: &str) -> Option<(Range<usize>, String)> {
    let tokens: Vec<(Token, usize, usize)> =
        tokenize(text).into_iter().filter(|(token, ..)| !matches!(token, Token::Whitespace | Token::Comment)).collect();
    let word = |index: usize| tokens.get(index).map(|&(_, start, end)| &text[start..end]);
    if word(0) != Some("package") || tokens.get(1).is_none_or(|token| token.0 != Token::Ident) {
        return None;
    }
    let package_end = tokens[1].2;
    let mut index = 2;
    let mut singles: Vec<(usize, Spec)> = Vec::new();
    let mut block: Option<(usize, usize, Vec<Spec>)> = None;
    while word(index) == Some("import") {
        let keyword = tokens[index].1;
        index += 1;
        if tokens.get(index).is_some_and(|token| token.0 == Token::Punct('(')) {
            let open = tokens[index].2;
            index += 1;
            let mut specs = Vec::new();
            let mut start = None;
            while let Some(&(token, from, to)) = tokens.get(index) {
                match token {
                    Token::Punct(')') => break,
                    Token::String => {
                        specs.push(Spec {
                            start: start.take().unwrap_or(from),
                            end: to,
                            path: unquote(&text[from..to]),
                        });
                    }
                    Token::Punct(';') => {}
                    _ => {
                        start.get_or_insert(from);
                    }
                }
                index += 1;
            }
            let close = tokens.get(index)?.1;
            index += 1;
            if specs.iter().any(|spec| spec.path == path) {
                return None;
            }
            block.get_or_insert((open, close, specs));
        } else {
            let start = tokens.get(index)?.1;
            if tokens.get(index).is_some_and(|token| token.0 != Token::String) {
                index += 1;
            }
            let &(Token::String, from, to) = tokens.get(index)? else { return None };
            index += 1;
            let spec = Spec { start, end: to, path: unquote(&text[from..to]) };
            if spec.path == path {
                return None;
            }
            singles.push((keyword, spec));
        }
        while tokens.get(index).is_some_and(|token| token.0 == Token::Punct(';')) {
            index += 1;
        }
    }
    let line = format!("\t\"{path}\"\n");
    if let Some((open, close, specs)) = block {
        if let Some(spec) = specs.iter().find(|spec| spec.path.as_str() > path) {
            let at = line_start(text, spec.start);
            if at >= open && blank(&text[at..spec.start]) {
                return Some((at..at, line));
            }
        }
        let at = line_start(text, close);
        return Some(if at > open && blank(&text[at..close]) {
            (at..at, line)
        } else {
            (close..close, format!("\n{line}"))
        });
    }
    if let [(keyword, spec)] = singles.as_slice() {
        let existing = &text[spec.start..spec.end];
        let new = format!("\"{path}\"");
        let (first, second) =
            if spec.path.as_str() < path { (existing, new.as_str()) } else { (new.as_str(), existing) };
        return Some((*keyword..spec.end, format!("import (\n\t{first}\n\t{second}\n)")));
    }
    if let Some((keyword, _)) = singles.first() {
        let at = line_start(text, *keyword);
        return Some((at..at, format!("import \"{path}\"\n")));
    }
    let end = text[package_end..].find('\n').map_or(text.len(), |newline| package_end + newline);
    Some((end..end, format!("\n\nimport \"{path}\"")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imported(text: &str, path: &str) -> Option<String> {
        let (range, insert) = add_import(text, path)?;
        let mut text = text.to_owned();
        text.replace_range(range, &insert);
        Some(text)
    }

    #[test]
    fn a_file_without_imports_gets_a_declaration_after_the_package_clause() {
        assert_eq!(
            imported("package main // app\n\nfunc main() {}\n", "fmt").unwrap(),
            "package main // app\n\nimport \"fmt\"\n\nfunc main() {}\n"
        );
        assert_eq!(imported("package x", "fmt").unwrap(), "package x\n\nimport \"fmt\"");
        assert_eq!(imported("func main() {}\n", "fmt"), None, "no package clause");
    }

    #[test]
    fn a_block_gets_the_import_in_order() {
        let text = "package main\n\nimport (\n\t\"errors\"\n\tstr \"strings\"\n)\n";
        assert_eq!(
            imported(text, "fmt").unwrap(),
            "package main\n\nimport (\n\t\"errors\"\n\t\"fmt\"\n\tstr \"strings\"\n)\n"
        );
        assert_eq!(
            imported(text, "testing").unwrap(),
            "package main\n\nimport (\n\t\"errors\"\n\tstr \"strings\"\n\t\"testing\"\n)\n"
        );
        assert_eq!(imported(text, "strings"), None, "imported already, with a name");
        assert_eq!(imported("package main\nimport ()\n", "fmt").unwrap(), "package main\nimport (\n\t\"fmt\"\n)\n");
    }

    #[test]
    fn a_single_import_becomes_a_block() {
        assert_eq!(
            imported("package main\n\nimport \"os\"\n\nfunc main() {}\n", "fmt").unwrap(),
            "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n\nfunc main() {}\n"
        );
        assert_eq!(
            imported("package a\nimport f \"fmt\"\nimport \"os\"\n", "errors").unwrap(),
            "package a\nimport \"errors\"\nimport f \"fmt\"\nimport \"os\"\n"
        );
        assert_eq!(imported("package a\nimport \"fmt\"\n// import \"x\"\n", "fmt"), None);
    }
}

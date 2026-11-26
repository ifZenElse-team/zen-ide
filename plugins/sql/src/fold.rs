use crate::dialect::Dialect;
use crate::lexer::TokenKind;
use crate::script::Script;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

pub fn fold(text: &str, dialect: Dialect) -> Vec<Fold> {
    let script = Script::read(text, dialect);
    let mut lines: Vec<(usize, usize)> = Vec::new();
    for statement in &script.statements {
        let span = script.span(statement);
        lines.push((script.line(span.start), script.line(span.end.saturating_sub(1))));
    }
    for block in &script.blocks {
        lines.push((script.line(script.tokens[block.open].start), script.line(script.tokens[block.close].start)));
    }
    let mut brackets = Vec::new();
    let mut regions = Vec::new();
    let mut run: Option<(usize, usize)> = None;
    for token in &script.tokens {
        let first = script.line(token.start);
        let last = script.line(token.end.saturating_sub(1).max(token.start));
        if token.kind == TokenKind::LineComment {
            match region(token.text(text)) {
                Some(true) => regions.push(first),
                Some(false) => {
                    if let Some(start) = regions.pop() {
                        lines.push((start, first));
                    }
                }
                None => {
                    run = match run {
                        Some((start, end)) if end + 1 == first => Some((start, first)),
                        _ => {
                            lines.extend(run);
                            Some((first, first))
                        }
                    };
                    continue;
                }
            }
        }
        lines.extend(run.take());
        match token.kind {
            TokenKind::BlockComment | TokenKind::String | TokenKind::Quoted => lines.push((first, last)),
            TokenKind::Symbol if token.is_symbol(text, '(') => brackets.push(first),
            TokenKind::Symbol if token.is_symbol(text, ')') => {
                if let Some(open) = brackets.pop() {
                    lines.push((open, first));
                }
            }
            TokenKind::Delimiter | TokenKind::Directive => brackets.clear(),
            _ => {}
        }
    }
    lines.extend(run);
    let mut folds: Vec<Fold> = lines
        .into_iter()
        .filter(|(start, end)| end > start)
        .map(|(start, end)| Fold { start_line: start as u32, end_line: end as u32 })
        .collect();
    folds.sort_by_key(|fold| (fold.start_line, std::cmp::Reverse(fold.end_line)));
    folds.dedup();
    folds
}

fn region(comment: &str) -> Option<bool> {
    let body = comment.trim_start_matches(['-', '#']).trim_start();
    let word: String = body.chars().take_while(char::is_ascii_alphabetic).collect::<String>().to_ascii_lowercase();
    match word.as_str() {
        "region" => Some(true),
        "endregion" => Some(false),
        _ => None,
    }
}

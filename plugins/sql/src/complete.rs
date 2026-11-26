use crate::dialect::Dialect;
use crate::lexer::{Token, TokenKind, tokens};
use crate::words::{FOLLOWERS, FUNCTIONS, KEYWORDS, PHRASES, STARTERS, TYPES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub from: usize,
    pub items: Vec<Suggestion>,
}

pub const MAX_ITEMS: usize = 2_000;

pub fn complete(text: &str, caret: usize, dialect: Dialect) -> Option<Completion> {
    if caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let before = &text[..caret];
    let found = tokens(before, dialect);
    let (prefix, earlier) = match found.split_last() {
        Some((last, earlier)) if last.end == caret => match last.kind {
            TokenKind::Word => (last.text(before), earlier),
            TokenKind::Symbol | TokenKind::Delimiter | TokenKind::Directive => ("", &found[..]),
            _ => return None,
        },
        _ => ("", &found[..]),
    };
    let from = caret - prefix.len();
    if from > 0 && matches!(before.as_bytes()[from - 1], b'.' | b':' | b'@' | b'$') && !before[..from].ends_with("::") {
        return None;
    }
    let statement = current_statement(earlier);
    let previous = statement.iter().rev().find(|token| !token.is_comment());
    let after_cast = previous.is_some_and(|token| token.is_symbol(before, ':') && before[..token.end].ends_with("::"));
    let wants_type = after_cast || wants_type(before, statement);
    let follows: &[&str] = previous
        .filter(|token| token.kind == TokenKind::Word)
        .and_then(|token| FOLLOWERS.iter().find(|(word, _)| token.is(before, word)))
        .map_or(&[], |(_, followers)| followers);
    let starts = statement.iter().all(Token::is_comment);
    let lower = !prefix.is_empty() && !prefix.bytes().any(|b| b.is_ascii_uppercase());
    let bit = dialect.bit();
    let matches = |word: &str| word.len() >= prefix.len() && word[..prefix.len()].eq_ignore_ascii_case(prefix);
    let mut ranked: Vec<(u8, usize, Suggestion)> = Vec::new();
    let mut push = |rank: u8, word: &str, kind: &'static str, insert: String| {
        let order = follows.iter().position(|follower| *follower == word).unwrap_or(usize::MAX);
        let rank = if order != usize::MAX { 0 } else { rank };
        ranked.push((rank, order, Suggestion { label: cased(word, lower), kind, insert }));
    };
    let keywords = KEYWORDS.iter().chain(PHRASES).filter(|(word, mask)| mask & bit != 0 && matches(word));
    for (word, _) in keywords {
        let rank = if starts && STARTERS.contains(word) {
            1
        } else if starts {
            4
        } else {
            3
        };
        push(rank, word, "keyword", cased(word, lower));
    }
    for (word, _) in TYPES.iter().filter(|(word, mask)| mask & bit != 0 && matches(word)) {
        push(if wants_type { 2 } else { 5 }, word, "type", cased(word, lower));
    }
    if !starts && !after_cast {
        for (word, _) in FUNCTIONS.iter().filter(|(word, mask)| mask & bit != 0 && matches(word)) {
            push(6, word, "function", format!("{word}($0)"));
        }
    }
    ranked.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then_with(|| a.2.label.cmp(&b.2.label)));
    ranked.dedup_by(|a, b| a.2.label == b.2.label && a.2.kind == b.2.kind);
    let items: Vec<Suggestion> = ranked.into_iter().take(MAX_ITEMS).map(|(_, _, item)| item).collect();
    (!items.is_empty()).then_some(Completion { from, items })
}

fn cased(word: &str, lower: bool) -> String {
    if lower && !word.bytes().any(|b| b.is_ascii_lowercase()) { word.to_ascii_lowercase() } else { word.to_owned() }
}

fn current_statement(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .rposition(|token| matches!(token.kind, TokenKind::Delimiter | TokenKind::Directive))
        .map_or(0, |at| at + 1);
    &tokens[start..]
}

fn wants_type(text: &str, statement: &[Token]) -> bool {
    let significant: Vec<&Token> = statement.iter().filter(|token| !token.is_comment()).collect();
    let Some((last, earlier)) = significant.split_last() else { return false };
    if last.is(text, "AS") {
        let mut depth = 0usize;
        for (at, token) in earlier.iter().enumerate().rev() {
            if token.is_symbol(text, ')') {
                depth += 1;
            } else if token.is_symbol(text, '(') {
                if depth == 0 {
                    return at > 0 && earlier[at - 1].is(text, "CAST");
                }
                depth -= 1;
            }
        }
        return false;
    }
    let defines = significant.first().is_some_and(|first| first.is(text, "CREATE") || first.is(text, "ALTER"));
    let open = significant.iter().fold(0i32, |depth, token| {
        depth + i32::from(token.is_symbol(text, '(')) - i32::from(token.is_symbol(text, ')'))
    });
    let named = matches!(last.kind, TokenKind::Word | TokenKind::Quoted)
        && !KEYWORDS.iter().any(|(word, _)| last.is(text, word));
    let column_start = earlier.last().is_some_and(|token| token.is_symbol(text, '(') || token.is_symbol(text, ','));
    let names_before = earlier.last().is_some_and(|token| {
        matches!(token.kind, TokenKind::Word | TokenKind::Quoted)
            && !KEYWORDS.iter().any(|(word, _)| token.is(text, word))
    });
    defines && ((open > 0 && named && column_start) || (last.is(text, "TYPE") && names_before))
}

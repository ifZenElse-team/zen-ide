use std::iter::Peekable;
use std::str::Chars;

use zen_text::search::Query;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    All,
    Text(String),
    And(Vec<Need>),
    Or(Vec<Need>),
}

pub(crate) fn need(query: &Query) -> Need {
    if !query.regex {
        return Need::Text(query.text.clone());
    }
    let mut chars = query.text.chars().peekable();
    match alternatives(&mut chars) {
        Some(found) if chars.next().is_none() => requirement(&Node::Group(found)),
        _ => Need::All,
    }
}

enum Node {
    Char(char),
    Other,
    Group(Vec<Vec<Node>>),
    Repeat(Box<Node>, bool),
}

fn alternatives(chars: &mut Peekable<Chars<'_>>) -> Option<Vec<Vec<Node>>> {
    let mut found = vec![Vec::new()];
    while let Some(&c) = chars.peek() {
        match c {
            ')' => break,
            '|' => {
                chars.next();
                found.push(Vec::new());
            }
            _ => {
                let atom = atom(chars)?;
                let atom = quantified(chars, atom)?;
                found.last_mut()?.push(atom);
            }
        }
    }
    Some(found)
}

fn atom(chars: &mut Peekable<Chars<'_>>) -> Option<Node> {
    let c = chars.next()?;
    Some(match c {
        '.' | '^' | '$' => Node::Other,
        '[' => {
            class(chars)?;
            Node::Other
        }
        '(' => {
            if chars.peek() == Some(&'?') {
                chars.next();
                match chars.next()? {
                    ':' => {}
                    'P' if chars.next()? == '<' => skip_to(chars, '>')?,
                    '<' => skip_to(chars, '>')?,
                    _ => return None,
                }
            }
            let inner = alternatives(chars)?;
            (chars.next()? == ')').then_some(())?;
            Node::Group(inner)
        }
        '\\' => escape(chars)?,
        '*' | '+' | '?' | '{' | ')' => return None,
        c => Node::Char(c),
    })
}

fn escape(chars: &mut Peekable<Chars<'_>>) -> Option<Node> {
    let c = chars.next()?;
    if matches!(c, '<' | '>') {
        return Some(Node::Other);
    }
    if !c.is_alphanumeric() {
        return Some(Node::Char(c));
    }
    let fixed = match c {
        'n' => return Some(Node::Char('\n')),
        't' => return Some(Node::Char('\t')),
        'r' => return Some(Node::Char('\r')),
        'x' => 2,
        'u' => 4,
        'U' => 8,
        'p' | 'P' => 1,
        _ => 0,
    };
    if chars.peek() == Some(&'{') {
        skip_to(chars, '}')?;
    } else {
        for _ in 0..fixed {
            chars.next()?;
        }
    }
    Some(Node::Other)
}

fn class(chars: &mut Peekable<Chars<'_>>) -> Option<()> {
    let mut depth = 1;
    let mut first = true;
    if chars.peek() == Some(&'^') {
        chars.next();
    }
    while depth > 0 {
        let c = chars.next()?;
        match c {
            '\\' => {
                chars.next()?;
            }
            '[' => {
                if chars.peek() == Some(&':') {
                    skip_to(chars, ']')?;
                } else {
                    depth += 1;
                    if chars.peek() == Some(&'^') {
                        chars.next();
                    }
                    first = true;
                    continue;
                }
            }
            ']' if !first => depth -= 1,
            _ => {}
        }
        first = false;
    }
    Some(())
}

fn skip_to(chars: &mut Peekable<Chars<'_>>, end: char) -> Option<()> {
    loop {
        if chars.next()? == end {
            return Some(());
        }
    }
}

fn quantified(chars: &mut Peekable<Chars<'_>>, atom: Node) -> Option<Node> {
    let at_least_one = match chars.peek() {
        Some('*' | '?') => {
            chars.next();
            false
        }
        Some('+') => {
            chars.next();
            true
        }
        Some('{') => {
            chars.next();
            let mut digits = String::new();
            loop {
                match chars.next()? {
                    '}' => break,
                    c => digits.push(c),
                }
            }
            let low = digits.split(',').next()?.trim();
            low.parse::<usize>().ok()? > 0
        }
        _ => return Some(atom),
    };
    if chars.peek() == Some(&'?') {
        chars.next();
    }
    if matches!(chars.peek(), Some('*' | '+' | '?' | '{')) {
        return None;
    }
    Some(Node::Repeat(Box::new(atom), at_least_one))
}

fn requirement(node: &Node) -> Need {
    match node {
        Node::Char(c) => Need::Text(c.to_string()),
        Node::Other | Node::Repeat(_, false) => Need::All,
        Node::Repeat(inner, true) => requirement(inner),
        Node::Group(found) if found.len() == 1 => sequence(&found[0]),
        Node::Group(found) => {
            let branches: Vec<Need> = found.iter().map(|branch| sequence(branch)).collect();
            if branches.contains(&Need::All) { Need::All } else { Need::Or(branches) }
        }
    }
}

fn sequence(nodes: &[Node]) -> Need {
    let mut needs = Vec::new();
    let mut run = String::new();
    walk_sequence(nodes, &mut run, &mut needs);
    flush(&mut run, &mut needs);
    match needs.len() {
        0 => Need::All,
        1 => needs.remove(0),
        _ => Need::And(needs),
    }
}

fn walk_sequence(nodes: &[Node], run: &mut String, needs: &mut Vec<Need>) {
    for node in nodes {
        match node {
            Node::Char(c) => run.push(*c),
            Node::Group(found) if found.len() == 1 => walk_sequence(&found[0], run, needs),
            Node::Repeat(inner, true) => match inner.as_ref() {
                Node::Char(c) => {
                    run.push(*c);
                    flush(run, needs);
                    run.push(*c);
                }
                inner => {
                    flush(run, needs);
                    push(needs, requirement(inner));
                }
            },
            other => {
                flush(run, needs);
                push(needs, requirement(other));
            }
        }
    }
}

fn flush(run: &mut String, needs: &mut Vec<Need>) {
    if !run.is_empty() {
        needs.push(Need::Text(std::mem::take(run)));
    }
}

fn push(needs: &mut Vec<Need>, need: Need) {
    if need != Need::All {
        needs.push(need);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(text: &str) -> Need {
        need(&Query { text: text.to_owned(), regex: true, ..Query::default() })
    }

    fn text(text: &str) -> Need {
        Need::Text(text.to_owned())
    }

    #[test]
    fn a_plain_query_needs_its_whole_text() {
        assert_eq!(need(&Query { text: "a.b(c".to_owned(), ..Query::default() }), text("a.b(c"));
    }

    #[test]
    fn a_regular_expression_needs_its_literal_runs() {
        assert_eq!(regex("func \\w+Handler"), Need::And(vec![text("func "), text("Handler")]));
        assert_eq!(regex("foo(bar)baz"), text("foobarbaz"));
        assert_eq!(regex("(?:foo|bar)Test"), Need::And(vec![Need::Or(vec![text("foo"), text("bar")]), text("Test")]));
        assert_eq!(regex("ab+c"), Need::And(vec![text("ab"), text("bc")]));
        assert_eq!(regex("colou?r"), Need::And(vec![text("colo"), text("r")]));
        assert_eq!(regex("a\\.b\\x41c"), Need::And(vec![text("a.b"), text("c")]));
        assert_eq!(regex("x[]a-z]{2,}yz"), Need::And(vec![text("x"), text("yz")]));
        assert_eq!(regex("(?P<name>abc)d"), text("abcd"));
        assert_eq!(regex("\\p{Lu}abc\\pLdef"), Need::And(vec![text("abc"), text("def")]));
    }

    #[test]
    fn what_cannot_be_read_needs_nothing() {
        assert_eq!(regex(".*"), Need::All);
        assert_eq!(regex("foo|.*"), Need::All);
        assert_eq!(regex("(?i)foo"), Need::All);
        assert_eq!(regex("(foo)?bar*"), text("ba"));
        assert_eq!(regex("a{0,3}"), Need::All);
        assert_eq!(regex("[unclosed"), Need::All);
        assert_eq!(regex("\\<foo\\>"), text("foo"));
        assert_eq!(regex("\\b{start}foo"), text("foo"));
    }
}

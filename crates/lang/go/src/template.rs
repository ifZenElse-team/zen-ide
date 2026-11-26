use std::ops::Range;

use zen_syntax::{HighlightKind, Highlighter, Span};

const KEYWORDS: &[&str] = &["if", "else", "end", "range", "with", "define", "template", "block", "break", "continue"];

const CONSTANTS: &[&str] = &["true", "false", "nil"];

#[derive(Clone, Copy, Debug, Default)]
pub struct GoTemplateHighlighter;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Html {
    Text,
    Comment,
    Tag,
    Quote(char),
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_end(text: &str, from: usize) -> usize {
    text[from..].find(|c: char| !is_word(c)).map_or(text.len(), |at| from + at)
}

struct Scanner<'a> {
    text: &'a str,
    html: bool,
    state: Html,
    spans: Vec<Span>,
}

impl Scanner<'_> {
    fn span(&mut self, range: Range<usize>, kind: HighlightKind) {
        if !range.is_empty() {
            self.spans.push(Span { range, kind });
        }
    }

    fn close_of(&self, from: usize) -> Option<usize> {
        let mut quote = None;
        let mut chars = self.text[from..].char_indices();
        while let Some((offset, c)) = chars.next() {
            match quote {
                None if matches!(c, '"' | '`' | '\'') => quote = Some(c),
                None if self.text[from + offset..].starts_with("}}") => return Some(from + offset),
                None => {}
                Some(open) if c == open => quote = None,
                Some(open) if c == '\\' && open != '`' => {
                    chars.next();
                }
                Some(_) => {}
            }
        }
        None
    }

    fn action(&mut self, open: usize) -> usize {
        let mut start = open + 2;
        let rest = &self.text[start..];
        if rest.starts_with('-') && rest[1..].starts_with(char::is_whitespace) {
            start += 1;
        }
        self.span(open..start, HighlightKind::Punctuation);
        let inner = start + self.text[start..].len() - self.text[start..].trim_start().len();
        if self.text[inner..].starts_with("/*") {
            let Some(found) = self.text[inner..].find("*/") else {
                self.span(inner..self.text.len(), HighlightKind::Comment);
                return self.text.len();
            };
            let after = inner + found + 2;
            self.span(inner..after, HighlightKind::Comment);
            let close = self.text[after..].find("}}").map_or(self.text.len(), |at| after + at);
            let trim = self.text[after..close].trim_end().strip_suffix('-').map(|kept| after + kept.len());
            let end = (close + 2).min(self.text.len());
            self.span(trim.unwrap_or(close)..end, HighlightKind::Punctuation);
            return end;
        }
        let Some(close) = self.close_of(start) else {
            self.pipeline(start, self.text.len());
            return self.text.len();
        };
        let body = self.text[start..close].trim_end();
        let trimmed = body.strip_suffix('-').filter(|kept| kept.ends_with(char::is_whitespace));
        let body_end = start + trimmed.unwrap_or(body).len();
        self.pipeline(start, body_end);
        let from = if trimmed.is_some() { start + body.len() - 1 } else { close };
        self.span(from..close + 2, HighlightKind::Punctuation);
        close + 2
    }

    fn quoted_end(&self, at: usize, quote: char, end: usize) -> usize {
        let mut chars = self.text[at + 1..end].char_indices();
        while let Some((offset, c)) = chars.next() {
            if c == '\\' && quote != '`' {
                chars.next();
            } else if c == quote {
                return at + 1 + offset + 1;
            }
        }
        end
    }

    fn pipeline(&mut self, from: usize, end: usize) {
        let mut at = from;
        while at < end {
            let Some(c) = self.text[at..end].chars().next() else { break };
            let next = at + c.len_utf8();
            if c.is_whitespace() {
                at = next;
            } else if matches!(c, '"' | '`' | '\'') {
                let close = self.quoted_end(at, c, end);
                let kind = if c == '\'' { HighlightKind::Constant } else { HighlightKind::String };
                self.span(at..close, kind);
                at = close;
            } else if c.is_ascii_digit() {
                let close = self.text[at..end]
                    .find(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_')))
                    .map_or(end, |offset| at + offset);
                self.span(at..close, HighlightKind::Number);
                at = close;
            } else if c == '$' {
                let close = word_end(self.text, next).min(end);
                self.span(at..close, HighlightKind::GlobalVariable);
                at = close;
            } else if c == '.' && self.text[next..end].starts_with(is_word) {
                let close = word_end(self.text, next).min(end);
                self.span(at..close, HighlightKind::Property);
                at = close;
            } else if is_word(c) {
                let close = word_end(self.text, at).min(end);
                let word = &self.text[at..close];
                let kind = if KEYWORDS.contains(&word) {
                    HighlightKind::Keyword
                } else if CONSTANTS.contains(&word) {
                    HighlightKind::Constant
                } else {
                    HighlightKind::Function
                };
                self.span(at..close, kind);
                at = close;
            } else {
                self.span(at..next, HighlightKind::Punctuation);
                at = next;
            }
        }
    }

    fn markup(&mut self, from: usize, end: usize) {
        let mut at = from;
        while at < end {
            match self.state {
                Html::Text => {
                    let Some(open) = self.text[at..end].find('<').map(|offset| at + offset) else { return };
                    if self.text[open..end].starts_with("<!--") {
                        self.state = Html::Comment;
                        at = open;
                        continue;
                    }
                    let name = open + 1 + usize::from(self.text[open + 1..end].starts_with('/'));
                    if !self.text[name..end].starts_with(|c: char| c.is_alphabetic() || c == '!') {
                        at = open + 1;
                        continue;
                    }
                    let close = self.text[name + 1..end]
                        .find(|c: char| !(is_word(c) || matches!(c, '-' | ':')))
                        .map_or(end, |offset| name + 1 + offset);
                    self.span(open..close, HighlightKind::Keyword);
                    self.state = Html::Tag;
                    at = close;
                }
                Html::Comment => {
                    let close = self.text[at..end].find("-->").map(|offset| at + offset + 3);
                    self.span(at..close.unwrap_or(end), HighlightKind::Comment);
                    if close.is_some() {
                        self.state = Html::Text;
                    }
                    at = close.unwrap_or(end);
                }
                Html::Quote(quote) => {
                    let close = self.text[at..end].find(quote).map(|offset| at + offset + 1);
                    self.span(at..close.unwrap_or(end), HighlightKind::String);
                    if close.is_some() {
                        self.state = Html::Tag;
                    }
                    at = close.unwrap_or(end);
                }
                Html::Tag => {
                    let Some(c) = self.text[at..end].chars().next() else { return };
                    if c == '>' {
                        self.span(at..at + 1, HighlightKind::Keyword);
                        self.state = Html::Text;
                        at += 1;
                    } else if matches!(c, '"' | '\'') {
                        self.span(at..at + 1, HighlightKind::String);
                        self.state = Html::Quote(c);
                        at += 1;
                    } else if is_word(c) {
                        let close = self.text[at..end]
                            .find(|c: char| !(is_word(c) || matches!(c, '-' | ':')))
                            .map_or(end, |offset| at + offset);
                        self.span(at..close, HighlightKind::Attribute);
                        at = close;
                    } else {
                        at += c.len_utf8();
                    }
                }
            }
        }
    }

    fn run(mut self) -> Vec<Span> {
        let mut at = 0;
        while at < self.text.len() {
            let open = self.text[at..].find("{{").map_or(self.text.len(), |offset| at + offset);
            if self.html {
                self.markup(at, open);
            }
            if open == self.text.len() {
                break;
            }
            at = self.action(open);
        }
        self.spans
    }
}

fn looks_like_html(text: &str) -> bool {
    let start = text.trim_start();
    start.starts_with('<') || text.contains("</") || text.contains("/>")
}

impl Highlighter for GoTemplateHighlighter {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".tmpl") || file_name.ends_with(".gotmpl") || file_name.ends_with(".gohtml")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        Scanner { text, html: looks_like_html(text), state: Html::Text, spans: Vec::new() }.run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<(&str, HighlightKind)> {
        GoTemplateHighlighter.highlight(text).into_iter().map(|span| (&text[span.range.clone()], span.kind)).collect()
    }

    #[test]
    fn it_takes_go_template_files_only() {
        for name in ["a.tmpl", "page.html.tmpl", "a.gotmpl", "a.gohtml"] {
            assert!(GoTemplateHighlighter.handles(name), "{name}");
        }
        assert!(!GoTemplateHighlighter.handles("a.html"));
    }

    #[test]
    fn actions_are_highlighted_and_the_text_around_them_is_not() {
        use HighlightKind::{Constant, Function, GlobalVariable, Keyword, Number, Property, Punctuation, String};
        assert_eq!(
            spans("Hi {{- if and .User.Name (gt $n 1) true }} there {{ printf \"%d}}\" 12 | html -}} {{end}}"),
            [
                ("{{-", Punctuation),
                ("if", Keyword),
                ("and", Function),
                (".User", Property),
                (".Name", Property),
                ("(", Punctuation),
                ("gt", Function),
                ("$n", GlobalVariable),
                ("1", Number),
                (")", Punctuation),
                ("true", Constant),
                ("}}", Punctuation),
                ("{{", Punctuation),
                ("printf", Function),
                ("\"%d}}\"", String),
                ("12", Number),
                ("|", Punctuation),
                ("html", Function),
                ("-}}", Punctuation),
                ("{{", Punctuation),
                ("end", Keyword),
                ("}}", Punctuation),
            ]
        );
    }

    #[test]
    fn comments_and_unfinished_actions_are_read_to_their_end() {
        use HighlightKind::{Comment, Function, Punctuation};
        assert_eq!(
            spans("{{- /* a }} comment */ -}}x{{/* open"),
            [
                ("{{-", Punctuation),
                ("/* a }} comment */", Comment),
                ("-}}", Punctuation),
                ("{{", Punctuation),
                ("/* open", Comment),
            ]
        );
        assert_eq!(spans("{{ f"), [("{{", Punctuation), ("f", Function)]);
        assert_eq!(spans("{{"), [("{{", Punctuation)]);
    }

    #[test]
    fn html_around_actions_is_highlighted_and_survives_an_action_in_a_value() {
        use HighlightKind::{Attribute, Comment, Keyword, Property, Punctuation, String};
        assert_eq!(
            spans("<a href=\"x{{.URL}}y\"><!-- c --></a>"),
            [
                ("<a", Keyword),
                ("href", Attribute),
                ("\"", String),
                ("x", String),
                ("{{", Punctuation),
                (".URL", Property),
                ("}}", Punctuation),
                ("y\"", String),
                (">", Keyword),
                ("<!-- c -->", Comment),
                ("</a", Keyword),
                (">", Keyword),
            ]
        );
    }

    #[test]
    fn the_braces_of_an_action_stay_brackets_for_the_editor_to_match() {
        for text in ["a {{ if .X }} b", "{{/* c */}}", "{{- .X -}}"] {
            let spans = GoTemplateHighlighter.highlight(text);
            let hidden = |byte: usize| {
                spans.iter().any(|span| {
                    span.range.contains(&byte) && matches!(span.kind, HighlightKind::String | HighlightKind::Comment)
                })
            };
            let braces = text.char_indices().filter(|(_, c)| matches!(c, '{' | '}'));
            assert!(braces.clone().count() == 4 && !braces.clone().any(|(at, _)| hidden(at)), "{text}");
        }
    }
}

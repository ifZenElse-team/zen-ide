use crate::complete::clamp;
use crate::data::VOID;
use crate::emmet::Expansion;
use crate::lexer::{End, Tok, Token, is_raw_text, lex};
use crate::tree::build;

pub const TRIGGERS: &str = ">/";

pub fn on_type(text: &str, caret: usize, typed: char) -> Option<Expansion> {
    let caret = clamp(text, caret);
    let prefix = &text[..caret];
    match typed {
        '>' if prefix.ends_with('>') => end_tag(text, caret),
        '/' if prefix.ends_with("</") => open_name(text, caret),
        _ => None,
    }
}

fn insert(caret: usize, text: String) -> Option<Expansion> {
    Some(Expansion { from: caret, to: caret, text })
}

fn end_tag(text: &str, caret: usize) -> Option<Expansion> {
    let (tokens, _) = lex(&text[..caret]);
    let last = tokens.last().filter(|token| token.kind == Tok::Gt && token.end == caret)?;
    let lt =
        tokens.iter().rposition(|token| matches!(token.kind, Tok::Lt | Tok::LtSlash) && token.start < last.start)?;
    if tokens[lt].kind != Tok::Lt {
        return None;
    }
    let name = tokens.get(lt + 1).filter(|token| token.kind == Tok::TagName)?;
    let name = &text[name.start..name.end];
    if name.is_empty() || VOID.iter().any(|void| void.eq_ignore_ascii_case(name)) {
        return None;
    }
    let (all, _) = lex(text);
    let (before, after): (Vec<Token>, Vec<Token>) = all.into_iter().partition(|token| token.end <= caret);
    let open = balance(text, &before, name);
    let closed_after = -balance(text, &after, name);
    if closed_after >= open {
        return None;
    }
    insert(caret, format!("$0</{name}>"))
}

fn balance(text: &str, tokens: &[Token], name: &str) -> i64 {
    let mut balance = 0i64;
    for (at, token) in tokens.iter().enumerate() {
        let opening = match token.kind {
            Tok::Lt => true,
            Tok::LtSlash => false,
            _ => continue,
        };
        let Some(tag) = tokens.get(at + 1).filter(|next| next.kind == Tok::TagName) else { continue };
        if !text[tag.start..tag.end].eq_ignore_ascii_case(name) {
            continue;
        }
        if !opening {
            balance -= 1;
            continue;
        }
        let ends =
            tokens[at + 2..].iter().find(|next| matches!(next.kind, Tok::Gt | Tok::SlashGt | Tok::Lt | Tok::LtSlash));
        if ends.is_none_or(|end| end.kind != Tok::SlashGt) {
            balance += 1;
        }
    }
    balance
}

fn open_name(text: &str, caret: usize) -> Option<Expansion> {
    if text[caret..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '>' || c == '-') {
        return None;
    }
    let prefix = &text[..caret];
    let (tokens, end) = lex(prefix);
    if end == End::Opaque {
        return raw_element(prefix, &tokens).and_then(|name| insert(caret, format!("{name}>")));
    }
    let End::TagName { lt, closing: true } = end else { return None };
    let before: Vec<Token> = tokens.into_iter().filter(|token| token.end <= lt).collect();
    let tree = build(prefix, &before);
    let open = tree.open.last()?;
    insert(caret, format!("{}>", tree.elements[*open].name))
}

fn raw_element<'a>(text: &'a str, tokens: &[Token]) -> Option<&'a str> {
    let lt = tokens.iter().rposition(|token| token.kind == Tok::Lt)?;
    let name = tokens.get(lt + 1).filter(|token| token.kind == Tok::TagName)?;
    let name = &text[name.start..name.end];
    let gt = lt + tokens[lt..].iter().position(|token| token.kind == Tok::Gt)?;
    let content = &tokens[gt + 1..];
    let only_text = content.iter().all(|token| matches!(token.kind, Tok::RawText | Tok::Text | Tok::Entity));
    (is_raw_text(name) && only_text && content.last().is_some_and(|token| token.end == text.len())).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str, c: char) -> Option<String> {
        let caret = text.find('|').expect("a caret");
        let source = text.replacen('|', "", 1);
        let added = on_type(&source, caret, c)?;
        assert_eq!((added.from, added.to), (caret, caret));
        Some(format!("{}{}{}", &source[..caret], added.text, &source[caret..]))
    }

    #[test]
    fn closes_a_start_tag() {
        assert_eq!(typed("<div>|", '>').as_deref(), Some("<div>$0</div>"));
        assert_eq!(typed("<p class=\"a>b\" id=x>|\n", '>').as_deref(), Some("<p class=\"a>b\" id=x>$0</p>\n"));
        assert_eq!(typed("<Custom-El>|", '>').as_deref(), Some("<Custom-El>$0</Custom-El>"));
        assert_eq!(typed("<script>|", '>').as_deref(), Some("<script>$0</script>"));
        assert_eq!(
            typed("<div>\n  <div>|\n</div>", '>').as_deref(),
            Some("<div>\n  <div>$0</div>\n</div>"),
            "the end tag after it belongs to the outer one"
        );
    }

    #[test]
    fn leaves_closed_and_empty_elements_alone() {
        assert_eq!(typed("<div>|</div>", '>'), None);
        assert_eq!(typed("<div class=\"x\">|\n  text\n</div>", '>'), None);
        assert_eq!(typed("<br>|", '>'), None);
        assert_eq!(typed("<IMG src=a>|", '>'), None);
        assert_eq!(typed("<a/>|", '>'), None);
        assert_eq!(typed("</div>|", '>'), None);
        assert_eq!(typed("<!-- <div>| -->", '>'), None);
        assert_eq!(typed("a >|", '>'), None);
        assert_eq!(typed("<a title=\">|\">", '>'), None);
        assert_eq!(typed("<div>|", 'x'), None);
    }

    #[test]
    fn handles_the_tricky_start_tags() {
        assert_eq!(typed("<a href=\"x\">|", '>').as_deref(), Some("<a href=\"x\">$0</a>"));
        assert_eq!(typed("<p title='a>b'>|", '>').as_deref(), Some("<p title='a>b'>$0</p>"));
        assert_eq!(typed("<p title=\"a>b\" data-x='>'>|", '>').as_deref(), Some("<p title=\"a>b\" data-x='>'>$0</p>"));
        assert_eq!(typed("<br>|", '>'), None);
        assert_eq!(typed("<img/>|", '>'), None);
        assert_eq!(typed("<img src=\"a.png\" />|", '>'), None);
        assert_eq!(typed("<!-- note -->|", '>'), None);
        assert_eq!(typed("<!-- <p> -->\n<div>|", '>').as_deref(), Some("<!-- <p> -->\n<div>$0</div>"));
        assert_eq!(typed("<!DOCTYPE html>|", '>'), None);
        assert_eq!(typed("<script>let s = \"<b>|\";</script>", '>'), None, "raw text of a script");
        assert_eq!(typed("<script>if (a<b>|) {}</script>", '>'), None);
        assert_eq!(typed("<style>a >|</style>", '>'), None);
        assert_eq!(typed("<ul>|\n  <li>a</li>\n</ul>", '>'), None, "closed further on");
        assert_eq!(typed("<div>|<div></div>", '>').as_deref(), Some("<div>$0</div><div></div>"));
        assert_eq!(typed("<script>x</|", '/').as_deref(), Some("<script>x</script>"));
        assert_eq!(typed("<script>if (a</|", '/').as_deref(), Some("<script>if (a</script>"));
    }

    #[test]
    fn completes_an_end_tag() {
        assert_eq!(typed("<ul><li>a</|", '/').as_deref(), Some("<ul><li>a</li>"));
        assert_eq!(typed("<ul><li>a</li></|\n", '/').as_deref(), Some("<ul><li>a</li></ul>\n"));
        assert_eq!(typed("<p>x <br> y</|", '/').as_deref(), Some("<p>x <br> y</p>"));
        assert_eq!(typed("text </|", '/'), None, "nothing is open");
        assert_eq!(typed("<div></|div>", '/'), None, "the name is there already");
        assert_eq!(typed("<div>a/|", '/'), None);
    }

    #[test]
    fn survives_any_caret() {
        let text = "<div a=\"<\">é<ul><li></li></ul><br/></";
        for caret in 0..=text.len() + 1 {
            for c in ['>', '/'] {
                if let Some(added) = on_type(text, caret, c) {
                    assert!(added.from == added.to && added.to <= text.len());
                }
            }
        }
    }
}

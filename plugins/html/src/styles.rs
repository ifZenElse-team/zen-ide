use crate::color::{self, Color, Syntax};
use crate::lexer::{Tok, lex};

pub(crate) fn colors(text: &str) -> Vec<Color> {
    let (tokens, _) = lex(text);
    let mut found = Vec::new();
    let mut element = "";
    for (at, token) in tokens.iter().enumerate() {
        match token.kind {
            Tok::TagName if at > 0 && tokens[at - 1].kind == Tok::Lt => element = &text[token.start..token.end],
            Tok::RawText if element.eq_ignore_ascii_case("style") => {
                let syntax = Syntax { scss: false, in_block: false };
                color::colors(text, token.start..token.end, syntax, &mut found);
            }
            Tok::AttrValue
                if at >= 2
                    && tokens[at - 1].kind == Tok::Eq
                    && tokens[at - 2].kind == Tok::AttrName
                    && text[tokens[at - 2].start..tokens[at - 2].end].eq_ignore_ascii_case("style") =>
            {
                let value = &text[token.start..token.end];
                let quoted = value.starts_with(['"', '\'']);
                let start = token.start + usize::from(quoted);
                let end =
                    if quoted && value.len() > 1 && value.ends_with(&value[..1]) { token.end - 1 } else { token.end };
                let syntax = Syntax { scss: false, in_block: true };
                color::colors(text, start..end.max(start), syntax, &mut found);
            }
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(&str, u32)> {
        colors(text).iter().map(|color| (&text[color.start..color.end], color.rgba)).collect()
    }

    #[test]
    fn finds_colours_in_styles_only() {
        let page = "<p style=\"color: #f00; background: rgba(0, 0, 255, .5)\" class=\"red\">red</p>\n<style>\n  a { color: navy }\n  #fff { }\n</style>\n<div style='border-color: tan'>";
        assert_eq!(
            found(page),
            [("#f00", 0xff0000ff), ("rgba(0, 0, 255, .5)", 0x0000ff80), ("navy", 0x000080ff), ("tan", 0xd2b48cff)]
        );
        assert_eq!(found("<script>a { color: red }</script><b style=color:lime>"), [("lime", 0x00ff00ff)]);
        assert_eq!(found("<i style=\"color: red"), [("red", 0xff0000ff)]);
    }
}

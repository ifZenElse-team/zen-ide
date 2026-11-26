use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Color {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) rgba: u32,
}

const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ffff),
    ("antiquewhite", 0xfaebd7ff),
    ("aqua", 0x00ffffff),
    ("aquamarine", 0x7fffd4ff),
    ("azure", 0xf0ffffff),
    ("beige", 0xf5f5dcff),
    ("bisque", 0xffe4c4ff),
    ("black", 0x000000ff),
    ("blanchedalmond", 0xffebcdff),
    ("blue", 0x0000ffff),
    ("blueviolet", 0x8a2be2ff),
    ("brown", 0xa52a2aff),
    ("burlywood", 0xdeb887ff),
    ("cadetblue", 0x5f9ea0ff),
    ("chartreuse", 0x7fff00ff),
    ("chocolate", 0xd2691eff),
    ("coral", 0xff7f50ff),
    ("cornflowerblue", 0x6495edff),
    ("cornsilk", 0xfff8dcff),
    ("crimson", 0xdc143cff),
    ("cyan", 0x00ffffff),
    ("darkblue", 0x00008bff),
    ("darkcyan", 0x008b8bff),
    ("darkgoldenrod", 0xb8860bff),
    ("darkgray", 0xa9a9a9ff),
    ("darkgreen", 0x006400ff),
    ("darkgrey", 0xa9a9a9ff),
    ("darkkhaki", 0xbdb76bff),
    ("darkmagenta", 0x8b008bff),
    ("darkolivegreen", 0x556b2fff),
    ("darkorange", 0xff8c00ff),
    ("darkorchid", 0x9932ccff),
    ("darkred", 0x8b0000ff),
    ("darksalmon", 0xe9967aff),
    ("darkseagreen", 0x8fbc8fff),
    ("darkslateblue", 0x483d8bff),
    ("darkslategray", 0x2f4f4fff),
    ("darkslategrey", 0x2f4f4fff),
    ("darkturquoise", 0x00ced1ff),
    ("darkviolet", 0x9400d3ff),
    ("deeppink", 0xff1493ff),
    ("deepskyblue", 0x00bfffff),
    ("dimgray", 0x696969ff),
    ("dimgrey", 0x696969ff),
    ("dodgerblue", 0x1e90ffff),
    ("firebrick", 0xb22222ff),
    ("floralwhite", 0xfffaf0ff),
    ("forestgreen", 0x228b22ff),
    ("fuchsia", 0xff00ffff),
    ("gainsboro", 0xdcdcdcff),
    ("ghostwhite", 0xf8f8ffff),
    ("gold", 0xffd700ff),
    ("goldenrod", 0xdaa520ff),
    ("gray", 0x808080ff),
    ("green", 0x008000ff),
    ("greenyellow", 0xadff2fff),
    ("grey", 0x808080ff),
    ("honeydew", 0xf0fff0ff),
    ("hotpink", 0xff69b4ff),
    ("indianred", 0xcd5c5cff),
    ("indigo", 0x4b0082ff),
    ("ivory", 0xfffff0ff),
    ("khaki", 0xf0e68cff),
    ("lavender", 0xe6e6faff),
    ("lavenderblush", 0xfff0f5ff),
    ("lawngreen", 0x7cfc00ff),
    ("lemonchiffon", 0xfffacdff),
    ("lightblue", 0xadd8e6ff),
    ("lightcoral", 0xf08080ff),
    ("lightcyan", 0xe0ffffff),
    ("lightgoldenrodyellow", 0xfafad2ff),
    ("lightgray", 0xd3d3d3ff),
    ("lightgreen", 0x90ee90ff),
    ("lightgrey", 0xd3d3d3ff),
    ("lightpink", 0xffb6c1ff),
    ("lightsalmon", 0xffa07aff),
    ("lightseagreen", 0x20b2aaff),
    ("lightskyblue", 0x87cefaff),
    ("lightslategray", 0x778899ff),
    ("lightslategrey", 0x778899ff),
    ("lightsteelblue", 0xb0c4deff),
    ("lightyellow", 0xffffe0ff),
    ("lime", 0x00ff00ff),
    ("limegreen", 0x32cd32ff),
    ("linen", 0xfaf0e6ff),
    ("magenta", 0xff00ffff),
    ("maroon", 0x800000ff),
    ("mediumaquamarine", 0x66cdaaff),
    ("mediumblue", 0x0000cdff),
    ("mediumorchid", 0xba55d3ff),
    ("mediumpurple", 0x9370dbff),
    ("mediumseagreen", 0x3cb371ff),
    ("mediumslateblue", 0x7b68eeff),
    ("mediumspringgreen", 0x00fa9aff),
    ("mediumturquoise", 0x48d1ccff),
    ("mediumvioletred", 0xc71585ff),
    ("midnightblue", 0x191970ff),
    ("mintcream", 0xf5fffaff),
    ("mistyrose", 0xffe4e1ff),
    ("moccasin", 0xffe4b5ff),
    ("navajowhite", 0xffdeadff),
    ("navy", 0x000080ff),
    ("oldlace", 0xfdf5e6ff),
    ("olive", 0x808000ff),
    ("olivedrab", 0x6b8e23ff),
    ("orange", 0xffa500ff),
    ("orangered", 0xff4500ff),
    ("orchid", 0xda70d6ff),
    ("palegoldenrod", 0xeee8aaff),
    ("palegreen", 0x98fb98ff),
    ("paleturquoise", 0xafeeeeff),
    ("palevioletred", 0xdb7093ff),
    ("papayawhip", 0xffefd5ff),
    ("peachpuff", 0xffdab9ff),
    ("peru", 0xcd853fff),
    ("pink", 0xffc0cbff),
    ("plum", 0xdda0ddff),
    ("powderblue", 0xb0e0e6ff),
    ("purple", 0x800080ff),
    ("rebeccapurple", 0x663399ff),
    ("red", 0xff0000ff),
    ("rosybrown", 0xbc8f8fff),
    ("royalblue", 0x4169e1ff),
    ("saddlebrown", 0x8b4513ff),
    ("salmon", 0xfa8072ff),
    ("sandybrown", 0xf4a460ff),
    ("seagreen", 0x2e8b57ff),
    ("seashell", 0xfff5eeff),
    ("sienna", 0xa0522dff),
    ("silver", 0xc0c0c0ff),
    ("skyblue", 0x87ceebff),
    ("slateblue", 0x6a5acdff),
    ("slategray", 0x708090ff),
    ("slategrey", 0x708090ff),
    ("snow", 0xfffafaff),
    ("springgreen", 0x00ff7fff),
    ("steelblue", 0x4682b4ff),
    ("tan", 0xd2b48cff),
    ("teal", 0x008080ff),
    ("thistle", 0xd8bfd8ff),
    ("tomato", 0xff6347ff),
    ("transparent", 0x00000000),
    ("turquoise", 0x40e0d0ff),
    ("violet", 0xee82eeff),
    ("wheat", 0xf5deb3ff),
    ("white", 0xffffffff),
    ("whitesmoke", 0xf5f5f5ff),
    ("yellow", 0xffff00ff),
    ("yellowgreen", 0x9acd32ff),
];

const MAX_COLORS: usize = 100_000;

pub(crate) fn named(word: &str) -> Option<u32> {
    if word.len() > 20 {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    NAMED.binary_search_by(|(name, _)| name.cmp(&lower.as_str())).ok().map(|at| NAMED[at].1)
}

#[cfg(test)]
pub(crate) fn parse(literal: &str) -> Option<u32> {
    let literal = literal.trim();
    if let Some(hex) = literal.strip_prefix('#') {
        return parse_hex(hex);
    }
    let Some(open) = literal.find('(') else { return named(literal) };
    let arguments = literal[open + 1..].strip_suffix(')')?;
    function(&literal[..open].to_ascii_lowercase(), arguments)
}

fn parse_hex(hex: &str) -> Option<u32> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 | 4 => {
            let mut value = 0u32;
            for at in 0..4 {
                let d = if at < hex.len() { hex_value(hex, at)? } else { 0xF };
                value = (value << 8) | (d * 17);
            }
            Some(value)
        }
        6 => u32::from_str_radix(hex, 16).ok().map(|value| (value << 8) | 0xFF),
        8 => u32::from_str_radix(hex, 16).ok(),
        _ => None,
    }
}

fn hex_value(hex: &str, at: usize) -> Option<u32> {
    (hex.as_bytes()[at] as char).to_digit(16)
}

fn function(name: &str, arguments: &str) -> Option<u32> {
    let rgb = matches!(name, "rgb" | "rgba");
    if !rgb && !matches!(name, "hsl" | "hsla") {
        return None;
    }
    let parts: Vec<&str> = if arguments.contains(',') {
        arguments.split(',').map(str::trim).collect()
    } else {
        let (channels, alpha) = match arguments.split_once('/') {
            Some((channels, alpha)) => (channels, Some(alpha.trim())),
            None => (arguments, None),
        };
        let mut parts: Vec<&str> = channels.split_ascii_whitespace().collect();
        if parts.len() != 3 {
            return None;
        }
        parts.extend(alpha);
        parts
    };
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let alpha = match parts.get(3) {
        Some(alpha) => unit(alpha)?,
        None => 1.0,
    };
    let (r, g, b) = if rgb {
        (channel(parts[0])?, channel(parts[1])?, channel(parts[2])?)
    } else {
        let hue = hue(parts[0])?;
        let saturation = fraction(parts[1])?;
        let lightness = fraction(parts[2])?;
        hsl_to_rgb(hue, saturation, lightness)
    };
    let byte = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    Some((byte(r) << 24) | (byte(g) << 16) | (byte(b) << 8) | byte(alpha))
}

fn number(text: &str) -> Option<f64> {
    if text.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    let valid = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'))
        && text.bytes().any(|b| b.is_ascii_digit());
    valid.then(|| text.parse::<f64>().ok()).flatten().filter(|value| value.is_finite())
}

fn channel(text: &str) -> Option<f64> {
    match text.strip_suffix('%') {
        Some(percent) => number(percent).map(|value| value / 100.0),
        None => number(text).map(|value| value / 255.0),
    }
}

fn unit(text: &str) -> Option<f64> {
    match text.strip_suffix('%') {
        Some(percent) => number(percent).map(|value| value / 100.0),
        None => number(text),
    }
}

fn fraction(text: &str) -> Option<f64> {
    text.strip_suffix('%').map_or_else(|| number(text), number).map(|value| (value / 100.0).clamp(0.0, 1.0))
}

fn hue(text: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let (value, scale) = [("deg", 1.0), ("grad", 0.9), ("rad", 180.0 / std::f64::consts::PI), ("turn", 360.0)]
        .iter()
        .find_map(|(suffix, scale)| lower.strip_suffix(suffix).map(|value| (value.to_owned(), *scale)))
        .unwrap_or((lower.clone(), 1.0));
    number(&value).map(|value| (value * scale).rem_euclid(360.0))
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (f64, f64, f64) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let segment = hue / 60.0;
    let second = chroma * (1.0 - (segment.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match segment as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let lift = lightness - chroma / 2.0;
    (r + lift, g + lift, b + lift)
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

pub(crate) fn encode(colors: &[Color]) -> Vec<u8> {
    let mut out = Vec::with_capacity(colors.len() * 12);
    for color in colors {
        for value in [color.start as u32, color.end as u32, color.rgba] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Syntax {
    pub(crate) scss: bool,
    pub(crate) in_block: bool,
}

pub(crate) fn colors(text: &str, range: Range<usize>, syntax: Syntax, out: &mut Vec<Color>) {
    let bytes = text.as_bytes();
    let end = range.end.min(bytes.len());
    let mut at = range.start.min(end);
    let mut depth = usize::from(syntax.in_block);
    while at < end && out.len() < MAX_COLORS {
        at = skip_blank(bytes, at, end, syntax);
        if at >= end {
            break;
        }
        match bytes[at] {
            b'{' => {
                depth += 1;
                at += 1;
                continue;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                at += 1;
                continue;
            }
            b';' => {
                at += 1;
                continue;
            }
            _ => {}
        }
        let (stop, colon) = statement(bytes, at, end, syntax);
        let ends_block = stop < end && bytes[stop] == b'{';
        if !ends_block {
            if bytes[at] == b'@' {
                let keyword = bytes[at + 1..stop].iter().take_while(|b| is_name_byte(**b)).count();
                let name = &text[at + 1..at + 1 + keyword];
                if syntax.scss && matches!(name, "include" | "return") {
                    value(text, at + 1 + keyword..stop, syntax, out);
                }
            } else if let Some(colon) = colon
                && (depth > 0 || syntax.scss)
            {
                value(text, colon + 1..stop, syntax, out);
            }
        }
        at = stop;
    }
}

fn skip_blank(bytes: &[u8], mut at: usize, end: usize, syntax: Syntax) -> usize {
    while at < end {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
        } else if let Some(after) = comment_end(bytes, at, end, syntax) {
            at = after;
        } else {
            break;
        }
    }
    at
}

fn comment_end(bytes: &[u8], at: usize, end: usize, syntax: Syntax) -> Option<usize> {
    if bytes.get(at) != Some(&b'/') || at + 1 >= end {
        return None;
    }
    match bytes[at + 1] {
        b'*' => Some(find(bytes, at + 2, end, b"*/").map_or(end, |close| close + 2)),
        b'/' if syntax.scss => Some(bytes[at..end].iter().position(|b| *b == b'\n').map_or(end, |line| at + line)),
        _ => None,
    }
}

fn find(bytes: &[u8], from: usize, end: usize, needle: &[u8]) -> Option<usize> {
    bytes[from.min(end)..end].windows(needle.len()).position(|window| window == needle).map(|at| from + at)
}

fn string_end(bytes: &[u8], at: usize, end: usize) -> usize {
    let quote = bytes[at];
    let mut i = at + 1;
    while i < end {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return i,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    end
}

fn interpolation_end(bytes: &[u8], at: usize, end: usize) -> usize {
    let mut depth = 0usize;
    let mut i = at + 1;
    while i < end {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            b'"' | b'\'' => {
                i = string_end(bytes, i, end);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    end
}

fn statement(bytes: &[u8], from: usize, end: usize, syntax: Syntax) -> (usize, Option<usize>) {
    let mut colon = None;
    let mut parens = 0usize;
    let mut at = from;
    while at < end {
        if let Some(after) = comment_end(bytes, at, end, syntax) {
            at = after;
            continue;
        }
        match bytes[at] {
            b'"' | b'\'' => {
                at = string_end(bytes, at, end);
                continue;
            }
            b'#' if syntax.scss && bytes.get(at + 1) == Some(&b'{') => {
                at = interpolation_end(bytes, at, end);
                continue;
            }
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b':' if colon.is_none() && parens == 0 => colon = Some(at),
            b'{' | b'}' | b';' if parens == 0 || bytes[at] != b';' => return (at, colon),
            _ => {}
        }
        at += 1;
    }
    (end, colon)
}

fn value(text: &str, range: Range<usize>, syntax: Syntax, out: &mut Vec<Color>) {
    let bytes = text.as_bytes();
    let end = range.end;
    let mut at = range.start;
    while at < end && out.len() < MAX_COLORS {
        if let Some(after) = comment_end(bytes, at, end, syntax) {
            at = after;
            continue;
        }
        let previous = at.checked_sub(1).map(|i| bytes[i]);
        let free = previous.is_none_or(|b| !is_name_byte(b) && !matches!(b, b'$' | b'@' | b'.' | b'%' | b'#' | b'\\'));
        match bytes[at] {
            b'"' | b'\'' => {
                at = string_end(bytes, at, end);
                continue;
            }
            b'#' if syntax.scss && bytes.get(at + 1) == Some(&b'{') => {
                at = interpolation_end(bytes, at, end);
                continue;
            }
            b'#' if free => {
                let digits = bytes[at + 1..end].iter().take_while(|b| is_name_byte(**b)).count();
                let literal = &text[at..at + 1 + digits];
                if let Some(rgba) = parse_hex(&literal[1..]) {
                    out.push(Color { start: at, end: at + 1 + digits, rgba });
                }
                at += 1 + digits;
                continue;
            }
            b if b.is_ascii_alphabetic() && free => {
                let word = bytes[at..end].iter().take_while(|b| is_name_byte(**b)).count();
                let name = &text[at..at + word];
                let after = at + word;
                if bytes.get(after) == Some(&b'(') && after < end {
                    let close = matching(bytes, after, end);
                    let lower = name.to_ascii_lowercase();
                    if lower == "url" {
                        at = close;
                        continue;
                    }
                    if close <= end
                        && bytes[close - 1] == b')'
                        && let Some(rgba) = function(&lower, &text[after + 1..close - 1])
                    {
                        out.push(Color { start: at, end: close, rgba });
                        at = close;
                        continue;
                    }
                    at = after + 1;
                    continue;
                }
                if let Some(rgba) = named(name) {
                    out.push(Color { start: at, end: after, rgba });
                }
                at = after;
                continue;
            }
            b if is_name_byte(b) || b == b'\\' => {
                at += 1 + usize::from(b == b'\\' && at + 1 < end);
                while at < end && is_name_byte(bytes[at]) {
                    at += 1;
                }
                while at < end && !text.is_char_boundary(at) {
                    at += 1;
                }
                continue;
            }
            _ => {}
        }
        at += 1;
        while at < end && !text.is_char_boundary(at) {
            at += 1;
        }
    }
}

fn matching(bytes: &[u8], open: usize, end: usize) -> usize {
    let mut depth = 0usize;
    let mut at = open;
    while at < end {
        match bytes[at] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return at + 1;
                }
            }
            b'"' | b'\'' => {
                at = string_end(bytes, at, end);
                continue;
            }
            b';' | b'{' | b'}' => return at,
            _ => {}
        }
        at += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: Syntax = Syntax { scss: false, in_block: false };

    fn found(text: &str, syntax: Syntax) -> Vec<(&str, u32)> {
        let mut out = Vec::new();
        colors(text, 0..text.len(), syntax, &mut out);
        out.iter().map(|color| (&text[color.start..color.end], color.rgba)).collect()
    }

    #[test]
    fn parses_literals() {
        assert_eq!(parse("#f80"), Some(0xff8800ff));
        assert_eq!(parse("#F808"), Some(0xff880088));
        assert_eq!(parse("#ff8000"), Some(0xff8000ff));
        assert_eq!(parse("#ff800080"), Some(0xff800080));
        assert_eq!(parse("#ff80"), Some(0xffff8800));
        assert_eq!(parse("#ff8"), Some(0xffff88ff));
        assert_eq!(parse("#ff80000"), None);
        assert_eq!(parse("#ggg"), None);
        assert_eq!(parse("RebeccaPurple"), Some(0x663399ff));
        assert_eq!(parse("transparent"), Some(0));
        assert_eq!(parse("currentcolor"), None);
        assert_eq!(parse("rgb(255, 128, 0)"), Some(0xff8000ff));
        assert_eq!(parse("rgba(255,128,0,.5)"), Some(0xff800080));
        assert_eq!(parse("rgb(100% 50% 0% / 25%)"), Some(0xff800040));
        assert_eq!(parse("RGB(300 -5 none)"), Some(0xff0000ff));
        assert_eq!(parse("hsl(120, 100%, 50%)"), Some(0x00ff00ff));
        assert_eq!(parse("hsla(0.5turn 100% 25% / 0.5)"), Some(0x00808080));
        assert_eq!(parse("hsl(240deg 100 50)"), Some(0x0000ffff));
        assert_eq!(parse("hsl(-120, 100%, 50%)"), Some(0x0000ffff));
        assert_eq!(parse("rgb(1, 2)"), None);
        assert_eq!(parse("rgb(var(--r), 0, 0)"), None);
        assert_eq!(parse("rgb(1 2 3 4)"), None);
        assert_eq!(parse("lab(50% 0 0)"), None);
    }

    #[test]
    fn the_names_are_sorted() {
        assert!(NAMED.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(NAMED.len(), 149);
    }

    #[test]
    fn finds_colours_in_values_only() {
        let text = "#fff, .red, a:hover { color: red; background: #0f0 url(red.png); border: 1px solid rgb(0 0 255 / 50%) }\n@media (color: red) { b { --x: hsl(0, 100%, 50%); content: \"red\"; c: darkred-ish } }";
        assert_eq!(
            found(text, SHEET),
            [
                ("red", 0xff0000ff),
                ("#0f0", 0x00ff00ff),
                ("rgb(0 0 255 / 50%)", 0x0000ff80),
                ("hsl(0, 100%, 50%)", 0xff0000ff)
            ]
        );
        assert_eq!(found("/* red */ a { /* blue */ color: /* green */ tan }", SHEET), [("tan", 0xd2b48cff)]);
        assert_eq!(
            found("a { b { color: navy } &:hover { color: teal } }", SHEET),
            [("navy", 0x000080ff), ("teal", 0x008080ff)]
        );
        assert_eq!(found("a { color: rgb(calc(1) 0 0) blue }", SHEET), [("blue", 0x0000ffff)]);
        assert_eq!(found("a { grid-area: red-zone; x: --red; y: $red }", SHEET), []);
    }

    #[test]
    fn reads_declarations_and_scss() {
        let style = Syntax { scss: false, in_block: true };
        assert_eq!(found("color: #000; background-color: white", style), [("#000", 0x000000ff), ("white", 0xffffffff)]);
        let scss = Syntax { scss: true, in_block: false };
        let text = "// red\n$accent: #f00;\n.a-#{$x} { color: darken($accent, 10%); // blue\n  @include tone(rgba(0, 0, 0, .5)); &:hover { fill: lime } }";
        assert_eq!(found(text, scss), [("#f00", 0xff0000ff), ("rgba(0, 0, 0, .5)", 0x00000080), ("lime", 0x00ff00ff)]);
        assert_eq!(found("// a: red", SHEET), [], "no declaration outside a block in CSS");
    }

    #[test]
    fn survives_any_input() {
        let pieces =
            ["a", "{", "}", ":", ";", "#", "#f", "(", ")", "rgb(", "'", "\"", "/*", "//", "#{", "é", "\\", "red", " "];
        for syntax in [SHEET, Syntax { scss: true, in_block: true }] {
            for a in pieces {
                for b in pieces {
                    for c in pieces {
                        for d in pieces {
                            let text = format!("{a}{b}{c}{d}");
                            let mut out = Vec::new();
                            colors(&text, 0..text.len(), syntax, &mut out);
                            let mut previous = 0;
                            for color in out {
                                assert!(color.start >= previous && color.end > color.start, "{text:?}");
                                assert!(text.is_char_boundary(color.start) && text.is_char_boundary(color.end));
                                previous = color.end;
                            }
                        }
                    }
                }
            }
        }
    }
}

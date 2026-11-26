use zen_theme::Rgba;

pub(crate) fn parse(text: &str) -> Option<Rgba> {
    let digits = text.trim();
    let digits = digits.strip_prefix('#').unwrap_or(digits);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let digits = match digits.len() {
        1..=6 => format!("{digits:0>6}"),
        8 => digits.to_owned(),
        _ => return None,
    };
    format!("#{digits}").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_values_are_left_padded() {
        assert_eq!(parse("0"), Some(Rgba::rgb(0, 0, 0)));
        assert_eq!(parse("ff"), Some(Rgba::rgb(0, 0, 0xFF)));
        assert_eq!(parse("12345"), Some(Rgba::rgb(0x01, 0x23, 0x45)));
        assert_eq!(parse("#a1b2c3"), Some(Rgba::rgb(0xA1, 0xB2, 0xC3)));
        assert_eq!(parse("A1B2C380"), Some(Rgba::new(0xA1, 0xB2, 0xC3, 0x80)));
    }

    #[test]
    fn malformed_values_are_rejected() {
        for text in ["", "#", "xyz", "1234567", "123456789", "12 34"] {
            assert_eq!(parse(text), None, "{text}");
        }
    }
}

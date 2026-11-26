use zen_core::keymap::Keystroke;

const KEYS: &[(&str, &str)] = &[
    ("ENTER", "Enter"),
    ("BACK_SPACE", "Backspace"),
    ("TAB", "Tab"),
    ("ESCAPE", "Escape"),
    ("SPACE", "Space"),
    ("DELETE", "Delete"),
    ("INSERT", "Insert"),
    ("HOME", "Home"),
    ("END", "End"),
    ("PAGE_UP", "PageUp"),
    ("PAGE_DOWN", "PageDown"),
    ("LEFT", "ArrowLeft"),
    ("RIGHT", "ArrowRight"),
    ("UP", "ArrowUp"),
    ("DOWN", "ArrowDown"),
    ("SLASH", "Slash"),
    ("BACK_SLASH", "Backslash"),
    ("COMMA", "Comma"),
    ("PERIOD", "Period"),
    ("MINUS", "Minus"),
    ("EQUALS", "Equals"),
    ("PLUS", "Plus"),
    ("OPEN_BRACKET", "OpenBracket"),
    ("CLOSE_BRACKET", "CloseBracket"),
    ("SEMICOLON", "Semicolon"),
    ("QUOTE", "Quote"),
    ("BACK_QUOTE", "Backtick"),
    ("MULTIPLY", "NumpadMultiply"),
    ("ADD", "Plus"),
    ("SUBTRACT", "Minus"),
];

fn key_name(token: &str) -> Option<String> {
    if let Some((_, ours)) = KEYS.iter().find(|(java, _)| *java == token) {
        return Some((*ours).to_owned());
    }
    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), None) if letter.is_ascii_uppercase() => return Some(letter.to_string()),
        (Some(digit), None) if digit.is_ascii_digit() => return Some(format!("Num{digit}")),
        _ => {}
    }
    token
        .strip_prefix('F')
        .filter(|number| number.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|number| number.parse::<u8>().ok())
        .filter(|number| (1..=24).contains(number) && !token.starts_with("F0"))
        .map(|number| format!("F{number}"))
}

pub(crate) fn parse(text: &str) -> Option<Keystroke> {
    let mut stroke = Keystroke::default();
    let mut key = None;
    for token in text.split_whitespace() {
        if key.is_some() {
            return None;
        }
        match token {
            "shift" => stroke.shift = true,
            "control" | "ctrl" => stroke.control = true,
            "meta" => stroke.command = true,
            "alt" => stroke.alt = true,
            "pressed" => {}
            other => key = Some(key_name(other)?),
        }
    }
    stroke.key = key?;
    (Keystroke::parse(&stroke.to_string()).as_ref() == Some(&stroke)).then_some(stroke)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_java_key_name_becomes_a_keystroke_that_reads_back() {
        let mut names: Vec<String> = KEYS.iter().map(|(java, _)| (*java).to_owned()).collect();
        names.extend(('A'..='Z').map(String::from));
        names.extend(('0'..='9').map(String::from));
        names.extend((1..=24).map(|number| format!("F{number}")));
        for name in names {
            let stroke = parse(&format!("meta {name}")).unwrap_or_else(|| panic!("{name}"));
            assert!(stroke.command, "{name}");
            assert_eq!(Keystroke::parse(&stroke.to_string()), Some(stroke.clone()), "{name}");
        }
    }

    #[test]
    fn modifiers_and_keys_map_as_on_macos() {
        let text = |java: &str| parse(java).map(|stroke| stroke.to_string());
        assert_eq!(text("meta shift F").as_deref(), Some("⇧⌘F"));
        assert_eq!(text("control alt L").as_deref(), Some("⌃⌥L"));
        assert_eq!(text("ctrl SPACE").as_deref(), Some("⌃Space"));
        assert_eq!(text("meta alt SLASH").as_deref(), Some("⌥⌘/"));
        assert_eq!(text("meta OPEN_BRACKET").as_deref(), Some("⌘["));
        assert_eq!(text("meta BACK_SPACE").as_deref(), Some("⌘⌫"));
        assert_eq!(text("alt ENTER").as_deref(), Some("⌥⏎"));
        assert_eq!(text("shift F6").as_deref(), Some("⇧F6"));
        assert_eq!(text("meta 1").as_deref(), Some("⌘1"));
        assert_eq!(text("alt shift UP").as_deref(), Some("⌥⇧↑"));
        assert_eq!(text("shift PAGE_DOWN").as_deref(), Some("⇧PageDown"));
        assert_eq!(text("meta pressed D").as_deref(), Some("⌘D"));
        assert_eq!(text("F24").as_deref(), Some("F24"));
        assert_eq!(text("meta MULTIPLY").as_deref(), Some("⌘NumPad*"));
        assert_eq!(text("meta alt ADD").as_deref(), Some("⌥⌘+"));
        assert_eq!(text("control shift SUBTRACT").as_deref(), Some("⌃⇧−"));
    }

    #[test]
    fn keys_that_cannot_be_named_are_refused() {
        for text in [
            "",
            "meta",
            "shift",
            "meta F25",
            "meta F0",
            "meta F+1",
            "meta NUMPAD5",
            "altGraph A",
            "typed a",
            "released A",
            "meta a",
            "meta A B",
            "button1 A",
            "meta KP_LEFT",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }
}

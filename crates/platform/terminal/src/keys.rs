#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Modifiers {
    fn parameter(self) -> Option<u8> {
        let value = u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl);
        (value != 0).then_some(value + 1)
    }
}

pub fn encode(key: Key, modifiers: Modifiers, app_cursor: bool) -> Vec<u8> {
    let alt_prefix = |bytes: &[u8]| {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if modifiers.alt {
            out.push(0x1b);
        }
        out.extend_from_slice(bytes);
        out
    };
    let letter = |final_byte: char, ss3: bool| match modifiers.parameter() {
        Some(m) => format!("\x1b[1;{m}{final_byte}").into_bytes(),
        None if ss3 => format!("\x1bO{final_byte}").into_bytes(),
        None => format!("\x1b[{final_byte}").into_bytes(),
    };
    let tilde = |number: u8| match modifiers.parameter() {
        Some(m) => format!("\x1b[{number};{m}~").into_bytes(),
        None => format!("\x1b[{number}~").into_bytes(),
    };
    match key {
        Key::Enter => alt_prefix(b"\r"),
        Key::Backspace if modifiers.ctrl => alt_prefix(&[0x08]),
        Key::Backspace => alt_prefix(&[0x7f]),
        Key::Tab if modifiers.shift => b"\x1b[Z".to_vec(),
        Key::Tab => alt_prefix(b"\t"),
        Key::Escape => alt_prefix(&[0x1b]),
        Key::Up => letter('A', app_cursor),
        Key::Down => letter('B', app_cursor),
        Key::Right => letter('C', app_cursor),
        Key::Left => letter('D', app_cursor),
        Key::Home => letter('H', app_cursor),
        Key::End => letter('F', app_cursor),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F(n @ 1..=4) => letter(char::from(b'P' + n - 1), true),
        Key::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        Key::F(_) => Vec::new(),
    }
}

pub fn control(ch: char) -> Option<u8> {
    match ch.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        out.extend(text.replace('\x1b', "").bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers { shift: false, alt: false, ctrl: false };

    #[test]
    fn cursor_keys_follow_decckm() {
        assert_eq!(encode(Key::Up, NONE, false), b"\x1b[A");
        assert_eq!(encode(Key::Up, NONE, true), b"\x1bOA");
        assert_eq!(encode(Key::Left, Modifiers { ctrl: true, ..NONE }, true), b"\x1b[1;5D");
        assert_eq!(encode(Key::End, Modifiers { shift: true, ..NONE }, false), b"\x1b[1;2F");
    }

    #[test]
    fn editing_and_function_keys() {
        assert_eq!(encode(Key::Enter, NONE, false), b"\r");
        assert_eq!(encode(Key::Backspace, NONE, false), [0x7f]);
        assert_eq!(encode(Key::Tab, Modifiers { shift: true, ..NONE }, false), b"\x1b[Z");
        assert_eq!(encode(Key::Delete, NONE, false), b"\x1b[3~");
        assert_eq!(encode(Key::PageUp, Modifiers { alt: true, ..NONE }, false), b"\x1b[5;3~");
        assert_eq!(encode(Key::F(1), NONE, false), b"\x1bOP");
        assert_eq!(encode(Key::F(12), NONE, false), b"\x1b[24~");
        assert_eq!(encode(Key::F(13), NONE, false), b"");
        assert_eq!(encode(Key::Escape, Modifiers { alt: true, ..NONE }, false), b"\x1b\x1b");
    }

    #[test]
    fn control_bytes() {
        assert_eq!(control('c'), Some(3));
        assert_eq!(control('C'), Some(3));
        assert_eq!(control('['), Some(0x1b));
        assert_eq!(control(' '), Some(0));
        assert_eq!(control('é'), None);
    }

    #[test]
    fn paste_modes() {
        assert_eq!(paste("a\nb", false), b"a\rb");
        assert_eq!(paste("a\r\nb", false), b"a\rb");
        assert_eq!(paste("x\x1b[201~y", true), b"\x1b[200~x[201~y\x1b[201~");
    }
}

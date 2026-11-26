use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use lsp_types::Position;

#[derive(Default)]
struct Mapped {
    by_path: BTreeMap<PathBuf, String>,
    by_uri: BTreeMap<String, PathBuf>,
}

static MAPPED: RwLock<Mapped> = RwLock::new(Mapped { by_path: BTreeMap::new(), by_uri: BTreeMap::new() });

pub fn map_uri(uri: &str, path: &Path) {
    let mut mapped = MAPPED.write().unwrap_or_else(PoisonError::into_inner);
    if let Some(old) = mapped.by_path.insert(path.to_owned(), uri.to_owned()) {
        mapped.by_uri.remove(&old);
    }
    if let Some(old) = mapped.by_uri.insert(uri.to_owned(), path.to_owned())
        && old != path
    {
        mapped.by_path.remove(&old);
    }
}

pub fn unmap_uri(path: &Path) {
    let mut mapped = MAPPED.write().unwrap_or_else(PoisonError::into_inner);
    if let Some(uri) = mapped.by_path.remove(path) {
        mapped.by_uri.remove(&uri);
    }
}

pub fn mapped_uri(path: &Path) -> Option<String> {
    MAPPED.read().unwrap_or_else(PoisonError::into_inner).by_path.get(path).cloned()
}

pub fn mapped_path(uri: &str) -> Option<PathBuf> {
    MAPPED.read().unwrap_or_else(PoisonError::into_inner).by_uri.get(uri).cloned()
}

pub fn scheme(uri: &str) -> Option<&str> {
    let (scheme, _) = uri.split_once("://")?;
    (!scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
        .then_some(scheme)
}

pub fn position(text: &str, offset: usize) -> Position {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let (mut line, mut line_start) = (0, 0);
    while let Some((_, next)) = line_break(text, line_start).filter(|(_, next)| *next <= offset) {
        line += 1;
        line_start = next;
    }
    let character: usize = text[line_start..offset].chars().map(char::len_utf16).sum();
    Position { line: line as u32, character: character as u32 }
}

pub fn offset(text: &str, position: Position) -> Option<usize> {
    let mut line_start = 0;
    for _ in 0..position.line {
        line_start = line_break(text, line_start)?.1;
    }
    let line_end = line_break(text, line_start).map_or(text.len(), |(end, _)| end);
    let line = &text[line_start..line_end];
    let mut units = 0;
    for (index, c) in line.char_indices() {
        if units >= position.character as usize {
            return Some(line_start + index);
        }
        units += c.len_utf16();
    }
    Some(line_end)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineStarts(Vec<usize>);

impl LineStarts {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        let mut from = 0;
        while let Some((_, next)) = line_break(text, from) {
            starts.push(next);
            from = next;
        }
        Self(starts)
    }

    pub fn offset(&self, text: &str, position: Position) -> Option<usize> {
        let line_start = *self.0.get(position.line as usize)?;
        let line_end = line_break(text, line_start).map_or(text.len(), |(end, _)| end);
        let mut units = 0;
        for (index, c) in text[line_start..line_end].char_indices() {
            if units >= position.character as usize {
                return Some(line_start + index);
            }
            units += c.len_utf16();
        }
        Some(line_end)
    }
}

fn line_break(text: &str, from: usize) -> Option<(usize, usize)> {
    let index = text[from..].find(['\n', '\r'])? + from;
    let next = if text[index..].starts_with("\r\n") { index + 2 } else { index + 1 };
    Some((index, next))
}

pub fn file_uri(path: &Path) -> String {
    if let Some(uri) = mapped_uri(path) {
        return uri;
    }
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => uri.push(byte as char),
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

pub fn uri_path(uri: &str) -> Option<PathBuf> {
    if let Some(path) = mapped_path(uri) {
        return Some(path);
    }
    let encoded = uri.strip_prefix("file://")?;
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = encoded.get(index + 1..index + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_starts_find_what_offset_finds() {
        let text = "ab\r\nxé😀y\n\nlast";
        let starts = LineStarts::new(text);
        for line in 0..6 {
            for character in 0..8 {
                let position = Position { line, character };
                assert_eq!(starts.offset(text, position), offset(text, position), "{position:?}");
            }
        }
        assert_eq!(LineStarts::new("").offset("", Position { line: 0, character: 3 }), Some(0));
    }

    #[test]
    fn utf16_positions() {
        let text = "ab\nxé😀y\n";
        assert_eq!(position(text, 0), Position { line: 0, character: 0 });
        assert_eq!(position(text, 3), Position { line: 1, character: 0 });
        let y = text.find('y').unwrap();
        assert_eq!(position(text, y), Position { line: 1, character: 4 });
        assert_eq!(offset(text, Position { line: 1, character: 4 }), Some(y));
        assert_eq!(offset(text, Position { line: 1, character: 99 }), Some(text.len() - 1), "the line's end");
        assert_eq!(offset(text, Position { line: 2, character: 0 }), Some(text.len()));
        assert_eq!(offset(text, Position { line: 5, character: 0 }), None);
        for at in 0..=text.len() {
            if text.is_char_boundary(at) {
                assert_eq!(offset(text, position(text, at)), Some(at), "{at}");
            }
        }
    }

    #[test]
    fn every_protocol_line_break_ends_a_line() {
        let text = "a\rbé\r\nc\nd";
        let d = text.find('d').unwrap();
        let c = text.find('c').unwrap();
        let b = text.find('b').unwrap();
        assert_eq!(position(text, b), Position { line: 1, character: 0 });
        assert_eq!(position(text, c), Position { line: 2, character: 0 });
        assert_eq!(position(text, d), Position { line: 3, character: 0 });
        assert_eq!(offset(text, Position { line: 1, character: 2 }), Some(b + 3));
        assert_eq!(offset(text, Position { line: 1, character: 9 }), Some(b + 3), "the line's end, before \\r\\n");
        assert_eq!(offset(text, Position { line: 3, character: 0 }), Some(d));
        assert_eq!(offset(text, Position { line: 4, character: 0 }), None);
        for at in 0..=text.len() {
            if text.is_char_boundary(at) && !(text[..at].ends_with('\r') && text[at..].starts_with('\n')) {
                assert_eq!(offset(text, position(text, at)), Some(at), "{at}");
            }
        }
    }

    #[test]
    fn file_uris() {
        let path = Path::new("/Users/me/My Go/π.go");
        let uri = file_uri(path);
        assert_eq!(uri, "file:///Users/me/My%20Go/%CF%80.go");
        assert_eq!(uri_path(&uri).as_deref(), Some(path));
        assert_eq!(uri_path("https://example.com"), None);
    }

    #[test]
    fn a_mapped_document_speaks_its_server_uri_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("String.java");
        let uri = "jdt://contents/rt.jar/java.lang/String.class?=demo";
        assert_eq!(uri_path(uri), None);
        map_uri(uri, &path);
        assert_eq!(file_uri(&path), uri);
        assert_eq!(uri_path(uri).as_deref(), Some(path.as_path()));
        let moved = dir.path().join("String2.java");
        map_uri(uri, &moved);
        assert_eq!(uri_path(uri).as_deref(), Some(moved.as_path()));
        assert!(file_uri(&path).starts_with("file://"), "the old path is a plain file again");
        assert_eq!(scheme(uri), Some("jdt"));

        let home = dir.path().join("module/zen_scratch/scratch.go");
        let scratch = dir.path().join("scratches/scratch.go");
        let plain = file_uri(&scratch);
        let mapped = file_uri(&home);
        map_uri(&mapped, &scratch);
        assert_eq!(file_uri(&scratch), mapped, "a file can live at another file's URI");
        assert_eq!(uri_path(&mapped).as_deref(), Some(scratch.as_path()));
        unmap_uri(&scratch);
        assert_eq!(file_uri(&scratch), plain);
        assert_eq!(uri_path(&mapped).as_deref(), Some(home.as_path()));
        assert_eq!(scheme("file:///a"), Some("file"));
        assert_eq!(scheme("no scheme"), None);
    }
}

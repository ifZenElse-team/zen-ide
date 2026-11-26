use std::collections::HashMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Languages, char_offset};

/// A place in a file: a range of its text with the line it is on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// The file.
    pub path: PathBuf,
    /// The first character of the range, a character offset in the file's text.
    pub start: usize,
    /// The character after the range.
    pub end: usize,
    /// The line the range starts on, one-based.
    pub line: u32,
    /// The column the range starts at, one-based, in characters.
    pub column: u32,
    /// The line's text without its leading and trailing white space.
    pub preview: Option<String>,
    /// The range within the preview, in characters, cut at the line's end.
    pub highlight: Option<[usize; 2]>,
}

#[derive(Debug, Default)]
pub struct TextCache {
    texts: HashMap<PathBuf, Option<String>>,
    indexes: HashMap<PathBuf, TextIndex>,
}

#[derive(Debug)]
struct TextIndex {
    starts: zen_lsp::position::LineStarts,
    breaks: Vec<(usize, usize)>,
}

impl TextIndex {
    fn new(text: &str) -> Self {
        let mut breaks = Vec::new();
        let mut chars = 0;
        let mut from = 0;
        for (at, _) in text.match_indices('\n') {
            chars += text[from..=at].chars().count();
            breaks.push((at, chars));
            from = at + 1;
        }
        Self { starts: zen_lsp::position::LineStarts::new(text), breaks }
    }

    fn line_of(&self, byte: usize) -> (usize, usize, usize) {
        let line = self.breaks.partition_point(|(at, _)| *at < byte);
        match line.checked_sub(1).map(|before| self.breaks[before]) {
            Some((at, chars)) => (line, at + 1, chars),
            None => (line, 0, 0),
        }
    }

    fn chars(&self, text: &str, byte: usize) -> usize {
        let (_, start, chars) = self.line_of(byte);
        chars + text[start..byte].chars().count()
    }

    fn place(&self, path: PathBuf, text: &str, range: &Value) -> Option<Place> {
        let position = |at: &Value| -> Option<lsp_types::Position> {
            Some(lsp_types::Position { line: at["line"].as_u64()? as u32, character: at["character"].as_u64()? as u32 })
        };
        let start = self.starts.offset(text, position(&range["start"])?)?;
        let end = self.starts.offset(text, position(&range["end"])?).unwrap_or(text.len()).max(start);
        let (line, line_start, chars_before) = self.line_of(start);
        let line_end = self.breaks.get(line).map_or(text.len(), |(at, _)| *at);
        let line_text = &text[line_start..line_end];
        let lead = line_text.len() - line_text.trim_start().len();
        let preview = line_text.trim();
        let from = text.get(line_start + lead..start).map_or(0, |before| before.chars().count());
        let to = from + text[start..end.min(line_end)].chars().count();
        let length = preview.chars().count();
        let column = text[line_start..start].chars().count();
        Some(Place {
            path,
            start: chars_before + column,
            end: self.chars(text, end),
            line: line as u32 + 1,
            column: column as u32 + 1,
            preview: Some(preview.to_owned()),
            highlight: Some([from.min(length), to.min(length)]),
        })
    }
}

impl TextCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&mut self, languages: &Languages, path: &Path) -> Option<&str> {
        self.texts.entry(path.to_owned()).or_insert_with(|| read(languages, path)).as_deref()
    }

    pub fn place(&mut self, languages: &Languages, path: &Path, range: &Value) -> Option<Place> {
        let text = self.texts.entry(path.to_owned()).or_insert_with(|| read(languages, path)).as_deref()?;
        let index = self.indexes.entry(path.to_owned()).or_insert_with(|| TextIndex::new(text));
        index.place(path.to_owned(), text, range)
    }
}

fn read(languages: &Languages, path: &Path) -> Option<String> {
    languages.synced_text(path).map(str::to_owned).or_else(|| zen_vfs::read_text(path).ok())
}

pub fn places(answer: &Value, languages: &Languages, cache: &mut TextCache) -> Vec<Place> {
    let items: Vec<&Value> = match answer {
        Value::Array(list) => list.iter().collect(),
        Value::Object(_) => vec![answer],
        _ => Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| {
            let (uri, range) = match item["targetUri"].as_str() {
                Some(uri) if item["targetSelectionRange"].is_object() => (uri, &item["targetSelectionRange"]),
                Some(uri) => (uri, &item["targetRange"]),
                None => (item["uri"].as_str()?, &item["range"]),
            };
            let path = zen_lsp::position::uri_path(uri)?;
            let text = cache.text(languages, &path)?;
            place(path, text, range)
        })
        .collect()
}

pub fn place(path: PathBuf, text: &str, range: &Value) -> Option<Place> {
    let position = |at: &Value| -> Option<lsp_types::Position> {
        Some(lsp_types::Position { line: at["line"].as_u64()? as u32, character: at["character"].as_u64()? as u32 })
    };
    let start = zen_lsp::position::offset(text, position(&range["start"])?)?;
    let end = zen_lsp::position::offset(text, position(&range["end"])?).unwrap_or(text.len()).max(start);
    let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = text[start..].find('\n').map_or(text.len(), |index| start + index);
    let line_text = &text[line_start..line_end];
    let lead = line_text.len() - line_text.trim_start().len();
    let preview = line_text.trim();
    let from = text.get(line_start + lead..start).map_or(0, |before| before.chars().count());
    let to = from + text[start..end.min(line_end)].chars().count();
    let length = preview.chars().count();
    Some(Place {
        path,
        start: char_offset(text, start),
        end: char_offset(text, end),
        line: text[..start].matches('\n').count() as u32 + 1,
        column: text[line_start..start].chars().count() as u32 + 1,
        preview: Some(preview.to_owned()),
        highlight: Some([from.min(length), to.min(length)]),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use zen_lsp::fake::Reply;

    use super::*;
    use crate::language::testing::fixture;
    use crate::language::{LookupKind, request_at};

    const MAIN: &str = "package main\n\n// привет\nfunc main() { s := \"ё😀\"; Println(s) }\n";
    const LIB: &str = "package main\n\n/* 😀 */ func Println(a ...any) {}\n";

    fn chars(text: &str, needle: &str) -> usize {
        text[..text.find(needle).unwrap()].chars().count()
    }

    #[test]
    fn a_request_counts_utf16_and_its_location_comes_back_in_characters() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let lib = fixture.path("lib.go");
        fixture.server.on(
            "textDocument/definition",
            json!({
                "uri": zen_lsp::position::file_uri(&lib),
                "range": { "start": { "line": 2, "character": 14 }, "end": { "line": 2, "character": 21 } }
            }),
        );
        fixture.open("main.go");
        let main = fixture.path("main.go");
        let at = chars(MAIN, "Println") + 3;
        let request = fixture
            .with(|languages, workbench| {
                request_at(languages, workbench, &main, at, "textDocument/definition", json!({ "extra": true }))
            })
            .unwrap();
        let sent = &fixture.server.wait_for("textDocument/definition", 1, Duration::from_secs(5))[0];
        let utf16 = "func main() { s := \"ё😀\"; Pri".encode_utf16().count();
        assert_eq!(sent["position"], json!({ "line": 3, "character": utf16 }));
        assert_eq!(sent["textDocument"]["uri"], zen_lsp::position::file_uri(&main));
        assert_eq!(sent["extra"], true);

        let answer = request.pending.wait(Duration::from_secs(5)).unwrap();
        let found = fixture.with(|languages, _| places(&answer, languages, &mut TextCache::new()));
        let start = chars(LIB, "Println");
        assert_eq!(
            found,
            vec![Place {
                path: lib,
                start,
                end: start + 7,
                line: 3,
                column: 14,
                preview: Some("/* 😀 */ func Println(a ...any) {}".to_owned()),
                highlight: Some([13, 20]),
            }],
            "read from disk: lib.go is not open"
        );
    }

    #[test]
    fn location_links_go_to_their_selection_range_in_the_synced_text() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.open("main.go");
        let main = fixture.path("main.go");
        let uri = zen_lsp::position::file_uri(&main);
        let utf16 = "func main() { s := \"ё😀\"; ".encode_utf16().count() as u64;
        let answer = json!([
            {
                "targetUri": uri,
                "targetRange": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 40 } },
                "targetSelectionRange": { "start": { "line": 3, "character": utf16 }, "end": { "line": 3, "character": utf16 + 7 } }
            },
            { "targetUri": uri, "targetRange": { "start": { "line": 2, "character": 3 }, "end": { "line": 2, "character": 9 } } },
            { "uri": "https://example.com/x.go", "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } } }
        ]);
        std::fs::write(&main, "changed on disk\n").unwrap();
        let found = fixture.with(|languages, _| places(&answer, languages, &mut TextCache::new()));
        assert_eq!(found.len(), 2, "{found:?}");
        let start = chars(MAIN, "Println");
        assert_eq!((found[0].start, found[0].end, found[0].line, found[0].column), (start, start + 7, 4, 26));
        assert_eq!(found[0].highlight, Some([25, 32]));
        assert_eq!(found[1].preview.as_deref(), Some("// привет"));
        assert_eq!((found[1].start, found[1].highlight), (chars(MAIN, "привет"), Some([3, 9])));
        assert!(places(&json!(null), &Languages::default(), &mut TextCache::new()).is_empty());
    }

    #[test]
    fn the_cache_finds_the_same_places_as_place_does() {
        let text = "package main\r\n\r\n// привет\nfunc main() { s := \"ё😀\"; Println(s) }\n\n\tx := 1\r\nend";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        std::fs::write(&path, text).unwrap();
        let mut cache = TextCache::new();
        let languages = Languages::default();
        for start_line in 0..8u64 {
            for start in [0u64, 1, 3, 9, 14, 21, 26, 40, 90] {
                for (end_line, end) in [(start_line, start + 4), (start_line + 1, 2), (start_line, 200), (9, 0)] {
                    let range = json!({
                        "start": { "line": start_line, "character": start },
                        "end": { "line": end_line, "character": end }
                    });
                    assert_eq!(cache.place(&languages, &path, &range), place(path.clone(), text, &range), "{range}");
                }
            }
        }
    }

    #[test]
    fn a_lookup_waits_for_its_answer_and_is_cancelled_when_replaced() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.reply("textDocument/hover", Reply::Hold);
        fixture.server.on("textDocument/implementation", json!([]));
        fixture.open("main.go");
        let main = fixture.path("main.go");
        fixture.with(|languages, workbench| {
            let lookup = languages
                .start_lookup(workbench, LookupKind::Documentation, &main, 3, "textDocument/hover", json!({}))
                .unwrap();
            assert!(!lookup.poll(Duration::from_millis(30)));
            assert!(lookup.is_pending());
            assert!(!languages.is_outdated(LookupKind::Documentation));
            languages
                .start_lookup(workbench, LookupKind::Documentation, &main, 5, "textDocument/implementation", json!({}))
                .unwrap();
            let lookup = languages.lookup_mut(LookupKind::Documentation).unwrap();
            assert!(lookup.poll(Duration::from_secs(5)));
            assert_eq!(lookup.answer, Some(Ok(json!([]))));
            assert!(languages.end_lookup(LookupKind::Documentation).is_some());
            assert!(languages.lookup(LookupKind::Documentation).is_none());
        });
        assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, Duration::from_secs(5)).len(), 1);
    }
}

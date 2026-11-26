use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::links::{LinkSpan, detect};

/// Where a console line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Stream {
    /// The program's standard output.
    Stdout,
    /// The program's standard error.
    Stderr,
    /// The IDE: the command line, the exit code.
    System,
}

/// One line of the console.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleLine {
    /// Where it came from.
    pub stream: Stream,
    /// The text, without escape sequences and control characters.
    pub text: String,
    /// The file positions in the text, as character ranges.
    pub links: Vec<LinkSpan>,
}

impl ConsoleLine {
    pub fn new(stream: Stream, raw: &str) -> Self {
        let text = strip_ansi(raw);
        let links = detect(&text);
        Self { stream, text, links }
    }
}

pub const DEFAULT_BUFFER_KB: usize = 1024;

pub const MAX_BUFFER_KB: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Console {
    lines: Vec<ConsoleLine>,
    first: usize,
    bytes: usize,
    limit: usize,
}

impl Default for Console {
    fn default() -> Self {
        Self::with_limit(DEFAULT_BUFFER_KB * 1024)
    }
}

impl Console {
    pub fn with_limit(bytes: usize) -> Self {
        Self { lines: Vec::new(), first: 0, bytes: 0, limit: bytes.max(1) }
    }

    pub fn set_limit(&mut self, bytes: usize) {
        self.limit = bytes.max(1);
        self.trim();
    }

    pub fn push(&mut self, line: ConsoleLine) -> usize {
        self.bytes += cost(&line);
        self.lines.push(line);
        let index = self.len() - 1;
        self.trim();
        index
    }

    pub fn push_at(&mut self, index: usize, line: ConsoleLine) -> usize {
        if index > self.len() {
            self.first = index;
            self.lines.clear();
            self.bytes = 0;
        }
        self.push(line)
    }

    fn trim(&mut self) {
        if self.bytes <= self.limit {
            return;
        }
        let keep = self.limit - self.limit / 8;
        let mut dropped = 0;
        let mut bytes = self.bytes;
        while dropped + 1 < self.lines.len() && bytes > keep {
            bytes -= cost(&self.lines[dropped]);
            dropped += 1;
        }
        self.lines.drain(..dropped);
        self.first += dropped;
        self.bytes = bytes;
    }

    pub fn lines(&self) -> &[ConsoleLine] {
        &self.lines
    }

    pub fn first(&self) -> usize {
        self.first
    }

    pub fn get(&self, index: usize) -> Option<&ConsoleLine> {
        self.lines.get(index.checked_sub(self.first)?)
    }

    pub fn range(&self, range: std::ops::Range<usize>) -> &[ConsoleLine] {
        let start = range.start.max(self.first) - self.first;
        let end = range.end.min(self.len()).saturating_sub(self.first);
        self.lines.get(start..end.max(start)).unwrap_or_default()
    }

    pub fn since(&self, from: usize) -> &[ConsoleLine] {
        self.range(from..self.len())
    }

    pub fn len(&self) -> usize {
        self.first + self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn cost(line: &ConsoleLine) -> usize {
    line.text.len() + 1
}

pub fn strip_ansi(text: &str) -> String {
    let text = text.strip_suffix('\r').unwrap_or(text);
    let text = text.rsplit('\r').next().unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    for code in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&code) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(code) = chars.next() {
                        if code == '\u{7}' {
                            break;
                        }
                        if code == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\t' => out.push('\t'),
            control if control.is_control() => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_sequences_are_stripped() {
        let cases = [
            ("\u{1b}[31mFAIL\u{1b}[0m TestA", "FAIL TestA"),
            ("\u{1b}[1;32mok\u{1b}[m\tpkg", "ok\tpkg"),
            ("\u{1b}]0;title\u{7}after", "after"),
            ("\u{1b}]8;;http://x\u{1b}\\link\u{1b}]8;;\u{1b}\\", "link"),
            ("progress 10%\rprogress 100%", "progress 100%"),
            ("windows line\r", "windows line"),
            ("bell\u{7} here\u{1b}[2K", "bell here"),
            ("plain", "plain"),
        ];
        for (raw, text) in cases {
            assert_eq!(strip_ansi(raw), text, "{raw:?}");
        }
    }

    #[test]
    fn a_console_keeps_lines_with_their_links() {
        let mut console = Console::default();
        assert!(console.is_empty());
        let first = console.push(ConsoleLine::new(Stream::Stdout, "\u{1b}[31mmain.go:3:1\u{1b}[0m: oops"));
        console.push(ConsoleLine::new(Stream::System, "Process finished"));
        assert_eq!(first, 0);
        assert_eq!(console.len(), 2);
        assert_eq!(console.lines()[0].text, "main.go:3:1: oops");
        assert_eq!(console.lines()[0].links.len(), 1);
        assert_eq!(console.since(1).len(), 1);
        assert!(console.since(5).is_empty());
    }

    #[test]
    fn a_console_keeps_only_its_buffer_and_indexes_stay_absolute() {
        let mut console = Console::with_limit(100);
        for index in 0..1000 {
            assert_eq!(console.push(ConsoleLine::new(Stream::Stdout, &format!("{index:09}"))), index);
        }
        assert_eq!(console.len(), 1000);
        assert!(console.lines().len() <= 10, "{}", console.lines().len());
        assert_eq!(console.lines().iter().map(cost).sum::<usize>(), console.bytes);
        assert_eq!(console.get(999).unwrap().text, "000000999");
        assert_eq!(console.get(console.first()).unwrap().text, format!("{:09}", console.first()));
        assert!(console.get(0).is_none());
        assert_eq!(console.since(0).len(), console.lines().len());
        assert_eq!(console.since(998).len(), 2);
        assert_eq!(console.range(990..995).len(), 5);
        assert!(console.range(0..5).is_empty());
        let mut huge = Console::with_limit(10);
        huge.push(ConsoleLine::new(Stream::Stdout, &"x".repeat(50)));
        assert_eq!(huge.lines().len(), 1, "the newest line stays");
        assert_eq!(huge.push_at(7, ConsoleLine::new(Stream::Stdout, "later")), 7);
        assert_eq!((huge.first(), huge.lines().len()), (7, 1));
    }
}

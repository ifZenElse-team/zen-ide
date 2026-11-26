use crate::console::{ConsoleLine, Stream};
use crate::test_tree::TestEvent;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedLine {
    pub console: Option<ConsoleLine>,
    pub events: Vec<TestEvent>,
}

impl ParsedLine {
    pub fn console(stream: Stream, text: &str) -> Self {
        Self { console: Some(ConsoleLine::new(stream, text)), events: Vec::new() }
    }
}

pub trait OutputParser: Send {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine;

    fn finish(&mut self) -> Vec<TestEvent>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PlainParser;

impl OutputParser for PlainParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        ParsedLine::console(stream, text)
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_parser_passes_lines_through() {
        let mut parser = PlainParser;
        let parsed = parser.line(Stream::Stderr, "\u{1b}[1mhello\u{1b}[0m");
        assert_eq!(parsed.console.map(|line| (line.stream, line.text)), Some((Stream::Stderr, "hello".to_owned())));
        assert!(parsed.events.is_empty());
        assert!(parser.finish().is_empty());
    }
}

use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use crate::{HighlightKind, Span, TextChange};

const LARGEST: usize = 64 * 1024;

const WIDENED_LINES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Literal {
    pub range: Range<usize>,
    pub content: Range<usize>,
    pub quote: u8,
    pub escapes: bool,
}

pub trait Injector: Send + Sync {
    fn language(&self) -> &str;

    fn may_hold(&self, text: &str) -> bool;

    fn accepts(&self, text: &str, literal: &Literal, code: &str) -> bool;

    fn highlight(&self, code: &str) -> Vec<Span>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub literal: Literal,
    pub language: Arc<str>,
    spans: Arc<[Span]>,
    hash: u64,
}

impl Fragment {
    pub fn spans(&self) -> impl Iterator<Item = Span> + '_ {
        let start = self.literal.content.start;
        self.spans
            .iter()
            .map(move |span| Span { range: span.range.start + start..span.range.end + start, kind: span.kind })
    }

    pub fn code(&self, text: &str) -> Code {
        Code::new(text, &self.literal)
    }

    fn shifted(mut self, by: isize) -> Self {
        let shift = |at: usize| (at as isize + by) as usize;
        self.literal.range = shift(self.literal.range.start)..shift(self.literal.range.end);
        self.literal.content = shift(self.literal.content.start)..shift(self.literal.content.end);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub text: String,
    starts: Vec<usize>,
}

impl Code {
    pub fn new(text: &str, literal: &Literal) -> Self {
        let content = &text[literal.content.clone()];
        let base = literal.content.start;
        let bytes = content.as_bytes();
        let mut code = Vec::with_capacity(bytes.len());
        let mut starts = Vec::with_capacity(bytes.len() + 1);
        let mut at = 0;
        while at < bytes.len() {
            let escaped = literal.escapes && bytes[at] == b'\\';
            let read = escaped.then(|| bytes.get(at + 1).copied()).flatten().and_then(|next| match next {
                b'n' => Some(b'\n'),
                b't' => Some(b'\t'),
                b'r' => Some(b'\r'),
                b'\\' | b'"' | b'\'' | b'`' => Some(next),
                _ => None,
            });
            match read {
                Some(byte) => {
                    code.push(byte);
                    starts.push(base + at);
                    at += 2;
                }
                None => {
                    code.push(bytes[at]);
                    starts.push(base + at);
                    at += 1;
                }
            }
        }
        starts.push(literal.content.end);
        let text = String::from_utf8(code).unwrap_or_else(|_| content.to_owned());
        if text.len() + 1 != starts.len() {
            return Self { starts: (base..=literal.content.end).collect(), text: content.to_owned() };
        }
        Self { text, starts }
    }

    pub fn to_text(&self, at: usize) -> usize {
        self.starts[at.min(self.starts.len() - 1)]
    }

    pub fn from_text(&self, at: usize) -> usize {
        self.starts.partition_point(|start| *start < at).min(self.text.len())
    }
}

pub fn literals(text: &str, spans: &[Span]) -> Vec<Literal> {
    let bytes = text.as_bytes();
    let mut joined: Vec<Range<usize>> = Vec::new();
    for span in spans.iter().filter(|span| span.kind == HighlightKind::String) {
        match joined.last_mut() {
            Some(last) if last.end == span.range.start && last.end > 0 && bytes.get(last.end - 1) == Some(&b'\n') => {
                last.end = span.range.end;
            }
            _ => joined.push(span.range.clone()),
        }
    }
    joined.into_iter().filter_map(|range| literal(text, range)).collect()
}

fn literal(text: &str, range: Range<usize>) -> Option<Literal> {
    let bytes = text.as_bytes().get(range.clone())?;
    let prefix =
        bytes.iter().take_while(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'#' | b'@' | b'$')).count();
    if prefix > 4 {
        return None;
    }
    let quote = *bytes.get(prefix)?;
    if !matches!(quote, b'"' | b'\'' | b'`') {
        return None;
    }
    let tripled = bytes.len() >= prefix + 6 && bytes[prefix..prefix + 3].iter().all(|byte| *byte == quote);
    let opening = if tripled { 3 } else { 1 };
    let hashes = bytes[..prefix].iter().filter(|byte| **byte == b'#').count();
    let raw = bytes[..prefix].iter().any(|byte| matches!(byte, b'r' | b'R'));
    let start = prefix + opening;
    let closing = opening + hashes;
    let closes = bytes.len() >= start + closing
        && bytes[bytes.len() - closing..bytes.len() - hashes].iter().all(|byte| *byte == quote)
        && bytes[bytes.len() - hashes..].iter().all(|byte| *byte == b'#');
    let end = if closes { bytes.len() - closing } else { bytes.len() };
    if !text.is_char_boundary(range.start + start) || !text.is_char_boundary(range.start + end) {
        return None;
    }
    Some(Literal { content: range.start + start..range.start + end, range, quote, escapes: !raw })
}

fn hashed(language: &str, content: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (language, content).hash(&mut hasher);
    hasher.finish()
}

fn mapped(spans: Vec<Span>, code: &Code, base: usize) -> Arc<[Span]> {
    spans
        .into_iter()
        .filter(|span| span.range.start < span.range.end && span.range.end <= code.text.len())
        .map(|span| Span {
            range: code.to_text(span.range.start) - base..code.to_text(span.range.end) - base,
            kind: span.kind,
        })
        .collect()
}

fn line_start(text: &str, at: usize) -> usize {
    text.as_bytes()[..at.min(text.len())].iter().rposition(|byte| *byte == b'\n').map_or(0, |found| found + 1)
}

fn next_line(text: &str, at: usize) -> usize {
    text.as_bytes()[at.min(text.len())..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(text.len(), |found| at + found + 1)
}

pub(crate) struct Injections {
    injectors: Vec<Arc<dyn Injector>>,
    fragments: Vec<Fragment>,
}

impl Injections {
    pub(crate) fn new(injectors: Vec<Arc<dyn Injector>>) -> Self {
        Self { injectors, fragments: Vec::new() }
    }

    pub(crate) fn same(&self, injectors: &[Arc<dyn Injector>]) -> bool {
        self.injectors.len() == injectors.len()
            && self.injectors.iter().zip(injectors).all(|(kept, given)| Arc::ptr_eq(kept, given))
    }

    pub(crate) fn fragments(&self) -> &[Fragment] {
        &self.fragments
    }

    pub(crate) fn scan_all(&mut self, text: &str, spans: &dyn Fn(Range<usize>) -> Vec<Span>) {
        let old = std::mem::take(&mut self.fragments);
        self.fragments = self.scan(text, 0..text.len(), spans, &old);
    }

    pub(crate) fn follow(
        &mut self,
        text: &str,
        change: TextChange,
        lexed: usize,
        spans: &dyn Fn(Range<usize>) -> Vec<Span>,
    ) {
        let by = change.new_end as isize - change.old_end as isize;
        let mut kept = Vec::with_capacity(self.fragments.len());
        let mut dropped = Vec::new();
        for fragment in std::mem::take(&mut self.fragments) {
            if fragment.literal.range.end <= change.start {
                kept.push(fragment);
            } else if fragment.literal.range.start >= change.old_end {
                kept.push(fragment.shifted(by));
            } else {
                dropped.push(fragment);
            }
        }
        let region = Self::widened(text, change, lexed, spans, &kept);
        let (inside, outside): (Vec<Fragment>, Vec<Fragment>) = kept.into_iter().partition(|fragment| {
            fragment.literal.range.start < region.end && region.start < fragment.literal.range.end
        });
        dropped.extend(inside);
        let found = self.scan(text, region, spans, &dropped);
        let mut fragments = outside;
        fragments.extend(found);
        fragments.sort_by_key(|fragment| fragment.literal.range.start);
        self.fragments = fragments;
    }

    fn widened(
        text: &str,
        change: TextChange,
        lexed: usize,
        spans: &dyn Fn(Range<usize>) -> Vec<Span>,
        kept: &[Fragment],
    ) -> Range<usize> {
        let mut start = line_start(text, change.start);
        let mut end = start;
        for _ in 0..lexed.max(1) {
            end = next_line(text, end);
        }
        end = end.max(next_line(text, change.new_end));
        start = line_start(text, start.saturating_sub(1));
        end = next_line(text, end);
        let in_string = |at: usize| {
            spans(at..at + 1).iter().any(|span| span.range.contains(&at) && span.kind == HighlightKind::String)
        };
        for _ in 0..WIDENED_LINES {
            let mut moved = false;
            for fragment in kept {
                if fragment.literal.range.start < end && start < fragment.literal.range.end {
                    moved |= fragment.literal.range.start < start || fragment.literal.range.end > end;
                    start = start.min(line_start(text, fragment.literal.range.start));
                    end = end.max(next_line(text, fragment.literal.range.end.saturating_sub(1)));
                }
            }
            if start > 0 && in_string(start) && in_string(start - 1) {
                start = line_start(text, start - 1);
                moved = true;
            }
            if end < text.len() && end > 0 && in_string(end) && in_string(end - 1) {
                end = next_line(text, end);
                moved = true;
            }
            if !moved {
                break;
            }
        }
        start..end
    }

    fn scan(
        &self,
        text: &str,
        region: Range<usize>,
        spans: &dyn Fn(Range<usize>) -> Vec<Span>,
        reuse: &[Fragment],
    ) -> Vec<Fragment> {
        let Some(piece) = text.get(region.clone()) else { return Vec::new() };
        let held: Vec<&Arc<dyn Injector>> = self.injectors.iter().filter(|injector| injector.may_hold(piece)).collect();
        if held.is_empty() {
            return Vec::new();
        }
        literals(text, &spans(region.clone()))
            .into_iter()
            .filter(|literal| {
                literal.range.start >= region.start
                    && literal.range.end <= region.end
                    && literal.content.len() <= LARGEST
            })
            .filter_map(|literal| {
                let code = Code::new(text, &literal);
                let injector = held.iter().find(|injector| injector.accepts(text, &literal, &code.text))?;
                let language = injector.language();
                let hash = hashed(language, &text[literal.content.clone()]);
                let spans = reuse
                    .iter()
                    .find(|old| old.hash == hash && &*old.language == language)
                    .map(|old| Arc::clone(&old.spans))
                    .unwrap_or_else(|| mapped(injector.highlight(&code.text), &code, literal.content.start));
                Some(Fragment { literal, language: Arc::from(language), spans, hash })
            })
            .collect()
    }

    pub(crate) fn overlay(&self, host: Vec<Span>, range: Range<usize>) -> Vec<Span> {
        let first = self.fragments.partition_point(|fragment| fragment.literal.content.end <= range.start);
        let touching: Vec<&Fragment> = self.fragments[first..]
            .iter()
            .take_while(|fragment| fragment.literal.content.start < range.end.max(range.start + 1))
            .collect();
        if touching.is_empty() {
            return host;
        }
        let limit = range.end.max(range.start + 1);
        let mut out = Vec::with_capacity(host.len());
        let mut push = |piece: Range<usize>, kind: HighlightKind| {
            if piece.start < piece.end && piece.end > range.start && piece.start < limit {
                out.push(Span { range: piece, kind });
            }
        };
        for span in host {
            if span.kind != HighlightKind::String {
                push(span.range, span.kind);
                continue;
            }
            let mut at = span.range.start;
            for fragment in &touching {
                let content = &fragment.literal.content;
                let (from, to) = (content.start.max(span.range.start), content.end.min(span.range.end));
                if from >= to {
                    continue;
                }
                push(at..from, HighlightKind::String);
                for injected in fragment.spans() {
                    push(injected.range.start.max(from)..injected.range.end.min(to), injected.kind);
                }
                at = to;
            }
            push(at..span.range.end, HighlightKind::String);
        }
        out
    }
}

#[cfg(test)]
mod tests;

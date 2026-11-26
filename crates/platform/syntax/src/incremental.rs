use std::ops::Range;
use std::sync::Arc;

use crate::inject::{Fragment, Injections, Injector};
use crate::{HighlightKind, Highlighter, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextChange {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
}

impl TextChange {
    pub fn merged(changes: &[TextChange]) -> Option<TextChange> {
        let (first, rest) = changes.split_first()?;
        let mut merged = *first;
        for change in rest {
            let start = merged.start.min(change.start);
            let old_end = if change.old_end > merged.new_end {
                merged.old_end + (change.old_end - merged.new_end)
            } else {
                merged.old_end
            };
            let new_end = (merged.new_end.max(change.old_end) + change.new_end) - change.old_end;
            merged = TextChange { start, old_end, new_end };
        }
        Some(merged)
    }
}

pub trait LineLexer {
    type State: Clone + PartialEq + Default;
    type Line;

    fn lex(&self, line: &str, state: &mut Self::State) -> Self::Line;
}

pub struct Lines<L: LineLexer> {
    starts: Vec<usize>,
    states: Vec<L::State>,
    lines: Vec<L::Line>,
    len: usize,
}

impl<L: LineLexer> std::fmt::Debug for Lines<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lines").field("lines", &self.starts.len()).field("len", &self.len).finish()
    }
}

impl<L: LineLexer> Lines<L> {
    pub fn new(lexer: &L, text: &str) -> Self {
        let starts = line_starts(text, 0, text.len(), true);
        let mut lines = Self {
            states: Vec::with_capacity(starts.len()),
            lines: Vec::with_capacity(starts.len()),
            starts,
            len: text.len(),
        };
        let mut state = L::State::default();
        for index in 0..lines.starts.len() {
            lines.states.push(state.clone());
            let line = &text[lines.starts[index]..lines.end(index)];
            lines.lines.push(lexer.lex(line, &mut state));
        }
        lines
    }

    pub fn edit(
        &mut self,
        lexer: &L,
        text: &str,
        change: TextChange,
        mut dropped: impl FnMut(L::Line),
    ) -> Range<usize> {
        let old_len = self.len;
        let delta = text.len() as isize - old_len as isize;
        debug_assert_eq!(
            change.new_end as isize - change.old_end as isize,
            delta,
            "the change makes the text's length"
        );
        let first = self.line_of(change.start.min(old_len));
        let last = self.line_of(change.old_end.min(old_len));
        let tail = last + 1 == self.starts.len();
        let region_start = self.starts[first];
        let region_end = (self.end(last) as isize + delta) as usize;
        let starts = line_starts(text, region_start, region_end, tail);
        let added = starts.len();
        for start in &mut self.starts[last + 1..] {
            *start = (*start as isize + delta) as usize;
        }
        self.len = text.len();
        self.starts.splice(first..=last, starts);
        let mut state = self.states[first].clone();
        let mut fresh = Vec::with_capacity(added);
        let mut states = Vec::with_capacity(added);
        for index in first..first + added {
            states.push(state.clone());
            fresh.push(lexer.lex(&text[self.starts[index]..self.end(index)], &mut state));
        }
        self.states.splice(first..=last, states);
        for line in self.lines.splice(first..=last, fresh) {
            dropped(line);
        }
        let mut index = first + added;
        while index < self.starts.len() && self.states[index] != state {
            self.states[index] = state.clone();
            let line = lexer.lex(&text[self.starts[index]..self.end(index)], &mut state);
            dropped(std::mem::replace(&mut self.lines[index], line));
            index += 1;
        }
        first..index
    }

    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset).saturating_sub(1)
    }

    pub fn start(&self, index: usize) -> usize {
        self.starts[index]
    }

    pub fn end(&self, index: usize) -> usize {
        self.starts.get(index + 1).copied().unwrap_or(self.len)
    }

    pub fn line(&self, index: usize) -> &L::Line {
        &self.lines[index]
    }

    pub fn lines(&self) -> &[L::Line] {
        &self.lines
    }

    pub fn state(&self, index: usize) -> &L::State {
        &self.states[index]
    }
}

fn line_starts(text: &str, from: usize, to: usize, tail: bool) -> Vec<usize> {
    let mut starts = vec![from];
    starts.extend(
        text.as_bytes()[from..to]
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .map(|(at, _)| from + at + 1)
            .filter(|start| *start < to || tail),
    );
    starts
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldRegion {
    pub range: Range<usize>,
    pub kind: FoldRegionKind,
    pub placeholder: Option<String>,
    pub option: Option<&'static str>,
}

impl FoldRegion {
    pub fn new(range: Range<usize>, kind: FoldRegionKind) -> Self {
        Self { range, kind, placeholder: None, option: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FoldRegionKind {
    Block,
    Imports,
    Comment,
}

pub trait Incremental: Send + Sync {
    fn edit(&mut self, text: &str, change: TextChange) -> usize;

    fn spans(&self, text: &str, range: Range<usize>) -> Vec<Span>;

    fn folds(&self, _text: &str) -> Option<Vec<FoldRegion>> {
        None
    }

    fn lines(&self) -> usize;
}

pub struct Highlighting {
    highlighter: Arc<dyn Highlighter>,
    kept: Kept,
    lexed: usize,
    injections: Option<Injections>,
}

enum Kept {
    Whole(Vec<Span>),
    Lines(Box<dyn Incremental>),
}

impl std::fmt::Debug for Highlighting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighting").field("incremental", &self.is_incremental()).field("lexed", &self.lexed).finish()
    }
}

impl Highlighting {
    pub fn new(highlighter: Arc<dyn Highlighter>, text: &str) -> Self {
        let mut highlighting = Self { highlighter, kept: Kept::Whole(Vec::new()), lexed: 0, injections: None };
        highlighting.reset(text);
        highlighting
    }

    fn reset(&mut self, text: &str) {
        self.kept = match self.highlighter.incremental(text) {
            Some(lines) => {
                self.lexed = lines.lines();
                Kept::Lines(lines)
            }
            None => {
                self.lexed = text.bytes().filter(|byte| *byte == b'\n').count() + 1;
                Kept::Whole(self.highlighter.highlight(text))
            }
        };
        if let Some(injections) = &mut self.injections {
            let kept = &self.kept;
            injections.scan_all(text, &|range| kept_spans(kept, text, range));
        }
    }

    pub fn inject(&mut self, text: &str, injectors: Vec<Arc<dyn Injector>>) {
        if injectors.is_empty() {
            self.injections = None;
            return;
        }
        if self.injections.as_ref().is_some_and(|injections| injections.same(&injectors)) {
            return;
        }
        let mut injections = Injections::new(injectors);
        let kept = &self.kept;
        injections.scan_all(text, &|range| kept_spans(kept, text, range));
        self.injections = Some(injections);
    }

    pub fn fragments(&self) -> &[Fragment] {
        self.injections.as_ref().map_or(&[], Injections::fragments)
    }

    pub fn fragment_at(&self, offset: usize) -> Option<&Fragment> {
        let fragments = self.fragments();
        let at = fragments.partition_point(|fragment| fragment.literal.content.end < offset);
        fragments.get(at).filter(|fragment| fragment.literal.content.start <= offset)
    }

    pub fn spans_with_injections(&self, text: &str, range: Range<usize>) -> Vec<Span> {
        let host = self.spans(text, range.clone());
        match &self.injections {
            Some(injections) => injections.overlay(host, range),
            None => host,
        }
    }

    pub fn update(&mut self, text: &str, changes: Option<&[TextChange]>) {
        match (&mut self.kept, changes) {
            (_, Some([])) => self.lexed = 0,
            (Kept::Lines(lines), Some(changes)) => {
                let merged = TextChange::merged(changes);
                self.lexed = merged.map_or(0, |change| lines.edit(text, change));
                if let (Some(injections), Some(change)) = (&mut self.injections, merged) {
                    let kept = &self.kept;
                    injections.follow(text, change, self.lexed, &|range| kept_spans(kept, text, range));
                }
            }
            _ => self.reset(text),
        }
    }

    pub fn highlighter(&self) -> &Arc<dyn Highlighter> {
        &self.highlighter
    }

    pub fn is_incremental(&self) -> bool {
        matches!(self.kept, Kept::Lines(_))
    }

    pub fn lexed(&self) -> usize {
        self.lexed
    }

    pub fn spans(&self, text: &str, range: Range<usize>) -> Vec<Span> {
        kept_spans(&self.kept, text, range)
    }

    pub fn kind_at(&self, text: &str, offset: usize) -> Option<HighlightKind> {
        self.spans(text, offset..offset + 1).into_iter().find(|span| span.range.contains(&offset)).map(|span| span.kind)
    }

    pub fn in_string_or_comment(&self, text: &str, offset: usize) -> bool {
        matches!(
            self.kind_at(text, offset),
            Some(HighlightKind::String | HighlightKind::Comment | HighlightKind::DocComment)
        )
    }

    pub fn folds(&self, text: &str) -> Option<Vec<FoldRegion>> {
        match &self.kept {
            Kept::Lines(lines) => lines.folds(text),
            Kept::Whole(_) => None,
        }
    }
}

fn kept_spans(kept: &Kept, text: &str, range: Range<usize>) -> Vec<Span> {
    match kept {
        Kept::Lines(lines) => lines.spans(text, range),
        Kept::Whole(spans) => {
            let first = spans.partition_point(|span| span.range.end <= range.start);
            spans[first..]
                .iter()
                .take_while(|span| span.range.start < range.end.max(range.start + 1))
                .cloned()
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Comments;

    impl LineLexer for Comments {
        type State = bool;
        type Line = (bool, String);

        fn lex(&self, line: &str, open: &mut bool) -> (bool, String) {
            let start = *open;
            let mut rest = line;
            loop {
                let mark = if *open { "*/" } else { "/*" };
                match rest.find(mark) {
                    Some(at) => {
                        *open = !*open;
                        rest = &rest[at + 2..];
                    }
                    None => break,
                }
            }
            (start, line.to_owned())
        }
    }

    fn apply(text: &mut String, start: usize, end: usize, insert: &str) -> TextChange {
        text.replace_range(start..end, insert);
        TextChange { start, old_end: end, new_end: start + insert.len() }
    }

    fn check(lines: &Lines<Comments>, text: &str) {
        let fresh = Lines::new(&Comments, text);
        assert_eq!(lines.starts, fresh.starts, "{text:?}");
        assert_eq!(lines.states, fresh.states, "{text:?}");
        assert_eq!(lines.lines, fresh.lines, "{text:?}");
    }

    #[test]
    fn an_edit_lexes_its_lines_and_those_whose_state_changed() {
        let mut text = String::from("a\nb\nc\nd\ne\n");
        let mut lines = Lines::new(&Comments, &text);
        assert_eq!(lines.len(), 6, "a text ending with a line break has an empty last line");
        let change = apply(&mut text, 2, 3, "bb");
        let mut dropped = Vec::new();
        assert_eq!(lines.edit(&Comments, &text, change, |line| dropped.push(line.1)), 1..2);
        assert_eq!(dropped, ["b\n"]);
        check(&lines, &text);
        let change = apply(&mut text, 0, 0, "/*");
        assert_eq!(lines.edit(&Comments, &text, change, |_| {}), 0..6);
        check(&lines, &text);
        let change = apply(&mut text, 8, 8, "*/");
        assert_eq!(lines.edit(&Comments, &text, change, |_| {}), 2..6);
        check(&lines, &text);
        let change = apply(&mut text, 4, 4, "x\ny\n");
        assert_eq!(lines.edit(&Comments, &text, change, |_| {}), 1..4);
        check(&lines, &text);
        let len = text.len();
        let change = apply(&mut text, 0, len, "");
        lines.edit(&Comments, &text, change, |_| {});
        check(&lines, &text);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn random_edits_leave_what_lexing_the_whole_text_gives() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |below: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % below.max(1) as u64) as usize
        };
        let pieces = ["\n", "/*", "*/", "x", "ab\n", "", "\n\n"];
        let mut text = String::from("one\n/* two\nthree */\nfour\n");
        let mut lines = Lines::new(&Comments, &text);
        for _ in 0..2_000 {
            let start = next(text.len() + 1);
            let end = (start + next(6)).min(text.len());
            let change = apply(&mut text, start, end, pieces[next(pieces.len())]);
            lines.edit(&Comments, &text, change, |_| {});
            check(&lines, &text);
        }
    }

    #[test]
    fn changes_merge_into_one() {
        let mut text = String::from("0123456789");
        let before = text.clone();
        let changes = [apply(&mut text, 2, 4, "abc"), apply(&mut text, 8, 9, ""), apply(&mut text, 0, 1, "zz")];
        let merged = TextChange::merged(&changes).unwrap();
        assert_eq!(merged.start, 0);
        assert_eq!(before[merged.old_end..], text[merged.new_end..]);
        assert_eq!(before[..merged.start], text[..merged.start]);
        assert_eq!(merged.new_end as isize - merged.old_end as isize, text.len() as isize - before.len() as isize);
        assert_eq!(TextChange::merged(&[]), None);
    }
}

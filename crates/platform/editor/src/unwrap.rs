use std::collections::{HashMap, HashSet};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnwrapKind {
    Unwrap,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnwrapOption {
    pub kind: UnwrapKind,
    pub construct: Option<String>,
    pub range: Range<usize>,
    pub replacement: String,
    pub kept: Vec<Range<usize>>,
    pub caret: usize,
}

const NONE: usize = usize::MAX;
const LOOPS: [&str; 3] = ["for", "while", "loop"];
const SWITCHES: [&str; 2] = ["switch", "select"];
const FUNCTIONS: [&str; 2] = ["func", "function"];
const DECLARATIONS: [&str; 4] = ["func", "fn", "function", "def"];
const NOT_CALLS: [&str; 16] = [
    "if", "for", "while", "switch", "select", "match", "return", "func", "function", "fn", "case", "else", "catch",
    "loop", "go", "defer",
];

const PATTERN_WORDS: [&str; 6] = ["let", "go", "defer", "struct", "enum", "impl"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Head {
    If,
    Else,
    Loop,
    Switch,
    Function,
    Bare,
    Other,
}

#[derive(Clone, Copy, Debug)]
struct Block {
    header: usize,
    open: usize,
    close: usize,
    head: Head,
}

struct Line {
    text: String,
    source: Option<(usize, usize)>,
}

struct Scan {
    chars: Vec<char>,
    skipped: Vec<bool>,
    partner: Vec<usize>,
    nested: HashSet<usize>,
    newline: &'static str,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

pub fn unwrap_options(text: &str, skipped: &[Range<usize>], caret: usize) -> Vec<UnwrapOption> {
    let scan = Scan::new(text, skipped);
    let caret = caret.min(scan.chars.len());
    let mut found: Vec<(Range<usize>, UnwrapOption)> = Vec::new();
    let blocks = scan.blocks();
    let by_header: HashMap<usize, usize> =
        blocks.iter().enumerate().filter(|(_, block)| block.head == Head::Else).map(|(at, b)| (b.header, at)).collect();
    for block in &blocks {
        match block.head {
            Head::If => scan.chain(&blocks, &by_header, *block, caret, &mut found),
            Head::Loop => {
                let range = block.header..block.close + 1;
                if range.contains(&caret) || range.end == caret {
                    let name = format!("{}…", scan.word(block.header));
                    found.push((range.clone(), scan.unwrap(name.into(), range, block.open + 1..block.close, caret)));
                }
            }
            Head::Switch => scan.cases(*block, caret, &mut found),
            Head::Function => {
                let start = scan.line_start(block.header);
                let end = scan.line_end(block.close);
                if (block.header..=block.close + 1).contains(&caret)
                    && scan.whole_statement(start..block.header, block.close + 1..end)
                {
                    let option =
                        scan.unwrap_lines(Some("func…".to_owned()), start..end, block.open + 1..block.close, caret);
                    found.push((block.header..block.close + 1, option));
                }
            }
            Head::Bare => {
                let range = block.open..block.close + 1;
                if (range.start..=range.end).contains(&caret) {
                    found.push((range.clone(), scan.unwrap(None, range, block.open + 1..block.close, caret)));
                }
            }
            Head::Else | Head::Other => {}
        }
    }
    scan.calls(caret, &mut found);
    found.sort_by_key(|(scope, _)| (std::cmp::Reverse(scope.start), scope.len()));
    found.into_iter().map(|(_, option)| option).collect()
}

impl Scan {
    fn new(text: &str, skipped: &[Range<usize>]) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let mut marks = vec![false; chars.len()];
        for range in skipped {
            let range = range.start.min(chars.len())..range.end.min(chars.len());
            marks[range].iter_mut().for_each(|mark| *mark = true);
        }
        let mut partner = vec![NONE; chars.len()];
        let mut nested = HashSet::new();
        let mut stack: Vec<usize> = Vec::new();
        for (at, &c) in chars.iter().enumerate() {
            if marks[at] {
                continue;
            }
            match c {
                '(' | '[' | '{' => {
                    if !stack.is_empty() {
                        nested.insert(at);
                    }
                    stack.push(at);
                }
                ')' | ']' | '}' => {
                    let open = match c {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    if let Some(depth) = stack.iter().rposition(|&at| chars[at] == open) {
                        let start = stack[depth];
                        stack.truncate(depth);
                        partner[start] = at;
                        partner[at] = start;
                    }
                }
                _ => {}
            }
        }
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        Self { chars, skipped: marks, partner, nested, newline }
    }

    fn word(&self, at: usize) -> String {
        self.chars[at.min(self.chars.len())..].iter().take_while(|&&c| is_word(c)).collect()
    }

    fn word_is(&self, at: usize, words: &[&str]) -> bool {
        (at == 0 || !is_word(self.chars[at - 1])) && words.contains(&self.word(at).as_str())
    }

    fn skip_blank(&self, mut at: usize, end: usize) -> usize {
        while at < end && self.chars[at].is_whitespace() {
            at += 1;
        }
        at
    }

    fn line_start(&self, at: usize) -> usize {
        let mut start = at.min(self.chars.len());
        while start > 0 && self.chars[start - 1] != '\n' {
            start -= 1;
        }
        start
    }

    fn line_end(&self, at: usize) -> usize {
        let mut end = at.min(self.chars.len());
        while end < self.chars.len() && self.chars[end] != '\n' {
            end += 1;
        }
        while end > at && self.chars[end - 1] == '\r' {
            end -= 1;
        }
        end
    }

    fn first_in_line(&self, at: usize) -> usize {
        let start = self.line_start(at);
        let mut first = start;
        while first < at && is_blank(self.chars[first]) {
            first += 1;
        }
        first
    }

    fn starts_line(&self, at: usize) -> bool {
        self.first_in_line(at) == at
    }

    fn ends_line(&self, at: usize) -> bool {
        let end = self.line_end(at);
        self.chars[at.min(end)..end].iter().all(|&c| c.is_whitespace())
    }

    fn indent(&self, at: usize) -> String {
        let start = self.line_start(at);
        self.chars[start..].iter().take_while(|&&c| is_blank(c)).collect()
    }

    fn label_colon(&self, at: usize) -> bool {
        let before = at.checked_sub(1).map(|at| self.chars[at]);
        let after = self.chars.get(at + 1).copied();
        before != Some(':') && !matches!(after, Some(':' | '='))
    }

    fn header(&self, open: usize, semicolons: bool) -> (usize, Option<char>) {
        let mut at = open;
        while at > 0 {
            let c = self.chars[at - 1];
            if self.skipped[at - 1] {
                at -= 1;
                continue;
            }
            if matches!(c, ')' | ']') && self.partner[at - 1] != NONE {
                at = self.partner[at - 1];
                continue;
            }
            if matches!(c, '\n' | '{' | '}' | '(' | '[' | ')' | ']') || (c == ';' && !semicolons) {
                return (self.skip_blank(at, open), Some(c));
            }
            at -= 1;
        }
        (self.skip_blank(0, open), None)
    }

    fn head(&self, header: usize, open: usize, boundary: Option<char>) -> (Head, usize) {
        if header == open {
            let bare = matches!(boundary, None | Some('\n' | ';' | '{' | '}'));
            return (if bare { Head::Bare } else { Head::Other }, header);
        }
        match self.word(header).as_str() {
            "if" => return (Head::If, header),
            "else" => return (Head::Else, header),
            word if LOOPS.contains(&word) => return (Head::Loop, header),
            word if SWITCHES.contains(&word) => return (Head::Switch, header),
            _ => {}
        }
        if !self.nested.contains(&open) {
            return (Head::Other, header);
        }
        let literal = (header..open).rev().find(|&at| {
            if self.skipped[at] || !self.word_is(at, &FUNCTIONS) {
                return false;
            }
            let params = self.skip_blank(at + self.word(at).chars().count(), open);
            self.chars.get(params) == Some(&'(')
                && self.partner[params] != NONE
                && self.partner[params] < open
                && self.signature_ends(self.partner[params] + 1, open)
        });
        literal.map_or((Head::Other, header), |at| (Head::Function, at))
    }

    fn signature_ends(&self, from: usize, open: usize) -> bool {
        let mut at = from;
        while at < open {
            if self.skipped[at] {
                at += 1;
            } else if matches!(self.chars[at], '(' | '[') && self.partner[at] != NONE {
                at = self.partner[at] + 1;
            } else if matches!(self.chars[at], ',' | '=' | ';' | ':' | '{' | '}') {
                return false;
            } else {
                at += 1;
            }
        }
        true
    }

    fn blocks(&self) -> Vec<Block> {
        let mut blocks = Vec::new();
        for (open, &c) in self.chars.iter().enumerate() {
            if c != '{' || self.skipped[open] || self.partner[open] == NONE {
                continue;
            }
            let close = self.partner[open];
            let (mut header, mut boundary) = self.header(open, false);
            if boundary == Some(';') {
                let (whole, before) = self.header(open, true);
                if self.word_is(whole, &["if", "for", "switch", "else"]) {
                    (header, boundary) = (whole, before);
                }
            }
            let (head, header) = self.head(header, open, boundary);
            blocks.push(Block { header, open, close, head });
        }
        blocks
    }

    fn whole_statement(&self, before: Range<usize>, after: Range<usize>) -> bool {
        let around = |at: usize| before.contains(&at) || after.contains(&at);
        let mut tail = after.end;
        while tail > after.start && (self.chars[tail - 1].is_whitespace() || self.skipped[tail - 1]) {
            tail -= 1;
        }
        if tail > after.start && self.chars[tail - 1] == ';' {
            tail -= 1;
        }
        before.clone().chain(after.start..tail).filter(|&at| !self.skipped[at]).all(|at| match self.chars[at] {
            '(' | '[' | '{' | ')' | ']' | '}' => self.partner[at] != NONE && around(self.partner[at]),
            ';' => false,
            _ => true,
        })
    }

    fn after_close(&self, close: usize) -> usize {
        let mut at = close + 1;
        while at < self.chars.len() && (self.chars[at].is_whitespace() || self.skipped[at]) {
            at += 1;
        }
        at
    }

    fn word_before(&self, at: usize) -> String {
        let mut end = at;
        while end > 0 && is_blank(self.chars[end - 1]) {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && is_word(self.chars[start - 1]) {
            start -= 1;
        }
        self.chars[start..end].iter().collect()
    }

    fn in_attribute(&self, at: usize) -> bool {
        let mut back = at;
        while back > 0 {
            back -= 1;
            if self.skipped[back] {
                continue;
            }
            match self.chars[back] {
                ')' | ']' | '}' if self.partner[back] != NONE => back = self.partner[back],
                '(' | '{' => return false,
                '[' => return back > 0 && (self.chars[back - 1] == '#' || self.chars[back - 1] == '!'),
                _ => {}
            }
        }
        false
    }

    fn is_pattern_or_operand(&self, start: usize, close: usize) -> bool {
        let after = self.skip_blank(close + 1, self.chars.len());
        PATTERN_WORDS.contains(&self.word_before(start).as_str())
            || self.chars[after..].starts_with(&['=', '>'])
            || self.in_attribute(start)
    }

    fn chain(
        &self,
        blocks: &[Block],
        by_header: &HashMap<usize, usize>,
        first: Block,
        caret: usize,
        found: &mut Vec<(Range<usize>, UnwrapOption)>,
    ) {
        let mut branches = vec![first];
        loop {
            let next = self.after_close(branches[branches.len() - 1].close);
            match by_header.get(&next) {
                Some(&at) if self.word_is(next, &["else"]) => branches.push(blocks[at]),
                _ => break,
            }
        }
        let whole = first.header..branches[branches.len() - 1].close + 1;
        let Some(index) = branches.iter().position(|branch| (branch.header..=branch.close + 1).contains(&caret)) else {
            return;
        };
        let branch = branches[index];
        let scope = branch.header..branch.close + 1;
        let body = branch.open + 1..branch.close;
        if index == 0 {
            found.push((scope, self.unwrap(Some("if…".to_owned()), whole, body, caret)));
            return;
        }
        found.push((scope.clone(), self.unwrap(Some("else…".to_owned()), whole.clone(), body, caret)));
        let removed = branches[index - 1].close + 1..branch.close + 1;
        let option = UnwrapOption {
            kind: UnwrapKind::Remove,
            construct: Some("else…".to_owned()),
            range: removed.clone(),
            replacement: String::new(),
            kept: vec![whole.start..removed.start, removed.end..whole.end],
            caret: removed.start,
        };
        found.push((scope, option));
    }

    fn cases(&self, block: Block, caret: usize, found: &mut Vec<(Range<usize>, UnwrapOption)>) {
        let mut keywords = Vec::new();
        let mut at = block.open + 1;
        while at < block.close {
            if self.skipped[at] {
                at += 1;
            } else if matches!(self.chars[at], '(' | '[' | '{') && self.partner[at] != NONE {
                at = self.partner[at] + 1;
            } else if self.word_is(at, &["case", "default"]) {
                keywords.push(at);
                at += self.word(at).chars().count();
            } else {
                at += 1;
            }
        }
        let whole = block.header..block.close + 1;
        for (index, &keyword) in keywords.iter().enumerate() {
            let next = keywords.get(index + 1).copied().unwrap_or(block.close);
            let end = if self.starts_line(next) { self.line_start(next) } else { next };
            let start = if self.starts_line(keyword) { self.line_start(keyword) } else { keyword };
            if !(keyword..=end).contains(&caret) {
                continue;
            }
            let Some(colon) = self.colon(keyword, end) else { continue };
            let name = format!("{}…", self.word(keyword));
            let scope = keyword..end;
            found.push((scope.clone(), self.unwrap(Some(name.clone()), whole.clone(), colon + 1..end, caret)));
            let option = UnwrapOption {
                kind: UnwrapKind::Remove,
                construct: Some(name),
                range: start..end,
                replacement: String::new(),
                kept: vec![whole.start..start, end..whole.end],
                caret: start,
            };
            found.push((scope, option));
        }
    }

    fn colon(&self, from: usize, end: usize) -> Option<usize> {
        let mut at = from;
        while at < end {
            if self.skipped[at] {
                at += 1;
            } else if matches!(self.chars[at], '(' | '[' | '{') && self.partner[at] != NONE {
                at = self.partner[at] + 1;
            } else if self.chars[at] == ':' && self.label_colon(at) {
                return Some(at);
            } else {
                at += 1;
            }
        }
        None
    }

    fn calls(&self, caret: usize, found: &mut Vec<(Range<usize>, UnwrapOption)>) {
        for (open, &c) in self.chars.iter().enumerate() {
            if c != '(' || self.skipped[open] || self.partner[open] == NONE {
                continue;
            }
            let close = self.partner[open];
            if close + 1 < caret {
                continue;
            }
            let mut start = open;
            while start > 0 && (is_word(self.chars[start - 1]) || self.chars[start - 1] == '.') {
                start -= 1;
            }
            let callee: String = self.chars[start..open].iter().collect();
            let last = callee.rsplit('.').next().unwrap_or_default();
            if callee.is_empty()
                || callee.starts_with('.')
                || callee.ends_with('.')
                || callee.starts_with(|c: char| c.is_ascii_digit())
                || NOT_CALLS.contains(&last)
                || DECLARATIONS.contains(&self.word(self.first_in_line(start)).as_str())
                || caret < start
                || self.is_pattern_or_operand(start, close)
            {
                continue;
            }
            let Some(argument) = self.argument(open, close) else { continue };
            let replacement: String = self.chars[argument.clone()].iter().collect();
            let range = start..close + 1;
            let moved = if argument.contains(&caret) || argument.end == caret {
                range.start + caret - argument.start
            } else {
                range.start
            };
            let option = UnwrapOption {
                kind: UnwrapKind::Unwrap,
                construct: Some(format!("{callee}(…)")),
                range: range.clone(),
                replacement,
                kept: vec![argument],
                caret: moved,
            };
            found.push((range, option));
        }
    }

    fn argument(&self, open: usize, close: usize) -> Option<Range<usize>> {
        let mut end = close;
        let mut at = open + 1;
        while at < close {
            if self.skipped[at] {
                at += 1;
            } else if matches!(self.chars[at], '(' | '[' | '{') && self.partner[at] != NONE {
                at = self.partner[at] + 1;
            } else if self.chars[at] == ',' {
                if self.chars[at + 1..close].iter().all(|c| c.is_whitespace()) {
                    end = at;
                    break;
                }
                return None;
            } else {
                at += 1;
            }
        }
        let start = self.skip_blank(open + 1, end);
        while end > start && self.chars[end - 1].is_whitespace() {
            end -= 1;
        }
        (start < end).then_some(start..end)
    }

    fn content(&self, body: &Range<usize>) -> Range<usize> {
        let start = self.skip_blank(body.start, body.end);
        let mut end = body.end;
        while end > start && self.chars[end - 1].is_whitespace() {
            end -= 1;
        }
        start..end
    }

    fn unwrap(&self, construct: Option<String>, range: Range<usize>, body: Range<usize>, caret: usize) -> UnwrapOption {
        if self.starts_line(range.start) && self.ends_line(range.end) {
            let lines = self.line_start(range.start)..self.line_end(range.end);
            return self.unwrap_lines(construct, lines, body, caret);
        }
        let target = self.indent(range.start);
        let lines = self.reflow(&body, &target);
        let mut replacement = String::new();
        let mut caret_at = None;
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                replacement.push_str(self.newline);
            }
            let text = if index == 0 { line.text.trim_start() } else { line.text.as_str() };
            let shift = line.text.chars().count() - text.chars().count();
            if let Some((source, len)) = line.source
                && (source..=source + len).contains(&caret)
            {
                caret_at = Some(replacement.chars().count() + caret - source + line.text.chars().count() - len - shift);
            }
            replacement.push_str(text);
        }
        let kept = vec![self.content(&body)].into_iter().filter(|range| !range.is_empty()).collect();
        let caret = range.start + caret_at.unwrap_or(0);
        UnwrapOption { kind: UnwrapKind::Unwrap, construct, range, replacement, kept, caret }
    }

    fn unwrap_lines(
        &self,
        construct: Option<String>,
        lines: Range<usize>,
        body: Range<usize>,
        caret: usize,
    ) -> UnwrapOption {
        let target = self.indent(lines.start);
        let reflowed = self.reflow(&body, &target);
        let mut range = lines;
        let mut replacement = String::new();
        let mut caret_at = None;
        for (index, line) in reflowed.iter().enumerate() {
            if index > 0 {
                replacement.push_str(self.newline);
            }
            if let Some((source, len)) = line.source
                && (source..=source + len).contains(&caret)
            {
                caret_at = Some(replacement.chars().count() + line.text.chars().count() - len + caret - source);
            }
            replacement.push_str(&line.text);
        }
        if reflowed.is_empty() {
            if range.end < self.chars.len() {
                range.end = self.line_end(range.end) + 1;
                while range.end < self.chars.len() && self.chars[range.end - 1] != '\n' {
                    range.end += 1;
                }
            } else if range.start > 0 {
                range.start -= 1;
                if range.start > 0 && self.chars[range.start - 1] == '\r' {
                    range.start -= 1;
                }
            }
        }
        let first =
            reflowed.first().map_or(0, |line| line.text.chars().count() - line.text.trim_start().chars().count());
        let caret = range.start + caret_at.unwrap_or(first);
        let kept = vec![self.content(&body)].into_iter().filter(|range| !range.is_empty()).collect();
        UnwrapOption { kind: UnwrapKind::Unwrap, construct, range, replacement, kept, caret }
    }

    fn reflow(&self, body: &Range<usize>, target: &str) -> Vec<Line> {
        let mut segments: Vec<(Range<usize>, bool)> = Vec::new();
        let mut start = body.start;
        let mut first = true;
        for at in body.clone() {
            if self.chars[at] == '\n' {
                let end = if at > start && self.chars[at - 1] == '\r' { at - 1 } else { at };
                segments.push((start..end, first));
                first = false;
                start = at + 1;
            }
        }
        segments.push((start..body.end, first));
        let blank = |range: &Range<usize>| self.chars[range.clone()].iter().all(|c| c.is_whitespace());
        if segments.first().is_some_and(|(range, _)| blank(range)) {
            segments.remove(0);
        }
        if segments.last().is_some_and(|(range, _)| blank(range)) {
            segments.pop();
        }
        let quoted = |range: &Range<usize>| range.start > 0 && self.skipped[range.start - 1];
        let leading = |range: &Range<usize>| self.chars[range.clone()].iter().take_while(|&&c| is_blank(c)).count();
        let base = segments
            .iter()
            .find(|(range, inline)| !inline && !blank(range) && !quoted(range))
            .map_or(0, |(range, _)| leading(range));
        segments
            .into_iter()
            .map(|(range, inline)| {
                if !inline && quoted(&range) {
                    let text = self.chars[range.clone()].iter().collect();
                    return Line { text, source: Some((range.start, range.len())) };
                }
                if blank(&range) {
                    return Line { text: String::new(), source: None };
                }
                let mut end = range.end;
                while end > range.start && self.chars[end - 1].is_whitespace() && !self.skipped[end - 1] {
                    end -= 1;
                }
                let from =
                    if inline || leading(&range) < base { range.start + leading(&range) } else { range.start + base };
                let rest: String = self.chars[from..end].iter().collect();
                Line { text: format!("{target}{rest}"), source: Some((from, end - from)) }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, mark: &str) -> usize {
        text[..text.find(mark).unwrap()].chars().count()
    }

    fn apply(text: &str, option: &UnwrapOption) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out: String = chars[..option.range.start].iter().collect();
        out.push_str(&option.replacement);
        out.extend(&chars[option.range.end..]);
        out
    }

    fn titles(options: &[UnwrapOption]) -> Vec<String> {
        options
            .iter()
            .map(|option| {
                let kind = if option.kind == UnwrapKind::Unwrap { "Unwrap" } else { "Remove" };
                match &option.construct {
                    Some(construct) => format!("{kind} '{construct}'"),
                    None => format!("{kind} braces"),
                }
            })
            .collect()
    }

    const GO: &str = "func f() {\n\tfor _, x := range xs {\n\t\tif x > 0 {\n\t\t\tfmt.Println(x)\n\t\t\tg()\n\t\t} else if x < 0 {\n\t\t\th()\n\t\t} else {\n\t\t\tk()\n\t\t}\n\t}\n}\n";

    #[test]
    fn the_constructs_around_the_caret_come_innermost_first() {
        let options = unwrap_options(GO, &[], at(GO, "Println") + 2);
        assert_eq!(titles(&options), ["Unwrap 'fmt.Println(…)'", "Unwrap 'if…'", "Unwrap 'for…'"]);
        let options = unwrap_options(GO, &[], at(GO, "k()"));
        assert_eq!(titles(&options), ["Unwrap 'else…'", "Remove 'else…'", "Unwrap 'for…'"]);
    }

    #[test]
    fn unwrapping_an_if_keeps_its_body_reindented() {
        let options = unwrap_options(GO, &[], at(GO, "g()"));
        let unwrapped = apply(GO, &options[0]);
        assert_eq!(
            unwrapped, "func f() {\n\tfor _, x := range xs {\n\t\tfmt.Println(x)\n\t\tg()\n\t}\n}\n",
            "the else branches go with the if"
        );
        assert_eq!(unwrapped.chars().skip(options[0].caret).take(3).collect::<String>(), "g()");
        assert_eq!(options[0].kept.len(), 1);
        let kept: String = GO.chars().skip(options[0].kept[0].start).take(options[0].kept[0].len()).collect();
        assert_eq!(kept, "fmt.Println(x)\n\t\t\tg()");
    }

    #[test]
    fn else_branches_unwrap_or_go() {
        let options = unwrap_options(GO, &[], at(GO, "h()"));
        assert_eq!(titles(&options)[..2], ["Unwrap 'else…'", "Remove 'else…'"]);
        assert_eq!(apply(GO, &options[0]), "func f() {\n\tfor _, x := range xs {\n\t\th()\n\t}\n}\n");
        assert_eq!(
            apply(GO, &options[1]),
            "func f() {\n\tfor _, x := range xs {\n\t\tif x > 0 {\n\t\t\tfmt.Println(x)\n\t\t\tg()\n\t\t} else {\n\t\t\tk()\n\t\t}\n\t}\n}\n"
        );
        let options = unwrap_options(GO, &[], at(GO, "k()"));
        assert_eq!(
            apply(GO, &options[1]),
            "func f() {\n\tfor _, x := range xs {\n\t\tif x > 0 {\n\t\t\tfmt.Println(x)\n\t\t\tg()\n\t\t} else if x < 0 {\n\t\t\th()\n\t\t}\n\t}\n}\n"
        );
    }

    #[test]
    fn a_loop_unwraps_to_its_body() {
        let options = unwrap_options(GO, &[], at(GO, "range"));
        assert_eq!(titles(&options), ["Unwrap 'for…'"]);
        assert_eq!(
            apply(GO, &options[0]),
            "func f() {\n\tif x > 0 {\n\t\tfmt.Println(x)\n\t\tg()\n\t} else if x < 0 {\n\t\th()\n\t} else {\n\t\tk()\n\t}\n}\n"
        );
    }

    #[test]
    fn switch_cases_unwrap_or_go() {
        let text = "func f() {\n\tswitch x {\n\tcase 1, 2:\n\t\ta()\n\t\tb()\n\tdefault:\n\t\tc()\n\t}\n}\n";
        let options = unwrap_options(text, &[], at(text, "b()"));
        assert_eq!(titles(&options), ["Unwrap 'case…'", "Remove 'case…'"]);
        assert_eq!(apply(text, &options[0]), "func f() {\n\ta()\n\tb()\n}\n");
        assert_eq!(apply(text, &options[1]), "func f() {\n\tswitch x {\n\tdefault:\n\t\tc()\n\t}\n}\n");
        let options = unwrap_options(text, &[], at(text, "c()"));
        assert_eq!(titles(&options), ["Unwrap 'default…'", "Remove 'default…'"]);
        assert_eq!(apply(text, &options[1]), "func f() {\n\tswitch x {\n\tcase 1, 2:\n\t\ta()\n\t\tb()\n\t}\n}\n");
        let select = "func f() {\n\tselect {\n\tcase v := <-ch:\n\t\tuse(v)\n\t}\n}\n";
        let options = unwrap_options(select, &[], at(select, "use"));
        assert_eq!(apply(select, &options[1]), "func f() {\n\tuse(v)\n}\n");
        assert_eq!(titles(&options), ["Unwrap 'use(…)'", "Unwrap 'case…'", "Remove 'case…'"]);
    }

    #[test]
    fn a_function_literal_unwraps_its_statement() {
        let text = "func f() {\n\tgo func() {\n\t\twork()\n\t}()\n\tdone()\n}\n";
        let options = unwrap_options(text, &[], at(text, "work"));
        assert_eq!(titles(&options), ["Unwrap 'func…'"]);
        assert_eq!(apply(text, &options[0]), "func f() {\n\twork()\n\tdone()\n}\n");
        let test = "func T(t *testing.T) {\n\tt.Run(\"a\", func(t *testing.T) {\n\t\tcheck(t)\n\t})\n}\n";
        let string = at(test, "\"a\"")..at(test, "\"a\"") + 3;
        let options = unwrap_options(test, std::slice::from_ref(&string), at(test, "check"));
        assert_eq!(titles(&options), ["Unwrap 'check(…)'", "Unwrap 'func…'"]);
        assert_eq!(apply(test, &options[1]), "func T(t *testing.T) {\n\tcheck(t)\n}\n");
        let assigned = "func f() {\n\tg := func() (int, error) {\n\t\treturn 1, nil\n\t}\n}\n";
        let options = unwrap_options(assigned, &[], at(assigned, "func()"));
        assert_eq!(apply(assigned, &options[0]), "func f() {\n\treturn 1, nil\n}\n");
        let declaration = "func main() {\n\tx()\n}\n";
        assert!(unwrap_options(declaration, &[], at(declaration, "x")).is_empty(), "neither the declaration nor x()");
    }

    #[test]
    fn bare_braces_unwrap() {
        let text = "func f() {\n\t{\n\t\ta := 1\n\t\tuse(a)\n\t}\n}\n";
        let options = unwrap_options(text, &[], at(text, "a :="));
        assert_eq!(titles(&options), ["Unwrap braces"]);
        assert_eq!(apply(text, &options[0]), "func f() {\n\ta := 1\n\tuse(a)\n}\n");
        let literal = "x := T{\n\ta: 1,\n}\n";
        assert!(unwrap_options(literal, &[], at(literal, "a:")).is_empty(), "a composite literal is not a block");
    }

    #[test]
    fn a_call_with_one_argument_unwraps_to_it() {
        let text = "v := strings.TrimSpace(read(f, 1))\n";
        let options = unwrap_options(text, &[], at(text, "read") + 1);
        assert_eq!(titles(&options), ["Unwrap 'strings.TrimSpace(…)'"], "read has two arguments");
        assert_eq!(apply(text, &options[0]), "v := read(f, 1)\n");
        assert_eq!(options[0].caret, at(text, "read") + 1 - "strings.TrimSpace(".chars().count());
        let trailing = "f(\n\tx,\n)\n";
        assert_eq!(apply(trailing, &unwrap_options(trailing, &[], 3)[0]), "x\n");
    }

    #[test]
    fn strings_and_comments_hide_their_brackets() {
        let text = "if a == \"}\" { // {\n\tb()\n}\n";
        let skipped = [at(text, "\"}\"")..at(text, "\"}\"") + 3, at(text, "//")..at(text, "\n")];
        let options = unwrap_options(text, &skipped, at(text, "b()"));
        assert_eq!(titles(&options), ["Unwrap 'if…'"]);
        assert_eq!(apply(text, &options[0]), "// {\nb()\n", "a comment after the brace stays");
        let string = 2..5;
        assert!(
            unwrap_options("f(\")\")", std::slice::from_ref(&string), 3)
                .iter()
                .all(|option| option.replacement == "\")\"")
        );
    }

    #[test]
    fn a_one_line_construct_and_an_empty_body() {
        let text = "\tif ok { run(x) }\n";
        let options = unwrap_options(text, &[], at(text, "ok"));
        assert_eq!(apply(text, &options[0]), "\trun(x)\n");
        let empty = "a()\nif ok {\n}\nb()\n";
        let options = unwrap_options(empty, &[], at(empty, "ok"));
        assert_eq!(apply(empty, &options[0]), "a()\nb()\n");
        let init = "\tif err := f(); err != nil {\n\t\treturn err\n\t}\n\tfor i := 0; i < n; i++ {\n\t\tstep(i)\n\t}\n";
        assert_eq!(
            apply(init, &unwrap_options(init, &[], at(init, "return"))[0]),
            "\treturn err\n\tfor i := 0; i < n; i++ {\n\t\tstep(i)\n\t}\n"
        );
        let options = unwrap_options(init, &[], at(init, "step") + 1);
        assert_eq!(titles(&options), ["Unwrap 'step(…)'", "Unwrap 'for…'"]);
        let inline = "x := 1; if ok { a() }; y()";
        let options = unwrap_options(inline, &[], at(inline, "ok"));
        assert_eq!(apply(inline, &options[0]), "x := 1; a(); y()");
    }

    #[test]
    fn other_languages_share_the_shapes() {
        let rust = "fn f() {\n    while go() {\n        step();\n    }\n}\n";
        let options = unwrap_options(rust, &[], at(rust, "step"));
        assert_eq!(titles(&options), ["Unwrap 'while…'"]);
        assert_eq!(apply(rust, &options[0]), "fn f() {\n    step();\n}\n");
        let js = "if (a == b) {\r\n  c();\r\n}\r\n";
        let options = unwrap_options(js, &[], at(js, "c()"));
        assert_eq!(titles(&options), ["Unwrap 'if…'"]);
        assert_eq!(apply(js, &options[0]), "c();\r\n");
    }
}

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    String,
    Comment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StructureOptions {
    pub whitespace: bool,
    pub plain: bool,
}

pub struct Structure {
    chars: Vec<char>,
    tokens: Vec<Token>,
    options: StructureOptions,
}

const OPENING: [char; 3] = ['(', '[', '{'];
const CLOSING: [char; 3] = [')', ']', '}'];
const QUOTES: [char; 3] = ['"', '\'', '`'];

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_mark(c: char) -> bool {
    matches!(
        c,
        '\u{300}'..='\u{36F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn closing_of(open: char) -> Option<char> {
    OPENING.iter().position(|&c| c == open).map(|index| CLOSING[index])
}

fn opening_of(close: char) -> Option<char> {
    CLOSING.iter().position(|&c| c == close).map(|index| OPENING[index])
}

fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn crosses(a: &Range<usize>, b: &Range<usize>) -> bool {
    (a.start < b.start && b.start < a.end && a.end < b.end) || (b.start < a.start && a.start < b.end && b.end < a.end)
}

fn agreeing(units: &mut Vec<Range<usize>>, server: &[Range<usize>]) {
    units.retain(|unit| !server.iter().any(|range| crosses(unit, range)));
}

impl Structure {
    pub fn new(text: &str, tokens: Vec<Token>, options: StructureOptions) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let mut tokens: Vec<Token> = tokens
            .into_iter()
            .map(|mut token| {
                token.range.end = token.range.end.min(chars.len());
                if token.kind == TokenKind::Comment {
                    while token.range.end > token.range.start && matches!(chars[token.range.end - 1], '\n' | '\r') {
                        token.range.end -= 1;
                    }
                }
                token
            })
            .filter(|token| token.range.start < token.range.end)
            .collect();
        tokens.sort_by_key(|token| token.range.start);
        Self { chars, tokens, options }
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn units(&self, range: Range<usize>) -> Vec<Range<usize>> {
        let range = range.start.min(self.len())..range.end.min(self.len());
        let mut out = Vec::new();
        self.word(&range, &mut out);
        if self.options.plain {
            self.prose(0..self.len(), false, &mut out);
        } else {
            match self.token_around(&range) {
                Some(Token { range: block, kind: TokenKind::Comment }) => {
                    self.prose(block.clone(), true, &mut out);
                    out.push(block.clone());
                    self.documented(&block, &mut out);
                    self.code(block, &mut out);
                }
                Some(token) => {
                    if let Some(content) = self.string_content(token.range.clone()) {
                        out.push(content);
                    }
                    out.push(token.range.clone());
                    self.code(token.range.clone(), &mut out);
                }
                None => {
                    self.touching(&range, &mut out);
                    self.code(range.clone(), &mut out);
                }
            }
        }
        out.push(0..self.len());
        out.retain(|unit| unit.start <= unit.end && contains(unit, &range));
        out.sort_by_key(|unit| (unit.len(), unit.start));
        out.dedup();
        out
    }

    pub fn extend(&self, range: Range<usize>, extra: &[Range<usize>]) -> Option<Range<usize>> {
        let extra: Vec<Range<usize>> = extra.iter().filter(|unit| unit.end <= self.len()).cloned().collect();
        let mut units = self.units(range.clone());
        agreeing(&mut units, &extra);
        units.extend(extra.into_iter().filter(|unit| contains(unit, &range)));
        units.into_iter().filter(|unit| unit.len() > range.len()).min_by_key(|unit| (unit.len(), unit.start))
    }

    pub fn shrink(&self, range: Range<usize>, extra: &[Range<usize>]) -> Option<Range<usize>> {
        if range.is_empty() {
            return None;
        }
        let mut seeds = vec![range.start];
        let mut inner = range.start;
        if OPENING.contains(&self.chars[inner]) || QUOTES.contains(&self.chars[inner]) {
            inner += 1;
        }
        while inner < range.end && self.chars[inner].is_whitespace() {
            inner += 1;
        }
        if inner < range.end {
            seeds.push(inner);
        }
        seeds.push(range.end);
        let extra: Vec<Range<usize>> = extra.iter().filter(|unit| unit.end <= self.len()).cloned().collect();
        let mut found: Vec<Range<usize>> = seeds.into_iter().flat_map(|seed| self.units(seed..seed)).collect();
        agreeing(&mut found, &extra);
        found.extend(extra);
        found.retain(|unit| !unit.is_empty() && contains(&range, unit) && unit.len() < range.len());
        found.into_iter().max_by_key(|unit| (unit.len(), std::cmp::Reverse(unit.start)))
    }

    fn token_at(&self, at: usize) -> Option<&Token> {
        let index = self.tokens.partition_point(|token| token.range.end <= at);
        self.tokens.get(index).filter(|token| token.range.start <= at)
    }

    fn skipped(&self, at: usize) -> bool {
        self.token_at(at).is_some()
    }

    fn token_around(&self, range: &Range<usize>) -> Option<Token> {
        let token = if range.is_empty() {
            let at = range.start;
            let inside = self.token_at(at).filter(|token| token.range.start < at);
            let after = at
                .checked_sub(1)
                .and_then(|before| self.token_at(before))
                .filter(|token| token.kind == TokenKind::Comment && token.range.end == at && token.range.start < at);
            inside.or(after)
        } else {
            self.token_at(range.start)
        }?;
        let whole = match token.kind {
            TokenKind::Comment => self.comment_block(token.range.clone()),
            TokenKind::String => token.range.clone(),
        };
        contains(&whole, range).then_some(Token { range: whole, kind: token.kind })
    }

    fn word(&self, range: &Range<usize>, out: &mut Vec<Range<usize>>) {
        if !self.chars[range.clone()].iter().all(|&c| is_word(c)) {
            return;
        }
        let (start, end) = self.word_bounds(range.start, range.end);
        if start < end {
            out.push(start..end);
        } else if self.options.whitespace && range.is_empty() {
            let mut start = range.start;
            while start > 0 && self.chars[start - 1].is_whitespace() {
                start -= 1;
            }
            let mut end = range.end;
            while end < self.len() && self.chars[end].is_whitespace() {
                end += 1;
            }
            if start < end {
                out.push(start..end);
            }
        }
    }

    fn word_bounds(&self, from: usize, to: usize) -> (usize, usize) {
        let mut start = from;
        loop {
            while start > 0 && is_word(self.chars[start - 1]) {
                start -= 1;
            }
            let mut marks = start;
            while marks > 0 && is_mark(self.chars[marks - 1]) {
                marks -= 1;
            }
            if marks < start && marks > 0 && is_word(self.chars[marks - 1]) {
                start = marks;
            } else {
                break;
            }
        }
        let mut end = to;
        loop {
            while end < self.len() && is_word(self.chars[end]) {
                end += 1;
            }
            if end > 0
                && end < self.len()
                && is_mark(self.chars[end])
                && (is_word(self.chars[end - 1]) || is_mark(self.chars[end - 1]))
            {
                while end < self.len() && is_mark(self.chars[end]) {
                    end += 1;
                }
            } else {
                break;
            }
        }
        (start, end)
    }

    fn splits_word(&self, at: usize) -> bool {
        let (start, end) = self.word_bounds(at, at);
        start < at && at < end
    }

    fn touching(&self, range: &Range<usize>, out: &mut Vec<Range<usize>>) {
        if !range.is_empty() {
            return;
        }
        let at = range.start;
        if at < self.len()
            && OPENING.contains(&self.chars[at])
            && !self.skipped(at)
            && let Some(close) = self.match_forward(at)
        {
            out.push(at..close + 1);
        }
        if at > 0
            && CLOSING.contains(&self.chars[at - 1])
            && !self.skipped(at - 1)
            && let Some(open) = self.match_backward(at - 1)
        {
            out.push(open..at);
        }
    }

    fn string_content(&self, token: Range<usize>) -> Option<Range<usize>> {
        let mut open = token.start;
        while open < token.end && !QUOTES.contains(&self.chars[open]) {
            open += 1;
        }
        if open >= token.end {
            return None;
        }
        let quote = self.chars[open];
        let triple = token.len() >= 6 && self.chars[open..(open + 3).min(token.end)].iter().all(|&c| c == quote);
        let width = if triple { 3 } else { 1 };
        let start = open + width;
        let mut end = token.end;
        while end > start && self.chars[end - 1] == '#' {
            end -= 1;
        }
        if end < start + width || self.chars[end - width..end].iter().any(|&c| c != quote) {
            return None;
        }
        Some(start..end - width)
    }

    fn comment_block(&self, token: Range<usize>) -> Range<usize> {
        let comments: Vec<&Token> = self.tokens.iter().filter(|token| token.kind == TokenKind::Comment).collect();
        let Some(index) = comments.iter().position(|candidate| candidate.range == token) else { return token };
        let joins = |before: &Range<usize>, after: &Range<usize>| {
            let gap = &self.chars[before.end..after.start];
            gap.iter().all(|c| c.is_whitespace()) && gap.iter().filter(|&&c| c == '\n').count() <= 1
        };
        let mut first = index;
        while first > 0 && joins(&comments[first - 1].range, &comments[first].range) {
            first -= 1;
        }
        let mut last = index;
        while last + 1 < comments.len() && joins(&comments[last].range, &comments[last + 1].range) {
            last += 1;
        }
        comments[first].range.start..comments[last].range.end
    }

    fn line_content(&self, line: Range<usize>, markers: bool) -> Range<usize> {
        let mut start = line.start;
        while start < line.end && self.chars[start].is_whitespace() {
            start += 1;
        }
        if markers {
            while start < line.end {
                let c = self.chars[start];
                let dash = c == '-' && self.chars.get(start + 1) == Some(&'-');
                if matches!(c, '/' | '*' | '#' | '!' | ';') || dash {
                    start += if dash { 2 } else { 1 };
                } else {
                    break;
                }
            }
        }
        let mut end = line.end;
        if markers && end >= start + 2 && self.chars[end - 2] == '*' && self.chars[end - 1] == '/' {
            end -= 2;
        }
        while end > start && self.chars[end - 1].is_whitespace() {
            end -= 1;
        }
        while start < end && self.chars[start].is_whitespace() {
            start += 1;
        }
        start..end
    }

    fn lines(&self, block: Range<usize>) -> Vec<Range<usize>> {
        let mut lines = Vec::new();
        let mut start = block.start;
        for at in block.clone() {
            if self.chars[at] == '\n' {
                lines.push(start..at);
                start = at + 1;
            }
        }
        lines.push(start..block.end);
        lines
    }

    fn prose(&self, block: Range<usize>, markers: bool, out: &mut Vec<Range<usize>>) {
        let contents: Vec<Range<usize>> =
            self.lines(block).into_iter().map(|line| self.line_content(line, markers)).collect();
        let mut index = 0;
        while index < contents.len() {
            if contents[index].is_empty() {
                index += 1;
                continue;
            }
            let first = index;
            while index + 1 < contents.len() && !contents[index + 1].is_empty() {
                index += 1;
            }
            let paragraph = &contents[first..=index];
            out.push(paragraph[0].start..paragraph[paragraph.len() - 1].end);
            self.sentences(paragraph, out);
            index += 1;
        }
    }

    fn sentences(&self, lines: &[Range<usize>], out: &mut Vec<Range<usize>>) {
        let positions: Vec<usize> = lines.iter().flat_map(Clone::clone).collect();
        let mut start: Option<usize> = None;
        let mut last = 0;
        for (index, &at) in positions.iter().enumerate() {
            let c = self.chars[at];
            if start.is_none() && !c.is_whitespace() {
                start = Some(at);
            }
            if !c.is_whitespace() {
                last = at + 1;
            }
            let next = positions.get(index + 1).map(|&next| (next, self.chars[next]));
            let ends = matches!(c, '.' | '!' | '?')
                && next.is_none_or(|(next, after)| next != at + 1 || after.is_whitespace());
            if let (true, Some(from)) = (ends, start) {
                out.push(from..at + 1);
                start = None;
            }
        }
        if let Some(from) = start {
            out.push(from..last);
        }
    }

    fn match_forward(&self, open_at: usize) -> Option<usize> {
        let mut waiting = vec![closing_of(self.chars[open_at])?];
        let mut at = open_at + 1;
        while at < self.len() {
            if let Some(token) = self.token_at(at) {
                at = token.range.end.max(at + 1);
                continue;
            }
            let c = self.chars[at];
            if let Some(close) = closing_of(c) {
                waiting.push(close);
            } else if opening_of(c).is_some() {
                if waiting.pop() != Some(c) {
                    return None;
                }
                if waiting.is_empty() {
                    return Some(at);
                }
            }
            at += 1;
        }
        None
    }

    fn match_backward(&self, close_at: usize) -> Option<usize> {
        let mut waiting = vec![opening_of(self.chars[close_at])?];
        let mut at = close_at;
        while at > 0 {
            at -= 1;
            if let Some(token) = self.token_at(at) {
                at = token.range.start;
                continue;
            }
            let c = self.chars[at];
            if let Some(open) = opening_of(c) {
                waiting.push(open);
            } else if closing_of(c).is_some() {
                if waiting.pop() != Some(c) {
                    return None;
                }
                if waiting.is_empty() {
                    return Some(at);
                }
            }
        }
        None
    }

    fn enclosing(&self, inner: &Range<usize>) -> Option<(usize, usize)> {
        let mut depth = 0usize;
        let mut at = inner.start;
        while at > 0 {
            at -= 1;
            if let Some(token) = self.token_at(at) {
                at = token.range.start;
                continue;
            }
            let c = self.chars[at];
            if CLOSING.contains(&c) {
                depth += 1;
            } else if OPENING.contains(&c) {
                if depth == 0 {
                    let close = self.match_forward(at)?;
                    return (close >= inner.end).then_some((at, close));
                }
                depth -= 1;
            }
        }
        None
    }

    fn code(&self, start: Range<usize>, out: &mut Vec<Range<usize>>) {
        let mut inner = start;
        loop {
            let group = self.enclosing(&inner);
            let (container, kind) = match group {
                Some((open, close)) => (open + 1..close, Some(self.chars[open])),
                None => (0..self.len(), None),
            };
            self.level(container.clone(), kind, inner.clone(), out);
            let Some((open, close)) = group else { break };
            out.push(self.trimmed(container.clone()));
            if self.options.whitespace {
                out.push(container);
            }
            inner = open..close + 1;
            out.push(inner.clone());
        }
    }

    fn trimmed(&self, range: Range<usize>) -> Range<usize> {
        let mut start = range.start;
        let mut end = range.end;
        while start < end && self.chars[start].is_whitespace() {
            start += 1;
        }
        while end > start && self.chars[end - 1].is_whitespace() {
            end -= 1;
        }
        start..end
    }

    fn previous_visible(&self, at: usize, floor: usize) -> Option<usize> {
        (floor..at).rev().find(|&index| !self.chars[index].is_whitespace())
    }

    fn next_visible(&self, at: usize, ceiling: usize) -> Option<usize> {
        (at..ceiling).find(|&index| !self.chars[index].is_whitespace())
    }

    fn continues(&self, newline: usize, container: &Range<usize>) -> bool {
        let before = self.previous_visible(newline, container.start).filter(|&at| !self.skipped(at));
        let after = self.next_visible(newline + 1, container.end).filter(|&at| !self.skipped(at));
        let doubled = |at: usize| at > container.start && self.chars[at - 1] == self.chars[at];
        let open_end = before.is_some_and(|at| {
            matches!(self.chars[at], '+' | '-' | '*' | '/' | '%' | '=' | '&' | '|' | '^' | '.' | '(' | '[')
                && !(matches!(self.chars[at], '+' | '-') && doubled(at))
        });
        let open_start = after.is_some_and(|at| {
            matches!(self.chars[at], '.' | '?')
                || (matches!(self.chars[at], '&' | '|') && self.chars.get(at + 1) == Some(&self.chars[at]))
        });
        open_end || open_start
    }

    fn separators(&self, container: &Range<usize>, commas: bool, lines: bool) -> Vec<(usize, char)> {
        let mut found = Vec::new();
        let mut depth = 0usize;
        let mut at = container.start;
        while at < container.end {
            if let Some(token) = self.token_at(at) {
                at = token.range.end.max(at + 1);
                continue;
            }
            let c = self.chars[at];
            if OPENING.contains(&c) {
                depth += 1;
            } else if CLOSING.contains(&c) {
                depth = depth.saturating_sub(1);
            } else if depth == 0 && self.separates(at, container, commas, lines) {
                found.push((at, c));
            }
            at += 1;
        }
        found
    }

    fn comma_items(&self, container: &Range<usize>) -> bool {
        let lines: Vec<(usize, char)> = self.separators(container, false, true);
        let mut start = container.start;
        let mut segments = Vec::new();
        for (at, _) in lines.iter().copied().chain(std::iter::once((container.end, '\n'))) {
            let segment = self.trimmed(start..at);
            if !segment.is_empty() {
                segments.push(segment);
            }
            start = at + 1;
        }
        let single_line = !self.chars[container.clone()].contains(&'\n');
        segments.len() > 1 && segments[..segments.len() - 1].iter().all(|segment| self.chars[segment.end - 1] == ',')
            || single_line && self.separators(container, true, false).iter().any(|(_, c)| *c == ',')
    }

    fn level(&self, container: Range<usize>, kind: Option<char>, inner: Range<usize>, out: &mut Vec<Range<usize>>) {
        let list = matches!(kind, Some('(' | '['));
        let commas = list || (kind == Some('{') && self.comma_items(&container));
        let start = self.boundary_before(&container, inner.start, commas, !list);
        let end = match self.boundary_after(&container, inner.end, commas, !list) {
            (at, Some(';')) if !commas => at + 1,
            (at, _) => at,
        };
        let segment = self.trimmed(start..end);
        if contains(&segment, &inner) {
            self.term(&segment, &inner, out);
            out.push(segment.clone());
            if !commas {
                out.push(self.with_doc(segment));
            }
        }
    }

    fn separates(&self, at: usize, container: &Range<usize>, commas: bool, lines: bool) -> bool {
        match self.chars[at] {
            ';' => true,
            ',' => commas,
            '\n' => lines && !self.continues(at, container),
            _ => false,
        }
    }

    fn boundary_before(&self, container: &Range<usize>, from: usize, commas: bool, lines: bool) -> usize {
        let mut at = from;
        while at > container.start {
            let index = at - 1;
            if let Some(token) = self.token_at(index) {
                at = token.range.start.max(container.start);
                continue;
            }
            let c = self.chars[index];
            if CLOSING.contains(&c) {
                match self.match_backward(index) {
                    Some(open) if open >= container.start => at = open,
                    _ => return at,
                }
                continue;
            }
            if OPENING.contains(&c) {
                return at;
            }
            if self.separates(index, container, commas, lines) {
                return at;
            }
            at = index;
        }
        container.start
    }

    fn boundary_after(
        &self,
        container: &Range<usize>,
        from: usize,
        commas: bool,
        lines: bool,
    ) -> (usize, Option<char>) {
        let mut at = from;
        while at < container.end {
            if let Some(token) = self.token_at(at) {
                at = token.range.end.min(container.end).max(at + 1);
                continue;
            }
            let c = self.chars[at];
            if OPENING.contains(&c) {
                match self.match_forward(at) {
                    Some(close) if close < container.end => at = close + 1,
                    _ => return (at, None),
                }
                continue;
            }
            if CLOSING.contains(&c) {
                return (at, None);
            }
            if self.separates(at, container, commas, lines) {
                return (at, Some(c));
            }
            at += 1;
        }
        (container.end, None)
    }

    fn with_doc(&self, segment: Range<usize>) -> Range<usize> {
        let mut start = segment.start;
        loop {
            let mut line_start = start;
            while line_start > 0 && self.chars[line_start - 1] != '\n' {
                line_start -= 1;
            }
            if self.chars[line_start..start].iter().any(|&c| !is_blank(c)) || line_start == 0 {
                return start..segment.end;
            }
            let previous_end = line_start - 1;
            let mut previous_start = previous_end;
            while previous_start > 0 && self.chars[previous_start - 1] != '\n' {
                previous_start -= 1;
            }
            let content = self.trimmed(previous_start..previous_end);
            if content.is_empty() {
                return start..segment.end;
            }
            let comment = self.token_at(content.start).is_some_and(|token| {
                token.kind == TokenKind::Comment && token.range.start <= content.start && token.range.end >= content.end
            });
            let attribute = self.chars[content.start] == '@'
                || (self.chars[content.start] == '#'
                    && self.chars.get(content.start + 1).is_some_and(|&c| c == '[' || c == '!'));
            if !comment && !attribute {
                return start..segment.end;
            }
            start = content.start;
        }
    }

    fn documented(&self, block: &Range<usize>, out: &mut Vec<Range<usize>>) {
        let Some(after) = self.next_visible(block.end, self.len()) else { return };
        if self.skipped(after) || self.chars[block.end..after].iter().filter(|&&c| c == '\n').count() != 1 {
            return;
        }
        let mut units = Vec::new();
        self.code(after..after, &mut units);
        if let Some(declaration) = units
            .into_iter()
            .filter(|unit| unit.start <= block.start && unit.end > block.end)
            .min_by_key(|unit| unit.len())
        {
            out.push(declaration);
        }
    }

    fn term(&self, segment: &Range<usize>, inner: &Range<usize>, out: &mut Vec<Range<usize>>) {
        let mut start = inner.start;
        loop {
            if start <= segment.start {
                break;
            }
            let at = start - 1;
            if let Some(token) = self.token_at(at) {
                if token.kind == TokenKind::String && token.range.start >= segment.start {
                    start = token.range.start;
                    continue;
                }
                break;
            }
            let c = self.chars[at];
            if is_word(c) || is_mark(c) || c == '.' || (c == ':' && at > segment.start && self.chars[at - 1] == ':') {
                start = if c == ':' { at - 1 } else { at };
            } else if c == '!' && at > segment.start && is_word(self.chars[at - 1]) {
                start = at;
            } else if CLOSING.contains(&c) {
                match self.match_backward(at) {
                    Some(open) if open >= segment.start => start = open,
                    _ => break,
                }
            } else {
                break;
            }
        }
        let mut ends = vec![inner.end];
        let mut end = inner.end;
        while end < segment.end {
            if let Some(token) = self.token_at(end) {
                if token.kind != TokenKind::String || token.range.start != end {
                    break;
                }
                end = token.range.end;
                ends.push(end);
                continue;
            }
            let c = self.chars[end];
            if is_word(c) {
                end = self.word_bounds(end, end).1.min(segment.end);
                ends.push(end);
            } else if c == '.' || c == '?' || c == '!' {
                end += 1;
            } else if c == ':' && self.chars.get(end + 1) == Some(&':') {
                end += 2;
            } else if OPENING.contains(&c) {
                match self.match_forward(end) {
                    Some(close) if close < segment.end => {
                        end = close + 1;
                        ends.push(end);
                    }
                    _ => break,
                }
            } else {
                break;
            }
        }
        if self.chars[start..inner.start].iter().all(|&c| !is_word(c) && c != '.' && c != ':')
            && ends.iter().all(|&end| end == inner.end)
        {
            return;
        }
        for end in ends {
            if end > start && self.chars[end - 1] != '.' && !self.splits_word(end) {
                out.push(start..end);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(text: &str) -> Vec<Token> {
        let chars: Vec<char> = text.chars().collect();
        let mut tokens = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            let start = at;
            if chars[at] == '/' && chars.get(at + 1) == Some(&'/') {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
                tokens.push(Token { range: start..at, kind: TokenKind::Comment });
            } else if chars[at] == '/' && chars.get(at + 1) == Some(&'*') {
                at += 2;
                while at + 1 < chars.len() && !(chars[at] == '*' && chars[at + 1] == '/') {
                    at += 1;
                }
                at = (at + 2).min(chars.len());
                tokens.push(Token { range: start..at, kind: TokenKind::Comment });
            } else if chars[at] == '"' || chars[at] == '`' {
                let quote = chars[at];
                at += 1;
                while at < chars.len() && chars[at] != quote {
                    if chars[at] == '\\' {
                        at += 1;
                    }
                    at += 1;
                }
                at = (at + 1).min(chars.len());
                tokens.push(Token { range: start..at, kind: TokenKind::String });
            } else {
                at += 1;
            }
        }
        tokens
    }

    fn steps(text: &str, caret: &str, options: StructureOptions) -> Vec<String> {
        let structure = Structure::new(text, lex(text), options);
        let at = text.find(caret).expect("the caret's mark is in the text");
        let mut range = text[..at].chars().count()..text[..at].chars().count();
        let mut seen = Vec::new();
        while let Some(next) = structure.extend(range.clone(), &[]) {
            seen.push(text.chars().skip(next.start).take(next.len()).collect());
            range = next;
        }
        seen
    }

    fn code(text: &str, caret: &str) -> Vec<String> {
        steps(text, caret, StructureOptions::default())
    }

    const GO: &str = "package main\n\nimport \"fmt\"\n\n// Greet says hello.\n// It prints the name.\nfunc Greet(name string, times int) {\n\tif times > 0 {\n\t\tfmt.Println(\"hello, world\", name)\n\t\tcount++\n\t}\n}\n";

    #[test]
    fn go_steps_from_a_string_out_to_the_file() {
        let at = GO.find("world").unwrap();
        let caret = &GO[at..];
        let steps = code(GO, caret);
        assert_eq!(
            steps,
            [
                "world",
                "hello, world",
                "\"hello, world\"",
                "\"hello, world\", name",
                "(\"hello, world\", name)",
                "fmt.Println(\"hello, world\", name)",
                "fmt.Println(\"hello, world\", name)\n\t\tcount++",
                "{\n\t\tfmt.Println(\"hello, world\", name)\n\t\tcount++\n\t}",
                "if times > 0 {\n\t\tfmt.Println(\"hello, world\", name)\n\t\tcount++\n\t}",
                "{\n\tif times > 0 {\n\t\tfmt.Println(\"hello, world\", name)\n\t\tcount++\n\t}\n}",
                "func Greet(name string, times int) {\n\tif times > 0 {\n\t\tfmt.Println(\"hello, world\", name)\n\t\tcount++\n\t}\n}",
                &GO[GO.find("// Greet").unwrap()..GO.len() - 1],
                GO,
            ]
        );
    }

    #[test]
    fn go_arguments_and_selectors_go_link_by_link() {
        let text = "package p\n\nfunc f() {\n\tx := a.b.c(one, two)\n}\n";
        assert_eq!(
            code(text, "b.c")[..5],
            ["b".to_owned(), "a.b".into(), "a.b.c".into(), "a.b.c(one, two)".into(), "x := a.b.c(one, two)".into()]
        );
        assert_eq!(
            code(text, "two")[..4],
            ["two".to_owned(), "one, two".into(), "(one, two)".into(), "a.b.c(one, two)".into()]
        );
    }

    #[test]
    fn comments_step_by_word_sentence_paragraph_and_the_whole_comment() {
        let text = "// First sentence here. Second one\n// goes on.\n//\n// Another paragraph.\nfunc f() {}\n";
        let steps = code(text, "one\n");
        assert_eq!(steps[0], "one");
        assert_eq!(steps[1], "Second one\n// goes on.");
        assert_eq!(steps[2], "First sentence here. Second one\n// goes on.");
        assert_eq!(steps[3], "// First sentence here. Second one\n// goes on.\n//\n// Another paragraph.");
        assert_eq!(steps[4], text.trim_end(), "then the declaration with its doc comment");
        assert_eq!(code(text, "First")[..2], ["First".to_owned(), "First sentence here.".into()]);
    }

    #[test]
    fn plain_text_steps_by_word_sentence_paragraph_and_the_file() {
        let text = "One two. Three (four) five.\nStill here.\n\nNext paragraph.";
        let steps = steps(text, "four", StructureOptions { plain: true, ..StructureOptions::default() });
        assert_eq!(steps, ["four", "Three (four) five.", "One two. Three (four) five.\nStill here.", text]);
    }

    const RUST: &str = "use std::fmt;\n\n/// A point.\n#[derive(Debug)]\nstruct Point {\n    x: i32,\n    y: i32,\n}\n\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    println!(\"{}\", p.x.max(3));\n}\n";

    #[test]
    fn rust_steps_through_macros_calls_and_blocks() {
        let steps = code(RUST, "max");
        assert_eq!(
            steps[..6],
            [
                "max".to_owned(),
                "p.x.max".into(),
                "p.x.max(3)".into(),
                "\"{}\", p.x.max(3)".into(),
                "(\"{}\", p.x.max(3))".into(),
                "println!(\"{}\", p.x.max(3))".into(),
            ]
        );
        assert_eq!(steps[6], "println!(\"{}\", p.x.max(3));");
        assert!(steps.contains(
            &"fn main() {\n    let p = Point { x: 1, y: 2 };\n    println!(\"{}\", p.x.max(3));\n}".to_owned()
        ));
        assert_eq!(
            code(RUST, "1, y")[..4],
            ["1".to_owned(), "x: 1".into(), "x: 1, y: 2".into(), "{ x: 1, y: 2 }".into()]
        );
    }

    #[test]
    fn rust_declarations_take_their_doc_comment_and_attributes() {
        let steps = code(RUST, "y: i32");
        assert_eq!(steps[..3], ["y".to_owned(), "y: i32".into(), "x: i32,\n    y: i32,".into()]);
        let structure = "struct Point {\n    x: i32,\n    y: i32,\n}";
        let at = steps.iter().position(|step| step == structure).expect("the struct");
        assert_eq!(steps[at + 1], format!("/// A point.\n#[derive(Debug)]\n{structure}"));
    }

    const JSON: &str = "{\n  \"name\": \"zen\",\n  \"tags\": [\"ide\", \"rust\"],\n  \"nested\": { \"on\": true }\n}\n";

    #[test]
    fn json_steps_through_values_members_and_objects() {
        assert_eq!(
            code(JSON, "rust"),
            [
                "rust",
                "\"rust\"",
                "\"ide\", \"rust\"",
                "[\"ide\", \"rust\"]",
                "\"tags\": [\"ide\", \"rust\"]",
                "\"name\": \"zen\",\n  \"tags\": [\"ide\", \"rust\"],\n  \"nested\": { \"on\": true }",
                JSON.trim_end(),
                JSON,
            ]
        );
        assert_eq!(code(JSON, "true")[..3], ["true".to_owned(), "\"on\": true".into(), "{ \"on\": true }".into()]);
    }

    #[test]
    fn a_caret_next_to_a_bracket_takes_its_group() {
        let text = "f(a) {\n\tb\n}";
        assert_eq!(code(text, "{")[0], "{\n\tb\n}");
        let after = text.len();
        let structure = Structure::new(text, Vec::new(), StructureOptions::default());
        assert_eq!(structure.extend(after..after, &[]), Some(5..after));
    }

    #[test]
    fn whitespace_is_a_unit_when_asked() {
        let text = "a   b";
        let on = StructureOptions { whitespace: true, ..StructureOptions::default() };
        assert_eq!(Structure::new(text, Vec::new(), on).extend(2..2, &[]), Some(1..4));
        assert_eq!(Structure::new(text, Vec::new(), StructureOptions::default()).extend(2..2, &[]), Some(0..5));
    }

    #[test]
    fn shrinking_a_selection_made_by_hand_takes_the_next_smaller_unit() {
        let text = "func f() {\n\tg(a, b)\n}\n";
        let structure = Structure::new(text, lex(text), StructureOptions::default());
        let block = text.find('{').unwrap()..text.rfind('}').unwrap() + 1;
        assert_eq!(structure.shrink(block, &[]), Some(12..19), "the block's content");
        let call = 12..19;
        assert_eq!(structure.shrink(call, &[]), Some(13..19), "the argument list");
        assert_eq!(structure.shrink(13..14, &[]), None, "nothing is smaller than a bracket");
    }

    fn first_step(text: &str, caret: usize) -> String {
        let structure = Structure::new(text, lex(text), StructureOptions::default());
        let unit = structure.extend(caret..caret, &[]).expect("a unit");
        text.chars().skip(unit.start).take(unit.len()).collect()
    }

    #[test]
    fn a_caret_inside_a_word_takes_the_whole_word() {
        let text = "package p\n\nfunc Greet() {\n\tfmt.Println(name)\n}\n";
        let at = |mark: &str, inside: usize| text.find(mark).unwrap() + inside;
        assert_eq!(first_step(text, at("func", 2)), "func");
        assert_eq!(first_step(text, at("Greet", 3)), "Greet");
        assert_eq!(first_step(text, at("fmt", 1)), "fmt");
        assert_eq!(first_step(text, at("Println", 4)), "Println");
        assert_eq!(code(text, "rintln")[1], "fmt.Println");
    }

    #[test]
    fn combining_marks_stay_with_their_word() {
        let text = "x := \"cafe\u{301} 1\u{FE0F}\u{20E3} ok\"";
        let chars: Vec<char> = text.chars().collect();
        let at = |c: char| chars.iter().position(|&found| found == c).unwrap();
        assert_eq!(first_step(text, at('c') + 1), "cafe\u{301}");
        assert_eq!(first_step(text, at('1')), "1\u{FE0F}\u{20E3}");
        assert_eq!(first_step(text, at('\u{301}')), "cafe\u{301}");
    }

    #[test]
    fn crlf_lines_step_like_lf_lines() {
        let text = "package p\r\n\r\n// Add adds.\r\nfunc add(a, b int) int {\r\n\treturn a.plus(b)\r\n}\r\n";
        let steps = code(text, "plus");
        assert_eq!(steps[..4], ["plus".to_owned(), "a.plus".into(), "a.plus(b)".into(), "return a.plus(b)".into()]);
        assert_eq!(steps[4], "{\r\n\treturn a.plus(b)\r\n}");
        assert_eq!(steps[6], "// Add adds.\r\nfunc add(a, b int) int {\r\n\treturn a.plus(b)\r\n}");
    }

    #[test]
    fn a_unit_that_cuts_across_a_server_range_gives_way() {
        let text = "x := a.b + c";
        let structure = Structure::new(text, Vec::new(), StructureOptions::default());
        let b = text.find("b ").unwrap();
        assert_eq!(structure.extend(b..b + 1, &[]), Some(5..8), "the text alone: a.b");
        assert_eq!(structure.extend(b..b + 1, &[7..12, 5..12]), Some(7..12), "a.b crosses the server's b + c");
        assert_eq!(structure.shrink(0..12, &[7..12, 5..12]), Some(5..12));
    }

    #[test]
    fn extra_ranges_join_the_units() {
        let text = "a + b";
        let structure = Structure::new(text, Vec::new(), StructureOptions::default());
        assert_eq!(structure.extend(0..1, &[0..3, 0..5]), Some(0..3));
        assert_eq!(
            structure.extend(0..1, &[2..5, 3..5]),
            Some(0..5),
            "a range that does not hold the selection is ignored"
        );
    }
}

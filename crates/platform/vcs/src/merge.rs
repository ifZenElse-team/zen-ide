use std::ops::Range;

use crate::diff::{self, lines};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkKind {
    Same,
    Ours,
    Theirs,
    Both,
    Conflict,
}

impl ChunkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChunkKind::Same => "same",
            ChunkKind::Ours => "ours",
            ChunkKind::Theirs => "theirs",
            ChunkKind::Both => "both",
            ChunkKind::Conflict => "conflict",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub kind: ChunkKind,
    pub base: Range<usize>,
    pub ours: Range<usize>,
    pub theirs: Range<usize>,
}

pub fn chunks(base: &str, ours: &str, theirs: &str) -> Vec<Chunk> {
    let (ours_hunks, theirs_hunks) = (diff::hunks(base, ours), diff::hunks(base, theirs));
    let (base_lines, ours_lines, theirs_lines) = (lines(base), lines(ours), lines(theirs));
    let mut chunks = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut base_at, mut ours_at, mut theirs_at) = (0, 0, 0);
    loop {
        let start = match (ours_hunks.get(i), theirs_hunks.get(j)) {
            (None, None) => break,
            (Some(a), None) => a.before.start,
            (None, Some(b)) => b.before.start,
            (Some(a), Some(b)) => a.before.start.min(b.before.start),
        };
        let (first_ours, first_theirs) = (i, j);
        let mut end = start;
        loop {
            let mut grew = false;
            while let Some(hunk) = ours_hunks.get(i).filter(|hunk| hunk.before.start <= end) {
                end = end.max(hunk.before.end);
                i += 1;
                grew = true;
            }
            while let Some(hunk) = theirs_hunks.get(j).filter(|hunk| hunk.before.start <= end) {
                end = end.max(hunk.before.end);
                j += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        if start > base_at {
            let length = start - base_at;
            chunks.push(Chunk {
                kind: ChunkKind::Same,
                base: base_at..start,
                ours: ours_at..ours_at + length,
                theirs: theirs_at..theirs_at + length,
            });
            ours_at += length;
            theirs_at += length;
        }
        let side = |hunks: &[diff::Hunk], at: usize| -> Range<usize> {
            match (hunks.first(), hunks.last()) {
                (Some(first), Some(last)) => {
                    first.after.start - (first.before.start - start)..last.after.end + (end - last.before.end)
                }
                _ => at..at + (end - start),
            }
        };
        let ours_range = side(&ours_hunks[first_ours..i], ours_at);
        let theirs_range = side(&theirs_hunks[first_theirs..j], theirs_at);
        let kind = match (i > first_ours, j > first_theirs) {
            (true, false) => ChunkKind::Ours,
            (false, true) => ChunkKind::Theirs,
            _ if ours_lines[ours_range.clone()] == theirs_lines[theirs_range.clone()] => ChunkKind::Both,
            _ => ChunkKind::Conflict,
        };
        let chunk = Chunk { kind, base: start..end, ours: ours_range.clone(), theirs: theirs_range.clone() };
        if kind == ChunkKind::Conflict {
            chunks.extend(trimmed(chunk, &base_lines, &ours_lines, &theirs_lines));
        } else {
            chunks.push(chunk);
        }
        base_at = end;
        ours_at = ours_range.end;
        theirs_at = theirs_range.end;
    }
    if base_at < base_lines.len() || ours_at < ours_lines.len() || theirs_at < theirs_lines.len() {
        chunks.push(Chunk {
            kind: ChunkKind::Same,
            base: base_at..base_lines.len(),
            ours: ours_at..ours_lines.len(),
            theirs: theirs_at..theirs_lines.len(),
        });
    }
    chunks
}

fn trimmed(chunk: Chunk, base: &[&str], ours: &[&str], theirs: &[&str]) -> Vec<Chunk> {
    let (o, t, b) = (&ours[chunk.ours.clone()], &theirs[chunk.theirs.clone()], &base[chunk.base.clone()]);
    let shortest = o.len().min(t.len());
    let prefix = (0..shortest).take_while(|&k| o[k] == t[k]).count();
    let suffix = (0..shortest - prefix).take_while(|&k| o[o.len() - 1 - k] == t[t.len() - 1 - k]).count();
    if prefix == 0 && suffix == 0 {
        return vec![chunk];
    }
    let base_prefix = if b.len() >= prefix && b[..prefix] == o[..prefix] { prefix } else { 0 };
    let base_suffix =
        if b.len() - base_prefix >= suffix && b[b.len() - suffix..] == o[o.len() - suffix..] { suffix } else { 0 };
    let mut out = Vec::new();
    let (base_start, ours_start, theirs_start) = (chunk.base.start, chunk.ours.start, chunk.theirs.start);
    if prefix > 0 {
        out.push(Chunk {
            kind: if base_prefix > 0 { ChunkKind::Same } else { ChunkKind::Both },
            base: base_start..base_start + base_prefix,
            ours: ours_start..ours_start + prefix,
            theirs: theirs_start..theirs_start + prefix,
        });
    }
    out.push(Chunk {
        kind: ChunkKind::Conflict,
        base: base_start + base_prefix..chunk.base.end - base_suffix,
        ours: ours_start + prefix..chunk.ours.end - suffix,
        theirs: theirs_start + prefix..chunk.theirs.end - suffix,
    });
    if suffix > 0 {
        out.push(Chunk {
            kind: if base_suffix > 0 { ChunkKind::Same } else { ChunkKind::Both },
            base: chunk.base.end - base_suffix..chunk.base.end,
            ours: chunk.ours.end - suffix..chunk.ours.end,
            theirs: chunk.theirs.end - suffix..chunk.theirs.end,
        });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    Auto,
    Unresolved,
    Ours,
    Theirs,
    Both,
    Edited,
}

impl Taken {
    pub fn as_str(self) -> &'static str {
        match self {
            Taken::Auto => "auto",
            Taken::Unresolved => "unresolved",
            Taken::Ours => "ours",
            Taken::Theirs => "theirs",
            Taken::Both => "both",
            Taken::Edited => "edited",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Ours,
    Theirs,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub chunk: Chunk,
    pub taken: Taken,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merge {
    base: String,
    ours: String,
    theirs: String,
    pieces: Vec<Piece>,
}

fn slice(text: &str, range: &Range<usize>) -> String {
    let lines = lines(text);
    lines[range.start.min(lines.len())..range.end.min(lines.len())].concat()
}

fn joined(first: &str, second: &str) -> String {
    if !first.is_empty() && !first.ends_with('\n') && !second.is_empty() {
        format!("{first}\n{second}")
    } else {
        format!("{first}{second}")
    }
}

impl Merge {
    pub fn new(base: &str, ours: &str, theirs: &str) -> Self {
        let pieces = chunks(base, ours, theirs)
            .into_iter()
            .map(|chunk| {
                let (taken, text) = match chunk.kind {
                    ChunkKind::Same => (Taken::Auto, slice(ours, &chunk.ours)),
                    ChunkKind::Ours | ChunkKind::Both => (Taken::Auto, slice(ours, &chunk.ours)),
                    ChunkKind::Theirs => (Taken::Auto, slice(theirs, &chunk.theirs)),
                    ChunkKind::Conflict => (Taken::Unresolved, slice(base, &chunk.base)),
                };
                Piece { chunk, taken, text }
            })
            .collect();
        Merge { base: base.to_owned(), ours: ours.to_owned(), theirs: theirs.to_owned(), pieces }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn ours(&self) -> &str {
        &self.ours
    }

    pub fn theirs(&self) -> &str {
        &self.theirs
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn result(&self) -> String {
        self.pieces.iter().fold(String::new(), |result, piece| joined(&result, &piece.text))
    }

    pub fn result_ranges(&self) -> Vec<Range<usize>> {
        let mut at = 0;
        self.pieces
            .iter()
            .map(|piece| {
                let length = lines(&piece.text).len();
                at += length;
                at - length..at
            })
            .collect()
    }

    pub fn unresolved(&self) -> usize {
        self.pieces.iter().filter(|piece| piece.taken == Taken::Unresolved).count()
    }

    pub fn take(&mut self, index: usize, pick: Pick) -> bool {
        let Some(piece) = self.pieces.get_mut(index) else { return false };
        let ours = slice(&self.ours, &piece.chunk.ours);
        let theirs = slice(&self.theirs, &piece.chunk.theirs);
        (piece.taken, piece.text) = match pick {
            Pick::Ours => (Taken::Ours, ours),
            Pick::Theirs => (Taken::Theirs, theirs),
            Pick::Both => (Taken::Both, joined(&ours, &theirs)),
        };
        true
    }

    pub fn set_piece_text(&mut self, index: usize, text: &str) -> bool {
        if index >= self.pieces.len() {
            return false;
        }
        let ranges = self.result_ranges();
        let result = self.result();
        let lines = lines(&result);
        let range = ranges[index].clone();
        let mut new = lines[..range.start].concat();
        new = joined(&new, text);
        let rest = lines[range.end..].concat();
        new = joined(&new, &rest);
        self.set_result(&new);
        true
    }

    pub fn set_result(&mut self, text: &str) {
        let old = self.result();
        if old == text {
            return;
        }
        let hunks = diff::hunks(&old, text);
        let new_lines = lines(text);
        let mut cuts: Vec<usize> = hunks.iter().flat_map(|hunk| [hunk.before.start, hunk.before.end]).collect();
        cuts.sort_unstable();
        cuts.dedup();
        self.split_same_at(&cuts);
        let ranges = self.result_ranges();
        let mut group_of: Vec<Option<usize>> = vec![None; self.pieces.len()];
        let mut groups: Vec<Group> = Vec::new();
        for (number, hunk) in hunks.iter().enumerate() {
            let (a, b) = (hunk.before.start, hunk.before.end);
            let mut touched: Vec<usize> = ranges
                .iter()
                .enumerate()
                .filter(|(_, range)| {
                    if range.is_empty() { a < range.start && range.start < b } else { range.start < b && a < range.end }
                })
                .map(|(index, _)| index)
                .collect();
            if touched.is_empty() {
                let inside = ranges.iter().position(|range| range.start < a && a < range.end);
                let change = |index: &usize| self.pieces[*index].chunk.kind != ChunkKind::Same;
                let ending = ranges.iter().enumerate().filter(|(_, r)| r.end == a).map(|(i, _)| i).rfind(change);
                let starting = ranges.iter().enumerate().filter(|(_, r)| r.start == a).map(|(i, _)| i).find(change);
                touched.extend(inside.or(ending).or(starting));
            }
            let mut group = Group { pieces: touched.clone(), hunks: vec![number], at: a, alive: true };
            for index in &touched {
                if let Some(other) = group_of[*index]
                    && let Some(old) = groups.get_mut(other).filter(|old| old.alive)
                {
                    old.alive = false;
                    group.pieces.append(&mut old.pieces);
                    group.hunks.append(&mut old.hunks);
                    group.at = group.at.min(old.at);
                }
            }
            group.pieces.sort_unstable();
            group.pieces.dedup();
            let id = groups.len();
            for index in &group.pieces {
                group_of[*index] = Some(id);
            }
            groups.push(group);
        }
        let delta = |hunk: &diff::Hunk| hunk.after.len() as isize - hunk.before.len() as isize;
        let shift = |position: usize, own: &[usize]| -> usize {
            let before: isize = hunks
                .iter()
                .enumerate()
                .filter(|(number, hunk)| !own.contains(number) && hunk.before.end <= position)
                .map(|(_, hunk)| delta(hunk))
                .sum();
            (position as isize + before).max(0) as usize
        };
        let mut pieces: Vec<Piece> = Vec::new();
        let mut index = 0;
        let mut lone: Vec<&Group> = groups.iter().filter(|group| group.alive && group.pieces.is_empty()).collect();
        lone.sort_by_key(|group| group.at);
        let mut lone = lone.into_iter().peekable();
        while index <= self.pieces.len() {
            while let Some(group) = lone.next_if(|group| index == self.pieces.len() || group.at <= ranges[index].start)
            {
                let hunk = &hunks[group.hunks[0]];
                let before = index.checked_sub(1).map(|previous| &self.pieces[previous].chunk);
                let end = |range: Option<&Range<usize>>| range.map_or(0, |range| range.end);
                pieces.push(Piece {
                    chunk: Chunk {
                        kind: ChunkKind::Same,
                        base: end(before.map(|c| &c.base))..end(before.map(|c| &c.base)),
                        ours: end(before.map(|c| &c.ours))..end(before.map(|c| &c.ours)),
                        theirs: end(before.map(|c| &c.theirs))..end(before.map(|c| &c.theirs)),
                    },
                    taken: Taken::Edited,
                    text: new_lines[hunk.after.clone()].concat(),
                });
            }
            if index == self.pieces.len() {
                break;
            }
            let Some(id) = group_of[index] else {
                pieces.push(self.pieces[index].clone());
                index += 1;
                continue;
            };
            let group = &groups[id];
            let (first, last) = (group.pieces[0], *group.pieces.last().unwrap_or(&group.pieces[0]));
            let start = ranges[first].start.min(group.hunks.iter().map(|h| hunks[*h].before.start).min().unwrap_or(0));
            let end = ranges[last].end.max(group.hunks.iter().map(|h| hunks[*h].before.end).max().unwrap_or(0));
            let own: isize = group.hunks.iter().map(|number| delta(&hunks[*number])).sum();
            let new_start = shift(start, &group.hunks);
            let new_end = ((new_start + (end - start)) as isize + own).max(new_start as isize) as usize;
            let joined_pieces = &self.pieces[first..=last];
            let kinds: Vec<ChunkKind> = joined_pieces.iter().map(|piece| piece.chunk.kind).collect();
            let all = |allowed: &[ChunkKind]| kinds.iter().all(|kind| allowed.contains(kind));
            let kind = if kinds.contains(&ChunkKind::Conflict) {
                ChunkKind::Conflict
            } else if all(&[ChunkKind::Same]) {
                ChunkKind::Same
            } else if all(&[ChunkKind::Same, ChunkKind::Ours]) {
                ChunkKind::Ours
            } else if all(&[ChunkKind::Same, ChunkKind::Theirs]) {
                ChunkKind::Theirs
            } else {
                ChunkKind::Both
            };
            let union = |of: fn(&Chunk) -> &Range<usize>| {
                of(&joined_pieces[0].chunk).start..of(&joined_pieces[joined_pieces.len() - 1].chunk).end
            };
            pieces.push(Piece {
                chunk: Chunk { kind, base: union(|c| &c.base), ours: union(|c| &c.ours), theirs: union(|c| &c.theirs) },
                taken: Taken::Edited,
                text: new_lines[new_start.min(new_lines.len())..new_end.min(new_lines.len())].concat(),
            });
            index = last + 1;
        }
        self.pieces = pieces;
        self.join_same();
        if self.result() != text {
            let end = |text: &str| lines(text).len();
            self.pieces = vec![Piece {
                chunk: Chunk {
                    kind: ChunkKind::Conflict,
                    base: 0..end(&self.base),
                    ours: 0..end(&self.ours),
                    theirs: 0..end(&self.theirs),
                },
                taken: Taken::Edited,
                text: text.to_owned(),
            }];
        }
    }

    fn split_same_at(&mut self, cuts: &[usize]) {
        let ranges = self.result_ranges();
        let mut pieces = Vec::with_capacity(self.pieces.len());
        for (piece, range) in self.pieces.drain(..).zip(ranges) {
            if piece.chunk.kind != ChunkKind::Same || piece.taken != Taken::Auto {
                pieces.push(piece);
                continue;
            }
            let text = lines(&piece.text).into_iter().map(str::to_owned).collect::<Vec<_>>();
            let mut from = range.start;
            let mut points: Vec<usize> =
                cuts.iter().copied().filter(|cut| range.start < *cut && *cut < range.end).collect();
            points.push(range.end);
            for to in points {
                let (offset, length) = (from - range.start, to - from);
                let chunk = &piece.chunk;
                pieces.push(Piece {
                    chunk: Chunk {
                        kind: ChunkKind::Same,
                        base: chunk.base.start + offset..chunk.base.start + offset + length,
                        ours: chunk.ours.start + offset..chunk.ours.start + offset + length,
                        theirs: chunk.theirs.start + offset..chunk.theirs.start + offset + length,
                    },
                    taken: Taken::Auto,
                    text: text[offset..offset + length].concat(),
                });
                from = to;
            }
        }
        self.pieces = pieces;
    }

    fn join_same(&mut self) {
        let mut pieces: Vec<Piece> = Vec::with_capacity(self.pieces.len());
        for piece in self.pieces.drain(..) {
            let same = |piece: &Piece| piece.chunk.kind == ChunkKind::Same && piece.taken == Taken::Auto;
            match pieces.last_mut() {
                Some(last) if same(last) && same(&piece) => {
                    last.chunk.base.end = piece.chunk.base.end;
                    last.chunk.ours.end = piece.chunk.ours.end;
                    last.chunk.theirs.end = piece.chunk.theirs.end;
                    last.text = joined(&last.text, &piece.text);
                }
                _ => pieces.push(piece),
            }
        }
        self.pieces = pieces;
    }
}

struct Group {
    pieces: Vec<usize>,
    hunks: Vec<usize>,
    at: usize,
    alive: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(chunks: &[Chunk]) -> Vec<ChunkKind> {
        chunks.iter().map(|chunk| chunk.kind).collect()
    }

    fn changes(merge: &Merge) -> Vec<(ChunkKind, Taken)> {
        merge
            .pieces()
            .iter()
            .filter(|piece| piece.chunk.kind != ChunkKind::Same || piece.taken != Taken::Auto)
            .map(|piece| (piece.chunk.kind, piece.taken))
            .collect()
    }

    #[test]
    fn changes_of_one_side_merge_and_overlapping_ones_conflict() {
        let base = "a\nb\nc\nd\ne\nf\ng\n";
        let ours = "a\nB\nc\nd\nE1\nf\ng\n";
        let theirs = "a\nb\nc\nD\nE2\nf\ng\nh\n";
        let chunks = chunks(base, ours, theirs);
        use ChunkKind::*;
        assert_eq!(kinds(&chunks), [Same, Ours, Same, Conflict, Same, Theirs]);
        let conflict = &chunks[3];
        assert_eq!((conflict.base.clone(), conflict.ours.clone(), conflict.theirs.clone()), (3..5, 3..5, 3..5));
        for (text, of) in [(base, 0), (ours, 1), (theirs, 2)] {
            let mut at = 0;
            for chunk in &chunks {
                let range = [&chunk.base, &chunk.ours, &chunk.theirs][of];
                assert_eq!(range.start, at);
                at = range.end;
            }
            assert_eq!(at, lines(text).len());
        }
        let merge = Merge::new(base, ours, theirs);
        assert_eq!(merge.result(), "a\nB\nc\nd\ne\nf\ng\nh\n", "the base's lines where they conflict");
        assert_eq!(merge.unresolved(), 1);
    }

    #[test]
    fn the_same_change_on_both_sides_is_no_conflict() {
        let merge = Merge::new("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n");
        assert_eq!(changes(&merge), [(ChunkKind::Both, Taken::Auto)]);
        assert_eq!(merge.result(), "a\nX\nc\n");
        assert_eq!(merge.unresolved(), 0);
    }

    #[test]
    fn lines_both_sides_add_around_a_conflict_are_taken_out_of_it() {
        let chunks = chunks("", "package a\n\nfunc A() {}\n\n// end\n", "package a\n\nfunc B() {}\n\n// end\n");
        use ChunkKind::*;
        assert_eq!(kinds(&chunks), [Both, Conflict, Both]);
        assert_eq!((chunks[1].ours.clone(), chunks[1].theirs.clone()), (2..3, 2..3));
        assert_eq!(chunks[1].base, 0..0);
    }

    #[test]
    fn taking_a_side_or_both_and_editing_resolve_conflicts() {
        let base = "a\nb\nc\n";
        let mut merge = Merge::new(base, "a\nOURS\nc\n", "a\nTHEIRS\nc\n");
        let conflict = merge.pieces().iter().position(|piece| piece.taken == Taken::Unresolved).unwrap();
        assert!(merge.take(conflict, Pick::Ours));
        assert_eq!(merge.result(), "a\nOURS\nc\n");
        assert!(merge.take(conflict, Pick::Both));
        assert_eq!(merge.result(), "a\nOURS\nTHEIRS\nc\n");
        assert_eq!(merge.result_ranges()[conflict], 1..3);
        assert!(merge.take(conflict, Pick::Theirs));
        assert_eq!(merge.result(), "a\nTHEIRS\nc\n");
        assert_eq!(merge.unresolved(), 0);
        assert!(!merge.take(99, Pick::Ours));
        assert!(merge.set_piece_text(conflict, "mine\nand theirs\n"));
        assert_eq!(merge.result(), "a\nmine\nand theirs\nc\n");
        assert_eq!(changes(&merge), [(ChunkKind::Conflict, Taken::Edited)]);
    }

    #[test]
    fn an_edit_of_the_result_joins_only_the_pieces_it_touches() {
        let base = "1\n2\n3\n4\n5\n6\n7\n8\n";
        let ours = "1\nO2\n3\n4\n5\n6\nO7\n8\n";
        let theirs = "1\nT2\n3\n4\n5\n6\n7\n8\n";
        let mut merge = Merge::new(base, ours, theirs);
        use ChunkKind::*;
        assert_eq!(changes(&merge), [(Conflict, Taken::Unresolved), (Ours, Taken::Auto)]);
        let result = merge.result();
        assert_eq!(result, "1\n2\n3\n4\n5\n6\nO7\n8\n");
        merge.set_result("1\n2\n3\nfour\n5\n6\nO7\n8\n");
        assert_eq!(merge.result(), "1\n2\n3\nfour\n5\n6\nO7\n8\n");
        assert_eq!(changes(&merge), [(Conflict, Taken::Unresolved), (Same, Taken::Edited), (Ours, Taken::Auto)]);
        assert_eq!(merge.unresolved(), 1);
        merge.set_result("1\nboth 2\n3\nfour\n5\n6\nO7\n8\n9\n");
        assert_eq!(merge.result(), "1\nboth 2\n3\nfour\n5\n6\nO7\n8\n9\n");
        assert_eq!(merge.unresolved(), 0);
        assert_eq!(
            changes(&merge),
            [(Conflict, Taken::Edited), (Same, Taken::Edited), (Ours, Taken::Auto), (Same, Taken::Edited)]
        );
        let mut at = (0, 0, 0);
        for piece in merge.pieces() {
            assert_eq!((piece.chunk.base.start, piece.chunk.ours.start, piece.chunk.theirs.start), at);
            at = (piece.chunk.base.end, piece.chunk.ours.end, piece.chunk.theirs.end);
        }
        assert_eq!(at, (8, 8, 8));
        merge.set_result("1\njoined\n8\n9\n");
        assert_eq!(merge.result(), "1\njoined\n8\n9\n");
        assert_eq!(merge.unresolved(), 0);
    }

    #[test]
    fn a_text_without_a_last_line_break_merges_and_takes_both() {
        let mut merge = Merge::new("a\nb", "a\nours", "a\ntheirs");
        let conflict = merge.pieces().iter().position(|piece| piece.taken == Taken::Unresolved).unwrap();
        merge.take(conflict, Pick::Both);
        assert_eq!(merge.result(), "a\nours\ntheirs");
        merge.set_result("a\nours\ntheirs\n");
        assert_eq!(merge.result(), "a\nours\ntheirs\n");
    }

    proptest::proptest! {
        #[test]
        fn the_result_is_never_out_of_step_with_what_was_typed(
            ours in "(a|b|c|\\n){0,16}",
            theirs in "(a|b|c|\\n){0,16}",
            first in "(a|b|c|\\n){0,16}",
            typed in "(a|b|c|\\n){0,16}",
        ) {
            let mut merge = Merge::new("a\nb\nc\nd\n", &ours, &theirs);
            merge.set_result(&first);
            merge.set_result(&typed);
            proptest::prop_assert_eq!(merge.result(), typed);
        }
    }
}

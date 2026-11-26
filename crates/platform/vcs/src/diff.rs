use std::ops::Range;

use gix_imara_diff::{Algorithm, Diff, InternedInput, sources};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub before: Range<usize>,
    pub after: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Added,
    Modified,
    Deleted,
}

impl LineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LineKind::Added => "added",
            LineKind::Modified => "modified",
            LineKind::Deleted => "deleted",
        }
    }
}

impl Hunk {
    pub fn kind(&self) -> LineKind {
        if self.before.is_empty() {
            LineKind::Added
        } else if self.after.is_empty() {
            LineKind::Deleted
        } else {
            LineKind::Modified
        }
    }
}

pub fn hunks(before: &str, after: &str) -> Vec<Hunk> {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|hunk| Hunk {
            before: hunk.before.start as usize..hunk.before.end as usize,
            after: hunk.after.start as usize..hunk.after.end as usize,
        })
        .collect()
}

pub fn changed_lines(before: &str, after: &str) -> Vec<Range<usize>> {
    let input = InternedInput::new(before, after);
    let diff = Diff::compute(Algorithm::Myers, &input);
    diff.hunks()
        .map(|hunk| hunk.after.start as usize..hunk.after.end as usize)
        .filter(|lines| !lines.is_empty())
        .collect()
}

pub fn word_changes(before: &str, after: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let input = InternedInput::new(sources::words(before), sources::words(after));
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_no_heuristic(&input);
    let ranges = |tokens: &[gix_imara_diff::Token], changed: &dyn Fn(u32) -> bool| {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut offset = 0;
        for (index, token) in tokens.iter().enumerate() {
            let length = input.interner[*token].chars().count();
            if changed(index as u32) {
                match ranges.last_mut() {
                    Some(last) if last.end == offset => last.end = offset + length,
                    _ => ranges.push(offset..offset + length),
                }
            }
            offset += length;
        }
        ranges
    };
    (ranges(&input.before, &|index| diff.is_removed(index)), ranges(&input.after, &|index| diff.is_added(index)))
}

pub fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

pub fn revert(before: &str, after: &str, hunk: &Hunk) -> String {
    let (old, new) = (lines(before), lines(after));
    let mut text = String::with_capacity(after.len());
    for line in &new[..hunk.after.start.min(new.len())] {
        text.push_str(line);
    }
    for line in &old[hunk.before.start.min(old.len())..hunk.before.end.min(old.len())] {
        text.push_str(line);
    }
    for line in &new[hunk.after.end.min(new.len())..] {
        text.push_str(line);
    }
    text
}

pub fn combine(before: &str, after: &str, removed: &dyn Fn(usize) -> bool, added: &dyn Fn(usize) -> bool) -> String {
    let (old, new) = (lines(before), lines(after));
    let mut text = String::with_capacity(before.len().max(after.len()));
    let push = |text: &mut String, line: &str| {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(line);
    };
    let mut next = 0;
    for hunk in hunks(before, after) {
        for line in &old[next.min(old.len())..hunk.before.start.min(old.len())] {
            push(&mut text, line);
        }
        for index in hunk.before.clone() {
            if !removed(index) {
                push(&mut text, old[index]);
            }
        }
        for index in hunk.after.clone() {
            if added(index) {
                push(&mut text, new[index]);
            }
        }
        next = hunk.before.end;
    }
    for line in &old[next.min(old.len())..] {
        push(&mut text, line);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combining_takes_only_the_chosen_lines() {
        let before = "a\nb\nc\nd\n";
        let after = "a\nB\nc\nnew\nd\n";
        let all = |_| true;
        let none = |_| false;
        assert_eq!(combine(before, after, &all, &all), after);
        assert_eq!(combine(before, after, &none, &none), before);
        assert_eq!(combine(before, after, &none, &|line| line == 3), "a\nb\nc\nnew\nd\n");
        assert_eq!(combine(before, after, &|line| line == 1, &none), "a\nc\nd\n");
        assert_eq!(combine("x\ny\n", "x\nz\n", &none, &|line| line == 1), "x\ny\nz\n");
        assert_eq!(combine("a", "a\nb\n", &none, &|line| line == 1), "a\nb\n");
        assert_eq!(combine("", "x\ny\n", &none, &|line| line == 0), "x\n");
    }

    #[test]
    fn changed_lines_are_the_new_lines_of_the_hunks() {
        assert_eq!(changed_lines("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n"), [1..2, 4..5]);
        assert_eq!(changed_lines("a\nb\n", "a\nb\n"), Vec::<Range<usize>>::new());
        assert_eq!(changed_lines("a\nb\nc\n", "a\nc\n"), Vec::<Range<usize>>::new(), "a removal changes no line");
    }

    #[test]
    fn hunks_say_which_lines_were_added_changed_and_removed() {
        let before = "a\nb\nc\nd\n";
        let after = "a\nB\nc\nnew\nd\n";
        let found = hunks(before, after);
        assert_eq!(found, [Hunk { before: 1..2, after: 1..2 }, Hunk { before: 3..3, after: 3..4 }]);
        assert_eq!(found[0].kind(), LineKind::Modified);
        assert_eq!(found[1].kind(), LineKind::Added);
        let removed = hunks("a\nb\nc\n", "a\nc\n");
        assert_eq!(removed, [Hunk { before: 1..2, after: 1..1 }]);
        assert_eq!(removed[0].kind(), LineKind::Deleted);
        assert!(hunks("same\n", "same\n").is_empty());
        assert_eq!(hunks("", "x\n"), [Hunk { before: 0..0, after: 0..1 }]);
    }

    #[test]
    fn words_that_changed_are_marked_in_both_lines() {
        let (before, after) = word_changes("let total = sum(a, b)", "let count = sum(a, b, c)");
        assert_eq!(before.as_slice(), &[4..9, 0..0][..1]);
        assert_eq!(after, [4..9, 20..23], "count, and \", c\"");
        let (before, after) = word_changes("näive x", "naïve x");
        assert_eq!(
            (before.first(), after.first(), before.len() + after.len()),
            (Some(&(0..5)), Some(&(0..5)), 2),
            "characters, not bytes"
        );
    }

    #[test]
    fn reverting_a_hunk_brings_back_the_old_lines_only() {
        let before = "a\nb\nc\nd\n";
        let after = "a\nB\nc\nnew\nd\n";
        let found = hunks(before, after);
        assert_eq!(revert(before, after, &found[0]), "a\nb\nc\nnew\nd\n");
        assert_eq!(revert(before, after, &found[1]), "a\nB\nc\nd\n");
        let removed = hunks("a\nb\n", "a\n");
        assert_eq!(revert("a\nb\n", "a\n", &removed[0]), "a\nb\n");
        assert_eq!(lines("x\ny"), ["x\n", "y"]);
    }
}

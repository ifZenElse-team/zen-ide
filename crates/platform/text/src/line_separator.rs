use std::borrow::Cow;
use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Edit, Text};

/// A line separator, as files on disk use them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LineSeparator {
    /// LF: Unix and macOS.
    Lf,
    /// CRLF: Windows.
    Crlf,
    /// CR: classic Mac OS.
    Cr,
}

impl LineSeparator {
    pub const ALL: [Self; 3] = [Self::Crlf, Self::Lf, Self::Cr];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
            Self::Cr => "\r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
            Self::Cr => "CR",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
            Self::Cr => "cr",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|separator| separator.id().eq_ignore_ascii_case(name.trim()))
    }

    pub fn system() -> Self {
        if cfg!(windows) { Self::Crlf } else { Self::Lf }
    }
}

fn breaks(text: &str) -> impl Iterator<Item = (usize, LineSeparator)> + '_ {
    let mut chars = text.chars().enumerate().peekable();
    std::iter::from_fn(move || {
        loop {
            let (offset, c) = chars.next()?;
            match c {
                '\n' => return Some((offset, LineSeparator::Lf)),
                '\r' if chars.peek().is_some_and(|(_, next)| *next == '\n') => {
                    chars.next();
                    return Some((offset, LineSeparator::Crlf));
                }
                '\r' => return Some((offset, LineSeparator::Cr)),
                _ => {}
            }
        }
    })
}

pub fn detect(text: &str) -> Option<LineSeparator> {
    breaks(text).next().map(|(_, separator)| separator)
}

pub fn is_mixed(text: &str) -> bool {
    let mut found = breaks(text).map(|(_, separator)| separator);
    let Some(first) = found.next() else { return false };
    found.any(|separator| separator != first)
}

pub fn convert(text: &str, to: LineSeparator) -> Cow<'_, str> {
    if breaks(text).all(|(_, separator)| separator == to) {
        return Cow::Borrowed(text);
    }
    let mut converted = String::with_capacity(text.len() + text.len() / 16);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => converted.push_str(to.as_str()),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                converted.push_str(to.as_str());
            }
            other => converted.push(other),
        }
    }
    Cow::Owned(converted)
}

pub fn convert_offsets(text: &str, to: LineSeparator, offsets: &[usize]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..offsets.len()).collect();
    order.sort_by_key(|index| offsets[*index]);
    let mut mapped = vec![0; offsets.len()];
    let mut queries = order.into_iter().peekable();
    let (mut old, mut new) = (0, 0);
    let added = to.as_str().len();
    for (start, separator) in breaks(text) {
        let end = start + separator.as_str().len();
        while let Some(index) = queries.next_if(|index| offsets[*index] <= start) {
            mapped[index] = new + offsets[index] - old;
        }
        new += start - old;
        while let Some(index) = queries.next_if(|index| offsets[*index] < end) {
            mapped[index] = new + added;
        }
        new += added;
        old = end;
    }
    for index in queries {
        mapped[index] = new + offsets[index].saturating_sub(old);
    }
    mapped
}

pub fn break_at(text: &Text, offset: usize) -> Option<(Range<usize>, LineSeparator)> {
    let len = text.len_chars();
    if offset >= len {
        return None;
    }
    let low = offset.saturating_sub(1);
    let around: Vec<char> = text.slice(low..(offset + 2).min(len)).ok()?.chars().collect();
    let at = offset - low;
    match around[at] {
        '\n' if at > 0 && around[at - 1] == '\r' => Some((offset - 1..offset + 1, LineSeparator::Crlf)),
        '\n' => Some((offset..offset + 1, LineSeparator::Lf)),
        '\r' if around.get(at + 1) == Some(&'\n') => Some((offset..offset + 2, LineSeparator::Crlf)),
        '\r' => Some((offset..offset + 1, LineSeparator::Cr)),
        _ => None,
    }
}

pub fn repairs(before: &Text, edits: &[Edit], to: LineSeparator) -> Vec<Edit> {
    let mut text = before.clone();
    let mut found: Vec<(Range<usize>, usize)> = Vec::new();
    for edit in edits.iter().filter(|edit| !edit.is_noop()) {
        let Ok((after, _)) = text.apply(std::slice::from_ref(edit)) else { return Vec::new() };
        let (start, end) = (edit.range.start, edit.inserted_range().end);
        let shift = end as isize - edit.range.end as isize;
        found.retain_mut(|(range, _)| {
            if range.end <= edit.range.start {
                true
            } else if range.start >= edit.range.end {
                *range = range.start.saturating_add_signed(shift)..range.end.saturating_add_signed(shift);
                true
            } else {
                false
            }
        });
        let low = start.saturating_sub(2);
        let Ok(window) = after.slice(low..(end + 2).min(after.len_chars())) else { return Vec::new() };
        for (offset, separator) in breaks(&window) {
            let range = low + offset..low + offset + separator.as_str().len();
            if range.end <= start.saturating_sub(1) || range.start > end {
                continue;
            }
            let overlaps = |kept: &(Range<usize>, usize)| kept.0.start < range.end && range.start < kept.0.end;
            let repaired = found.iter().any(overlaps);
            found.retain(|kept| !overlaps(kept));
            if separator == to {
                continue;
            }
            let inserted = range.start < end && range.end > start;
            let kept = if repaired || inserted {
                false
            } else if range.end <= start {
                break_at(&text, range.start) == Some((range.clone(), separator))
            } else {
                let old = range.start - end + edit.range.end;
                break_at(&text, old) == Some((old..old + range.len(), separator))
            };
            if !kept {
                let whole = range.start >= start && range.end <= end;
                let count = if separator == LineSeparator::Crlf && !whole && !repaired { 2 } else { 1 };
                found.push((range, count));
            }
        }
        text = after;
    }
    found.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    found.into_iter().map(|(range, count)| Edit::replace(range, to.as_str().repeat(count))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_first_separator_and_mixed_texts() {
        assert_eq!(detect("a\r\nb\nc"), Some(LineSeparator::Crlf));
        assert_eq!(detect("a\rb"), Some(LineSeparator::Cr));
        assert_eq!(detect("a\nb\r\n"), Some(LineSeparator::Lf));
        assert_eq!(detect("one line"), None);
        assert!(is_mixed("a\r\nb\nc"));
        assert!(!is_mixed("a\r\nb\r\n"));
        assert!(!is_mixed(""));
    }

    #[test]
    fn converts_every_line_break() {
        assert_eq!(convert("a\r\nb\nc\rd", LineSeparator::Lf), "a\nb\nc\nd");
        assert_eq!(convert("a\nb\n", LineSeparator::Crlf), "a\r\nb\r\n");
        assert_eq!(convert("a\r\n\nb", LineSeparator::Cr), "a\r\rb");
        assert!(matches!(convert("a\nb", LineSeparator::Lf), Cow::Borrowed(_)));
    }

    #[test]
    fn offsets_follow_the_conversion() {
        let text = "ab\ncd\r\nef";
        let converted = convert(text, LineSeparator::Crlf);
        assert_eq!(converted, "ab\r\ncd\r\nef");
        assert_eq!(convert_offsets(text, LineSeparator::Crlf, &[9, 0, 3, 2, 6, 5]), vec![10, 0, 4, 2, 8, 6]);
        assert_eq!(convert_offsets(text, LineSeparator::Lf, &[7, 6, 9]), vec![6, 6, 8]);
    }

    fn apply_repaired(text: &str, edits: &[Edit], to: LineSeparator) -> String {
        let before = Text::from(text);
        let mut all = edits.to_vec();
        all.extend(repairs(&before, edits, to));
        before.apply(&all).unwrap().0.to_string()
    }

    #[test]
    fn repairs_the_line_breaks_edits_make() {
        let crlf = LineSeparator::Crlf;
        assert_eq!(apply_repaired("a\r\nb", &[Edit::insert(1, "x\ny")], crlf), "ax\r\ny\r\nb");
        assert_eq!(apply_repaired("a\r\nb", &[Edit::delete(1..2)], crlf), "a\r\nb", "a deleted CR comes back");
        assert_eq!(apply_repaired("a\r\nb", &[Edit::delete(2..3)], crlf), "a\r\nb", "a deleted LF comes back");
        assert_eq!(apply_repaired("a\r\nb", &[Edit::insert(2, "x")], crlf), "a\r\nx\r\nb", "a split CRLF");
        assert_eq!(
            apply_repaired("a\nb\r\nc", &[Edit::insert(3, "x")], crlf),
            "a\nbx\r\nc",
            "a mixed file keeps its LF"
        );
        assert_eq!(apply_repaired("a\nb", &[Edit::replace(0..1, "z")], crlf), "z\nb", "untouched breaks stay");
        assert_eq!(apply_repaired("a\rb", &[Edit::insert(3, "\n")], LineSeparator::Cr), "a\rb\r");
        assert_eq!(
            apply_repaired("a\rb", &[Edit::insert(2, "\n")], LineSeparator::Cr),
            "a\r\rb",
            "CR then LF is no CRLF"
        );
        assert_eq!(apply_repaired("ab", &[Edit::insert(1, "\r"), Edit::insert(2, "\n")], LineSeparator::Lf), "a\nb");
    }

    #[test]
    fn finds_the_line_break_of_a_character() {
        let text = Text::from("a\r\nb\nc\rd");
        assert_eq!(break_at(&text, 1), Some((1..3, LineSeparator::Crlf)));
        assert_eq!(break_at(&text, 2), Some((1..3, LineSeparator::Crlf)));
        assert_eq!(break_at(&text, 4), Some((4..5, LineSeparator::Lf)));
        assert_eq!(break_at(&text, 6), Some((6..7, LineSeparator::Cr)));
        assert_eq!(break_at(&text, 0), None);
        assert_eq!(break_at(&text, 99), None);
    }

    mod properties {
        use proptest::prelude::*;

        use super::super::*;
        use super::apply_repaired;

        fn separator() -> impl Strategy<Value = LineSeparator> {
            prop_oneof![Just(LineSeparator::Lf), Just(LineSeparator::Crlf), Just(LineSeparator::Cr)]
        }

        fn lines(separator: LineSeparator) -> impl Strategy<Value = String> {
            prop::collection::vec("[a-c ]{0,4}", 1..8).prop_map(move |lines| lines.join(separator.as_str()))
        }

        fn edits(len: usize) -> impl Strategy<Value = Vec<(usize, usize, String)>> {
            prop::collection::vec((0..=len, 0usize..4, "[ab\r\n]{0,4}"), 1..5)
        }

        fn sequential(text: &str, raw: &[(usize, usize, String)]) -> Vec<Edit> {
            let mut len = text.chars().count();
            let mut edits = Vec::new();
            for (start, removed, inserted) in raw {
                let start = (*start).min(len);
                let end = (start + removed).min(len);
                edits.push(Edit::replace(start..end, inserted.clone()));
                len = len - (end - start) + inserted.chars().count();
            }
            edits
        }

        fn without_breaks(text: &str) -> String {
            text.chars().filter(|c| !matches!(c, '\r' | '\n')).collect()
        }

        fn foreign(text: &str, to: LineSeparator) -> usize {
            breaks(text).filter(|(_, found)| *found != to).count()
        }

        proptest! {
            #[test]
            fn edits_keep_the_files_separator(
                (to, text, raw) in separator().prop_flat_map(|to| (Just(to), lines(to)))
                    .prop_flat_map(|(to, text)| { let len = text.chars().count(); (Just(to), Just(text), edits(len)) })
            ) {
                let edits = sequential(&text, &raw);
                let plain = Text::from(text.as_str()).apply(&edits).unwrap().0.to_string();
                let result = apply_repaired(&text, &edits, to);
                prop_assert_eq!(foreign(&result, to), 0, "{:?} -> {:?}", text, result);
                prop_assert!(!is_mixed(&result));
                prop_assert_eq!(without_breaks(&result), without_breaks(&plain));
            }

            #[test]
            fn mixed_files_get_no_new_foreign_breaks(
                to in separator(),
                pieces in prop::collection::vec(("[a-c]{1,3}", separator()), 1..8),
                raw in edits(30),
            ) {
                let text: String = pieces.iter().map(|(line, separator)| format!("{line}{}", separator.as_str())).collect();
                let edits = sequential(&text, &raw);
                let plain = Text::from(text.as_str()).apply(&edits).unwrap().0.to_string();
                let result = apply_repaired(&text, &edits, to);
                prop_assert!(foreign(&result, to) <= foreign(&text, to) + foreign(&plain, to));
                prop_assert!(foreign(&result, to) <= foreign(&plain, to).max(foreign(&text, to)));
                prop_assert_eq!(without_breaks(&result), without_breaks(&plain));
            }

            #[test]
            fn conversion_and_offsets(text in "[ab\r\n]{0,20}", to in separator(), offsets in prop::collection::vec(0usize..21, 0..5)) {
                let converted = convert(&text, to);
                prop_assert_eq!(foreign(&converted, to), 0);
                prop_assert_eq!(without_breaks(&converted), without_breaks(&text));
                let len = text.chars().count();
                let offsets: Vec<usize> = offsets.into_iter().map(|offset| offset.min(len)).collect();
                for mapped in convert_offsets(&text, to, &offsets) {
                    prop_assert!(mapped <= converted.chars().count());
                }
            }
        }
    }

    #[test]
    fn names_and_ids() {
        assert_eq!(LineSeparator::parse("CRLF"), Some(LineSeparator::Crlf));
        assert_eq!(LineSeparator::parse("cr"), Some(LineSeparator::Cr));
        assert_eq!(LineSeparator::parse("lf "), Some(LineSeparator::Lf));
        assert_eq!(LineSeparator::parse("\n"), None);
        assert_eq!(LineSeparator::Crlf.label(), "CRLF");
    }
}

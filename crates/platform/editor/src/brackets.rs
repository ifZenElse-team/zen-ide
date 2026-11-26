use zen_text::Text;

pub const BRACKETS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

pub const ANGLE_BRACKETS: [(char, char); 4] = [BRACKETS[0], BRACKETS[1], BRACKETS[2], ('<', '>')];

pub fn bracket_pairs(angle: bool) -> &'static [(char, char)] {
    if angle { &ANGLE_BRACKETS } else { &BRACKETS }
}

pub const REACH: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BracketMatch {
    pub bracket: usize,
    pub matching: Option<usize>,
}

pub fn matching_bracket(
    text: &Text,
    offset: usize,
    pairs: &[(char, char)],
    skipped: &dyn Fn(usize) -> bool,
) -> Option<BracketMatch> {
    let len = text.len_chars();
    let offset = offset.min(len);
    let start = offset.saturating_sub(REACH);
    let end = (offset + REACH).min(len);
    let base = text.offset_to_byte(start).ok()?;
    let window: Vec<(char, usize)> =
        text.slice(start..end).ok()?.char_indices().map(|(byte, c)| (c, base + byte)).collect();
    let opening = |c: char| pairs.iter().find(|(open, _)| *open == c).map(|(_, close)| *close);
    let closing = |c: char| pairs.iter().find(|(_, close)| *close == c).map(|(open, _)| *open);
    let bracket = |at: usize| {
        window.get(at).filter(|(c, byte)| (opening(*c).is_some() || closing(*c).is_some()) && !skipped(*byte))
    };
    let after = offset - start;
    let before = after.checked_sub(1);
    let is = |at: Option<usize>, open: bool| {
        at.filter(|&at| bracket(at).is_some_and(|(c, _)| opening(*c).is_some() == open))
    };
    let at = is(Some(after), true).or(is(before, false)).or(is(Some(after), false)).or(is(before, true))?;
    let (c, _) = window[at];
    let forward = opening(c).is_some();
    let mut waiting = vec![if forward { opening(c) } else { closing(c) }?];
    let found = |matching| Some(BracketMatch { bracket: start + at, matching });
    let indices: Box<dyn Iterator<Item = usize>> =
        if forward { Box::new(at + 1..window.len()) } else { Box::new((0..at).rev()) };
    for index in indices {
        let Some(&(c, _)) = bracket(index) else { continue };
        let (wanted, pushed) = if forward { (closing(c), opening(c)) } else { (opening(c), closing(c)) };
        if let Some(other) = pushed {
            waiting.push(other);
        } else if wanted.is_some() {
            if waiting.pop() != Some(c) {
                return found(None);
            }
            if waiting.is_empty() {
                return found(Some(start + index));
            }
        }
    }
    let whole = if forward { end == len } else { start == 0 };
    whole.then(|| BracketMatch { bracket: start + at, matching: None })
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;

    fn matched(text: &str, offset: usize) -> Option<BracketMatch> {
        matching_bracket(&Text::from(text), offset, &BRACKETS, &|_| false)
    }

    fn pair(bracket: usize, matching: usize) -> Option<BracketMatch> {
        Some(BracketMatch { bracket, matching: Some(matching) })
    }

    fn unmatched(bracket: usize) -> Option<BracketMatch> {
        Some(BracketMatch { bracket, matching: None })
    }

    #[test]
    fn the_bracket_after_the_caret_first_then_the_one_before() {
        let text = "f(a[1], {b})";
        assert_eq!(matched(text, 1), pair(1, 11), "before `(`");
        assert_eq!(matched(text, 12), pair(11, 1), "after `)`");
        assert_eq!(matched(text, 3), pair(3, 5), "before `[`");
        assert_eq!(matched(text, 10), pair(10, 8), "between `b` and the brace");
        assert_eq!(matched(text, 9), pair(8, 10), "between the brace and `b`");
        assert_eq!(matched(text, 0), None, "next to a name");
        assert_eq!(matched("()", 1), pair(1, 0), "between the two, the one after wins");
    }

    #[test]
    fn an_opening_bracket_after_the_caret_then_a_closing_one_before_it() {
        let text = "f(g(x))(y)";
        assert_eq!(matched(text, 6), pair(5, 3), "between `))`: the one just closed");
        assert_eq!(matched(text, 7), pair(7, 9), "between `)(`: the one that opens");
        assert_eq!(matched("((x))", 1), pair(1, 3), "between `((`: the one after");
    }

    #[test]
    fn nesting_counts_every_kind() {
        let text = "{ ( [ ] { } ) }";
        assert_eq!(matched(text, 0), pair(0, 14));
        assert_eq!(matched(text, 2), pair(2, 12));
        assert_eq!(matched(text, 15), pair(14, 0));
        assert_eq!(matched(text, 8), pair(8, 10));
    }

    #[test]
    fn brackets_in_strings_and_comments_do_not_count() {
        let text = "f(\"(\", x) // )\n";
        let skipped: [Range<usize>; 2] = [2..5, 10..14];
        let skip = |byte: usize| skipped.iter().any(|range| range.contains(&byte));
        let found = |offset| matching_bracket(&Text::from(text), offset, &BRACKETS, &skip);
        assert_eq!(found(1), pair(1, 8));
        assert_eq!(found(3), None, "a bracket in a string is not highlighted");
        assert_eq!(found(13), None, "nor one in a comment");
    }

    #[test]
    fn skipped_takes_byte_offsets() {
        let text = "ж(\"ё)\")";
        let skip = |byte: usize| (3..8).contains(&byte);
        assert_eq!(matching_bracket(&Text::from(text), 1, &BRACKETS, &skip), pair(1, 6));
    }

    #[test]
    fn an_unmatched_or_mismatched_bracket_has_no_match() {
        assert_eq!(matched("f(a", 1), unmatched(1));
        assert_eq!(matched("a)", 2), unmatched(1));
        assert_eq!(matched("( ]", 0), unmatched(0));
        assert_eq!(matched("( [ )", 0), unmatched(0));
    }

    #[test]
    fn angle_brackets_only_when_asked_for() {
        assert_eq!(matched("Vec<u8>", 3), None);
        let pairs = bracket_pairs(true);
        assert_eq!(matching_bracket(&Text::from("Vec<u8>"), 3, pairs, &|_| false), pair(3, 6));
        assert_eq!(bracket_pairs(false), BRACKETS);
    }

    #[test]
    fn the_search_stops_at_its_reach() {
        let far = format!("({})", "x".repeat(REACH));
        assert_eq!(matched(&far, 0), None, "the match is beyond the reach: not known");
        assert_eq!(matched(&far, far.len()), None);
        let near = format!("({})", "x".repeat(REACH - 2));
        assert_eq!(matched(&near, 0), pair(0, REACH - 1));
        let open = format!("({}", "x".repeat(REACH - 2));
        assert_eq!(matched(&open, 0), unmatched(0), "the whole rest of the text was looked at");
        let long_open = format!("{}({}", "y".repeat(3 * REACH), "x".repeat(REACH - 2));
        assert_eq!(matched(&long_open, 3 * REACH), unmatched(3 * REACH));
    }
}

use std::collections::HashSet;
use std::ops::Range;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn word_prefix(before: &str) -> &str {
    let start = before.char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(before.len(), |(at, _)| at);
    &before[start..]
}

fn words(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let start = loop {
            let (at, c) = chars.next()?;
            if is_word(c) {
                break at;
            }
        };
        let mut end = text.len();
        while let Some(&(at, c)) = chars.peek() {
            if !is_word(c) {
                end = at;
                break;
            }
            chars.next();
        }
        Some((start..end, &text[start..end]))
    })
}

pub fn expansions(text: &str, caret: usize, prefix: &str, forward: bool, others: &[&str]) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let completes = |word: &str| word.len() > prefix.len() && word.starts_with(prefix);
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (range, word) in words(text).filter(|(_, word)| completes(word)) {
        if range.end < caret {
            before.push(word);
        } else if range.start > caret {
            after.push(word);
        }
    }
    before.reverse();
    let (first, second) = if forward { (after, before) } else { (before, after) };
    let elsewhere = others.iter().flat_map(|other| words(other)).map(|(_, word)| word).filter(|word| completes(word));
    let mut seen = HashSet::new();
    first.into_iter().chain(second).chain(elsewhere).filter(|word| seen.insert(*word)).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prefix_is_the_word_part_left_of_the_caret() {
        assert_eq!(word_prefix("fmt.Pri"), "Pri");
        assert_eq!(word_prefix("ёлка_1"), "ёлка_1");
        assert_eq!(word_prefix("a "), "");
        assert_eq!(word_prefix("it"), "it");
        assert_eq!(word_prefix(""), "");
    }

    #[test]
    fn backward_takes_the_nearest_word_above_then_below_then_other_texts() {
        let text = "counter := 1\ncount2 := counter\nco\ncolumn := conn\ncounter2";
        let caret = text.find("co\n").unwrap() + 2;
        let found = expansions(text, caret, "co", false, &["config cool count2", "colour"]);
        assert_eq!(found, ["counter", "count2", "column", "conn", "counter2", "config", "cool", "colour"]);
    }

    #[test]
    fn forward_takes_the_words_below_first() {
        let text = "counter := 1\ncount2 := counter\nco\ncolumn := conn\ncounter2";
        let caret = text.find("co\n").unwrap() + 2;
        let found = expansions(text, caret, "co", true, &["cool"]);
        assert_eq!(found, ["column", "conn", "counter2", "counter", "count2", "cool"]);
    }

    #[test]
    fn the_word_at_the_caret_and_the_prefix_itself_are_left_out() {
        let text = "co cost // co in a comment, \"coin\"";
        assert_eq!(expansions(text, 2, "co", false, &[]), ["cost", "comment", "coin"]);
        assert_eq!(expansions("cosmos co", 2, "co", false, &[]), Vec::<String>::new(), "the caret is in cosmos");
        assert_eq!(expansions("Count count", 11, "co", false, &[]), Vec::<String>::new(), "case counts");
        assert_eq!(expansions("anything", 0, "", false, &["more"]), Vec::<String>::new(), "no prefix");
    }

    #[test]
    fn cyrillic_words_and_byte_offsets() {
        let text = "счётчик := 1\nсч\nсчёт_2";
        let caret = text.find("сч\n").unwrap() + "сч".len();
        assert_eq!(expansions(text, caret, "сч", false, &["счастье"]), ["счётчик", "счёт_2", "счастье"]);
        assert_eq!(expansions(text, caret, "сч", true, &[]), ["счёт_2", "счётчик"]);
    }
}

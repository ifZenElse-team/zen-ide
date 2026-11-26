use std::ops::Range;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn name_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let offset = offset.min(chars.len());
    let mut start = offset;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    (end > start && !chars[start].is_ascii_digit()).then_some(start..end)
}

pub fn occurrences(text: &str, offset: usize) -> Vec<Range<usize>> {
    let Some(name) = name_at(text, offset) else { return Vec::new() };
    let chars: Vec<char> = text.chars().collect();
    let word = &chars[name.clone()];
    let mut found = Vec::new();
    let mut at = 0;
    while at + word.len() <= chars.len() {
        if chars[at..at + word.len()] == *word
            && (at == 0 || !is_word(chars[at - 1]))
            && chars.get(at + word.len()).is_none_or(|c| !is_word(*c))
        {
            found.push(at..at + word.len());
            at += word.len();
        } else {
            at += 1;
        }
    }
    found
}

pub const MAX_SELECTION: usize = 1000;

pub fn selection_occurrences(text: &str, selection: Range<usize>) -> Vec<Range<usize>> {
    if selection.is_empty() || selection.len() > MAX_SELECTION {
        return Vec::new();
    }
    let byte = |chars: usize| text.char_indices().nth(chars).map_or(text.len(), |(at, _)| at);
    let (start, end) = (byte(selection.start), byte(selection.end));
    let Some(needle) = text.get(start..end) else { return Vec::new() };
    if needle.trim().is_empty() || needle.contains('\n') {
        return Vec::new();
    }
    let (mut bytes, mut chars) = (0, 0);
    let mut found = Vec::new();
    for (at, _) in text.match_indices(needle) {
        chars += text[bytes..at].chars().count();
        bytes = at;
        found.push(chars..chars + selection.len());
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_words_only() {
        let text = "in string\nkey string\nmap[string]strings _string";
        let caret = text.find("string").unwrap() + 2;
        let found = occurrences(text, caret);
        let words: Vec<String> = found.iter().map(|range| text[range.clone()].to_owned()).collect();
        assert_eq!(words, ["string", "string", "string"]);
        assert_eq!(found[2], 25..31);
    }

    #[test]
    fn the_caret_may_touch_the_name_and_numbers_are_not_names() {
        let text = "ёж := ёж + 12";
        assert_eq!(occurrences(text, 2).len(), 2, "right after the name, in characters");
        assert_eq!(name_at(text, 3), None);
        assert!(occurrences(text, 12).is_empty());
    }

    #[test]
    fn a_selection_is_found_inside_words_and_comments() {
        let text = "ёж := ёжик // ёж\nЁж";
        let found = selection_occurrences(text, 0..2);
        assert_eq!(found, [0..2, 6..8, 14..16], "case-sensitive, inside words and comments");
        assert_eq!(selection_occurrences(text, 2..3), Vec::<Range<usize>>::new(), "white space only");
        assert!(selection_occurrences(text, 14..18).is_empty(), "across lines");
        assert!(selection_occurrences(text, 3..3).is_empty());
        let long = "a".repeat(MAX_SELECTION + 1);
        assert!(selection_occurrences(&long, 0..MAX_SELECTION + 1).is_empty());
        assert_eq!(selection_occurrences("aaaa", 0..2), [0..2, 2..4], "without overlaps");
    }
}

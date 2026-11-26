pub const OFF: &str = "@formatter:off";
pub const ON: &str = "@formatter:on";

pub fn has_markers(text: &str) -> bool {
    text.contains(OFF)
}

fn parts(text: &str) -> Vec<(bool, &str)> {
    let mut parts = Vec::new();
    let mut protected = false;
    let mut start = 0;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let opens = !protected && line.contains(OFF);
        if opens {
            parts.push((false, &text[start..at]));
            protected = true;
            start = at;
        }
        at += line.len();
        if protected && !opens && line.contains(ON) {
            parts.push((true, &text[start..at]));
            protected = false;
            start = at;
        }
    }
    parts.push((protected, &text[start..]));
    parts
}

pub fn restore(old: &str, formatted: &str) -> String {
    let kept: Vec<&str> = parts(old).into_iter().filter(|(protected, _)| *protected).map(|(_, part)| part).collect();
    let new = parts(formatted);
    if new.iter().filter(|(protected, _)| *protected).count() != kept.len() {
        return old.to_owned();
    }
    let mut kept = kept.into_iter();
    new.into_iter().map(|(protected, part)| if protected { kept.next().unwrap_or(part) } else { part }).collect()
}

pub fn protected_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut at = 0;
    let mut ranges = Vec::new();
    for (protected, part) in parts(text) {
        if protected {
            ranges.push(at..at + part.len());
        }
        at += part.len();
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_between_the_markers_keeps_its_form() {
        let old = "a  =  1\n// @formatter:off\nb  =  2\n// @formatter:on\nc  =  3\n";
        let formatted = "a = 1\n// @formatter:off\nb = 2\n// @formatter:on\nc = 3\n";
        assert_eq!(restore(old, formatted), "a = 1\n// @formatter:off\nb  =  2\n// @formatter:on\nc = 3\n");
    }

    #[test]
    fn a_marker_never_turned_on_again_keeps_the_rest_of_the_file() {
        let old = "a  =  1\n// @formatter:off\nb  =  2\n";
        assert_eq!(restore(old, "a = 1\n// @formatter:off\nb = 2\n"), "a = 1\n// @formatter:off\nb  =  2\n");
    }

    #[test]
    fn a_formatter_that_drops_a_marker_changes_nothing() {
        let old = "// @formatter:off\nb  =  2\n";
        assert_eq!(restore(old, "b = 2\n"), old);
    }

    #[test]
    fn the_ranges_cover_the_marked_lines() {
        let text = "a\n// @formatter:off\nb\n// @formatter:on\nc\n";
        assert_eq!(protected_ranges(text), vec![2..39]);
        assert!(protected_ranges("a\n").is_empty());
    }
}

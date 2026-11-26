use crate::Rgba;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FontStyle {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
}

impl FontStyle {
    pub(crate) fn parse(text: &str) -> FontStyle {
        let mut style = FontStyle::default();
        for word in text.split_whitespace() {
            match word {
                "bold" => style.bold = true,
                "italic" => style.italic = true,
                "underline" => style.underline = true,
                "strikethrough" => style.strikethrough = true,
                _ => {}
            }
        }
        style
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selector {
    path: Vec<String>,
    exclusions: Vec<Vec<String>>,
}

pub(crate) type Score = (usize, usize, usize);

impl Selector {
    pub(crate) fn parse(text: &str) -> Option<Selector> {
        if text.contains(['(', ')', '|', '&', '>', ',', '*']) {
            return None;
        }
        let mut pieces = text.split(" - ");
        let path = words(pieces.next()?)?;
        let exclusions = pieces.map(words).collect::<Option<Vec<_>>>()?;
        Some(Selector { path, exclusions })
    }

    pub(crate) fn score(&self, stack: &[&str]) -> Option<Score> {
        let score = path_score(&self.path, stack)?;
        if self.exclusions.iter().any(|exclusion| path_score(exclusion, stack).is_some()) {
            return None;
        }
        Some(score)
    }
}

fn words(text: &str) -> Option<Vec<String>> {
    let words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
    if words.is_empty() || words.iter().any(|word| word.starts_with('-') || word.starts_with('.')) {
        None
    } else {
        Some(words)
    }
}

fn segment_matches(selector: &str, scope: &str) -> bool {
    scope.strip_prefix(selector).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

fn path_score(path: &[String], stack: &[&str]) -> Option<Score> {
    let (last, ancestors) = path.split_last()?;
    (0..stack.len()).rev().find_map(|depth| {
        if !segment_matches(last, stack[depth]) {
            return None;
        }
        let mut limit = depth;
        for ancestor in ancestors.iter().rev() {
            limit = (0..limit).rev().find(|&index| segment_matches(ancestor, stack[index]))?;
        }
        Some((depth, last.split('.').count(), path.len()))
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rule {
    pub(crate) selector: Selector,
    pub(crate) foreground: Option<Rgba>,
    pub(crate) background: Option<Rgba>,
    pub(crate) font_style: Option<FontStyle>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Match {
    pub(crate) foreground: Option<Rgba>,
    pub(crate) background: Option<Rgba>,
    pub(crate) font_style: Option<FontStyle>,
}

fn better<T: Copy>(best: &mut Option<(Score, T)>, score: Score, value: Option<T>) {
    if let Some(value) = value
        && best.is_none_or(|(old, _)| score >= old)
    {
        *best = Some((score, value));
    }
}

pub(crate) fn resolve(rules: &[Rule], stack: &[&str]) -> Option<Match> {
    let mut foreground = None;
    let mut background = None;
    let mut font_style = None;
    for rule in rules {
        let Some(score) = rule.selector.score(stack) else { continue };
        better(&mut foreground, score, rule.foreground);
        better(&mut background, score, rule.background);
        better(&mut font_style, score, rule.font_style);
    }
    let found = Match {
        foreground: foreground.map(|(_, value)| value),
        background: background.map(|(_, value)| value),
        font_style: font_style.map(|(_, value)| value),
    };
    (found != Match::default()).then_some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(selector: &str, stack: &[&str]) -> Option<Score> {
        Selector::parse(selector).and_then(|selector| selector.score(stack))
    }

    fn rule(selector: &str, color: u8) -> Rule {
        Rule {
            selector: Selector::parse(selector).unwrap_or_else(|| panic!("{selector}")),
            foreground: Some(Rgba::rgb(color, 0, 0)),
            background: None,
            font_style: None,
        }
    }

    fn winner(rules: &[Rule], stack: &[&str]) -> Option<u8> {
        resolve(rules, stack).and_then(|found| found.foreground).map(Rgba::r)
    }

    #[test]
    fn a_selector_matches_by_dotted_prefix() {
        let table: &[(&str, &str, bool)] = &[
            ("keyword", "keyword", true),
            ("keyword", "keyword.control", true),
            ("keyword.control", "keyword.control.go", true),
            ("keyword.control", "keyword", false),
            ("key", "keyword", false),
            ("keyword.con", "keyword.control", false),
            ("string", "string.quoted.double", true),
            ("comment.block", "comment.line", false),
        ];
        for &(selector, scope, expected) in table {
            assert_eq!(score(selector, &[scope]).is_some(), expected, "{selector} on {scope}");
        }
    }

    #[test]
    fn a_descendant_selector_needs_its_ancestors_in_order() {
        let stack = ["source.go", "meta.function-call", "entity.name.function"];
        assert_eq!(score("meta.function-call entity.name.function", &stack), Some((2, 3, 2)));
        assert_eq!(score("source meta.function-call entity.name.function", &stack), Some((2, 3, 3)));
        assert_eq!(score("entity.name.function meta.function-call", &stack), None);
        assert_eq!(score("meta.function-call", &stack), Some((1, 2, 1)));
        assert_eq!(score("meta.block entity.name.function", &stack), None);
    }

    #[test]
    fn an_exclusion_removes_a_match() {
        let stack = ["comment.block.documentation"];
        assert!(score("comment - comment.block.documentation", &stack).is_none());
        assert!(score("comment - string", &stack).is_some());
    }

    #[test]
    fn unsupported_selectors_are_not_read() {
        for selector in ["(a | b)", "a > b", "a & b", "*", "", "  ", "-keyword", ".a"] {
            assert_eq!(Selector::parse(selector), None, "{selector:?}");
        }
    }

    #[test]
    fn the_most_specific_rule_wins_and_later_rules_win_ties() {
        let rules = [rule("keyword", 1), rule("keyword.control", 2), rule("keyword", 3)];
        assert_eq!(winner(&rules, &["keyword.control.go"]), Some(2));
        assert_eq!(winner(&rules, &["keyword.other"]), Some(3));
        assert_eq!(winner(&rules, &["string"]), None);
        let deeper = [rule("meta.function-call entity.name.function", 4), rule("entity.name.function", 5)];
        assert_eq!(winner(&deeper, &["meta.function-call", "entity.name.function"]), Some(4));
        assert_eq!(winner(&deeper, &["entity.name.function"]), Some(5));
        let ancestor = [rule("entity.name.function", 6), rule("meta.function-call", 7)];
        assert_eq!(winner(&ancestor, &["meta.function-call", "entity.name.function"]), Some(6));
    }

    #[test]
    fn each_property_comes_from_the_best_rule_that_sets_it() {
        let mut italic = rule("comment.block.documentation", 0);
        italic.foreground = None;
        italic.font_style = Some(FontStyle::parse("italic underline"));
        let rules = [rule("comment", 9), italic];
        let found = resolve(&rules, &["comment.block.documentation"]).unwrap_or_default();
        assert_eq!(found.foreground, Some(Rgba::rgb(9, 0, 0)));
        assert_eq!(found.font_style, Some(FontStyle { italic: true, underline: true, ..FontStyle::default() }));
        assert_eq!(resolve(&rules, &["comment.line"]).and_then(|found| found.font_style), None);
    }

    #[test]
    fn font_styles_are_words() {
        assert_eq!(
            FontStyle::parse(" bold  strikethrough unknown"),
            FontStyle { bold: true, strikethrough: true, ..FontStyle::default() }
        );
        assert_eq!(FontStyle::parse(""), FontStyle::default());
    }
}

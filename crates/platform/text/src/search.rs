use std::ops::Range;

use regex::{Captures, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

/// What to look for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    /// The text or the regular expression.
    pub text: String,
    /// Match case (Cc).
    pub match_case: bool,
    /// Whole words only (W).
    pub whole_words: bool,
    /// A regular expression (.*).
    pub regex: bool,
    /// Where in the code a match may start: anywhere, in comments, in string literals or outside them.
    pub context: SearchContext,
    /// Keep the case of each match in its replacement (Preserve Case); the search then ignores case.
    pub preserve_case: bool,
}

/// Where in the code a match counts, by the token its start lies in (the search's Filter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchContext {
    /// Anywhere.
    #[default]
    Anywhere,
    /// Only in comments.
    InComments,
    /// Only in string literals.
    InStringLiterals,
    /// Anywhere but in comments.
    ExceptComments,
    /// Anywhere but in string literals.
    ExceptStringLiterals,
    /// Anywhere but in comments and string literals.
    ExceptCommentsAndStringLiterals,
}

impl SearchContext {
    pub const ALL: [SearchContext; 6] = [
        SearchContext::Anywhere,
        SearchContext::InComments,
        SearchContext::InStringLiterals,
        SearchContext::ExceptComments,
        SearchContext::ExceptStringLiterals,
        SearchContext::ExceptCommentsAndStringLiterals,
    ];

    pub fn allows(self, comment: bool, string: bool) -> bool {
        match self {
            SearchContext::Anywhere => true,
            SearchContext::InComments => comment,
            SearchContext::InStringLiterals => string,
            SearchContext::ExceptComments => !comment,
            SearchContext::ExceptStringLiterals => !string,
            SearchContext::ExceptCommentsAndStringLiterals => !comment && !string,
        }
    }
}

impl Query {
    fn compile(&self) -> Result<Regex, String> {
        let pattern = if self.regex { self.text.clone() } else { regex::escape(&self.text) };
        let pattern = if self.whole_words { format!(r"\b(?:{pattern})\b") } else { pattern };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.match_case || self.preserve_case)
            .multi_line(true)
            .build()
            .map_err(|error| error.to_string())
    }
}

pub fn find_all(text: &str, query: &Query) -> Result<Vec<Range<usize>>, String> {
    if query.text.is_empty() {
        return Ok(Vec::new());
    }
    let regex = query.compile()?;
    let mut ranges = Vec::new();
    let mut chars = 0;
    let mut bytes = 0;
    for found in regex.find_iter(text).filter(|found| !found.is_empty()) {
        chars += text[bytes..found.start()].chars().count();
        let start = chars;
        chars += found.as_str().chars().count();
        bytes = found.end();
        ranges.push(start..chars);
    }
    Ok(ranges)
}

pub fn escape(text: &str) -> String {
    regex::escape(text)
}

pub fn replacement(text: &str, range: Range<usize>, query: &Query, replace: &str) -> Result<String, String> {
    let start = text.char_indices().nth(range.start).map_or(text.len(), |(byte, _)| byte);
    let end = text.char_indices().nth(range.end).map_or(text.len(), |(byte, _)| byte);
    let new = if query.regex {
        query
            .compile()?
            .captures_at(text, start)
            .filter(|captures| captures.get(0).is_some_and(|m| m.start() == start && m.end() == end))
            .map_or_else(|| replace.to_owned(), |captures| expand(&captures, replace))
    } else {
        replace.to_owned()
    };
    Ok(if query.preserve_case { keep_case(&text[start..end], new) } else { new })
}

pub fn replacements(text: &str, query: &Query, replace: &str) -> Result<Vec<(Range<usize>, String)>, String> {
    replacements_within(text, query, replace, 0..usize::MAX)
}

pub fn replacements_within(
    text: &str,
    query: &Query,
    replace: &str,
    within: Range<usize>,
) -> Result<Vec<(Range<usize>, String)>, String> {
    if query.text.is_empty() {
        return Ok(Vec::new());
    }
    let regex = query.compile()?;
    let mut out = Vec::new();
    let mut chars = 0;
    let mut bytes = 0;
    for found in regex.find_iter(text) {
        if found.is_empty() {
            continue;
        }
        chars += text[bytes..found.start()].chars().count();
        let start = chars;
        chars += found.as_str().chars().count();
        bytes = found.end();
        if start >= within.end {
            break;
        }
        if start < within.start {
            continue;
        }
        let new = if query.regex {
            regex
                .captures_at(text, found.start())
                .filter(|captures| captures.get(0).is_some_and(|m| m.range() == found.range()))
                .map_or_else(|| replace.to_owned(), |captures| expand(&captures, replace))
        } else {
            replace.to_owned()
        };
        let new = if query.preserve_case { keep_case(found.as_str(), new) } else { new };
        out.push((start..chars, new));
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Upper,
    Lower,
}

fn convert(text: &str, case: Case) -> String {
    match case {
        Case::Upper => text.to_uppercase(),
        Case::Lower => text.to_lowercase(),
    }
}

fn with_first(text: &str, case: Case) -> String {
    match text.char_indices().find(|(_, c)| c.is_alphabetic()) {
        Some((at, c)) => {
            let first = convert(&c.to_string(), case);
            format!("{}{first}{}", &text[..at], &text[at + c.len_utf8()..])
        }
        None => text.to_owned(),
    }
}

fn expand(captures: &Captures, template: &str) -> String {
    let mut out = String::new();
    let mut chunk = String::new();
    let mut mode: Option<Case> = None;
    let mut next: Option<Case> = None;
    let mut flush = |chunk: &mut String, mode: Option<Case>, next: &mut Option<Case>| {
        if chunk.is_empty() {
            return;
        }
        let mut expanded = String::new();
        captures.expand(chunk, &mut expanded);
        chunk.clear();
        if let Some(case) = mode {
            expanded = convert(&expanded, case);
        }
        if !expanded.is_empty()
            && let Some(case) = next.take()
        {
            let first = expanded.chars().next().map_or(0, char::len_utf8);
            expanded = format!("{}{}", convert(&expanded[..first], case), &expanded[first..]);
        }
        out.push_str(&expanded);
    };
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&escaped) = chars.peek()
        {
            match escaped {
                'U' | 'L' | 'E' | 'u' | 'l' => {
                    flush(&mut chunk, mode, &mut next);
                    match escaped {
                        'U' => mode = Some(Case::Upper),
                        'L' => mode = Some(Case::Lower),
                        'E' => mode = None,
                        'u' => next = Some(Case::Upper),
                        _ => next = Some(Case::Lower),
                    }
                }
                '\\' => chunk.push('\\'),
                '$' => chunk.push_str("$$"),
                _ => {
                    chunk.push(c);
                    continue;
                }
            }
            chars.next();
            continue;
        }
        chunk.push(c);
    }
    flush(&mut chunk, mode, &mut next);
    out
}

fn keep_case(found: &str, new: String) -> String {
    let letters: Vec<char> = found.chars().filter(|c| c.is_uppercase() || c.is_lowercase()).collect();
    let Some((&first, tail)) = letters.split_first() else { return new };
    let tail_upper = !tail.is_empty() && tail.iter().all(|c| c.is_uppercase());
    let tail_lower = !tail.is_empty() && tail.iter().all(|c| c.is_lowercase());
    let case = if first.is_uppercase() { Case::Upper } else { Case::Lower };
    if tail_upper && case == Case::Upper {
        new.to_uppercase()
    } else if tail_lower && case == Case::Lower {
        new.to_lowercase()
    } else if tail_upper && is_one_case(&new) {
        with_first(&new.to_uppercase(), case)
    } else if tail_lower && is_one_case(&new) {
        with_first(&new.to_lowercase(), case)
    } else {
        with_first(&new, case)
    }
}

fn is_one_case(text: &str) -> bool {
    let mut letters = text.chars().filter(|c| c.is_uppercase() || c.is_lowercase()).peekable();
    let Some(first) = letters.peek().copied() else { return true };
    letters.all(|c| c.is_uppercase() == first.is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str) -> Query {
        Query { text: text.to_owned(), ..Query::default() }
    }

    #[test]
    fn plain_text_ignores_case_unless_asked() {
        let text = "Item item ITEM items";
        assert_eq!(find_all(text, &query("item")).unwrap(), [0..4, 5..9, 10..14, 15..19]);
        let cased = Query { match_case: true, ..query("item") };
        assert_eq!(find_all(text, &cased).unwrap(), [5..9, 15..19]);
        let words = Query { whole_words: true, ..query("item") };
        assert_eq!(find_all(text, &words).unwrap(), [0..4, 5..9, 10..14]);
        assert_eq!(find_all(text, &query("i.em")).unwrap(), Vec::<Range<usize>>::new(), "a dot is a dot");
    }

    #[test]
    fn contexts_allow_by_token() {
        use SearchContext::*;
        let allowed = |context: SearchContext| {
            [(false, false), (true, false), (false, true)].map(|(comment, string)| context.allows(comment, string))
        };
        assert_eq!(allowed(Anywhere), [true, true, true]);
        assert_eq!(allowed(InComments), [false, true, false]);
        assert_eq!(allowed(InStringLiterals), [false, false, true]);
        assert_eq!(allowed(ExceptComments), [true, false, true]);
        assert_eq!(allowed(ExceptStringLiterals), [true, true, false]);
        assert_eq!(allowed(ExceptCommentsAndStringLiterals), [true, false, false]);
        assert_eq!(SearchContext::ALL[0], SearchContext::default());
    }

    #[test]
    fn offsets_are_characters() {
        assert_eq!(find_all("привет мир, мир", &query("мир")).unwrap(), [7..10, 12..15]);
    }

    #[test]
    fn regular_expressions_and_groups() {
        let regex = Query { regex: true, match_case: true, ..query(r"(\w+)\.Add\((\w+)\)") };
        let text = "cart.Add(tea)\nbox.Add(cup)";
        let found = find_all(text, &regex).unwrap();
        assert_eq!(found, [0..13, 14..26]);
        assert_eq!(replacement(text, found[1].clone(), &regex, "$1.Put($2)").unwrap(), "box.Put(cup)");
        assert!(find_all(text, &Query { regex: true, ..query("(") }).is_err());
        assert_eq!(
            find_all(text, &Query { regex: true, ..query("^") }).unwrap(),
            Vec::<Range<usize>>::new(),
            "empty matches are left out"
        );
    }

    #[test]
    fn case_conversion_in_the_replacement() {
        let regex = Query { regex: true, match_case: true, ..query(r"(\w+)_(\w+)") };
        let text = "user_name";
        let range = 0..9;
        let with = |replace: &str| replacement(text, range.clone(), &regex, replace).unwrap();
        assert_eq!(with(r"\U$1\E_$2"), "USER_name");
        assert_eq!(with(r"$1\u$2"), "userName");
        assert_eq!(with(r"\U$1$2"), "USERNAME", "without \\E to the end");
        assert_eq!(with(r"\u\L$1$2"), "Username", "\\u wins over \\L for the first letter");
        assert_eq!(with(r"\l\UX$1"), "xUSER");
        assert_eq!(with(r"\L${1}X\E-$2"), "userx-name");
        assert_eq!(with(r"\\u\$1"), r"\u$1", "escaped backslash and dollar stay");
        assert_eq!(with(r"a\tb"), r"a\tb", "other escapes stay as written");
        let plain = query("user");
        assert_eq!(replacement(text, 0..4, &plain, r"\Ux").unwrap(), r"\Ux", "plain text takes the replacement as is");
    }

    #[test]
    fn preserve_case_keeps_the_case_of_each_match() {
        let preserve = Query { preserve_case: true, match_case: true, ..query("fooBar") };
        let text = "fooBar FooBar FOOBAR foobar";
        let all = replacements(text, &preserve, "bazQux").unwrap();
        let texts: Vec<&str> = all.iter().map(|(_, new)| new.as_str()).collect();
        assert_eq!(texts, ["bazQux", "BazQux", "BAZQUX", "bazqux"], "Preserve Case ignores Match Case");
        assert_eq!(all[1].0, 7..13);
        assert_eq!(replacement(text, 14..20, &preserve, "bazQux").unwrap(), "BAZQUX");
        let regex = Query { regex: true, preserve_case: true, ..query(r"get(\w+)") };
        assert_eq!(replacement("GetName", 0..7, &regex, "fetch$1").unwrap(), "FetchName");
        let single = Query { preserve_case: true, ..query("x") };
        assert_eq!(replacements("X x", &single, "yes").unwrap(), [(0..1, "Yes".to_owned()), (2..3, "yes".to_owned())]);
        assert_eq!(replacements("1+2", &Query { preserve_case: true, ..query("+") }, "plus").unwrap()[0].1, "plus");
        let word = Query { preserve_case: true, ..query("user") };
        let shown = |text: &str, replace: &str| replacements(text, &word, replace).unwrap()[0].1.clone();
        assert_eq!(shown("User", "ACCOUNT"), "Account", "a capitalized match capitalizes a one-case replacement");
        assert_eq!(shown("User", "bazQux"), "BazQux", "and keeps a mixed one as written");
        assert_eq!(shown("uSER", "account"), "aCCOUNT", "a small letter and then capitals");
        assert_eq!(shown("uSER", "bazQux"), "bazQux");
        assert_eq!(shown("USER", "bazQux"), "BAZQUX", "all capitals win over a mixed replacement");
        assert_eq!(shown("user", "bazQux"), "bazqux", "and so do all small letters");
        assert_eq!(shown("uSeR", "account"), "account", "a mixed match sets the first letter only");
        let id = Query { regex: true, preserve_case: true, ..query(r"_\w+") };
        assert_eq!(replacement("_User1", 0..6, &id, "_account2").unwrap(), "_Account2", "letters only decide");
    }

    #[test]
    fn replacements_agree_with_one_replacement_at_a_time() {
        let regex = Query { regex: true, ..query(r"(\w)(\w*)") };
        let text = "мир tea, Box";
        let all = replacements(text, &regex, r"\U$1\E$2!").unwrap();
        assert_eq!(all.iter().map(|(range, _)| range.clone()).collect::<Vec<_>>(), find_all(text, &regex).unwrap());
        for (range, new) in &all {
            assert_eq!(&replacement(text, range.clone(), &regex, r"\U$1\E$2!").unwrap(), new);
        }
        assert_eq!(all[0].1, "Мир!");
        assert!(replacements(text, &query(""), "x").unwrap().is_empty());
        assert!(replacements(text, &Query { regex: true, ..query("(") }, "x").is_err());
        let some = replacements_within(text, &regex, r"\U$1\E$2!", 4..9).unwrap();
        assert_eq!(some, all[1..2], "only the matches that start in the range");
    }
}

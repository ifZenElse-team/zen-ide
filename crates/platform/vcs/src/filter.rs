use crate::{Commit, LogQuery, VcsError};

pub struct CommitFilter {
    text: Option<TextMatcher>,
    authors: Vec<String>,
    since: Option<i64>,
    until: Option<i64>,
}

#[derive(Clone, Debug)]
pub enum TextMatcher {
    Plain { text: String, match_case: bool },
    Regex(regex::Regex),
}

impl TextMatcher {
    pub fn new(text: &str, regex: bool, match_case: bool) -> Result<Option<Self>, VcsError> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        if regex {
            return regex::RegexBuilder::new(text)
                .case_insensitive(!match_case)
                .build()
                .map(|regex| Some(TextMatcher::Regex(regex)))
                .map_err(|error| VcsError::Git(error.to_string()));
        }
        let text = if match_case { text.to_owned() } else { text.to_lowercase() };
        Ok(Some(TextMatcher::Plain { text, match_case }))
    }

    pub fn matches(&self, part: &str) -> bool {
        match self {
            TextMatcher::Plain { text, match_case: true } => part.contains(text.as_str()),
            TextMatcher::Plain { text, match_case: false } if text.is_ascii() && part.is_ascii() => {
                contains_any_case(part.as_bytes(), text.as_bytes())
            }
            TextMatcher::Plain { text, match_case: false } => part.to_lowercase().contains(text.as_str()),
            TextMatcher::Regex(regex) => regex.is_match(part),
        }
    }

    pub fn find(&self, part: &str) -> Option<std::ops::Range<usize>> {
        match self {
            TextMatcher::Plain { text, match_case: true } => {
                part.find(text.as_str()).map(|start| start..start + text.len())
            }
            TextMatcher::Plain { text, match_case: false } => {
                let folded = part.to_lowercase();
                (folded.len() == part.len())
                    .then(|| folded.find(text.as_str()).map(|start| start..start + text.len()))
                    .flatten()
            }
            TextMatcher::Regex(regex) => regex.find(part).filter(|found| !found.is_empty()).map(|found| found.range()),
        }
    }

    pub fn matches_id(&self, id: &str) -> bool {
        match self {
            TextMatcher::Plain { text, match_case: false } => id.starts_with(text.as_str()),
            TextMatcher::Plain { text, match_case: true } => id.starts_with(&text.to_lowercase()),
            TextMatcher::Regex(_) => false,
        }
    }

    pub fn matches_oid(&self, id: &crate::history::Oid) -> bool {
        match self {
            TextMatcher::Plain { text, match_case: false } => id.starts_with_hex(text),
            TextMatcher::Plain { text, match_case: true } => id.starts_with_hex(&text.to_lowercase()),
            TextMatcher::Regex(_) => false,
        }
    }
}

fn contains_any_case(haystack: &[u8], needle: &[u8]) -> bool {
    let Some((&first, rest)) = needle.split_first() else { return true };
    if haystack.len() < needle.len() {
        return false;
    }
    let upper = first.to_ascii_uppercase();
    (0..=haystack.len() - needle.len()).any(|at| {
        let byte = haystack[at];
        (byte == first || byte == upper) && haystack[at + 1..at + needle.len()].eq_ignore_ascii_case(rest)
    })
}

impl CommitFilter {
    pub fn new(query: &LogQuery) -> Result<Self, VcsError> {
        let text = TextMatcher::new(query.text.as_deref().unwrap_or_default(), query.regex, query.match_case)?;
        let authors = query
            .author
            .iter()
            .chain(&query.authors)
            .map(|author| author.trim().to_lowercase())
            .filter(|author| !author.is_empty())
            .collect();
        Ok(CommitFilter { text, authors, since: query.since, until: query.until })
    }

    pub fn matches(&self, commit: &Commit) -> bool {
        if self.since.is_some_and(|since| commit.time < since) || self.until.is_some_and(|until| commit.time > until) {
            return false;
        }
        if !self.authors.is_empty() {
            let (name, email) = (commit.author.to_lowercase(), commit.email.to_lowercase());
            if !self.authors.iter().any(|author| name.contains(author.as_str()) || email.contains(author.as_str())) {
                return false;
            }
        }
        match &self.text {
            None => true,
            Some(text) => text.matches_id(&commit.id) || text.matches(&commit.subject) || text.matches(&commit.body),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(subject: &str, author: &str, time: i64) -> Commit {
        Commit {
            id: "abc123def".to_owned(),
            parents: Vec::new(),
            author: author.to_owned(),
            email: format!("{}@example.com", author.to_lowercase()),
            time,
            offset: 0,
            subject: subject.to_owned(),
            body: "Body text".to_owned(),
        }
    }

    fn passes(query: LogQuery, commit: &Commit) -> bool {
        CommitFilter::new(&query).unwrap().matches(commit)
    }

    #[test]
    fn text_matches_the_message_or_the_start_of_the_id() {
        let fix = commit("Fix the Parser", "Ada", 100);
        let text = |text: &str| LogQuery { text: Some(text.into()), ..LogQuery::default() };
        assert!(passes(text("parser"), &fix), "any case by default");
        assert!(passes(text("body"), &fix), "the body too");
        assert!(passes(text("abc1"), &fix), "the id's start");
        assert!(passes(text("ABC1"), &fix), "the id's start in any case");
        assert!(!passes(text("123"), &fix), "not the id's middle");
        assert!(!passes(text("lexer"), &fix));
    }

    #[test]
    fn match_case_and_regular_expressions() {
        let fix = commit("Fix the Parser", "Ada", 100);
        let query = |text: &str, regex: bool, match_case: bool| LogQuery {
            text: Some(text.into()),
            regex,
            match_case,
            ..LogQuery::default()
        };
        assert!(passes(query("Parser", false, true), &fix));
        assert!(!passes(query("parser", false, true), &fix));
        assert!(passes(query("^fix .*parser$", true, false), &fix));
        assert!(!passes(query("^fix .*parser$", true, true), &fix));
        assert!(passes(query("P[a-z]+r", true, true), &fix));
        assert!(CommitFilter::new(&query("(", true, false)).is_err(), "an invalid expression is an error");
    }

    #[test]
    fn authors_and_dates() {
        let fix = commit("Fix", "Ada Lovelace", 1_000);
        let authors = |names: &[&str]| LogQuery {
            authors: names.iter().map(|name| name.to_string()).collect(),
            ..LogQuery::default()
        };
        assert!(passes(authors(&["grace", "ada"]), &fix), "any of them");
        assert!(passes(authors(&["LOVELACE@"]), &fix), "the e-mail, any case");
        assert!(!passes(authors(&["grace"]), &fix));
        let dates = |since: Option<i64>, until: Option<i64>| LogQuery { since, until, ..LogQuery::default() };
        assert!(passes(dates(Some(1_000), Some(1_000)), &fix), "both ends included");
        assert!(!passes(dates(Some(1_001), None), &fix));
        assert!(!passes(dates(None, Some(999)), &fix));
    }

    #[test]
    fn a_text_matcher_matches_in_any_case_without_lowercasing_ascii() {
        assert!(TextMatcher::new("  ", false, false).unwrap().is_none(), "nothing to look for");
        let plain = TextMatcher::new("Parser", false, false).unwrap().unwrap();
        assert!(plain.matches("fix the PARSER"));
        assert!(plain.matches("pArSeR") && !plain.matches("parse") && !plain.matches(""));
        assert!(plain.matches("Привет, PARSER"), "text that is not ASCII is lowercased first");
        let cyrillic = TextMatcher::new("ПРИВЕТ", false, false).unwrap().unwrap();
        assert!(cyrillic.matches("скажи привет") && !cyrillic.matches("hello"));
        assert!(TextMatcher::new("AB12", false, false).unwrap().unwrap().matches_id("ab12ff"), "an id's start");
        let id = crate::history::Oid::from_hex("ab12ff0000000000000000000000000000000000").unwrap();
        assert!(TextMatcher::new("AB12", false, false).unwrap().unwrap().matches_oid(&id));
        assert!(!TextMatcher::new("ab13", false, false).unwrap().unwrap().matches_oid(&id));
        let exact = TextMatcher::new("Parser", false, true).unwrap().unwrap();
        assert!(!exact.matches("fix the parser"));
        assert!(exact.matches("the Parser"));
        let regex = TextMatcher::new("^ab+c$", true, false).unwrap().unwrap();
        assert!(regex.matches("ABBC"));
        assert!(!regex.matches_id("abc"), "a regular expression does not look at ids");
        assert!(!regex.matches_oid(&id));
        assert!(TextMatcher::new("(", true, false).is_err());
        assert_eq!(plain.find("Fix the PARSER now"), Some(8..14));
        assert_eq!(exact.find("the parser"), None);
        assert_eq!(regex.find("xABBC"), None);
        assert_eq!(TextMatcher::new("b+", true, false).unwrap().unwrap().find("aBBc"), Some(1..3));
    }
}

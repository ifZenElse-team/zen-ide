use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_text::search::Query;

pub const SETTINGS: &str = "todo.settings";

pub const DEFAULT_PATTERNS: [&str; 2] = [r"\btodo\b.*", r"\bfixme\b.*"];

const NOTHING: &str = r"\b\B";

/// The icon of a TODO pattern's items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TodoIcon {
    /// The TODO mark.
    #[default]
    Default,
    /// A question mark.
    Question,
    /// An exclamation mark.
    Important,
}

/// A TODO pattern of Settings → Editor → TODO.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoPattern {
    /// The regular expression.
    pub pattern: String,
    /// Whether case matters.
    pub case_sensitive: bool,
    /// The icon of its items.
    pub icon: TodoIcon,
    /// Its own text colour as `#rrggbb`; the colour scheme's TODO colour when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl Default for TodoPattern {
    fn default() -> Self {
        Self { pattern: String::new(), case_sensitive: false, icon: TodoIcon::Default, color: None }
    }
}

impl TodoPattern {
    fn of(pattern: &str) -> Self {
        Self { pattern: pattern.to_owned(), ..Self::default() }
    }

    fn source(&self) -> Option<String> {
        let flag = if self.case_sensitive { "" } else { "(?i)" };
        let start = self.pattern.strip_suffix(".*").unwrap_or(&self.pattern);
        Regex::new(&self.pattern).ok().map(|_| format!("(?:{flag}{start})"))
    }
}

/// A named set of TODO patterns of Settings → Editor → TODO.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoFilter {
    /// The filter's name.
    pub name: String,
    /// The patterns it shows, by their text.
    pub patterns: Vec<String>,
}

/// Settings → Editor → TODO and the Commit tool window's Check TODO.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoSettings {
    /// The patterns.
    pub patterns: Vec<TodoPattern>,
    /// The filters.
    pub filters: Vec<TodoFilter>,
    /// Continue a TODO on the indented lines below it.
    pub multiline: bool,
    /// Check TODO before a commit.
    pub commit_check: bool,
    /// The filter Check TODO uses; every pattern when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_filter: Option<String>,
}

impl Default for TodoSettings {
    fn default() -> Self {
        Self {
            patterns: DEFAULT_PATTERNS.iter().map(|pattern| TodoPattern::of(pattern)).collect(),
            filters: Vec::new(),
            multiline: true,
            commit_check: false,
            commit_filter: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TodoPatterns {
    settings: TodoSettings,
}

impl TodoPatterns {
    pub fn of(session: &Session) -> Self {
        Self { settings: session.settings().get_or(SETTINGS, TodoSettings::default()) }
    }

    pub fn settings(&self) -> &TodoSettings {
        &self.settings
    }

    pub fn patterns(&self) -> Vec<String> {
        self.settings.patterns.iter().map(|pattern| pattern.pattern.clone()).collect()
    }

    pub fn words(&self) -> Vec<String> {
        let mut words: Vec<String> = Vec::new();
        for word in self.settings.patterns.iter().filter_map(|pattern| word(&pattern.pattern)) {
            if !words.contains(&word) {
                words.push(word);
            }
        }
        words
    }

    pub fn query(&self) -> Query {
        let sources: Vec<String> = self.settings.patterns.iter().filter_map(TodoPattern::source).collect();
        let text = if sources.is_empty() { NOTHING.to_owned() } else { sources.join("|") };
        Query { text, regex: true, match_case: true, ..Query::default() }
    }

    pub fn compiled(&self) -> CompiledPatterns {
        let compiled = self.settings.patterns.iter().enumerate();
        let list = compiled.filter_map(|(at, pattern)| Some((Regex::new(&pattern.source()?).ok()?, at))).collect();
        CompiledPatterns { list, settings: self.settings.clone() }
    }
}

#[derive(Clone, Debug)]
pub struct CompiledPatterns {
    list: Vec<(Regex, usize)>,
    settings: TodoSettings,
}

impl CompiledPatterns {
    pub fn has_match(&self, text: &str) -> bool {
        self.list.iter().any(|(regex, _)| regex.is_match(text))
    }

    pub fn pattern_at_start(&self, text: &str) -> Option<&TodoPattern> {
        let at_start = |(regex, _): &&(Regex, usize)| regex.find(text).is_some_and(|found| found.start() == 0);
        self.list.iter().find(at_start).map(|(_, at)| &self.settings.patterns[*at])
    }

    pub fn passes(&self, filter: Option<&str>, text: &str) -> bool {
        let Some(filter) = filter.and_then(|name| self.settings.filters.iter().find(|filter| filter.name == name))
        else {
            return true;
        };
        self.pattern_at_start(text).is_some_and(|pattern| filter.patterns.contains(&pattern.pattern))
    }
}

pub fn is_valid(pattern: &str) -> bool {
    Regex::new(pattern).is_ok()
}

pub fn word(pattern: &str) -> Option<String> {
    let rest = pattern.strip_prefix(r"\b").unwrap_or(pattern);
    let word: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!word.is_empty()).then(|| word.to_uppercase())
}

/// What `todo.patterns.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatternList {
    /// The patterns, regular expressions (Settings → Editor → TODO).
    pub patterns: Vec<String>,
    /// The words they start with, which completion offers in comments.
    pub words: Vec<String>,
    /// The filters' names.
    pub filters: Vec<String>,
}

pub struct ListPatterns;

impl Command for ListPatterns {
    type Params = NoParams;
    type Output = PatternList;
    const ID: &'static str = "todo.patterns.list";
    const TITLE: &'static str = "TODO Patterns";
    const DESCRIPTION: &'static str = "The TODO patterns (regular expressions, set with set_settings and `todo`), the words \
         they start with, which completion offers in comments, and the names of the filters.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<PatternList, CommandError> {
        let patterns = TodoPatterns::of(session);
        let filters = patterns.settings.filters.iter().map(|filter| filter.name.clone()).collect();
        Ok(PatternList { words: patterns.words(), patterns: patterns.patterns(), filters })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListPatterns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(patterns: &[&str]) -> TodoPatterns {
        let patterns = patterns.iter().map(|pattern| TodoPattern::of(pattern)).collect();
        TodoPatterns { settings: TodoSettings { patterns, ..TodoSettings::default() } }
    }

    #[test]
    fn the_words_are_those_the_patterns_start_with() {
        assert_eq!(TodoPatterns::default().words(), vec!["TODO".to_owned(), "FIXME".to_owned()]);
        assert_eq!(word(r"\bhack\b.*").as_deref(), Some("HACK"));
        assert_eq!(word("note:.*").as_deref(), Some("NOTE"));
        assert_eq!(word(r"(?:a|b)"), None);
        assert_eq!(with(&[r"\btodo\b.*", r"\bTODO\(\w+\)"]).words(), vec!["TODO".to_owned()]);
    }

    #[test]
    fn the_patterns_are_listed_with_their_words() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let listed = registry.dispatch(&mut session, "todo.patterns.list", serde_json::json!({})).unwrap();
        assert_eq!(
            listed,
            serde_json::json!({ "patterns": [r"\btodo\b.*", r"\bfixme\b.*"], "words": ["TODO", "FIXME"], "filters": [] })
        );
    }

    #[test]
    fn case_matters_only_for_a_pattern_that_says_so_and_a_bad_pattern_is_left_out() {
        let mut patterns = with(&[r"\bhack\b.*", "(", r"\bnote\b.*"]);
        patterns.settings.patterns[2].case_sensitive = true;
        let query = patterns.query();
        let found = |text: &str| zen_text::search::find_all(text, &query).unwrap().len();
        assert_eq!(found("HACK here"), 1);
        assert_eq!(found("NOTE here"), 0);
        assert_eq!(found("note here"), 1);
        assert_eq!(found("nothing"), 0);
        let none = with(&[]).query();
        assert!(zen_text::search::find_all("todo", &none).unwrap().is_empty(), "no patterns find nothing");
    }

    #[test]
    fn a_filter_passes_the_items_of_its_patterns() {
        let mut patterns = TodoPatterns::default();
        patterns
            .settings
            .filters
            .push(TodoFilter { name: "bugs".to_owned(), patterns: vec![r"\bfixme\b.*".to_owned()] });
        let compiled = patterns.compiled();
        assert!(compiled.passes(Some("bugs"), "FIXME: broken"));
        assert!(!compiled.passes(Some("bugs"), "TODO: later"));
        assert!(compiled.passes(None, "TODO: later"));
        assert!(compiled.passes(Some("gone"), "TODO: later"), "an unknown filter shows everything");
    }
}

use std::sync::Arc;

pub use crate::text_search::glob;

#[derive(Clone)]
pub struct FileFilter(Arc<dyn Fn(&str) -> bool + Send + Sync>);

impl FileFilter {
    pub fn new(keep: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(keep))
    }

    pub fn all() -> Self {
        Self::new(|_| true)
    }

    pub fn accepts(&self, relative: &str) -> bool {
        (self.0)(relative)
    }

    pub fn and(self, other: FileFilter) -> Self {
        Self::new(move |relative| self.accepts(relative) && other.accepts(relative))
    }
}

impl PartialEq for FileFilter {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FileFilter {}

impl std::fmt::Debug for FileFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileFilter")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    Empty,
    Unexpected(usize),
    Unsupported(String),
    UnknownScope(String),
    Cycle(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    File { module: Option<String>, path: String },
    Named(String),
    Not(Box<Pattern>),
    And(Box<Pattern>, Box<Pattern>),
    Or(Box<Pattern>, Box<Pattern>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Not,
    And,
    Or,
    Named(String),
    File(Option<String>, String),
}

const STOP: &[char] = &['(', ')', '!', '&', '|'];

fn tokens(text: &str) -> Result<Vec<(usize, Token)>, ScopeError> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let start = at;
        if c.is_whitespace() {
            at += 1;
            continue;
        }
        let pair = |second: char| chars.get(at + 1) == Some(&second);
        let token = match c {
            '(' => Token::Open,
            ')' => Token::Close,
            '!' => Token::Not,
            '&' if pair('&') => {
                at += 1;
                Token::And
            }
            '|' if pair('|') => {
                at += 1;
                Token::Or
            }
            '&' | '|' => return Err(ScopeError::Unexpected(at)),
            '$' => {
                let mut end = at + 1;
                while end < chars.len() && !chars[end].is_whitespace() && !STOP.contains(&chars[end]) {
                    end += 1;
                }
                if end == at + 1 {
                    return Err(ScopeError::Unexpected(at));
                }
                let name: String = chars[at + 1..end].iter().collect();
                at = end - 1;
                Token::Named(name)
            }
            _ => {
                let mut end = at;
                while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let kind: String = chars[at..end].iter().collect();
                let mut module = None;
                if chars.get(end) == Some(&'[') {
                    let close = chars[end..].iter().position(|c| *c == ']').ok_or(ScopeError::Unexpected(end))?;
                    module = Some(chars[end + 1..end + close].iter().collect::<String>());
                    end += close + 1;
                }
                if kind.is_empty() || chars.get(end) != Some(&':') {
                    return Err(ScopeError::Unexpected(end.min(chars.len())));
                }
                if kind != "file" {
                    return Err(ScopeError::Unsupported(kind));
                }
                let mut stop = end + 1;
                while stop < chars.len()
                    && !chars[stop].is_whitespace()
                    && chars[stop] != ')'
                    && !(chars[stop] == '&' && chars.get(stop + 1) == Some(&'&'))
                    && !(chars[stop] == '|' && chars.get(stop + 1) == Some(&'|'))
                {
                    stop += 1;
                }
                let path: String = chars[end + 1..stop].iter().collect();
                if path.is_empty() {
                    return Err(ScopeError::Unexpected(end + 1));
                }
                at = stop - 1;
                Token::File(module, path)
            }
        };
        found.push((start, token));
        at += 1;
    }
    Ok(found)
}

struct Parser {
    tokens: Vec<(usize, Token)>,
    at: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(_, token)| token)
    }

    fn offset(&self) -> usize {
        self.tokens.get(self.at).map_or(self.end, |(offset, _)| *offset)
    }

    fn or(&mut self) -> Result<Pattern, ScopeError> {
        let mut left = self.and()?;
        while self.peek() == Some(&Token::Or) {
            self.at += 1;
            left = Pattern::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Pattern, ScopeError> {
        let mut left = self.unary()?;
        while self.peek() == Some(&Token::And) {
            self.at += 1;
            left = Pattern::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Pattern, ScopeError> {
        let offset = self.offset();
        let Some((_, token)) = self.tokens.get(self.at).cloned() else { return Err(ScopeError::Unexpected(offset)) };
        self.at += 1;
        match token {
            Token::Not => Ok(Pattern::Not(Box::new(self.unary()?))),
            Token::Open => {
                let inner = self.or()?;
                if self.peek() != Some(&Token::Close) {
                    return Err(ScopeError::Unexpected(self.offset()));
                }
                self.at += 1;
                Ok(inner)
            }
            Token::Named(name) => Ok(Pattern::Named(name)),
            Token::File(module, path) => Ok(Pattern::File { module, path }),
            Token::Close | Token::And | Token::Or => Err(ScopeError::Unexpected(offset)),
        }
    }
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Pattern, ScopeError> {
        let tokens = tokens(text)?;
        if tokens.is_empty() {
            return Err(ScopeError::Empty);
        }
        let mut parser = Parser { tokens, at: 0, end: text.chars().count() };
        let pattern = parser.or()?;
        if parser.at < parser.tokens.len() {
            return Err(ScopeError::Unexpected(parser.offset()));
        }
        Ok(pattern)
    }

    fn names(&self, into: &mut Vec<String>) {
        match self {
            Pattern::File { .. } => {}
            Pattern::Named(name) => into.push(name.clone()),
            Pattern::Not(inner) => inner.names(into),
            Pattern::And(a, b) | Pattern::Or(a, b) => {
                a.names(into);
                b.names(into);
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeSet {
    named: Vec<(String, Result<Pattern, ScopeError>)>,
    modules: Vec<(String, String)>,
}

impl ScopeSet {
    pub fn new(modules: Vec<(String, String)>) -> Self {
        Self { named: Vec::new(), modules }
    }

    pub fn add(&mut self, name: &str, pattern: &str) {
        self.named.retain(|(own, _)| own != name);
        self.named.push((name.to_owned(), Pattern::parse(pattern)));
    }

    pub fn named(&self, name: &str) -> Option<&Result<Pattern, ScopeError>> {
        self.named.iter().find(|(own, _)| own == name).map(|(_, pattern)| pattern)
    }

    pub fn check(&self, pattern: &Pattern) -> Result<(), ScopeError> {
        self.check_from(pattern, &mut Vec::new())
    }

    fn check_from(&self, pattern: &Pattern, path: &mut Vec<String>) -> Result<(), ScopeError> {
        let mut names = Vec::new();
        pattern.names(&mut names);
        for name in names {
            if path.contains(&name) {
                return Err(ScopeError::Cycle(name));
            }
            let inner = self.named(&name).ok_or_else(|| ScopeError::UnknownScope(name.clone()))?.clone()?;
            path.push(name);
            self.check_from(&inner, path)?;
            path.pop();
        }
        Ok(())
    }

    pub fn matches(&self, pattern: &Pattern, relative: &str) -> bool {
        self.matches_at(pattern, &relative.to_lowercase(), 0)
    }

    fn matches_at(&self, pattern: &Pattern, relative: &str, depth: usize) -> bool {
        match pattern {
            Pattern::File { module: None, path } => path_matches(path, relative),
            Pattern::File { module: Some(module), path } => {
                let module = module.to_lowercase();
                self.modules.iter().filter(|(name, _)| glob(&module, &name.to_lowercase())).any(|(_, root)| {
                    let root = root.trim_matches('/').to_lowercase();
                    let inside = if root.is_empty() {
                        Some(relative)
                    } else {
                        relative.strip_prefix(&root).and_then(|rest| rest.strip_prefix('/'))
                    };
                    inside.is_some_and(|inside| path_matches(path, inside))
                })
            }
            Pattern::Named(name) => {
                depth < 32 && matches!(self.named(name), Some(Ok(inner)) if self.matches_at(inner, relative, depth + 1))
            }
            Pattern::Not(inner) => !self.matches_at(inner, relative, depth),
            Pattern::And(a, b) => self.matches_at(a, relative, depth) && self.matches_at(b, relative, depth),
            Pattern::Or(a, b) => self.matches_at(a, relative, depth) || self.matches_at(b, relative, depth),
        }
    }
}

fn path_matches(pattern: &str, relative: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    if !pattern.contains(&'/') {
        let name: Vec<char> = relative.rsplit('/').next().unwrap_or(relative).chars().collect();
        return wild(&pattern, &name);
    }
    let text: Vec<char> = relative.chars().collect();
    wild(&pattern, &text)
}

fn wild(pattern: &[char], text: &[char]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some('/') if pattern.get(1) == Some(&'/') => {
            text.first() == Some(&'/')
                && (0..text.len()).filter(|at| text[*at] == '/').any(|at| wild(&pattern[2..], &text[at + 1..]))
        }
        Some('*') => {
            let mut at = 0;
            loop {
                if wild(&pattern[1..], &text[at..]) {
                    return true;
                }
                if at == text.len() || text[at] == '/' {
                    return false;
                }
                at += 1;
            }
        }
        Some('?') => text.first().is_some_and(|c| *c != '/') && wild(&pattern[1..], &text[1..]),
        Some(c) => text.first() == Some(c) && wild(&pattern[1..], &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> ScopeSet {
        let mut set = ScopeSet::new(vec![
            ("example.com/shop".to_owned(), String::new()),
            ("tools".to_owned(), "tools".to_owned()),
        ]);
        set.add("Tests", "file:*_test.go");
        set.add("Sources", "file:internal//*&&!$Tests");
        set.add("Loop", "$Loop2");
        set.add("Loop2", "$Loop");
        set
    }

    fn matches(pattern: &str, relative: &str) -> bool {
        let set = set();
        set.matches(&Pattern::parse(pattern).unwrap(), relative)
    }

    #[test]
    fn file_patterns_match_names_and_paths() {
        assert!(matches("file:*.go", "internal/cart/cart.go"), "without a slash, the name");
        assert!(!matches("file:*.go", "README.md"));
        assert!(matches("file:internal//*", "internal/cart/cart.go"));
        assert!(matches("file:internal//*", "internal/main.go"));
        assert!(!matches("file:internal//*", "cmd/main.go"));
        assert!(matches("file:internal/*", "internal/main.go"));
        assert!(!matches("file:internal/*", "internal/cart/cart.go"), "one folder only");
        assert!(matches("file:*/cart/*.go", "internal/cart/cart.go"));
        assert!(matches("file:internal//cart.go", "internal/cart/cart.go"));
        assert!(matches("file:internal//cart.go", "internal/cart.go"));
        assert!(matches("file:INTERNAL//*.GO", "internal/cart/cart.go"), "case does not matter");
        assert!(matches("file:ma?n.go", "main.go"));
    }

    #[test]
    fn operators_parentheses_and_named_scopes() {
        assert!(matches("file:*.go&&!file:*_test.go", "cart.go"));
        assert!(!matches("file:*.go&&!file:*_test.go", "cart_test.go"));
        assert!(matches("file:*.md||file:*.go", "README.md"));
        assert!(matches("!(file:*.md || file:*.go)", "go.mod"));
        assert!(!matches("!(file:*.md || file:*.go)", "a.go"));
        assert!(matches("file:a.go||file:b.go&&file:c.go", "a.go"), "&& binds tighter than ||");
        assert!(matches("$Sources", "internal/cart/cart.go"));
        assert!(!matches("$Sources", "internal/cart/cart_test.go"));
        assert!(!matches("$Loop", "a.go"), "a loop matches nothing");
        assert!(matches("file[tools]:*.go", "tools/gen.go"));
        assert!(matches("file[tools]:gen.go", "tools/gen.go"));
        assert!(!matches("file[tools]:*.go", "main.go"));
        assert!(matches("file[example.com/*]:*.go", "main.go"));
        assert!(matches("!file:a.go&&file:*.go", "b.go"), "! binds tighter than &&");
        assert!(!matches("!file:a.go&&file:*.go", "a.go"));
        assert!(!matches("!(file:a.go||file:b.go)&&file:*.go", "b.go"));
        assert!(matches("((file:a.go)||(file:b.go&&(!file:c.go)))", "b.go"), "nested parentheses");
        assert!(matches("!!file:a.go", "a.go"));
        assert!(!matches("(file:a.go||file:b.go)&&file:c.go", "a.go"), "parentheses override precedence");
        assert!(matches("file:*.go || file:*.md && file:x", "a.go"));
    }

    #[test]
    fn a_scope_added_again_replaces_the_one_of_the_same_name() {
        let mut set = set();
        set.add("Tests", "$Sources");
        assert!(matches!(set.check(&Pattern::parse("$Tests").unwrap()), Err(ScopeError::Cycle(_))));
        set.add("Tests", "file:*.md");
        assert!(set.matches(&Pattern::parse("$Tests").unwrap(), "README.md"));
        assert!(!set.matches(&Pattern::parse("$Tests").unwrap(), "a_test.go"));
    }

    #[test]
    fn errors() {
        assert_eq!(Pattern::parse("  "), Err(ScopeError::Empty));
        assert_eq!(Pattern::parse("src:*..*"), Err(ScopeError::Unsupported("src".to_owned())));
        assert!(matches!(Pattern::parse("file:*.go&&"), Err(ScopeError::Unexpected(_))));
        assert!(matches!(Pattern::parse("(file:*.go"), Err(ScopeError::Unexpected(_))));
        assert!(matches!(Pattern::parse("file:*.go & file:a"), Err(ScopeError::Unexpected(_))));
        assert!(matches!(Pattern::parse("file:"), Err(ScopeError::Unexpected(_))));
        assert!(matches!(Pattern::parse("(file:a.go))"), Err(ScopeError::Unexpected(_))));
        assert!(matches!(Pattern::parse("()"), Err(ScopeError::Unexpected(_))));
        assert!(matches!(Pattern::parse("$"), Err(ScopeError::Unexpected(_))));
        assert!(matches!(Pattern::parse("file:a.go file:b.go"), Err(ScopeError::Unexpected(_))));
        let set = set();
        assert_eq!(set.check(&Pattern::parse("$Nope").unwrap()), Err(ScopeError::UnknownScope("Nope".to_owned())));
        assert!(matches!(set.check(&Pattern::parse("$Loop").unwrap()), Err(ScopeError::Cycle(_))));
        assert_eq!(set.check(&Pattern::parse("$Sources||file:*.md").unwrap()), Ok(()));
    }

    #[test]
    fn filters() {
        let go = FileFilter::new(|relative| relative.ends_with(".go"));
        let tests = FileFilter::new(|relative| relative.contains("_test"));
        let both = go.clone().and(tests);
        assert!(both.accepts("a_test.go") && !both.accepts("a.go") && !both.accepts("a_test.md"));
        assert_eq!(go, go.clone());
        assert_ne!(go, FileFilter::all());
    }
}

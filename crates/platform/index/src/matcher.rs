use std::cmp::Ordering;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
    Middle,
    Humps,
    Prefix,
    Exact,
}

impl MatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Prefix => "prefix",
            Self::Humps => "humps",
            Self::Middle => "middle",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameMatch {
    pub kind: MatchKind,
    pub degree: i64,
    pub ranges: Vec<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub line: u32,
    pub column: Option<u32>,
}

pub fn split_location(query: &str) -> (&str, Option<Location>) {
    let number = |text: &str| {
        (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit())).then(|| text.parse().ok()).flatten()
    };
    let query = match query.strip_suffix(':') {
        Some(rest) if !rest.is_empty() && !rest.ends_with(':') => rest,
        _ => query,
    };
    let Some((rest, last)) = query.rsplit_once(':') else { return (query, None) };
    let Some(last) = number(last.trim_end()) else { return (query, None) };
    if let Some((name, line)) = rest.rsplit_once(':')
        && let Some(line) = number(line)
        && !name.is_empty()
    {
        return (name, Some(Location { line, column: Some(last) }));
    }
    if rest.is_empty() || rest.ends_with(':') {
        return (query, None);
    }
    (rest, Some(Location { line: last, column: None }))
}

#[derive(Clone, Copy, Debug)]
struct Letter {
    ch: char,
    lower: char,
    upper: bool,
    free: bool,
    word: bool,
}

fn fold(c: char) -> char {
    if c.is_ascii() { c.to_ascii_lowercase() } else { c.to_lowercase().next().unwrap_or(c) }
}

#[derive(Default)]
struct Scratch {
    text: Vec<char>,
    lower: Vec<char>,
    starts: Vec<bool>,
    cells: Vec<(i64, usize)>,
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::default();
}

#[derive(Clone, Debug)]
pub struct NamePattern {
    letters: Vec<Letter>,
    literal: bool,
    end: bool,
}

impl NamePattern {
    pub fn new(text: &str) -> Self {
        let mut letters = Vec::new();
        let (mut free, mut word, mut literal) = (false, false, true);
        for ch in text.trim_start().chars() {
            match ch {
                '*' => {
                    free = true;
                    literal = false;
                }
                ' ' => word = !letters.is_empty(),
                _ => {
                    letters.push(Letter { ch, lower: fold(ch), upper: ch.is_uppercase(), free, word });
                    (free, word) = (false, false);
                }
            }
        }
        Self { letters, literal, end: word }
    }

    pub fn is_empty(&self) -> bool {
        self.letters.is_empty()
    }

    pub fn may_match(&self, name: &str) -> bool {
        let mut letters = name.chars().map(fold);
        self.letters.iter().all(|wanted| letters.any(|letter| letter == wanted.lower))
    }

    pub fn matches(&self, name: &str) -> Option<NameMatch> {
        self.matches_ending(name, false, true)
    }

    pub fn matches_file(&self, name: &str) -> Option<NameMatch> {
        self.matches_ending(name, true, true)
    }

    fn matches_ending(&self, name: &str, file: bool, ranged: bool) -> Option<NameMatch> {
        if self.letters.is_empty() {
            return Some(NameMatch { kind: MatchKind::Middle, degree: 0, ranges: Vec::new() });
        }
        if !self.may_match(name) {
            return None;
        }
        SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let Scratch { text, lower, starts, cells } = &mut *scratch;
            text.clear();
            text.extend(name.chars());
            lower.clear();
            lower.extend(text.iter().map(|&c| fold(c)));
            let stem = file.then(|| text.iter().rposition(|&c| c == '.').filter(|&at| at > 0)).flatten();
            let ends = [Some(text.len()), stem];
            starts.clear();
            starts.extend((0..text.len()).map(|index| word_start(text, index)));
            self.literal_match(text, lower, starts, &ends)
                .or_else(|| self.humps(text, lower, starts, cells, &ends, ranged))
        })
    }

    fn literal_match(
        &self,
        text: &[char],
        lower: &[char],
        starts: &[bool],
        ends: &[Option<usize>; 2],
    ) -> Option<NameMatch> {
        if !self.literal {
            return None;
        }
        let (mut at, mut exact_case, mut capitals_fit) = (0, 0, true);
        for letter in &self.letters {
            if letter.word {
                if text.get(at) != Some(&' ') {
                    return None;
                }
                at += 1;
            }
            if lower.get(at) != Some(&letter.lower) {
                return None;
            }
            if letter.upper {
                if letter.ch == text[at] {
                    exact_case += 1;
                } else if !starts[at] {
                    capitals_fit = false;
                }
            }
            at += 1;
        }
        if ends.contains(&Some(at)) {
            return Some(NameMatch {
                kind: MatchKind::Exact,
                degree: exact_case,
                ranges: std::iter::once(0..at).collect(),
            });
        }
        if self.end || !capitals_fit {
            return None;
        }
        let boundary = !text[at].is_alphanumeric() || starts[at];
        Some(NameMatch {
            kind: MatchKind::Prefix,
            degree: i64::from(boundary) * 2 + exact_case,
            ranges: std::iter::once(0..at).collect(),
        })
    }

    fn humps(
        &self,
        text: &[char],
        lower: &[char],
        starts: &[bool],
        cells: &mut Vec<(i64, usize)>,
        ends: &[Option<usize>; 2],
        ranged: bool,
    ) -> Option<NameMatch> {
        const NONE: i64 = i64::MIN;
        const START: i64 = 1 << 40;
        let (count, width) = (self.letters.len(), text.len());
        if count > width {
            return None;
        }
        cells.clear();
        cells.resize(count * width, (NONE, 0));
        let capital = |letter: &Letter, j: usize| i64::from(letter.upper && letter.ch == text[j]) * 2;
        let first = self.letters[0];
        for j in 0..width {
            if first.lower != lower[j] || (first.upper && !starts[j] && first.ch != text[j]) {
                continue;
            }
            let at_start = if starts[j] { START + 5 } else { 0 };
            cells[j].0 = 10 + at_start + if j == 0 { 15 } else { 0 } + capital(&first, j);
        }
        for (i, letter) in self.letters.iter().enumerate().skip(1) {
            let (row, previous) = (i * width, (i - 1) * width);
            let mut before: (i64, usize) = (NONE, 0);
            for j in i..width {
                if j >= 2 && cells[previous + j - 2].0 > before.0 {
                    before = (cells[previous + j - 2].0, j - 2);
                }
                if letter.lower != lower[j] {
                    continue;
                }
                let exact = capital(letter, j);
                let adjacent = cells[previous + j - 1].0;
                let joins = !letter.word || starts[j];
                let capital_fits = !letter.upper || letter.ch == text[j] || starts[j];
                if adjacent != NONE && joins && capital_fits {
                    cells[row + j].0 = adjacent + 18 + exact;
                    cells[row + j].1 = j - 1;
                }
                let jumps = starts[j] || letter.free;
                let capital_jumps = !letter.upper || starts[j] || letter.ch == text[j];
                if before.0 != NONE && jumps && capital_jumps {
                    let jumped = before.0 + 10 + if starts[j] { 5 } else { 0 } + exact;
                    if jumped > cells[row + j].0 {
                        cells[row + j].0 = jumped;
                        cells[row + j].1 = before.1;
                    }
                }
            }
        }
        let last = (count - 1) * width;
        let end = (0..width)
            .filter(|&j| cells[last + j].0 != NONE)
            .filter(|&j| !self.end || ends.contains(&Some(j + 1)))
            .max_by_key(|&j| (cells[last + j].0, std::cmp::Reverse(j)))?;
        let best = cells[last + end].0;
        let (kind, degree) = if best >= START { (MatchKind::Humps, best - START) } else { (MatchKind::Middle, best) };
        if !ranged {
            return Some(NameMatch { kind, degree, ranges: Vec::new() });
        }
        let mut positions = vec![end; count];
        for i in (1..count).rev() {
            positions[i - 1] = cells[i * width + positions[i]].1;
        }
        Some(NameMatch { kind, degree, ranges: ranges(&positions) })
    }
}

fn ranges(positions: &[usize]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for &at in positions {
        match ranges.last_mut() {
            Some(range) if range.end == at => range.end = at + 1,
            _ => ranges.push(at..at + 1),
        }
    }
    ranges
}

fn word_start(text: &[char], index: usize) -> bool {
    let Some(&current) = text.get(index) else { return false };
    if index == 0 {
        return true;
    }
    let previous = text[index - 1];
    if previous.is_ascii_lowercase() && current.is_ascii_lowercase() {
        return false;
    }
    let next = text.get(index + 1).copied();
    !previous.is_alphanumeric()
        || (current.is_uppercase() && previous.is_lowercase())
        || (current.is_uppercase() && previous.is_uppercase() && next.is_some_and(char::is_lowercase))
        || (current.is_ascii_digit() && !previous.is_ascii_digit())
        || (!current.is_ascii_digit() && previous.is_ascii_digit() && current.is_alphabetic())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedMatch {
    pub name: NameMatch,
    pub owners: Vec<(usize, Vec<Range<usize>>)>,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    owners: Vec<NamePattern>,
    name: NamePattern,
    location: Option<Location>,
}

impl Pattern {
    pub fn path(query: &str) -> Self {
        Self::split(query, &['/', '\\'])
    }

    pub fn qualified(query: &str) -> Self {
        Self::split(&query.replace("::", "."), &['.', '/'])
    }

    fn split(query: &str, separators: &[char]) -> Self {
        let (query, location) = split_location(query.trim_start());
        let mut parts: Vec<&str> = query.split(separators).collect();
        let name = parts.pop().map(NamePattern::new).unwrap_or_else(|| NamePattern::new(""));
        let owners = parts
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .map(|part| NamePattern::new(part.trim()))
            .collect();
        Self { owners, name, location }
    }

    pub fn location(&self) -> Option<Location> {
        self.location
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_empty() && self.owners.is_empty()
    }

    pub fn name(&self) -> &NamePattern {
        &self.name
    }

    pub fn matches(&self, name: &str, owners: &[&str]) -> Option<OwnedMatch> {
        self.matches_with(self.name.matches(name)?, owners, true)
    }

    pub fn matches_path(&self, relative: &str) -> Option<NameMatch> {
        self.path_match(relative, true)
    }

    pub(crate) fn path_match(&self, relative: &str, ranged: bool) -> Option<NameMatch> {
        let (folder, name) = relative.rsplit_once('/').map_or(("", relative), |(folder, name)| (folder, name));
        let mut found = self.name.matches_ending(name, true, ranged)?;
        let at = if folder.is_empty() { 0 } else { folder.chars().count() + 1 };
        if self.owners.is_empty() {
            found.ranges.iter_mut().for_each(|range| *range = range.start + at..range.end + at);
            return Some(found);
        }
        let folders: Vec<&str> = if folder.is_empty() { Vec::new() } else { folder.split('/').collect() };
        let owned = self.matches_with(found, &folders, ranged)?;
        let mut starts = Vec::with_capacity(folders.len());
        let mut at = 0;
        for part in &folders {
            starts.push(at);
            at += part.chars().count() + 1;
        }
        let mut ranges: Vec<Range<usize>> = owned
            .owners
            .iter()
            .flat_map(|(index, ranges)| {
                ranges.iter().map(|range| range.start + starts[*index]..range.end + starts[*index])
            })
            .collect();
        ranges.extend(owned.name.ranges.iter().map(|range| range.start + at..range.end + at));
        Some(NameMatch { ranges, ..owned.name })
    }

    pub fn matches_qualified(&self, qualified: &str, outer: &[&str]) -> Option<NameMatch> {
        let mut parts: Vec<(usize, &str)> = Vec::new();
        let mut start = 0;
        for part in qualified.split(['.', '/']) {
            if !part.is_empty() {
                parts.push((start, part));
            }
            start += part.chars().count() + 1;
        }
        let (at, name) = parts.pop()?;
        let mut owners: Vec<&str> = outer.to_vec();
        owners.extend(parts.iter().map(|(_, part)| *part));
        let owned = self.matches_with(self.name.matches(name)?, &owners, true)?;
        let mut ranges: Vec<Range<usize>> = owned
            .owners
            .iter()
            .filter(|(index, _)| *index >= outer.len())
            .flat_map(|(index, ranges)| {
                let offset = parts[index - outer.len()].0;
                ranges.iter().map(move |range| range.start + offset..range.end + offset)
            })
            .collect();
        ranges.extend(owned.name.ranges.iter().map(|range| range.start + at..range.end + at));
        Some(NameMatch { ranges, ..owned.name })
    }

    fn matches_with(&self, mut name: NameMatch, owners: &[&str], ranged: bool) -> Option<OwnedMatch> {
        let mut matched = Vec::new();
        let mut at = owners.len();
        for part in self.owners.iter().rev() {
            let (index, found) = (0..at)
                .rev()
                .find_map(|index| part.matches_ending(owners[index], false, ranged).map(|found| (index, found)))?;
            name.degree += found.degree / 4;
            matched.push((index, found.ranges));
            at = index;
        }
        matched.reverse();
        Some(OwnedMatch { name, owners: matched })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rank {
    pub kind: MatchKind,
    pub degree: i64,
    pub length: usize,
    pub in_project: bool,
    pub recent: Option<usize>,
}

impl Rank {
    pub fn new(found: &NameMatch, length: usize) -> Self {
        Self { kind: found.kind, degree: found.degree, length, in_project: true, recent: None }
    }

    pub fn in_project(self, in_project: bool) -> Self {
        Self { in_project, ..self }
    }
}

impl Ord for Rank {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .kind
            .cmp(&self.kind)
            .then(other.in_project.cmp(&self.in_project))
            .then(other.degree.cmp(&self.degree))
            .then(self.recent.unwrap_or(usize::MAX).cmp(&other.recent.unwrap_or(usize::MAX)))
            .then(self.length.cmp(&other.length))
    }
}

impl PartialOrd for Rank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(pattern: &str, name: &str) -> Option<MatchKind> {
        NamePattern::new(pattern).matches(name).map(|found| found.kind)
    }

    fn pairs(ranges: &[Range<usize>]) -> Vec<(usize, usize)> {
        ranges.iter().map(|range| (range.start, range.end)).collect()
    }

    fn ranges_of(pattern: &str, name: &str) -> Vec<Range<usize>> {
        NamePattern::new(pattern).matches(name).map(|found| found.ranges).unwrap_or_default()
    }

    #[test]
    fn a_capital_starts_a_hump() {
        for pattern in ["NPE", "nPE", "nullpoex", "NuPoEx", "npe"] {
            assert_eq!(kind(pattern, "NullPointerException"), Some(MatchKind::Humps), "{pattern}");
        }
        assert_eq!(NamePattern::new("mgo").matches_file("main.go").map(|found| found.kind), Some(MatchKind::Humps));
        assert_eq!(kind("IS", "Items"), None, "a capital only at a word start");
        assert_eq!(kind("iS", "ItemStore"), Some(MatchKind::Humps));
        assert_eq!(kind("httpS", "HTTPServer"), Some(MatchKind::Prefix));
        assert_eq!(kind("hs", "HTTPServer"), Some(MatchKind::Humps));
        assert_eq!(kind("eiew", "EditorView"), None, "letters only go on at a hump");
        assert_eq!(ranges_of("NPE", "NullPointerException"), [0..1, 4..5, 11..12]);
        assert_eq!(ranges_of("nullpoex", "NullPointerException"), [0..6, 11..13]);
    }

    #[test]
    fn exact_then_prefix_then_humps_then_middle() {
        assert_eq!(kind("item", "Item"), Some(MatchKind::Exact));
        assert_eq!(NamePattern::new("main").matches_file("main.go").map(|found| found.kind), Some(MatchKind::Exact));
        assert_eq!(kind("item", "ItemStore"), Some(MatchKind::Prefix));
        assert_eq!(kind("store", "ItemStore"), Some(MatchKind::Humps));
        assert_eq!(kind("tem", "ItemStore"), Some(MatchKind::Middle));
        assert_eq!(kind("Tem", "ItemStore"), None, "a capital does not match inside a word");
        assert_eq!(kind("xyz", "ItemStore"), None);
        let whole = NamePattern::new("cart").matches_file("cart_test.go").unwrap();
        let longer = NamePattern::new("cart").matches_file("carting.md").unwrap();
        assert!(whole.degree > longer.degree, "a whole word first");
        assert_eq!(pairs(&ranges_of("tem", "ItemStore")), [(1, 4)]);
    }

    #[test]
    fn stars_spaces_and_a_trailing_space() {
        assert_eq!(kind("*tore", "ItemStore"), Some(MatchKind::Middle));
        assert_eq!(kind("i*re", "ItemStore"), Some(MatchKind::Humps));
        assert_eq!(kind("ire", "ItemStore"), None, "without a star the letters go on at a hump");
        assert_eq!(ranges_of("i*re", "ItemStore"), [0..1, 7..9]);
        assert_eq!(kind("dup line", "Duplicate Line"), Some(MatchKind::Humps));
        assert_eq!(kind("dup ine", "Duplicate Line"), None, "a space is a word boundary");
        assert_eq!(kind("item ", "Item"), Some(MatchKind::Exact));
        assert_eq!(kind("item ", "ItemStore"), None, "a trailing space ends the name");
        assert_eq!(kind("IStore ", "ItemStore"), Some(MatchKind::Humps));
        assert_eq!(kind("IS ", "ItemStore"), None);
        assert_eq!(kind("IStore ", "ItemStoreLong"), None);
        assert_eq!(NamePattern::new("main ").matches_file("main.go").map(|found| found.kind), Some(MatchKind::Exact));
        assert_eq!(NamePattern::new("ma ").matches_file("main.go"), None);
        assert!(NamePattern::new("  ").is_empty());
        assert_eq!(kind("duplicate line", "Duplicate Line"), Some(MatchKind::Exact));
        assert_eq!(kind("duplicate li", "Duplicate Line or Selection"), Some(MatchKind::Prefix));
        assert_eq!(
            ranges_of("duplicate li", "Duplicate Line or Selection"),
            std::iter::once(0..12).collect::<Vec<_>>()
        );
        let long = "a".repeat(80);
        assert_eq!(kind(&long, &format!("x{long}")), Some(MatchKind::Middle), "a long match inside a word");
    }

    #[test]
    fn ranges_count_characters_not_bytes() {
        assert_eq!(ranges_of("ФП", "ФайлПроекта"), [0..1, 4..5]);
        assert_eq!(kind("фп", "ФайлПроекта"), Some(MatchKind::Humps));
        assert_eq!(kind("Пр", "ФайлПроекта"), Some(MatchKind::Humps));
        assert_eq!(kind("Ай", "ФайлПроекта"), None);
        let found = Pattern::path("папка/фа").matches_path("папка/файл.go").unwrap();
        assert_eq!(found.ranges, [0..5, 6..8]);
        let found = Pattern::qualified("Склад.по").matches_qualified("Склад.Получить", &[]).unwrap();
        assert_eq!(found.ranges, [0..5, 6..8]);
    }

    #[test]
    fn a_line_and_column_are_split_off() {
        assert_eq!(split_location("main.go:42"), ("main.go", Some(Location { line: 42, column: None })));
        assert_eq!(split_location("main.go:42:7"), ("main.go", Some(Location { line: 42, column: Some(7) })));
        assert_eq!(split_location("Store::get"), ("Store::get", None));
        assert_eq!(split_location("main.go"), ("main.go", None));
        assert_eq!(split_location(":42"), (":42", None));
        assert_eq!(split_location("main.go:"), ("main.go", None), "a colon being typed");
        assert_eq!(split_location("main.go:12:"), ("main.go", Some(Location { line: 12, column: None })));
        assert_eq!(split_location("Store::"), ("Store::", None));
        assert!(Pattern::qualified("Store:").matches("Store", &[]).is_some());
        let pattern = Pattern::path("internal/ma:12:3");
        assert_eq!(pattern.location(), Some(Location { line: 12, column: Some(3) }));
        assert!(pattern.matches_path("internal/cart/main.go").is_some());
    }

    #[test]
    fn slashes_and_dots_split_owners_from_the_name() {
        let path = Pattern::path("internal/ca");
        let found = path.matches_path("internal/cart/cart.go").unwrap();
        assert_eq!(found.kind, MatchKind::Prefix);
        assert_eq!(found.ranges, [0..8, 14..16]);
        assert!(path.matches_path("cmd/cart.go").is_none());
        assert!(Pattern::path("ui/mod").matches_path("crates/platform/ui/src/ide/mod.rs").is_some());
        assert!(Pattern::path("ui/mod").matches_path("crates/platform/core/src/mod.rs").is_none());
        let found = Pattern::path("internal/").matches_path("internal/cart/cart.go").unwrap();
        assert_eq!(pairs(&found.ranges), [(0, 8)]);
        let qualified = Pattern::qualified("model.IS.count");
        let owned = qualified.matches("count", &["example.com", "shop", "model", "ItemStore"]).unwrap();
        assert_eq!(
            owned.owners.iter().map(|(index, ranges)| (*index, pairs(ranges))).collect::<Vec<_>>(),
            [(2, vec![(0, 5)]), (3, vec![(0, 1), (4, 5)])]
        );
        assert!(qualified.matches("count", &["model", "Other"]).is_none());
        assert!(Pattern::qualified("Store::get").matches("Get", &["Store"]).is_some());
        let found = Pattern::qualified("st.get").matches_qualified("Store.GetItem", &["shop"]).unwrap();
        assert_eq!(found.ranges, [0..2, 6..9]);
        assert!(Pattern::qualified("shop.st.get").matches_qualified("Store.GetItem", &["shop"]).is_some());
        assert!(Pattern::qualified("cart.get").matches_qualified("Store.GetItem", &["shop"]).is_none());
    }

    #[test]
    fn ranks_order_kind_project_degree_recent_and_length() {
        let rank = |kind, degree, length, in_project, recent| Rank { kind, degree, length, in_project, recent };
        let mut ranks = [
            rank(MatchKind::Middle, 9, 3, true, Some(0)),
            rank(MatchKind::Humps, 1, 9, true, None),
            rank(MatchKind::Exact, 0, 4, false, None),
            rank(MatchKind::Exact, 0, 4, true, Some(3)),
            rank(MatchKind::Exact, 0, 4, true, Some(1)),
            rank(MatchKind::Exact, 0, 3, false, None),
            rank(MatchKind::Exact, 5, 5, false, None),
            rank(MatchKind::Humps, 7, 8, false, None),
        ];
        ranks.sort();
        let order: Vec<(MatchKind, i64, usize, bool, Option<usize>)> =
            ranks.iter().map(|rank| (rank.kind, rank.degree, rank.length, rank.in_project, rank.recent)).collect();
        assert_eq!(
            order,
            [
                (MatchKind::Exact, 0, 4, true, Some(1)),
                (MatchKind::Exact, 0, 4, true, Some(3)),
                (MatchKind::Exact, 5, 5, false, None),
                (MatchKind::Exact, 0, 3, false, None),
                (MatchKind::Exact, 0, 4, false, None),
                (MatchKind::Humps, 1, 9, true, None),
                (MatchKind::Humps, 7, 8, false, None),
                (MatchKind::Middle, 9, 3, true, Some(0)),
            ]
        );
    }

    #[test]
    fn the_project_comes_before_a_shorter_sdk_name_but_not_before_a_better_match() {
        let rank = |pattern: &str, name: &str, in_project: bool| {
            let found = NamePattern::new(pattern).matches(name).unwrap();
            Rank::new(&found, name.chars().count()).in_project(in_project)
        };
        assert!(rank("Cart", "CartStore", true) < rank("Cart", "CartPool", false), "equal matches: the project's");
        assert!(rank("Cart", "CartPool", true) < rank("Cart", "CartStore", true), "both in the project: the shorter");
        assert!(rank("Cart", "Cart", false) < rank("Cart", "CartStore", true), "an exact SDK name: before a prefix");
        assert!(rank("Cart", "CartPool", false) < rank("Cart", "MyCart", true), "an SDK prefix: before a later word");
        let (store, sdk) = (rank("i*re", "ItemStore", true), rank("i*re", "IP_RECVIF", false));
        assert_eq!((store.kind, sdk.kind), (MatchKind::Humps, MatchKind::Humps));
        assert!(sdk.degree > store.degree, "the SDK name matches closer");
        assert!(store < sdk, "the same kind: the project's first");
    }
}

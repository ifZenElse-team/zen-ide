use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

pub const TEXT: &str = "text";

pub const DETECTED: &str = "detected";

pub const NATIVE: &str = "native";

pub const DEFAULT_IGNORED: &[&str] = &[
    "*.pyc",
    "*.pyo",
    "*.rbc",
    "*.yarb",
    "*~",
    ".DS_Store",
    ".git",
    ".hg",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".svn",
    "CVS",
    "__pycache__",
    "_svn",
    "vssver.scc",
    "vssver2.scc",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileType {
    pub id: String,
    pub name: String,
    pub localized: bool,
    pub patterns: Vec<String>,
    pub hashbangs: Vec<String>,
    pub contents: Vec<ContentMarker>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentMarker {
    pub starts: String,
    pub unless: Vec<String>,
    pub first_tag: Option<String>,
}

impl ContentMarker {
    pub fn matches(&self, head: &str) -> bool {
        if self.starts.is_empty() || head.contains('\0') {
            return false;
        }
        let text = head.strip_prefix('\u{feff}').unwrap_or(head).trim_start();
        if !text.starts_with(&self.starts) {
            return false;
        }
        let starts_with_ignoring_case =
            |prefix: &str| text.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix));
        if self.unless.iter().any(|prefix| starts_with_ignoring_case(prefix)) {
            return false;
        }
        match &self.first_tag {
            Some(word) => text.find('>').is_some_and(|end| text[..end].contains(word.as_str())),
            None => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mapping {
    pub type_id: String,
    pub pattern: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub added: Vec<Mapping>,
    pub removed: Vec<Mapping>,
    pub added_hashbangs: Vec<Mapping>,
    pub removed_hashbangs: Vec<Mapping>,
    pub ignored: Option<Vec<String>>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        *self == Changes::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effective {
    pub declared: FileType,
    pub patterns: Vec<String>,
    pub hashbangs: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    declared: Vec<FileType>,
    changes: Changes,
    overrides: BTreeMap<PathBuf, String>,
    effective: Vec<Effective>,
    effective_matcher: Matcher,
    declared_matcher: Matcher,
}

#[derive(Clone, Debug, Default)]
struct Matcher {
    names: HashMap<String, usize>,
    extensions: HashMap<String, usize>,
    wildcards: Vec<Wildcard>,
}

#[derive(Clone, Debug)]
struct Wildcard {
    pattern: String,
    starts: String,
    ends: String,
    rank: (u8, usize),
    index: usize,
}

impl Matcher {
    fn new<'a>(types: impl Iterator<Item = &'a [String]>) -> Self {
        let mut matcher = Matcher::default();
        for (index, patterns) in types.enumerate() {
            for pattern in patterns {
                let rank = specificity(pattern);
                match rank.0 {
                    3 => {
                        matcher.names.entry(pattern.to_ascii_lowercase()).or_insert(index);
                    }
                    1 => {
                        matcher.extensions.entry(pattern[2..].to_ascii_lowercase()).or_insert(index);
                    }
                    _ => {
                        let lower = pattern.to_ascii_lowercase();
                        let starts = lower.split(['*', '?']).next().unwrap_or_default().to_owned();
                        let ends = lower.rsplit(['*', '?']).next().unwrap_or_default().to_owned();
                        matcher.wildcards.push(Wildcard { pattern: pattern.clone(), starts, ends, rank, index });
                    }
                }
            }
        }
        matcher
    }

    fn best(&self, name: &str) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        if let Some(index) = self.names.get(&lower) {
            return Some(*index);
        }
        let mut found: Option<(usize, (u8, usize))> = None;
        for wildcard in &self.wildcards {
            let better = found.is_none_or(|(_, best)| wildcard.rank > best);
            if better
                && lower.starts_with(&wildcard.starts)
                && lower.ends_with(&wildcard.ends)
                && glob_matches(&wildcard.pattern, name, true)
            {
                found = Some((wildcard.index, wildcard.rank));
            }
        }
        if let Some((index, _)) = found {
            return Some(index);
        }
        let (_, extension) = lower.rsplit_once('.')?;
        self.extensions.get(extension).copied()
    }
}

pub fn glob_matches(pattern: &str, name: &str, fold_case: bool) -> bool {
    let same = |a: char, b: char| a == b || (fold_case && a.eq_ignore_ascii_case(&b));
    let next = |text: &str, at: usize| text[at..].chars().next();
    let (mut p, mut t, mut star, mut mark) = (0, 0, None, 0);
    while let Some(c) = next(name, t) {
        match next(pattern, p) {
            Some(wanted) if wanted == '?' || same(wanted, c) => {
                p += wanted.len_utf8();
                t += c.len_utf8();
            }
            Some('*') => {
                star = Some(p);
                mark = t;
                p += 1;
            }
            _ => {
                let Some(at) = star else { return false };
                p = at + 1;
                mark += next(name, mark).map_or(1, char::len_utf8);
                t = mark;
            }
        }
    }
    pattern[p..].chars().all(|c| c == '*')
}

fn specificity(pattern: &str) -> (u8, usize) {
    let fixed = pattern.chars().filter(|c| *c != '*' && *c != '?').count();
    let wild = pattern.contains(['*', '?']);
    let extension = pattern.strip_prefix("*.").is_some_and(|rest| !rest.is_empty() && !rest.contains(['*', '?', '.']));
    let kind = if !wild {
        3
    } else if extension {
        1
    } else {
        2
    };
    (kind, fixed)
}

pub fn interpreter(first_line: &str) -> Option<String> {
    let line = first_line.strip_prefix("#!")?;
    let mut words = line.split_whitespace();
    let program = words.next()?;
    let base = |word: &str| word.rsplit('/').next().unwrap_or(word).to_owned();
    if base(program) == "env" {
        return words.find(|word| !word.starts_with('-') && !word.contains('=')).map(base);
    }
    Some(base(program))
}

fn hashbang_matches(pattern: &str, interpreter: &str) -> bool {
    if glob_matches(pattern, interpreter, true) {
        return true;
    }
    interpreter
        .strip_prefix(pattern)
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

impl Registry {
    pub fn new(declared: Vec<FileType>, changes: Changes) -> Self {
        let mut registry = Self { declared, changes, ..Self::default() };
        registry.refresh();
        registry
    }

    fn refresh(&mut self) {
        self.effective = self.declared.iter().map(|declared| self.effective(declared)).collect();
        self.effective_matcher = Matcher::new(self.effective.iter().map(|ty| ty.patterns.as_slice()));
        self.declared_matcher = Matcher::new(self.declared.iter().map(|ty| ty.patterns.as_slice()));
    }

    pub fn declared(&self) -> &[FileType] {
        &self.declared
    }

    fn is_declared(&self, id: &str) -> bool {
        self.declared.iter().any(|ty| ty.id == id)
    }

    pub fn types_changed(&self) -> bool {
        let changes = &self.changes;
        !(changes.added.is_empty()
            && changes.removed.is_empty()
            && changes.added_hashbangs.is_empty()
            && changes.removed_hashbangs.is_empty())
    }

    pub fn changes(&self) -> &Changes {
        &self.changes
    }

    pub fn types(&self) -> Vec<Effective> {
        self.effective.clone()
    }

    pub fn get(&self, id: &str) -> Option<Effective> {
        self.effective.iter().find(|ty| ty.declared.id == id).cloned()
    }

    fn effective(&self, declared: &FileType) -> Effective {
        let collect = |own: &[String], added: &[Mapping], removed: &[Mapping]| {
            let taken = |pattern: &str| {
                removed.iter().any(|mapping| mapping.type_id == declared.id && mapping.pattern == pattern)
                    || added.iter().any(|mapping| {
                        mapping.type_id != declared.id
                            && mapping.pattern == pattern
                            && self.is_declared(&mapping.type_id)
                    })
            };
            let mut patterns: Vec<String> = own.iter().filter(|pattern| !taken(pattern)).cloned().collect();
            for mapping in added.iter().filter(|mapping| mapping.type_id == declared.id) {
                if !patterns.contains(&mapping.pattern) {
                    patterns.push(mapping.pattern.clone());
                }
            }
            patterns
        };
        Effective {
            declared: declared.clone(),
            patterns: collect(&declared.patterns, &self.changes.added, &self.changes.removed),
            hashbangs: collect(&declared.hashbangs, &self.changes.added_hashbangs, &self.changes.removed_hashbangs),
        }
    }

    pub fn type_of(&self, name: &str) -> Option<FileType> {
        self.effective_matcher.best(name).map(|index| self.effective[index].declared.clone())
    }

    fn type_id_of(&self, name: &str) -> Option<&str> {
        self.effective_matcher.best(name).map(|index| self.effective[index].declared.id.as_str())
    }

    fn declared_type_id_of(&self, name: &str) -> Option<&str> {
        self.declared_matcher.best(name).map(|index| self.declared[index].id.as_str())
    }

    pub fn type_by_hashbang(&self, first_line: &str) -> Option<FileType> {
        let interpreter = interpreter(first_line)?;
        self.effective
            .iter()
            .find(|ty| ty.hashbangs.iter().any(|pattern| hashbang_matches(pattern, &interpreter)))
            .map(|ty| ty.declared.clone())
    }

    pub fn type_by_content(&self, head: &str) -> Option<FileType> {
        self.effective
            .iter()
            .find(|ty| ty.declared.contents.iter().any(|marker| marker.matches(head)))
            .map(|ty| ty.declared.clone())
    }

    pub fn has_content_markers(&self) -> bool {
        self.declared.iter().any(|ty| !ty.contents.is_empty())
    }

    pub fn type_by_head(&self, head: &str) -> Option<FileType> {
        self.type_by_hashbang(first_line(head)).or_else(|| self.type_by_content(head))
    }

    pub fn type_of_file(&self, name: &str, head: Option<&str>) -> Option<FileType> {
        self.type_of(name).or_else(|| head.and_then(|head| self.type_by_head(head)))
    }

    pub fn known_name(&self, name: &str, first_line: Option<&str>) -> Option<String> {
        let plain = |head: &str| !head.starts_with("#!") && !self.has_content_markers();
        if !self.types_changed() && (first_line.is_none_or(plain) || self.declared_type_id_of(name).is_some()) {
            return Some(name.to_owned());
        }
        let now = self.type_id_of(name);
        let before = self.declared_type_id_of(name);
        match (now, before) {
            (Some(now), Some(before)) if now == before => Some(name.to_owned()),
            (Some(now), _) => {
                let now = self.declared.iter().find(|ty| ty.id == now)?;
                self.known_as(now, name)
            }
            (None, Some(_)) => None,
            (None, None) => match first_line.and_then(|head| self.type_by_head(head)) {
                Some(by_head) => Some(self.sample(&by_head).unwrap_or_else(|| name.to_owned())),
                None => Some(name.to_owned()),
            },
        }
    }

    fn known_as(&self, file_type: &FileType, name: &str) -> Option<String> {
        if file_type.id == DETECTED || file_type.id == NATIVE {
            return self.plain_name();
        }
        Some(self.sample(file_type).unwrap_or_else(|| name.to_owned()))
    }

    fn plain_name(&self) -> Option<String> {
        self.declared.iter().find(|ty| ty.id == TEXT).and_then(|text| self.sample(text))
    }

    pub fn overridden(&self, path: &Path) -> Option<FileType> {
        let id = self.overrides.get(path)?;
        self.declared.iter().find(|ty| ty.id == *id).cloned()
    }

    pub fn overrides(&self) -> &BTreeMap<PathBuf, String> {
        &self.overrides
    }

    pub fn set_override(&mut self, path: &Path, type_id: Option<&str>) {
        match type_id {
            Some(id) => self.overrides.insert(path.to_owned(), id.to_owned()),
            None => self.overrides.remove(path),
        };
    }

    pub fn type_at(&self, path: &Path, first_line: Option<&str>) -> Option<FileType> {
        if let Some(own) = self.overridden(path) {
            return Some(own);
        }
        let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
        let read = self.head_on_disk(path, &name, first_line);
        self.type_of_file(&name, first_line.or(read.as_deref()))
    }

    pub fn known_name_at(&self, path: &Path, first_line: Option<&str>) -> Option<String> {
        let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
        match self.overridden(path) {
            Some(own) if self.type_id_of(&name) != Some(own.id.as_str()) => self.known_as(&own, &name),
            _ => {
                let read = self.head_on_disk(path, &name, first_line);
                self.known_name(&name, first_line.or(read.as_deref()))
            }
        }
    }

    fn head_on_disk(&self, path: &Path, name: &str, given: Option<&str>) -> Option<Arc<str>> {
        let looked_for = self.has_content_markers() || self.effective.iter().any(|ty| !ty.hashbangs.is_empty());
        if given.is_some() || !looked_for || self.type_id_of(name).is_some() {
            return None;
        }
        disk_head(path)
    }

    pub fn by_content(&self, path: &Path, head: Option<&str>) -> bool {
        let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
        if self.overridden(path).is_some() || self.type_id_of(&name).is_some() {
            return false;
        }
        let read = if head.is_none() { disk_head(path) } else { None };
        let Some(head) = head.or(read.as_deref()) else { return false };
        self.type_by_hashbang(first_line(head)).is_none() && self.type_by_content(head).is_some()
    }

    fn sample(&self, file_type: &FileType) -> Option<String> {
        let effective = self.effective.iter().find(|ty| ty.declared.id == file_type.id);
        let pattern = file_type
            .patterns
            .iter()
            .find(|pattern| effective.is_some_and(|ty| ty.patterns.contains(pattern)))
            .or_else(|| file_type.patterns.first())?;
        Some(pattern.replace('*', "file").replace('?', "x"))
    }

    pub fn owner_of(&self, pattern: &str, type_id: &str) -> Option<FileType> {
        self.effective
            .iter()
            .find(|ty| ty.declared.id != type_id && ty.patterns.iter().any(|own| own == pattern))
            .map(|ty| ty.declared.clone())
    }

    pub fn ignored(&self) -> Vec<String> {
        self.changes.ignored.clone().unwrap_or_else(|| DEFAULT_IGNORED.iter().map(|name| (*name).to_owned()).collect())
    }

    pub fn is_ignored(&self, name: &str) -> bool {
        match &self.changes.ignored {
            Some(ignored) => ignored.iter().any(|pattern| glob_matches(pattern, name, false)),
            None => DEFAULT_IGNORED.iter().any(|pattern| glob_matches(pattern, name, false)),
        }
    }

    pub fn add_pattern(&mut self, type_id: &str, pattern: &str) -> Option<FileType> {
        let moved = self.owner_of(pattern, type_id);
        give(&self.declared, |ty| &ty.patterns, &mut self.changes.added, &mut self.changes.removed, type_id, pattern);
        self.refresh();
        moved
    }

    pub fn remove_pattern(&mut self, type_id: &str, pattern: &str) {
        take(&self.declared, |ty| &ty.patterns, &mut self.changes.added, &mut self.changes.removed, type_id, pattern);
        self.refresh();
    }

    pub fn add_hashbang(&mut self, type_id: &str, hashbang: &str) -> Option<FileType> {
        let moved = self
            .effective
            .iter()
            .find(|ty| ty.declared.id != type_id && ty.hashbangs.iter().any(|own| own == hashbang))
            .map(|ty| ty.declared.clone());
        let changes = &mut self.changes;
        give(
            &self.declared,
            |ty| &ty.hashbangs,
            &mut changes.added_hashbangs,
            &mut changes.removed_hashbangs,
            type_id,
            hashbang,
        );
        self.refresh();
        moved
    }

    pub fn remove_hashbang(&mut self, type_id: &str, hashbang: &str) {
        let changes = &mut self.changes;
        take(
            &self.declared,
            |ty| &ty.hashbangs,
            &mut changes.added_hashbangs,
            &mut changes.removed_hashbangs,
            type_id,
            hashbang,
        );
        self.refresh();
    }

    pub fn set_ignored(&mut self, ignored: Vec<String>) {
        let mut cleaned: Vec<String> = Vec::new();
        for name in ignored.into_iter().map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()) {
            if !cleaned.contains(&name) {
                cleaned.push(name);
            }
        }
        let defaults = DEFAULT_IGNORED.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        self.changes.ignored = (cleaned != defaults).then_some(cleaned);
    }
}

fn give(
    declared: &[FileType],
    own: impl Fn(&FileType) -> &Vec<String>,
    added: &mut Vec<Mapping>,
    removed: &mut Vec<Mapping>,
    type_id: &str,
    pattern: &str,
) {
    added.retain(|mapping| mapping.pattern != pattern);
    removed.retain(|mapping| !(mapping.type_id == type_id && mapping.pattern == pattern));
    let here = declared.iter().any(|ty| ty.id == type_id && own(ty).iter().any(|item| item == pattern));
    let elsewhere = declared.iter().any(|ty| ty.id != type_id && own(ty).iter().any(|item| item == pattern));
    if !here || elsewhere {
        added.push(Mapping { type_id: type_id.to_owned(), pattern: pattern.to_owned() });
    }
}

fn take(
    declared: &[FileType],
    own: impl Fn(&FileType) -> &Vec<String>,
    added: &mut Vec<Mapping>,
    removed: &mut Vec<Mapping>,
    type_id: &str,
    pattern: &str,
) {
    let was_added = added.iter().any(|mapping| mapping.type_id == type_id && mapping.pattern == pattern);
    added.retain(|mapping| !(mapping.type_id == type_id && mapping.pattern == pattern));
    for ty in declared.iter().filter(|ty| (ty.id == type_id || was_added) && own(ty).iter().any(|item| item == pattern))
    {
        let mapping = Mapping { type_id: ty.id.clone(), pattern: pattern.to_owned() };
        if !removed.contains(&mapping) {
            removed.push(mapping);
        }
    }
}

#[cfg(test)]
fn best<'a>(types: impl Iterator<Item = (&'a FileType, &'a [String])>, name: &str) -> Option<&'a FileType> {
    let mut found: Option<(&FileType, (u8, usize))> = None;
    for (ty, patterns) in types {
        for pattern in patterns.iter().filter(|pattern| glob_matches(pattern, name, true)) {
            let rank = specificity(pattern);
            if found.is_none_or(|(_, best)| rank > best) {
                found = Some((ty, rank));
            }
        }
    }
    found.map(|(ty, _)| ty)
}

struct Global {
    registry: Arc<Registry>,
    generation: u64,
}

static GLOBAL: RwLock<Option<Global>> = RwLock::new(None);

fn update(change: impl FnOnce(&mut Registry)) {
    let mut global = GLOBAL.write().unwrap_or_else(PoisonError::into_inner);
    let slot = global.get_or_insert_with(|| Global { registry: Arc::new(Registry::default()), generation: 0 });
    let mut registry = (*slot.registry).clone();
    change(&mut registry);
    registry.refresh();
    slot.registry = Arc::new(registry);
    slot.generation += 1;
}

pub fn current() -> Arc<Registry> {
    let global = GLOBAL.read().unwrap_or_else(PoisonError::into_inner);
    global.as_ref().map_or_else(|| Arc::new(Registry::default()), |global| Arc::clone(&global.registry))
}

pub fn generation() -> u64 {
    GLOBAL.read().unwrap_or_else(PoisonError::into_inner).as_ref().map_or(0, |global| global.generation)
}

pub fn declare(types: impl IntoIterator<Item = FileType>) {
    let types: Vec<FileType> = types.into_iter().collect();
    let same = {
        let current = current();
        types.iter().all(|ty| current.declared.contains(ty))
    };
    if same {
        return;
    }
    update(|registry| {
        for ty in types {
            match registry.declared.iter_mut().find(|known| known.id == ty.id) {
                Some(known) => *known = ty,
                None => registry.declared.push(ty),
            }
        }
    });
}

pub fn undeclare(id: &str) {
    if current().declared.iter().all(|ty| ty.id != id) {
        return;
    }
    update(|registry| registry.declared.retain(|ty| ty.id != id));
}

pub fn set_overrides(overrides: BTreeMap<PathBuf, String>) {
    if current().overrides == overrides {
        return;
    }
    update(|registry| registry.overrides = overrides);
}

pub fn known_name_at(path: &Path, first_line: Option<&str>) -> Option<String> {
    current().known_name_at(path, first_line)
}

pub fn type_at(path: &Path, first_line: Option<&str>) -> Option<FileType> {
    current().type_at(path, first_line)
}

pub fn set_changes(changes: Changes) {
    if current().changes == changes {
        return;
    }
    update(|registry| registry.changes = changes);
}

pub fn known_name(name: &str, first_line: Option<&str>) -> Option<String> {
    current().known_name(name, first_line)
}

pub fn is_ignored(name: &str) -> bool {
    current().is_ignored(name)
}

pub fn ignored() -> Vec<String> {
    current().ignored()
}

pub const FIRST_LINE_LIMIT: usize = 512;

pub const HEAD_LIMIT: usize = 4096;

pub fn head(text: &str) -> &str {
    let mut end = text.len().min(HEAD_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

const HEAD_CACHE_LIMIT: usize = 4096;

type HeadCache = HashMap<PathBuf, (u64, Option<std::time::SystemTime>, Option<Arc<str>>)>;

static HEADS: std::sync::Mutex<Option<HeadCache>> = std::sync::Mutex::new(None);

pub fn disk_head(path: &Path) -> Option<Arc<str>> {
    let meta = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
    let stamp = (meta.len(), meta.modified().ok());
    let mut cache = HEADS.lock().unwrap_or_else(PoisonError::into_inner);
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some((len, modified, head)) = cache.get(path)
        && (*len, *modified) == stamp
    {
        return head.clone();
    }
    let head = read_head(path);
    if cache.len() >= HEAD_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(path.to_owned(), (stamp.0, stamp.1, head.clone()));
    head
}

fn read_head(path: &Path) -> Option<Arc<str>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(HEAD_LIMIT);
    file.take(HEAD_LIMIT as u64).read_to_end(&mut bytes).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    Some(Arc::from(head(&text)))
}

pub fn first_line(text: &str) -> &str {
    let mut end = text.len().min(FIRST_LINE_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    head.split('\n').next().unwrap_or_default().trim_end_matches('\r')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(id: &str, patterns: &[&str], hashbangs: &[&str]) -> FileType {
        FileType {
            id: id.to_owned(),
            name: id.to_uppercase(),
            localized: false,
            patterns: patterns.iter().map(|pattern| (*pattern).to_owned()).collect(),
            hashbangs: hashbangs.iter().map(|hashbang| (*hashbang).to_owned()).collect(),
            contents: Vec::new(),
        }
    }

    fn registry() -> Registry {
        Registry::new(
            vec![
                ty("go", &["*.go"], &[]),
                ty("yaml", &["*.yml", "*.yaml"], &[]),
                ty("gitlab-ci", &[".gitlab-ci.yml", "*.gitlab-ci.yml"], &[]),
                ty("env", &[".env", ".env.*"], &[]),
                ty("json", &["*.json"], &[]),
                ty("shell", &["*.sh", ".bashrc"], &["sh", "bash", "zsh"]),
                ty("markdown", &["*.md", "*.markdown"], &[]),
            ],
            Changes::default(),
        )
    }

    #[test]
    fn globs_take_stars_and_question_marks() {
        assert!(glob_matches("*.go", "main.go", true));
        assert!(glob_matches("*.GO", "main.go", true));
        assert!(!glob_matches("*.GO", "main.go", false));
        assert!(glob_matches("*~", "notes.txt~", false));
        assert!(glob_matches("vssver?.scc", "vssver2.scc", false));
        assert!(glob_matches("Dockerfile.*", "Dockerfile.dev", true));
        assert!(!glob_matches("*.go", "main.gox", true));
        assert!(glob_matches("*", "", true));
    }

    #[test]
    fn a_whole_name_beats_a_pattern_which_beats_an_extension() {
        let registry = registry();
        let id = |name: &str| registry.type_of(name).map(|ty| ty.id);
        assert_eq!(id("main.go").as_deref(), Some("go"));
        assert_eq!(id(".gitlab-ci.yml").as_deref(), Some("gitlab-ci"));
        assert_eq!(id("build.gitlab-ci.yml").as_deref(), Some("gitlab-ci"));
        assert_eq!(id("a.yml").as_deref(), Some("yaml"));
        assert_eq!(id(".env.json").as_deref(), Some("env"), "a name pattern beats the extension");
        assert_eq!(id("README.MD").as_deref(), Some("markdown"));
        assert_eq!(id("LICENSE"), None);
    }

    #[test]
    fn without_changes_every_name_is_known_as_itself() {
        let registry = registry();
        for name in ["main.go", ".gitlab-ci.yml", "LICENSE", "a.tsx", "x.zzz"] {
            assert_eq!(registry.known_name(name, None).as_deref(), Some(name));
        }
    }

    #[test]
    fn a_moved_pattern_gives_the_new_types_first_name() {
        let mut registry = registry();
        let moved = registry.add_pattern("json", "*.yml");
        assert_eq!(moved.map(|ty| ty.id).as_deref(), Some("yaml"), "it says where the pattern came from");
        assert_eq!(registry.type_of("a.yml").map(|ty| ty.id).as_deref(), Some("json"));
        assert_eq!(registry.known_name("a.yml", None).as_deref(), Some("file.json"));
        assert_eq!(registry.known_name("a.yaml", None).as_deref(), Some("a.yaml"), "other names stay");
        assert!(!registry.get("yaml").unwrap().patterns.contains(&"*.yml".to_owned()));
        assert!(registry.get("json").unwrap().patterns.contains(&"*.yml".to_owned()));
        assert_eq!(registry.owner_of("*.yml", "go").map(|ty| ty.id).as_deref(), Some("json"));
        registry.remove_pattern("json", "*.yml");
        assert_eq!(registry.type_of("a.yml"), None, "the pattern left YAML when it moved");
        assert_eq!(registry.known_name("a.yml", None), None);
        registry.add_pattern("yaml", "*.yml");
        assert_eq!(registry.changes(), &Changes::default(), "back where it was declared");
    }

    #[test]
    fn a_new_pattern_gives_a_type_and_a_removed_one_takes_it_away() {
        let mut registry = registry();
        assert_eq!(registry.add_pattern("go", "*.gotmpl"), None);
        assert_eq!(registry.known_name("page.gotmpl", None).as_deref(), Some("file.go"));
        registry.remove_pattern("markdown", "*.md");
        assert_eq!(registry.known_name("README.md", None), None, "no type any more");
        assert_eq!(registry.known_name("notes.markdown", None).as_deref(), Some("notes.markdown"));
        registry.add_pattern("markdown", "*.md");
        assert_eq!(registry.changes().removed, Vec::new(), "adding a declared pattern back undoes its removal");
        assert_eq!(registry.known_name("README.md", None).as_deref(), Some("README.md"));
        registry.remove_pattern("go", "*.gotmpl");
        assert!(registry.changes().added.is_empty());
    }

    #[test]
    fn removing_a_name_pattern_falls_back_to_the_extension_type() {
        let mut registry = registry();
        registry.remove_pattern("gitlab-ci", ".gitlab-ci.yml");
        registry.remove_pattern("gitlab-ci", "*.gitlab-ci.yml");
        assert_eq!(registry.type_of(".gitlab-ci.yml").map(|ty| ty.id).as_deref(), Some("yaml"));
        assert_eq!(registry.known_name(".gitlab-ci.yml", None).as_deref(), Some("file.yml"));
    }

    #[test]
    fn hashbangs_name_the_type_of_a_file_no_pattern_names() {
        let mut registry = registry();
        assert_eq!(interpreter("#!/usr/bin/env -S bash -e").as_deref(), Some("bash"));
        assert_eq!(interpreter("#!/bin/zsh").as_deref(), Some("zsh"));
        assert_eq!(interpreter("#!/usr/bin/env FOO=1 python3").as_deref(), Some("python3"));
        assert_eq!(interpreter("echo"), None);
        assert_eq!(registry.known_name("deploy", Some("#!/bin/bash")).as_deref(), Some("file.sh"));
        assert_eq!(registry.known_name("main.go", Some("#!/bin/bash")).as_deref(), Some("main.go"), "names first");
        assert_eq!(registry.known_name("tool", Some("#!/usr/bin/env node")).as_deref(), Some("tool"));
        registry.add_hashbang("json", "node");
        assert_eq!(registry.known_name("tool", Some("#!/usr/bin/env node")).as_deref(), Some("file.json"));
        registry.remove_hashbang("shell", "bash");
        assert_eq!(registry.known_name("deploy", Some("#!/bin/bash")).as_deref(), Some("deploy"));
        assert!(hashbang_matches("python", "python3.12"));
        assert!(!hashbang_matches("python", "pythonx"));
    }

    #[test]
    fn the_ignored_names_start_from_the_defaults_and_can_be_edited() {
        let mut registry = registry();
        assert_eq!(registry.ignored().len(), 17);
        for name in [".git", "__pycache__", "a.pyc", "notes~", ".DS_Store", "CVS", "vssver2.scc"] {
            assert!(registry.is_ignored(name), "{name}");
        }
        for name in ["src", ".idea", "target", "cvs", "a.py"] {
            assert!(!registry.is_ignored(name), "{name}");
        }
        let mut edited = registry.ignored();
        edited.retain(|name| name != ".git");
        edited.push(" *.log ".to_owned());
        edited.push("*.log".to_owned());
        registry.set_ignored(edited);
        assert!(!registry.is_ignored(".git") && registry.is_ignored("x.log"));
        assert_eq!(registry.ignored().iter().filter(|name| *name == "*.log").count(), 1);
        registry.set_ignored(DEFAULT_IGNORED.iter().map(|name| (*name).to_owned()).collect());
        assert_eq!(registry.changes().ignored, None, "the defaults are no change");
    }

    #[test]
    fn globs_step_over_whole_characters() {
        assert!(glob_matches("?.md", "ж.md", true));
        assert!(glob_matches("*ё*", "приёмка.txt", false));
        assert!(!glob_matches("ПРИ*", "при", false));
        assert!(glob_matches("a*b*c", "aXbYbZc", false));
        assert!(!glob_matches("a*b", "a", false));
    }

    #[test]
    fn a_pattern_given_to_a_type_that_went_away_goes_back_to_its_owner() {
        let mut registry = registry();
        registry.add_pattern("json", "*.md");
        assert_eq!(registry.type_of("README.md").map(|ty| ty.id).as_deref(), Some("json"));
        let declared = registry.declared().iter().filter(|ty| ty.id != "json").cloned().collect();
        let registry = Registry::new(declared, registry.changes().clone());
        assert_eq!(registry.type_of("README.md").map(|ty| ty.id).as_deref(), Some("markdown"));
        assert_eq!(registry.known_name("README.md", None).as_deref(), Some("README.md"));
    }

    #[test]
    fn the_first_line_is_read_only_so_far() {
        let long = format!("#!/bin/bash {}\necho", "x".repeat(10_000));
        assert_eq!(first_line(&long).len(), FIRST_LINE_LIMIT);
        assert_eq!(interpreter(first_line(&long)).as_deref(), Some("bash"));
        let wide = "ж".repeat(FIRST_LINE_LIMIT);
        assert!(first_line(&wide).len() <= FIRST_LINE_LIMIT);
        assert_eq!(first_line("#!/bin/sh\r\nexit"), "#!/bin/sh");
    }

    #[test]
    fn a_files_own_type_changes_only_that_file() {
        let mut registry = registry();
        registry.declared.push(ty(TEXT, &["*.txt"], &[]));
        registry.refresh();
        let (main, other) = (Path::new("/p/main.go"), Path::new("/p/other.go"));
        registry.set_override(main, Some(TEXT));
        assert_eq!(registry.type_at(main, None).map(|ty| ty.id).as_deref(), Some(TEXT));
        assert_eq!(registry.known_name_at(main, None).as_deref(), Some("file.txt"));
        assert_eq!(registry.known_name_at(other, None).as_deref(), Some("other.go"), "the other file keeps Go");
        registry.set_override(Path::new("/p/notes"), Some("markdown"));
        assert_eq!(registry.known_name_at(Path::new("/p/notes"), None).as_deref(), Some("file.md"));
        registry.set_override(Path::new("/p/README.md"), Some("markdown"));
        assert_eq!(registry.known_name_at(Path::new("/p/README.md"), None).as_deref(), Some("README.md"));
        registry.set_override(Path::new("/p/a.yml"), Some("gone"));
        assert_eq!(registry.overridden(Path::new("/p/a.yml")), None, "a type nobody declares now is not used");
        assert_eq!(registry.known_name_at(Path::new("/p/a.yml"), None).as_deref(), Some("a.yml"));
        registry.set_override(main, None);
        assert_eq!(registry.known_name_at(main, None).as_deref(), Some("main.go"));
        assert_eq!(registry.overrides().len(), 3);
    }

    #[test]
    fn the_detected_and_application_types_are_known_as_plain_text() {
        let mut registry = registry();
        registry.declared.extend([ty(TEXT, &["*.txt"], &[]), ty(DETECTED, &[], &[]), ty(NATIVE, &[], &[])]);
        registry.refresh();
        registry.add_pattern(DETECTED, "*.zz");
        registry.add_pattern(NATIVE, "*.go");
        assert_eq!(registry.type_of("a.zz").map(|ty| ty.id).as_deref(), Some(DETECTED));
        assert_eq!(registry.known_name("a.zz", None).as_deref(), Some("file.txt"));
        assert_eq!(registry.known_name("main.go", None).as_deref(), Some("file.txt"), "not highlighted as Go");
        assert_eq!(registry.type_at(Path::new("/p/main.go"), None).map(|ty| ty.id).as_deref(), Some(NATIVE));
    }

    #[test]
    fn the_indexed_lookup_agrees_with_trying_every_pattern() {
        let mut registry = registry();
        registry.add_pattern("json", "*.yml");
        registry.add_pattern("go", "Dockerfile*");
        registry.add_pattern("markdown", "README");
        let names = [
            "a.yml",
            "A.YML",
            ".gitlab-ci.yml",
            "x.gitlab-ci.yml",
            ".env",
            ".env.json",
            "Dockerfile",
            "Dockerfile.dev",
            "README",
            "readme",
            "README.md",
            "main.go",
            ".go",
            "noext",
            "a.tar.json",
            ".bashrc",
            "x.sh",
            "",
        ];
        for name in names {
            let linear = best(registry.effective.iter().map(|ty| (&ty.declared, ty.patterns.as_slice())), name);
            assert_eq!(registry.type_of(name).as_ref(), linear, "{name}");
            let declared = best(registry.declared.iter().map(|ty| (ty, ty.patterns.as_slice())), name);
            assert_eq!(registry.declared_type_id_of(name), declared.map(|ty| ty.id.as_str()), "{name}");
        }
    }

    fn xml_markers() -> Vec<ContentMarker> {
        vec![
            ContentMarker { starts: "<?xml".to_owned(), ..ContentMarker::default() },
            ContentMarker {
                starts: "<!DOCTYPE".to_owned(),
                unless: vec!["<!DOCTYPE html".to_owned()],
                ..ContentMarker::default()
            },
            ContentMarker {
                starts: "<".to_owned(),
                unless: vec!["<html".to_owned(), "<!".to_owned(), "<?".to_owned()],
                first_tag: Some("xmlns".to_owned()),
            },
        ]
    }

    fn with_xml() -> Registry {
        let mut types = registry().declared().to_vec();
        types.push(ty("html", &["*.html"], &[]));
        types.push(FileType { contents: xml_markers(), ..ty("xml", &["*.xml", "*.plist"], &[]) });
        Registry::new(types, Changes::default())
    }

    const PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict/>\n</plist>\n";

    #[test]
    fn content_markers_name_xml_and_never_html() {
        let registry = with_xml();
        let by_content = |head: &str| registry.type_by_content(head).map(|ty| ty.id);
        for head in [
            PLIST,
            "\u{feff}<?xml version=\"1.0\"?><a/>",
            "\n\n   <?xml version=\"1.0\"?>",
            "<!DOCTYPE plist SYSTEM \"x.dtd\">\n<plist/>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\"/>",
            "<project xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
        ] {
            assert_eq!(by_content(head).as_deref(), Some("xml"), "{head}");
        }
        for head in [
            "<!DOCTYPE html>\n<html>",
            "<!doctype HTML>",
            "<html xmlns=\"http://www.w3.org/1999/xhtml\">",
            "<div>no namespace</div>",
            "<?xml\0binary",
            "plain <?xml in the middle",
            "",
        ] {
            assert_eq!(by_content(head), None, "{head:?}");
        }
        assert_eq!(registry.known_name("timeweb-cloud.cyberduckprofile", Some(PLIST)).as_deref(), Some("file.xml"));
        assert_eq!(registry.known_name("Info", Some(PLIST)).as_deref(), Some("file.xml"));
        assert_eq!(registry.known_name("page.html", Some(PLIST)).as_deref(), Some("page.html"), "a pattern wins");
        assert_eq!(registry.known_name("notes", Some("just text")).as_deref(), Some("notes"));
        assert_eq!(
            registry.known_name("run", Some("#!/bin/bash\n<?xml")).as_deref(),
            Some("file.sh"),
            "a hashbang first"
        );
    }

    #[test]
    fn a_user_pattern_and_an_override_win_over_content() {
        let mut registry = with_xml();
        let profile = Path::new("/p/timeweb-cloud.cyberduckprofile");
        assert_eq!(registry.type_at(profile, Some(PLIST)).map(|ty| ty.id).as_deref(), Some("xml"));
        assert!(registry.by_content(profile, Some(PLIST)));
        registry.add_pattern("markdown", "*.cyberduckprofile");
        assert_eq!(registry.type_at(profile, Some(PLIST)).map(|ty| ty.id).as_deref(), Some("markdown"));
        assert!(!registry.by_content(profile, Some(PLIST)));
        let mut registry = with_xml();
        registry.set_override(profile, Some("json"));
        assert_eq!(registry.type_at(profile, Some(PLIST)).map(|ty| ty.id).as_deref(), Some("json"));
        assert!(!registry.by_content(profile, Some(PLIST)));
    }

    #[test]
    fn a_file_on_disk_is_read_once_and_only_its_head() {
        let dir = tempfile::tempdir().unwrap();
        let registry = with_xml();
        let profile = dir.path().join("timeweb-cloud.cyberduckprofile");
        std::fs::write(&profile, PLIST).unwrap();
        assert_eq!(registry.known_name_at(&profile, None).as_deref(), Some("file.xml"));
        assert_eq!(registry.type_at(&profile, None).map(|ty| ty.id).as_deref(), Some("xml"));
        assert!(registry.by_content(&profile, None));

        let binary = dir.path().join("blob");
        let mut bytes = b"<?xml version=\"1.0\"?>".to_vec();
        bytes.extend([0u8; 16]);
        std::fs::write(&binary, bytes).unwrap();
        assert_eq!(disk_head(&binary), None, "a binary file has no head");
        assert_eq!(registry.known_name_at(&binary, None).as_deref(), Some("blob"));

        let late = dir.path().join("late");
        std::fs::write(&late, format!("{}<?xml version=\"1.0\"?>", " ".repeat(HEAD_LIMIT))).unwrap();
        assert_eq!(registry.type_at(&late, None), None, "only the head is read");
        let huge = dir.path().join("huge");
        let mut text = String::from("<?xml version=\"1.0\"?>\n<a>");
        text.push_str(&"x".repeat(4 * 1024 * 1024));
        std::fs::write(&huge, &text).unwrap();
        assert_eq!(disk_head(&huge).map(|head| head.len()), Some(HEAD_LIMIT));

        assert_eq!(disk_head(dir.path()), None, "a folder");
        assert_eq!(registry.known_name_at(&dir.path().join("missing"), None).as_deref(), Some("missing"));

        std::fs::write(&profile, "plain now, and longer than before").unwrap();
        assert_eq!(
            registry.known_name_at(&profile, None).as_deref(),
            Some("timeweb-cloud.cyberduckprofile"),
            "read again"
        );
    }
}

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use zen_text::search::{Query, find_all};

use crate::literals::need;
use crate::{FileFilter, FileIndex};

pub(crate) const MAX_FILE: u64 = 2_500_000;

pub type KeepMatches = dyn Fn(&Path, &str, Vec<Range<usize>>) -> Vec<Range<usize>> + Sync;

pub trait SearchMemory: Sync {
    fn stamp(&self, path: &Path, open: Option<&str>) -> Option<u64>;
    fn recall(&self, path: &Path, stamp: u64) -> Option<Vec<TextMatch>>;
    fn remember(&self, path: &Path, stamp: u64, matches: &[TextMatch]);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    pub query: Query,
    pub mask: String,
    pub directory: String,
    pub limit: usize,
    pub filter: Option<FileFilter>,
    pub extra: Vec<(PathBuf, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMatch {
    pub path: PathBuf,
    pub relative: String,
    pub line: usize,
    pub column: usize,
    pub range: Range<usize>,
    pub line_text: String,
}

impl FileIndex {
    pub fn search_text(
        &self,
        search: &Search,
        open: &HashMap<PathBuf, String>,
        cancel: &AtomicBool,
        found: &(dyn Fn(Vec<TextMatch>) + Sync),
    ) -> Result<usize, String> {
        self.search_text_keeping(search, open, cancel, found, &|_, _, ranges| ranges)
    }

    pub fn search_text_keeping(
        &self,
        search: &Search,
        open: &HashMap<PathBuf, String>,
        cancel: &AtomicBool,
        found: &(dyn Fn(Vec<TextMatch>) + Sync),
        keep: &KeepMatches,
    ) -> Result<usize, String> {
        self.search_text_with(search, open, cancel, found, keep, None, &|_| {})
    }

    pub fn search_text_remembering(
        &self,
        search: &Search,
        open: &HashMap<PathBuf, String>,
        cancel: &AtomicBool,
        found: &(dyn Fn(Vec<TextMatch>) + Sync),
        keep: &KeepMatches,
        memory: Option<&dyn SearchMemory>,
    ) -> Result<usize, String> {
        self.search_text_with(search, open, cancel, found, keep, memory, &|_| {})
    }

    pub fn search_text_visiting(
        &self,
        search: &Search,
        open: &HashMap<PathBuf, String>,
        cancel: &AtomicBool,
        found: &(dyn Fn(Vec<TextMatch>) + Sync),
        keep: &KeepMatches,
        visit: &(dyn Fn(&str) + Sync),
    ) -> Result<usize, String> {
        self.search_text_with(search, open, cancel, found, keep, None, visit)
    }

    #[allow(clippy::too_many_arguments)]
    fn search_text_with(
        &self,
        search: &Search,
        open: &HashMap<PathBuf, String>,
        cancel: &AtomicBool,
        found: &(dyn Fn(Vec<TextMatch>) + Sync),
        keep: &KeepMatches,
        memory: Option<&dyn SearchMemory>,
        visit: &(dyn Fn(&str) + Sync),
    ) -> Result<usize, String> {
        find_all("", &search.query)
            .map(|_| ())
            .or_else(|error| if search.query.text.is_empty() { Ok(()) } else { Err(error) })?;
        if search.query.text.is_empty() {
            return Ok(0);
        }
        let masks = Masks::parse(&search.mask);
        let directory = search.directory.trim_matches('/');
        let files: Vec<(PathBuf, String)> = self
            .file_list()
            .into_iter()
            .filter(|(_, relative)| directory.is_empty() || relative.starts_with(&format!("{directory}/")))
            .filter(|(_, relative)| search.filter.as_ref().is_none_or(|filter| filter.accepts(relative)))
            .chain(search.extra.iter().cloned())
            .filter(|(_, relative)| masks.matches(relative.rsplit('/').next().unwrap_or(relative)))
            .collect();
        let files = self.text.candidates(files, &need(&search.query), !search.query.match_case, open);
        let total = AtomicUsize::new(0);
        let next = AtomicUsize::new(0);
        let limit = if search.limit == 0 { usize::MAX } else { search.limit };
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
        let work = || {
            loop {
                if cancel.load(Ordering::Relaxed) || total.load(Ordering::Relaxed) >= limit {
                    return;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((path, relative)) = files.get(index) else { return };
                visit(relative);
                let unsaved = open.get(path);
                let stamp = memory.and_then(|memory| memory.stamp(path, unsaved.map(String::as_str)));
                let mut matches = match memory.zip(stamp).and_then(|(memory, stamp)| memory.recall(path, stamp)) {
                    Some(matches) => matches,
                    None => {
                        let Some(text) = unsaved.cloned().or_else(|| read_text(path)) else { continue };
                        let Ok(ranges) = find_all(&text, &search.query) else { return };
                        let ranges = if ranges.is_empty() { ranges } else { keep(path, &text, ranges) };
                        let matches = locate(&text, &ranges, path, relative);
                        if let Some((memory, stamp)) = memory.zip(stamp) {
                            memory.remember(path, stamp, &matches);
                        }
                        matches
                    }
                };
                if matches.is_empty() {
                    continue;
                }
                let room = limit.saturating_sub(total.fetch_add(matches.len(), Ordering::Relaxed));
                matches.truncate(room);
                if !matches.is_empty() {
                    found(matches);
                }
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(work);
            }
        });
        #[cfg(target_arch = "wasm32")]
        {
            let _ = workers;
            work();
        }
        Ok(total.load(Ordering::Relaxed).min(limit))
    }
}

pub(crate) fn read_text(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.iter().take(8000).any(|byte| *byte == 0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn locate(text: &str, ranges: &[Range<usize>], path: &Path, relative: &str) -> Vec<TextMatch> {
    let mut matches = Vec::with_capacity(ranges.len());
    let mut line = 0;
    let mut line_start_char = 0;
    let mut line_start_byte = 0;
    let mut chars = text.char_indices().enumerate().peekable();
    for range in ranges {
        while let Some(&(char_index, (byte, c))) = chars.peek() {
            if char_index >= range.start {
                break;
            }
            chars.next();
            let ends_line = c == '\n' || (c == '\r' && !text[byte + 1..].starts_with('\n'));
            if ends_line {
                line += 1;
                line_start_char = char_index + 1;
                line_start_byte = byte + 1;
            }
        }
        let rest = &text[line_start_byte..];
        let line_text = rest.split(['\n', '\r']).next().unwrap_or(rest).to_owned();
        matches.push(TextMatch {
            path: path.to_owned(),
            relative: relative.to_owned(),
            line,
            column: range.start - line_start_char,
            range: range.clone(),
            line_text,
        });
    }
    matches
}

struct Masks {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl Masks {
    fn parse(masks: &str) -> Self {
        let mut include = Vec::new();
        let mut exclude = Vec::new();
        for mask in masks.split(',').map(str::trim).filter(|mask| !mask.is_empty()) {
            match mask.strip_prefix('!') {
                Some(excluded) => exclude.push(excluded.to_lowercase()),
                None => include.push(mask.to_lowercase()),
            }
        }
        Self { include, exclude }
    }

    fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        (self.include.is_empty() || self.include.iter().any(|mask| glob(mask, &name)))
            && !self.exclude.iter().any(|mask| glob(mask, &name))
    }
}

pub fn glob(pattern: &str, text: &str) -> bool {
    let (pattern, text): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut p, mut t, mut star, mut mark) = (0, 0, None, 0);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if let Some(s) = star {
            p = s + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use proptest::prelude::*;

    use super::*;
    use crate::{IndexProgress, Progress};

    fn project() -> (tempfile::TempDir, FileIndex) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("internal/cart")).unwrap();
        std::fs::write(root.join("main.go"), "package main\n\nfunc main() {\n\tcart.New()\n}\n").unwrap();
        std::fs::write(
            root.join("internal/cart/cart.go"),
            "package cart\n\n// New returns a cart.\nfunc New() *Cart { return &Cart{} }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("internal/cart/cart_test.go"),
            "package cart\n\nfunc TestNew(t *testing.T) { New() }\n",
        )
        .unwrap();
        std::fs::write(root.join("logo.png"), [0u8, 1, 2, b'N', b'e', b'w']).unwrap();
        let index = FileIndex::new(root);
        while !index.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        (dir, index)
    }

    fn run(index: &FileIndex, search: &Search, open: &HashMap<PathBuf, String>) -> Vec<TextMatch> {
        let found = Mutex::new(Vec::new());
        index
            .search_text(search, open, &AtomicBool::new(false), &|matches| found.lock().unwrap().extend(matches))
            .unwrap();
        let mut found = found.into_inner().unwrap();
        found.sort_by(|a, b| (&a.relative, a.line).cmp(&(&b.relative, b.line)));
        found
    }

    fn search(text: &str) -> Search {
        Search { query: Query { text: text.to_owned(), match_case: true, ..Query::default() }, ..Search::default() }
    }

    #[test]
    fn a_file_with_carriage_returns_only_reports_its_lines() {
        let text = "one\rneedle two\r\nthree needle\n";
        let found = locate(text, &[4..10, 22..28], Path::new("/p/a.txt"), "a.txt");
        let lines: Vec<(usize, usize, &str)> =
            found.iter().map(|one| (one.line, one.column, one.line_text.as_str())).collect();
        assert_eq!(lines, [(1, 0, "needle two"), (2, 6, "three needle")]);
    }

    #[test]
    fn finds_matches_with_lines_and_columns() {
        let (_dir, index) = project();
        let found = run(&index, &search("New"), &HashMap::new());
        let places: Vec<(String, usize, usize)> =
            found.iter().map(|m| (m.relative.clone(), m.line, m.column)).collect();
        assert_eq!(
            places,
            [
                ("internal/cart/cart.go".to_owned(), 2, 3),
                ("internal/cart/cart.go".to_owned(), 3, 5),
                ("internal/cart/cart_test.go".to_owned(), 2, 9),
                ("internal/cart/cart_test.go".to_owned(), 2, 29),
                ("main.go".to_owned(), 3, 6),
            ],
            "binary files are skipped"
        );
        assert_eq!(found[4].line_text, "\tcart.New()");
    }

    #[test]
    fn masks_directories_unsaved_text_and_limits() {
        let (dir, index) = project();
        let masked = run(&index, &Search { mask: "*.go, !*_test.go".to_owned(), ..search("New") }, &HashMap::new());
        assert!(masked.iter().all(|m| !m.relative.ends_with("_test.go")));
        let scoped = run(&index, &Search { directory: "internal".to_owned(), ..search("package") }, &HashMap::new());
        assert_eq!(scoped.len(), 2);
        let open = HashMap::from([(dir.path().join("main.go"), "package main // New unsaved".to_owned())]);
        let unsaved = run(&index, &search("unsaved"), &open);
        assert_eq!(unsaved.len(), 1, "open files are searched in their editor text");
        let limited = run(&index, &Search { limit: 2, ..search("New") }, &HashMap::new());
        assert!(limited.len() <= 2);
        let bad = index.search_text(
            &Search { query: Query { text: "(".to_owned(), regex: true, ..Query::default() }, ..Search::default() },
            &HashMap::new(),
            &AtomicBool::new(false),
            &|_| {},
        );
        assert!(bad.is_err());
    }

    #[test]
    fn keep_decides_which_matches_of_a_file_count() {
        let (_dir, index) = project();
        let found = Mutex::new(Vec::new());
        let keep = |path: &Path, text: &str, ranges: Vec<Range<usize>>| {
            assert!(text.contains("New"));
            if path.ends_with("cart.go") { ranges.into_iter().take(1).collect() } else { Vec::new() }
        };
        let total = index
            .search_text_keeping(
                &search("New"),
                &HashMap::new(),
                &AtomicBool::new(false),
                &|matches| found.lock().unwrap().extend(matches),
                &keep,
            )
            .unwrap();
        let found = found.into_inner().unwrap();
        assert_eq!((total, found.len()), (1, 1));
        assert_eq!((found[0].relative.as_str(), found[0].line), ("internal/cart/cart.go", 2));
    }

    #[test]
    fn find_in_files_skips_what_the_project_excludes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("gen")).unwrap();
        std::fs::write(root.join("main.go"), "needle\n").unwrap();
        std::fs::write(root.join("gen/out.go"), "needle\n").unwrap();
        std::fs::write(root.join("trace.log"), "needle\n").unwrap();
        let index = FileIndex::with_excluded(root, &[root.join("gen")], &["*.log".to_owned()]);
        while !index.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let found = run(&index, &search("needle"), &HashMap::new());
        let files: Vec<&str> = found.iter().map(|m| m.relative.as_str()).collect();
        assert_eq!(files, ["main.go"]);
    }

    fn wait_for_text(index: &FileIndex) {
        while !index.text_status().ready {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn unindexed(index: &FileIndex) -> FileIndex {
        FileIndex { text: Arc::default(), ..index.clone() }
    }

    fn places(index: &FileIndex, search: &Search, open: &HashMap<PathBuf, String>) -> Vec<(String, Range<usize>)> {
        let mut found: Vec<(String, Range<usize>)> =
            run(index, search, open).into_iter().map(|m| (m.relative, m.range)).collect();
        found.sort_by(|a, b| (&a.0, a.1.start).cmp(&(&b.0, b.1.start)));
        found
    }

    #[test]
    fn binary_large_and_excluded_files_are_not_indexed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("gen")).unwrap();
        std::fs::write(root.join("main.go"), "needle\n").unwrap();
        std::fs::write(root.join("gen/out.go"), "needle\n").unwrap();
        std::fs::write(root.join("logo.png"), [0u8, b'n', b'e', b'e', b'd', b'l', b'e']).unwrap();
        std::fs::write(root.join("huge.txt"), "needle ".repeat(MAX_FILE as usize / 7 + 1)).unwrap();
        let progress: Arc<Mutex<Vec<IndexProgress>>> = Arc::default();
        let seen = Arc::clone(&progress);
        let report: Progress = Arc::new(move |step| seen.lock().unwrap().push(step));
        let index = FileIndex::with_progress(root, &[root.join("gen")], &[], Some(report));
        wait_for_text(&index);
        let status = index.text_status();
        assert_eq!((status.files, status.skipped, status.indexed, status.total), (1, 2, 3, 3));
        assert!(status.trigrams > 0);
        assert_eq!(progress.lock().unwrap().last(), Some(&IndexProgress { indexed: 3, total: 3, done: true }));
        let found = run(&index, &search("needle"), &HashMap::new());
        assert_eq!(found.iter().map(|m| m.relative.as_str()).collect::<Vec<_>>(), ["main.go"]);
    }

    #[test]
    fn changed_files_are_read_again_and_searched_in_full_until_then() {
        let (dir, index) = project();
        wait_for_text(&index);
        let root = dir.path();
        std::fs::write(root.join("main.go"), "package main // fresh\n").unwrap();
        std::fs::create_dir_all(root.join("more")).unwrap();
        std::fs::write(root.join("more/extra.go"), "fresh too\n").unwrap();
        index.refresh(&[root.join("main.go"), root.join("more")]);
        let found = run(&index, &search("fresh"), &HashMap::new());
        assert_eq!(found.len(), 2, "found at once, read again or not");
        for _ in 0..400 {
            if index.text_status().files == 4 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(index.text_status().files, 4);
        assert_eq!(run(&index, &search("fresh"), &HashMap::new()).len(), 2);
        std::fs::remove_file(root.join("more/extra.go")).unwrap();
        index.refresh(&[root.join("more")]);
        assert_eq!(index.text_status().files, 3);
        assert_eq!(run(&index, &search("fresh"), &HashMap::new()).len(), 1);
    }

    #[test]
    fn while_paused_changed_files_wait_and_are_read_on_resuming() {
        let (dir, index) = project();
        wait_for_text(&index);
        let root = dir.path();
        let before = index.text_status().files;
        let busy = index.busy_time();
        assert!(busy > std::time::Duration::ZERO, "the first build is counted");
        index.set_paused(true);
        assert!(index.is_paused());
        std::fs::write(root.join("main.go"), "package main // fresh\n").unwrap();
        std::fs::write(root.join("new.go"), "fresh too\n").unwrap();
        index.refresh(&[root.join("main.go"), root.join("new.go")]);
        assert_eq!(index.deferred(), 2, "nothing is read while paused");
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(index.text_status().files, before - 1, "main.go is forgotten, not read again");
        assert_eq!(index.busy_time(), busy, "no worker ran");
        assert_eq!(run(&index, &search("fresh"), &HashMap::new()).len(), 2, "searched in full meanwhile");
        assert!(index.search("new.go", 5, &[]).iter().any(|found| found.relative == "new.go"), "names stay current");
        index.set_paused(false);
        assert_eq!(index.deferred(), 0);
        for _ in 0..400 {
            if index.text_status().files == before + 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(index.text_status().files, before + 1, "read on resuming");
        assert!(index.busy_time() > busy);
        assert!(index.memory() > 0);
        assert_eq!(run(&index, &search("fresh"), &HashMap::new()).len(), 2);
    }

    #[test]
    fn before_the_text_index_is_built_every_file_is_searched() {
        let (_dir, index) = project();
        wait_for_text(&index);
        let building = unindexed(&index);
        assert!(!building.text_status().ready);
        let open = HashMap::new();
        assert_eq!(places(&building, &search("New"), &open), places(&index, &search("New"), &open));
        assert_eq!(places(&building, &search("New"), &open).len(), 5);
    }

    #[test]
    fn a_query_over_a_hundred_thousand_files_shows_its_first_results_within_a_hundred_milliseconds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let texts: Vec<(String, String)> = (0..100_000)
            .map(|i| {
                let relative = format!("pkg{}/sub{}/file_{i}.go", i % 300, i % 17);
                let needle = if i % 5_000 == 0 { "uniqueNeedle" } else { "handler" };
                (relative, format!("package pkg\n\nfunc Serve{i}() {{ return {needle}({i}) }}\n"))
            })
            .collect();
        for (relative, text) in texts.iter().filter(|(_, text)| text.contains("uniqueNeedle")) {
            std::fs::create_dir_all(root.join(relative).parent().unwrap()).unwrap();
            std::fs::write(root.join(relative), text).unwrap();
        }
        let pairs: Vec<(&str, &str)> =
            texts.iter().map(|(relative, text)| (relative.as_str(), text.as_str())).collect();
        let names = texts
            .iter()
            .map(|(relative, _)| crate::Indexed { name: crate::file_name(relative), relative: relative.clone() })
            .collect();
        let index = FileIndex {
            root: root.to_owned(),
            excluded: Arc::default(),
            files: Arc::new(Mutex::new(Some(names))),
            text: Arc::new(crate::trigram::built(&pairs)),
            git_ignores: Arc::new(crate::git_ignore::GitIgnores::new(root)),
            deferred: Arc::default(),
            pending: Arc::default(),
        };
        let started = std::time::Instant::now();
        let first = Mutex::new(None);
        let count = index
            .search_text(&search("uniqueNeedle"), &HashMap::new(), &AtomicBool::new(false), &|_| {
                first.lock().unwrap().get_or_insert_with(|| started.elapsed());
            })
            .unwrap();
        let first = first.into_inner().unwrap().unwrap();
        assert_eq!(count, 20);
        let limit = if cfg!(debug_assertions) { 2_000 } else { 100 };
        if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
            assert!(first.as_millis() < limit, "{first:?}");
        }
    }

    fn regex_piece() -> impl Strategy<Value = String> {
        const ATOMS: &[&str] = &[
            "a",
            "b",
            "A",
            "ab",
            "ba",
            "k",
            "K",
            "s",
            "\\.",
            ".",
            "[ab]",
            "\\w",
            "(a|b)",
            "(?:ab|ba)",
            "x",
            " ",
            "п",
            "П",
            "\\<",
            "\\>",
            "\\b",
            "^",
            "$",
            "(?i)",
        ];
        const QUANTIFIERS: &[&str] = &["", "", "", "?", "*", "+", "{2}", "{0,1}"];
        (prop::sample::select(ATOMS), prop::sample::select(QUANTIFIERS))
            .prop_map(|(atom, quantifier)| format!("{atom}{quantifier}"))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn the_index_finds_what_a_full_scan_finds(
            texts in prop::collection::vec("[abABsSK\u{17F}\u{212A}пПx .\n]{0,60}", 1..6),
            plain in prop::collection::vec("[abABsSK\u{17F}\u{212A}пПx .]{1,5}", 1..6),
            regexes in prop::collection::vec(prop::collection::vec(regex_piece(), 1..5), 1..6),
            edited in "[abAB .]{0,20}",
        ) {
            let dir = tempfile::tempdir().unwrap();
            for (i, text) in texts.iter().enumerate() {
                std::fs::write(dir.path().join(format!("f{i}.txt")), text).unwrap();
            }
            let index = FileIndex::new(dir.path());
            while !index.is_ready() {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            wait_for_text(&index);
            let full = unindexed(&index);
            let open = HashMap::from([(dir.path().join("f0.txt"), edited)]);
            let queries = plain
                .iter()
                .map(|text| (text.clone(), false))
                .chain(regexes.iter().map(|pieces| (pieces.concat(), true)));
            for (text, regex) in queries {
                for (match_case, whole_words) in [(true, false), (false, false), (false, true)] {
                    let query = Query { text: text.clone(), match_case, whole_words, regex, ..Query::default() };
                    if find_all("", &query).is_err() {
                        continue;
                    }
                    let search = Search { query, ..Search::default() };
                    for open in [&HashMap::new(), &open] {
                        prop_assert_eq!(places(&index, &search, open), places(&full, &search, open), "{:?}", search.query);
                    }
                }
            }
        }
    }

    #[test]
    fn a_filter_keeps_project_files_and_extra_files_are_searched_too() {
        let (_dir, index) = project();
        let tests = FileFilter::new(|relative| relative.ends_with("_test.go"));
        let found = run(&index, &Search { filter: Some(tests), ..search("New") }, &HashMap::new());
        assert!(found.iter().all(|m| m.relative == "internal/cart/cart_test.go") && !found.is_empty());
        let library = tempfile::tempdir().unwrap();
        std::fs::write(library.path().join("lib.go"), "func New() {}\n").unwrap();
        let extra = vec![(library.path().join("lib.go"), "sdk/lib.go".to_owned())];
        let none = FileFilter::new(|_| false);
        let found = run(&index, &Search { filter: Some(none), extra, ..search("New") }, &HashMap::new());
        let files: Vec<&str> = found.iter().map(|m| m.relative.as_str()).collect();
        assert_eq!(files, ["sdk/lib.go"]);
        assert_eq!(crate::list_files(library.path()), [(library.path().join("lib.go"), "lib.go".to_owned())]);
    }

    #[test]
    fn wildcards() {
        assert!(glob("*.go", "main.go"));
        assert!(glob("*_test.go", "cart_test.go"));
        assert!(!glob("*.go", "main.gox"));
        assert!(glob("ma?n.*", "main.go"));
    }
}

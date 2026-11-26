use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};

use crate::literals::Need;
use crate::text_search::read_text;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexProgress {
    pub indexed: usize,
    pub total: usize,
    pub done: bool,
}

pub type Progress = Arc<dyn Fn(IndexProgress) + Send + Sync>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextIndexStatus {
    pub ready: bool,
    pub paused: bool,
    pub indexed: usize,
    pub total: usize,
    pub files: usize,
    pub skipped: usize,
    pub trigrams: usize,
}

const PROGRESS_EVERY: usize = 500;

#[derive(Debug, Default)]
struct Hold {
    paused: bool,
    released: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BuildPause(Arc<(Mutex<Hold>, Condvar)>);

impl BuildPause {
    fn hold(&self) -> MutexGuard<'_, Hold> {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set(&self, paused: bool) {
        self.hold().paused = paused;
        self.0.1.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        self.hold().paused
    }

    fn wait(&self) {
        let mut hold = self.hold();
        while hold.paused && !hold.released {
            hold = self.0.1.wait(hold).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn release(&self) {
        self.hold().released = true;
        self.0.1.notify_all();
    }
}

#[derive(Debug, Default)]
struct State {
    live: Vec<bool>,
    ids: HashMap<String, u32>,
    skipped: HashSet<String>,
    postings: HashMap<u32, Postings>,
    invalidated: HashMap<String, u64>,
    dead: usize,
    readers: usize,
}

#[derive(Debug, Default)]
struct Postings {
    last: u32,
    bytes: Vec<u8>,
}

impl Postings {
    fn push(&mut self, id: u32) {
        let mut delta = id - self.last;
        self.last = id;
        loop {
            let byte = (delta & 0x7F) as u8;
            delta >>= 7;
            if delta == 0 {
                self.bytes.push(byte);
                return;
            }
            self.bytes.push(byte | 0x80);
        }
    }

    fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        let (mut at, mut id) = (0, 0u32);
        std::iter::from_fn(move || {
            let mut delta = 0u32;
            let mut shift = 0;
            loop {
                let byte = *self.bytes.get(at)?;
                at += 1;
                delta |= u32::from(byte & 0x7F) << shift;
                shift += 7;
                if byte & 0x80 == 0 {
                    id += delta;
                    return Some(id);
                }
            }
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct TextIndex {
    state: Mutex<State>,
    epoch: AtomicU64,
    indexed: AtomicUsize,
    total: AtomicUsize,
    built: AtomicBool,
    busy: AtomicU64,
    pause: BuildPause,
}

impl Drop for TextIndex {
    fn drop(&mut self) {
        self.pause.release();
    }
}

impl TextIndex {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn status(&self) -> TextIndexStatus {
        let state = self.state();
        TextIndexStatus {
            ready: self.built.load(Ordering::Acquire),
            paused: self.pause.is_paused(),
            indexed: self.indexed.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            files: state.ids.len(),
            skipped: state.skipped.len(),
            trigrams: state.postings.len(),
        }
    }

    pub(crate) fn build_pause(&self) -> BuildPause {
        self.pause.clone()
    }

    pub(crate) fn busy_nanos(&self) -> u64 {
        self.busy.load(Ordering::Relaxed)
    }

    pub(crate) fn memory(&self) -> usize {
        let state = self.state();
        let postings: usize = state.postings.values().map(|postings| postings.bytes.capacity() + 40).sum();
        let names: usize = state.ids.keys().chain(&state.skipped).map(|name| name.capacity() + 40).sum();
        postings + names + state.live.capacity()
    }

    fn insert(&self, relative: &str, started: u64, trigrams: Option<&[u32]>) {
        let mut state = self.state();
        let changed = ancestors(relative).any(|path| state.invalidated.get(path).is_some_and(|at| *at > started));
        if changed {
            return;
        }
        state.forget(relative);
        let Some(trigrams) = trigrams else {
            state.skipped.insert(relative.to_owned());
            return;
        };
        let Ok(id) = u32::try_from(state.live.len()) else { return };
        state.live.push(true);
        state.ids.insert(relative.to_owned(), id);
        for trigram in trigrams {
            state.postings.entry(*trigram).or_default().push(id);
        }
    }

    pub(crate) fn invalidate(&self, relatives: &[String]) {
        let at = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let mut state = self.state();
        let mut folders = HashSet::new();
        for relative in relatives {
            state.invalidated.insert(relative.clone(), at);
            if !state.forget(relative) {
                folders.insert(relative.as_str());
            }
        }
        if !folders.is_empty() {
            let inside: Vec<String> = state
                .ids
                .keys()
                .chain(&state.skipped)
                .filter(|path| ancestors(path).skip(1).any(|folder| folders.contains(folder)))
                .cloned()
                .collect();
            for path in inside {
                state.forget(&path);
            }
        }
        if state.dead > 1024 && state.dead > state.ids.len() {
            state.compact();
        }
    }

    pub(crate) fn candidates(
        &self,
        files: Vec<(PathBuf, String)>,
        need: &Need,
        case_insensitive: bool,
        open: &HashMap<PathBuf, String>,
    ) -> Vec<(PathBuf, String)> {
        let state = self.state();
        let Some(ids) = state.evaluate(need, case_insensitive) else { return files };
        files
            .into_iter()
            .filter(|(path, relative)| {
                open.contains_key(path)
                    || match state.ids.get(relative) {
                        Some(id) => ids.binary_search(id).is_ok(),
                        None => !state.skipped.contains(relative),
                    }
            })
            .collect()
    }
}

impl State {
    fn forget(&mut self, relative: &str) -> bool {
        if self.skipped.remove(relative) {
            return true;
        }
        let Some(id) = self.ids.remove(relative) else { return false };
        if let Some(live) = self.live.get_mut(id as usize) {
            *live = false;
        }
        self.dead += 1;
        true
    }

    fn compact(&mut self) {
        let live = &self.live;
        self.postings.retain(|_, postings| {
            let mut kept = Postings::default();
            for id in postings.ids().filter(|id| live.get(*id as usize).copied().unwrap_or(false)) {
                kept.push(id);
            }
            kept.bytes.shrink_to_fit();
            *postings = kept;
            !postings.bytes.is_empty()
        });
        self.dead = 0;
    }

    fn shrink(&mut self) {
        for postings in self.postings.values_mut() {
            postings.bytes.shrink_to_fit();
        }
        self.postings.shrink_to_fit();
    }

    fn evaluate(&self, need: &Need, case_insensitive: bool) -> Option<Vec<u32>> {
        match need {
            Need::All => None,
            Need::Text(text) => {
                let mut wanted = literal_trigrams(text, case_insensitive);
                if wanted.is_empty() {
                    return None;
                }
                let mut lists: Vec<&Postings> = Vec::with_capacity(wanted.len());
                for trigram in wanted.drain(..) {
                    match self.postings.get(&trigram) {
                        Some(postings) => lists.push(postings),
                        None => return Some(Vec::new()),
                    }
                }
                lists.sort_by_key(|postings| postings.bytes.len());
                let mut found: Vec<u32> = lists[0].ids().collect();
                for postings in &lists[1..] {
                    if found.is_empty() {
                        break;
                    }
                    found = intersect(&found, postings.ids());
                }
                Some(found)
            }
            Need::And(needs) => needs
                .iter()
                .filter_map(|need| self.evaluate(need, case_insensitive))
                .reduce(|found, more| intersect(&found, more.into_iter())),
            Need::Or(needs) => {
                let mut found = Vec::new();
                for need in needs {
                    found = union(&found, &self.evaluate(need, case_insensitive)?);
                }
                Some(found)
            }
        }
    }
}

fn ancestors(relative: &str) -> impl Iterator<Item = &str> {
    std::iter::once(relative).chain(relative.match_indices('/').map(|(at, _)| &relative[..at]))
}

fn intersect(a: &[u32], b: impl Iterator<Item = u32>) -> Vec<u32> {
    let mut found = Vec::new();
    let mut i = 0;
    for id in b {
        while a.get(i).is_some_and(|at| *at < id) {
            i += 1;
        }
        match a.get(i) {
            None => break,
            Some(at) if *at == id => {
                found.push(id);
                i += 1;
            }
            Some(_) => {}
        }
    }
    found
}

fn union(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut found: Vec<u32> = a.iter().chain(b).copied().collect();
    found.sort_unstable();
    found.dedup();
    found
}

fn fold(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{212A}' => bytes.push(b'k'),
            '\u{017F}' => bytes.push(b's'),
            c if c.is_ascii() => bytes.push(c.to_ascii_lowercase() as u8),
            c => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    bytes
}

fn key(window: &[u8]) -> u32 {
    (u32::from(window[0]) << 16) | (u32::from(window[1]) << 8) | u32::from(window[2])
}

pub(crate) fn trigrams(text: &str) -> Vec<u32> {
    let mut found: Vec<u32> = fold(text).windows(3).map(key).collect();
    found.sort_unstable();
    found.dedup();
    found
}

fn literal_trigrams(text: &str, case_insensitive: bool) -> Vec<u32> {
    let mut found: Vec<u32> =
        fold(text).windows(3).filter(|window| !case_insensitive || window.iter().all(u8::is_ascii)).map(key).collect();
    found.sort_unstable();
    found.dedup();
    found
}

pub(crate) fn read_into(index: &Weak<TextIndex>, root: &Path, files: &[String], progress: Option<&Progress>) {
    let pause = progress.and(index.upgrade()).map(|shared| shared.build_pause());
    if let Some(shared) = index.upgrade() {
        shared.state().readers += 1;
        if progress.is_some() {
            shared.total.store(files.len(), Ordering::Relaxed);
        }
    }
    if let Some(progress) = progress {
        progress(IndexProgress { indexed: 0, total: files.len(), done: false });
    }
    let next = AtomicUsize::new(0);
    let read = AtomicUsize::new(0);
    let work = || {
        loop {
            if let Some(pause) = &pause {
                pause.wait();
            }
            let at = next.fetch_add(1, Ordering::Relaxed);
            let Some(relative) = files.get(at) else { return };
            let Some(index) = index.upgrade() else { return };
            let started = index.epoch.load(Ordering::SeqCst);
            #[cfg(not(target_arch = "wasm32"))]
            let began = std::time::Instant::now();
            let found = read_text(&root.join(relative)).map(|text| trigrams(&text));
            index.insert(relative, started, found.as_deref());
            #[cfg(not(target_arch = "wasm32"))]
            index.busy.fetch_add(u64::try_from(began.elapsed().as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
            let count = read.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(progress) = progress {
                index.indexed.store(count, Ordering::Relaxed);
                if count.is_multiple_of(PROGRESS_EVERY) {
                    progress(IndexProgress { indexed: count, total: files.len(), done: false });
                }
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8).min(files.len().max(1));
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(work);
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    work();
    if let Some(index) = index.upgrade() {
        let mut state = index.state();
        state.readers -= 1;
        if state.readers == 0 {
            state.invalidated = HashMap::new();
        }
        if progress.is_some() {
            state.shrink();
            drop(state);
            index.built.store(true, Ordering::Release);
        }
    }
    if let Some(progress) = progress {
        progress(IndexProgress { indexed: read.load(Ordering::Relaxed), total: files.len(), done: true });
    }
}

#[cfg(test)]
pub(crate) fn built(files: &[(&str, &str)]) -> TextIndex {
    let index = TextIndex::default();
    for (relative, text) in files {
        index.insert(relative, 0, Some(&trigrams(text)));
    }
    index.total.store(files.len(), Ordering::Relaxed);
    index.indexed.store(files.len(), Ordering::Relaxed);
    index.built.store(true, Ordering::Release);
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use zen_text::search::{Query, find_all};

    use crate::literals::need;

    fn matching(index: &TextIndex, query: &Query) -> Vec<String> {
        let files: Vec<(PathBuf, String)> =
            ["a.go", "b.go", "c.go", "new.go"].iter().map(|name| (PathBuf::from(name), (*name).to_owned())).collect();
        index
            .candidates(files, &need(query), !query.match_case, &HashMap::new())
            .into_iter()
            .map(|(_, relative)| relative)
            .collect()
    }

    fn query(text: &str, match_case: bool, regex: bool) -> Query {
        Query { text: text.to_owned(), match_case, regex, ..Query::default() }
    }

    #[test]
    fn candidates_are_the_files_with_every_trigram_and_unknown_files() {
        let index = built(&[("a.go", "func Handler() {}"), ("b.go", "type handler struct"), ("c.go", "package c")]);
        assert_eq!(matching(&index, &query("Handler", true, false)), ["a.go", "b.go", "new.go"]);
        assert_eq!(matching(&index, &query("struct", true, false)), ["b.go", "new.go"]);
        assert_eq!(matching(&index, &query("func|package", false, true)), ["a.go", "c.go", "new.go"]);
        assert_eq!(matching(&index, &query("zzz", true, false)), ["new.go"]);
        assert_eq!(matching(&index, &query("ab", true, false)).len(), 4, "too short for a trigram");
    }

    #[test]
    fn changed_files_are_forgotten_and_stale_reads_dropped() {
        let index = built(&[("a.go", "needle"), ("src/x/b.go", "needle"), ("src/c.go", "needle")]);
        index.invalidate(&["a.go".to_owned(), "src".to_owned()]);
        assert_eq!(index.status().files, 0);
        index.insert("a.go", 0, Some(&trigrams("hay")));
        assert_eq!(index.status().files, 0, "read before the change");
        let now = index.epoch.load(Ordering::SeqCst);
        index.insert("src/c.go", now, Some(&trigrams("hay")));
        index.insert("src/d.png", now, None);
        assert_eq!((index.status().files, index.status().skipped), (1, 1));
    }

    #[test]
    fn posting_lists_keep_their_ids_through_compaction() {
        let ids = [0, 1, 127, 128, 300, 16_384, 70_000, u32::MAX - 1];
        let mut postings = Postings::default();
        for id in ids {
            postings.push(id);
        }
        assert_eq!(postings.ids().collect::<Vec<_>>(), ids);
        let files: Vec<(String, String)> = (0..3000)
            .map(|i| (format!("f{i}.go"), format!("{} common", if i % 2 == 0 { "even" } else { "odd" })))
            .collect();
        let pairs: Vec<(&str, &str)> = files.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
        let index = built(&pairs);
        let gone: Vec<String> = (0..2000).map(|i| format!("f{i}.go")).collect();
        index.invalidate(&gone);
        let state = index.state();
        assert_eq!(state.dead, 0, "compacted");
        let even = state.evaluate(&Need::Text("even".to_owned()), false).unwrap();
        assert_eq!(even.len(), 500);
        assert!(even.iter().all(|id| state.live[*id as usize]));
    }

    fn regex_piece() -> impl Strategy<Value = String> {
        const ATOMS: &[&str] = &[
            "a",
            "b",
            "ab",
            "A",
            "K",
            "s",
            "п",
            "П",
            " ",
            "\n",
            ".",
            "\\.",
            "\\n",
            "\\x61",
            "\\x{62}",
            "\\u0430",
            "\\pL",
            "\\p{Lu}",
            "\\d",
            "\\w",
            "\\b",
            "\\B",
            "\\<",
            "\\>",
            "\\b{start}",
            "\\A",
            "\\z",
            "^",
            "$",
            "[ab]",
            "[^a]",
            "[]a]",
            "[a-c&&[^b]]",
            "[[:alpha:]]",
            "[\\]a]",
            "(a|b)",
            "(?:ab|ba)",
            "(a|)",
            "(?P<n>ab)",
            "(?<m>ba)",
            "(?i)",
            "(?i:ab)",
            "(?-i)",
            "(?s:.)",
            "(ab|п)+",
            "\\{",
            "\\*",
            " \\<ab",
            "ab\\> ",
            " \\bab",
            "ab\\b{end} ",
        ];
        const QUANTIFIERS: &[&str] = &["", "", "", "?", "*", "+", "+?", "??", "{2}", "{0,1}", "{1,}", "{0}"];
        (prop::sample::select(ATOMS), prop::sample::select(QUANTIFIERS))
            .prop_map(|(atom, quantifier)| format!("{atom}{quantifier}"))
    }

    fn text() -> impl Strategy<Value = String> {
        const WORDS: &[&str] = &[
            "a", "b", "ab", "ba", "A", "B", "k", "K", "\u{212A}", "s", "S", "\u{17F}", "п", "П", "а", "x", " ", " ",
            "\n", "[", "]", "{", "}", "*", "1", "<", ">",
        ];
        prop::collection::vec(prop::sample::select(WORDS), 0..24).prop_map(|words| words.concat())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn every_matching_file_is_a_candidate(
            texts in prop::collection::vec(text(), 1..8),
            pieces in prop::collection::vec(regex_piece(), 1..6),
            plain in "[abABkKsS\u{17F}\u{212A}пП .]{1,6}",
        ) {
            let named: Vec<(String, &str)> =
                texts.iter().enumerate().map(|(i, text)| (format!("f{i}"), text.as_str())).collect();
            let pairs: Vec<(&str, &str)> = named.iter().map(|(name, text)| (name.as_str(), *text)).collect();
            let index = built(&pairs);
            for (text, regex) in [(pieces.concat(), true), (plain.clone(), false)] {
                for (match_case, whole_words) in [(true, false), (false, false), (true, true), (false, true)] {
                    let query = Query { text: text.clone(), match_case, whole_words, regex, ..Query::default() };
                    if find_all("", &query).is_err() {
                        continue;
                    }
                    let state = index.state();
                    let Some(ids) = state.evaluate(&need(&query), !match_case) else { continue };
                    for (name, text) in &pairs {
                        if !find_all(text, &query).unwrap().is_empty() {
                            prop_assert!(ids.contains(&state.ids[*name]), "{:?} in {:?}", query, text);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_only_non_ascii_letters_matching_ascii_ones_regardless_of_case_are_folded() {
        let every: Vec<char> = (0x80..=0x10FFFF).filter_map(char::from_u32).collect();
        let text: String = every.iter().collect();
        let found = find_all(&text, &query("[a-z]", false, true)).unwrap();
        let letters: Vec<char> = found.iter().map(|range| every[range.start]).collect();
        assert_eq!(letters, ['\u{017F}', '\u{212A}']);
        assert!(letters.iter().all(|c| fold(&c.to_string()).iter().all(u8::is_ascii_lowercase)));
    }
}

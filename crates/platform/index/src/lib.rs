use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
mod git_ignore;
pub mod layout;
mod literals;
mod matcher;
pub mod scope;
mod text_search;
mod trigram;
pub use matcher::{Location, MatchKind, NameMatch, NamePattern, OwnedMatch, Pattern, Rank, split_location};
pub use scope::FileFilter;
pub use text_search::{KeepMatches, Search, SearchMemory, TextMatch};
pub use trigram::{BuildPause, IndexProgress, Progress, TextIndexStatus};

fn skipped_name(name: &str) -> bool {
    zen_vfs::HIDDEN.contains(&name) || zen_vfs::TOOL_FOLDERS.contains(&name)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Indexed {
    pub(crate) relative: String,
    pub(crate) name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub relative: String,
    pub kind: MatchKind,
    pub ranges: Vec<Range<usize>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exclusions {
    pub folders: Vec<PathBuf>,
    pub patterns: Vec<String>,
    pub ignored: Vec<String>,
}

impl Exclusions {
    pub fn new(folders: &[PathBuf], patterns: &[String]) -> Self {
        let patterns = patterns.iter().map(|pattern| pattern.trim().to_owned()).filter(|pattern| !pattern.is_empty());
        Self { folders: folders.to_vec(), patterns: patterns.collect(), ignored: zen_vfs::file_types::ignored() }
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.patterns.is_empty()
    }

    pub fn excludes_name(&self, name: &str) -> bool {
        self.patterns.iter().any(|pattern| text_search::glob(pattern, name))
            || self.ignored.iter().any(|pattern| zen_vfs::file_types::glob_matches(pattern, name, false))
    }

    pub fn excludes_entry(&self, path: &Path) -> bool {
        self.folders.iter().any(|folder| folder == path)
            || path.file_name().is_some_and(|name| self.excludes_name(&name.to_string_lossy()))
    }

    pub fn excludes(&self, root: &Path, path: &Path) -> bool {
        if self.folders.iter().any(|folder| path.starts_with(folder)) {
            return true;
        }
        let relative = path.strip_prefix(root).unwrap_or(path);
        relative.components().any(|part| self.excludes_name(&part.as_os_str().to_string_lossy()))
    }
}

#[derive(Clone, Debug)]
pub struct FileIndex {
    root: PathBuf,
    excluded: Arc<Exclusions>,
    files: Arc<Mutex<Option<Vec<Indexed>>>>,
    text: Arc<trigram::TextIndex>,
    #[cfg(not(target_arch = "wasm32"))]
    git_ignores: Arc<git_ignore::GitIgnores>,
    deferred: Arc<Mutex<Deferred>>,
    pending: Arc<Mutex<Vec<PathBuf>>>,
}

struct Later {
    excluded: Arc<Exclusions>,
    #[cfg(not(target_arch = "wasm32"))]
    git_ignores: Arc<git_ignore::GitIgnores>,
    deferred: Arc<Mutex<Deferred>>,
    pending: Arc<Mutex<Vec<PathBuf>>>,
}

#[derive(Debug, Default)]
struct Deferred {
    paused: bool,
    files: std::collections::BTreeSet<String>,
}

impl FileIndex {
    pub fn new(root: &Path) -> Self {
        Self::with_excluded(root, &[], &[])
    }

    pub fn with_excluded(root: &Path, excluded: &[PathBuf], patterns: &[String]) -> Self {
        Self::with_progress(root, excluded, patterns, None)
    }

    pub fn with_progress(root: &Path, excluded: &[PathBuf], patterns: &[String], progress: Option<Progress>) -> Self {
        let exclusions = Arc::new(Exclusions::new(excluded, patterns));
        let index = Self {
            root: root.to_owned(),
            excluded: Arc::clone(&exclusions),
            files: Arc::default(),
            text: Arc::default(),
            #[cfg(not(target_arch = "wasm32"))]
            git_ignores: Arc::new(git_ignore::GitIgnores::new(root)),
            deferred: Arc::default(),
            pending: Arc::default(),
        };
        let (files, text, root) = (Arc::clone(&index.files), Arc::downgrade(&index.text), root.to_owned());
        let later = Later {
            excluded: Arc::clone(&index.excluded),
            #[cfg(not(target_arch = "wasm32"))]
            git_ignores: Arc::clone(&index.git_ignores),
            deferred: Arc::clone(&index.deferred),
            pending: Arc::clone(&index.pending),
        };
        let build = move || {
            let walked = walk(&root, &root, &exclusions);
            let names: Vec<String> = walked.iter().map(|file| file.relative.clone()).collect();
            if let Some(text) = text.upgrade() {
                let index = FileIndex {
                    root: root.clone(),
                    excluded: Arc::clone(&later.excluded),
                    files: Arc::clone(&files),
                    text,
                    #[cfg(not(target_arch = "wasm32"))]
                    git_ignores: Arc::clone(&later.git_ignores),
                    deferred: Arc::clone(&later.deferred),
                    pending: Arc::clone(&later.pending),
                };
                index.take_walk(walked);
            }
            drop(files);
            let progress = progress.unwrap_or_else(|| Arc::new(|_| {}));
            trigram::read_into(&text, &root, &names, Some(&progress));
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(build);
        #[cfg(target_arch = "wasm32")]
        build();
        index
    }

    fn take_walk(&self, walked: Vec<Indexed>) {
        let pending = match self.files.lock() {
            Ok(mut files) => {
                *files = Some(walked);
                self.pending.lock().map(|mut pending| std::mem::take(&mut *pending)).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        };
        if !pending.is_empty() {
            self.refresh(&pending);
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn exclusions(&self) -> &Exclusions {
        &self.excluded
    }

    pub fn text_status(&self) -> TextIndexStatus {
        self.text.status()
    }

    pub fn build_pause(&self) -> BuildPause {
        self.text.build_pause()
    }

    pub fn busy_time(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.text.busy_nanos())
    }

    pub fn memory(&self) -> usize {
        let names: usize = self
            .files
            .lock()
            .ok()
            .and_then(|files| {
                files
                    .as_ref()
                    .map(|files| files.iter().map(|file| file.relative.capacity() + file.name.capacity() + 48).sum())
            })
            .unwrap_or(0);
        names + self.text.memory()
    }

    pub fn set_paused(&self, paused: bool) {
        let files = {
            let Ok(mut deferred) = self.deferred.lock() else { return };
            if deferred.paused == paused {
                return;
            }
            deferred.paused = paused;
            std::mem::take(&mut deferred.files)
        };
        let files: Vec<String> = files.into_iter().filter(|relative| self.root.join(relative).is_file()).collect();
        if files.is_empty() {
            return;
        }
        let (text, root) = (Arc::downgrade(&self.text), self.root.clone());
        let read = move || trigram::read_into(&text, &root, &files, None);
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(read);
        #[cfg(target_arch = "wasm32")]
        read();
    }

    pub fn is_paused(&self) -> bool {
        self.deferred.lock().is_ok_and(|deferred| deferred.paused)
    }

    pub fn deferred(&self) -> usize {
        self.deferred.lock().map_or(0, |deferred| deferred.files.len())
    }

    pub fn is_ready(&self) -> bool {
        self.files.lock().is_ok_and(|files| files.is_some())
    }

    pub fn len(&self) -> usize {
        self.files.lock().ok().and_then(|files| files.as_ref().map(Vec::len)).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn file_list(&self) -> Vec<(PathBuf, String)> {
        let Ok(guard) = self.files.lock() else { return Vec::new() };
        guard.as_ref().map_or_else(Vec::new, |files| {
            files.iter().map(|file| (self.root.join(&file.relative), file.relative.clone())).collect()
        })
    }

    pub fn git_ignores(&self, path: &Path) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.git_ignores.ignores(path)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            false
        }
    }

    pub fn refresh(&self, paths: &[PathBuf]) {
        if paths.contains(&self.root) {
            let index = self.clone();
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::spawn(move || index.rescan());
            #[cfg(target_arch = "wasm32")]
            index.rescan();
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if paths.iter().any(|path| path.file_name().is_some_and(|name| name == ".gitignore" || name == ".ignore")) {
            self.git_ignores.forget();
        }
        let mut changed = Vec::new();
        let mut added = Vec::new();
        if let Ok(mut guard) = self.files.lock() {
            let mut fresh: Vec<Indexed> = Vec::new();
            for path in paths {
                let Some(relative) = relative(&self.root, path) else { continue };
                if skipped(&relative)
                    || self.git_ignores(path)
                    || self.excluded.excludes(&self.root, path)
                    || zen_vfs::excluded_folders::excludes(&self.root, path)
                {
                    continue;
                }
                changed.push(relative.clone());
                if guard.is_none() {
                    if let Ok(mut pending) = self.pending.lock() {
                        pending.push(path.clone());
                    }
                    continue;
                }
                if path.is_file() {
                    fresh.push(Indexed { name: file_name(&relative), relative });
                } else if path.is_dir() {
                    fresh.extend(walk(&self.root, path, &self.excluded));
                }
            }
            if let Some(files) = guard.as_mut() {
                let gone: HashSet<&str> = changed.iter().map(String::as_str).collect();
                files.retain(|file| !in_any(&gone, &file.relative));
                let mut seen = HashSet::new();
                fresh.retain(|file| seen.insert(file.relative.clone()));
                added.extend(fresh.iter().map(|file| file.relative.clone()));
                files.extend(fresh);
            }
        }
        if changed.is_empty() {
            return;
        }
        self.text.invalidate(&changed);
        if added.is_empty() {
            return;
        }
        if let Ok(mut deferred) = self.deferred.lock()
            && deferred.paused
        {
            deferred.files.extend(added);
            return;
        }
        let (text, root) = (Arc::downgrade(&self.text), self.root.clone());
        let read = move || trigram::read_into(&text, &root, &added, None);
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(read);
        #[cfg(target_arch = "wasm32")]
        read();
    }

    pub fn rescan(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.git_ignores.forget();
        let walked = walk(&self.root, &self.root, &self.excluded);
        let names: Vec<String> = walked.iter().map(|file| file.relative.clone()).collect();
        let Ok(mut files) = self.files.lock() else { return };
        let Some(current) = files.as_mut() else { return };
        let mut changed: Vec<String> = current.iter().map(|file| file.relative.clone()).collect();
        changed.extend(names.iter().cloned());
        changed.sort_unstable();
        changed.dedup();
        *current = walked;
        drop(files);
        self.text.invalidate(&changed);
        trigram::read_into(&Arc::downgrade(&self.text), &self.root, &names, None);
    }

    pub fn search(&self, query: &str, limit: usize, recent: &[PathBuf]) -> Vec<Found> {
        let Ok(guard) = self.files.lock() else { return Vec::new() };
        let Some(files) = guard.as_ref() else { return Vec::new() };
        let pattern = Pattern::path(query);
        let recent: Vec<String> = recent.iter().filter_map(|path| relative(&self.root, path)).collect();
        let place = |file: &Indexed| recent.iter().position(|other| *other == file.relative);
        let mut found: Vec<(Rank, &Indexed)> = files
            .iter()
            .filter(|file| pattern.name().may_match(&file.name))
            .filter_map(|file| {
                let matched = pattern.path_match(&file.relative, false)?;
                let mut rank = Rank::new(&matched, file.name.chars().count());
                rank.recent = place(file);
                if pattern.is_empty() {
                    rank.length = file.relative.len();
                    rank.degree = -(rank.recent.unwrap_or(usize::MAX).min(1 << 20) as i64);
                }
                Some((rank, file))
            })
            .collect();
        let order = |a: &(Rank, &Indexed), b: &(Rank, &Indexed)| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.relative.len().cmp(&b.1.relative.len()))
                .then_with(|| a.1.relative.cmp(&b.1.relative))
        };
        if found.len() > limit && limit > 0 {
            found.select_nth_unstable_by(limit - 1, order);
        }
        found.truncate(limit);
        found.sort_by(order);
        found
            .into_iter()
            .filter_map(|(_, file)| {
                let matched = pattern.matches_path(&file.relative)?;
                Some(Found {
                    path: self.root.join(&file.relative),
                    relative: file.relative.clone(),
                    kind: matched.kind,
                    ranges: matched.ranges,
                })
            })
            .collect()
    }
}

pub fn list_files(folder: &Path) -> Vec<(PathBuf, String)> {
    walk(folder, folder, &Exclusions::new(&[], &[]))
        .into_iter()
        .map(|file| (folder.join(&file.relative), file.relative))
        .collect()
}

pub(crate) fn file_name(relative: &str) -> String {
    relative.rsplit('/').next().unwrap_or(relative).to_owned()
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> =
        relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn skipped(relative: &str) -> bool {
    relative.split('/').any(skipped_name)
}

fn in_any(folders: &HashSet<&str>, relative: &str) -> bool {
    folders.contains(relative) || relative.match_indices('/').any(|(at, _)| folders.contains(&relative[..at]))
}

#[cfg(not(target_arch = "wasm32"))]
fn walk(root: &Path, folder: &Path, excluded: &Exclusions) -> Vec<Indexed> {
    let excluded = excluded.clone();
    let walker = ignore::WalkBuilder::new(folder)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .filter_entry(move |entry| {
            let name = entry.file_name().to_string_lossy();
            let folder = entry.file_type().is_some_and(|kind| kind.is_dir());
            !skipped_name(&name)
                && !excluded.excludes_entry(entry.path())
                && !(folder && zen_vfs::excluded_folders::is_excluded(entry.path()))
        })
        .build();
    walker
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .filter_map(|entry| relative(root, entry.path()))
        .map(|relative| Indexed { name: file_name(&relative), relative })
        .collect()
}

#[cfg(target_arch = "wasm32")]
fn walk(_root: &Path, _folder: &Path, _excluded: &Exclusions) -> Vec<Indexed> {
    Vec::new()
}

pub fn score(query: &str, name: &str, path: &str) -> Option<i64> {
    let pattern = Pattern::path(query);
    let found = if path.ends_with(name) { pattern.matches_path(path)? } else { pattern.name().matches_file(name)? };
    Some(found.kind as i64 * 1_000_000 + found.degree.clamp(-999, 999) * 1_000 - name.chars().count().min(999) as i64)
}

pub fn match_positions(query: &str, text: &str) -> Option<Vec<usize>> {
    let query: String = query.chars().filter(|c| !c.is_whitespace()).collect();
    subsequence(&query, text).map(|(_, positions)| positions)
}

fn subsequence(query: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    let text: Vec<char> = text.chars().collect();
    let query: Vec<char> = query.chars().collect();
    let word_start = |i: usize| {
        i == 0
            || matches!(text[i - 1], '_' | '-' | '.' | ' ' | '/')
            || (text[i].is_uppercase() && text[i - 1].is_lowercase())
    };
    let mut score = 0;
    let mut position = 0;
    let mut previous: Option<usize> = None;
    let mut positions = Vec::with_capacity(query.len());
    for q in query {
        let q = q.to_lowercase().next().unwrap_or(q);
        let ahead = (position..text.len()).filter(|&i| text[i].to_lowercase().next() == Some(q));
        let candidates: Vec<usize> = ahead.collect();
        let at = candidates
            .iter()
            .copied()
            .find(|&i| previous.is_some_and(|p| i == p + 1))
            .or_else(|| candidates.iter().copied().find(|&i| word_start(i)))
            .or_else(|| candidates.first().copied())?;
        score += 10;
        if previous.is_some_and(|p| at == p + 1) {
            score += 15;
        }
        if word_start(at) {
            score += 20;
        }
        previous = Some(at);
        positions.push(at);
        position = at + 1;
    }
    Some((score, positions))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(query: &str, files: &[&str]) -> Vec<String> {
        let mut scored: Vec<(i64, &str)> = files
            .iter()
            .filter_map(|path| score(query, path.rsplit('/').next().unwrap(), path).map(|score| (score, *path)))
            .collect();
        scored.sort_by_key(|item| std::cmp::Reverse(item.0));
        scored.into_iter().map(|(_, path)| path.to_owned()).collect()
    }

    #[test]
    fn exact_and_prefix_names_come_first() {
        let files =
            ["internal/cart/cart.go", "internal/cart/cart_test.go", "cmd/main.go", "docs/carting.md", "a/b/scart.go"];
        let ranked = rank("cart", &files);
        assert_eq!(ranked[0], "internal/cart/cart.go");
        assert_eq!(ranked[1], "internal/cart/cart_test.go");
        assert!(!ranked.contains(&"cmd/main.go".to_owned()));
    }

    #[test]
    fn camel_humps_and_word_starts() {
        let files = ["src/EditorView.kt", "src/everything.kt", "src/editor_view.rs"];
        let ranked = rank("ev", &files);
        assert_ne!(ranked.last().map(String::as_str), Some("src/EditorView.kt"));
        assert!(score("edvi", "editor_view.rs", "src/editor_view.rs").is_some());
        assert!(score("xyz", "editor_view.rs", "src/editor_view.rs").is_none());
    }

    #[test]
    fn match_positions_are_the_letters_score_matched() {
        assert_eq!(match_positions("edvi", "editor_view.rs"), Some(vec![0, 1, 7, 8]));
        assert_eq!(match_positions("SB", "ServerBuilder"), Some(vec![0, 6]), "camel humps");
        assert_eq!(match_positions("ини", "Инициализация"), Some(vec![0, 1, 2]), "any script, any case");
        assert_eq!(match_positions("", "main"), Some(Vec::new()));
        assert_eq!(match_positions("xyz", "main"), None);
    }

    #[test]
    fn paths_match_segment_by_segment() {
        assert!(score("ui/mod", "mod.rs", "crates/platform/ui/src/ide/mod.rs").is_some());
        assert!(score("ui/mod", "mod.rs", "crates/platform/core/src/mod.rs").is_none());
        assert!(score("ide/view", "editor_view.rs", "crates/platform/ui/src/ide/editor_view.rs").is_some());
    }

    #[test]
    fn indexes_respects_gitignore_and_follows_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for folder in ["node_modules/left-pad", "gen", "vendor/example.com/m", "build", "dist"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        for file in
            ["src/main.go", "node_modules/left-pad/index.js", "gen/x.go", "vendor/example.com/m/m.go", "build/b.txt"]
        {
            std::fs::write(root.join(file), "").unwrap();
        }
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::write(root.join(".gitignore"), "gen/\n").unwrap();
        let index = FileIndex::new(root);
        while !index.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let names: Vec<String> = index.search("", 100, &[]).into_iter().map(|found| found.relative).collect();
        assert!(names.contains(&"src/main.go".to_owned()));
        assert!(names.contains(&".gitignore".to_owned()));
        assert!(!names.iter().any(|name| name.starts_with("node_modules") || name.starts_with("gen")), "{names:?}");
        assert!(names.contains(&"vendor/example.com/m/m.go".to_owned()), "vendor is not excluded by its name");
        assert!(names.contains(&"build/b.txt".to_owned()), "neither is build: {names:?}");
        std::fs::write(root.join("src/util.go"), "").unwrap();
        std::fs::write(root.join("node_modules/left-pad/new.js"), "").unwrap();
        std::fs::write(root.join("dist/d.txt"), "").unwrap();
        std::fs::remove_file(root.join("src/main.go")).unwrap();
        index.refresh(&[
            root.join("src/util.go"),
            root.join("src/main.go"),
            root.join("node_modules/left-pad/new.js"),
            root.join("dist/d.txt"),
        ]);
        assert!(index.search("new.js", 10, &[]).is_empty(), "a change in node_modules stays out");
        assert_eq!(index.search("d.txt", 10, &[])[0].relative, "dist/d.txt");
        let found = index.search("util", 10, &[]);
        assert_eq!(found[0].relative, "src/util.go");
        assert!(index.search("main", 10, &[]).is_empty());
    }

    #[test]
    fn version_control_internals_stay_out_when_the_user_stops_ignoring_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for folder in [".git/objects/pack", ".hg/store", "src/__pycache__"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        for file in [".git/objects/pack/a.pack", ".git/HEAD", ".hg/store/x", "src/main.py", "src/__pycache__/main.pyc"]
        {
            std::fs::write(root.join(file), "").unwrap();
        }
        let nothing_ignored = Exclusions::default();
        let names: Vec<String> = walk(root, root, &nothing_ignored).into_iter().map(|file| file.relative).collect();
        assert!(names.contains(&"src/__pycache__/main.pyc".to_owned()), "{names:?}");
        assert!(!names.iter().any(|name| name.starts_with(".git/") || name.starts_with(".hg/")), "{names:?}");
        assert!(skipped(".git/objects/pack/a.pack"));
        let ignored = Exclusions { ignored: vec!["__pycache__".to_owned()], ..Exclusions::default() };
        let names: Vec<String> = walk(root, root, &ignored).into_iter().map(|file| file.relative).collect();
        assert_eq!(names, ["src/main.py"]);
    }

    #[test]
    fn the_projects_exclusions_are_left_out_and_stay_out_after_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for folder in ["src", "out/classes", "tools/bin", "docs/tmp.cache"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        for file in
            ["src/main.go", "src/run.log", "out/classes/a.go", "tools/bin/tool", "tools/gen.go", "docs/tmp.cache/x"]
        {
            std::fs::write(root.join(file), "").unwrap();
        }
        let excluded = [root.join("out"), root.join("tools/bin")];
        let patterns = ["*.log".to_owned(), " *.cache ".to_owned(), String::new()];
        let index = FileIndex::with_excluded(root, &excluded, &patterns);
        assert_eq!(index.exclusions().patterns, ["*.log", "*.cache"]);
        while !index.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut names: Vec<String> = index.file_list().into_iter().map(|(_, relative)| relative).collect();
        names.sort();
        assert_eq!(names, ["src/main.go", "tools/gen.go"]);
        std::fs::write(root.join("out/new.go"), "").unwrap();
        std::fs::write(root.join("src/more.log"), "").unwrap();
        std::fs::write(root.join("src/more.go"), "").unwrap();
        index.refresh(&[
            root.join("out/new.go"),
            root.join("src/more.log"),
            root.join("src/more.go"),
            root.join("tools"),
        ]);
        let mut names: Vec<String> = index.file_list().into_iter().map(|(_, relative)| relative).collect();
        names.sort();
        assert_eq!(names, ["src/main.go", "src/more.go", "tools/gen.go"]);
    }

    #[test]
    fn exclusions_match_folders_and_names() {
        let exclusions = Exclusions::new(&[PathBuf::from("/p/dist")], &["*.tmp".to_owned(), "cache?".to_owned()]);
        assert!(!exclusions.is_empty());
        assert!(Exclusions::default().is_empty());
        assert!(exclusions.excludes(Path::new("/p"), Path::new("/p/dist/app.js")));
        assert!(exclusions.excludes(Path::new("/p"), Path::new("/p/src/a.tmp")));
        assert!(exclusions.excludes(Path::new("/p"), Path::new("/p/cache1/a.go")));
        assert!(!exclusions.excludes(Path::new("/p"), Path::new("/p/distant/a.go")));
        assert!(!exclusions.excludes(Path::new("/cache1"), Path::new("/cache1/a.go")));
        assert!(exclusions.excludes_entry(Path::new("/p/dist")));
        assert!(!exclusions.excludes_entry(Path::new("/p/dist/sub")));
        assert!(exclusions.excludes_entry(Path::new("/p/x/b.tmp")));
    }

    #[test]
    fn a_refresh_of_the_project_folder_finds_what_changed_when_events_were_lost() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "needle one").unwrap();
        let index = FileIndex::new(dir.path());
        while !index.text_status().ready {
            std::thread::yield_now();
        }
        std::fs::remove_file(dir.path().join("old.txt")).unwrap();
        std::fs::write(dir.path().join("new.txt"), "needle two").unwrap();

        index.refresh(&[dir.path().to_owned()]);

        let start = std::time::Instant::now();
        while index.search("new", 10, &[]).is_empty() {
            assert!(start.elapsed() < std::time::Duration::from_secs(20), "the rescan never ended");
            std::thread::yield_now();
        }
        assert!(index.search("old", 10, &[]).is_empty());
    }

    #[test]
    fn a_rescan_finds_what_changed_without_a_refresh_and_forgets_what_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "needle one").unwrap();
        let index = FileIndex::new(dir.path());
        while !index.text_status().ready {
            std::thread::yield_now();
        }
        std::fs::remove_file(dir.path().join("old.txt")).unwrap();
        std::fs::write(dir.path().join("new.txt"), "needle two").unwrap();
        assert_eq!(index.len(), 1, "nothing told the index");

        index.rescan();

        assert_eq!(index.len(), 1);
        assert_eq!(index.search("new", 10, &[]).len(), 1);
        assert!(index.search("old", 10, &[]).is_empty());
        assert_eq!(index.text_status().files, 1);
    }

    #[test]
    fn a_file_refreshed_while_the_first_walk_runs_is_indexed_once_it_ends() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "alpha\n").unwrap();
        let excluded = Arc::new(Exclusions::default());
        let index = FileIndex {
            root: dir.path().to_owned(),
            excluded: Arc::clone(&excluded),
            files: Arc::default(),
            text: Arc::default(),
            git_ignores: Arc::new(git_ignore::GitIgnores::new(dir.path())),
            deferred: Arc::default(),
            pending: Arc::default(),
        };
        let walked = walk(dir.path(), dir.path(), &excluded);
        let late = dir.path().join("b.txt");
        std::fs::write(&late, "bravo\n").unwrap();
        index.refresh(std::slice::from_ref(&late));
        assert!(!index.is_ready());
        index.take_walk(walked);
        let names: Vec<String> = index.file_list().into_iter().map(|(_, relative)| relative).collect();
        assert!(names.contains(&"a.txt".to_owned()) && names.contains(&"b.txt".to_owned()), "{names:?}");
        assert!(index.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn changes_git_ignores_stay_out_and_many_changes_are_taken_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        std::fs::write(root.join(".gitignore"), "/target\n*.o\n").unwrap();
        let mut paths = Vec::new();
        for i in 0..20_000 {
            let folder = root.join(format!("src/m{}", i % 100));
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join(format!("f{i}.rs"));
            std::fs::write(&path, "").unwrap();
            paths.push(path);
        }
        let index = FileIndex::new(root);
        while !index.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(index.len(), 20_002);
        let mut built = Vec::new();
        for i in 0..20_000 {
            let folder = root.join(format!("target/debug/deps/c{}", i % 100));
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join(format!("lib{i}.rlib"));
            std::fs::write(&path, "").unwrap();
            built.push(path);
        }
        std::fs::write(root.join("src/m0/x.o"), "").unwrap();
        built.push(root.join("target"));
        built.push(root.join("src/m0/x.o"));
        index.refresh(&built);
        assert_eq!(index.len(), 20_002, "a build in the ignored target adds nothing");
        assert!(index.search("lib7.rlib", 10, &[]).is_empty());

        let started = std::time::Instant::now();
        index.refresh(&paths);
        let limit = if cfg!(debug_assertions) { 10_000 } else { 1_000 };
        if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
            assert!(started.elapsed().as_millis() < limit, "{:?}", started.elapsed());
        }
        assert_eq!(index.len(), 20_002, "the changed files are there once each");
        std::fs::remove_dir_all(root.join("src/m1")).unwrap();
        std::fs::create_dir_all(root.join("src/m1/new")).unwrap();
        std::fs::write(root.join("src/m1/new/n.rs"), "").unwrap();
        index.refresh(&[root.join("src/m1"), root.join("src/m1/new/n.rs")]);
        assert_eq!(index.len(), 20_002 - 200 + 1);
        assert_eq!(index.search("n.rs", 10, &[])[0].relative, "src/m1/new/n.rs");

        std::fs::write(root.join(".gitignore"), "*.o\n").unwrap();
        index.refresh(&[root.join(".gitignore"), root.join("target")]);
        assert_eq!(
            index.search("lib7.rlib", 10, &[])[0].relative,
            "target/debug/deps/c7/lib7.rlib",
            "not ignored any more"
        );
    }

    fn a_hundred_thousand_files() -> FileIndex {
        let files: Vec<Indexed> = (0..100_000)
            .map(|i| {
                let relative = format!("pkg{}/sub{}/file_{i}_handler.go", i % 300, i % 17);
                Indexed { name: file_name(&relative), relative }
            })
            .collect();
        FileIndex {
            root: PathBuf::from("/p"),
            excluded: Arc::default(),
            files: Arc::new(Mutex::new(Some(files))),
            text: Arc::default(),
            git_ignores: Arc::new(git_ignore::GitIgnores::new(Path::new("/p"))),
            deferred: Arc::default(),
            pending: Arc::default(),
        }
    }

    #[test]
    fn a_hundred_thousand_files_are_searched() {
        let index = a_hundred_thousand_files();
        for query in ["f99h", "fh", "pkg1/handler", "*99*go"] {
            let found = index.search(query, 50, &[]);
            assert!(!found.is_empty(), "{query}");
            assert!(found.iter().all(|found| !found.ranges.is_empty()), "{query}");
        }
    }

    #[test]
    #[ignore = "a timing check; run by hand on a release build"]
    fn a_hundred_thousand_files_answer_within_fifty_milliseconds() {
        let index = a_hundred_thousand_files();
        let limit = if cfg!(debug_assertions) { 2_000 } else { 50 };
        for query in ["f99h", "fh", "pkg1/handler", "*99*go"] {
            let spent = (0..3)
                .map(|_| {
                    let started = std::time::Instant::now();
                    index.search(query, 50, &[]);
                    started.elapsed()
                })
                .min()
                .unwrap_or_default();
            assert!(spent.as_millis() < limit, "{query}: {spent:?}");
        }
    }

    #[test]
    fn a_paused_build_keeps_what_it_has_and_goes_on_when_resumed() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..4 {
            std::fs::write(dir.path().join(format!("f{index}.txt")), "hello world\n").unwrap();
        }
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (tell, finished) = std::sync::mpsc::channel();
        let wait = Mutex::new(wait);
        let progress: Progress = Arc::new(move |step: IndexProgress| {
            if !step.done && step.indexed == 0 {
                let _ = wait.lock().unwrap().recv();
            }
            if step.done {
                let _ = tell.send(step.indexed);
            }
        });
        let index = FileIndex::with_progress(dir.path(), &[], &[], Some(progress));
        let pause = index.build_pause();
        pause.set(true);
        go.send(()).unwrap();
        while !index.is_ready() {
            std::thread::yield_now();
        }
        let status = index.text_status();
        assert!(status.paused && !status.ready);
        assert_eq!((status.indexed, status.files), (0, 0), "nothing is read while paused");
        assert_eq!(index.len(), 4, "the names listed stay usable");
        assert!(finished.try_recv().is_err());
        pause.set(false);
        assert_eq!(finished.recv().unwrap(), 4);
        assert!(index.text_status().ready && !index.text_status().paused);
    }

    #[test]
    fn a_paused_build_ends_when_its_index_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (tell, finished) = std::sync::mpsc::channel();
        let wait = Mutex::new(wait);
        let progress: Progress = Arc::new(move |step: IndexProgress| {
            if !step.done {
                let _ = wait.lock().unwrap().recv();
            } else {
                let _ = tell.send(());
            }
        });
        let index = FileIndex::with_progress(dir.path(), &[], &[], Some(progress));
        index.build_pause().set(true);
        drop(index);
        go.send(()).unwrap();
        finished.recv().unwrap();
    }
}

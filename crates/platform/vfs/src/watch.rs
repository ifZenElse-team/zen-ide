use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher as _};

pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
    seen: Arc<Mutex<Seen>>,
}

#[derive(Debug, Default)]
struct Seen {
    changes: Vec<PathBuf>,
    missed: bool,
}

struct Filter {
    root: PathBuf,
    real_root: PathBuf,
}

impl Filter {
    fn add(&self, event: notify::Result<notify::Event>, seen: &mut Seen) -> bool {
        let Ok(event) = event else {
            seen.missed = true;
            return true;
        };
        let missed = event.need_rescan();
        seen.missed |= missed;
        if matches!(event.kind, EventKind::Access(_)) {
            return missed;
        }
        let before = seen.changes.len();
        let paths = event.paths.into_iter().map(|path| respell(&self.root, &self.real_root, path));
        seen.changes.extend(paths.filter(|path| relevant(&self.root, path)));
        missed || seen.changes.len() > before
    }
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watcher").finish_non_exhaustive()
    }
}

impl Watcher {
    pub fn new(root: &Path, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, notify::Error> {
        let seen: Arc<Mutex<Seen>> = Arc::default();
        let sink = Arc::clone(&seen);
        let filter = Filter {
            root: root.to_owned(),
            real_root: std::fs::canonicalize(root).unwrap_or_else(|_| root.to_owned()),
        };
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if sink.lock().is_ok_and(|mut seen| filter.add(event, &mut seen)) {
                wake();
            }
        })?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher, seen })
    }

    pub fn take_changes(&self) -> Vec<PathBuf> {
        let mut changes = self.seen.lock().map(|mut seen| std::mem::take(&mut seen.changes)).unwrap_or_default();
        changes.sort();
        changes.dedup();
        changes
    }

    pub fn take_missed(&self) -> bool {
        self.seen.lock().is_ok_and(|mut seen| std::mem::take(&mut seen.missed))
    }
}

#[derive(Debug, Default)]
pub struct WatchRetry {
    failed: Option<(PathBuf, Instant)>,
}

impl WatchRetry {
    const WAIT: Duration = Duration::from_secs(30);

    pub fn due(&self, root: &Path, now: Instant) -> bool {
        self.failed.as_ref().is_none_or(|(failed, at)| failed != root || now.duration_since(*at) >= Self::WAIT)
    }

    pub fn failed(&mut self, root: &Path, now: Instant) -> bool {
        let first = self.failed.as_ref().is_none_or(|(failed, _)| failed != root);
        self.failed = Some((root.to_owned(), now));
        first
    }
}

fn respell(root: &Path, real_root: &Path, path: PathBuf) -> PathBuf {
    if path.starts_with(root) {
        return path;
    }
    match path.strip_prefix(real_root) {
        Ok(rest) => root.join(rest),
        Err(_) => path,
    }
}

fn relevant(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else { return true };
    let mut parts = relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        if crate::HIDDEN.contains(&part.as_str()) || crate::is_ignored(&part) {
            return false;
        }
        if last && part.starts_with('.') && part.ends_with(".zen-save") {
            return false;
        }
    }
    let folder = path.parent().unwrap_or(path);
    !crate::excluded_folders::excludes(root, folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(watcher: &Watcher, wanted: impl Fn(&[PathBuf]) -> bool) -> Vec<PathBuf> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            seen.extend(watcher.take_changes());
            if wanted(&seen) {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        seen
    }

    #[test]
    fn a_watcher_that_cannot_start_is_tried_again_after_a_while_and_reported_once() {
        let (project, other) = (Path::new("/p"), Path::new("/q"));
        let start = Instant::now();
        let mut retry = WatchRetry::default();
        assert!(retry.due(project, start));
        assert!(retry.failed(project, start), "the first failure is reported");
        assert!(!retry.due(project, start + Duration::from_millis(16)), "not on the next frame");
        assert!(retry.due(other, start), "another project starts at once");
        let later = start + WatchRetry::WAIT;
        assert!(retry.due(project, later));
        assert!(!retry.failed(project, later), "reported already");
        assert!(!retry.due(project, later + Duration::from_secs(1)));
    }

    #[test]
    fn events_the_system_dropped_ask_for_a_look_at_everything() {
        let filter = Filter { root: PathBuf::from("/p"), real_root: PathBuf::from("/p") };
        let mut seen = Seen::default();
        let overflow = notify::Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert!(filter.add(Ok(overflow), &mut seen), "wakes the window");
        assert!(seen.missed && seen.changes.is_empty());

        let mut seen = Seen::default();
        let inside_git = notify::Event::new(EventKind::Other)
            .set_flag(notify::event::Flag::Rescan)
            .add_path(PathBuf::from("/p/.git/refs"));
        assert!(filter.add(Ok(inside_git), &mut seen));
        assert!(seen.missed && seen.changes.is_empty(), "whatever folder the notice names");

        let mut seen = Seen::default();
        assert!(filter.add(Err(notify::Error::generic("the queue overflowed")), &mut seen));
        assert!(seen.missed, "a failing watcher may have missed changes too");

        let mut seen = Seen::default();
        let plain = notify::Event::new(EventKind::Any).add_path(PathBuf::from("/p/main.go"));
        assert!(filter.add(Ok(plain), &mut seen));
        assert!(!seen.missed && seen.changes == [PathBuf::from("/p/main.go")]);
        let unseen = notify::Event::new(EventKind::Any).add_path(PathBuf::from("/p/.git/index"));
        assert!(!filter.add(Ok(unseen), &mut Seen::default()), "nothing to wake for");
    }

    #[test]
    fn sees_files_created_changed_renamed_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let watcher = Watcher::new(&root, || {}).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let file = root.join("main.go");
        std::fs::write(&file, "package main").unwrap();
        let seen = wait_for(&watcher, |seen| seen.contains(&file));
        assert!(seen.contains(&file), "created: {seen:?}");
        let renamed = root.join("app.go");
        std::fs::rename(&file, &renamed).unwrap();
        let seen = wait_for(&watcher, |seen| seen.contains(&renamed));
        assert!(seen.contains(&renamed), "renamed: {seen:?}");
        std::fs::remove_file(&renamed).unwrap();
        let seen = wait_for(&watcher, |seen| seen.contains(&renamed));
        assert!(seen.contains(&renamed), "removed: {seen:?}");
    }

    #[cfg(unix)]
    #[test]
    fn reports_changes_in_a_project_opened_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap().join("real");
        std::fs::create_dir_all(real.join(".git")).unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join("link")).unwrap();
        let root = dir.path().join("link");
        let watcher = Watcher::new(&root, || {}).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(real.join(".git/index"), "x").unwrap();
        std::fs::write(real.join("main.go"), "package main").unwrap();
        let file = root.join("main.go");
        let seen = wait_for(&watcher, |seen| seen.contains(&file));
        assert!(seen.contains(&file), "spelled from the link: {seen:?}");
        let inside = |path: &PathBuf| path.starts_with(&root) && !path.starts_with(root.join(".git"));
        assert!(seen.iter().all(inside), "{seen:?}");
        assert_eq!(respell(&root, &real, real.join("a/b.go")), root.join("a/b.go"));
        assert_eq!(respell(&root, &real, PathBuf::from("/elsewhere")), Path::new("/elsewhere"));
    }

    #[test]
    fn leaves_out_git_excluded_folders_and_own_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/x")).unwrap();
        std::fs::create_dir_all(dir.path().join("vendor/x")).unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(relevant(dir.path(), &dir.path().join("node_modules")), "the excluded folder itself shows in the tree");
        assert!(!relevant(dir.path(), &dir.path().join("node_modules/x/index.js")));
        assert!(relevant(dir.path(), &dir.path().join("vendor/x/x.go")), "vendor is not excluded by its name");
        let root = Path::new("/p");
        assert!(relevant(root, Path::new("/p/main.go")));
        assert!(relevant(root, Path::new("/p/target/debug/x")), "no Cargo.toml beside it");
        assert!(!relevant(root, Path::new("/p/.git/index")));
        assert!(!relevant(root, Path::new("/p/src/.main.go.zen-save")));
        assert!(relevant(root, Path::new("/p/.gitignore")));
        assert!(!relevant(root, Path::new("/p/src/__pycache__/a.cpython-312.pyc")), "ignored names");
        assert!(!relevant(root, Path::new("/p/.hg/store/data")));
    }
}

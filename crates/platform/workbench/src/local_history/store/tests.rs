use super::*;

const DAY: u64 = 24 * 3600 * 1000;

fn store(folder: &Path) -> Store {
    Store::open(folder, Limits::default())
}

fn objects(folder: &Path) -> usize {
    std::fs::read_dir(folder.join("objects"))
        .map(|prefixes| {
            prefixes.flatten().map(|prefix| std::fs::read_dir(prefix.path()).map_or(0, Iterator::count)).sum()
        })
        .unwrap_or(0)
}

#[test]
fn identical_contents_are_kept_once() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/projects/app");
    let mut store = store(data.path());
    assert_eq!(store.record(root, "a.go", VersionKind::Save, None, Some(b"one"), 1, true).unwrap(), Some(1));
    assert_eq!(store.record(root, "a.go", VersionKind::Save, None, Some(b"one"), 2, true).unwrap(), None);
    assert_eq!(store.record(root, "b.go", VersionKind::Save, None, Some(b"one"), 3, true).unwrap(), Some(2));
    assert_eq!(store.record(root, "a.go", VersionKind::Save, None, Some(b"two"), 4, true).unwrap(), Some(3));
    assert_eq!(store.record(root, "a.go", VersionKind::Save, None, Some(b"one"), 5, true).unwrap(), Some(4));
    assert_eq!(store.entries(root).len(), 4);
    assert_eq!(objects(data.path()), 2, "one content for each text, whatever the file");
    let before = store.record(root, "a.go", VersionKind::Before, Some("rename".into()), Some(b"one"), 6, false);
    assert_eq!(before.unwrap(), Some(5), "an action's marker is kept even without a change");
    assert_eq!(store.latest(root, "a.go").unwrap().kind, VersionKind::Before);
}

#[test]
fn contents_are_compressed_when_smaller_and_read_back() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let mut store = store(data.path());
    let text = "fn main() {}\n".repeat(400);
    store.record(root, "big.rs", VersionKind::Save, None, Some(text.as_bytes()), 1, true).unwrap();
    store.record(root, "tiny.rs", VersionKind::Save, None, Some(b"x"), 1, true).unwrap();
    let big = store.latest(root, "big.rs").unwrap().hash.clone().unwrap();
    let tiny = store.latest(root, "tiny.rs").unwrap().hash.clone().unwrap();
    assert_eq!(store.content(&big).unwrap(), text.as_bytes());
    assert_eq!(store.content(&tiny).unwrap(), b"x");
    let stored = std::fs::read(data.path().join("objects").join(&big[..2]).join(&big[2..])).unwrap();
    assert_eq!(stored[0], b'z');
    assert!(stored.len() < text.len() / 10, "{} bytes stored", stored.len());
    let stored = std::fs::read(data.path().join("objects").join(&tiny[..2]).join(&tiny[2..])).unwrap();
    assert_eq!(stored, b"rx", "a content that does not shrink is kept as it is");
    assert!(store.content("../../etc").is_err());
}

#[test]
fn binary_contents_are_marked() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let mut store = store(data.path());
    store.record(root, "logo.png", VersionKind::Save, None, Some(&[0x89, b'P', 0, 1, 2]), 1, true).unwrap();
    store.record(root, "a.txt", VersionKind::Save, None, Some("привет".as_bytes()), 1, true).unwrap();
    assert!(store.latest(root, "logo.png").unwrap().binary);
    assert!(!store.latest(root, "a.txt").unwrap().binary);
}

#[test]
fn deletions_and_labels_have_no_content() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let mut store = store(data.path());
    assert_eq!(store.record(root, "a.go", VersionKind::Deleted, None, None, 1, true).unwrap(), None, "unknown file");
    store.record(root, "a.go", VersionKind::Save, None, Some(b"a"), 2, true).unwrap();
    assert!(store.record(root, "a.go", VersionKind::Deleted, None, None, 3, true).unwrap().is_some());
    assert_eq!(store.record(root, "a.go", VersionKind::Deleted, None, None, 4, true).unwrap(), None, "deleted once");
    let label = store.label(root, None, "before refactoring", 5).unwrap();
    let entry = store.entries(root).iter().find(|entry| entry.id == label).unwrap();
    assert_eq!(
        (entry.kind, entry.path.as_deref(), entry.label.as_deref()),
        (VersionKind::Label, None, Some("before refactoring"))
    );
    assert_eq!(store.latest(root, "a.go").unwrap().kind, VersionKind::Deleted, "labels are not versions");
}

#[test]
fn the_history_survives_a_restart() {
    let data = tempfile::tempdir().unwrap();
    let (one, two) = (Path::new("/p/one"), Path::new("/p/two"));
    {
        let mut store = store(data.path());
        store.record(one, "main.go", VersionKind::Save, None, Some(b"package main\n"), 10, true).unwrap();
        store.record(two, "lib.rs", VersionKind::External, None, Some(b"fn f() {}\n"), 11, true).unwrap();
        store.label(one, Some("main.go"), "ok", 12).unwrap();
    }
    let mut store = store(data.path());
    assert_eq!(store.projects().collect::<Vec<_>>(), vec![one, two]);
    assert_eq!(store.entries(one).len(), 2);
    let hash = store.latest(one, "main.go").unwrap().hash.clone().unwrap();
    assert_eq!(store.content(&hash).unwrap(), b"package main\n");
    assert_eq!(store.record(one, "main.go", VersionKind::Save, None, Some(b"package main\n"), 13, true).unwrap(), None);
    assert_eq!(store.record(one, "main.go", VersionKind::Save, None, Some(b"package x\n"), 14, true).unwrap(), Some(3));
    std::fs::OpenOptions::new()
        .append(true)
        .open(store.projects[one].dir.join(LOG))
        .unwrap()
        .write_all(b"{broken\n")
        .unwrap();
    let store = Store::open(data.path(), Limits::default());
    assert_eq!(store.entries(one).len(), 3, "a broken line is skipped");
}

#[test]
fn pruning_drops_old_versions_and_their_contents() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let now = 100 * DAY;
    let mut store = store(data.path());
    store.record(root, "a.go", VersionKind::Save, None, Some(b"old"), now - 6 * DAY, true).unwrap();
    store.record(root, "b.go", VersionKind::Save, None, Some(b"shared"), now - 6 * DAY, true).unwrap();
    store.record(root, "a.go", VersionKind::Save, None, Some(b"new"), now - DAY, true).unwrap();
    store.record(root, "c.go", VersionKind::Save, None, Some(b"shared"), now - DAY, true).unwrap();
    let other = Path::new("/q");
    store.record(other, "x.go", VersionKind::Save, None, Some(b"gone"), now - 7 * DAY, true).unwrap();
    let pruned = store.prune(now).unwrap();
    assert_eq!(pruned, Pruned { entries: 3, contents: 2 });
    let left: Vec<_> = store.entries(root).iter().map(|entry| entry.path.clone().unwrap()).collect();
    assert_eq!(left, ["a.go", "c.go"]);
    assert_eq!(objects(data.path()), 2, "the shared content stays for c.go");
    assert_eq!(store.projects().count(), 1, "a project without history is forgotten");
    let store = Store::open(data.path(), Limits::default());
    assert_eq!(store.entries(root).len(), 2, "pruning rewrote the log");
}

#[test]
fn pruning_keeps_the_contents_under_the_size_limit() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let limits = Limits { max_bytes: 2500, ..Limits::default() };
    let mut store = Store::open(data.path(), limits);
    let noise = |seed: u64| {
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        (0..1000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state.to_le_bytes()[0]
            })
            .collect::<Vec<u8>>()
    };
    for (time, seed) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
        store.record(root, "data.bin", VersionKind::Save, None, Some(&noise(seed)), time, true).unwrap();
    }
    assert!(store.stored_bytes() > 2500);
    store.prune(10).unwrap();
    assert!(store.stored_bytes() <= 2500, "{}", store.stored_bytes());
    let times: Vec<u64> = store.entries(root).iter().map(|entry| entry.time).collect();
    assert_eq!(times, [3, 4], "the oldest go first");
}

#[test]
fn large_files_are_not_recorded() {
    let data = tempfile::tempdir().unwrap();
    let mut store = Store::open(data.path(), Limits { max_file_bytes: 4, ..Limits::default() });
    assert_eq!(store.record(Path::new("/p"), "a", VersionKind::Save, None, Some(b"12345"), 1, true).unwrap(), None);
    assert_eq!(store.record(Path::new("/p"), "a", VersionKind::Save, None, Some(b"1234"), 1, true).unwrap(), Some(1));
}

#[test]
fn a_store_of_a_newer_format_is_left_alone() {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("store.json"), br#"{"format":99}"#).unwrap();
    let mut store = store(data.path());
    assert_eq!(store.record(Path::new("/p"), "a", VersionKind::Save, None, Some(b"a"), 1, true).unwrap(), None);
    assert!(store.label(Path::new("/p"), None, "x", 1).is_err());
    assert!(!data.path().join("projects").exists());
}

#[test]
fn entries_under_a_folder() {
    let entry = |path: &str| Entry {
        id: 1,
        time: 1,
        path: Some(path.to_owned()),
        kind: VersionKind::Save,
        label: None,
        hash: None,
        size: 0,
        binary: false,
    };
    assert!(entry("src/a.go").is_under("src"));
    assert!(entry("src").is_under("src"));
    assert!(entry("src/a.go").is_under(""));
    assert!(!entry("srcs/a.go").is_under("src"));
}

#[test]
fn an_unfinished_last_line_is_cut_and_the_log_goes_on() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let mut store = store(data.path());
    store.record(root, "a.go", VersionKind::Save, None, Some(b"one"), 1, true).unwrap();
    let log = store.projects[root].dir.join(LOG);
    std::fs::OpenOptions::new().append(true).open(&log).unwrap().write_all(br#"{"id":2,"time":2,"pa"#).unwrap();
    let mut store = Store::open(data.path(), Limits::default());
    assert_eq!(store.entries(root).len(), 1);
    store.record(root, "a.go", VersionKind::Save, None, Some(b"two"), 3, true).unwrap();
    let store = Store::open(data.path(), Limits::default());
    let ids: Vec<u64> = store.entries(root).iter().map(|entry| entry.id).collect();
    assert_eq!(ids, [1, 2], "the version after the cut line is read back");
}

#[test]
fn a_damaged_store_opens_without_failing() {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("store.json"), b"\xff not json").unwrap();
    for (prefix, name) in [("日本", "語"), ("zz", "not-a-hash"), ("ab", "c")] {
        std::fs::create_dir_all(data.path().join("objects").join(prefix)).unwrap();
        std::fs::write(data.path().join("objects").join(prefix).join(name), b"junk").unwrap();
    }
    let project = data.path().join("projects").join("broken");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(PROJECT), b"{").unwrap();
    let mut store = store(data.path());
    assert_eq!(store.stored_bytes(), 0, "files that are not contents are ignored");
    let root = Path::new("/p");
    store.record(root, "a.go", VersionKind::Save, None, Some(b"a"), 1, true).unwrap();
    let hash = store.entries(root)[0].hash.clone().unwrap();
    std::fs::remove_file(store.object_path(&hash)).unwrap();
    assert!(store.content(&hash).is_err(), "a missing content is an error");
    assert!(store.content("日本語").is_err());
    store.prune(2).unwrap();
}

#[test]
fn pruning_keeps_the_last_content_of_a_file_deleted_since() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let now = 100 * DAY;
    let mut store = store(data.path());
    store.record(root, "gone.go", VersionKind::Save, None, Some(b"first"), now - 8 * DAY, true).unwrap();
    store.record(root, "gone.go", VersionKind::Save, None, Some(b"last"), now - 7 * DAY, true).unwrap();
    store.record(root, "gone.go", VersionKind::Deleted, None, None, now - DAY, true).unwrap();
    store.record(root, "old.go", VersionKind::Save, None, Some(b"old"), now - 7 * DAY, true).unwrap();
    store.record(root, "old.go", VersionKind::Deleted, None, None, now - 6 * DAY, true).unwrap();
    store.prune(now).unwrap();
    let left: Vec<(String, Option<&str>)> = store
        .entries(root)
        .iter()
        .map(|entry| (entry.path.clone().unwrap(), entry.hash.as_ref().map(|_| "content")))
        .collect();
    assert_eq!(left, [("gone.go".to_owned(), Some("content")), ("gone.go".to_owned(), None)]);
    let kept = store.entries(root)[0].hash.clone().unwrap();
    assert_eq!(store.content(&kept).unwrap(), b"last");
}

#[test]
fn two_processes_on_one_store_never_share_an_id_and_see_each_others_versions() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let mut windowed = store(data.path());
    let mut headless = store(data.path());
    assert_eq!(windowed.record(root, "a.go", VersionKind::Save, None, Some(b"a"), 1, true).unwrap(), Some(1));
    assert_eq!(headless.record(root, "b.go", VersionKind::Save, None, Some(b"b"), 2, true).unwrap(), Some(2));
    assert_eq!(windowed.record(root, "c.go", VersionKind::Save, None, Some(b"c"), 3, true).unwrap(), Some(3));
    assert_eq!(headless.label(root, None, "mine", 4).unwrap(), 4);
    windowed.refresh();
    headless.refresh();
    assert_eq!(windowed.entries(root), headless.entries(root));
    assert_eq!(read_log(&windowed.projects[root].dir, false).0.len(), 4);
}

#[test]
fn a_change_recorded_elsewhere_counts_when_deduplicating() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let mut windowed = store(data.path());
    let mut headless = store(data.path());
    windowed.record(root, "a.go", VersionKind::Save, None, Some(b"one"), 1, true).unwrap();
    headless.record(root, "a.go", VersionKind::Save, None, Some(b"two"), 2, true).unwrap();
    let again = windowed.record(root, "a.go", VersionKind::Save, None, Some(b"two"), 3, true).unwrap();
    assert_eq!(again, None, "the latest version is the other process's");
}

#[test]
fn pruning_keeps_what_another_process_recorded_after_it_opened() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let now = 100 * DAY;
    let mut windowed = store(data.path());
    windowed.record(root, "old.go", VersionKind::Save, None, Some(b"kept text"), now - 8 * DAY, true).unwrap();
    windowed.record(root, "new.go", VersionKind::Save, None, Some(b"newer"), now - DAY, true).unwrap();
    let mut headless = store(data.path());

    windowed.record(root, "again.go", VersionKind::Save, None, Some(b"kept text"), now, true).unwrap();
    let hash = windowed.latest(root, "again.go").unwrap().hash.clone().unwrap();
    assert_eq!(headless.prune(now).unwrap().entries, 1, "only the expired version");

    windowed.refresh();
    let paths: Vec<_> = windowed.entries(root).iter().filter_map(|entry| entry.path.as_deref()).collect();
    assert_eq!(paths, ["new.go", "again.go"]);
    assert_eq!(windowed.content(&hash).unwrap(), b"kept text", "the object is still referenced");
}

#[test]
fn a_store_recovers_after_another_process_pruned_its_project_away() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let now = 100 * DAY;
    let mut windowed = store(data.path());
    windowed.record(root, "a.go", VersionKind::Save, None, Some(b"a"), 1, true).unwrap();
    let mut headless = store(data.path());
    headless.prune(now).unwrap();
    assert_eq!(headless.projects().count(), 0);

    assert_eq!(windowed.record(root, "a.go", VersionKind::Save, None, Some(b"a"), now, true).unwrap(), Some(1));
    let hash = windowed.latest(root, "a.go").unwrap().hash.clone().unwrap();
    assert_eq!(windowed.content(&hash).unwrap(), b"a", "written again, the object having gone");
}

#[test]
fn stores_writing_at_once_lose_nothing() {
    let data = tempfile::tempdir().unwrap();
    let root = Path::new("/p");
    let writers: Vec<_> = (0..4_u8)
        .map(|writer| {
            let folder = data.path().to_owned();
            std::thread::spawn(move || {
                let mut store = store(&folder);
                for number in 0..25_u8 {
                    let relative = format!("{writer}-{number}.go");
                    store.record(root, &relative, VersionKind::Save, None, Some(&[writer, number]), 1, true).unwrap();
                }
            })
        })
        .collect();
    writers.into_iter().for_each(|writer| writer.join().unwrap());
    let store = store(data.path());
    let mut ids: Vec<u64> = store.entries(root).iter().map(|entry| entry.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 100);
}

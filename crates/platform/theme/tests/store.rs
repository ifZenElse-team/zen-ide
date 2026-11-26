use std::path::{Path, PathBuf};

use zen_theme::{
    EditorKey, ResolvedTheme, Rgba, StoreError, ThemeFiles, ThemeSource, ThemeStore, UiKey, VsCodeImporter,
    ZEN_DARK_ID, ZEN_LIGHT_ID, unreadable,
};

struct Disk;

impl ThemeFiles for Disk {
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Ok(Vec::new()) };
        Ok(entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .collect())
    }

    fn read(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }

    fn write(&self, path: &Path, text: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, text).map_err(|error| error.to_string())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

fn store(dir: &Path) -> ThemeStore {
    ThemeStore::new(Some(dir.to_owned()), Box::new(Disk), vec![Box::new(VsCodeImporter::new())])
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap_or_else(|error| panic!("{error}"));
}

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vscode").join(path)
}

fn rgb(hex: &str) -> Rgba {
    hex.parse().unwrap_or_else(|error| panic!("{error:?}"))
}

#[test]
fn lists_good_broken_and_clashing_files_without_failing() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    write(dir.path(), "a-good.json", r#"{"formatVersion": 1, "id": "dusk", "name": "Dusk", "dark": true}"#);
    write(dir.path(), "b-broken.json", "{ not json");
    write(dir.path(), "c-builtin.json", r#"{"formatVersion": 1, "id": "zen-dark", "name": "Fake", "dark": true}"#);
    write(dir.path(), "d-again.json", r#"{"formatVersion": 1, "id": "dusk", "name": "Dusk Again", "dark": false}"#);
    write(dir.path(), "notes.txt", "not a theme");
    let store = store(dir.path());

    let entries = store.entries();
    let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids, ["zen-dark", "zen-light", "dusk", "b-broken", "zen-dark", "dusk"]);
    assert_eq!(entries[0].source, ThemeSource::Builtin);
    assert_eq!(entries[0].path, None);
    assert_eq!(entries[1].source, ThemeSource::Builtin);
    assert!(!entries[1].dark);
    assert_eq!(entries[2].source, ThemeSource::User);
    assert_eq!(entries[2].error, None);
    assert_eq!(entries[2].path.as_deref(), Some(dir.path().join("a-good.json").as_path()));
    assert!(entries[3].error.as_deref().is_some_and(|error| error.contains("line 1")), "{:?}", entries[3]);
    assert!(entries[4].error.as_deref().is_some_and(|error| error.contains("built-in")), "{:?}", entries[4]);
    assert!(entries[5].error.as_deref().is_some_and(|error| error.contains("a-good.json")), "{:?}", entries[5]);
    assert_eq!(store.theme(ZEN_DARK_ID).map(|theme| theme.name), Some("Zen Dark".to_owned()));
    assert_eq!(store.theme("dusk").map(|theme| theme.name), Some("Dusk".to_owned()));
}

#[test]
fn a_store_without_a_folder_has_the_built_in_themes() {
    let mut store = ThemeStore::new(None, Box::new(Disk), Vec::new());
    assert_eq!(store.entries().len(), 2);
    assert_eq!(store.current(), ZEN_DARK_ID);
    assert_eq!(store.import(&fixture("full.json")), Err(StoreError::NoFolder));
}

#[test]
fn selects_usable_themes_only_and_counts_changes() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    write(dir.path(), "dusk.json", r#"{"formatVersion": 1, "name": "Dusk", "dark": true}"#);
    write(dir.path(), "broken.json", "{");
    let mut store = store(dir.path());
    let start = store.generation();

    assert_eq!(store.select("nope"), Err(StoreError::UnknownTheme("nope".to_owned())));
    assert!(matches!(store.select("broken"), Err(StoreError::Broken { id, .. }) if id == "broken"));
    assert_eq!(store.generation(), start);
    assert_eq!(store.select("dusk"), Ok(()));
    assert_eq!(store.current(), "dusk");
    assert_eq!(store.generation(), start + 1);
    assert_eq!(store.select("dusk"), Ok(()));
    assert_eq!(store.generation(), start + 1);

    std::fs::remove_file(dir.path().join("dusk.json")).unwrap_or_else(|error| panic!("{error}"));
    store.reload();
    assert_eq!(store.current(), ZEN_DARK_ID);
    assert_eq!(store.generation(), start + 2);
}

#[test]
fn resolves_with_a_parent_from_the_folder() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    write(
        dir.path(),
        "base.json",
        r##"{"formatVersion": 1, "id": "base", "name": "Base", "dark": true, "ui": {"accent": "#FF0000"}}"##,
    );
    write(
        dir.path(),
        "child.json",
        r##"{"formatVersion": 1, "id": "child", "name": "Child", "dark": true, "parent": "base", "ui": {"link": "#0000FF"}}"##,
    );
    let mut store = store(dir.path());
    store.select("child").unwrap_or_else(|error| panic!("{error}"));

    let resolution = store.resolve_current();
    assert!(resolution.warnings.is_empty(), "{:?}", resolution.warnings);
    assert_eq!(resolution.theme.id, "child");
    assert_eq!(resolution.theme.ui(UiKey::Accent), rgb("#FF0000"));
    assert_eq!(resolution.theme.ui(UiKey::Link), rgb("#0000FF"));
    let zen = store.resolve(ZEN_DARK_ID).map(|resolution| resolution.theme);
    assert_eq!(zen.map(|theme| theme.ui(UiKey::TextPrimary)), Some(resolution.theme.ui(UiKey::TextPrimary)));
    assert!(store.resolve("nope").is_none());
}

#[test]
fn imports_vs_code_and_own_themes_with_unique_ids() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let themes = dir.path().join("themes");
    let mut store = store(&themes);
    let start = store.generation();

    let first = store.import(&fixture("full.json")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first.id, "fixture-night");
    assert_eq!(first.name, "Fixture Night");
    assert!(first.mapped > 10, "{first:?}");
    assert!(themes.join("fixture-night.json").is_file());
    let second = store.import(&fixture("full.json")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(second.id, "fixture-night-2");
    let third = store.import(&fixture("full.json")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(third.id, "fixture-night-3");
    assert_eq!(store.generation(), start + 3);
    assert_eq!(store.current(), ZEN_DARK_ID);
    assert!(store.entries().iter().all(|entry| entry.error.is_none()), "{:?}", store.entries());

    let chained = store.import(&fixture("chain/child.json")).unwrap_or_else(|error| panic!("{error}"));
    let theme = store.resolve(&chained.id).unwrap_or_else(|| panic!("no {}", chained.id)).theme;
    assert_eq!(theme.editor(EditorKey::Caret), rgb("#ABCDEF"), "the include was read next to the file");
    assert_eq!(theme.editor(EditorKey::Background), rgb("#202020"));

    let own = dir.path().join("own.json");
    write(
        dir.path(),
        "own.json",
        r##"{"formatVersion": 1, "id": "zen-dark", "name": "Own", "dark": true, "ui": {"accent": "#123456", "no.such": "#000000"}}"##,
    );
    let imported = store.import(&own).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(imported.id, "zen-dark-2");
    assert_eq!(imported.mapped, 1);
    assert_eq!(imported.unmapped, ["ui.no.such"]);
    assert_eq!(store.theme("zen-dark-2").map(|theme| theme.name), Some("Own".to_owned()));
}

#[test]
fn reports_files_it_cannot_import() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let mut store = store(&dir.path().join("themes"));
    let missing = dir.path().join("missing.json");
    assert!(matches!(store.import(&missing), Err(StoreError::Read { .. })));
    let unknown = dir.path().join("unknown.json");
    write(dir.path(), "unknown.json", r#"{"hello": "world"}"#);
    assert_eq!(store.import(&unknown), Err(StoreError::UnknownFormat(unknown)));
    let bad = dir.path().join("bad.json");
    write(dir.path(), "bad.json", r#"{"formatVersion": 1, "name": "Bad", "dark": true, "ui": {"accent": "red"}}"#);
    assert!(matches!(store.import(&bad), Err(StoreError::Import { .. })));
    assert_eq!(store.entries().len(), 2);
}

#[test]
fn an_imported_id_never_leaves_the_themes_folder() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let dir = root.path().join("themes");
    std::fs::create_dir(&dir).unwrap_or_else(|error| panic!("{error}"));
    let mut store = store(&dir);
    for (file, id) in [("up.json", "../../escaped"), ("abs.json", "/tmp/escaped"), ("dot.json", ".hidden")] {
        let source = root.path().join(file);
        let text = format!(r#"{{"formatVersion": 1, "id": "{id}", "name": "Night Owl", "dark": true}}"#);
        std::fs::write(&source, text).unwrap_or_else(|error| panic!("{error}"));
        let imported = store.import(&source).unwrap_or_else(|error| panic!("{error}"));
        assert!(imported.id.starts_with("night-owl"), "{id} became {}", imported.id);
        assert!(dir.join(format!("{}.json", imported.id)).is_file());
    }
    assert!(!root.path().join("escaped.json").exists());
    assert_eq!(std::fs::read_dir(root.path()).map(Iterator::count).unwrap_or_default(), 4);
}

#[test]
fn follows_the_system_appearance_with_a_theme_for_each() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    write(dir.path(), "day.json", r#"{"formatVersion": 1, "id": "day", "name": "Day", "dark": false}"#);
    write(dir.path(), "dusk.json", r#"{"formatVersion": 1, "id": "dusk", "name": "Dusk", "dark": true}"#);
    let mut store = store(dir.path());
    store.select("dusk").unwrap_or_else(|error| panic!("{error}"));
    store.set_system_dark(false);
    assert_eq!(store.current(), "dusk", "not following: the system does not matter");

    let start = store.generation();
    store.set_follow_system(true, None, None).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(store.current(), ZEN_LIGHT_ID, "the system is light");
    let appearance = store.appearance();
    assert!(appearance.follow_system && !appearance.system_dark);
    assert_eq!((appearance.dark.as_str(), appearance.light.as_str()), ("dusk", ZEN_LIGHT_ID));
    assert!(store.generation() > start);

    store.select("day").unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(store.current(), "day", "a light theme chosen while the system is light is the light theme");
    store.set_system_dark(true);
    assert_eq!(store.current(), "dusk");
    let generation = store.generation();
    store.set_system_dark(true);
    assert_eq!(store.generation(), generation, "nothing changed");
    store.select(ZEN_DARK_ID).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(store.appearance().dark, ZEN_DARK_ID);
    assert_eq!(store.appearance().light, "day");

    assert!(matches!(store.set_follow_system(true, Some("nope"), None), Err(StoreError::UnknownTheme(_))));
    store.set_follow_system(true, Some("dusk"), Some(ZEN_LIGHT_ID)).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(store.current(), "dusk");
    store.set_follow_system(false, None, None).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(store.current(), ZEN_DARK_ID, "back to the theme chosen last");
    assert_eq!(store.chosen(), ZEN_DARK_ID);

    store.set_follow_system(true, None, None).unwrap_or_else(|error| panic!("{error}"));
    std::fs::remove_file(dir.path().join("dusk.json")).unwrap_or_else(|error| panic!("{error}"));
    store.reload();
    assert_eq!(store.appearance().dark, ZEN_DARK_ID, "a theme that is gone gives way to the built-in one");
}

#[test]
fn an_imported_light_theme_builds_on_zen_light_and_reads() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let themes = dir.path().join("themes");
    let mut store = store(&themes);
    let imported = store.import(&fixture("light.json")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(imported.id, "fixture-day");
    assert!(imported.mapped > 40, "{imported:?}");
    assert!(imported.adjusted.contains(&"ui.text.secondary".to_owned()), "{imported:?}");
    assert!(imported.adjusted.contains(&"terminal.ansi.3".to_owned()), "{imported:?}");
    let theme = store.theme(&imported.id).unwrap_or_else(|| panic!("no theme"));
    assert_eq!(theme.parent.as_deref(), Some(ZEN_LIGHT_ID));
    let resolution = store.resolve(&imported.id).unwrap_or_else(|| panic!("no theme"));
    assert!(resolution.warnings.is_empty(), "{:?}", resolution.warnings);
    let problems: Vec<String> = unreadable(&resolution.theme).iter().map(ToString::to_string).collect();
    assert!(problems.is_empty(), "{problems:#?}");
    let light = ResolvedTheme::zen_light();
    assert_eq!(
        resolution.theme.ui(UiKey::PopupHover),
        light.ui(UiKey::PopupHover),
        "unmapped keys come from Zen Light"
    );
    assert_eq!(resolution.theme.editor(EditorKey::FoldMarker), light.editor(EditorKey::FoldMarker));

    let night = store.import(&fixture("full.json")).unwrap_or_else(|error| panic!("{error}"));
    let theme = store.theme(&night.id).unwrap_or_else(|| panic!("no theme"));
    assert_eq!(theme.parent.as_deref(), Some(ZEN_DARK_ID), "a dark theme still builds on Zen Dark");
}

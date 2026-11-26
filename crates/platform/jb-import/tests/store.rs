use std::path::{Path, PathBuf};

use zen_jb_import::importers;
use zen_theme::{EditorKey, Rgba, ThemeFiles, ThemeImporter, ThemeStore, VsCodeImporter};

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

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path)
}

fn rgb(hex: &str) -> Rgba {
    hex.parse().unwrap_or_else(|error| panic!("{error:?}"))
}

fn store(dir: &Path) -> ThemeStore {
    let mut all: Vec<Box<dyn ThemeImporter>> = importers();
    all.push(Box::new(VsCodeImporter::new()));
    ThemeStore::new(Some(dir.to_owned()), Box::new(Disk), all)
}

#[test]
fn imports_colour_schemes_and_ui_themes_with_their_scheme() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let themes = dir.path().join("themes");
    let mut store = store(&themes);

    let scheme = store.import(&fixture("every-key.icls")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(scheme.id, "fixture-night");
    assert!(scheme.mapped > 10, "{scheme:?}");
    assert!(themes.join("fixture-night.json").is_file());
    let again = store.import(&fixture("every-key.icls")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(again.id, "fixture-night-2");

    let xml = store.import(&fixture("ember/Ember.xml")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(xml.id, "ember-test");
    let ui = store.import(&fixture("ember/Ember.theme.json")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(ui.id, "ember-test-2");
    assert!(!ui.unmapped.iter().any(|name| name == "editorScheme"), "the scheme next to it was read: {ui:?}");
    store.select(&ui.id).unwrap_or_else(|error| panic!("{error}"));
    let theme = store.resolve_current().theme;
    assert_eq!(theme.name, "Ember Test");
    assert_eq!(theme.editor(EditorKey::Caret), rgb("#E06040"));
    assert!(store.entries().iter().all(|entry| entry.error.is_none()), "{:?}", store.entries());
}

#[test]
fn every_imported_scheme_reads_on_its_own_backgrounds() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let mut store = store(&dir.path().join("themes"));
    for path in ["chains.icls", "every-key.icls", "ember/Ember.xml", "ember/Ember.theme.json"] {
        let imported = store.import(&fixture(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        let theme = store.theme(&imported.id).unwrap_or_else(|| panic!("{path}"));
        assert_eq!(theme.parent.as_deref(), Some(zen_theme::base_id(theme.dark)), "{path}");
        let resolved = store.resolve(&imported.id).unwrap_or_else(|| panic!("{path}")).theme;
        let problems: Vec<String> = zen_theme::unreadable(&resolved).iter().map(ToString::to_string).collect();
        assert!(problems.is_empty(), "{path}: {problems:#?}");
    }
}

#[test]
fn a_ui_theme_without_its_scheme_still_imports() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let lonely = dir.path().join("Lonely.theme.json");
    std::fs::copy(fixture("ember/Ember.theme.json"), &lonely).unwrap_or_else(|error| panic!("{error}"));
    let mut store = store(&dir.path().join("themes"));
    let imported = store.import(&lonely).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(imported.id, "ember-test");
    assert!(imported.unmapped.iter().any(|name| name == "editorScheme"), "{imported:?}");
}

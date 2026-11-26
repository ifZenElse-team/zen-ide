use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_theme::{Appearance, Imported, StoreError, ThemeEntry, ThemeFiles, ThemeStore};

pub type SharedThemes = Arc<Mutex<ThemeStore>>;

pub struct DiskFiles;

impl ThemeFiles for DiskFiles {
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        Ok(entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
                !name.starts_with('.') && name.ends_with(".json") && path.is_file()
            })
            .collect())
    }

    fn read(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }

    fn write(&self, path: &Path, text: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        zen_vfs::write_atomic(path, text.as_bytes()).map_err(|error| error.to_string())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeList {
    pub current: String,
    pub themes: Vec<ThemeEntry>,
    pub appearance: Appearance,
}

/// The parameters of `ui.theme.followSystem`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FollowSystemParams {
    /// Whether the theme follows the system's light or dark appearance.
    pub enabled: bool,
    /// The theme to use while the system is dark, from `ui.theme.list`; left out, it stays as it is.
    #[serde(default)]
    pub dark: Option<String>,
    /// The theme to use while the system is light, from `ui.theme.list`; left out, it stays as it is.
    #[serde(default)]
    pub light: Option<String>,
}

/// The parameters of `ui.theme.select`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectParams {
    /// The id of the theme, from `ui.theme.list`.
    pub id: String,
}

/// The parameters of `ui.theme.import`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportParams {
    /// The theme file, an absolute path: a Zen theme, a JetBrains `.icls`, `.xml` or `.theme.json`, or a VS Code colour theme.
    pub path: PathBuf,
    /// Whether to use the new theme at once.
    #[serde(default)]
    pub select: bool,
}

pub fn lock(store: &SharedThemes) -> MutexGuard<'_, ThemeStore> {
    store.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn list(store: &ThemeStore) -> ThemeList {
    ThemeList { current: store.current().to_owned(), themes: store.entries().to_vec(), appearance: store.appearance() }
}

pub fn follow_system(store: &mut ThemeStore, params: &FollowSystemParams) -> Result<(), CommandError> {
    store
        .set_follow_system(params.enabled, params.dark.as_deref(), params.light.as_deref())
        .map_err(|error| message(&error))
}

pub fn select(store: &mut ThemeStore, id: &str) -> Result<(), CommandError> {
    store.select(id).map_err(|error| message(&error))
}

pub fn import(store: &mut ThemeStore, params: &ImportParams) -> Result<Imported, CommandError> {
    let imported = store.import(&params.path).map_err(|error| message(&error))?;
    if params.select {
        select(store, &imported.id)?;
    }
    Ok(imported)
}

pub fn message(error: &StoreError) -> CommandError {
    CommandError::new(match error {
        StoreError::UnknownTheme(id) => t!("workbench-theme-error-unknown", id = id.as_str()),
        StoreError::Broken { id, error } => {
            t!("workbench-theme-error-broken", id = id.as_str(), error = error.as_str())
        }
        StoreError::NoFolder => t!("workbench-theme-error-no-folder"),
        StoreError::Read { path, error } => {
            t!("workbench-theme-error-read", path = path.display().to_string(), error = error.as_str())
        }
        StoreError::UnknownFormat(path) => t!("workbench-theme-error-format", path = path.display().to_string()),
        StoreError::Import { path, error } => {
            t!("workbench-theme-error-import", path = path.display().to_string(), error = error.as_str())
        }
        StoreError::Write { path, error } => {
            t!("workbench-theme-error-write", path = path.display().to_string(), error = error.as_str())
        }
    })
}

pub struct ListThemes(SharedThemes);

impl Command for ListThemes {
    type Params = NoParams;
    type Output = ThemeList;
    const ID: &'static str = "ui.theme.list";
    const TITLE: &'static str = "Themes";
    const DESCRIPTION: &'static str = "Lists the themes (id, name, dark, source builtin or user, the file and why it cannot be used) and the id of the one in use.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<ThemeList, CommandError> {
        Ok(list(&lock(&self.0)))
    }
}

pub struct SelectTheme(SharedThemes);

impl Command for SelectTheme {
    type Params = SelectParams;
    type Output = NoParams;
    const ID: &'static str = "ui.theme.select";
    const TITLE: &'static str = "Select Theme";
    const DESCRIPTION: &'static str = "Uses a theme in every window at once and remembers the choice.";

    fn run(&self, _session: &mut Session, params: SelectParams) -> Result<NoParams, CommandError> {
        select(&mut lock(&self.0), &params.id)?;
        Ok(NoParams {})
    }
}

pub struct ImportTheme(SharedThemes);

impl Command for ImportTheme {
    type Params = ImportParams;
    type Output = Imported;
    const ID: &'static str = "ui.theme.import";
    const TITLE: &'static str = "Import Theme";
    const DESCRIPTION: &'static str = "Converts a Zen, JetBrains (.icls, .xml, .theme.json) or VS Code theme file into a theme of the themes folder, \
         a taken id getting -2, -3 and so on; returns its id, name, how many entries were mapped and those that were not. \
         With select, uses it at once.";

    fn run(&self, _session: &mut Session, params: ImportParams) -> Result<Imported, CommandError> {
        import(&mut lock(&self.0), &params)
    }
}

pub struct FollowSystem(SharedThemes);

impl Command for FollowSystem {
    type Params = FollowSystemParams;
    type Output = NoParams;
    const ID: &'static str = "ui.theme.followSystem";
    const TITLE: &'static str = "Sync Theme with OS";
    const DESCRIPTION: &'static str = "Makes the theme follow the system's light or dark appearance (Settings → Appearance → Sync with OS), \
         or stops it; dark and light name the theme for each, and choosing a theme while following sets the one of its appearance. \
         The choice is remembered.";

    fn run(&self, _session: &mut Session, params: FollowSystemParams) -> Result<NoParams, CommandError> {
        follow_system(&mut lock(&self.0), &params)?;
        Ok(NoParams {})
    }
}

pub struct ReloadThemes(SharedThemes);

impl Command for ReloadThemes {
    type Params = NoParams;
    type Output = NoParams;
    const ID: &'static str = "ui.theme.reload";
    const TITLE: &'static str = "Reload Themes";
    const DESCRIPTION: &'static str =
        "Reads the themes folder again; a theme in use that is gone gives way to the built-in theme of its appearance.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<NoParams, CommandError> {
        lock(&self.0).reload();
        Ok(NoParams {})
    }
}

pub fn register(registry: &mut Registry, store: &SharedThemes) -> Result<(), RegistryError> {
    registry.register(ListThemes(Arc::clone(store)))?;
    registry.register(SelectTheme(Arc::clone(store)))?;
    registry.register(ImportTheme(Arc::clone(store)))?;
    registry.register(ReloadThemes(Arc::clone(store)))?;
    registry.register(FollowSystem(Arc::clone(store)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_theme::{VsCodeImporter, ZEN_DARK_ID};

    use super::*;

    fn setup(dir: &Path) -> (Registry, SharedThemes, Session) {
        zen_i18n::register(crate::LOCALES);
        let store = Arc::new(Mutex::new(ThemeStore::new(
            Some(dir.to_owned()),
            Box::new(DiskFiles),
            vec![Box::new(VsCodeImporter::new())],
        )));
        let mut registry = Registry::new();
        register(&mut registry, &store).unwrap_or_else(|error| panic!("{error}"));
        (registry, store, Session::new())
    }

    fn fixture(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../theme/tests/fixtures/vscode").join(path)
    }

    #[test]
    fn lists_selects_imports_and_reloads() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let themes = dir.path().join("themes");
        std::fs::create_dir(&themes).unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(themes.join("broken.json"), "{").unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(themes.join(".hidden.json"), "{").unwrap_or_else(|error| panic!("{error}"));
        let (registry, store, mut session) = setup(&themes);

        let listed =
            registry.dispatch(&mut session, "ui.theme.list", json!({})).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(listed["current"], ZEN_DARK_ID);
        assert_eq!(
            listed["themes"][0],
            json!({ "id": "zen-dark", "name": "Zen Dark", "dark": true, "source": "builtin" })
        );
        assert_eq!(
            listed["themes"][1],
            json!({ "id": "zen-light", "name": "Zen Light", "dark": false, "source": "builtin" })
        );
        assert_eq!(listed["themes"][2]["id"], "broken");
        assert_eq!(listed["themes"][2]["source"], "user");
        assert!(listed["themes"][2]["error"].is_string(), "{listed}");
        assert_eq!(listed["themes"].as_array().map(Vec::len), Some(3));
        assert_eq!(
            listed["appearance"],
            json!({ "followSystem": false, "systemDark": true, "dark": "zen-dark", "light": "zen-light" })
        );

        let error = registry.dispatch(&mut session, "ui.theme.select", json!({ "id": "nope" })).unwrap_err();
        assert!(error.to_string().contains("nope"), "{error}");

        let path = fixture("full.json");
        let imported = registry
            .dispatch(&mut session, "ui.theme.import", json!({ "path": path }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(imported["id"], "fixture-night");
        assert_eq!(imported["name"], "Fixture Night");
        assert!(imported["mapped"].as_u64().is_some_and(|mapped| mapped > 0), "{imported}");
        assert!(imported["unmapped"].is_array());
        assert_eq!(lock(&store).current(), ZEN_DARK_ID);

        let again = registry
            .dispatch(&mut session, "ui.theme.import", json!({ "path": path, "select": true }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(again["id"], "fixture-night-2");
        assert_eq!(lock(&store).current(), "fixture-night-2");

        let selected = registry
            .dispatch(&mut session, "ui.theme.select", json!({ "id": "fixture-night" }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(selected, json!({}));
        assert_eq!(lock(&store).current(), "fixture-night");

        std::fs::remove_file(themes.join("fixture-night.json")).unwrap_or_else(|error| panic!("{error}"));
        let generation = lock(&store).generation();
        let reloaded =
            registry.dispatch(&mut session, "ui.theme.reload", json!({})).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(reloaded, json!({}));
        assert_eq!(lock(&store).current(), ZEN_DARK_ID);
        assert!(lock(&store).generation() > generation);
        let listed =
            registry.dispatch(&mut session, "ui.theme.list", json!({})).unwrap_or_else(|error| panic!("{error}"));
        let ids: Vec<&str> =
            listed["themes"].as_array().into_iter().flatten().filter_map(|theme| theme["id"].as_str()).collect();
        assert_eq!(ids, ["zen-dark", "zen-light", "broken", "fixture-night-2"]);
    }

    #[test]
    fn import_errors_are_messages() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let (registry, _store, mut session) = setup(&dir.path().join("themes"));
        let missing = dir.path().join("missing.json");
        let error = registry.dispatch(&mut session, "ui.theme.import", json!({ "path": missing })).unwrap_err();
        assert!(error.to_string().contains("missing.json"), "{error}");
        let text = dir.path().join("text.json");
        std::fs::write(&text, "[]").unwrap_or_else(|error| panic!("{error}"));
        let error = registry.dispatch(&mut session, "ui.theme.import", json!({ "path": text })).unwrap_err();
        assert!(error.to_string().contains("text.json"), "{error}");
    }

    #[test]
    fn the_theme_follows_the_system_on_command() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let (registry, store, mut session) = setup(&dir.path().join("themes"));
        lock(&store).set_system_dark(false);
        let done = registry
            .dispatch(&mut session, "ui.theme.followSystem", json!({ "enabled": true }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(done, json!({}));
        assert_eq!(lock(&store).current(), zen_theme::ZEN_LIGHT_ID, "the system is light");
        let listed =
            registry.dispatch(&mut session, "ui.theme.list", json!({})).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(listed["appearance"]["followSystem"], true);
        assert_eq!(listed["current"], "zen-light");

        let error = registry
            .dispatch(&mut session, "ui.theme.followSystem", json!({ "enabled": true, "light": "nope" }))
            .unwrap_err();
        assert!(error.to_string().contains("nope"), "{error}");
        registry
            .dispatch(&mut session, "ui.theme.followSystem", json!({ "enabled": false }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(lock(&store).current(), ZEN_DARK_ID);
    }
}

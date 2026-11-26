use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use zen_core::keymap::{KeymapImport, KeymapImporter, Keystroke};

use super::*;
use crate::keymaps::KeymapStore;
use crate::plugin_manager::{CatalogStatus, PluginDetails, PluginList, PluginManager, PluginSummary, Queued};
use crate::themes::DiskFiles;

struct FakeImporter;

impl KeymapImporter for FakeImporter {
    fn detects(&self, text: &str) -> bool {
        text.starts_with("<keymap")
    }

    fn import(&self, text: &str) -> Result<KeymapImport, String> {
        if text.contains("broken") {
            return Err("broken on purpose".to_owned());
        }
        Ok(KeymapImport {
            name: text.split('"').nth(1).unwrap_or_default().to_owned(),
            parent: None,
            shortcuts: [("editor.line.duplicate".to_owned(), vec![Keystroke::parse("⇧⌘D").unwrap_or_default()])].into(),
            mapped: 1,
            unmapped: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

struct FakePlugins(Vec<(&'static str, &'static str)>);

impl PluginManager for FakePlugins {
    fn catalog(&self, query: &str) -> PluginList {
        self.installed(query)
    }

    fn installed(&self, _: &str) -> PluginList {
        let plugins = self
            .0
            .iter()
            .map(|(id, version)| PluginSummary {
                id: (*id).to_owned(),
                name: (*id).to_owned(),
                description: String::new(),
                installed: Some((*version).to_owned()),
                latest: None,
                update: None,
                bundled: false,
                enabled: true,
                installing: false,
                extensions: Vec::new(),
                names: Vec::new(),
                language_servers: false,
                runs: Vec::new(),
                person_only: Vec::new(),
                tool_windows: Vec::new(),
            })
            .collect();
        PluginList {
            catalog: CatalogStatus::NotFetched,
            plugins,
            installing: Vec::new(),
            restart_required: Vec::new(),
            check_failed: None,
        }
    }

    fn details(&self, id: &str) -> Result<PluginDetails, String> {
        Err(id.to_owned())
    }

    fn install(&self, id: &str) -> Result<Queued, String> {
        Err(id.to_owned())
    }

    fn update(&self, id: &str) -> Result<Queued, String> {
        Err(id.to_owned())
    }

    fn update_all(&self) -> Result<Queued, String> {
        Err(String::new())
    }

    fn refresh(&self) -> PluginList {
        self.installed("")
    }

    fn set_enabled(&self, id: &str, _: bool) -> Result<(), String> {
        Err(id.to_owned())
    }

    fn uninstall(&self, id: &str) -> Result<(), String> {
        Err(id.to_owned())
    }
}

const THEME: &str =
    r##"{"formatVersion": 1, "id": "dusk", "name": "Dusk", "dark": true, "ui": {"accent": "#FF0000"}}"##;
const KEYMAP: &str =
    r#"{"formatVersion": 1, "id": "mine", "name": "Mine", "shortcuts": {"editor.line.delete": ["⌥⌘K"]}}"#;

fn target(data: &Path, plugins: Vec<(&'static str, &'static str)>) -> SettingsTarget {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let keymaps = Arc::new(Mutex::new(KeymapStore::new(Some(data.to_owned()), vec![Box::new(FakeImporter)])));
    let themes =
        Arc::new(Mutex::new(zen_theme::ThemeStore::new(Some(data.join("themes")), Box::new(DiskFiles), Vec::new())));
    SettingsTarget {
        data: Some(data.to_owned()),
        product: "GoGarden".to_owned(),
        version: "1.2.3".to_owned(),
        keymaps: Some(keymaps),
        themes: Some(themes),
        plugins: Some(Arc::new(FakePlugins(plugins))),
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn filled(data: &Path) {
    write(&data.join("settings.json"), r#"{"ligatures": true, "keymap": "mine"}"#);
    write(&data.join("language-servers.json"), "{}");
    write(&data.join("keymap.json"), r#"{"editor.line.duplicate": ["⇧⌘D"]}"#);
    write(&data.join("keymaps/mine.json"), KEYMAP);
    write(&data.join("themes/dusk.json"), THEME);
    write(&data.join("templates.json"), r#"{"formatVersion": 1}"#);
    write(&data.join("modules.json"), r#"{"go": true}"#);
    write(&data.join("plugins/disabled.json"), r#"["zen.markdown"]"#);
    write(&data.join("recent-projects.json"), "[]");
    write(&data.join("local-history/entry"), "history");
}

fn names(path: &Path) -> Vec<String> {
    let archive = Archive::parse(std::fs::read(path).unwrap(), Limits::SETTINGS).unwrap();
    archive.entries().iter().map(|entry| entry.name.clone()).collect()
}

fn checksum(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().filter_map(Result::ok) {
            if entry.file_type().unwrap().is_dir() {
                folders.push(entry.path());
            } else {
                found.push((entry.path().display().to_string(), std::fs::read(entry.path()).unwrap()));
            }
        }
    }
    found.sort();
    found
}

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(1_792_000_000);
    for (name, text) in files {
        writer.add(name, text.as_bytes()).unwrap();
    }
    writer.finish()
}

#[test]
fn export_writes_the_chosen_components_into_a_zip() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    filled(&data);
    let target = target(&data, vec![("zen.markdown", "1.0.0")]);
    let zip = dir.path().join("out/settings.zip");
    let chosen = ExportParams { path: zip.clone(), components: Some(vec![Component::Keymaps, Component::Settings]) };
    let report = export(&target, &chosen).unwrap();
    assert_eq!(
        names(&zip),
        [MANIFEST, "settings.json", "language-servers.json", "keymap.json", "keymaps/mine.json"],
        "{report:?}"
    );
    assert_eq!(report.bytes, std::fs::metadata(&zip).unwrap().len());
    let archive = Archive::parse(std::fs::read(&zip).unwrap(), Limits::SETTINGS).unwrap();
    let manifest: Manifest = serde_json::from_slice(&archive.read(&archive.entries()[0]).unwrap()).unwrap();
    assert_eq!((manifest.kind.as_str(), manifest.product.as_str()), ("export", "GoGarden"));
    assert_eq!(manifest.components, [Component::Settings, Component::Keymaps]);

    export(&target, &ExportParams { path: zip.clone(), components: None }).unwrap();
    let all = names(&zip);
    for name in ["themes/dusk.json", "templates.json", "modules.json", "plugins/disabled.json", INSTALLED] {
        assert!(all.iter().any(|entry| entry == name), "{name} in {all:?}");
    }
    assert!(!all.iter().any(|entry| entry.contains("recent") || entry.contains("history")), "{all:?}");

    let refused = |path: PathBuf, components: Option<Vec<Component>>| {
        export(&target, &ExportParams { path, components }).unwrap_err().message
    };
    assert!(refused(dir.path().join("project/.idea/s.zip"), None).contains(".idea"));
    assert!(refused(dir.path().join("project/.zen/s.zip"), None).contains(".zen"));
    assert!(refused(PathBuf::from("settings.zip"), None).contains("absolute"));
    assert!(refused(dir.path().to_owned(), None).contains("folder"));
    assert!(refused(dir.path().join("x.zip"), Some(Vec::new())).contains("component"));
}

#[test]
fn our_zip_imports_back_into_another_data_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("to"));
    filled(&from);
    let zip = dir.path().join("settings.zip");
    export(
        &target(&from, vec![("zen.markdown", "1.0.0"), ("zen.toml", "2.0.0")]),
        &ExportParams { path: zip.clone(), components: None },
    )
    .unwrap();
    std::fs::create_dir_all(&to).unwrap();
    let target = target(&to, vec![("zen.markdown", "1.0.0")]);
    let seen = inspect(&target, &zip).unwrap();
    assert_eq!(seen.kind, SourceKind::Zen);
    assert_eq!(seen.product.as_deref(), Some("GoGarden"));
    let ids: Vec<Component> = seen.components.iter().map(entry_id).collect();
    assert_eq!(ids, Component::ALL);
    assert_eq!(seen.skipped.len(), 1, "{seen:?}");
    assert_eq!(seen.skipped[0].entry, "plugins/installed.json: zen.toml");
    assert!(seen.skipped[0].reason.contains("zen.toml 2.0.0 is not installed"), "{:?}", seen.skipped);

    let only_keymaps =
        import(&target, &ImportParams { path: zip.clone(), components: Some(vec![Component::Keymaps]) }).unwrap();
    assert!(!only_keymaps.restart_required);
    assert_eq!(only_keymaps.imported.len(), 1);
    assert!(to.join("keymaps/mine.json").is_file() && !to.join("settings.json").exists());
    let listed = crate::keymaps::lock(target.keymaps.as_ref().unwrap()).entries().to_vec();
    assert!(listed.iter().any(|entry| entry.id == "mine"), "read again after the import: {listed:?}");

    let all = import(&target, &ImportParams { path: zip, components: None }).unwrap();
    assert!(all.restart_required);
    for file in ["settings.json", "themes/dusk.json", "templates.json", "modules.json", "plugins/disabled.json"] {
        assert_eq!(std::fs::read(to.join(file)).unwrap(), std::fs::read(from.join(file)).unwrap(), "{file}");
    }
    assert!(!to.join(INSTALLED).exists(), "the plug-in list is only compared");
    assert!(!to.join("recent-projects.json").exists());
}

fn entry_id(entries: &ComponentEntries) -> Component {
    entries.id
}

#[test]
fn entries_that_cannot_be_read_are_skipped_with_why() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let zip = dir.path().join("mixed.zip");
    std::fs::write(
        &zip,
        zip_of(&[
            ("settings.json", "[1, 2]"),
            ("themes/bad.json", "{ not json"),
            ("keymaps/mine.json", KEYMAP),
            ("notes.txt", "hello"),
            ("templates.json", "{}"),
        ]),
    )
    .unwrap();
    let target = target(&data, Vec::new());
    let report = import(&target, &ImportParams { path: zip, components: None }).unwrap();
    let skipped: Vec<&str> = report.skipped.iter().map(|skipped| skipped.entry.as_str()).collect();
    assert_eq!(skipped, ["settings.json", "themes/bad.json", "notes.txt"], "{report:?}");
    assert!(report.skipped.iter().all(|skipped| !skipped.reason.is_empty()));
    assert!(data.join("keymaps/mine.json").is_file() && data.join("templates.json").is_file());
    assert!(!data.join("settings.json").exists() && !data.join("notes.txt").exists());
}

#[test]
fn a_jetbrains_export_gives_its_keymaps_and_colour_schemes() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let zip = dir.path().join("jetbrains.zip");
    std::fs::write(
        &zip,
        zip_of(&[
            (JETBRAINS_MARKER, ""),
            ("keymaps/My Mac.xml", r#"<keymap name="My Mac" version="1"/>"#),
            ("keymaps/broken.xml", r#"<keymap name="broken"/>"#),
            ("colors/Dusk.icls", r#"<scheme name="Dusk"/>"#),
            ("options/editor.xml", "<application/>"),
            ("codestyles/Default.xml", "<code_scheme/>"),
        ]),
    )
    .unwrap();
    let target = target(&data, Vec::new());
    let seen = inspect(&target, &zip).unwrap();
    assert_eq!(seen.kind, SourceKind::JetBrains);
    assert_eq!(seen.components.iter().map(entry_id).collect::<Vec<_>>(), [Component::Keymaps, Component::Themes]);
    let skipped: Vec<&str> = seen.skipped.iter().map(|skipped| skipped.entry.as_str()).collect();
    assert_eq!(skipped, [JETBRAINS_MARKER, "options/editor.xml", "codestyles/Default.xml"]);

    let report = import(&target, &ImportParams { path: zip.clone(), components: None }).unwrap();
    assert!(!report.restart_required);
    assert_eq!(
        report.imported,
        [ComponentEntries { id: Component::Keymaps, entries: vec!["keymaps/My Mac.xml".into()] }]
    );
    let skipped: Vec<&str> = report.skipped.iter().map(|skipped| skipped.entry.as_str()).collect();
    assert_eq!(
        skipped,
        [JETBRAINS_MARKER, "options/editor.xml", "codestyles/Default.xml", "keymaps/broken.xml", "colors/Dusk.icls"],
        "{report:?}"
    );
    let file: KeymapFile = serde_json::from_slice(&std::fs::read(data.join("keymaps/my-mac.json")).unwrap()).unwrap();
    assert_eq!(file.name, "My Mac");
    assert_eq!(file.source.as_deref(), Some(zip.join("keymaps/My Mac.xml").display().to_string().as_str()));
    let only_themes = ImportParams { path: zip, components: Some(vec![Component::Plugins]) };
    assert!(import(&target, &only_themes).unwrap_err().message.contains("nothing"));
}

#[test]
fn a_single_keymap_or_theme_file_is_carried_over_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let target = target(&data, Vec::new());
    let keymap = dir.path().join("Mine.xml");
    write(&keymap, "<keymap version=\"1\" name=\"Mine\"></keymap>");
    let found = inspect(&target, &keymap).unwrap();
    assert_eq!(found.kind, SourceKind::File);
    assert_eq!(found.components[0].id, Component::Keymaps);
    let params = ImportParams { path: keymap, components: None };
    let report = import(&target, &params).unwrap();
    assert_eq!(report.imported[0].entries, ["Mine.xml"], "{report:?}");
    assert!(!report.restart_required, "a keymap applies at once");

    let theme = dir.path().join("Night.json");
    write(&theme, "[]");
    let report = import(&target, &ImportParams { path: theme, components: None }).unwrap();
    assert!(report.imported.is_empty(), "a broken theme is not carried over: {report:?}");
    assert_eq!(report.skipped[0].entry, "Night.json", "and is reported with why: {report:?}");

    let other = dir.path().join("notes.xml");
    write(&other, "<notes/>");
    assert!(inspect(&target, &other).is_err(), "an XML file of neither kind is refused");
}

#[test]
fn hostile_and_foreign_archives_are_refused_and_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    filled(&data);
    let before = checksum(&data);
    let target = target(&data, Vec::new());
    let refused = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let params = ImportParams { path, components: None };
        import(&target, &params).unwrap_err().message
    };

    let mut slip = zip_of(&[("xxxxxxxxxx.json", "{}")]);
    for at in
        slip.windows(15).enumerate().filter(|(_, w)| *w == b"xxxxxxxxxx.json").map(|(at, _)| at).collect::<Vec<_>>()
    {
        slip[at..at + 15].copy_from_slice(b"../../evil.json");
    }
    assert!(refused("slip.zip", &slip).contains("../../evil.json"));
    assert!(!dir.path().join("evil.json").exists());

    let mut absolute = zip_of(&[("xetc-passwd", "{}")]);
    for at in
        absolute.windows(11).enumerate().filter(|(_, w)| *w == b"xetc-passwd").map(|(at, _)| at).collect::<Vec<_>>()
    {
        absolute[at..at + 11].copy_from_slice(b"/etc/passwd");
    }
    assert!(refused("absolute.zip", &absolute).contains("/etc/passwd"));

    let big = dir.path().join("big.zip");
    std::fs::File::create(&big).unwrap().set_len(Limits::SETTINGS.archive + 1).unwrap();
    let message = import(&target, &ImportParams { path: big, components: None }).unwrap_err().message;
    assert!(message.contains("64 MB"), "{message}");

    let crowded: Vec<(String, String)> =
        (0..=Limits::SETTINGS.entries).map(|n| (format!("f{n}"), String::new())).collect();
    let crowded: Vec<(&str, &str)> = crowded.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert!(refused("crowded.zip", &zip_of(&crowded)).contains("4096"));

    let mut bomb = zip_of(&[("settings.json", &" ".repeat(9 * 1024 * 1024))]);
    assert!(refused("bomb.zip", &bomb).contains("8 MB"));
    let central = bomb.windows(4).rposition(|w| w == 0x0201_4b50u32.to_le_bytes()).unwrap();
    bomb[central + 24..central + 28].copy_from_slice(&2u32.to_le_bytes());
    let report = import(
        &target,
        &ImportParams {
            path: {
                let path = dir.path().join("lying.zip");
                std::fs::write(&path, &bomb).unwrap();
                path
            },
            components: None,
        },
    );
    assert!(report.is_err() || report.unwrap().skipped.iter().any(|skipped| skipped.entry == "settings.json"));

    assert!(refused("text.zip", b"not an archive at all").contains("not a ZIP"));
    assert!(refused("empty.zip", &zip_of(&[("readme.md", "hi")])).contains("no settings"));
    assert_eq!(checksum(&data), before, "nothing in the data folder changed");
}

#[test]
fn restore_backs_up_then_removes_and_the_backup_imports_back() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    filled(&data);
    let project = dir.path().join("project");
    write(&project.join(".zen/runConfigurations.json"), "{}");
    write(&project.join(".idea/workspace.xml"), "<project/>");
    let project_before = checksum(&project);
    let settings_before: Vec<(String, Vec<u8>)> = Component::ALL
        .into_iter()
        .flat_map(|id| component_files(&data, id))
        .map(|file| (file.clone(), std::fs::read(data.join(&file)).unwrap()))
        .collect();
    let target = target(&data, Vec::new());

    let report = restore_defaults(&target).unwrap();
    assert!(report.restart_required);
    assert!(report.backup.starts_with(data.join(BACKUPS)), "{report:?}");
    assert_eq!(report.files.len(), settings_before.len());
    for (file, bytes) in &settings_before {
        assert!(!data.join(file).exists(), "{file} removed");
        assert_eq!(&std::fs::read(report.backup.join(file)).unwrap(), bytes, "{file} backed up");
    }
    assert!(!data.join("keymaps").exists() && !data.join("themes").exists());
    assert!(data.join("recent-projects.json").is_file() && data.join("local-history/entry").is_file());
    assert!(data.join("plugins").is_dir(), "the plug-in folder stays");

    let seen = inspect(&target, &report.backup).unwrap();
    assert_eq!(seen.kind, SourceKind::Backup);
    let back = import(&target, &ImportParams { path: report.backup.clone(), components: None }).unwrap();
    assert!(back.skipped.is_empty(), "{back:?}");
    for (file, bytes) in &settings_before {
        assert_eq!(&std::fs::read(data.join(file)).unwrap(), bytes, "{file} restored");
    }
    let second = restore_defaults(&target).unwrap();
    assert_ne!(second.backup, report.backup);
    assert_eq!(checksum(&project), project_before, "the project's .zen and .idea are untouched");
}

#[test]
fn the_commands_need_a_data_folder_and_are_registered() {
    let mut target = target(Path::new("/nonexistent"), Vec::new());
    target.data = None;
    let mut registry = Registry::new();
    register(&mut registry, target).unwrap();
    let mut session = Session::new();
    for (id, params) in [
        ("settings.ide.components", json!({})),
        ("settings.ide.restoreDefaults", json!({})),
        ("settings.ide.export", json!({ "path": "/tmp/x.zip" })),
        ("settings.ide.import", json!({ "path": "/tmp/x.zip" })),
    ] {
        let error = registry.dispatch(&mut session, id, params).unwrap_err().to_string();
        assert!(error.contains("data folder"), "{id}: {error}");
    }
    assert!(registry.dispatch(&mut session, "settings.ide.inspect", json!({ "path": "relative.zip" })).is_err());
}

#[test]
fn utc_dates_are_right() {
    assert_eq!(utc(0), (1970, 1, 1, 0, 0, 0));
    assert_eq!(utc(1_792_627_200 + 3_723), (2026, 10, 22, 1, 2, 3));
    assert_eq!(utc(951_782_400), (2000, 2, 29, 0, 0, 0));
}

const HIDDEN: &str = r#"{"mcpRedactFiles":["secret/**"]}"#;
const NOT_HIDDEN: &str = r#"{"mcpRedactFiles":[]}"#;

fn hidden_files_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Registry, Session) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let file = data.join("settings.json");
    let mut registry = Registry::new();
    register(&mut registry, target(&data, Vec::new())).unwrap();
    let mut session = Session::new();
    let zip = dir.path().join("mine.zip");
    std::fs::write(&file, NOT_HIDDEN).unwrap();
    registry.dispatch(&mut session, "settings.ide.export", json!({ "path": zip })).unwrap();
    std::fs::write(&file, HIDDEN).unwrap();
    (dir, file, zip, registry, session)
}

fn assert_refused_for_an_agent(registry: &Registry, session: &mut Session, file: &Path, command: &str, params: Value) {
    let answer = zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params));
    let answer = format!("{answer:?}");
    assert!(answer.contains(crate::trust::AGENT_NOT_ALLOWED), "{command}: {answer}");
    assert_eq!(std::fs::read_to_string(file).unwrap(), HIDDEN, "{command} left the hidden files as they were");
}

#[test]
fn an_agent_cannot_import_settings_past_what_set_settings_refuses() {
    let (_dir, file, zip, registry, mut session) = hidden_files_fixture();
    assert_refused_for_an_agent(&registry, &mut session, &file, "settings.ide.import", json!({ "path": zip }));
    registry.dispatch(&mut session, "settings.ide.import", json!({ "path": zip })).expect("the person's import");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), NOT_HIDDEN);
}

#[test]
fn an_agent_cannot_restore_defaults_past_what_set_settings_refuses() {
    let (_dir, file, _zip, registry, mut session) = hidden_files_fixture();
    assert_refused_for_an_agent(&registry, &mut session, &file, "settings.ide.restoreDefaults", json!({}));
    registry.dispatch(&mut session, "settings.ide.restoreDefaults", json!({})).expect("the person's restore");
    assert!(!file.exists());
}

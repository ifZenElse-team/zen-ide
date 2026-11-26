use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zen_jb_import::IdeaProject;
use zen_jb_import::idea::is_idea_file;
use zen_project::settings::{CodeStyleOverride, Encodings, FileEncoding, ReadFiles, SettingsProvider};

struct Disk(PathBuf);

impl ReadFiles for Disk {
    fn read(&self, relative: &Path) -> Option<String> {
        std::fs::read_to_string(self.0.join(relative)).ok()
    }

    fn list(&self, relative_dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.0.join(relative_dir)) else { return Vec::new() };
        let mut listed: Vec<PathBuf> =
            entries.filter_map(Result::ok).map(|entry| relative_dir.join(entry.file_name())).collect();
        listed.sort();
        listed
    }
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/idea-project")
}

#[test]
fn the_sources_are_the_shared_project_files() {
    let root = fixture();
    let settings = IdeaProject::with_user_home(None).load(&root, &Disk(root.clone())).unwrap();
    let relative: Vec<String> = settings
        .sources
        .iter()
        .map(|source| source.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        relative,
        [
            ".idea/codeStyles/Project.xml",
            ".idea/codeStyles/codeStyleConfig.xml",
            ".idea/encodings.xml",
            ".idea/inspectionProfiles/Project_Default.xml",
            ".idea/inspectionProfiles/profiles_settings.xml",
            ".idea/modules.xml",
            ".idea/runConfigurations/Serve.xml",
            ".idea/scopes/Generated.xml",
            ".idea/scopes/Orchard_sources.xml",
            ".run/Unit_tests.run.xml",
            "orchard.iml",
            "tools/tools.iml",
        ]
    );
    assert!(settings.problems.is_empty(), "{:?}", settings.problems);
    for source in &settings.sources {
        let text = std::fs::read_to_string(source).unwrap();
        roxmltree::Document::parse(&text).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
        assert!(is_idea_file(source.strip_prefix(&root).unwrap()));
    }
}

#[test]
fn every_fixture_file_is_watched_but_the_personal_one() {
    let provider = IdeaProject::new();
    assert!(provider.watches(Path::new(".idea/scopes/Generated.xml")));
    assert!(provider.watches(Path::new("tools/tools.iml")));
    assert!(provider.watches(Path::new(".run/Unit_tests.run.xml")));
    assert!(!provider.watches(Path::new(".idea/workspace.xml")));
    assert!(!provider.watches(Path::new("main.go")));
    assert_eq!(provider.id(), "idea");
}

fn load(fixture_name: &str) -> (PathBuf, zen_project::settings::ProjectSettings) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture_name);
    let settings = IdeaProject::with_user_home(None).load(&root, &Disk(root.clone())).unwrap();
    (root, settings)
}

#[test]
fn excluded_folders_and_patterns_come_from_every_module() {
    let (root, settings) = load("idea-project");
    assert_eq!(settings.excluded, [root.join(".cache"), root.join("dist"), root.join("tools/bin")]);
    assert_eq!(settings.exclude_patterns, ["*.tmp"]);
}

#[test]
fn scopes_and_inspection_profiles_are_read_as_written() {
    let (root, settings) = load("idea-project");
    let scopes: Vec<(&str, &str)> =
        settings.scopes.iter().map(|scope| (scope.name.as_str(), scope.pattern.as_str())).collect();
    assert_eq!(
        scopes,
        [
            ("Generated", "file:*.pb.go"),
            ("Orchard sources", "file[orchard]:internal//*&&!file[orchard]:internal/testdata//*"),
        ]
    );
    assert_eq!(settings.inspection_profiles.len(), 1);
    let profile = &settings.inspection_profiles[0];
    assert_eq!(profile.name, "Project Default");
    assert_eq!(profile.file, root.join(".idea/inspectionProfiles/Project_Default.xml"));
    assert!(profile.is_project_profile);
    let tools: Vec<(&str, bool, &str)> =
        profile.tools.iter().map(|tool| (tool.class.as_str(), tool.enabled, tool.level.as_str())).collect();
    assert_eq!(tools, [("GoUnusedParameter", false, "WEAK WARNING"), ("GoShadowedVar", true, "ERROR")]);
}

#[test]
fn modules_in_subfolders_resolve_their_macros_and_broken_files_are_problems() {
    let (root, settings) = load("idea-edge");
    assert_eq!(settings.excluded, [root.join("logs"), root.join("svc/generated")]);
    assert_eq!(settings.exclude_patterns, ["*.log", "*.out"]);
    assert_eq!(settings.scopes.len(), 1);
    assert_eq!(settings.scopes[0].name, "Services");
    assert_eq!(settings.inspection_profiles.len(), 1);
    assert_eq!(settings.inspection_profiles[0].name, "Team Strict");
    assert!(!settings.inspection_profiles[0].is_project_profile, "the project uses the IDE's profile");
    let mut broken: Vec<PathBuf> = settings.problems.iter().map(|problem| problem.file.clone()).collect();
    broken.sort();
    assert_eq!(
        broken,
        [
            root.join(".idea/inspectionProfiles/Broken.xml"),
            root.join(".idea/scopes/Broken.xml"),
            root.join("broken/broken.iml"),
        ]
    );
    assert!(settings.problems.iter().all(|problem| problem.message.starts_with("cannot parse ")));
}

#[test]
fn the_code_style_and_the_encodings_are_read() {
    let root = fixture();
    let settings = IdeaProject::with_user_home(None).load(&root, &Disk(root.clone())).unwrap();
    let override_of = |use_tabs, tab_size, indent, continuation_indent, right_margin| CodeStyleOverride {
        use_tabs,
        tab_size,
        indent,
        continuation_indent,
        right_margin,
        visual_guides: None,
    };
    let expected = BTreeMap::from([
        ("default".to_owned(), override_of(None, None, None, None, Some(110))),
        ("go".to_owned(), override_of(Some(true), Some(6), Some(6), Some(12), None)),
        ("json".to_owned(), override_of(None, Some(3), Some(3), None, None)),
        ("markdown".to_owned(), override_of(None, None, None, None, Some(80))),
    ]);
    assert_eq!(settings.code_style, expected);
    let file = |path: &str, charset: &str| FileEncoding { path: root.join(path), charset: charset.to_owned() };
    assert_eq!(
        settings.encodings,
        Encodings {
            default: Some("UTF-8".to_owned()),
            files: vec![file("legacy", "windows-1251"), file("legacy/notes.txt", "KOI8-R")],
            bom_for_new_files: Some(zen_project::settings::NewFileBom::Without),
        }
    );
    assert_eq!(settings.encodings.charset_for(&root.join("legacy/greeting.txt")), Some("windows-1251"));
    assert!(std::str::from_utf8(&std::fs::read(root.join("legacy/greeting.txt")).unwrap()).is_err());
}

fn write(root: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

#[test]
fn a_project_scheme_is_read_only_when_the_project_uses_it() {
    let scheme = r#"<component name="ProjectCodeStyleConfiguration"><code_scheme name="Project">
        <codeStyleSettings language="yaml"><indentOptions><option name="INDENT_SIZE" value="4" /></indentOptions></codeStyleSettings>
    </code_scheme></component>"#;
    let config = |on: &str| {
        format!(
            r#"<component name="ProjectCodeStyleConfiguration"><state><option name="USE_PER_PROJECT_SETTINGS" value="{on}" /></state></component>"#
        )
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        &[(".idea/codeStyles/codeStyleConfig.xml", &config("false")), (".idea/codeStyles/Project.xml", scheme)],
    );
    let load = || IdeaProject::with_user_home(None).load(root, &Disk(root.to_owned())).unwrap();
    assert!(load().code_style.is_empty());
    assert!(load().problems.is_empty());
    write(root, &[(".idea/codeStyles/codeStyleConfig.xml", &config("true"))]);
    assert_eq!(load().code_style["yaml"].indent, Some(4));
    std::fs::remove_file(root.join(".idea/codeStyles/codeStyleConfig.xml")).unwrap();
    assert!(load().code_style.is_empty(), "without the configuration, the project's scheme is not in use");
}

#[test]
fn broken_files_are_problems_and_give_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        &[
            (
                ".idea/codeStyles/codeStyleConfig.xml",
                r#"<component name="ProjectCodeStyleConfiguration"><state><option name="USE_PER_PROJECT_SETTINGS" value="true" /></state></component>"#,
            ),
            (".idea/codeStyles/Project.xml", "<component name=\"ProjectCodeStyleConfiguration\"><code_scheme"),
            (".idea/encodings.xml", "<project><component name=\"Encoding\">"),
        ],
    );
    let settings = IdeaProject::with_user_home(None).load(root, &Disk(root.to_owned())).unwrap();
    assert!(settings.code_style.is_empty());
    assert_eq!(settings.encodings, Encodings::default());
    let files: Vec<PathBuf> = settings.problems.iter().map(|problem| problem.file.clone()).collect();
    assert_eq!(files, vec![root.join(".idea/codeStyles/Project.xml"), root.join(".idea/encodings.xml")]);
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().map(Result::unwrap) {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn exclude(root: &Path, folder: &str, excluded: bool) -> Result<zen_project::settings::ExclusionChange, String> {
    IdeaProject::with_user_home(None).set_excluded(root, &root.join(folder), excluded).unwrap()
}

fn excluded(root: &Path) -> Vec<PathBuf> {
    IdeaProject::with_user_home(None).load(root, &Disk(root.to_owned())).unwrap().excluded
}

const ORCHARD_LINE: &str = "      <excludeFolder url=\"file://$MODULE_DIR$/dist\" />\n";

#[test]
fn marking_a_folder_adds_one_line_to_its_module_and_cancelling_takes_it_away() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orchard");
    copy_tree(&fixture(), &root);
    std::fs::create_dir_all(root.join("gen/pb")).unwrap();
    std::fs::create_dir_all(root.join("tools/out")).unwrap();
    let original = std::fs::read_to_string(root.join("orchard.iml")).unwrap();
    let tools = std::fs::read_to_string(root.join("tools/tools.iml")).unwrap();
    let modules = std::fs::read_to_string(root.join(".idea/modules.xml")).unwrap();

    let change = exclude(&root, "gen", true).unwrap();
    assert!(change.changed);
    assert_eq!(change.files, [root.join("orchard.iml")]);
    let line = "      <excludeFolder url=\"file://$MODULE_DIR$/gen\" />\n";
    let cache = "      <excludeFolder url=\"file://$MODULE_DIR$/.cache\" />\n";
    let expected = original.replacen(cache, &format!("{cache}{line}"), 1);
    assert_eq!(std::fs::read_to_string(root.join("orchard.iml")).unwrap(), expected, "after the last one before it");
    assert_eq!(excluded(&root), [root.join(".cache"), root.join("dist"), root.join("gen"), root.join("tools/bin")]);
    assert!(!exclude(&root, "gen", true).unwrap().changed, "marked already");

    let change = exclude(&root, "tools/out", true).unwrap();
    assert_eq!(change.files, [root.join("tools/tools.iml")], "the module whose content root is the closest");
    let tools_line = "      <excludeFolder url=\"file://$MODULE_DIR$/out\" />\n";
    assert_eq!(
        std::fs::read_to_string(root.join("tools/tools.iml")).unwrap(),
        tools.replacen(
            "      <excludeFolder url=\"file://$MODULE_DIR$/bin\" />\n",
            &format!("      <excludeFolder url=\"file://$MODULE_DIR$/bin\" />\n{tools_line}"),
            1
        )
    );

    assert_eq!(exclude(&root, "gen", false).unwrap().files, [root.join("orchard.iml")]);
    assert_eq!(std::fs::read_to_string(root.join("orchard.iml")).unwrap(), original, "byte for byte");
    exclude(&root, "tools/out", false).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("tools/tools.iml")).unwrap(), tools);
    assert_eq!(std::fs::read_to_string(root.join(".idea/modules.xml")).unwrap(), modules);
    assert!(!exclude(&root, "gen", false).unwrap().changed, "nothing to cancel");

    exclude(&root, "dist", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("orchard.iml")).unwrap(),
        original.replacen(ORCHARD_LINE, "", 1),
        "a folder the JetBrains IDE excluded"
    );
    exclude(&root, "dist", true).unwrap_err();
    std::fs::create_dir(root.join("dist")).unwrap();
    exclude(&root, "dist", true).unwrap();
    let sorted = original.replacen(ORCHARD_LINE, "", 1).replacen(cache, &format!("{cache}{ORCHARD_LINE}"), 1);
    assert_eq!(std::fs::read_to_string(root.join("orchard.iml")).unwrap(), sorted);
}

#[test]
fn a_folder_sorting_first_goes_first_and_line_endings_are_kept() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let text = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<module type=\"WEB_MODULE\" version=\"4\">\r\n  <component name=\"NewModuleRootManager\">\r\n    <content url=\"file://$MODULE_DIR$\">\r\n      <sourceFolder url=\"file://$MODULE_DIR$/src\" isTestSource=\"false\" />\r\n      <excludeFolder url=\"file://$MODULE_DIR$/tmp\" />\r\n      <excludePattern pattern=\"*.log\" />\r\n    </content>\r\n  </component>\r\n</module>\r\n";
    write(root, &[("app.iml", text), ("assets/x.txt", "")]);
    exclude(root, "assets", true).unwrap();
    let line = "      <excludeFolder url=\"file://$MODULE_DIR$/assets\" />\r\n";
    let tmp = "      <excludeFolder url=\"file://$MODULE_DIR$/tmp\" />";
    assert_eq!(std::fs::read_to_string(root.join("app.iml")).unwrap(), text.replacen(tmp, &format!("{line}{tmp}"), 1));
    exclude(root, "assets", false).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("app.iml")).unwrap(), text);
}

#[test]
fn an_empty_content_root_opens_for_the_first_folder_and_closes_after_it() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let text = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<module type=\"WEB_MODULE\" version=\"4\">\n  <component name=\"Go\" enabled=\"true\" />\n  <component name=\"NewModuleRootManager\">\n    <content url=\"file://$MODULE_DIR$\" />\n    <orderEntry type=\"inheritedJdk\" />\n  </component>\n</module>\n";
    let modules = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <component name=\"ProjectModuleManager\">\n    <modules>\n      <module fileurl=\"file://$PROJECT_DIR$/.idea/shop.iml\" filepath=\"$PROJECT_DIR$/.idea/shop.iml\" />\n    </modules>\n  </component>\n</project>\n";
    write(root, &[(".idea/shop.iml", text), (".idea/modules.xml", modules), ("node_modules/x/index.js", "")]);
    exclude(root, "node_modules", true).unwrap();
    let opened = "    <content url=\"file://$MODULE_DIR$\">\n      <excludeFolder url=\"file://$MODULE_DIR$/node_modules\" />\n    </content>\n";
    assert_eq!(
        std::fs::read_to_string(root.join(".idea/shop.iml")).unwrap(),
        text.replacen("    <content url=\"file://$MODULE_DIR$\" />\n", opened, 1)
    );
    assert_eq!(excluded(root), [root.join("node_modules")], "$MODULE_DIR$ of .idea/shop.iml is the project");
    exclude(root, "node_modules", false).unwrap();
    assert_eq!(std::fs::read_to_string(root.join(".idea/shop.iml")).unwrap(), text);
}

#[test]
fn a_project_without_modules_gets_one_as_a_jetbrains_ide_makes_it() {
    for existing in [None, Some(".idea/misc.xml")] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("shop");
        write(&root, &[("main.go", "package main\n"), ("gen/x.go", "package gen\n")]);
        if let Some(file) = existing {
            write(&root, &[(file, "<project version=\"4\" />\n")]);
        }
        let change = exclude(&root, "gen", true).unwrap();
        assert_eq!(change.files, [root.join(".idea/shop.iml"), root.join(".idea/modules.xml")]);
        assert_eq!(
            std::fs::read_to_string(root.join(".idea/modules.xml")).unwrap(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <component name=\"ProjectModuleManager\">\n    <modules>\n      <module fileurl=\"file://$PROJECT_DIR$/.idea/shop.iml\" filepath=\"$PROJECT_DIR$/.idea/shop.iml\" />\n    </modules>\n  </component>\n</project>\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".idea/shop.iml")).unwrap(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<module type=\"WEB_MODULE\" version=\"4\">\n  <component name=\"NewModuleRootManager\">\n    <content url=\"file://$MODULE_DIR$\">\n      <excludeFolder url=\"file://$MODULE_DIR$/gen\" />\n    </content>\n    <orderEntry type=\"inheritedJdk\" />\n    <orderEntry type=\"sourceFolder\" forTests=\"false\" />\n  </component>\n</module>\n"
        );
        assert_eq!(excluded(&root), [root.join("gen")]);
        if let Some(file) = existing {
            assert_eq!(std::fs::read_to_string(root.join(file)).unwrap(), "<project version=\"4\" />\n");
        }
        exclude(&root, "gen", false).unwrap();
        assert!(excluded(&root).is_empty());
        assert!(
            std::fs::read_to_string(root.join(".idea/shop.iml"))
                .unwrap()
                .contains("<content url=\"file://$MODULE_DIR$\" />\n")
        );
    }
}

#[test]
fn what_cannot_be_marked_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(root, &[("main.go", "")]);
    assert!(exclude(root, "", true).is_err(), "the project itself");
    assert!(exclude(root, "missing", true).is_err());
    let outside = IdeaProject::with_user_home(None).set_excluded(root, Path::new("/elsewhere"), true).unwrap();
    assert!(outside.is_err());
    assert!(!root.join(".idea").exists(), "nothing was created");
    let modules = "<project version=\"4\"><component name=\"ProjectModuleManager\"><modules /></component></project>";
    write(root, &[(".idea/modules.xml", modules), ("gen/x.go", "")]);
    let error = exclude(root, "gen", true).unwrap_err();
    assert!(error.contains("no module"), "{error}");
    assert_eq!(std::fs::read_to_string(root.join(".idea/modules.xml")).unwrap(), modules);
}

#[test]
fn a_byte_order_mark_tabs_and_a_module_outside_dot_idea_are_kept() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shop");
    let svc = "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<module type=\"WEB_MODULE\" version=\"4\">\r\n\t<component name=\"NewModuleRootManager\">\r\n\t\t<content url=\"file://$MODULE_DIR$\">\r\n\t\t\t<excludeFolder url=\"file://$MODULE_DIR$/tmp\" />\r\n\t\t</content>\r\n\t\t<content url=\"file://$MODULE_DIR$/../shared\" />\r\n\t</component>\r\n</module>";
    let modules = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <component name=\"ProjectModuleManager\">\n    <modules>\n      <module fileurl=\"file://$PROJECT_DIR$/.idea/shop.iml\" filepath=\"$PROJECT_DIR$/.idea/shop.iml\" />\n      <module fileurl=\"file://$PROJECT_DIR$/svc/svc.iml\" filepath=\"$PROJECT_DIR$/svc/svc.iml\" />\n    </modules>\n  </component>\n</project>\n";
    let shop = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<module type=\"WEB_MODULE\" version=\"4\">\n  <component name=\"NewModuleRootManager\">\n    <content url=\"file://$MODULE_DIR$\" />\n  </component>\n</module>\n";
    write(
        &root,
        &[
            (".idea/modules.xml", modules),
            (".idea/shop.iml", shop),
            ("svc/svc.iml", svc),
            ("svc/gen/x.go", ""),
            ("shared/out/y.go", ""),
            ("docs/a.md", ""),
        ],
    );
    let change = exclude(&root, "svc/gen", true).unwrap();
    assert_eq!(change.files, [root.join("svc/svc.iml")], "the module closest to the folder");
    let marked = std::fs::read_to_string(root.join("svc/svc.iml")).unwrap();
    assert_eq!(
        marked,
        svc.replacen(
            "\t\t\t<excludeFolder url=\"file://$MODULE_DIR$/tmp\" />",
            "\t\t\t<excludeFolder url=\"file://$MODULE_DIR$/gen\" />\r\n\t\t\t<excludeFolder url=\"file://$MODULE_DIR$/tmp\" />",
            1
        ),
        "the byte order mark, tabs and CRLF stay"
    );
    exclude(&root, "shared/out", true).unwrap();
    assert!(
        std::fs::read_to_string(root.join("svc/svc.iml"))
            .unwrap()
            .contains("\t\t<content url=\"file://$MODULE_DIR$/../shared\">\r\n\t\t\t<excludeFolder url=\"file://$PROJECT_DIR$/shared/out\" />\r\n\t\t</content>"),
        "a content root outside the module's folder"
    );
    exclude(&root, "docs", true).unwrap();
    assert_eq!(
        excluded(&root),
        [root.join("docs"), root.join("shared/out"), root.join("svc/gen"), root.join("svc/tmp")]
    );
    for folder in ["svc/gen", "shared/out"] {
        exclude(&root, folder, false).unwrap();
    }
    assert_eq!(std::fs::read_to_string(root.join("svc/svc.iml")).unwrap(), svc, "byte for byte");
    exclude(&root, "docs", false).unwrap();
    assert_eq!(std::fs::read_to_string(root.join(".idea/shop.iml")).unwrap(), shop);
}

#[cfg(unix)]
#[test]
fn a_read_only_project_file_is_an_error_and_stays_as_it_was() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shop");
    let shop = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<module type=\"WEB_MODULE\" version=\"4\">\n  <component name=\"NewModuleRootManager\">\n    <content url=\"file://$MODULE_DIR$\" />\n  </component>\n</module>\n";
    write(&root, &[("shop.iml", shop), ("gen/x.go", "")]);
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
    let error = exclude(&root, "gen", true);
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(error.is_err(), "{error:?}");
    assert_eq!(std::fs::read_to_string(root.join("shop.iml")).unwrap(), shop);
    let names: Vec<_> = std::fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(names.len(), 2, "no temporary file is left: {names:?}");
    std::fs::set_permissions(root.join("shop.iml"), std::fs::Permissions::from_mode(0o444)).unwrap();
    let error = exclude(&root, "gen", true).unwrap_err();
    assert!(error.contains("read-only"), "{error}");
    assert_eq!(
        std::fs::read_to_string(root.join("shop.iml")).unwrap(),
        shop,
        "a read-only module file is not replaced"
    );

    let bare = temp.path().join("bare");
    write(&bare, &[("gen/x.go", "")]);
    std::fs::create_dir(bare.join(".idea")).unwrap();
    std::fs::set_permissions(bare.join(".idea"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let error = exclude(&bare, "gen", true);
    std::fs::set_permissions(bare.join(".idea"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(error.is_err(), "{error:?}");
    assert_eq!(std::fs::read_dir(bare.join(".idea")).unwrap().count(), 0);
}

fn resource(root: &Path, folder: &str, marked: bool) -> Result<zen_project::settings::ExclusionChange, String> {
    IdeaProject::with_user_home(None).set_resource_root(root, &root.join(folder), marked).unwrap()
}

fn patterns(root: &Path, patterns: &[&str]) -> Result<zen_project::settings::ExclusionChange, String> {
    let patterns: Vec<String> = patterns.iter().map(|pattern| (*pattern).to_owned()).collect();
    IdeaProject::with_user_home(None).set_exclude_patterns(root, &patterns).unwrap()
}

fn loaded(root: &Path) -> zen_project::settings::ProjectSettings {
    IdeaProject::with_user_home(None).load(root, &Disk(root.to_owned())).unwrap()
}

#[test]
fn a_resource_root_is_a_source_folder_line_before_the_exclusions_and_unmarking_takes_it_away() {
    use zen_project::settings::{SourceFolder, SourceFolderKind};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orchard");
    copy_tree(&fixture(), &root);
    for folder in ["static", "assets", "dist"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    let original = std::fs::read_to_string(root.join("orchard.iml")).unwrap();
    let first_exclusion = "      <excludeFolder url=\"file://$MODULE_DIR$/dist\" />\n";

    let change = resource(&root, "static", true).unwrap();
    assert_eq!(change.files, [root.join("orchard.iml")]);
    let line = "      <sourceFolder url=\"file://$MODULE_DIR$/static\" type=\"java-resource\" />\n";
    let marked = original.replacen(first_exclusion, &format!("{line}{first_exclusion}"), 1);
    assert_eq!(std::fs::read_to_string(root.join("orchard.iml")).unwrap(), marked, "before the excluded folders");
    resource(&root, "assets", true).unwrap();
    let assets = "      <sourceFolder url=\"file://$MODULE_DIR$/assets\" type=\"java-resource\" />\n";
    assert_eq!(
        std::fs::read_to_string(root.join("orchard.iml")).unwrap(),
        marked.replacen(line, &format!("{assets}{line}"), 1),
        "sorted by URL"
    );
    let settings = loaded(&root);
    assert_eq!(
        settings.source_folders,
        [
            SourceFolder { path: root.join("assets"), kind: SourceFolderKind::Resources },
            SourceFolder { path: root.join("static"), kind: SourceFolderKind::Resources },
        ]
    );
    assert!(settings.content_roots.contains(&root), "{:?}", settings.content_roots);
    assert!(!resource(&root, "static", true).unwrap().changed, "marked already");

    resource(&root, "assets", false).unwrap();
    resource(&root, "static", false).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("orchard.iml")).unwrap(), original, "byte for byte");
    assert!(!resource(&root, "static", false).unwrap().changed);

    resource(&root, "dist", true).unwrap();
    let text = std::fs::read_to_string(root.join("orchard.iml")).unwrap();
    assert!(!text.contains("excludeFolder url=\"file://$MODULE_DIR$/dist\""), "a folder has one mark: {text}");
    assert!(text.contains("<sourceFolder url=\"file://$MODULE_DIR$/dist\" type=\"java-resource\" />"));
    exclude(&root, "dist", true).unwrap();
    let text = std::fs::read_to_string(root.join("orchard.iml")).unwrap();
    assert!(!text.contains("sourceFolder url=\"file://$MODULE_DIR$/dist\""), "{text}");
    assert!(text.contains("<excludeFolder url=\"file://$MODULE_DIR$/dist\" />"), "{text}");
    assert!(loaded(&root).excluded.contains(&root.join("dist")));
}

#[test]
fn a_sources_root_becomes_a_resource_root_and_test_resources_count_as_resources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let text = "<module type=\"JAVA_MODULE\" version=\"4\">\n  <component name=\"NewModuleRootManager\">\n    <content url=\"file://$MODULE_DIR$\">\n      <sourceFolder url=\"file://$MODULE_DIR$/res\" isTestSource=\"false\" />\n      <sourceFolder url=\"file://$MODULE_DIR$/fixtures\" type=\"java-test-resource\" />\n    </content>\n  </component>\n</module>\n";
    write(root, &[("app.iml", text), ("res/a.txt", ""), ("fixtures/b.txt", "")]);
    resource(root, "res", true).unwrap();
    let fixtures = "      <sourceFolder url=\"file://$MODULE_DIR$/fixtures\" type=\"java-test-resource\" />\n";
    let res = "      <sourceFolder url=\"file://$MODULE_DIR$/res\" type=\"java-resource\" />\n";
    let sources = "      <sourceFolder url=\"file://$MODULE_DIR$/res\" isTestSource=\"false\" />\n";
    assert_eq!(
        std::fs::read_to_string(root.join("app.iml")).unwrap(),
        text.replacen(sources, "", 1).replacen(fixtures, &format!("{fixtures}{res}"), 1)
    );
    resource(root, "fixtures", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("app.iml")).unwrap(),
        "<module type=\"JAVA_MODULE\" version=\"4\">\n  <component name=\"NewModuleRootManager\">\n    <content url=\"file://$MODULE_DIR$\">\n      <sourceFolder url=\"file://$MODULE_DIR$/res\" type=\"java-resource\" />\n    </content>\n  </component>\n</module>\n"
    );
}

#[test]
fn the_exclude_files_patterns_are_one_line_each_and_the_rest_stays_byte_for_byte() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orchard");
    copy_tree(&fixture(), &root);
    let original = std::fs::read_to_string(root.join("orchard.iml")).unwrap();
    let tools = std::fs::read_to_string(root.join("tools/tools.iml")).unwrap();
    let before = loaded(&root).exclude_patterns;
    assert_eq!(before, ["*.tmp"]);

    assert!(!patterns(&root, &before.iter().map(String::as_str).collect::<Vec<_>>()).unwrap().changed, "unchanged");
    let mut wanted: Vec<&str> = before.iter().map(String::as_str).collect();
    wanted.extend(["*.bak", "tmp?"]);
    let change = patterns(&root, &wanted).unwrap();
    assert_eq!(change.files, [root.join("orchard.iml")], "into the module of the project's root");
    let tmp = "      <excludePattern pattern=\"*.tmp\" />\n";
    assert_eq!(
        std::fs::read_to_string(root.join("orchard.iml")).unwrap(),
        original.replacen(
            tmp,
            &format!("{tmp}      <excludePattern pattern=\"*.bak\" />\n      <excludePattern pattern=\"tmp?\" />\n"),
            1
        )
    );
    let mut expected = before.clone();
    expected.extend(["*.bak".to_owned(), "tmp?".to_owned()]);
    let mut read = loaded(&root).exclude_patterns;
    read.sort();
    expected.sort();
    assert_eq!(read, expected);

    patterns(&root, &before.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("orchard.iml")).unwrap(), original, "byte for byte");
    assert_eq!(std::fs::read_to_string(root.join("tools/tools.iml")).unwrap(), tools);

    assert!(patterns(&root, &["bin/*"]).unwrap_err().contains("name pattern"), "names only");
    patterns(&root, &[]).unwrap();
    assert!(loaded(&root).exclude_patterns.is_empty());
    assert!(!std::fs::read_to_string(root.join("orchard.iml")).unwrap().contains("excludePattern"));
}

#[test]
fn a_pattern_line_with_several_names_is_kept_while_all_of_them_are_and_a_project_without_modules_gets_one() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shop");
    let text = "<module type=\"WEB_MODULE\" version=\"4\">\r\n  <component name=\"NewModuleRootManager\">\r\n    <content url=\"file://$MODULE_DIR$\">\r\n      <excludePattern pattern=\"*.log;*.bak\" />\r\n    </content>\r\n  </component>\r\n</module>\r\n";
    write(&root, &[("shop.iml", text)]);
    assert!(!patterns(&root, &["*.bak", "*.log"]).unwrap().changed);
    patterns(&root, &["*.log", "*.swp"]).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("shop.iml")).unwrap(),
        text.replacen(
            "      <excludePattern pattern=\"*.log;*.bak\" />\r\n",
            "      <excludePattern pattern=\"*.log\" />\r\n      <excludePattern pattern=\"*.swp\" />\r\n",
            1
        ),
        "CRLF kept"
    );

    let bare = temp.path().join("bare");
    write(&bare, &[("main.go", "package main\n")]);
    let change = patterns(&bare, &["*.log"]).unwrap();
    assert_eq!(change.files, [bare.join(".idea/bare.iml"), bare.join(".idea/modules.xml")]);
    assert!(std::fs::read_to_string(bare.join(".idea/bare.iml")).unwrap().contains(
        "    <content url=\"file://$MODULE_DIR$\">\n      <excludePattern pattern=\"*.log\" />\n    </content>\n"
    ));
    assert_eq!(loaded(&bare).exclude_patterns, ["*.log"]);
    resource(&bare, "", true).unwrap_err();
}

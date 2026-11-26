use std::collections::BTreeMap;
use std::path::PathBuf;

use zen_core::keymap::{ImportWarning, Keymap, KeymapImporter, Keystroke, Unmapped, UnmappedReason};
use zen_jb_import::{JetBrainsKeymapImporter, KEYMAP_ACTIONS, MACOS_PARENT, keymap_importers};

const BEGIN: &str = "<!-- keymap-actions:begin -->\n";
const END: &str = "<!-- keymap-actions:end -->\n";

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/keymaps").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn keys(texts: &[&str]) -> Vec<Keystroke> {
    texts.iter().map(|text| Keystroke::parse(text).unwrap_or_else(|| panic!("{text}"))).collect()
}

fn unmapped(action_id: &str, reason: UnmappedReason, shortcut: Option<&str>) -> Unmapped {
    Unmapped { action_id: action_id.to_owned(), reason, shortcut: shortcut.map(str::to_owned) }
}

#[test]
fn a_macos_based_keymap_becomes_changes_of_our_macos_keymap() {
    let report =
        JetBrainsKeymapImporter.import(&fixture("synthetic-macos.xml")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.name, "Fixture Keys");
    assert_eq!(report.parent.as_deref(), Some(MACOS_PARENT));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let expected: BTreeMap<String, Vec<Keystroke>> = [
        ("editor.line.duplicate", keys(&["⇧⌘D"])),
        ("navigate.declaration.goTo", keys(&["⌃B", "⌘B"])),
        ("editor.comment.line", Vec::new()),
        ("editor.code.reformat", keys(&["⌥⌘L", "⌘K, ⌘F"])),
        ("view.gotoFile.open", keys(&["⇧⌘N"])),
        ("editor.caret.keepPrimary", Vec::new()),
    ]
    .into_iter()
    .map(|(action, keys)| (action.to_owned(), keys))
    .collect();
    assert_eq!(report.shortcuts, expected);
    assert_eq!(report.mapped, 7);
    assert_eq!(
        report.unmapped,
        [
            unmapped("GotoDeclaration", UnmappedReason::MouseShortcut, Some("meta button1")),
            unmapped("EditorEscape", UnmappedReason::UnknownKey, Some("meta NUMPAD5")),
            unmapped("Fixture.NoSuchAction", UnmappedReason::UnknownAction, None),
        ]
    );

    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.shortcut("editor.line.duplicate").as_deref(), Some("⇧⌘D"));
    assert!(keymap.keystrokes("editor.comment.line").is_empty(), "an action without shortcuts loses them");
    assert_eq!(keymap.shortcut("editor.line.delete").as_deref(), Some("⌘⌫"), "actions not listed keep ours");
}

#[test]
fn another_parent_is_applied_over_our_macos_keymap_with_a_warning() {
    let report = JetBrainsKeymapImporter
        .import(&fixture("synthetic-other-parent.xml"))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.warnings, [ImportWarning::ForeignParent { parent: Some("Fixture Base".to_owned()) }]);
    assert_eq!(report.shortcuts.get("view.settings.open"), Some(&keys(&["⌃⌥S"])));
    let json = serde_json::to_value(&report).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(json["warnings"][0], serde_json::json!({ "kind": "foreignParent", "parent": "Fixture Base" }));
}

#[test]
fn only_keymap_files_are_detected() {
    let importers = keymap_importers();
    assert_eq!(importers.len(), 1);
    assert!(importers[0].detects(&fixture("synthetic-macos.xml")));
    assert!(!importers[0].detects(&fixture("synthetic-not-a-keymap.xml")));
    assert!(!importers[0].detects("{}"));
    let error = JetBrainsKeymapImporter.import(&fixture("synthetic-not-a-keymap.xml")).unwrap_err();
    assert!(error.contains("<scheme>"), "{error}");
    assert!(JetBrainsKeymapImporter.import("<keymap").is_err());
}

#[test]
fn the_report_is_camel_case() {
    let report =
        JetBrainsKeymapImporter.import(&fixture("synthetic-macos.xml")).unwrap_or_else(|error| panic!("{error}"));
    let json = serde_json::to_value(&report).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        json["unmapped"][2],
        serde_json::json!({ "actionId": "Fixture.NoSuchAction", "reason": "unknownAction" })
    );
    assert_eq!(json["shortcuts"]["editor.line.duplicate"], serde_json::json!(["⇧⌘D"]));
}

#[test]
fn every_mapped_action_exists_in_our_keymap() {
    let actions: Vec<String> = Keymap::default_macos().bindings().into_iter().map(|binding| binding.action).collect();
    let mut seen = Vec::new();
    for (jetbrains, ours) in KEYMAP_ACTIONS {
        assert!(actions.iter().any(|action| action == ours), "{jetbrains} → {ours} is not an action");
        assert!(!seen.contains(jetbrains), "{jetbrains} is listed twice");
        seen.push(*jetbrains);
    }
}

#[test]
fn the_documented_mapping_table_is_current() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/jetbrains-compatibility.md");
    let doc = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut table = String::from("\n| JetBrains action | Zen IDE action |\n|---|---|\n");
    for (jetbrains, ours) in KEYMAP_ACTIONS {
        table.push_str(&format!("| `{jetbrains}` | `{ours}` |\n"));
    }
    table.push('\n');
    let start = doc.find(BEGIN).expect("the keymap-actions:begin marker") + BEGIN.len();
    let end = doc.find(END).expect("the keymap-actions:end marker");
    let updated = format!("{}{table}{}", &doc[..start], &doc[end..]);
    if std::env::var_os("ZEN_KEYMAP_BLESS").is_some_and(|value| value == "1") {
        std::fs::write(&path, &updated).unwrap_or_else(|error| panic!("{error}"));
    } else {
        assert!(
            updated == doc,
            "docs/jetbrains-compatibility.md is out of date; run `ZEN_KEYMAP_BLESS=1 cargo test -p zen-jb-import --test keymap`"
        );
    }
}

#[test]
fn run_actions_are_imported() {
    let text = r#"<!-- Synthetic. -->
<keymap version="1" name="Runner" parent="Mac OS X 10.5+">
  <action id="Run"><keyboard-shortcut first-keystroke="shift F10" /></action>
  <action id="RunClass"><keyboard-shortcut first-keystroke="control shift F10" /></action>
  <action id="Stop"><keyboard-shortcut first-keystroke="control F2" /></action>
  <action id="ActivateRunToolWindow"><keyboard-shortcut first-keystroke="alt 4" /></action>
  <action id="editRunConfigurations"><keyboard-shortcut first-keystroke="control alt R" /></action>
</keymap>
"#;
    let report = JetBrainsKeymapImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.shortcut("run.process.start").as_deref(), Some("⇧F10"));
    assert_eq!(keymap.shortcut("run.process.start/context").as_deref(), Some("⌃⇧F10"));
    assert_eq!(keymap.shortcut("run.process.stop").as_deref(), Some("⌃F2"));
    assert_eq!(keymap.shortcut("view.toolWindow.run").as_deref(), Some("⌥4"));
    assert_eq!(keymap.shortcut("view.runConfigurations.open").as_deref(), Some("⌃⌥R"));
}

#[test]
fn extend_and_shrink_selection_are_imported() {
    let text = r#"<!-- Synthetic. -->
<keymap version="1" name="Classic keys" parent="Mac OS X 10.5+">
  <action id="EditorSelectWord"><keyboard-shortcut first-keystroke="meta W" /></action>
  <action id="EditorUnSelectWord"><keyboard-shortcut first-keystroke="shift meta W" /></action>
</keymap>
"#;
    let report = JetBrainsKeymapImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.shortcut("editor.selection.extend").as_deref(), Some("⌘W"));
    assert_eq!(keymap.shortcut("editor.selection.shrink").as_deref(), Some("⇧⌘W"));
}

#[test]
fn unwrap_is_imported() {
    let text = r#"<!-- Synthetic. -->
<keymap version="1" name="Unwrapper" parent="Mac OS X 10.5+">
  <action id="Unwrap"><keyboard-shortcut first-keystroke="control shift DELETE" /></action>
</keymap>
"#;
    let report = JetBrainsKeymapImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.shortcut("view.unwrap.open").as_deref(), Some("⌃⇧⌦"));
}

#[test]
fn surround_actions_are_imported() {
    let text = r#"<!-- Synthetic. -->
<keymap version="1" name="Surround" parent="Mac OS X 10.5+">
  <action id="SurroundWith"><keyboard-shortcut first-keystroke="control alt T" /></action>
  <action id="SurroundWithLiveTemplate"><keyboard-shortcut first-keystroke="control alt J" /></action>
</keymap>
"#;
    let report = JetBrainsKeymapImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.shortcut("view.surround.open").as_deref(), Some("⌃⌥T"));
    assert_eq!(keymap.shortcut("view.surroundWithTemplate.open").as_deref(), Some("⌃⌥J"));
}

#[test]
fn zoom_ide_actions_are_imported() {
    let text = r#"<!-- Synthetic. -->
<keymap version="1" name="Zoomer" parent="Mac OS X 10.5+">
  <action id="ZoomInIdeAction"><keyboard-shortcut first-keystroke="control alt EQUALS" /></action>
  <action id="ZoomOutIdeAction"><keyboard-shortcut first-keystroke="control alt MINUS" /></action>
  <action id="ResetIdeScaleAction"><keyboard-shortcut first-keystroke="control alt 0" /></action>
</keymap>
"#;
    let report = JetBrainsKeymapImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.keystrokes("view.ideZoom.in"), keys(&["⌃⌥="]));
    assert_eq!(keymap.keystrokes("view.ideZoom.out"), keys(&["⌃⌥-"]));
    assert_eq!(keymap.keystrokes("view.ideZoom.reset"), keys(&["⌃⌥0"]));
}

#[test]
fn folding_actions_are_imported_with_their_two_keystrokes() {
    let text = r#"<!-- Synthetic. -->
<keymap version="1" name="Folds" parent="Mac OS X 10.5+">
  <action id="ExpandRegionRecursively"><keyboard-shortcut first-keystroke="meta alt ADD" /></action>
  <action id="CollapseRegionRecursively"><keyboard-shortcut first-keystroke="meta alt SUBTRACT" /></action>
  <action id="ExpandToLevel2"><keyboard-shortcut first-keystroke="meta MULTIPLY" second-keystroke="2" /></action>
  <action id="ExpandAllToLevel4"><keyboard-shortcut first-keystroke="meta alt MULTIPLY" second-keystroke="4" /></action>
  <action id="ExpandDocComments"><keyboard-shortcut first-keystroke="control alt E" /></action>
  <action id="CollapseDocComments"><keyboard-shortcut first-keystroke="meta K" second-keystroke="meta NUMPAD5" /></action>
</keymap>
"#;
    let report = JetBrainsKeymapImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.mapped, 6);
    assert_eq!(
        report.unmapped,
        [unmapped("CollapseDocComments", UnmappedReason::UnknownKey, Some("meta K, meta NUMPAD5"))]
    );
    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.shortcut("editor.fold.expandRecursively").as_deref(), Some("⌥⌘+"));
    assert_eq!(keymap.shortcut("editor.fold.collapseRecursively").as_deref(), Some("⌥⌘−"));
    assert_eq!(keymap.shortcut("editor.fold.expandToLevel/2").as_deref(), Some("⌘NumPad*, 2"));
    assert_eq!(keymap.shortcut("editor.fold.expandAllToLevel/4").as_deref(), Some("⌥⌘NumPad*, 4"));
    assert_eq!(keymap.shortcut("editor.fold.expandDocComments").as_deref(), Some("⌃⌥E"));
    assert!(keymap.keystrokes("editor.fold.collapseDocComments").is_empty());
    for (jetbrains, ours) in [
        ("ExpandToLevel1", "editor.fold.expandToLevel/1"),
        ("ExpandToLevel5", "editor.fold.expandToLevel/5"),
        ("ExpandAllToLevel1", "editor.fold.expandAllToLevel/1"),
        ("ExpandAllToLevel5", "editor.fold.expandAllToLevel/5"),
    ] {
        assert!(KEYMAP_ACTIONS.contains(&(jetbrains, ours)), "{jetbrains}");
    }
}

#[test]
fn fold_selection_and_custom_folding_are_imported() {
    let text = r#"<!-- Synthetic. -->
<keymap version="1" name="Regions" parent="Mac OS X 10.5+">
  <action id="CollapseSelection"><keyboard-shortcut first-keystroke="control PERIOD" /></action>
  <action id="GotoCustomRegion"><keyboard-shortcut first-keystroke="control alt PERIOD" /></action>
</keymap>
"#;
    let report = JetBrainsKeymapImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((report.mapped, report.unmapped.len()), (2, 0), "{:?}", report.unmapped);
    let keymap = Keymap::default_macos().with_user_changes(report.shortcuts);
    assert_eq!(keymap.shortcut("editor.fold.selection").as_deref(), Some("⌃."));
    assert_eq!(keymap.shortcut("view.customFolding.open").as_deref(), Some("⌃⌥."));
}

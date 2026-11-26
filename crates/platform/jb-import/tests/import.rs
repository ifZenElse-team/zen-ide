use std::path::{Path, PathBuf};

use zen_jb_import::{IclsImporter, ThemeJsonImporter, importers};
use zen_theme::{
    EditorKey, ImportReport, Rgba, Style, SyntaxKey, ThemeImporter, UiKey, Underline, ZEN_DARK_ID, ZEN_LIGHT_SYNTAX,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn hex(text: &str) -> Rgba {
    text.parse().unwrap_or_else(|error| panic!("{error}"))
}

fn plain(text: &str) -> Style {
    Style::color(hex(text))
}

fn icls(name: &str) -> ImportReport {
    IclsImporter.import(&fixture(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn assert_resolves_cleanly(report: &ImportReport) {
    let resolution = report.theme.resolve(|_| None);
    assert!(resolution.warnings.is_empty(), "{:?}", resolution.warnings);
}

#[test]
fn every_scheme_colour_and_attribute_row_is_mapped() {
    let report = icls("every-key.icls");
    let theme = &report.theme;
    assert_eq!(theme.name, "Fixture Night");
    assert_eq!(theme.id, "fixture-night");
    assert!(theme.dark);
    assert_eq!(theme.parent.as_deref(), Some(ZEN_DARK_ID));

    let editor = [
        (EditorKey::Caret, "#C1C1C1"),
        (EditorKey::CurrentLine, "#C2C2C2"),
        (EditorKey::Selection, "#C3C3C3"),
        (EditorKey::LineNumber, "#C4C4C4"),
        (EditorKey::Foreground, "#A0A0A0"),
        (EditorKey::Background, "#101010"),
        (EditorKey::SearchMatch, "#B10001"),
        (EditorKey::SearchMark, "#B10002"),
        (EditorKey::FoldMarker, "#B10003"),
        (EditorKey::FoldText, "#B10009"),
        (EditorKey::InlayHintBackground, "#B1000A"),
        (EditorKey::InlayHintForeground, "#B1000B"),
        (EditorKey::SymbolHighlight, "#B10004"),
        (EditorKey::SymbolHighlightWrite, "#B100E4"),
        (EditorKey::BracketMatchedForeground, "#B1000A"),
        (EditorKey::BracketMatchedBackground, "#B1000B"),
        (EditorKey::BracketUnmatchedForeground, "#B1000C"),
        (EditorKey::BracketUnmatchedBackground, "#B1000D"),
        (EditorKey::DiagnosticError, "#B10005"),
        (EditorKey::DiagnosticWarning, "#B10006"),
        (EditorKey::DiagnosticInfo, "#B10007"),
        (EditorKey::DiagnosticHint, "#B10008"),
        (EditorKey::DiagnosticTypo, "#B1000F"),
        (EditorKey::Unused, "#B1000E"),
    ];
    for (key, color) in editor {
        assert_eq!(theme.editor.get(&key), Some(&hex(color)), "editor.{key}");
    }
    assert_eq!(theme.editor.len(), editor.len());

    let bold = |text: &str| Style { bold: true, ..plain(text) };
    let syntax = [
        (SyntaxKey::Default, plain("#A0A0A0")),
        (SyntaxKey::Keyword, bold("#A10001")),
        (SyntaxKey::String, plain("#A10002")),
        (SyntaxKey::Number, plain("#A10003")),
        (SyntaxKey::Comment, plain("#A10004").italic()),
        (SyntaxKey::DocComment, bold("#A10006").italic()),
        (SyntaxKey::Function, plain("#A10007")),
        (SyntaxKey::SemanticCall, plain("#A10008")),
        (SyntaxKey::Type, plain("#A10009")),
        (SyntaxKey::Constant, plain("#A1000B")),
        (SyntaxKey::Attribute, plain("#A1000C")),
        (SyntaxKey::Lifetime, plain("#A1000D")),
        (SyntaxKey::Punctuation, plain("#A1000E")),
    ];
    for (key, style) in syntax {
        assert_eq!(theme.syntax.get(&key), Some(&style), "syntax.{key}");
    }
    assert_eq!(theme.syntax.len(), syntax.len());
    assert!(theme.ui.is_empty() && theme.terminal.is_empty());

    assert_eq!(report.mapped, 39);
    assert_eq!(
        report.unmapped,
        ["LINE_SPACING", "GUTTER_BACKGROUND", "ADDED_LINES_COLOR", "DEFAULT_IDENTIFIER", "CUSTOM_KEYWORD1_ATTRIBUTES"]
    );
    assert_resolves_cleanly(&report);
}

#[test]
fn base_attributes_chains_padding_and_font_types() {
    let report = icls("chains.icls");
    let theme = &report.theme;
    assert!(!theme.dark, "a scheme based on the light default is light");
    assert_eq!(theme.parent.as_deref(), Some(zen_theme::ZEN_LIGHT_ID), "a light scheme builds on Zen Light");

    assert_eq!(theme.editor.get(&EditorKey::Caret), Some(&hex("#000000")));
    assert_eq!(theme.editor.get(&EditorKey::LineNumber), Some(&hex("#0000FF")));
    assert_eq!(theme.editor.get(&EditorKey::Selection), Some(&hex("#012345")));
    assert_eq!(theme.editor.get(&EditorKey::CurrentLine), None);

    let number = Style { strikethrough: true, ..plain("#ABCDEF").italic() };
    assert_eq!(theme.syntax.get(&SyntaxKey::Number), Some(&number));
    assert_eq!(theme.syntax.get(&SyntaxKey::Constant), Some(&number), "two links of baseAttributes");
    assert_eq!(theme.syntax.get(&SyntaxKey::Keyword), None, "a cycle gives nothing");
    assert_eq!(theme.syntax.get(&SyntaxKey::String), None, "a base outside the file gives nothing");

    let metadata = theme.syntax.get(&SyntaxKey::Attribute).copied();
    let expected = Style {
        background: Some(hex("#222222")),
        ..Style::color(ZEN_LIGHT_SYNTAX[SyntaxKey::Attribute as usize].1.color)
    };
    assert_eq!(
        metadata,
        Some(expected),
        "no foreground keeps the light base's; an effect without a colour is not drawn"
    );

    for name in ["CARET_ROW_COLOR", "DEFAULT_KEYWORD", "LOOP_A", "LOOP_B", "DEFAULT_STRING", "MIDDLE"] {
        assert!(report.unmapped.iter().any(|entry| entry == name), "{name} in {:?}", report.unmapped);
    }
    assert_resolves_cleanly(&report);
}

#[test]
fn effect_types_become_underlines() {
    let scheme = |effect: u8| {
        format!(
            r#"<scheme name="E" parent_scheme="Darcula"><attributes><option name="DEFAULT_KEYWORD"><value>
            <option name="FOREGROUND" value="102030"/><option name="EFFECT_COLOR" value="ff0000"/>
            <option name="EFFECT_TYPE" value="{effect}"/></value></option></attributes></scheme>"#
        )
    };
    let style = |effect: u8| {
        let report = IclsImporter.import(&scheme(effect)).unwrap_or_else(|error| panic!("{error}"));
        report.theme.syntax[&SyntaxKey::Keyword]
    };
    assert_eq!(style(0), plain("#102030"));
    assert_eq!(style(1).underline, Some(Underline::Line));
    assert_eq!(style(2).underline, Some(Underline::Wave));
    assert!(style(3).strikethrough && style(3).underline.is_none());
    assert_eq!(style(4).underline, Some(Underline::Line));
}

#[test]
fn the_ctrl_click_link_becomes_the_navigation_link() {
    let scheme = r#"<scheme name="L" parent_scheme="Darcula"><attributes><option name="CTRL_CLICKABLE"><value>
        <option name="FOREGROUND" value="5599ff"/><option name="EFFECT_COLOR" value="5599ff"/>
        <option name="EFFECT_TYPE" value="1"/></value></option></attributes></scheme>"#;
    let report = IclsImporter.import(scheme).unwrap_or_else(|error| panic!("{error}"));
    let link = report.theme.syntax[&SyntaxKey::NavigationLink];
    assert_eq!(link.color, hex("#5599FF"));
    assert_eq!(link.underline, Some(Underline::Line));
    assert!(!report.unmapped.iter().any(|entry| entry == "CTRL_CLICKABLE"), "{:?}", report.unmapped);
}

#[test]
fn malformed_schemes_are_errors_not_panics() {
    for text in [fixture("broken.icls"), String::new(), "<other name=\"x\"/>".to_owned(), "not xml".to_owned()] {
        assert!(IclsImporter.import(&text).is_err(), "{text:?}");
    }
    for name in ["every-key.icls", "chains.icls", "ember/Ember.xml", "ember/Ember.theme.json"] {
        let text = fixture(name);
        for end in (0..text.len()).filter(|end| text.is_char_boundary(*end)) {
            let _ = IclsImporter.import(&text[..end]);
            let _ = ThemeJsonImporter.import(&text[..end]);
        }
    }
}

#[test]
fn ui_themes_map_wildcards_named_colours_and_exact_keys() {
    let report = ThemeJsonImporter.import(&fixture("ember/Ember.theme.json")).unwrap_or_else(|error| panic!("{error}"));
    let theme = &report.theme;
    assert_eq!(theme.name, "Ember Test");
    assert!(theme.dark);
    let ui = [
        (UiKey::WindowBackground, "#141414"),
        (UiKey::IslandBackground, "#181818"),
        (UiKey::TextPrimary, "#E0E0E0"),
        (UiKey::TextError, "#FF5050"),
        (UiKey::Link, "#E06040"),
        (UiKey::Accent, "#C04020"),
        (UiKey::AccentForeground, "#FFFFFF"),
        (UiKey::ListHover, "#262626"),
        (UiKey::ListSelection, "#303A50"),
        (UiKey::PopupBackground, "#202020"),
        (UiKey::PopupBorder, "#404040"),
        (UiKey::PopupHover, "#303A50"),
        (UiKey::TextFieldBackground, "#1A1A1A"),
        (UiKey::TextFieldSelection, "#303A50"),
        (UiKey::ButtonSecondaryBorder, "#505050"),
        (UiKey::TabActiveBorder, "#C04020"),
    ];
    for (key, color) in ui {
        assert_eq!(theme.ui.get(&key), Some(&hex(color)), "ui.{key}");
    }
    assert_eq!(theme.ui.len(), ui.len());
    assert_eq!(report.mapped, ui.len());
    assert!(theme.editor.is_empty() && theme.syntax.is_empty());
    assert_eq!(report.unmapped, ["icons", "Button.arc", "Toolbar.noSuchColor", "Tree.rowHeight", "editorScheme"]);
    assert_resolves_cleanly(&report);
}

#[test]
fn every_ui_mapping_row_reads_its_exact_key() {
    let rows = [
        ("Panel.background", UiKey::WindowBackground),
        ("ToolWindow.background", UiKey::IslandBackground),
        ("Label.foreground", UiKey::TextPrimary),
        ("Component.infoForeground", UiKey::TextSecondary),
        ("Label.errorForeground", UiKey::TextError),
        ("Link.activeForeground", UiKey::Link),
        ("Button.default.startBackground", UiKey::Accent),
        ("Button.default.foreground", UiKey::AccentForeground),
        ("List.hoverBackground", UiKey::ListHover),
        ("Tree.selectionBackground", UiKey::ListSelection),
        ("Popup.background", UiKey::PopupBackground),
        ("Popup.borderColor", UiKey::PopupBorder),
        ("MenuItem.selectionBackground", UiKey::PopupHover),
        ("TextField.background", UiKey::TextFieldBackground),
        ("Component.borderColor", UiKey::TextFieldBorder),
        ("TextField.selectionBackground", UiKey::TextFieldSelection),
        ("TextField.caretForeground", UiKey::TextFieldCaret),
        ("Button.startBorderColor", UiKey::ButtonSecondaryBorder),
        ("Borders.color", UiKey::TabBorder),
        ("EditorTabs.underlinedTabBackground", UiKey::TabActiveBackground),
        ("EditorTabs.underlineColor", UiKey::TabActiveBorder),
    ];
    let fallbacks = [
        ("Editor.background", UiKey::IslandBackground),
        ("Label.infoForeground", UiKey::TextSecondary),
        ("Tree.hoverBackground", UiKey::ListHover),
        ("List.selectionBackground", UiKey::ListSelection),
    ];
    for (index, (source, key)) in rows.iter().chain(fallbacks.iter()).enumerate() {
        let color = format!("#{:06X}", 0x100000 + index);
        let text = format!(r#"{{"name": "Row", "dark": false, "ui": {{"{source}": "{color}"}}}}"#);
        let report = ThemeJsonImporter.import(&text).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(report.theme.ui.get(key), Some(&hex(&color)), "{source}");
        assert_eq!(report.theme.ui.len(), 1, "{source}");
        assert!(report.unmapped.is_empty(), "{source}: {:?}", report.unmapped);
        assert!(!report.theme.dark);
    }
}

#[test]
fn the_sibling_editor_scheme_is_merged() {
    let path = fixture_path("ember/Ember.theme.json");
    let text = fixture("ember/Ember.theme.json");
    assert_eq!(ThemeJsonImporter.scheme_path(&path, &text), Some(fixture_path("ember/Ember.xml")));

    let report = ThemeJsonImporter.import_file(&path).unwrap_or_else(|error| panic!("{error}"));
    let theme = &report.theme;
    assert_eq!(theme.name, "Ember Test");
    assert_eq!(theme.ui.get(&UiKey::Accent), Some(&hex("#C04020")));
    assert_eq!(theme.editor.get(&EditorKey::Caret), Some(&hex("#E06040")));
    assert_eq!(theme.editor.get(&EditorKey::Background), Some(&hex("#121212")));
    assert_eq!(theme.syntax.get(&SyntaxKey::Default), Some(&plain("#D8D8D8")));
    assert_eq!(theme.syntax.get(&SyntaxKey::Keyword), Some(&Style { bold: true, ..plain("#C04020") }));
    assert_eq!(report.mapped, 16 + 3);
    assert!(!report.unmapped.iter().any(|entry| entry == "editorScheme"));
    assert!(report.unmapped.iter().any(|entry| entry == "UNKNOWN_FIXTURE_ATTRIBUTE"));
    assert_resolves_cleanly(&report);
}

#[test]
fn a_scheme_with_another_extension_or_none_at_all() {
    let folder = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let theme = folder.path().join("Mine.theme.json");
    let json = r##"{"name": "Mine", "dark": true, "editorScheme": "/schemes/Mine.xml", "ui": {"Panel.background": "#010203"}}"##;
    std::fs::write(&theme, json).unwrap_or_else(|error| panic!("{error}"));

    let report = ThemeJsonImporter.import_file(&theme).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.unmapped, ["editorScheme"]);
    assert!(report.theme.editor.is_empty());

    let scheme = folder.path().join("Mine.icls");
    std::fs::write(&scheme, fixture("ember/Ember.xml")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(ThemeJsonImporter.scheme_path(&theme, json), Some(scheme));
    let report = ThemeJsonImporter.import_file(&theme).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.theme.editor.get(&EditorKey::Caret), Some(&hex("#E06040")));
    assert_eq!(report.theme.ui.get(&UiKey::WindowBackground), Some(&hex("#010203")));

    assert!(ThemeJsonImporter.import_file(&folder.path().join("missing.theme.json")).is_err());
}

#[test]
fn malformed_ui_themes_are_errors() {
    for text in ["", "[1, 2]", "{\"name\": ", "nonsense"] {
        assert!(ThemeJsonImporter.import(text).is_err(), "{text:?}");
    }
    let broken_scheme = ThemeJsonImporter.import_with_scheme(&fixture("ember/Ember.theme.json"), Some("<scheme"));
    assert!(broken_scheme.is_err());
}

#[test]
fn each_importer_detects_its_own_files_only() {
    let scheme = fixture("every-key.icls");
    let ui = fixture("ember/Ember.theme.json");
    let vs_code =
        r##"{"name": "Code", "type": "dark", "colors": {"editor.background": "#000000"}, "tokenColors": []}"##;
    let settings = r#"{"editor.fontSize": 14}"#;

    assert!(IclsImporter.detects(&scheme));
    assert!(IclsImporter.detects(&fixture("chains.icls")));
    assert!(!IclsImporter.detects(&ui));
    assert!(!IclsImporter.detects("<keymap name=\"x\"/>"));
    assert!(ThemeJsonImporter.detects(&ui));
    assert!(!ThemeJsonImporter.detects(&scheme));
    assert!(!ThemeJsonImporter.detects(vs_code));
    assert!(!ThemeJsonImporter.detects(settings));

    assert!(IclsImporter.detects_path(Path::new("a/Night.icls"), &scheme));
    assert!(IclsImporter.detects_path(Path::new("a/Night.XML"), &scheme));
    assert!(!IclsImporter.detects_path(Path::new("a/Night.json"), &scheme));
    assert!(ThemeJsonImporter.detects_path(Path::new("a/Ember.theme.json"), &ui));
    assert!(!ThemeJsonImporter.detects_path(Path::new("a/Ember.icls"), &ui));
    assert!(!ThemeJsonImporter.detects_path(Path::new("a/code.json"), vs_code));

    let all = importers();
    let names: Vec<&str> = all.iter().map(|importer| importer.format_name()).collect();
    assert_eq!(names, ["JetBrains colour scheme", "JetBrains UI theme"]);
    assert_eq!(all.iter().filter(|importer| importer.detects(&scheme)).count(), 1);
    assert_eq!(all.iter().filter(|importer| importer.detects(&ui)).count(), 1);
}

#[test]
fn file_status_change_marker_and_diff_colours_are_mapped() {
    let text = r#"<scheme name="Status Colours" version="142" parent_scheme="Darcula">
  <colors>
    <option name="ADDED_LINES_COLOR" value="c10001" />
    <option name="MODIFIED_LINES_COLOR" value="c10002" />
    <option name="DELETED_LINES_COLOR" value="c10003" />
    <option name="FILESTATUS_MODIFIED" value="c20001" />
    <option name="FILESTATUS_ADDED" value="c20002" />
    <option name="FILESTATUS_DELETED" value="c20003" />
    <option name="FILESTATUS_UNKNOWN" value="c20004" />
    <option name="FILESTATUS_IDEA_FILESTATUS_IGNORED" value="c20005" />
    <option name="FILESTATUS_IDEA_FILESTATUS_MERGED_WITH_CONFLICTS" value="c20006" />
  </colors>
  <attributes>
    <option name="DIFF_INSERTED"><value><option name="BACKGROUND" value="c30001" /></value></option>
    <option name="DIFF_DELETED"><value><option name="BACKGROUND" value="c30002" /></value></option>
    <option name="DIFF_MODIFIED"><value><option name="BACKGROUND" value="c30003" /></value></option>
  </attributes>
</scheme>"#;
    let report = IclsImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    let theme = &report.theme;
    let editor = [
        (EditorKey::GutterAdded, "#C10001"),
        (EditorKey::GutterModified, "#C10002"),
        (EditorKey::GutterDeleted, "#C10003"),
        (EditorKey::DiffInserted, "#C30001"),
        (EditorKey::DiffDeleted, "#C30002"),
        (EditorKey::DiffModified, "#C30003"),
    ];
    assert_eq!(theme.editor, editor.into_iter().map(|(key, color)| (key, hex(color))).collect());
    let ui = [
        (UiKey::VcsModified, "#C20001"),
        (UiKey::VcsAdded, "#C20002"),
        (UiKey::VcsDeleted, "#C20003"),
        (UiKey::VcsUnversioned, "#C20004"),
        (UiKey::VcsIgnored, "#C20005"),
        (UiKey::VcsConflict, "#C20006"),
    ];
    assert_eq!(theme.ui, ui.into_iter().map(|(key, color)| (key, hex(color))).collect());
    assert_eq!(report.mapped, 12);
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
    assert_resolves_cleanly(&report);
}

#[test]
fn sticky_lines_colours_are_mapped() {
    let text = r#"<scheme name="Sticky Colours" version="142" parent_scheme="Darcula">
  <colors>
    <option name="STICKY_LINES_BACKGROUND" value="c40001" />
  </colors>
</scheme>"#;
    let report = IclsImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    let editor = [(EditorKey::StickyBackground, "#C40001")];
    assert_eq!(report.theme.editor, editor.into_iter().map(|(key, color)| (key, hex(color))).collect());
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
}

#[test]
fn the_method_separator_colour_is_mapped() {
    let text = r#"<scheme name="Separator Colour" version="142" parent_scheme="Darcula">
  <colors>
    <option name="METHOD_SEPARATORS_COLOR" value="c60001" />
  </colors>
</scheme>"#;
    let report = IclsImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    let editor = [(EditorKey::MethodSeparator, "#C60001")];
    assert_eq!(report.theme.editor, editor.into_iter().map(|(key, color)| (key, hex(color))).collect());
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
}

#[test]
fn blame_annotation_colours_are_mapped() {
    let text = r#"<scheme name="Annotation Colours" version="142" parent_scheme="Darcula">
  <colors>
    <option name="ANNOTATIONS_LAST_COMMIT_COLOR" value="c50001" />
    <option name="ANNOTATIONS_COLOR" value="c50002" />
  </colors>
</scheme>"#;
    let report = IclsImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    let editor = [(EditorKey::AnnotationRecent, "#C50001"), (EditorKey::AnnotationOld, "#C50002")];
    assert_eq!(report.theme.editor, editor.into_iter().map(|(key, color)| (key, hex(color))).collect());
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
}

#[test]
fn whitespace_indent_guide_and_right_margin_colours_are_mapped() {
    let text = r#"<scheme name="Guide Colours" version="142" parent_scheme="Darcula">
  <colors>
    <option name="WHITESPACES" value="c60001" />
    <option name="INDENT_GUIDE" value="c60002" />
    <option name="SELECTED_INDENT_GUIDE" value="c60003" />
    <option name="RIGHT_MARGIN_COLOR" value="c60004" />
  </colors>
</scheme>"#;
    let report = IclsImporter.import(text).unwrap_or_else(|error| panic!("{error}"));
    let editor = [
        (EditorKey::Whitespace, "#C60001"),
        (EditorKey::IndentGuide, "#C60002"),
        (EditorKey::IndentGuideSelected, "#C60003"),
        (EditorKey::RightMargin, "#C60004"),
    ];
    assert_eq!(report.theme.editor, editor.into_iter().map(|(key, color)| (key, hex(color))).collect());
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
}

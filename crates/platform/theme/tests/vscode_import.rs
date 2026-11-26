use std::path::PathBuf;

use zen_theme::import::vscode::{self, COLORS, DERIVED, GLOBAL_SETTINGS, SCOPES, SEMANTIC, Target};
use zen_theme::{
    EditorKey, ImportReport, Key, MAX_DEPTH, ResolvedTheme, Rgba, Style, SyntaxKey, TerminalKey, ThemeImporter, UiKey,
    Underline, VsCodeImporter,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vscode")
}

fn read_fixture(path: &str) -> Option<String> {
    std::fs::read_to_string(fixtures().join(path)).ok()
}

fn import(path: &str) -> ImportReport {
    let text = read_fixture(path).unwrap_or_else(|| panic!("no fixture {path}"));
    VsCodeImporter::with_includes(path, read_fixture).import(&text).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn import_text(text: &str) -> ImportReport {
    VsCodeImporter::new().import(text).unwrap_or_else(|error| panic!("{error}"))
}

fn rgb(text: &str) -> Rgba {
    text.parse().unwrap_or_else(|error| panic!("{error}"))
}

fn over(color: &str, background: &str) -> Rgba {
    let (color, background) = (rgb(color), rgb(background));
    let alpha = f64::from(color.a()) / 255.0;
    let mix = |front: u8, back: u8| (f64::from(front) * alpha + f64::from(back) * (1.0 - alpha)).round() as u8;
    Rgba::rgb(mix(color.r(), background.r()), mix(color.g(), background.g()), mix(color.b(), background.b()))
}

fn plain(color: &str) -> Style {
    Style::color(rgb(color))
}

#[test]
fn the_importer_detects_vs_code_themes_only() {
    let importer = VsCodeImporter::new();
    assert_eq!(importer.format_name(), "VS Code colour theme");
    assert!(importer.detects(&read_fixture("full.json").unwrap_or_default()));
    assert!(importer.detects("{ // comment\n \"tokenColors\": [],\n}"));
    assert!(importer.detects(r##"{"$schema": "vscode://schemas/color-theme", "name": "x"}"##));
    assert!(!importer.detects(&zen_theme::to_json(&zen_theme::zen_dark())));
    assert!(!importer.detects(r##"{"name": "x", "dark": true, "ui": {}, "colors": {}}"##));
    assert!(!importer.detects(r##"{"name": "x"}"##));
    assert!(!importer.detects("[1, 2]"));
    assert!(!importer.detects("not json"));
}

#[test]
fn jsonc_comments_and_trailing_commas_are_read() {
    let report = import_text(
        "{\n  // a comment\n  \"name\": \"C\", /* block */\n  \"colors\": {\"editor.background\": \"#123\",},\n}",
    );
    assert_eq!(report.theme.name, "C");
    assert_eq!(report.theme.editor.get(&EditorKey::Background), Some(&rgb("#112233")));
}

#[test]
fn malformed_files_are_errors_with_their_place() {
    let importer = VsCodeImporter::new();
    let error = importer.import("{\n  \"colors\": {\n    \"a\": ,\n  }\n}").err().map(|error| error.to_string());
    assert!(error.as_deref().is_some_and(|error| error.starts_with("line 4, column 3")), "{error:?}");
    let error = importer.import("[]").err().map(|error| error.to_string());
    assert_eq!(error.as_deref(), Some("a VS Code colour theme is a JSON object"));
    let broken = VsCodeImporter::with_includes("dir/a.json", |path| (path == "dir/b.json").then(|| "{".to_owned()));
    let error = broken.import(r##"{"include": "./b.json"}"##).err().map(|error| error.to_string());
    assert!(error.as_deref().is_some_and(|error| error.starts_with("dir/b.json: line 1")), "{error:?}");
}

#[test]
fn the_theme_takes_its_name_and_kind() {
    let report = import("full.json");
    assert_eq!(report.theme.id, "fixture-night");
    assert_eq!(report.theme.name, "Fixture Night");
    assert!(report.theme.dark);
    assert_eq!(report.theme.parent.as_deref(), Some("zen-dark"));
    let unnamed = import_text(r##"{"colors": {"editor.background": "#F0F0F0"}}"##);
    assert_eq!(unnamed.theme.name, vscode::DEFAULT_NAME);
    assert!(!unnamed.theme.dark);
    assert!(import_text(r##"{"type": "hc", "colors": {"editor.background": "#FFFFFF"}}"##).theme.dark);
    assert!(!import_text(r##"{"type": "hcLight"}"##).theme.dark);
    assert!(import_text("{}").theme.dark);
}

#[test]
fn colours_map_to_keys() {
    let theme = import("full.json").theme;
    let bg = "#101820";
    let editor = [
        (EditorKey::Background, rgb(bg)),
        (EditorKey::Foreground, rgb("#C0C8D0")),
        (EditorKey::Caret, rgb("#FFCC00")),
        (EditorKey::Selection, rgb("#3060A080")),
        (EditorKey::CurrentLine, rgb("#FFFFFF10")),
        (EditorKey::LineNumber, over("#FFFFFF80", bg)),
        (EditorKey::SearchMatch, rgb("#40804060")),
        (EditorKey::SearchMark, rgb("#60A060")),
        (EditorKey::SymbolHighlight, rgb("#FFFFFF20")),
        (EditorKey::SymbolHighlightWrite, rgb("#FF404030")),
        (EditorKey::BracketMatchedBackground, rgb("#0064001A")),
        (EditorKey::DiagnosticError, rgb("#FF4050")),
        (EditorKey::DiagnosticWarning, rgb("#E0B040")),
        (EditorKey::DiagnosticInfo, rgb("#4090E0")),
    ];
    assert_eq!(theme.editor, editor.into_iter().collect());
    let ui = [
        (UiKey::WindowBackground, rgb("#0C1218")),
        (UiKey::IslandBackground, rgb("#141C24")),
        (UiKey::TextError, rgb("#FF6060")),
        (UiKey::TextWarning, rgb("#E0B040")),
        (UiKey::Link, rgb("#5AB0FF")),
        (UiKey::Accent, rgb("#2A7AE0")),
        (UiKey::AccentForeground, rgb("#FFFFFF")),
        (UiKey::ListHover, rgb("#FFFFFF0A")),
        (UiKey::ListSelection, rgb("#2050A0")),
        (UiKey::PopupBackground, rgb("#182028")),
        (UiKey::PopupBorder, rgb("#304050")),
        (UiKey::TextFieldBackground, rgb("#0A0E12")),
        (UiKey::TextFieldBorder, rgb("#405060")),
        (UiKey::ButtonSecondaryBorder, rgb("#506070")),
        (UiKey::TabBorder, rgb("#202830")),
        (UiKey::TabActiveBackground, rgb("#1A2430")),
        (UiKey::TabActiveBorder, rgb("#2A7AE0")),
    ];
    assert_eq!(theme.ui, ui.into_iter().collect());
    let ansi = [
        "#000000", "#CC0000", "#00CC00", "#CCCC00", "#0000CC", "#CC00CC", "#00CCCC", "#CCCCCC", "#666666", "#FF3333",
        "#33FF33", "#FFFF33", "#3333FF", "#FF33FF", "#33FFFF", "#FFFFFF",
    ];
    let mut terminal: Vec<(TerminalKey, Rgba)> = vec![
        (TerminalKey::Background, rgb("#0E141A")),
        (TerminalKey::Foreground, rgb("#D0D8E0")),
        (TerminalKey::Cursor, over("#FFCC0080", "#0E141A")),
    ];
    terminal.extend(TerminalKey::ANSI.iter().zip(ansi).map(|(key, color)| (*key, rgb(color))));
    assert_eq!(theme.terminal, terminal.into_iter().collect());
}

#[test]
fn the_first_of_several_colours_for_a_key_wins() {
    let theme = import_text(
        r##"{"colors": {"statusBar.background": "#333333", "titleBar.activeBackground": "#222222",
            "list.inactiveSelectionBackground": "#444444"}}"##,
    )
    .theme;
    assert_eq!(theme.ui.get(&UiKey::WindowBackground), Some(&rgb("#222222")));
    assert_eq!(theme.ui.get(&UiKey::ListSelection), Some(&rgb("#444444")));
}

#[test]
fn alpha_is_kept_on_overlays_and_composited_elsewhere() {
    let theme = import("light.json").theme;
    assert!(!theme.dark);
    let background = over("#FFFFFF88", "#FFFFFF");
    assert_eq!(theme.editor.get(&EditorKey::Background), Some(&background));
    assert_eq!(theme.editor.get(&EditorKey::Foreground), Some(&over("#222222DD", "#FFFFFF")));
    assert_eq!(theme.ui.get(&UiKey::ListHover), Some(&rgb("#00000011")));
    assert_eq!(theme.editor.get(&EditorKey::DiffInserted), Some(&over("#9BE5A040", "#FFFFFF")));
    assert_eq!(theme.editor.get(&EditorKey::DiffFiller), Some(&over("#00000010", "#FFFFFF")));
    assert_eq!(theme.syntax.get(&SyntaxKey::String), Some(&plain("#00AA00")));
    let dark = import_text(
        r##"{"type": "dark", "colors": {"editor.background": "#00000080", "terminal.ansiRed": "#FF000080"}}"##,
    );
    let editor = over("#00000080", &ResolvedTheme::zen_dark().editor(EditorKey::Background).to_string());
    assert_eq!(dark.theme.editor.get(&EditorKey::Background), Some(&editor));
    assert_eq!(dark.theme.terminal.get(&TerminalKey::Ansi1), Some(&over("#FF000080", &editor.to_string())));
    assert!(Target::Editor(EditorKey::Selection).keeps_alpha());
    assert!(Target::Ui(UiKey::ListHover).keeps_alpha());
    assert!(!Target::Editor(EditorKey::Background).keeps_alpha());
    assert!(!Target::Terminal(TerminalKey::Cursor).keeps_alpha());
}

#[test]
fn token_colours_become_code_styles() {
    let theme = import("full.json").theme;
    let syntax = |key: SyntaxKey| theme.syntax.get(&key).copied();
    let bold = |color: &str| Style { bold: true, ..plain(color) };
    assert_eq!(syntax(SyntaxKey::Default), Some(plain("#C0C8D0")));
    assert_eq!(syntax(SyntaxKey::Comment), Some(plain("#607080").italic()));
    assert_eq!(syntax(SyntaxKey::DocComment), Some(plain("#70A080").italic()));
    assert_eq!(syntax(SyntaxKey::Keyword), Some(bold("#E08050")));
    assert_eq!(syntax(SyntaxKey::String), Some(plain("#80C080")));
    assert_eq!(syntax(SyntaxKey::Number), Some(plain("#40C0D0")));
    assert_eq!(syntax(SyntaxKey::Macro), Some(plain("#60A8F0")));
    assert_eq!(syntax(SyntaxKey::Type), Some(Style { underline: Some(Underline::Line), ..plain("#70B0C0") }));
    assert_eq!(
        syntax(SyntaxKey::Heading),
        Some(Style { bold: true, underline: Some(Underline::Line), ..plain("#D080C0") })
    );
    assert_eq!(syntax(SyntaxKey::Emphasis), Some(plain("#C0C8D0").italic()));
    assert_eq!(syntax(SyntaxKey::Strong), Some(Style { bold: true, strikethrough: true, ..plain("#E0E0E0") }));
    assert_eq!(syntax(SyntaxKey::Punctuation), Some(Style::color(over("#90989F80", "#101820"))));
    assert_eq!(syntax(SyntaxKey::Lifetime), None);
    assert_eq!(syntax(SyntaxKey::Constant), None);
}

#[test]
fn the_global_rule_fills_in_what_colors_leave_out() {
    let theme = import("full.json").theme;
    assert_eq!(theme.editor.get(&EditorKey::Foreground), Some(&rgb("#C0C8D0")));
    assert_eq!(theme.editor.get(&EditorKey::Caret), Some(&rgb("#FFCC00")));
    let legacy = import_text(
        r##"{"tokenColors": [{"settings": {"background": "#202020", "foreground": "#E0E0E0", "caret": "#FF0000",
            "selection": "#FFFFFF30", "lineHighlight": "#FFFFFF08"}},
            {"scope": "keyword", "settings": {"fontStyle": "bold"}}]}"##,
    );
    let editor = &legacy.theme.editor;
    assert_eq!(editor.get(&EditorKey::Background), Some(&rgb("#202020")));
    assert_eq!(editor.get(&EditorKey::Foreground), Some(&rgb("#E0E0E0")));
    assert_eq!(editor.get(&EditorKey::Caret), Some(&rgb("#FF0000")));
    assert_eq!(editor.get(&EditorKey::Selection), Some(&rgb("#FFFFFF30")));
    assert_eq!(editor.get(&EditorKey::CurrentLine), Some(&rgb("#FFFFFF08")));
    assert_eq!(legacy.theme.syntax.get(&SyntaxKey::Keyword), Some(&Style { bold: true, ..plain("#E0E0E0") }));
    assert_eq!(legacy.mapped, 6);
}

#[test]
fn a_rule_without_a_colour_keeps_zen_darks_colour_when_nothing_else_is_known() {
    let theme = import_text(r##"{"tokenColors": [{"scope": "comment", "settings": {"fontStyle": "bold"}}]}"##).theme;
    let zen = ResolvedTheme::zen_dark().syntax(SyntaxKey::Comment);
    assert_eq!(theme.syntax.get(&SyntaxKey::Comment), Some(&Style { bold: true, ..Style::color(zen.color) }));
}

#[test]
fn scopes_match_by_prefix_specificity_and_order() {
    let report = import_text(
        r##"{"colors": {"editor.foreground": "#101010"}, "tokenColors": [
            {"scope": "storage", "settings": {"foreground": "#010101"}},
            {"scope": "entity.name", "settings": {"foreground": "#020202"}},
            {"scope": "entity.name.function", "settings": {"foreground": "#030303"}},
            {"scope": ["string.quoted", "constant"], "settings": {"foreground": "#040404"}},
            {"scope": "constant", "settings": {"foreground": "#050505"}},
            {"scope": "support.type, support.type.primitive", "settings": {"foreground": "#060606"}},
            {"scope": "support.type.primitive", "settings": {"foreground": "#070707"}},
            {"scope": "support.type", "settings": {"foreground": "#080808"}},
            {"scope": "comment - comment.block.documentation", "settings": {"foreground": "#090909"}}
        ]}"##,
    );
    let syntax = |key: SyntaxKey| report.theme.syntax.get(&key).map(|style| style.color.to_string());
    assert_eq!(syntax(SyntaxKey::Keyword).as_deref(), Some("#010101"));
    assert_eq!(syntax(SyntaxKey::Lifetime).as_deref(), Some("#010101"));
    assert_eq!(syntax(SyntaxKey::Function).as_deref(), Some("#030303"));
    assert_eq!(syntax(SyntaxKey::Macro).as_deref(), Some("#030303"));
    assert_eq!(syntax(SyntaxKey::SemanticCall).as_deref(), Some("#030303"));
    assert_eq!(syntax(SyntaxKey::Type).as_deref(), Some("#020202"));
    assert_eq!(syntax(SyntaxKey::SemanticPackage).as_deref(), Some("#020202"));
    assert_eq!(syntax(SyntaxKey::String), None);
    assert_eq!(syntax(SyntaxKey::Number).as_deref(), Some("#050505"));
    assert_eq!(syntax(SyntaxKey::Constant).as_deref(), Some("#050505"));
    assert_eq!(syntax(SyntaxKey::SemanticBuiltinType).as_deref(), Some("#070707"));
    assert_eq!(syntax(SyntaxKey::Comment).as_deref(), Some("#090909"));
    assert_eq!(syntax(SyntaxKey::DocComment), None);
    assert_eq!(report.unmapped, vec!["tokenColors: string.quoted"]);
    assert_eq!(report.mapped, 11);
}

#[test]
fn calls_take_a_call_specific_rule() {
    let theme = import("full.json").theme;
    let report = import_text(
        r##"{"tokenColors": [{"scope": "meta.function-call entity.name.function", "settings": {"foreground": "#B0A080"}},
            {"scope": "entity.name.function", "settings": {"foreground": "#60A8F0"}}]}"##,
    );
    assert_eq!(report.theme.syntax.get(&SyntaxKey::SemanticCall), Some(&plain("#B0A080")));
    assert_eq!(report.theme.syntax.get(&SyntaxKey::Function), Some(&plain("#60A8F0")));
    assert_eq!(theme.syntax.get(&SyntaxKey::SemanticCall), Some(&plain("#E0C090")));
}

#[test]
fn semantic_token_colours_override_token_colours() {
    let theme = import("full.json").theme;
    let syntax = |key: SyntaxKey| theme.syntax.get(&key).copied();
    assert_eq!(syntax(SyntaxKey::SemanticPackage), Some(plain("#C0D080")));
    assert_eq!(syntax(SyntaxKey::SemanticType), Some(plain("#80C0E0").italic()));
    assert_eq!(syntax(SyntaxKey::Function), Some(Style { bold: true, ..plain("#60A8F0") }));
    assert_eq!(syntax(SyntaxKey::SemanticCall), Some(plain("#E0C090")));
    assert_eq!(syntax(SyntaxKey::Keyword), Some(Style { bold: true, ..plain("#E08050") }));

    let text = r##"{"semanticHighlighting": false, "semanticTokenColors": {"namespace": "#C0D080"},
        "tokenColors": [{"scope": "entity.name.namespace", "settings": {"foreground": "#A0B070"}}]}"##;
    let off = import_text(text);
    assert_eq!(off.theme.syntax.get(&SyntaxKey::SemanticPackage), Some(&plain("#A0B070")));
    assert_eq!(off.unmapped, vec!["semanticTokenColors: namespace"]);
}

#[test]
fn the_report_counts_and_lists_what_was_not_mapped() {
    let report = import("full.json");
    assert_eq!(
        report.unmapped,
        vec![
            "author",
            "activityBar.background",
            "statusBar.background",
            "tokenColors.settings.fontFamily",
            "tokenColors: source.go",
            "tokenColors: (a | b)",
            "tokenColors: invalid.illegal",
            "tokenColors: No settings",
            "semanticTokenColors: *.deprecated",
            "semanticTokenColors: keyword",
            "semanticTokenColors: variable.readonly:go",
        ]
    );
    let (colors, global, selectors, semantic) = (50, 3, 15, 4);
    assert_eq!(report.mapped, colors + global + selectors + semantic);
}

#[test]
fn includes_merge_parent_first() {
    let report = import("chain/child.json");
    let theme = &report.theme;
    assert_eq!(theme.name, "Chain Child");
    assert!(!theme.dark);
    assert_eq!(theme.editor.get(&EditorKey::Background), Some(&rgb("#202020")));
    assert_eq!(theme.editor.get(&EditorKey::Foreground), Some(&rgb("#D0D0D0")));
    assert_eq!(theme.editor.get(&EditorKey::Caret), Some(&rgb("#ABCDEF")));
    assert_eq!(theme.syntax.get(&SyntaxKey::Keyword), Some(&plain("#CC0000")));
    assert_eq!(theme.syntax.get(&SyntaxKey::String), Some(&plain("#00AA00")));
    assert_eq!(theme.syntax.get(&SyntaxKey::Comment), Some(&plain("#888888")));
    assert_eq!(theme.syntax.get(&SyntaxKey::SemanticPackage), Some(&plain("#445566")));
    assert_eq!(theme.syntax.get(&SyntaxKey::SemanticType), Some(&plain("#334455")));
    assert!(report.unmapped.is_empty(), "{:?}", report.unmapped);
}

#[test]
fn include_cycles_missing_files_and_deep_chains_are_reported() {
    let report = import("cycle/a.json");
    assert_eq!(report.theme.name, "Cycle A");
    assert_eq!(report.theme.editor.get(&EditorKey::Foreground), Some(&rgb("#AAAAAA")));
    assert_eq!(report.theme.editor.get(&EditorKey::Background), Some(&rgb("#0B0B0B")));
    assert_eq!(report.unmapped, vec!["include ./a.json (a cycle)"]);

    let missing = import_text(r##"{"include": "./base.json", "name": "M"}"##);
    assert_eq!(missing.unmapped, vec!["include ./base.json (not found)"]);

    let endless = |path: &str| {
        let next: usize = path.trim_start_matches("deep/").trim_end_matches(".json").parse().ok()?;
        Some(format!(
            r##"{{"include": "../deep/{}.json", "colors": {{"editor.foreground": "#00000{}"}}}}"##,
            next + 1,
            next % 10
        ))
    };
    let deep = VsCodeImporter::with_includes("deep/0.json", endless);
    let report = deep.import(&endless("deep/0.json").unwrap_or_default()).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.unmapped, vec![format!("include ../deep/{}.json (too deep)", MAX_DEPTH + 1)]);
    assert_eq!(report.theme.editor.get(&EditorKey::Foreground), Some(&rgb("#000000")));
}

#[test]
fn imported_themes_resolve_without_warnings() {
    for path in ["full.json", "light.json", "chain/child.json", "cycle/a.json"] {
        let report = import(path);
        let resolution = report.theme.resolve(|_| None);
        assert!(resolution.warnings.is_empty(), "{path}: {:?}", resolution.warnings);
        for (key, style) in &report.theme.syntax {
            assert_eq!(resolution.theme.syntax(*key), *style, "{path}: {key}");
        }
        let base = ResolvedTheme::base(report.theme.dark);
        for key in UiKey::ALL.iter().filter(|key| !report.theme.ui.contains_key(key)) {
            assert_eq!(resolution.theme.ui(*key), base.ui(*key), "{path}: {key}");
        }
    }
}

#[test]
fn a_light_theme_builds_on_zen_light() {
    let report = import("light.json");
    assert_eq!(report.theme.parent.as_deref(), Some(zen_theme::ZEN_LIGHT_ID));
    assert!(report.mapped > 40, "the fixture is a whole light theme: {}", report.mapped);
    let resolved = report.theme.resolve(|_| None).theme;
    let light = ResolvedTheme::zen_light();
    for key in [UiKey::PopupHover, UiKey::TreeTestRow, UiKey::BannerBackground, UiKey::TextWarning] {
        assert_eq!(resolved.ui(key), light.ui(key), "{key}");
    }
    assert_eq!(resolved.editor(EditorKey::FoldMarker), light.editor(EditorKey::FoldMarker));
    assert_eq!(resolved.terminal(TerminalKey::Ansi5), light.terminal(TerminalKey::Ansi5));
    assert_eq!(import("full.json").theme.parent.as_deref(), Some(zen_theme::ZEN_DARK_ID));
}

#[test]
fn diff_colours_the_theme_leaves_out_are_tints_of_its_change_markers() {
    let theme = import_text(
        r##"{"type": "light", "colors": {
            "editor.background": "#FFFFFF",
            "editorGutter.modifiedBackground": "#0000FF",
            "editorGutter.addedBackground": "#00FF0080",
            "diffEditor.insertedLineBackground": "#E0FFE0"
        }}"##,
    )
    .theme;
    assert_eq!(theme.editor.get(&EditorKey::DiffModified), Some(&over("#0000FF40", "#FFFFFF")));
    assert_eq!(theme.editor.get(&EditorKey::DiffModifiedWord), Some(&over("#0000FF80", "#FFFFFF")));
    assert_eq!(theme.editor.get(&EditorKey::DiffInserted), Some(&rgb("#E0FFE0")), "the theme's own colour wins");
    assert_eq!(theme.editor.get(&EditorKey::DiffInsertedWord), Some(&over("#00FF0040", "#FFFFFF")));
    assert_eq!(theme.editor.get(&EditorKey::DiffDeleted), None, "no marker, nothing derived");
}

const BEGIN: &str = "<!-- vscode:begin -->\n";
const END: &str = "<!-- vscode:end -->\n";

fn tables() -> String {
    let mut text = String::from("### Colours\n\n| VS Code colour | Zen key | Alpha |\n|---|---|---|\n");
    for (color, target) in COLORS {
        let alpha = if target.keeps_alpha() { "kept" } else { "composited" };
        text.push_str(&format!("| `{color}` | `{}` | {alpha} |\n", target.id()));
    }
    text.push_str("\n### Derived diff colours\n\n| VS Code colour | Zen key, when the theme does not set it | Alpha |\n|---|---|---|\n");
    for (color, key, alpha) in DERIVED {
        text.push_str(&format!("| `{color}` | `editor.{}` | `{alpha:02X}` of `FF`, over the editor |\n", key.id()));
    }
    text.push_str("\n### The global rule\n\n| Setting | Stands in for |\n|---|---|\n");
    for (setting, color) in GLOBAL_SETTINGS {
        text.push_str(&format!("| `{setting}` | `{color}` |\n"));
    }
    text.push_str("\n### Token colours\n\n| Zen key | Scopes probed, in order |\n|---|---|\n");
    for (key, stacks) in SCOPES {
        let stacks: Vec<String> = stacks.iter().map(|stack| format!("`{}`", stack.join(" "))).collect();
        text.push_str(&format!("| `syntax.{}` | {} |\n", key.id(), stacks.join(", ")));
    }
    text.push_str("\n### Semantic token colours\n\n| Zen key | Selectors, in order |\n|---|---|\n");
    for (key, selectors) in SEMANTIC {
        let selectors: Vec<String> = selectors.iter().map(|selector| format!("`{selector}`")).collect();
        text.push_str(&format!("| `syntax.{}` | {} |\n", key.id(), selectors.join(", ")));
    }
    text
}

#[test]
fn the_documented_tables_are_the_code() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/themes/import-vscode.md");
    let document = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let start = document.find(BEGIN).map(|index| index + BEGIN.len()).unwrap_or_else(|| panic!("no {BEGIN}"));
    let end = document.find(END).unwrap_or_else(|| panic!("no {END}"));
    let current = tables();
    if std::env::var_os("ZEN_THEME_BLESS").is_some_and(|value| value == "1") {
        let updated = format!("{}\n{current}\n{}", &document[..start], &document[end..]);
        std::fs::write(&path, updated).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    } else {
        assert!(
            document[start..end].trim() == current.trim(),
            "docs/themes/import-vscode.md is out of date; run `ZEN_THEME_BLESS=1 cargo test -p zen-theme --test vscode_import`"
        );
    }
}

#[test]
fn change_marker_diff_and_file_status_colours_are_mapped() {
    let theme = import_text(
        r##"{
        "colors": {
            "editor.background": "#101820",
            "editorGutter.addedBackground": "#00A000",
            "editorGutter.modifiedBackground": "#0000A0",
            "editorGutter.deletedBackground": "#A00000",
            "diffEditor.insertedLineBackground": "#003000",
            "diffEditor.removedLineBackground": "#300000",
            "diffEditor.insertedTextBackground": "#006000",
            "diffEditor.removedTextBackground": "#600000",
            "gitDecoration.modifiedResourceForeground": "#6080F0",
            "gitDecoration.addedResourceForeground": "#60F080",
            "gitDecoration.deletedResourceForeground": "#808080",
            "gitDecoration.untrackedResourceForeground": "#F06060",
            "gitDecoration.ignoredResourceForeground": "#A0A040",
            "gitDecoration.conflictingResourceForeground": "#F04040",
            "scmGraph.foreground1": "#100001",
            "scmGraph.foreground5": "#100005"
        }
    }"##,
    )
    .theme;
    let editor = [
        (EditorKey::Background, rgb("#101820")),
        (EditorKey::GutterAdded, rgb("#00A000")),
        (EditorKey::GutterModified, rgb("#0000A0")),
        (EditorKey::GutterDeleted, rgb("#A00000")),
        (EditorKey::DiffInserted, rgb("#003000")),
        (EditorKey::DiffDeleted, rgb("#300000")),
        (EditorKey::DiffInsertedWord, rgb("#006000")),
        (EditorKey::DiffDeletedWord, rgb("#600000")),
        (EditorKey::DiffModified, over("#0000A040", "#101820")),
        (EditorKey::DiffModifiedWord, over("#0000A080", "#101820")),
    ];
    assert_eq!(theme.editor, editor.into_iter().collect());
    let ui = [
        (UiKey::TabModified, rgb("#6080F0")),
        (UiKey::TreeExcludedText, rgb("#A0A040")),
        (UiKey::VcsModified, rgb("#6080F0")),
        (UiKey::VcsAdded, rgb("#60F080")),
        (UiKey::VcsDeleted, rgb("#808080")),
        (UiKey::VcsUnversioned, rgb("#F06060")),
        (UiKey::VcsIgnored, rgb("#A0A040")),
        (UiKey::VcsConflict, rgb("#F04040")),
        (UiKey::VcsGraph0, rgb("#100001")),
        (UiKey::VcsGraph4, rgb("#100005")),
    ];
    assert_eq!(theme.ui, ui.into_iter().collect());
}

#[test]
fn whitespace_indent_guide_and_ruler_colours_prefer_the_current_names() {
    let theme = import_text(
        r##"{"type": "dark", "colors": {
            "editor.background": "#000000",
            "editorWhitespace.foreground": "#E3E4E229",
            "editorIndentGuide.background": "#111111",
            "editorIndentGuide.background1": "#222222",
            "editorIndentGuide.activeBackground": "#333333",
            "editorIndentGuide.activeBackground1": "#444444",
            "editorRuler.foreground": "#5A5A5A"
        }}"##,
    )
    .theme;
    assert_eq!(theme.editor.get(&EditorKey::Whitespace), Some(&over("#E3E4E229", "#000000")));
    assert_eq!(theme.editor.get(&EditorKey::IndentGuide), Some(&rgb("#222222")), "background1 before the old name");
    assert_eq!(theme.editor.get(&EditorKey::IndentGuideSelected), Some(&rgb("#444444")));
    assert_eq!(theme.editor.get(&EditorKey::RightMargin), Some(&rgb("#5A5A5A")));
    let old = import_text(r##"{"colors": {"editorIndentGuide.background": "#111111"}}"##).theme;
    assert_eq!(old.editor.get(&EditorKey::IndentGuide), Some(&rgb("#111111")), "the old name alone still counts");
}

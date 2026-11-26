use egui::Color32;
use zen_syntax::HighlightKind;
use zen_theme::{ResolvedTheme, Rgba, Style, SyntaxKey, Theme, UiKey};

use super::IdePalette;
use crate::theme_bridge::CodeStyle;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const BEFORE: &[(&str, Color32)] = &[
    ("frame", rgb(0x25, 0x28, 0x2C)),
    ("island", rgb(0x19, 0x1A, 0x1D)),
    ("text", rgb(0xDF, 0xE1, 0xE5)),
    ("text_secondary", rgb(0x86, 0x8A, 0x91)),
    ("gutter_text", rgb(0x4B, 0x50, 0x59)),
    ("current_line", rgb(0x1F, 0x20, 0x24)),
    ("hover", rgb(0x2C, 0x2E, 0x33)),
    ("popup_hover", rgb(0x3A, 0x3D, 0x43)),
    ("selection", rgb(0x2E, 0x43, 0x6E)),
    ("caret", rgb(0xCE, 0xD0, 0xD6)),
    ("search", rgb(0x2F, 0x4C, 0x3A)),
    ("search_mark", rgb(0x5F, 0x8C, 0x63)),
    ("accent", rgb(0x35, 0x74, 0xF0)),
    ("excluded", rgb(0xD8, 0x8A, 0x3C)),
    ("tab_border", rgb(0x4E, 0x51, 0x57)),
    ("test_row", rgb(0x1E, 0x26, 0x1D)),
    ("active_tab", rgb(0x27, 0x35, 0x55)),
    ("active_tab_border", rgb(0x35, 0x4C, 0x85)),
    ("package", rgb(0xB2, 0xBF, 0x85)),
    ("semantic_type", rgb(0x7E, 0xAD, 0xBB)),
    ("call", rgb(0xAD, 0x9E, 0x7D)),
    ("doc_name", rgb(0xA1, 0xA1, 0xA1)),
    ("fold_mark", rgb(0x3B, 0x3E, 0x44)),
    ("fold_arrow", rgb(0x70, 0x73, 0x79)),
    ("fold_text", rgb(0x86, 0x8A, 0x91)),
    ("inlay_hint_background", rgb(0x2B, 0x2D, 0x31)),
    ("inlay_hint_foreground", rgb(0x8C, 0x90, 0x97)),
    ("gutter_separator", rgb(0x32, 0x34, 0x38)),
    ("sticky_background", rgb(0x1D, 0x1E, 0x21)),
    ("sticky_border", rgb(0x32, 0x34, 0x38)),
    ("code", rgb(0xBC, 0xBE, 0xC4)),
    ("keyword", rgb(0xCF, 0x8E, 0x6D)),
    ("string", rgb(0x6A, 0xAB, 0x73)),
    ("number", rgb(0x2A, 0xAC, 0xB8)),
    ("comment", rgb(0x7A, 0x7E, 0x85)),
    ("doc_comment", rgb(0x5F, 0x82, 0x6B)),
    ("macro_call", rgb(0x56, 0xA8, 0xF5)),
    ("function", rgb(0x56, 0xA8, 0xF5)),
    ("type_name", rgb(0xBC, 0xBE, 0xC4)),
    ("lifetime", rgb(0x20, 0x99, 0x9D)),
    ("attribute", rgb(0xB3, 0xAE, 0x60)),
    ("constant", rgb(0xC7, 0x7D, 0xBB)),
    ("heading", rgb(0xC8, 0x7D, 0xBB)),
    ("run", rgb(0x5F, 0xB8, 0x65)),
    ("banner", rgb(0x25, 0x32, 0x4D)),
    ("link", rgb(0x6C, 0xA4, 0xF8)),
    ("vcs_modified", rgb(0x6C, 0xA4, 0xF8)),
    ("vcs_added", rgb(0x73, 0xBD, 0x79)),
    ("vcs_deleted", rgb(0x6F, 0x73, 0x7A)),
    ("vcs_unversioned", rgb(0xD1, 0x67, 0x5A)),
    ("vcs_ignored", rgb(0xA8, 0xA3, 0x4F)),
    ("vcs_conflict", rgb(0xE5, 0x5C, 0x5C)),
    ("vcs_incoming", rgb(0x6C, 0xA4, 0xF8)),
    ("vcs_outgoing", rgb(0x73, 0xBD, 0x79)),
    ("gutter_added", rgb(0x54, 0x91, 0x59)),
    ("gutter_modified", rgb(0x37, 0x5F, 0xAD)),
    ("gutter_deleted", rgb(0x86, 0x8A, 0x91)),
    ("diff_inserted", rgb(0x1F, 0x35, 0x26)),
    ("diff_deleted", rgb(0x34, 0x36, 0x3A)),
    ("diff_modified", rgb(0x1E, 0x2E, 0x4B)),
    ("diff_inserted_word", rgb(0x2C, 0x54, 0x37)),
    ("diff_deleted_word", rgb(0x4A, 0x4D, 0x52)),
    ("diff_modified_word", rgb(0x2B, 0x45, 0x75)),
    ("diff_filler", rgb(0x22, 0x23, 0x27)),
    ("breakpoint", rgb(0xE5, 0x5C, 0x5C)),
    ("breakpoint_disabled", rgb(0xE5, 0x6E, 0x6E)),
    ("execution_line", rgb(0x27, 0x35, 0x55)),
    ("execution_arrow", rgb(0x6C, 0xA4, 0xF8)),
    ("annotation_recent", rgb(0xE8, 0xB9, 0x4F)),
    ("annotation_old", rgb(0x5A, 0x5E, 0x66)),
];

fn former_literals() -> Vec<(&'static str, Color32, &'static str)> {
    vec![
        ("popup", rgb(0x2B, 0x2D, 0x30), "apply window_fill and every popup frame"),
        ("popup_border", rgb(0x43, 0x45, 0x4A), "the popup frames' stroke, the doc popups' rule"),
        ("popup_shadow", Color32::from_black_alpha(110), "dialog_frame's shadow"),
        ("popup_code", Color32::from_rgba_unmultiplied(0x80, 0x80, 0x80, 0x30), "doc_popups.rs inline code"),
        ("text_field", rgb(0x1E, 0x1F, 0x22), "the Go to Line and new-entry fields"),
        ("text_field_border", rgb(0x4E, 0x51, 0x57), "the new-entry field without focus"),
        ("text_field_selection", rgb(0x21, 0x42, 0x83), "apply selection.bg_fill"),
        ("text_field_caret", rgb(0xCE, 0xD0, 0xD6), "apply text_cursor"),
        ("button_border", rgb(0x4E, 0x51, 0x57), "secondary buttons in mod.rs, settings.rs, welcome.rs"),
        ("accent_text", Color32::WHITE, "primary buttons' labels"),
        ("stripe_active_icon", Color32::WHITE, "the active stripe button's icon"),
        ("search_highlight", rgb(0x2F, 0x4C, 0x3A), "the search matches in find_window.rs, as in Settings labels"),
        ("excluded_background", rgb(0x29, 0x1F, 0x1C), "the excluded folder row"),
        ("modified_tab", rgb(0x6C, 0xA4, 0xF8), "a dirty editor tab's name"),
        ("error_text", rgb(0xE5, 0x6E, 0x6E), "settings.rs: the gopls install failure"),
        ("warning_text", rgb(0xE5, 0xA0, 0x4B), "keymap_page.rs: the shortcut conflict"),
        ("checker_light", rgb(0x4A, 0x4D, 0x52), "image_view.rs chessboard"),
        ("checker_dark", rgb(0x3A, 0x3C, 0x40), "image_view.rs chessboard"),
        ("project_text", Color32::WHITE, "project_badge initials"),
        ("editor_background", rgb(0x19, 0x1A, 0x1D), "the island under the editor, the preedit"),
        ("editor_selection", rgb(0x2E, 0x43, 0x6E), "editor_view.rs selection band"),
        ("symbol_highlight", rgb(0x38, 0x3B, 0x39), "IdePalette::DARK.occurrence, the occurrence bands"),
        ("symbol_highlight_write", rgb(0x4A, 0x36, 0x3A), "the written usages' bands"),
        ("diagnostic_error", rgb(0xF7, 0x54, 0x64), "editor_view.rs problem_color"),
        ("diagnostic_warning", rgb(0xE0, 0xB0, 0x4E), "editor_view.rs problem_color"),
        ("diagnostic_info", rgb(0x6F, 0x9F, 0xE8), "editor_view.rs problem_color"),
        ("diagnostic_hint", rgb(0x86, 0x8A, 0x91), "editor_view.rs problem_color"),
        ("diagnostic_typo", rgb(0x5F, 0xA3, 0x6A), "a misspelt word's wavy line (ADR-0113)"),
        ("completion_function", rgb(0xC0, 0x8B, 0xF5), "editor_view.rs kind_mark"),
        ("completion_variable", rgb(0x6F, 0xB3, 0xD9), "editor_view.rs kind_mark"),
        ("completion_field", rgb(0xE0, 0xB0, 0x4E), "editor_view.rs kind_mark"),
        ("completion_constant", rgb(0x9A, 0xC6, 0x7A), "editor_view.rs kind_mark"),
        ("completion_type", rgb(0x5F, 0xA8, 0xF0), "editor_view.rs kind_mark"),
        ("completion_interface", rgb(0x5F, 0xC8, 0xB0), "editor_view.rs kind_mark"),
        ("completion_module", rgb(0xD0, 0x8C, 0x6A), "editor_view.rs kind_mark"),
        ("completion_keyword", rgb(0xCF, 0x8E, 0x6D), "editor_view.rs kind_mark"),
        ("completion_snippet", rgb(0xA0, 0xA4, 0xAC), "editor_view.rs kind_mark"),
        ("completion_other", rgb(0xA0, 0xA4, 0xAC), "editor_view.rs kind_mark"),
    ]
}

fn field(p: &IdePalette, name: &str) -> Color32 {
    match name {
        "frame" => p.frame,
        "island" => p.island,
        "text" => p.text,
        "text_secondary" => p.text_secondary,
        "gutter_text" => p.gutter_text,
        "current_line" => p.current_line,
        "hover" => p.hover,
        "popup_hover" => p.popup_hover,
        "selection" => p.selection,
        "caret" => p.caret,
        "search" => p.search,
        "search_mark" => p.search_mark,
        "search_highlight" => p.search_highlight,
        "accent" => p.accent,
        "excluded" => p.excluded,
        "tab_border" => p.tab_border,
        "test_row" => p.test_row,
        "active_tab" => p.active_tab,
        "active_tab_border" => p.active_tab_border,
        "package" => p.package,
        "semantic_type" => p.semantic_type,
        "call" => p.call,
        "doc_name" => p.doc_name,
        "fold_mark" => p.fold_mark,
        "fold_arrow" => p.fold_arrow,
        "fold_text" => p.fold_text,
        "inlay_hint_background" => p.inlay_hint_background,
        "inlay_hint_foreground" => p.inlay_hint_foreground,
        "gutter_separator" => p.gutter_separator,
        "sticky_background" => p.sticky_background,
        "sticky_border" => p.sticky_border,
        "code" => p.code,
        "keyword" => p.keyword,
        "string" => p.string,
        "number" => p.number,
        "comment" => p.comment,
        "doc_comment" => p.doc_comment,
        "macro_call" => p.macro_call,
        "function" => p.function,
        "type_name" => p.type_name,
        "lifetime" => p.lifetime,
        "attribute" => p.attribute,
        "constant" => p.constant,
        "heading" => p.heading,
        "run" => p.run,
        "banner" => p.banner,
        "link" => p.link,
        "popup" => p.popup,
        "popup_border" => p.popup_border,
        "popup_shadow" => p.popup_shadow,
        "popup_code" => p.popup_code,
        "text_field" => p.text_field,
        "text_field_border" => p.text_field_border,
        "text_field_selection" => p.text_field_selection,
        "text_field_caret" => p.text_field_caret,
        "button_border" => p.button_border,
        "accent_text" => p.accent_text,
        "stripe_active_icon" => p.stripe_active_icon,
        "excluded_background" => p.excluded_background,
        "modified_tab" => p.modified_tab,
        "error_text" => p.error_text,
        "warning_text" => p.warning_text,
        "checker_light" => p.checker_light,
        "checker_dark" => p.checker_dark,
        "project_text" => p.project_text,
        "editor_background" => p.editor_background,
        "editor_selection" => p.editor_selection,
        "symbol_highlight" => p.symbol_highlight,
        "symbol_highlight_write" => p.symbol_highlight_write,
        "diagnostic_error" => p.diagnostic_error,
        "diagnostic_warning" => p.diagnostic_warning,
        "diagnostic_info" => p.diagnostic_info,
        "diagnostic_hint" => p.diagnostic_hint,
        "diagnostic_typo" => p.diagnostic_typo,
        "completion_function" => p.completion_function,
        "completion_variable" => p.completion_variable,
        "completion_field" => p.completion_field,
        "completion_constant" => p.completion_constant,
        "completion_type" => p.completion_type,
        "completion_interface" => p.completion_interface,
        "completion_module" => p.completion_module,
        "completion_keyword" => p.completion_keyword,
        "completion_snippet" => p.completion_snippet,
        "completion_other" => p.completion_other,
        "vcs_modified" => p.vcs_modified,
        "vcs_added" => p.vcs_added,
        "vcs_deleted" => p.vcs_deleted,
        "vcs_unversioned" => p.vcs_unversioned,
        "vcs_ignored" => p.vcs_ignored,
        "vcs_conflict" => p.vcs_conflict,
        "vcs_incoming" => p.vcs_incoming,
        "vcs_outgoing" => p.vcs_outgoing,
        "gutter_added" => p.gutter_added,
        "gutter_modified" => p.gutter_modified,
        "gutter_deleted" => p.gutter_deleted,
        "diff_inserted" => p.diff_inserted,
        "diff_deleted" => p.diff_deleted,
        "diff_modified" => p.diff_modified,
        "diff_inserted_word" => p.diff_inserted_word,
        "diff_deleted_word" => p.diff_deleted_word,
        "diff_modified_word" => p.diff_modified_word,
        "diff_filler" => p.diff_filler,
        "breakpoint" => p.breakpoint,
        "breakpoint_disabled" => p.breakpoint_disabled,
        "execution_line" => p.execution_line,
        "execution_arrow" => p.execution_arrow,
        "annotation_recent" => p.annotation_recent,
        "annotation_old" => p.annotation_old,
        _ => panic!("no field {name}"),
    }
}

#[test]
fn zen_dark_is_the_old_dark_palette() {
    let p = IdePalette::zen_dark();
    for (name, before) in BEFORE {
        assert_eq!(field(&p, name), *before, "IdePalette::DARK.{name}");
    }
    for (name, before, site) in former_literals() {
        assert_eq!(field(&p, name), before, "{name}, formerly a literal at {site}");
    }
    assert_eq!(
        p.projects,
        [
            rgb(0x4A, 0x7F, 0xD6),
            rgb(0x3E, 0x8E, 0x7E),
            rgb(0xC0, 0x6E, 0x45),
            rgb(0x96, 0x5E, 0xC2),
            rgb(0x3F, 0x9A, 0xB0),
            rgb(0xB8, 0x5A, 0x8A),
        ]
    );
    assert_eq!(
        p.graph,
        [
            rgb(0x5F, 0x9E, 0xE8),
            rgb(0x6A, 0xB8, 0x6E),
            rgb(0xD9, 0x8C, 0x4A),
            rgb(0xB0, 0x7C, 0xD8),
            rgb(0x3F, 0xB8, 0xB0),
            rgb(0xD8, 0x6F, 0x7E),
        ]
    );
}

#[test]
fn zen_dark_draws_code_as_before() {
    let p = IdePalette::zen_dark();
    let old = |kind: HighlightKind| -> (Color32, bool) {
        match kind {
            HighlightKind::Keyword => (p.keyword, false),
            HighlightKind::String => (p.string, false),
            HighlightKind::Number => (p.number, false),
            HighlightKind::Comment => (p.comment, false),
            HighlightKind::DocComment => (p.doc_comment, true),
            HighlightKind::Macro => (p.macro_call, false),
            HighlightKind::Function => (p.function, false),
            HighlightKind::Type => (p.type_name, false),
            HighlightKind::Lifetime => (p.lifetime, false),
            HighlightKind::Attribute => (p.attribute, false),
            HighlightKind::Constant => (p.constant, true),
            HighlightKind::Punctuation => (p.code, false),
            HighlightKind::Heading => (p.heading, true),
            HighlightKind::Strong => (p.text, false),
            HighlightKind::Emphasis => (p.code, true),
            HighlightKind::Link => (p.function, false),
            HighlightKind::Code => (p.string, false),
            HighlightKind::Property | HighlightKind::GlobalVariable => {
                unreachable!("kinds added after this palette, not in the list below")
            }
        }
    };
    let kinds = [
        HighlightKind::Keyword,
        HighlightKind::String,
        HighlightKind::Number,
        HighlightKind::Comment,
        HighlightKind::DocComment,
        HighlightKind::Macro,
        HighlightKind::Function,
        HighlightKind::Type,
        HighlightKind::Lifetime,
        HighlightKind::Attribute,
        HighlightKind::Constant,
        HighlightKind::Punctuation,
        HighlightKind::Heading,
        HighlightKind::Strong,
        HighlightKind::Emphasis,
        HighlightKind::Link,
        HighlightKind::Code,
    ];
    for kind in kinds {
        let (color, italic) = old(kind);
        let expected = CodeStyle { italic, ..CodeStyle::plain(color) };
        assert_eq!(p.syntax(kind), expected, "{kind:?}");
    }
    assert_eq!(p.style(SyntaxKey::SemanticPackage), CodeStyle::plain(p.package));
    assert_eq!(p.style(SyntaxKey::SemanticType), CodeStyle::plain(p.semantic_type));
    assert_eq!(p.style(SyntaxKey::SemanticBuiltinType), CodeStyle::plain(p.keyword));
    assert_eq!(p.style(SyntaxKey::SemanticCall), CodeStyle::plain(p.call));
    assert_eq!(p.style(SyntaxKey::DocName), CodeStyle::plain(p.doc_name));
    assert_eq!(
        p.style(SyntaxKey::NavigationLink),
        CodeStyle { underline: Some(zen_theme::Underline::Line), ..CodeStyle::plain(p.link) }
    );
    assert_eq!(p.style(SyntaxKey::Default), CodeStyle::plain(p.code));
}

#[test]
fn projects_problems_and_completions_keep_their_colours() {
    let p = IdePalette::zen_dark();
    assert_eq!(p.project("zen-ide"), p.projects[project_index("zen-ide")]);
    assert_eq!(p.problem("error"), rgb(0xF7, 0x54, 0x64));
    assert_eq!(p.problem("warning"), rgb(0xE0, 0xB0, 0x4E));
    assert_eq!(p.problem("information"), rgb(0x6F, 0x9F, 0xE8));
    assert_eq!(p.problem("hint"), rgb(0x86, 0x8A, 0x91));
    assert_eq!(p.completion_mark("method"), ("f", rgb(0xC0, 0x8B, 0xF5)));
    assert_eq!(p.completion_mark("enumMember"), ("c", rgb(0x9A, 0xC6, 0x7A)));
    assert_eq!(p.completion_mark("module"), ("p", rgb(0xD0, 0x8C, 0x6A)));
    assert_eq!(p.completion_mark("unknown"), ("•", rgb(0xA0, 0xA4, 0xAC)));
}

fn project_index(name: &str) -> usize {
    let hash = name.bytes().fold(0u32, |hash, byte| hash.wrapping_mul(31).wrapping_add(u32::from(byte)));
    hash as usize % 6
}

#[test]
fn a_theme_changes_the_palette_and_its_styles() {
    let mut theme = Theme::new("mine", "Mine", true);
    theme.ui.insert(UiKey::Accent, Rgba::rgb(0xFF, 0, 0));
    theme.syntax.insert(SyntaxKey::Keyword, Style { bold: true, ..Style::color(Rgba::rgb(0, 0xFF, 0)) });
    let resolved = theme.resolve(|_| None).theme;
    let p = IdePalette::from_theme(&resolved);
    assert_eq!(p.accent, Color32::RED);
    assert_eq!(p.keyword, Color32::GREEN);
    assert!(p.syntax(HighlightKind::Keyword).bold);
    assert_eq!(p.frame, IdePalette::zen_dark().frame);
    assert_ne!(p, IdePalette::from_theme(&ResolvedTheme::zen_dark()));
}

#[test]
fn applying_a_palette_sets_egui_and_remembers_it() {
    let ctx = egui::Context::default();
    assert_eq!(IdePalette::of(&ctx), IdePalette::zen_dark());
    let mut theme = Theme::new("mine", "Mine", true);
    theme.ui.insert(UiKey::PopupBackground, Rgba::rgb(0x10, 0x20, 0x30));
    let p = IdePalette::from_theme(&theme.resolve(|_| None).theme);
    p.apply(&ctx);
    assert_eq!(IdePalette::of(&ctx), p);
    let visuals = ctx.global_style().visuals.clone();
    assert_eq!(visuals.window_fill, rgb(0x10, 0x20, 0x30));
    assert_eq!(visuals.selection.bg_fill, rgb(0x21, 0x42, 0x83));
    assert_eq!(visuals.text_cursor.stroke.color, rgb(0xCE, 0xD0, 0xD6));
    assert_eq!(visuals.panel_fill, p.frame);
}

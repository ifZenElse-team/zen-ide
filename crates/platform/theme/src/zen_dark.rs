use crate::zen_light::{ZEN_LIGHT_ID, zen_light};
use crate::{EditorKey, Rgba, Style, SyntaxKey, TerminalKey, Theme, UiKey};

pub const ZEN_DARK_ID: &str = "zen-dark";

pub const ZEN_DARK_UI: [(UiKey, Rgba); UiKey::COUNT] = [
    (UiKey::WindowBackground, Rgba::rgb(0x25, 0x28, 0x2C)),
    (UiKey::IslandBackground, Rgba::rgb(0x19, 0x1A, 0x1D)),
    (UiKey::TextPrimary, Rgba::rgb(0xDF, 0xE1, 0xE5)),
    (UiKey::TextSecondary, Rgba::rgb(0x86, 0x8A, 0x91)),
    (UiKey::TextError, Rgba::rgb(0xE5, 0x6E, 0x6E)),
    (UiKey::TextWarning, Rgba::rgb(0xE5, 0xA0, 0x4B)),
    (UiKey::Link, Rgba::rgb(0x6C, 0xA4, 0xF8)),
    (UiKey::Accent, Rgba::rgb(0x35, 0x74, 0xF0)),
    (UiKey::AccentForeground, Rgba::rgb(0xFF, 0xFF, 0xFF)),
    (UiKey::ListHover, Rgba::rgb(0x2C, 0x2E, 0x33)),
    (UiKey::ListSelection, Rgba::rgb(0x2E, 0x43, 0x6E)),
    (UiKey::SearchHighlight, Rgba::rgb(0x2F, 0x4C, 0x3A)),
    (UiKey::PopupBackground, Rgba::rgb(0x2B, 0x2D, 0x30)),
    (UiKey::PopupBorder, Rgba::rgb(0x43, 0x45, 0x4A)),
    (UiKey::PopupHover, Rgba::rgb(0x3A, 0x3D, 0x43)),
    (UiKey::PopupShadow, Rgba::new(0x00, 0x00, 0x00, 110)),
    (UiKey::PopupCodeBackground, Rgba::new(0x80, 0x80, 0x80, 0x30)),
    (UiKey::TextFieldBackground, Rgba::rgb(0x1E, 0x1F, 0x22)),
    (UiKey::TextFieldBorder, Rgba::rgb(0x4E, 0x51, 0x57)),
    (UiKey::TextFieldSelection, Rgba::rgb(0x21, 0x42, 0x83)),
    (UiKey::TextFieldCaret, Rgba::rgb(0xCE, 0xD0, 0xD6)),
    (UiKey::ButtonSecondaryBorder, Rgba::rgb(0x4E, 0x51, 0x57)),
    (UiKey::TabBorder, Rgba::rgb(0x4E, 0x51, 0x57)),
    (UiKey::TabActiveBackground, Rgba::rgb(0x27, 0x35, 0x55)),
    (UiKey::TabActiveBorder, Rgba::rgb(0x35, 0x4C, 0x85)),
    (UiKey::TabModified, Rgba::rgb(0x6C, 0xA4, 0xF8)),
    (UiKey::TreeExcludedText, Rgba::rgb(0xD8, 0x8A, 0x3C)),
    (UiKey::TreeExcludedBackground, Rgba::rgb(0x29, 0x1F, 0x1C)),
    (UiKey::TreeTestRow, Rgba::rgb(0x1E, 0x26, 0x1D)),
    (UiKey::VcsModified, Rgba::rgb(0x6C, 0xA4, 0xF8)),
    (UiKey::VcsAdded, Rgba::rgb(0x73, 0xBD, 0x79)),
    (UiKey::VcsDeleted, Rgba::rgb(0x6F, 0x73, 0x7A)),
    (UiKey::VcsUnversioned, Rgba::rgb(0xD1, 0x67, 0x5A)),
    (UiKey::VcsIgnored, Rgba::rgb(0xA8, 0xA3, 0x4F)),
    (UiKey::VcsConflict, Rgba::rgb(0xE5, 0x5C, 0x5C)),
    (UiKey::VcsIncoming, Rgba::rgb(0x6C, 0xA4, 0xF8)),
    (UiKey::VcsOutgoing, Rgba::rgb(0x73, 0xBD, 0x79)),
    (UiKey::VcsCurrentBranch, Rgba::rgb(0xE8, 0xB9, 0x4F)),
    (UiKey::VcsGraph0, Rgba::rgb(0x5F, 0x9E, 0xE8)),
    (UiKey::VcsGraph1, Rgba::rgb(0x6A, 0xB8, 0x6E)),
    (UiKey::VcsGraph2, Rgba::rgb(0xD9, 0x8C, 0x4A)),
    (UiKey::VcsGraph3, Rgba::rgb(0xB0, 0x7C, 0xD8)),
    (UiKey::VcsGraph4, Rgba::rgb(0x3F, 0xB8, 0xB0)),
    (UiKey::VcsGraph5, Rgba::rgb(0xD8, 0x6F, 0x7E)),
    (UiKey::BannerBackground, Rgba::rgb(0x25, 0x32, 0x4D)),
    (UiKey::IconRun, Rgba::rgb(0x5F, 0xB8, 0x65)),
    (UiKey::StripeActiveIcon, Rgba::rgb(0xFF, 0xFF, 0xFF)),
    (UiKey::ImageCheckerLight, Rgba::rgb(0x4A, 0x4D, 0x52)),
    (UiKey::ImageCheckerDark, Rgba::rgb(0x3A, 0x3C, 0x40)),
    (UiKey::Project0, Rgba::rgb(0x4A, 0x7F, 0xD6)),
    (UiKey::Project1, Rgba::rgb(0x3E, 0x8E, 0x7E)),
    (UiKey::Project2, Rgba::rgb(0xC0, 0x6E, 0x45)),
    (UiKey::Project3, Rgba::rgb(0x96, 0x5E, 0xC2)),
    (UiKey::Project4, Rgba::rgb(0x3F, 0x9A, 0xB0)),
    (UiKey::Project5, Rgba::rgb(0xB8, 0x5A, 0x8A)),
    (UiKey::ProjectForeground, Rgba::rgb(0xFF, 0xFF, 0xFF)),
];

pub const ZEN_DARK_EDITOR: [(EditorKey, Rgba); EditorKey::COUNT] = [
    (EditorKey::Background, Rgba::rgb(0x19, 0x1A, 0x1D)),
    (EditorKey::Foreground, Rgba::rgb(0xBC, 0xBE, 0xC4)),
    (EditorKey::LineNumber, Rgba::rgb(0x4B, 0x50, 0x59)),
    (EditorKey::CurrentLine, Rgba::rgb(0x1F, 0x20, 0x24)),
    (EditorKey::Caret, Rgba::rgb(0xCE, 0xD0, 0xD6)),
    (EditorKey::Selection, Rgba::rgb(0x2E, 0x43, 0x6E)),
    (EditorKey::SearchMatch, Rgba::rgb(0x2F, 0x4C, 0x3A)),
    (EditorKey::SearchMark, Rgba::rgb(0x5F, 0x8C, 0x63)),
    (EditorKey::FoldMarker, Rgba::rgb(0x3B, 0x3E, 0x44)),
    (EditorKey::FoldArrow, Rgba::rgb(0x70, 0x73, 0x79)),
    (EditorKey::FoldText, Rgba::rgb(0x86, 0x8A, 0x91)),
    (EditorKey::InlayHintBackground, Rgba::rgb(0x2B, 0x2D, 0x31)),
    (EditorKey::InlayHintForeground, Rgba::rgb(0x8C, 0x90, 0x97)),
    (EditorKey::GutterSeparator, Rgba::rgb(0x32, 0x34, 0x38)),
    (EditorKey::MethodSeparator, Rgba::rgb(0x43, 0x45, 0x4A)),
    (EditorKey::StickyBackground, Rgba::rgb(0x1D, 0x1E, 0x21)),
    (EditorKey::StickyBorder, Rgba::rgb(0x32, 0x34, 0x38)),
    (EditorKey::SymbolHighlight, Rgba::rgb(0x38, 0x3B, 0x39)),
    (EditorKey::SymbolHighlightWrite, Rgba::rgb(0x4A, 0x36, 0x3A)),
    (EditorKey::BracketMatchedBackground, Rgba::rgb(0x3B, 0x51, 0x4D)),
    (EditorKey::BracketMatchedForeground, Rgba::rgb(0xFF, 0xEF, 0x28)),
    (EditorKey::BracketUnmatchedBackground, Rgba::rgb(0x5E, 0x38, 0x38)),
    (EditorKey::BracketUnmatchedForeground, Rgba::rgb(0xBC, 0xBE, 0xC4)),
    (EditorKey::DiagnosticError, Rgba::rgb(0xF7, 0x54, 0x64)),
    (EditorKey::DiagnosticWarning, Rgba::rgb(0xE0, 0xB0, 0x4E)),
    (EditorKey::DiagnosticInfo, Rgba::rgb(0x6F, 0x9F, 0xE8)),
    (EditorKey::DiagnosticHint, Rgba::rgb(0x86, 0x8A, 0x91)),
    (EditorKey::DiagnosticTypo, Rgba::rgb(0x5F, 0xA3, 0x6A)),
    (EditorKey::CompletionFunction, Rgba::rgb(0xC0, 0x8B, 0xF5)),
    (EditorKey::CompletionVariable, Rgba::rgb(0x6F, 0xB3, 0xD9)),
    (EditorKey::CompletionField, Rgba::rgb(0xE0, 0xB0, 0x4E)),
    (EditorKey::CompletionConstant, Rgba::rgb(0x9A, 0xC6, 0x7A)),
    (EditorKey::CompletionType, Rgba::rgb(0x5F, 0xA8, 0xF0)),
    (EditorKey::CompletionInterface, Rgba::rgb(0x5F, 0xC8, 0xB0)),
    (EditorKey::CompletionModule, Rgba::rgb(0xD0, 0x8C, 0x6A)),
    (EditorKey::CompletionKeyword, Rgba::rgb(0xCF, 0x8E, 0x6D)),
    (EditorKey::CompletionSnippet, Rgba::rgb(0xA0, 0xA4, 0xAC)),
    (EditorKey::CompletionOther, Rgba::rgb(0xA0, 0xA4, 0xAC)),
    (EditorKey::GutterAdded, Rgba::rgb(0x54, 0x91, 0x59)),
    (EditorKey::GutterModified, Rgba::rgb(0x37, 0x5F, 0xAD)),
    (EditorKey::GutterDeleted, Rgba::rgb(0x86, 0x8A, 0x91)),
    (EditorKey::DiffInserted, Rgba::rgb(0x1F, 0x35, 0x26)),
    (EditorKey::DiffDeleted, Rgba::rgb(0x34, 0x36, 0x3A)),
    (EditorKey::DiffModified, Rgba::rgb(0x1E, 0x2E, 0x4B)),
    (EditorKey::DiffInsertedWord, Rgba::rgb(0x2C, 0x54, 0x37)),
    (EditorKey::DiffDeletedWord, Rgba::rgb(0x4A, 0x4D, 0x52)),
    (EditorKey::DiffModifiedWord, Rgba::rgb(0x2B, 0x45, 0x75)),
    (EditorKey::DiffFiller, Rgba::rgb(0x22, 0x23, 0x27)),
    (EditorKey::Breakpoint, Rgba::rgb(0xE5, 0x5C, 0x5C)),
    (EditorKey::BreakpointDisabled, Rgba::rgb(0xE5, 0x6E, 0x6E)),
    (EditorKey::ExecutionLine, Rgba::rgb(0x27, 0x35, 0x55)),
    (EditorKey::ExecutionArrow, Rgba::rgb(0x6C, 0xA4, 0xF8)),
    (EditorKey::AnnotationRecent, Rgba::rgb(0xE8, 0xB9, 0x4F)),
    (EditorKey::AnnotationOld, Rgba::rgb(0x5A, 0x5E, 0x66)),
    (EditorKey::CoverageCovered, Rgba::rgb(0x4E, 0x8A, 0x52)),
    (EditorKey::CoveragePartly, Rgba::rgb(0xB0, 0x8A, 0x2E)),
    (EditorKey::CoverageNotCovered, Rgba::rgb(0xA8, 0x4A, 0x48)),
    (EditorKey::Unused, Rgba::rgb(0x6F, 0x73, 0x7A)),
    (EditorKey::Whitespace, Rgba::rgb(0x4B, 0x4E, 0x54)),
    (EditorKey::IndentGuide, Rgba::rgb(0x2E, 0x30, 0x35)),
    (EditorKey::IndentGuideSelected, Rgba::rgb(0x50, 0x53, 0x5A)),
    (EditorKey::RightMargin, Rgba::rgb(0x39, 0x3B, 0x40)),
];

pub const ZEN_DARK_SYNTAX: [(SyntaxKey, Style); SyntaxKey::COUNT] = [
    (SyntaxKey::Keyword, Style::color(Rgba::rgb(0xCF, 0x8E, 0x6D))),
    (SyntaxKey::String, Style::color(Rgba::rgb(0x6A, 0xAB, 0x73))),
    (SyntaxKey::Number, Style::color(Rgba::rgb(0x2A, 0xAC, 0xB8))),
    (SyntaxKey::Comment, Style::color(Rgba::rgb(0x7A, 0x7E, 0x85))),
    (SyntaxKey::DocComment, Style::color(Rgba::rgb(0x5F, 0x82, 0x6B)).italic()),
    (SyntaxKey::DocName, Style::color(Rgba::rgb(0xA1, 0xA1, 0xA1))),
    (SyntaxKey::Macro, Style::color(Rgba::rgb(0x56, 0xA8, 0xF5))),
    (SyntaxKey::Type, Style::color(Rgba::rgb(0xBC, 0xBE, 0xC4))),
    (SyntaxKey::Function, Style::color(Rgba::rgb(0x56, 0xA8, 0xF5))),
    (SyntaxKey::Lifetime, Style::color(Rgba::rgb(0x20, 0x99, 0x9D))),
    (SyntaxKey::Constant, Style::color(Rgba::rgb(0xC7, 0x7D, 0xBB)).italic()),
    (SyntaxKey::Attribute, Style::color(Rgba::rgb(0xB3, 0xAE, 0x60))),
    (SyntaxKey::Punctuation, Style::color(Rgba::rgb(0xBC, 0xBE, 0xC4))),
    (SyntaxKey::Heading, Style::color(Rgba::rgb(0xC8, 0x7D, 0xBB)).italic()),
    (SyntaxKey::Strong, Style::color(Rgba::rgb(0xDF, 0xE1, 0xE5))),
    (SyntaxKey::Emphasis, Style::color(Rgba::rgb(0xBC, 0xBE, 0xC4)).italic()),
    (SyntaxKey::Link, Style::color(Rgba::rgb(0x56, 0xA8, 0xF5))),
    (SyntaxKey::Code, Style::color(Rgba::rgb(0x6A, 0xAB, 0x73))),
    (SyntaxKey::Todo, Style::color(Rgba::rgb(0x8F, 0xB8, 0xE8))),
    (SyntaxKey::SemanticPackage, Style::color(Rgba::rgb(0xB2, 0xBF, 0x85))),
    (SyntaxKey::SemanticType, Style::color(Rgba::rgb(0x7E, 0xAD, 0xBB))),
    (SyntaxKey::SemanticBuiltinType, Style::color(Rgba::rgb(0xCF, 0x8E, 0x6D))),
    (SyntaxKey::SemanticCall, Style::color(Rgba::rgb(0xAD, 0x9E, 0x7D))),
    (SyntaxKey::Property, Style::color(Rgba::rgb(0xC7, 0x7D, 0xBB))),
    (SyntaxKey::GlobalVariable, Style::color(Rgba::rgb(0xC7, 0x7D, 0xBB)).italic()),
    (SyntaxKey::NavigationLink, Style::color(Rgba::rgb(0x6C, 0xA4, 0xF8)).underlined()),
    (SyntaxKey::Default, Style::color(Rgba::rgb(0xBC, 0xBE, 0xC4))),
];

pub const ZEN_DARK_TERMINAL: [(TerminalKey, Rgba); TerminalKey::COUNT] = [
    (TerminalKey::Background, Rgba::rgb(0x1F, 0x20, 0x23)),
    (TerminalKey::Foreground, Rgba::rgb(0xDF, 0xE1, 0xE5)),
    (TerminalKey::Cursor, Rgba::rgb(0xCE, 0xD0, 0xD6)),
    (TerminalKey::Selection, Rgba::rgb(0x2E, 0x43, 0x6E)),
    (TerminalKey::Ansi0, Rgba::rgb(0x45, 0x48, 0x4D)),
    (TerminalKey::Ansi1, Rgba::rgb(0xE5, 0x8A, 0x86)),
    (TerminalKey::Ansi2, Rgba::rgb(0x93, 0xC4, 0x9A)),
    (TerminalKey::Ansi3, Rgba::rgb(0xE3, 0xC4, 0x7E)),
    (TerminalKey::Ansi4, Rgba::rgb(0x8A, 0xAF, 0xE6)),
    (TerminalKey::Ansi5, Rgba::rgb(0xCD, 0xA2, 0xDC)),
    (TerminalKey::Ansi6, Rgba::rgb(0x86, 0xC7, 0xC8)),
    (TerminalKey::Ansi7, Rgba::rgb(0xC4, 0xC6, 0xCC)),
    (TerminalKey::Ansi8, Rgba::rgb(0x6F, 0x73, 0x7A)),
    (TerminalKey::Ansi9, Rgba::rgb(0xF2, 0xA7, 0xA3)),
    (TerminalKey::Ansi10, Rgba::rgb(0xAD, 0xD5, 0xB2)),
    (TerminalKey::Ansi11, Rgba::rgb(0xEE, 0xD6, 0x9C)),
    (TerminalKey::Ansi12, Rgba::rgb(0xA9, 0xC6, 0xF2)),
    (TerminalKey::Ansi13, Rgba::rgb(0xDD, 0xBC, 0xE8)),
    (TerminalKey::Ansi14, Rgba::rgb(0xA6, 0xDA, 0xDB)),
    (TerminalKey::Ansi15, Rgba::rgb(0xF2, 0xF3, 0xF5)),
];

pub fn zen_dark() -> Theme {
    Theme {
        id: ZEN_DARK_ID.to_owned(),
        name: "Zen Dark".to_owned(),
        dark: true,
        parent: None,
        ui: ZEN_DARK_UI.into_iter().collect(),
        editor: ZEN_DARK_EDITOR.into_iter().collect(),
        syntax: ZEN_DARK_SYNTAX.into_iter().collect(),
        terminal: ZEN_DARK_TERMINAL.into_iter().collect(),
        unknown_keys: Vec::new(),
    }
}

pub const BUILTIN_IDS: [&str; 2] = [ZEN_DARK_ID, ZEN_LIGHT_ID];

pub fn builtin(id: &str) -> Option<Theme> {
    match id {
        ZEN_DARK_ID => Some(zen_dark()),
        ZEN_LIGHT_ID => Some(zen_light()),
        _ => None,
    }
}

pub fn base_id(dark: bool) -> &'static str {
    if dark { ZEN_DARK_ID } else { ZEN_LIGHT_ID }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;

    fn in_order<K: Key, V>(table: &[(K, V)]) {
        assert_eq!(table.iter().map(|(key, _)| *key).collect::<Vec<_>>(), K::ALL);
    }

    #[test]
    fn every_key_is_set_once_in_documentation_order() {
        in_order(&ZEN_DARK_UI);
        in_order(&ZEN_DARK_EDITOR);
        in_order(&ZEN_DARK_SYNTAX);
        in_order(&ZEN_DARK_TERMINAL);
    }

    #[test]
    fn zen_dark_and_zen_light_are_built_in() {
        assert_eq!(builtin(ZEN_DARK_ID), Some(zen_dark()));
        assert_eq!(builtin(ZEN_LIGHT_ID), Some(zen_light()));
        assert_eq!(builtin("zen-sepia"), None);
        assert_eq!(BUILTIN_IDS.map(|id| builtin(id).map(|theme| theme.dark)), [Some(true), Some(false)]);
        assert_eq!(base_id(true), ZEN_DARK_ID);
        assert_eq!(base_id(false), ZEN_LIGHT_ID);
    }
}

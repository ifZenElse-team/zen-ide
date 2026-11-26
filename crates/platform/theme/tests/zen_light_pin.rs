use zen_theme::{EditorKey, Key, ResolvedTheme, SyntaxKey, TerminalKey, UiKey, unreadable};

const UI: &[(&str, &str)] = &[
    ("window.background", "#EBECF0"),
    ("island.background", "#FFFFFF"),
    ("text.primary", "#1E1F22"),
    ("text.secondary", "#6C707E"),
    ("text.error", "#CC2E3E"),
    ("text.warning", "#9E6200"),
    ("link", "#2F5EB8"),
    ("accent", "#3574F0"),
    ("accent.foreground", "#FFFFFF"),
    ("list.hover", "#EDEEF1"),
    ("list.selection", "#D4E2FF"),
    ("searchHighlight", "#CCE8CF"),
    ("popup.background", "#FFFFFF"),
    ("popup.border", "#C9CCD6"),
    ("popup.hover", "#DFE7F8"),
    ("popup.shadow", "#00000038"),
    ("popup.code.background", "#80808020"),
    ("textField.background", "#FFFFFF"),
    ("textField.border", "#C9CCD6"),
    ("textField.selection", "#B4D0FF"),
    ("textField.caret", "#1E1F22"),
    ("button.secondary.border", "#C9CCD6"),
    ("tab.border", "#DFE1E5"),
    ("tab.active.background", "#E5ECFB"),
    ("tab.active.border", "#A9C1F2"),
    ("tab.modified", "#2F5EB8"),
    ("tree.excluded.text", "#A85700"),
    ("tree.excluded.background", "#FDF0E4"),
    ("tree.testRow", "#EEF7EE"),
    ("vcs.modified", "#2F5EB8"),
    ("vcs.added", "#2F7D32"),
    ("vcs.deleted", "#6C707E"),
    ("vcs.unversioned", "#B54434"),
    ("vcs.ignored", "#7A7514"),
    ("vcs.conflict", "#CC1F1F"),
    ("vcs.incoming", "#2F5EB8"),
    ("vcs.outgoing", "#2F7D32"),
    ("vcs.currentBranch", "#C88A00"),
    ("vcs.graph.0", "#3D7FD9"),
    ("vcs.graph.1", "#3E9B45"),
    ("vcs.graph.2", "#D97A2B"),
    ("vcs.graph.3", "#9654CF"),
    ("vcs.graph.4", "#1F9C93"),
    ("vcs.graph.5", "#D2506A"),
    ("banner.background", "#E7EEFC"),
    ("icon.run", "#3E9B45"),
    ("stripe.active.icon", "#FFFFFF"),
    ("image.checker.light", "#FFFFFF"),
    ("image.checker.dark", "#E6E7EA"),
    ("project.0", "#4A7FD6"),
    ("project.1", "#3E8E7E"),
    ("project.2", "#C06E45"),
    ("project.3", "#965EC2"),
    ("project.4", "#3F8AA0"),
    ("project.5", "#B85A8A"),
    ("project.foreground", "#FFFFFF"),
];

const EDITOR: &[(&str, &str)] = &[
    ("background", "#FFFFFF"),
    ("foreground", "#080808"),
    ("lineNumber", "#9A9EAB"),
    ("currentLine", "#F5F8FE"),
    ("caret", "#000000"),
    ("selection", "#A6D2FF"),
    ("search.match", "#F5E7A1"),
    ("search.mark", "#D9B530"),
    ("fold.marker", "#E4E5E8"),
    ("fold.arrow", "#8C8F99"),
    ("fold.text", "#5E626E"),
    ("inlayHint.background", "#EDEEF1"),
    ("inlayHint.foreground", "#5E626E"),
    ("gutter.separator", "#EBECF0"),
    ("methodSeparator", "#D3D5DB"),
    ("stickyLines.background", "#F7F8FA"),
    ("stickyLines.border", "#E1E3E8"),
    ("symbolHighlight", "#E4E4FF"),
    ("symbolHighlight.write", "#FCE2E4"),
    ("bracket.matched.background", "#99CCFF"),
    ("bracket.matched.foreground", "#080808"),
    ("bracket.unmatched.background", "#F7C6C6"),
    ("bracket.unmatched.foreground", "#080808"),
    ("diagnostic.error", "#D72B3F"),
    ("diagnostic.warning", "#B57A00"),
    ("diagnostic.info", "#3574F0"),
    ("diagnostic.hint", "#8C8F99"),
    ("diagnostic.typo", "#4E9A5B"),
    ("completion.function", "#8A4FD6"),
    ("completion.variable", "#1D7FAD"),
    ("completion.field", "#A86E00"),
    ("completion.constant", "#3E8A3E"),
    ("completion.type", "#2E6FD1"),
    ("completion.interface", "#1F8C77"),
    ("completion.module", "#B25C33"),
    ("completion.keyword", "#0033B3"),
    ("completion.snippet", "#6C707E"),
    ("completion.other", "#6C707E"),
    ("gutter.added", "#8FCB94"),
    ("gutter.modified", "#8EAEEB"),
    ("gutter.deleted", "#A8ABB3"),
    ("diff.inserted", "#DDF2DE"),
    ("diff.deleted", "#EAEBEE"),
    ("diff.modified", "#E3ECFC"),
    ("diff.insertedWord", "#B6E2B9"),
    ("diff.deletedWord", "#D0D3D9"),
    ("diff.modifiedWord", "#C2D5F8"),
    ("diff.filler", "#F4F5F7"),
    ("breakpoint", "#E55C5C"),
    ("breakpoint.disabled", "#E56E6E"),
    ("execution.line", "#DCE6FA"),
    ("execution.arrow", "#3574F0"),
    ("annotation.recent", "#9E6200"),
    ("annotation.old", "#A8ABB3"),
    ("coverage.covered", "#6CBF73"),
    ("coverage.partly", "#E8C25A"),
    ("coverage.notCovered", "#E87E7A"),
    ("unused", "#8C8C8C"),
    ("whitespace", "#BEC1C8"),
    ("indentGuide", "#EBECF0"),
    ("indentGuide.selected", "#C4C7CF"),
    ("rightMargin", "#E1E3E8"),
];

const SYNTAX: &[(&str, &str)] = &[
    ("keyword", "#0033B3"),
    ("string", "#067D17"),
    ("number", "#1750EB"),
    ("comment", "#7F7F7F"),
    ("docComment", "#5A7763 italic"),
    ("docComment.name", "#5E626E"),
    ("macro", "#1F5FA8"),
    ("type", "#080808"),
    ("function", "#00627A"),
    ("lifetime", "#208084"),
    ("constant", "#871094 italic"),
    ("attribute", "#806E0A"),
    ("punctuation", "#080808"),
    ("heading", "#871094 italic"),
    ("strong", "#000000"),
    ("emphasis", "#080808 italic"),
    ("link", "#2F5EB8"),
    ("code", "#067D17"),
    ("todo", "#1F5FA8"),
    ("semantic.package", "#4D6B1F"),
    ("semantic.type", "#007680"),
    ("semantic.builtinType", "#0033B3"),
    ("semantic.call", "#7A5B12"),
    ("property", "#871094"),
    ("globalVariable", "#871094 italic"),
    ("navigationLink", "#2F5EB8 underline"),
    ("default", "#080808"),
];

const TERMINAL: &[(&str, &str)] = &[
    ("background", "#FFFFFF"),
    ("foreground", "#1E1F22"),
    ("cursor", "#1E1F22"),
    ("selection", "#A6D2FF"),
    ("ansi.0", "#2B2D30"),
    ("ansi.1", "#C4372F"),
    ("ansi.2", "#2F7D32"),
    ("ansi.3", "#8F6400"),
    ("ansi.4", "#2D5FBF"),
    ("ansi.5", "#9B3FA6"),
    ("ansi.6", "#137A84"),
    ("ansi.7", "#9EA1AA"),
    ("ansi.8", "#6C707E"),
    ("ansi.9", "#D04A41"),
    ("ansi.10", "#3B8C40"),
    ("ansi.11", "#A07200"),
    ("ansi.12", "#3F75D6"),
    ("ansi.13", "#B04FBA"),
    ("ansi.14", "#1A8A94"),
    ("ansi.15", "#C4C7CF"),
];

fn check<K: Key>(pins: &[(&str, &str)], value: impl Fn(K) -> String) {
    let ids: Vec<&str> = pins.iter().map(|(id, _)| *id).collect();
    let keys: Vec<&str> = K::ALL.iter().map(|key| key.id()).collect();
    assert_eq!(ids, keys, "the pins of `{}` list every key once, in order", K::SECTION);
    for (id, expected) in pins {
        let key = K::from_id(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(value(key), *expected, "{}.{id}", K::SECTION);
    }
}

#[test]
fn zen_light_is_pinned_key_by_key() {
    let light = ResolvedTheme::zen_light();
    assert!(!light.dark);
    assert_eq!((light.id.as_str(), light.name.as_str()), ("zen-light", "Zen Light"));
    check::<UiKey>(UI, |key| light.ui(key).to_string());
    check::<EditorKey>(EDITOR, |key| light.editor(key).to_string());
    check::<SyntaxKey>(SYNTAX, |key| light.syntax(key).to_string());
    check::<TerminalKey>(TERMINAL, |key| light.terminal(key).to_string());
}

#[test]
fn the_built_in_light_theme_resolves_to_the_pins_and_reads() {
    let resolved = zen_theme::zen_light().resolve(|_| None);
    assert!(resolved.warnings.is_empty());
    assert_eq!(resolved.theme, ResolvedTheme::zen_light());
    let problems: Vec<String> = unreadable(&resolved.theme).iter().map(ToString::to_string).collect();
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn zen_light_is_light_where_zen_dark_is_dark() {
    let (dark, light) = (ResolvedTheme::zen_dark(), ResolvedTheme::zen_light());
    for key in [UiKey::WindowBackground, UiKey::IslandBackground, UiKey::PopupBackground, UiKey::TextFieldBackground] {
        assert!(!dark.ui(key).is_light() && light.ui(key).is_light(), "{key}");
    }
    for key in [EditorKey::Background, EditorKey::CurrentLine, EditorKey::DiffInserted, EditorKey::DiffModified] {
        assert!(!dark.editor(key).is_light() && light.editor(key).is_light(), "{key}");
    }
    assert!(light.terminal(TerminalKey::Background).is_light());
    assert!(!light.ui(UiKey::TextPrimary).is_light() && !light.editor(EditorKey::Foreground).is_light());
}

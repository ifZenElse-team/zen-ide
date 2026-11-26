use std::fmt;

pub trait Key: Copy + Ord + fmt::Debug + 'static {
    const SECTION: &'static str;
    const ALL: &'static [Self];

    fn id(self) -> &'static str;

    fn doc(self) -> &'static str;

    fn index(self) -> usize;

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|key| key.id() == id)
    }
}

macro_rules! keys {
    (
        $(#[doc = $type_doc:literal])*
        $name:ident, $section:literal {
            $($variant:ident = $id:literal, $doc:literal;)*
        }
    ) => {
        $(#[doc = $type_doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $(#[doc = $doc] $variant,)*
        }

        impl $name {
            pub const COUNT: usize = <$name as Key>::ALL.len();
        }

        impl Key for $name {
            const SECTION: &'static str = $section;
            const ALL: &'static [$name] = &[$($name::$variant,)*];

            fn id(self) -> &'static str {
                match self {
                    $($name::$variant => $id,)*
                }
            }

            fn doc(self) -> &'static str {
                match self {
                    $($name::$variant => $doc,)*
                }
            }

            fn index(self) -> usize {
                self as usize
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.id())
            }
        }
    };
}

keys! {
    UiKey, "ui" {
        WindowBackground = "window.background", "The window behind the panels: the main toolbar, the stripes and the status bar.";
        IslandBackground = "island.background", "The raised panels: tool windows and the frame of the editor.";
        TextPrimary = "text.primary", "Primary text.";
        TextSecondary = "text.secondary", "Secondary text: paths, hints, status bar items, inactive icons.";
        TextError = "text.error", "Error messages in dialogs and settings.";
        TextWarning = "text.warning", "Warnings in dialogs and settings, such as a shortcut conflict.";
        Link = "link", "Links and link-styled actions.";
        Accent = "accent", "The accent: primary buttons, focused fields, the active stripe button.";
        AccentForeground = "accent.foreground", "Text on the accent colour, such as a primary button's label.";
        ListHover = "list.hover", "A hovered row or button in a panel.";
        ListSelection = "list.selection", "A selected row in a list or tree.";
        SearchHighlight = "searchHighlight", "The background of the part of a label that matches a search, as in the Settings dialog.";
        PopupBackground = "popup.background", "Popups, menus and dialogs, which float a step lighter than the window.";
        PopupBorder = "popup.border", "The outline of popups, menus and dialogs.";
        PopupHover = "popup.hover", "A hovered item in a popup or menu.";
        PopupShadow = "popup.shadow", "The shadow under popups and dialogs.";
        PopupCodeBackground = "popup.code.background", "The background of code in documentation popups.";
        TextFieldBackground = "textField.background", "The inside of a text field in a dialog.";
        TextFieldBorder = "textField.border", "The outline of a text field that has no focus.";
        TextFieldSelection = "textField.selection", "Selected text in a text field.";
        TextFieldCaret = "textField.caret", "The caret in a text field.";
        ButtonSecondaryBorder = "button.secondary.border", "The outline of a secondary button.";
        TabBorder = "tab.border", "Borders of tabs, tool bars and panels.";
        TabActiveBackground = "tab.active.background", "The fill of the active editor tab.";
        TabActiveBorder = "tab.active.border", "The outline of the active editor tab.";
        TabModified = "tab.modified", "The name of an editor tab with unsaved changes.";
        TreeExcludedText = "tree.excluded.text", "Excluded folders, such as build output, in the project tree; also invalid input.";
        TreeExcludedBackground = "tree.excluded.background", "The row of an excluded folder in the project tree.";
        TreeTestRow = "tree.testRow", "The row of a test file in the project tree.";
        VcsModified = "vcs.modified", "A file changed since the last commit: its name in the project tree, the tabs and the change lists.";
        VcsAdded = "vcs.added", "A new file added to version control.";
        VcsDeleted = "vcs.deleted", "A deleted file.";
        VcsUnversioned = "vcs.unversioned", "A file not known to version control.";
        VcsIgnored = "vcs.ignored", "A file version control ignores.";
        VcsConflict = "vcs.conflict", "A file with merge conflicts.";
        VcsIncoming = "vcs.incoming", "The count of commits to pull, in the branches popup.";
        VcsOutgoing = "vcs.outgoing", "The count of commits to push, in the branches popup.";
        VcsCurrentBranch = "vcs.currentBranch", "The label icon of the current branch, in the branches popup.";
        VcsGraph0 = "vcs.graph.0", "The first lane colour of the log graph.";
        VcsGraph1 = "vcs.graph.1", "The second lane colour of the log graph.";
        VcsGraph2 = "vcs.graph.2", "The third lane colour of the log graph.";
        VcsGraph3 = "vcs.graph.3", "The fourth lane colour of the log graph.";
        VcsGraph4 = "vcs.graph.4", "The fifth lane colour of the log graph.";
        VcsGraph5 = "vcs.graph.5", "The sixth lane colour of the log graph.";
        BannerBackground = "banner.background", "Editor notifications, the banner above the editor.";
        IconRun = "icon.run", "The Run button and other run icons.";
        StripeActiveIcon = "stripe.active.icon", "The icon of the active tool window stripe button.";
        ImageCheckerLight = "image.checker.light", "The light squares of the board behind transparent images.";
        ImageCheckerDark = "image.checker.dark", "The dark squares of the board behind transparent images.";
        Project0 = "project.0", "The first project colour: a project's badge and the glow of the main toolbar.";
        Project1 = "project.1", "The second project colour.";
        Project2 = "project.2", "The third project colour.";
        Project3 = "project.3", "The fourth project colour.";
        Project4 = "project.4", "The fifth project colour.";
        Project5 = "project.5", "The sixth project colour.";
        ProjectForeground = "project.foreground", "The initials on a project's badge.";
    }
}

impl UiKey {
    pub const PROJECTS: [UiKey; 6] =
        [UiKey::Project0, UiKey::Project1, UiKey::Project2, UiKey::Project3, UiKey::Project4, UiKey::Project5];

    pub const GRAPH: [UiKey; 6] =
        [UiKey::VcsGraph0, UiKey::VcsGraph1, UiKey::VcsGraph2, UiKey::VcsGraph3, UiKey::VcsGraph4, UiKey::VcsGraph5];
}

keys! {
    EditorKey, "editor" {
        Background = "background", "The editor's background.";
        Foreground = "foreground", "Text that is not highlighted, such as a folded region's tail or a preview.";
        LineNumber = "lineNumber", "Line numbers in the gutter.";
        CurrentLine = "currentLine", "The line with the caret.";
        Caret = "caret", "The caret.";
        Selection = "selection", "Selected text.";
        SearchMatch = "search.match", "A search match.";
        SearchMark = "search.mark", "The marks of search matches by the scroll bar.";
        FoldMarker = "fold.marker", "The background of a folded region's mark.";
        FoldArrow = "fold.arrow", "The fold arrows in the gutter.";
        FoldText = "fold.text", "The text of a folded region's mark.";
        InlayHintBackground = "inlayHint.background", "The background of an inlay hint, such as a parameter name before an argument.";
        InlayHintForeground = "inlayHint.foreground", "The text of an inlay hint.";
        GutterSeparator = "gutter.separator", "The thin line between the gutter and the text.";
        MethodSeparator = "methodSeparator", "The thin line above each top-level declaration while Show method separators is on.";
        StickyBackground = "stickyLines.background", "The background of the sticky lines over the top of the editor.";
        StickyBorder = "stickyLines.border", "The line under the sticky lines.";
        SymbolHighlight = "symbolHighlight", "The background of the name at the caret and of its other occurrences in the file, and of the places the symbol is read.";
        SymbolHighlightWrite = "symbolHighlight.write", "The background of the places the symbol at the caret is written: declared, assigned or changed.";
        BracketMatchedBackground = "bracket.matched.background", "The background of the bracket next to the caret and of the bracket that matches it.";
        BracketMatchedForeground = "bracket.matched.foreground", "The bracket next to the caret and the bracket that matches it.";
        BracketUnmatchedBackground = "bracket.unmatched.background", "The background of the bracket next to the caret when nothing matches it.";
        BracketUnmatchedForeground = "bracket.unmatched.foreground", "The bracket next to the caret when nothing matches it.";
        DiagnosticError = "diagnostic.error", "An error.";
        DiagnosticWarning = "diagnostic.warning", "A warning.";
        DiagnosticInfo = "diagnostic.info", "Information.";
        DiagnosticHint = "diagnostic.hint", "A hint, or a problem of unknown severity.";
        DiagnosticTypo = "diagnostic.typo", "A misspelt word, from the spelling inspection: its wavy line and its mark.";
        CompletionFunction = "completion.function", "The mark of a function, method or constructor in the completion list.";
        CompletionVariable = "completion.variable", "The mark of a variable or value in the completion list.";
        CompletionField = "completion.field", "The mark of a field or property in the completion list.";
        CompletionConstant = "completion.constant", "The mark of a constant or enum member in the completion list.";
        CompletionType = "completion.type", "The mark of a struct, class or enum in the completion list.";
        CompletionInterface = "completion.interface", "The mark of an interface or type parameter in the completion list.";
        CompletionModule = "completion.module", "The mark of a module or package in the completion list.";
        CompletionKeyword = "completion.keyword", "The mark of a keyword in the completion list.";
        CompletionSnippet = "completion.snippet", "The mark of a snippet in the completion list.";
        CompletionOther = "completion.other", "The mark of any other kind of item in the completion list.";
        GutterAdded = "gutter.added", "The change marker in the gutter of lines added since the last commit.";
        GutterModified = "gutter.modified", "The change marker in the gutter of lines modified since the last commit.";
        GutterDeleted = "gutter.deleted", "The change marker in the gutter where lines were deleted since the last commit.";
        DiffInserted = "diff.inserted", "The background of an inserted line in the diff viewer.";
        DiffDeleted = "diff.deleted", "The background of a deleted line in the diff viewer.";
        DiffModified = "diff.modified", "The background of a modified line in the diff viewer.";
        DiffInsertedWord = "diff.insertedWord", "The background of the changed words of an inserted line in the diff viewer.";
        DiffDeletedWord = "diff.deletedWord", "The background of the changed words of a deleted line in the diff viewer.";
        DiffModifiedWord = "diff.modifiedWord", "The background of the changed words of a modified line in the diff viewer.";
        DiffFiller = "diff.filler", "The filler rows of the diff viewer across from lines the other side does not have.";
        Breakpoint = "breakpoint", "A line breakpoint in the gutter, drawn over the line number.";
        BreakpointDisabled = "breakpoint.disabled", "The outline of a disabled line breakpoint in the gutter.";
        ExecutionLine = "execution.line", "The background of the line where the debugger stopped.";
        ExecutionArrow = "execution.arrow", "The arrow in the gutter at the line where the debugger stopped.";
        AnnotationRecent = "annotation.recent", "Blame annotations of the newest commits in the gutter; older ones fade towards annotation.old.";
        AnnotationOld = "annotation.old", "Blame annotations of the oldest commits in the gutter.";
        CoverageCovered = "coverage.covered", "The coverage mark in the gutter of a line the tests ran, and its line in the HTML report.";
        CoveragePartly = "coverage.partly", "The coverage mark in the gutter of a line the tests ran only partly: some of its parts or branches did not run.";
        CoverageNotCovered = "coverage.notCovered", "The coverage mark in the gutter of a line the tests did not run.";
        Unused = "unused", "Unused code, such as an unused requirement or import: drawn in this colour instead of underlined.";
        Whitespace = "whitespace", "The dots of spaces and the marks of tabs while whitespaces are shown.";
        IndentGuide = "indentGuide", "The thin vertical lines at each indent level.";
        IndentGuideSelected = "indentGuide.selected", "The indent guide of the block the caret is in.";
        RightMargin = "rightMargin", "The vertical line at the right margin and the visual guides of the code style.";
    }
}

keys! {
    SyntaxKey, "syntax" {
        Keyword = "keyword", "A keyword, such as `fn` or `func`.";
        String = "string", "A string or character literal.";
        Number = "number", "A number literal.";
        Comment = "comment", "A comment.";
        DocComment = "docComment", "A documentation comment.";
        DocName = "docComment.name", "The name a documentation comment starts with, such as `Open` in `// Open opens a file`.";
        Macro = "macro", "A macro invocation, such as `println!`.";
        Type = "type", "A type or trait name, as the highlighter sees it.";
        Function = "function", "A function name in a definition or call.";
        Lifetime = "lifetime", "A lifetime or label, such as `'a`.";
        Constant = "constant", "A constant or static.";
        Attribute = "attribute", "An attribute, such as `#[derive(Debug)]`.";
        Punctuation = "punctuation", "Punctuation and operators.";
        Heading = "heading", "A heading in a markup language, such as `# Title` in Markdown.";
        Strong = "strong", "Strong text in a markup language.";
        Emphasis = "emphasis", "Emphasised text in a markup language.";
        Link = "link", "A link's text in a markup language.";
        Code = "code", "Code inside a markup language: inline code and code blocks.";
        Todo = "todo", "TODO text in a comment, as the patterns of Settings → Editor → TODO find it.";
        SemanticPackage = "semantic.package", "A package name, from the language server.";
        SemanticType = "semantic.type", "A type that is not built in, from the language server.";
        SemanticBuiltinType = "semantic.builtinType", "A built-in type, from the language server.";
        SemanticCall = "semantic.call", "A call of a function or a method, from the language server; declarations keep `function`.";
        Property = "property", "A property or field where it is declared, such as `name` in `{ name: string }`.";
        GlobalVariable = "globalVariable", "A global variable, such as `console`, `window` or a module-level variable.";
        NavigationLink = "navigationLink", "The name under the pointer while ⌘ (Ctrl) is held, which a click goes to.";
        Default = "default", "Code that no other key covers.";
    }
}

keys! {
    TerminalKey, "terminal" {
        Background = "background", "The default background.";
        Foreground = "foreground", "The default text colour.";
        Cursor = "cursor", "The block cursor.";
        Selection = "selection", "Selected text.";
        Ansi0 = "ansi.0", "ANSI black.";
        Ansi1 = "ansi.1", "ANSI red.";
        Ansi2 = "ansi.2", "ANSI green.";
        Ansi3 = "ansi.3", "ANSI yellow.";
        Ansi4 = "ansi.4", "ANSI blue.";
        Ansi5 = "ansi.5", "ANSI magenta.";
        Ansi6 = "ansi.6", "ANSI cyan.";
        Ansi7 = "ansi.7", "ANSI white.";
        Ansi8 = "ansi.8", "ANSI bright black.";
        Ansi9 = "ansi.9", "ANSI bright red.";
        Ansi10 = "ansi.10", "ANSI bright green.";
        Ansi11 = "ansi.11", "ANSI bright yellow.";
        Ansi12 = "ansi.12", "ANSI bright blue.";
        Ansi13 = "ansi.13", "ANSI bright magenta.";
        Ansi14 = "ansi.14", "ANSI bright cyan.";
        Ansi15 = "ansi.15", "ANSI bright white.";
    }
}

impl TerminalKey {
    pub const ANSI: [TerminalKey; 16] = [
        TerminalKey::Ansi0,
        TerminalKey::Ansi1,
        TerminalKey::Ansi2,
        TerminalKey::Ansi3,
        TerminalKey::Ansi4,
        TerminalKey::Ansi5,
        TerminalKey::Ansi6,
        TerminalKey::Ansi7,
        TerminalKey::Ansi8,
        TerminalKey::Ansi9,
        TerminalKey::Ansi10,
        TerminalKey::Ansi11,
        TerminalKey::Ansi12,
        TerminalKey::Ansi13,
        TerminalKey::Ansi14,
        TerminalKey::Ansi15,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check<K: Key>() {
        for (index, key) in K::ALL.iter().enumerate() {
            assert_eq!(key.index(), index, "{key:?}");
            assert_eq!(K::from_id(key.id()), Some(*key));
            assert!(!K::ALL[index + 1..].iter().any(|other| other.id() == key.id()), "{} twice", key.id());
            assert!(key.doc().ends_with('.') && !key.doc().contains('\n'), "{}: one sentence", key.id());
        }
        assert_eq!(K::from_id("no.such.key"), None);
    }

    #[test]
    fn ids_are_unique_and_indexes_match() {
        check::<UiKey>();
        check::<EditorKey>();
        check::<SyntaxKey>();
        check::<TerminalKey>();
    }

    #[test]
    fn ansi_keys_are_in_order() {
        for (index, key) in TerminalKey::ANSI.iter().enumerate() {
            assert_eq!(key.id(), format!("ansi.{index}"));
        }
        for (index, key) in UiKey::PROJECTS.iter().enumerate() {
            assert_eq!(key.id(), format!("project.{index}"));
        }
    }
}

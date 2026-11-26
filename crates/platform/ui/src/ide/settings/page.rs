use zen_modules::{CodeStylePage, SettingsPage};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(in crate::ide) enum PageId {
    ProjectStructure,
    #[default]
    Appearance,
    MenusAndToolbars,
    SystemSettings,
    HttpProxy,
    ServerCertificates,
    Notifications,
    TrustedLocations,
    Keymap,
    Editor,
    EditorGeneral,
    CodeFolding,
    PostfixCompletion,
    EditorTabs,
    EditorFont,
    CodeStyle,
    CodeStyleOf(&'static str),
    CodeStyleOther,
    InlayHints,
    CodeVision,
    Inspections,
    FileTemplates,
    LiveTemplates,
    FileTypes,
    FileEncodings,
    NaturalLanguages,
    Spelling,
    Todo,
    Plugins,
    VersionControl,
    Build,
    Debugger,
    DataViews,
    Languages,
    Markdown,
    Module(&'static str),
    LanguageServers,
    Schemas,
    Tools,
    ActionsOnSave,
    ExternalTools,
    FileWatchers,
    Caffeine,
    Terminal,
    Mcp,
    Advanced,
}

impl PageId {
    #[cfg(test)]
    pub(in crate::ide) const ALL: &[PageId] = &[
        PageId::ProjectStructure,
        PageId::Appearance,
        PageId::MenusAndToolbars,
        PageId::SystemSettings,
        PageId::HttpProxy,
        PageId::ServerCertificates,
        PageId::Notifications,
        PageId::TrustedLocations,
        PageId::Keymap,
        PageId::Editor,
        PageId::EditorGeneral,
        PageId::CodeFolding,
        PageId::EditorTabs,
        PageId::PostfixCompletion,
        PageId::EditorFont,
        PageId::CodeStyle,
        PageId::CodeStyleOther,
        PageId::InlayHints,
        PageId::CodeVision,
        PageId::Inspections,
        PageId::FileTemplates,
        PageId::LiveTemplates,
        PageId::FileTypes,
        PageId::FileEncodings,
        PageId::NaturalLanguages,
        PageId::Spelling,
        PageId::Todo,
        PageId::Plugins,
        PageId::VersionControl,
        PageId::Build,
        PageId::Debugger,
        PageId::DataViews,
        PageId::Languages,
        PageId::Markdown,
        PageId::LanguageServers,
        PageId::Schemas,
        PageId::Tools,
        PageId::ActionsOnSave,
        PageId::ExternalTools,
        PageId::FileWatchers,
        PageId::Caffeine,
        PageId::Terminal,
        PageId::Mcp,
        PageId::Advanced,
    ];

    pub(in crate::ide) fn name(self) -> String {
        match self {
            PageId::Module(id) => id.to_owned(),
            PageId::CodeStyleOf(language) => {
                let mut chars = language.chars();
                let first = chars.next().map(|first| first.to_ascii_uppercase().to_string()).unwrap_or_default();
                format!("codeStyle{first}{}", chars.as_str())
            }
            page => {
                let debug = format!("{page:?}");
                let mut chars = debug.chars();
                chars.next().map(|first| first.to_ascii_lowercase().to_string()).unwrap_or_default() + chars.as_str()
            }
        }
    }

    pub(in crate::ide) fn from_name(name: &str, sections: &[SectionDef]) -> Option<PageId> {
        sections.iter().map(|section| section.page).find(|page| page.name() == name)
    }

    pub(in crate::ide) fn section_in(self, sections: &[SectionDef]) -> SectionDef {
        let find = |page| sections.iter().find(|section| section.page == page).copied();
        find(self).or_else(|| find(PageId::default())).unwrap_or(SECTIONS[0])
    }
}

pub(in crate::ide) fn sections(pages: &[SettingsPage], styles: &[CodeStylePage]) -> Vec<SectionDef> {
    let mut tree = Vec::with_capacity(SECTIONS.len() + pages.len() + styles.len());
    for section in SECTIONS {
        if section.page == PageId::CodeStyleOther {
            tree.extend(styles.iter().map(|style| SectionDef {
                key: style.title,
                level: 2,
                page: PageId::CodeStyleOf(style.language),
                search_keys: CODE_STYLE_SEARCH,
            }));
            tree.push(SectionDef {
                key: "settings-section-code-style-sql",
                level: 2,
                page: PageId::CodeStyleOf(SQL),
                search_keys: SQL_STYLE_SEARCH,
            });
        }
        if section.page == PageId::Markdown {
            tree.extend(pages.iter().filter(|page| page.parent.is_empty()).map(|page| SectionDef {
                key: page.title,
                level: 1,
                page: PageId::Module(page.id),
                search_keys: page.search,
            }));
        }
        tree.push(*section);
    }
    for page in pages.iter().filter(|page| !page.parent.is_empty()) {
        let (at, level) = match tree.iter().position(|section| section.page.name() == page.parent) {
            Some(parent) => {
                let level = tree[parent].level + 1;
                let end = tree[parent + 1..].iter().position(|section| section.level < level);
                (end.map_or(tree.len(), |end| parent + 1 + end), level)
            }
            None => (tree.iter().position(|section| section.page == PageId::LanguageServers).unwrap_or(tree.len()), 1),
        };
        tree.insert(at, SectionDef { key: page.title, level, page: PageId::Module(page.id), search_keys: page.search });
    }
    tree
}

pub(in crate::ide) const CODE_FOLDING_SEARCH: &[&str] = &[
    "settings-folding-show-arrows",
    "settings-folding-arrows-always",
    "settings-folding-arrows-hover",
    "settings-folding-bottom-arrows",
    "settings-folding-by-default",
    "settings-folding-general",
    "settings-folding-file-header",
    "settings-folding-imports",
    "settings-folding-doc-comments",
    "settings-folding-method-bodies",
    "settings-folding-custom-regions",
];

const CODE_STYLE_SEARCH: &[&str] = &[
    "settings-code-style-tabs",
    "settings-code-style-tab-spaces",
    "settings-code-style-tab-wrapping",
    "settings-code-style-tab-blank-lines",
    "settings-code-style-tab-imports",
    "settings-code-style-tab-arrangement",
    "settings-code-style-arrangement-order",
    "settings-code-style-use-tabs",
    "settings-code-style-tab-size",
    "settings-code-style-indent",
];

pub(in crate::ide) const SQL: &str = "sql";

const SQL_STYLE_SEARCH: &[&str] = &[
    "settings-code-style-tabs",
    "settings-code-style-use-tabs",
    "settings-code-style-tab-size",
    "settings-code-style-indent",
    "settings-code-style-sql-formatting",
    "settings-code-style-sql-keyword-case",
    "settings-code-style-sql-commas",
    "settings-code-style-sql-clause-own-line",
    "settings-code-style-sql-item-per-line",
];

#[derive(Clone, Copy, Debug)]
pub(in crate::ide) struct SectionDef {
    pub(in crate::ide) key: &'static str,
    pub(in crate::ide) level: u8,
    pub(in crate::ide) page: PageId,
    pub(in crate::ide) search_keys: &'static [&'static str],
}

const fn section(key: &'static str, level: u8, page: PageId, search_keys: &'static [&'static str]) -> SectionDef {
    SectionDef { key, level, page, search_keys }
}

pub(in crate::ide) const SECTIONS: &[SectionDef] = &[
    section(
        "settings-project-structure",
        0,
        PageId::ProjectStructure,
        &[
            "settings-project-structure-mark-as",
            "settings-project-structure-excluded",
            "settings-project-structure-resource-root",
            "settings-project-structure-excluded-folders",
            "settings-project-structure-resource-folders",
            "settings-project-structure-exclude-files",
            "settings-project-structure-modules",
            "settings-project-structure-libraries",
            "settings-project-structure-artifacts",
            "settings-project-structure-sdks",
            "settings-project-structure-language-level",
            "settings-project-structure-compiler-output",
            "settings-project-structure-dependencies",
        ],
    ),
    section(
        "settings-appearance",
        0,
        PageId::Appearance,
        &[
            "settings-theme",
            "settings-theme-load",
            "settings-ui-font-custom",
            "settings-ui-font-size",
            "settings-accessibility",
            "settings-ide-zoom",
            "settings-ide-zoom-reset",
            "settings-presentation",
            "settings-presentation-font-size",
            "settings-language",
        ],
    ),
    section(
        "settings-menus",
        1,
        PageId::MenusAndToolbars,
        &[
            "settings-menus-main-menu",
            "settings-menus-main-toolbar",
            "settings-menus-editor-popup",
            "settings-menus-gutter-popup",
            "settings-menus-tab-popup",
            "settings-menus-project-popup",
        ],
    ),
    section(
        "settings-system",
        1,
        PageId::SystemSettings,
        &[
            "settings-system-confirm-exit",
            "settings-system-confirm-exit-note",
            "settings-open-project",
            "settings-open-window",
            "settings-open-current",
            "settings-open-ask",
            "settings-open-note",
            "settings-default-project-directory",
            "settings-default-project-directory-note",
            "settings-generators",
            "settings-generators-network",
            "settings-generators-network-note",
            "settings-system-whats-new",
            "settings-system-tips",
            "settings-system-files",
            "settings-system-autosave-idle",
            "settings-system-autosave-seconds",
            "settings-system-autosave-deactivation",
            "settings-system-autosave-note",
        ],
    ),
    section(
        "settings-proxy",
        2,
        PageId::HttpProxy,
        &[
            "settings-proxy-none",
            "settings-proxy-auto",
            "settings-proxy-pac",
            "settings-proxy-clear-passwords",
            "settings-proxy-manual",
            "settings-proxy-host",
            "settings-proxy-port",
            "settings-proxy-no-proxy-for",
            "settings-proxy-authentication",
            "settings-proxy-login",
            "settings-proxy-password",
            "settings-proxy-remember",
            "settings-proxy-check",
        ],
    ),
    section(
        "settings-certificates",
        2,
        PageId::ServerCertificates,
        &[
            "settings-certificates-auto",
            "settings-certificates-accepted",
            "settings-certificates-empty",
            "settings-certificates-issued-to",
            "settings-certificates-issued-by",
            "settings-certificates-fingerprint",
        ],
    ),
    section(
        "settings-notifications",
        1,
        PageId::Notifications,
        &["settings-notifications-balloons", "settings-notifications-groups", "settings-notifications-note"],
    ),
    section(
        "settings-trusted-locations",
        1,
        PageId::TrustedLocations,
        &["settings-trusted-locations-about", "settings-trusted-locations-empty", "settings-trusted-locations-home"],
    ),
    section("settings-keymap", 0, PageId::Keymap, &[]),
    section(
        "settings-editor",
        0,
        PageId::Editor,
        &[
            "settings-editor-mouse",
            super::editor::WHEEL_FONT_SIZE,
            "settings-editor-wheel-active",
            "settings-editor-wheel-all",
            "settings-editor-smart-keys",
            "settings-editor-camel-humps",
            "settings-editor-camel-humps-double-click",
            "settings-editor-smart-keys-web",
            "settings-editor-closing-tags",
            "settings-editor-gutter",
            "settings-editor-run-icons",
            "settings-editor-soft-wraps",
            "settings-editor-soft-wrap-files",
            "settings-editor-appearance",
            "settings-editor-code-structure",
            "settings-editor-hard-wrap-guides",
            "settings-editor-line-numbers",
            "settings-editor-line-numbers-absolute",
            "settings-editor-line-numbers-relative",
            "settings-editor-line-numbers-hybrid",
            "settings-editor-method-separators",
            "settings-editor-indent-guides",
            "settings-editor-presentation",
            "settings-editor-whitespaces",
            "settings-editor-whitespaces-leading",
            "settings-editor-whitespaces-inner",
            "settings-editor-whitespaces-trailing",
            "settings-editor-whitespaces-selection",
            "settings-editor-breadcrumbs",
            "settings-editor-breadcrumbs-show",
            "settings-editor-sticky",
            "settings-editor-sticky-show",
            "settings-editor-sticky-max",
            "settings-editor-limits",
            "settings-editor-clipboard-limit",
            "settings-editor-console",
            "settings-editor-console-buffer-override",
            "settings-editor-console-buffer-kb",
            "settings-editor-on-save",
            "settings-editor-on-save-trailing-spaces",
            "settings-editor-on-save-modified-lines",
            "settings-editor-on-save-all-lines",
            "settings-editor-on-save-keep-caret-line",
            "settings-editor-on-save-blank-lines",
            "settings-editor-on-save-line-break",
        ],
    ),
    section(
        "settings-section-general",
        1,
        PageId::EditorGeneral,
        &["settings-section-code-folding", "settings-section-postfix-completion"],
    ),
    section("settings-section-code-folding", 2, PageId::CodeFolding, CODE_FOLDING_SEARCH),
    section("settings-section-editor-tabs", 2, PageId::EditorTabs, super::editor_tabs::SEARCH),
    section(
        "settings-section-postfix-completion",
        2,
        PageId::PostfixCompletion,
        &["settings-postfix-enable", "settings-postfix-note"],
    ),
    section(
        "settings-section-font",
        1,
        PageId::EditorFont,
        &[
            "settings-font-family",
            "settings-font-size",
            "settings-font-line-height",
            "settings-editor-ligatures",
            "settings-font-preview",
        ],
    ),
    section(
        "settings-section-code-style",
        1,
        PageId::CodeStyle,
        &[
            "settings-code-style-general",
            "settings-code-style-formatter",
            "settings-code-style-hard-wrap",
            "settings-code-style-wrap-on-typing",
            "settings-code-style-visual-guides",
            "settings-code-style-do-not-format",
            "settings-code-style-formatter-markers",
            "settings-code-style-line-separator",
            "settings-code-style-editorconfig",
            "settings-code-style-indents-detection",
            "settings-code-style-detect-indents",
            "settings-code-style-scheme",
            "settings-code-style-scheme-default",
            "settings-code-style-scheme-project",
            "settings-code-style-scheme-note",
        ],
    ),
    section(
        "settings-section-code-style-other",
        2,
        PageId::CodeStyleOther,
        &[
            "settings-code-style-tabs",
            "settings-code-style-use-tabs",
            "settings-code-style-tab-size",
            "settings-code-style-indent",
            "settings-code-style-json",
            "settings-code-style-yaml",
            "settings-code-style-toml",
            "settings-code-style-markdown",
        ],
    ),
    section(
        "settings-section-inlay-hints",
        1,
        PageId::InlayHints,
        &[
            "settings-inlay-hints-show",
            "settings-inlay-hints-kinds",
            "settings-inlay-hints-parameter-names",
            "settings-inlay-hints-field-names",
            "settings-inlay-hints-variable-types",
            "settings-inlay-hints-literal-types",
            "settings-inlay-hints-type-arguments",
            "settings-inlay-hints-constant-values",
        ],
    ),
    section(
        "settings-section-code-vision",
        2,
        PageId::CodeVision,
        &["settings-code-vision-show", "settings-code-vision-usages", "settings-code-vision-authors"],
    ),
    section(
        "settings-section-inspections",
        1,
        PageId::Inspections,
        &[
            "settings-inspections-proofreading",
            "settings-inspections-spelling",
            "settings-inspections-process-code",
            "settings-inspections-process-literals",
            "settings-inspections-process-comments",
            "settings-inspections-profile",
            "settings-inspections-only-cleanup",
        ],
    ),
    section(
        "settings-section-file-templates",
        1,
        PageId::FileTemplates,
        &[
            "settings-file-templates-scheme",
            "settings-file-templates-files",
            "settings-file-templates-includes",
            "settings-file-templates-code",
            "settings-file-templates-create",
            "settings-file-templates-create-child",
            "settings-file-templates-reset",
            "settings-file-templates-description",
        ],
    ),
    section(
        "settings-section-live-templates",
        1,
        PageId::LiveTemplates,
        &[
            "settings-templates-abbreviation",
            "settings-templates-description",
            "settings-templates-text",
            "settings-templates-variables",
        ],
    ),
    section(
        "settings-section-file-types",
        1,
        PageId::FileTypes,
        &[
            "settings-file-types-recognized",
            "settings-file-types-ignored",
            "settings-file-types-patterns",
            "settings-file-types-hashbangs",
            "settings-file-types-contents",
            "settings-file-types-ignored-note",
        ],
    ),
    section(
        "settings-section-file-encodings",
        1,
        PageId::FileEncodings,
        &[
            "settings-file-encodings-global",
            "settings-file-encodings-project",
            "settings-file-encodings-table",
            "settings-file-encodings-create",
        ],
    ),
    section(
        "settings-section-natural-languages",
        1,
        PageId::NaturalLanguages,
        &["settings-natural-languages-english", "settings-natural-languages-configure"],
    ),
    section(
        "settings-section-spelling",
        2,
        PageId::Spelling,
        &[
            "settings-spelling-single",
            "settings-spelling-accepted",
            "settings-spelling-custom",
            "settings-spelling-project-level",
            "settings-spelling-application-level",
            "settings-spelling-application-dictionary",
            "settings-spelling-project-dictionary",
            "settings-spelling-configure",
        ],
    ),
    section("settings-section-todo", 1, PageId::Todo, super::todo::SEARCH),
    section(
        "settings-plugins",
        0,
        PageId::Plugins,
        &["settings-plugins-marketplace", "settings-plugins-installed", "settings-plugins-built-in"],
    ),
    section(
        "settings-vcs",
        0,
        PageId::VersionControl,
        &["settings-vcs-workspace", "settings-vcs-avatars", "settings-vcs-avatars-note"],
    ),
    section(
        "settings-build",
        0,
        PageId::Build,
        &[
            "settings-build-tools",
            "settings-build-reload-after",
            "settings-build-reload-any",
            "settings-build-reload-external",
        ],
    ),
    section(
        "settings-debugger",
        1,
        PageId::Debugger,
        &[
            "settings-debugger-remove",
            "settings-debugger-remove-click",
            "settings-debugger-remove-drag",
            "settings-debugger-coroutines",
            "settings-debugger-coroutines-debug-mode",
        ],
    ),
    section(
        "settings-debugger-data-views",
        2,
        PageId::DataViews,
        &[
            "settings-debugger-inline-values",
            "settings-debugger-collection-views",
            "settings-debugger-value-tooltip",
            "settings-debugger-tooltip-delay",
        ],
    ),
    section("settings-languages", 0, PageId::Languages, &[]),
    section(
        "settings-section-markdown",
        1,
        PageId::Markdown,
        &["settings-markdown-preview-layout", "settings-markdown-sync-scroll", "settings-markdown-preview-font-size"],
    ),
    section(
        "settings-language-servers",
        1,
        PageId::LanguageServers,
        &["settings-lsp-enabled", "settings-lsp-path", "settings-lsp-arguments", "settings-lsp-undismiss"],
    ),
    section(
        "settings-schemas",
        1,
        PageId::Schemas,
        &["settings-schemas-shipped", "settings-schemas-own", "settings-schemas-downloads"],
    ),
    section("settings-tools", 0, PageId::Tools, &[]),
    section(
        "settings-actions-on-save",
        1,
        PageId::ActionsOnSave,
        &[
            "settings-on-save-reformat",
            "settings-on-save-optimize-imports",
            "settings-on-save-file-watcher",
            "settings-on-save-autosave",
        ],
    ),
    section(
        "settings-external-tools",
        1,
        PageId::ExternalTools,
        &[
            "settings-external-tools-empty",
            "settings-external-tools-name",
            "settings-external-tools-program",
            "settings-external-tools-arguments",
            "settings-external-tools-working-directory",
            "settings-external-tools-synchronize",
            "settings-external-tools-open-console",
            "settings-external-tools-filters",
        ],
    ),
    section(
        "settings-file-watchers",
        1,
        PageId::FileWatchers,
        &[
            "settings-file-watchers-empty",
            "settings-file-watchers-name",
            "settings-file-watchers-file-type",
            "settings-file-watchers-scope",
            "settings-file-watchers-program",
            "settings-file-watchers-arguments",
            "settings-file-watchers-output-paths",
            "settings-file-watchers-working-directory",
            "settings-file-watchers-environment",
            "settings-file-watchers-auto-save",
            "settings-file-watchers-external",
            "settings-file-watchers-syntax-errors",
            "settings-file-watchers-stdout",
            "settings-file-watchers-show-console",
            "settings-file-watchers-filters",
        ],
    ),
    section(
        "settings-caffeine",
        1,
        PageId::Caffeine,
        &[
            "settings-caffeine-what",
            "settings-caffeine-length",
            "settings-caffeine-custom-minutes",
            "settings-caffeine-while",
            "settings-caffeine-battery",
            "settings-caffeine-low-level",
            "settings-caffeine-start-on-launch",
            "ide-caffeine-what-display",
            "ide-caffeine-off-on-battery",
            "ide-caffeine-resume",
        ],
    ),
    section(
        "settings-terminal",
        1,
        PageId::Terminal,
        &[
            "settings-terminal-sessions",
            "settings-terminal-restore",
            "settings-terminal-environment",
            "settings-terminal-login-shell",
            "settings-terminal-agents",
            "settings-terminal-links",
            "settings-terminal-links-url",
            "settings-terminal-links-file",
        ],
    ),
    section(
        "settings-mcp",
        1,
        PageId::Mcp,
        &["settings-mcp-enable", "settings-mcp-port", "settings-mcp-language", "settings-mcp-connect"],
    ),
    section(
        "settings-advanced",
        0,
        PageId::Advanced,
        &[
            "settings-advanced-editor",
            "settings-advanced-zoom-indicator",
            "settings-advanced-tab-rendering",
            "settings-advanced-extend-whitespace",
            "settings-advanced-bookmarks",
            "settings-advanced-popup-line-only",
            "settings-advanced-debugger",
            "settings-advanced-recent-expressions",
            "settings-advanced-run-to-cursor-inlay",
            "settings-advanced-search-everywhere",
            "settings-advanced-wait-for-all",
            "settings-advanced-text-on-all",
            "settings-advanced-editor-tabs",
            "settings-advanced-declaration-same-tab",
            "settings-advanced-pinned-left",
            "settings-advanced-find-in-files",
            "settings-advanced-fif-limit",
            "settings-advanced-fif-too-many",
            "settings-advanced-run-debug",
            "settings-advanced-confirm-rerun",
            "settings-advanced-temporary-limit",
            "settings-advanced-project-view",
            "settings-advanced-scratches",
            "settings-advanced-markdown",
            "settings-advanced-markdown-hide-toolbar",
            "settings-advanced-user-interface",
            "settings-advanced-double-modifiers",
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    const ENGLISH: &[&str] = &[
        include_str!("../../../locales/en/ui.ftl"),
        include_str!("../../../locales/en/settings-appearance.ftl"),
        include_str!("../../../locales/en/settings-editor.ftl"),
        include_str!("../../../locales/en/settings-modules.ftl"),
        include_str!("../../../locales/en/settings-search.ftl"),
        include_str!("../../../locales/en/caffeine.ftl"),
        include_str!("../../../locales/en/external-tools.ftl"),
        include_str!("../../../locales/en/file-watchers.ftl"),
    ];

    fn defined(id: &str) -> bool {
        ENGLISH.iter().any(|text| text.starts_with(&format!("{id} = ")) || text.contains(&format!("\n{id} = ")))
    }

    const GO_PAGE: SettingsPage = SettingsPage {
        id: "go",
        title: "settings-go",
        search: &["settings-go-goroot"],
        form: "toolchain.go.settings",
        commands: &[],
        parent: "",
    };
    const GO_STYLE: CodeStylePage = CodeStylePage { language: "go", title: "settings-section-code-style-go" };

    #[test]
    fn the_tree_keeps_its_page_order() {
        let tree: Vec<(&str, u8)> =
            sections(&[GO_PAGE], &[GO_STYLE]).iter().map(|section| (section.key, section.level)).collect();
        assert_eq!(
            tree,
            [
                ("settings-project-structure", 0),
                ("settings-appearance", 0),
                ("settings-menus", 1),
                ("settings-system", 1),
                ("settings-proxy", 2),
                ("settings-certificates", 2),
                ("settings-notifications", 1),
                ("settings-trusted-locations", 1),
                ("settings-keymap", 0),
                ("settings-editor", 0),
                ("settings-section-general", 1),
                ("settings-section-code-folding", 2),
                ("settings-section-editor-tabs", 2),
                ("settings-section-postfix-completion", 2),
                ("settings-section-font", 1),
                ("settings-section-code-style", 1),
                ("settings-section-code-style-go", 2),
                ("settings-section-code-style-sql", 2),
                ("settings-section-code-style-other", 2),
                ("settings-section-inlay-hints", 1),
                ("settings-section-code-vision", 2),
                ("settings-section-inspections", 1),
                ("settings-section-file-templates", 1),
                ("settings-section-live-templates", 1),
                ("settings-section-file-types", 1),
                ("settings-section-file-encodings", 1),
                ("settings-section-natural-languages", 1),
                ("settings-section-spelling", 2),
                ("settings-section-todo", 1),
                ("settings-plugins", 0),
                ("settings-vcs", 0),
                ("settings-build", 0),
                ("settings-debugger", 1),
                ("settings-debugger-data-views", 2),
                ("settings-languages", 0),
                ("settings-go", 1),
                ("settings-section-markdown", 1),
                ("settings-language-servers", 1),
                ("settings-schemas", 1),
                ("settings-tools", 0),
                ("settings-actions-on-save", 1),
                ("settings-external-tools", 1),
                ("settings-file-watchers", 1),
                ("settings-caffeine", 1),
                ("settings-terminal", 1),
                ("settings-mcp", 1),
                ("settings-advanced", 0),
            ]
        );
        assert_eq!(sections(&[], &[]).len(), SECTIONS.len() + 1, "without modules, the platform's tree and SQL");
    }

    #[test]
    fn configure_editor_tabs_names_the_editor_tabs_page() {
        let tree = sections(&[], &[]);
        assert_eq!(PageId::from_name("editorTabs", &tree), Some(PageId::EditorTabs));
        let bindings = zen_core::keymap::Keymap::default_macos().bindings();
        let binding = bindings.iter().find(|binding| binding.action == "view.settings.open/editorTabs").unwrap();
        assert_eq!(binding.params["page"], "editorTabs");
    }

    #[test]
    fn a_page_with_a_parent_goes_last_under_it_one_level_down() {
        let page = |id, title, parent| SettingsPage { id, title, search: &[], form: "", commands: &[], parent };
        let pages = [
            page("buildTools", "build-tools-page", "build"),
            page("maven", "maven-page", "buildTools"),
            page("mavenImporting", "maven-importing", "maven"),
            page("mavenRunner", "maven-runner", "maven"),
            page("lost", "lost-page", "nowhere"),
        ];
        let tree: Vec<(&str, u8)> = sections(&pages, &[]).iter().map(|section| (section.key, section.level)).collect();
        let build = tree.iter().position(|(key, _)| *key == "settings-build").unwrap();
        assert_eq!(
            tree[build..build + 8],
            [
                ("settings-build", 0),
                ("settings-debugger", 1),
                ("settings-debugger-data-views", 2),
                ("build-tools-page", 1),
                ("maven-page", 2),
                ("maven-importing", 3),
                ("maven-runner", 3),
                ("settings-languages", 0),
            ]
        );
        let servers = tree.iter().position(|(key, _)| *key == "settings-language-servers").unwrap();
        assert_eq!(tree[servers - 1], ("lost-page", 1), "a page whose parent is not there goes under Languages");
    }

    #[test]
    fn a_module_page_is_named_as_before_and_found_by_its_name() {
        let tree = sections(&[GO_PAGE], &[GO_STYLE]);
        assert_eq!(PageId::Module("go").name(), "go");
        assert_eq!(PageId::CodeStyleOf("go").name(), "codeStyleGo");
        assert_eq!(PageId::EditorFont.name(), "editorFont");
        assert_eq!(PageId::from_name("go", &tree), Some(PageId::Module("go")));
        assert_eq!(PageId::from_name("codeStyleGo", &tree), Some(PageId::CodeStyleOf("go")));
        assert_eq!(PageId::from_name("go", SECTIONS), None, "an IDE without the module has no Go page");
        assert_eq!(PageId::Module("go").section_in(&tree).key, "settings-go");
    }

    fn position(page: PageId) -> usize {
        match page {
            PageId::ProjectStructure => 0,
            PageId::Appearance => 1,
            PageId::MenusAndToolbars => 2,
            PageId::SystemSettings => 3,
            PageId::HttpProxy => 4,
            PageId::ServerCertificates => 5,
            PageId::Notifications => 6,
            PageId::TrustedLocations => 7,
            PageId::Keymap => 8,
            PageId::Editor => 9,
            PageId::EditorGeneral => 10,
            PageId::CodeFolding => 11,
            PageId::EditorTabs => 12,
            PageId::PostfixCompletion => 13,
            PageId::EditorFont => 14,
            PageId::CodeStyle => 15,
            PageId::CodeStyleOther => 16,
            PageId::InlayHints => 17,
            PageId::CodeVision => 18,
            PageId::Inspections => 19,
            PageId::FileTemplates => 20,
            PageId::LiveTemplates => 21,
            PageId::FileTypes => 22,
            PageId::FileEncodings => 23,
            PageId::NaturalLanguages => 24,
            PageId::Spelling => 25,
            PageId::Todo => 26,
            PageId::Plugins => 27,
            PageId::VersionControl => 28,
            PageId::Build => 29,
            PageId::Debugger => 30,
            PageId::DataViews => 31,
            PageId::Languages => 32,
            PageId::Markdown => 33,
            PageId::LanguageServers => 34,
            PageId::Schemas => 35,
            PageId::Tools => 36,
            PageId::ActionsOnSave => 37,
            PageId::ExternalTools => 38,
            PageId::FileWatchers => 39,
            PageId::Caffeine => 40,
            PageId::Terminal => 41,
            PageId::Mcp => 42,
            PageId::Advanced => 43,
            PageId::CodeStyleOf(_) | PageId::Module(_) => usize::MAX,
        }
    }

    #[test]
    fn every_page_has_one_section_in_tree_order() {
        for (index, page) in PageId::ALL.iter().enumerate() {
            assert_eq!(position(*page), index, "{page:?} is listed in ALL in tree order");
        }
        let pages: Vec<PageId> = SECTIONS.iter().map(|section| section.page).collect();
        assert_eq!(pages, PageId::ALL);
        for page in PageId::ALL {
            assert_eq!(page.section_in(SECTIONS).page, *page);
            assert_eq!(PageId::from_name(&page.name(), SECTIONS), Some(*page));
        }
    }

    #[test]
    fn a_child_is_one_level_below_its_parent() {
        let tree = sections(&[GO_PAGE], &[GO_STYLE]);
        assert_eq!(tree[0].level, 0);
        for pair in tree.windows(2) {
            assert!(pair[1].level <= pair[0].level + 1, "{} jumps a level", pair[1].key);
        }
    }

    #[test]
    fn every_title_and_search_text_is_a_message() {
        for section in SECTIONS {
            assert!(defined(section.key), "no message {}", section.key);
            for key in section.search_keys {
                assert!(defined(key), "no message {key} for {}", section.key);
            }
        }
        for key in CODE_STYLE_SEARCH.iter().chain(CODE_FOLDING_SEARCH) {
            assert!(defined(key), "no message {key}");
        }
    }
}

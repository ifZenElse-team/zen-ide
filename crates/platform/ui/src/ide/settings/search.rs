use zen_workbench::plugin_manager::PluginSummary;

use super::page::{PageId, SectionDef};
use super::plugins::PluginsTab;
use super::widgets::search_match;

#[derive(Default)]
pub(in crate::ide) struct SettingsSearch {
    pub(in crate::ide) text: String,
    pub(in crate::ide) focus: bool,
    pub(in crate::ide) last: Option<String>,
    pub(in crate::ide) found: Vec<PageId>,
    pub(in crate::ide) reveal: bool,
    pub(in crate::ide) scroll_to: Option<f32>,
    pub(in crate::ide) keymap_query: Option<(String, String)>,
    pub(in crate::ide) plugins_query: Option<(String, String, PluginsTab)>,
    pub(in crate::ide) index: Option<SearchIndex>,
}

impl SettingsSearch {
    pub(in crate::ide) fn query(&self) -> Option<&str> {
        Some(self.text.trim()).filter(|query| !query.is_empty())
    }

    pub(in crate::ide) fn clear(&mut self) -> bool {
        let had = !self.text.is_empty();
        self.text.clear();
        had
    }
}

pub(in crate::ide) struct SearchIndex {
    pub(in crate::ide) language: String,
    pub(in crate::ide) at: f64,
    actions: Vec<(String, String)>,
    pub(in crate::ide) plugins: Option<Vec<PluginSummary>>,
}

impl SearchIndex {
    pub(in crate::ide) fn new(
        language: String,
        at: f64,
        actions: impl IntoIterator<Item = (String, String)>,
        plugins: Option<Vec<PluginSummary>>,
    ) -> Self {
        let actions =
            actions.into_iter().map(|(shown, english)| (shown.to_lowercase(), english.to_lowercase())).collect();
        Self { language, at, actions, plugins }
    }

    pub(in crate::ide) fn has_action(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        let words: Vec<&str> =
            query.split(|ch: char| !ch.is_alphanumeric()).filter(|word| word.chars().count() >= MIN_WORD).collect();
        !query.is_empty()
            && self.actions.iter().any(|(shown, english)| {
                shown.contains(&query)
                    || english.contains(&query)
                    || [shown, english].into_iter().any(|name| starts_a_word(name, &words))
            })
    }

    pub(in crate::ide) fn has_plugin(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        !query.is_empty() && self.plugins.iter().flatten().any(|plugin| names_plugin(plugin, &query))
    }

    pub(in crate::ide) fn found(&self, query: &str) -> Vec<PageId> {
        let mut found = Vec::new();
        if self.has_action(query) {
            found.push(PageId::Keymap);
        }
        if self.has_plugin(query) {
            found.push(PageId::Plugins);
        }
        found
    }
}

const MIN_WORD: usize = 3;

fn starts_a_word(name: &str, words: &[&str]) -> bool {
    name.split(|ch: char| !ch.is_alphanumeric()).any(|part| words.iter().any(|word| part.starts_with(word)))
}

fn names_plugin(plugin: &PluginSummary, query: &str) -> bool {
    let name = format!("{} {}", plugin.name, plugin.id).to_lowercase();
    let file_type = query.trim_start_matches('.');
    query.split_whitespace().all(|word| name.contains(word))
        || plugin.extensions.iter().any(|extension| extension.to_lowercase() == file_type)
}

#[cfg(test)]
pub(in crate::ide) fn matches(
    query: &str,
    sections: &[SectionDef],
    tr: impl Fn(&str) -> String,
    found: &[PageId],
) -> Vec<PageId> {
    matches_with(query, sections, &[], tr, found)
}

pub(in crate::ide) fn matches_with(
    query: &str,
    sections: &[SectionDef],
    more: &[(PageId, &str)],
    tr: impl Fn(&str) -> String,
    found: &[PageId],
) -> Vec<PageId> {
    if query.trim().is_empty() {
        return sections.iter().map(|section| section.page).collect();
    }
    sections
        .iter()
        .filter(|section| {
            found.contains(&section.page)
                || std::iter::once(section.key)
                    .chain(section.search_keys.iter().copied())
                    .chain(more.iter().filter(|(page, _)| *page == section.page).map(|(_, key)| *key))
                    .any(|key| search_match(&tr(key), query).is_some())
        })
        .map(|section| section.page)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ide) struct TreeRow {
    pub(in crate::ide) index: usize,
    pub(in crate::ide) matched: bool,
}

pub(in crate::ide) fn visible_rows(sections: &[SectionDef], matched: &[PageId]) -> Vec<TreeRow> {
    let mut shown = vec![false; sections.len()];
    for (index, section) in sections.iter().enumerate() {
        if !matched.contains(&section.page) {
            continue;
        }
        shown[index] = true;
        let mut level = section.level;
        for parent in (0..index).rev() {
            if level == 0 {
                break;
            }
            if sections[parent].level < level {
                shown[parent] = true;
                level = sections[parent].level;
            }
        }
    }
    shown
        .iter()
        .enumerate()
        .filter(|(_, shown)| **shown)
        .map(|(index, _)| TreeRow { index, matched: matched.contains(&sections[index].page) })
        .collect()
}

pub(in crate::ide) fn titled(query: &str, sections: &[SectionDef], tr: impl Fn(&str) -> String) -> Vec<PageId> {
    sections
        .iter()
        .filter(|section| search_match(&tr(section.key), query).is_some())
        .map(|section| section.page)
        .collect()
}

pub(in crate::ide) fn page_for(current: PageId, matched: &[PageId], titled: &[PageId], own: &[PageId]) -> PageId {
    if matched.contains(&current) {
        return current;
    }
    matched
        .iter()
        .find(|page| titled.contains(page))
        .or_else(|| matched.iter().find(|page| own.contains(page)))
        .or_else(|| matched.first())
        .copied()
        .unwrap_or(current)
}

#[cfg(test)]
mod tests {
    use super::super::page::SECTIONS;
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

    const RUSSIAN: &[&str] = &[
        include_str!("../../../locales/ru/ui.ftl"),
        include_str!("../../../locales/ru/settings-appearance.ftl"),
        include_str!("../../../locales/ru/settings-editor.ftl"),
        include_str!("../../../locales/ru/settings-modules.ftl"),
        include_str!("../../../locales/ru/settings-search.ftl"),
        include_str!("../../../locales/ru/caffeine.ftl"),
        include_str!("../../../locales/ru/external-tools.ftl"),
        include_str!("../../../locales/ru/file-watchers.ftl"),
    ];

    fn translator(sources: &'static [&'static str]) -> impl Fn(&str) -> String {
        move |id: &str| {
            sources
                .iter()
                .flat_map(|source| source.lines())
                .find_map(|line| line.strip_prefix(id).and_then(|rest| rest.strip_prefix(" = ")))
                .map_or_else(|| id.to_owned(), str::to_owned)
        }
    }

    fn sections() -> Vec<SectionDef> {
        vec![
            SectionDef { key: "top-a", level: 0, page: PageId::Appearance, search_keys: &["text-theme"] },
            SectionDef { key: "top-b", level: 0, page: PageId::Editor, search_keys: &[] },
            SectionDef { key: "child-b", level: 1, page: PageId::CodeStyle, search_keys: &[] },
            SectionDef { key: "grandchild-b", level: 2, page: PageId::CodeStyleOf("go"), search_keys: &["text-tabs"] },
            SectionDef { key: "other-child-b", level: 1, page: PageId::EditorFont, search_keys: &["text-size"] },
            SectionDef { key: "top-c", level: 0, page: PageId::Advanced, search_keys: &[] },
        ]
    }

    fn texts(id: &str) -> String {
        match id {
            "top-a" => "Appearance",
            "top-b" => "Editor",
            "child-b" => "Code Style",
            "grandchild-b" => "Go",
            "other-child-b" => "Font",
            "top-c" => "Advanced",
            "text-theme" => "Theme",
            "text-tabs" => "Use tab character",
            "text-size" => "Размер шрифта",
            other => other,
        }
        .to_owned()
    }

    #[test]
    fn a_section_matches_by_its_title_or_a_search_text() {
        let sections = sections();
        assert_eq!(matches("appear", &sections, texts, &[]), [PageId::Appearance]);
        assert_eq!(matches("THEME", &sections, texts, &[]), [PageId::Appearance]);
        assert_eq!(matches("tab", &sections, texts, &[]), [PageId::CodeStyleOf("go")]);
        assert_eq!(matches("ШРИФТ", &sections, texts, &[]), [PageId::EditorFont]);
        assert!(matches("nothing like it", &sections, texts, &[]).is_empty());
    }

    #[test]
    fn an_empty_query_keeps_every_section() {
        let sections = sections();
        let every: Vec<PageId> = sections.iter().map(|section| section.page).collect();
        assert_eq!(matches("", &sections, texts, &[]), every);
        assert_eq!(matches("   ", &sections, texts, &[]), every);
        let rows = visible_rows(&sections, &every);
        assert_eq!(rows.len(), sections.len());
        assert!(rows.iter().all(|row| row.matched));
    }

    #[test]
    fn the_parents_of_a_match_stay_visible_but_dimmed() {
        let sections = sections();
        let rows = visible_rows(&sections, &matches("tab", &sections, texts, &[]));
        assert_eq!(
            rows,
            [
                TreeRow { index: 1, matched: false },
                TreeRow { index: 2, matched: false },
                TreeRow { index: 3, matched: true },
            ]
        );
        let rows = visible_rows(&sections, &matches("font", &sections, texts, &[]));
        assert_eq!(rows, [TreeRow { index: 1, matched: false }, TreeRow { index: 4, matched: true }]);
        assert!(visible_rows(&sections, &[]).is_empty());
    }

    #[test]
    fn the_current_page_moves_to_the_first_match_only_when_it_does_not_match() {
        let matched = [PageId::Editor, PageId::Module("go")];
        assert_eq!(page_for(PageId::Module("go"), &matched, &matched, &matched), PageId::Module("go"));
        assert_eq!(page_for(PageId::Appearance, &matched, &matched, &matched), PageId::Editor);
        assert_eq!(page_for(PageId::Mcp, &[], &[], &[]), PageId::Mcp);
        let keymap_too = [PageId::Appearance, PageId::Keymap, PageId::EditorFont];
        assert_eq!(
            page_for(PageId::Mcp, &keymap_too, &[PageId::EditorFont], &keymap_too),
            PageId::EditorFont,
            "a titled page first"
        );
        assert_eq!(
            page_for(PageId::Mcp, &keymap_too, &[PageId::Mcp], &keymap_too),
            PageId::Appearance,
            "titled, not matched"
        );
        assert_eq!(page_for(PageId::Mcp, &keymap_too[1..], &[], &[]), PageId::Keymap, "else the first in the tree");
        assert_eq!(
            page_for(PageId::Mcp, &keymap_too[1..], &[], &[PageId::EditorFont]),
            PageId::EditorFont,
            "a page with an option of that name before a Keymap action of that name"
        );
        assert_eq!(
            page_for(PageId::Keymap, &keymap_too, &[PageId::EditorFont], &[PageId::EditorFont]),
            PageId::Keymap,
            "the current stays"
        );
    }

    fn every_text_finds_its_page(tr: &impl Fn(&str) -> String) {
        for section in SECTIONS {
            for key in std::iter::once(section.key).chain(section.search_keys.iter().copied()) {
                let text = tr(key).to_uppercase();
                assert!(matches(&text, SECTIONS, tr, &[]).contains(&section.page), "{key} finds {:?}", section.page);
            }
        }
    }

    #[test]
    fn the_real_tree_is_searched_in_english() {
        let english = translator(ENGLISH);
        every_text_finds_its_page(&english);
        assert_eq!(matches("Use \"CamelHumps\" words", SECTIONS, &english, &[]), [PageId::Editor]);
        assert_eq!(matches("double-click selects", SECTIONS, &english, &[]), [PageId::Editor]);
        let rows = visible_rows(SECTIONS, &[PageId::LanguageServers]);
        let keys: Vec<(&str, bool)> = rows.iter().map(|row| (SECTIONS[row.index].key, row.matched)).collect();
        assert_eq!(keys, [("settings-languages", false), ("settings-language-servers", true)]);
    }

    #[test]
    fn the_real_tree_is_searched_in_russian() {
        let russian = translator(RUSSIAN);
        let english = translator(ENGLISH);
        assert_ne!(russian("settings-editor-ligatures"), english("settings-editor-ligatures"), "Russian is loaded");
        every_text_finds_its_page(&russian);
        assert_eq!(matches("слова «CamelHumps»", SECTIONS, &russian, &[]), [PageId::Editor]);
        assert_eq!(matches("Двойной щелчок выделяет", SECTIONS, &russian, &[]), [PageId::Editor]);
        let english_text = english("settings-editor-ligatures");
        assert!(matches(&english_text, SECTIONS, &russian, &[]).is_empty(), "only the interface language is searched");
    }

    fn plugin(id: &str, name: &str, description: &str, extensions: &[&str]) -> PluginSummary {
        PluginSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            installed: Some("0.1.0".to_owned()),
            latest: None,
            update: None,
            bundled: true,
            enabled: true,
            installing: false,
            extensions: extensions.iter().map(|extension| (*extension).to_owned()).collect(),
            names: Vec::new(),
            language_servers: false,
            runs: Vec::new(),
            person_only: Vec::new(),
            tool_windows: Vec::new(),
        }
    }

    fn index() -> SearchIndex {
        SearchIndex::new(
            "ru".to_owned(),
            0.0,
            [
                ("Переформатировать код".to_owned(), "Reformat Code".to_owned()),
                ("Дублировать строку".to_owned(), "Duplicate Line or Selection".to_owned()),
            ],
            Some(vec![
                plugin("markdown", "Markdown", "Highlights Markdown: headings, links and code.", &["md"]),
                plugin("sql", "SQL", "SQL support.", &["sql"]),
            ]),
        )
    }

    #[test]
    fn an_action_name_finds_the_keymap_in_either_language() {
        let index = index();
        assert_eq!(index.found("reformat"), [PageId::Keymap]);
        assert_eq!(index.found("ФОРМАТИР"), [PageId::Keymap]);
        assert_eq!(index.found("  duplicate line "), [PageId::Keymap]);
        assert!(index.found("no action like this").is_empty());
        assert!(index.found("   ").is_empty());
        assert_eq!(index.found("line size"), [PageId::Keymap], "a word starting a word of a name");
        assert_eq!(index.found("строк и размер"), [PageId::Keymap], "in either language");
        assert!(index.found("ine size").is_empty(), "not inside a word");
        assert!(index.found("or so").is_empty(), "short words are not looked for alone");
    }

    #[test]
    fn an_installed_plugin_finds_the_plugins_page_by_its_name_or_file_type() {
        let index = index();
        assert_eq!(index.found("markdown"), [PageId::Plugins]);
        assert_eq!(index.found("mark"), [PageId::Plugins], "a part of the name");
        assert_eq!(index.found("SQL"), [PageId::Plugins], "by name, case ignored");
        assert_eq!(index.found("md"), [PageId::Plugins], "by file type");
        assert_eq!(index.found(".MD"), [PageId::Plugins], "with the dot");
        assert!(index.found("mdx").is_empty(), "only a whole file type");
        assert!(index.found("headings").is_empty(), "the description is not searched");
        assert!(index.found("port").is_empty(), "nor its words, such as support");
        assert!(index.found("markdown sql").is_empty(), "every word in one plug-in's name");
        let unread = SearchIndex::new("en".to_owned(), 0.0, Vec::new(), None);
        assert!(unread.found("markdown").is_empty(), "no plug-ins read, none found");
    }

    #[test]
    fn the_keymap_and_plugins_found_stay_in_the_tree_with_the_text_matches() {
        let english = translator(ENGLISH);
        assert_eq!(matches("keymap", SECTIONS, &english, &[]), [PageId::Keymap]);
        assert!(matches("duplicate line", SECTIONS, &english, &[]).is_empty());
        assert_eq!(matches("duplicate line", SECTIONS, &english, &[PageId::Keymap]), [PageId::Keymap]);
        assert_eq!(
            matches("reformat code", SECTIONS, &english, &[PageId::Keymap]),
            [PageId::Keymap, PageId::ActionsOnSave],
            "an action and a setting of the same name"
        );
        assert_eq!(
            matches("markdown", SECTIONS, &english, &[]),
            [PageId::CodeStyleOther, PageId::Markdown, PageId::Advanced],
            "the language and its page"
        );
        assert_eq!(
            matches("markdown", SECTIONS, &english, &[PageId::Plugins]),
            [PageId::CodeStyleOther, PageId::Plugins, PageId::Markdown, PageId::Advanced],
            "and the plug-in, in tree order"
        );
        let rows = visible_rows(SECTIONS, &matches("markdown", SECTIONS, &english, &[PageId::Plugins]));
        let keys: Vec<(&str, bool)> = rows.iter().map(|row| (SECTIONS[row.index].key, row.matched)).collect();
        assert!(keys.contains(&("settings-plugins", true)), "{keys:?}");
        assert!(matches("", SECTIONS, &english, &[PageId::Keymap]).len() == SECTIONS.len(), "empty: every page");
    }

    #[test]
    fn a_module_page_is_searched_by_its_texts_only_while_the_module_is_on() {
        let english = translator(ENGLISH);
        let page = zen_modules::SettingsPage {
            id: "demo",
            title: "settings-languages",
            search: &["settings-declared-network"],
            form: "demo.settings",
            commands: &[],
            parent: "",
        };
        let on = super::super::page::sections(&[page], &[]);
        let text = english("settings-declared-network");
        let needle = text.split(':').next().unwrap();
        assert!(matches(needle, &on, &english, &[]).contains(&PageId::Module("demo")));
        assert!(
            !matches(needle, SECTIONS, &english, &[]).contains(&PageId::Module("demo")),
            "off, the page is not there"
        );
    }

    #[test]
    fn the_field_gives_a_trimmed_query() {
        let mut search = SettingsSearch::default();
        assert_eq!(search.query(), None);
        search.text = "  font ".to_owned();
        assert_eq!(search.query(), Some("font"));
        assert!(search.clear());
        assert!(!search.clear());
        assert_eq!(search.query(), None);
    }
}

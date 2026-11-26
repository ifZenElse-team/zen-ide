use std::collections::BTreeMap;
use std::path::Path;

use zen_core::Settings;
pub use zen_project::settings::{CodeStyle, CodeStyleGeneral, DEFAULT_LANGUAGE, PLAIN_TEXT};

pub const PREFIX: &str = "editor.codeStyle.";
pub const HARD_WRAP_AT: &str = "editor.hardWrapAt";
pub const VISUAL_GUIDES: &str = "editor.visualGuides";
pub const WRAP_ON_TYPING: &str = "editor.wrapOnTyping";
pub const DO_NOT_FORMAT: &str = "editor.formatter.doNotFormat";
pub const FORMATTER_MARKERS: &str = "editor.formatter.markers";

pub fn key(language: &str, field: &str) -> String {
    format!("{PREFIX}{language}.{field}")
}

pub fn apply(settings: &mut Settings, styles: &BTreeMap<String, CodeStyle>) {
    let stale: Vec<String> =
        settings.iter().map(|(key, _)| key).filter(|key| key.starts_with(PREFIX)).map(str::to_owned).collect();
    for key in stale {
        settings.reset(&key);
    }
    for (language, style) in styles {
        let _ = settings.set(&key(language, "useTabs"), style.use_tabs);
        let _ = settings.set(&key(language, "tabSize"), style.tab_size);
        let _ = settings.set(&key(language, "indent"), style.indent);
    }
}

pub fn apply_general(settings: &mut Settings, general: &CodeStyleGeneral) {
    let _ = settings.set(HARD_WRAP_AT, general.hard_wrap_at);
    let _ = settings.set(VISUAL_GUIDES, &general.visual_guides);
    let _ = settings.set(WRAP_ON_TYPING, general.wrap_on_typing);
    let _ = settings.set(DO_NOT_FORMAT, &general.do_not_format);
    let _ = settings.set(FORMATTER_MARKERS, general.formatter_markers);
}

pub fn general_of(settings: &Settings) -> CodeStyleGeneral {
    let defaults = CodeStyleGeneral::default();
    CodeStyleGeneral {
        hard_wrap_at: settings.get_or(HARD_WRAP_AT, defaults.hard_wrap_at),
        wrap_on_typing: settings.get_or(WRAP_ON_TYPING, defaults.wrap_on_typing),
        visual_guides: settings.get::<Vec<u16>>(VISUAL_GUIDES).unwrap_or_default(),
        do_not_format: settings.get::<Vec<String>>(DO_NOT_FORMAT).unwrap_or_default(),
        formatter_markers: settings.get_or(FORMATTER_MARKERS, defaults.formatter_markers),
    }
}

const FILE_NAMES: &[(&str, &str)] = &[
    ("go.mod", "go.mod"),
    ("go.work", "go.mod"),
    ("go.sum", "go.sum"),
    ("Makefile", "makefile"),
    ("makefile", "makefile"),
    ("GNUmakefile", "makefile"),
    ("Dockerfile", "dockerfile"),
];

const EXTENSIONS: &[(&str, &str)] = &[
    ("go", "go"),
    ("rs", "rust"),
    ("json", "json"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("toml", "toml"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("py", "python"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascriptreact"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "typescriptreact"),
    ("sh", "shellscript"),
    ("bash", "shellscript"),
    ("zsh", "shellscript"),
    ("htm", "html"),
    ("html", "html"),
    ("proto", "proto"),
    ("sql", "sql"),
    ("ddl", "sql"),
    ("dml", "sql"),
    ("pgsql", "sql"),
    ("psql", "sql"),
    ("mysql", "sql"),
    ("plsql", "sql"),
    ("xml", "xml"),
    ("css", "css"),
    ("scss", "scss"),
    ("properties", "properties"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("txt", PLAIN_TEXT),
    ("text", PLAIN_TEXT),
];

pub fn language_of(path: &Path) -> String {
    language_of_file(path, None)
}

pub fn language_of_file(path: &Path, first_line: Option<&str>) -> String {
    let Some(name) = zen_vfs::file_types::known_name_at(path, first_line) else {
        return DEFAULT_LANGUAGE.to_owned();
    };
    if let Some((_, id)) = FILE_NAMES.iter().find(|(file, _)| *file == name) {
        return (*id).to_owned();
    }
    let Some((_, extension)) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) else {
        return DEFAULT_LANGUAGE.to_owned();
    };
    let extension = extension.to_ascii_lowercase();
    EXTENSIONS.iter().find(|(known, _)| *known == extension).map_or(extension, |(_, id)| (*id).to_owned())
}

pub fn globs_of(language: &str) -> Vec<String> {
    let names = FILE_NAMES.iter().filter(|(_, id)| *id == language).map(|(name, _)| (*name).to_owned());
    let extensions = EXTENSIONS.iter().filter(|(_, id)| *id == language).map(|(extension, _)| format!("*.{extension}"));
    names.chain(extensions).collect()
}

pub fn languages() -> Vec<&'static str> {
    let mut languages: Vec<&str> =
        FILE_NAMES.iter().chain(EXTENSIONS).map(|(_, id)| *id).filter(|language| *language != PLAIN_TEXT).collect();
    languages.sort_unstable();
    languages.dedup();
    languages
}

pub fn has_own_style(language: &str) -> bool {
    language != PLAIN_TEXT && FILE_NAMES.iter().chain(EXTENSIONS).any(|(_, id)| *id == language)
}

fn entry(settings: &Settings, language: &str) -> Option<CodeStyle> {
    let use_tabs = settings.get::<bool>(&key(language, "useTabs"));
    let tab_size = settings.get::<u8>(&key(language, "tabSize"));
    let indent = settings.get::<u8>(&key(language, "indent"));
    if use_tabs.is_none() && tab_size.is_none() && indent.is_none() {
        return None;
    }
    let base = built_in(language);
    Some(CodeStyle {
        use_tabs: use_tabs.unwrap_or(base.use_tabs),
        tab_size: tab_size.unwrap_or(base.tab_size).max(1),
        indent: indent.unwrap_or(base.indent).max(1),
    })
}

pub fn explicit(settings: &Settings, language: &str) -> Option<CodeStyle> {
    entry(settings, language).or_else(|| {
        let built_in = CodeStyle::BUILT_IN.iter().any(|(id, _)| *id == language);
        if built_in { None } else { entry(settings, DEFAULT_LANGUAGE) }
    })
}

pub fn for_path(settings: &Settings, path: &Path) -> CodeStyle {
    let language = language_of(path);
    explicit(settings, &language).unwrap_or_else(|| built_in(&language))
}

pub fn for_text(settings: &Settings, path: &Path, text: &str) -> CodeStyle {
    let language = language_of_file(path, Some(zen_vfs::file_types::head(text)));
    explicit(settings, &language).unwrap_or_else(|| built_in(&language))
}

fn built_in(language: &str) -> CodeStyle {
    let style = CodeStyle::default_for(language);
    CodeStyle { use_tabs: style.use_tabs || matches!(language, "go.mod" | "makefile"), ..style }
}

pub fn indent_unit(style: CodeStyle) -> String {
    if style.use_tabs { "\t".to_owned() } else { " ".repeat(usize::from(style.indent.max(1))) }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn style(use_tabs: bool, tab_size: u8, indent: u8) -> CodeStyle {
        CodeStyle { use_tabs, tab_size, indent }
    }

    #[test]
    fn the_general_options_go_through_the_session() {
        let mut settings = Settings::default();
        assert_eq!(general_of(&settings), CodeStyleGeneral::default());
        let general = CodeStyleGeneral {
            hard_wrap_at: 80,
            wrap_on_typing: true,
            visual_guides: vec![72],
            do_not_format: vec!["*.gen.go".to_owned()],
            formatter_markers: true,
        };
        apply_general(&mut settings, &general);
        assert_eq!(general_of(&settings), general);
    }

    #[test]
    fn languages_are_named_as_language_servers_name_them() {
        let id = |path: &str| language_of(&PathBuf::from(path));
        assert_eq!(id("/p/main.go"), "go");
        assert_eq!(id("/p/go.mod"), "go.mod");
        assert_eq!(id("/p/Cargo.toml"), "toml");
        assert_eq!(id("/p/a.YML"), "yaml");
        assert_eq!(id("/p/README.md"), "markdown");
        assert_eq!(id("/p/lib.rs"), "rust");
        assert_eq!(id("/p/Makefile"), "makefile");
        assert_eq!(id("/p/LICENSE"), DEFAULT_LANGUAGE);
        assert_eq!(id("/p/notes.TXT"), PLAIN_TEXT);
        assert_eq!(id("/p/App.kt"), "kotlin");
        assert_eq!(id("/p/data.zzz"), "zzz");
    }

    #[test]
    fn only_the_named_programming_languages_have_a_style_of_their_own() {
        for language in ["go", "go.mod", "rust", "yaml", "kotlin", "makefile"] {
            assert!(has_own_style(language), "{language}");
        }
        for language in [PLAIN_TEXT, DEFAULT_LANGUAGE, "zzz"] {
            assert!(!has_own_style(language), "{language}");
        }
    }

    #[test]
    fn go_takes_tabs_and_json_two_spaces() {
        let settings = Settings::default();
        assert_eq!(for_path(&settings, Path::new("/p/main.go")), style(true, 4, 4));
        assert_eq!(for_path(&settings, Path::new("/p/package.json")), style(false, 2, 2));
        assert_eq!(for_path(&settings, Path::new("/p/notes.txt")), style(false, 4, 4));
        assert_eq!(indent_unit(for_path(&settings, Path::new("/p/main.go"))), "\t");
        assert_eq!(indent_unit(for_path(&settings, Path::new("/p/package.json"))), "  ");
    }

    #[test]
    fn the_users_entries_win_and_are_replaced_as_a_whole() {
        let mut settings = Settings::default();
        let mut styles = BTreeMap::new();
        styles.insert("go".to_owned(), style(false, 8, 2));
        styles.insert(DEFAULT_LANGUAGE.to_owned(), style(true, 3, 3));
        apply(&mut settings, &styles);
        assert_eq!(settings.get::<u8>("editor.codeStyle.go.tabSize"), Some(8));
        assert_eq!(for_path(&settings, Path::new("/p/main.go")), style(false, 8, 2));
        assert_eq!(for_path(&settings, Path::new("/p/lib.rs")), style(true, 3, 3), "the default covers others");
        assert_eq!(
            for_path(&settings, Path::new("/p/a.json")),
            style(false, 2, 2),
            "a built-in language keeps its own"
        );
        apply(&mut settings, &BTreeMap::new());
        assert_eq!(settings.iter().count(), 0);
        assert_eq!(for_path(&settings, Path::new("/p/main.go")), style(true, 4, 4));
    }

    #[test]
    fn a_partial_entry_fills_in_the_built_in_fields() {
        let mut settings = Settings::default();
        settings.set("editor.codeStyle.json.indent", 4).unwrap();
        assert_eq!(for_path(&settings, Path::new("/p/a.json")), style(false, 2, 4));
    }

    #[test]
    fn go_mod_and_makefiles_take_tabs_until_the_user_sets_them() {
        let mut settings = Settings::default();
        assert_eq!(for_path(&settings, Path::new("/p/go.mod")), style(true, 4, 4));
        assert_eq!(for_path(&settings, Path::new("/p/Makefile")), style(true, 4, 4));
        settings.set("editor.codeStyle.makefile.useTabs", false).unwrap();
        assert_eq!(for_path(&settings, Path::new("/p/Makefile")), style(false, 4, 4));
    }
}

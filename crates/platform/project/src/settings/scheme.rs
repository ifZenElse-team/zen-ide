use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CodeStyle, CodeStyleOverride, DEFAULT_LANGUAGE};

pub const DEFAULT_RIGHT_MARGIN: u16 = 120;

/// Editor → Code Style → General and Formatter of one IDE scheme.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CodeStyleGeneral {
    /// Hard wrap at: the column of the right margin (120 by default).
    pub hard_wrap_at: u16,
    /// Wrap on typing: a typed word past the right margin moves to a new line (off by default).
    pub wrap_on_typing: bool,
    /// Visual guides: the columns of the extra vertical lines.
    pub visual_guides: Vec<u16>,
    /// Do not format: file globs left alone by Reformat Code, Optimize Imports and the save actions; `*` is every file.
    pub do_not_format: Vec<String>,
    /// Turn formatter on/off with markers in code comments: `@formatter:off` to `@formatter:on` stays as it is.
    pub formatter_markers: bool,
}

impl Default for CodeStyleGeneral {
    fn default() -> Self {
        Self {
            hard_wrap_at: DEFAULT_RIGHT_MARGIN,
            wrap_on_typing: false,
            visual_guides: Vec::new(),
            do_not_format: Vec::new(),
            formatter_markers: false,
        }
    }
}

/// The options of a scheme file that belong to no language; an absent one comes from the IDE.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CodeStyleOptions {
    /// Wrap on typing.
    pub wrap_on_typing: Option<bool>,
    /// Do not format: file globs.
    pub do_not_format: Option<Vec<String>>,
    /// Turn formatter on/off with markers in code comments.
    pub formatter_markers: Option<bool>,
}

impl CodeStyleOptions {
    pub fn or(self, fallback: Self) -> Self {
        Self {
            wrap_on_typing: self.wrap_on_typing.or(fallback.wrap_on_typing),
            do_not_format: self.do_not_format.or(fallback.do_not_format),
            formatter_markers: self.formatter_markers.or(fallback.formatter_markers),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A whole code style scheme as a file keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SchemeFile {
    /// The scheme's name in the file.
    pub name: String,
    /// The style of each language; `default` holds the scheme-wide right margin, guides and other file types.
    pub styles: BTreeMap<String, CodeStyleOverride>,
    /// The options that belong to no language.
    pub options: CodeStyleOptions,
}

impl SchemeFile {
    pub fn of(name: &str, languages: &BTreeMap<String, CodeStyle>, general: &CodeStyleGeneral) -> Self {
        let mut styles: BTreeMap<String, CodeStyleOverride> =
            languages.iter().map(|(language, style)| (language.clone(), CodeStyleOverride::from(*style))).collect();
        let scheme = styles.entry(DEFAULT_LANGUAGE.to_owned()).or_default();
        scheme.right_margin = Some(general.hard_wrap_at);
        scheme.visual_guides = Some(general.visual_guides.clone());
        let options = CodeStyleOptions {
            wrap_on_typing: Some(general.wrap_on_typing),
            do_not_format: Some(general.do_not_format.clone()),
            formatter_markers: Some(general.formatter_markers),
        };
        Self { name: name.to_owned(), styles, options }
    }

    pub fn general_over(&self, base: &CodeStyleGeneral) -> CodeStyleGeneral {
        let scheme = self.styles.get(DEFAULT_LANGUAGE);
        CodeStyleGeneral {
            hard_wrap_at: scheme.and_then(|style| style.right_margin).unwrap_or(base.hard_wrap_at),
            visual_guides: scheme
                .and_then(|style| style.visual_guides.clone())
                .unwrap_or_else(|| base.visual_guides.clone()),
            wrap_on_typing: self.options.wrap_on_typing.unwrap_or(base.wrap_on_typing),
            do_not_format: self.options.do_not_format.clone().unwrap_or_else(|| base.do_not_format.clone()),
            formatter_markers: self.options.formatter_markers.unwrap_or(base.formatter_markers),
        }
    }

    pub fn languages_over(&self, base: &BTreeMap<String, CodeStyle>) -> BTreeMap<String, CodeStyle> {
        let mut languages = base.clone();
        for (language, style) in &self.styles {
            if style.use_tabs.is_some() || style.tab_size.is_some() || style.indent.is_some() {
                let start = languages.get(language).copied().unwrap_or_else(|| CodeStyle::default_for(language));
                languages.insert(language.clone(), style.apply_to(start));
            }
        }
        languages
    }

    pub fn set_general(&mut self, general: &CodeStyleGeneral) {
        let scheme = self.styles.entry(DEFAULT_LANGUAGE.to_owned()).or_default();
        scheme.right_margin = Some(general.hard_wrap_at);
        scheme.visual_guides = Some(general.visual_guides.clone());
        self.options = CodeStyleOptions {
            wrap_on_typing: Some(general.wrap_on_typing),
            do_not_format: Some(general.do_not_format.clone()),
            formatter_markers: Some(general.formatter_markers),
        };
    }

    pub fn set_style(&mut self, language: &str, style: CodeStyle) {
        let entry = self.styles.entry(language.to_owned()).or_default();
        entry.use_tabs = Some(style.use_tabs);
        entry.tab_size = Some(style.tab_size);
        entry.indent = Some(style.indent);
    }
}

impl From<CodeStyle> for CodeStyleOverride {
    fn from(style: CodeStyle) -> Self {
        Self {
            use_tabs: Some(style.use_tabs),
            tab_size: Some(style.tab_size),
            indent: Some(style.indent),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn general() -> CodeStyleGeneral {
        CodeStyleGeneral {
            hard_wrap_at: 100,
            wrap_on_typing: true,
            visual_guides: vec![80],
            do_not_format: vec!["*.min.js".to_owned()],
            formatter_markers: true,
        }
    }

    #[test]
    fn an_ide_scheme_survives_the_way_through_a_scheme_file() {
        let go = CodeStyle { use_tabs: false, tab_size: 8, indent: 2 };
        let languages = BTreeMap::from([("go".to_owned(), go)]);
        let file = SchemeFile::of("Team", &languages, &general());
        assert_eq!(file.general_over(&CodeStyleGeneral::default()), general());
        assert_eq!(file.languages_over(&BTreeMap::new())["go"], go);
    }

    #[test]
    fn what_a_file_leaves_out_comes_from_the_base() {
        let file = SchemeFile::default();
        assert_eq!(file.general_over(&general()), general());
        let base = BTreeMap::from([("go".to_owned(), CodeStyle::default_for("go"))]);
        assert_eq!(file.languages_over(&base), base);
    }

    #[test]
    fn a_right_margin_alone_makes_no_language_entry() {
        let mut file = SchemeFile::default();
        file.set_general(&general());
        assert!(file.languages_over(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn editing_a_style_keeps_what_the_project_set_besides_it() {
        let mut file = SchemeFile::default();
        file.styles.insert("go".to_owned(), CodeStyleOverride { continuation_indent: Some(8), ..Default::default() });
        file.set_style("go", CodeStyle::default_for("go"));
        assert_eq!(file.styles["go"].continuation_indent, Some(8));
        assert_eq!(file.styles["go"].use_tabs, Some(true));
    }
}

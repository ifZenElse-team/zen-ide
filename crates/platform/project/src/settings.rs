mod project;
mod scheme;

pub use project::{
    CodeStyleOverride, Encodings, ExclusionChange, FileEncoding, ForeignRunConfiguration, ForeignTask,
    InspectionProfile, InspectionTool, NewFileBom, ProjectSettings, ReadFiles, Scope, SettingsProblem,
    SettingsProvider, SourceFolder, SourceFolderKind, is_utf8_compatible,
};
use schemars::JsonSchema;
pub use scheme::{CodeStyleGeneral, CodeStyleOptions, DEFAULT_RIGHT_MARGIN, SchemeFile};
use serde::{Deserialize, Serialize};

pub const DEFAULT_LANGUAGE: &str = "default";

pub const PLAIN_TEXT: &str = "plaintext";

/// The indentation of one language: Editor → Code Style → Tabs and Indents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CodeStyle {
    /// Whether indentation is written with tab characters.
    pub use_tabs: bool,
    /// How many columns a tab character takes.
    pub tab_size: u8,
    /// How many columns one level of indentation takes.
    pub indent: u8,
}

impl Default for CodeStyle {
    fn default() -> Self {
        Self::SPACES_4
    }
}

impl CodeStyle {
    const SPACES_4: Self = Self { use_tabs: false, tab_size: 4, indent: 4 };
    const SPACES_2: Self = Self { use_tabs: false, tab_size: 2, indent: 2 };
    const TABS_4: Self = Self { use_tabs: true, tab_size: 4, indent: 4 };

    pub const BUILT_IN: &[(&str, CodeStyle)] = &[
        (DEFAULT_LANGUAGE, Self::SPACES_4),
        ("go", Self::TABS_4),
        ("json", Self::SPACES_2),
        ("yaml", Self::SPACES_2),
        ("toml", Self::SPACES_2),
        ("markdown", Self::SPACES_4),
    ];

    pub fn default_for(language: &str) -> CodeStyle {
        Self::BUILT_IN.iter().find(|(id, _)| *id == language).map_or_else(Self::default, |(_, style)| *style)
    }
}

/// The whole code style of one file: its indentation and the margins the editor draws.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileCodeStyle {
    /// Whether indentation is written with tab characters.
    pub use_tabs: bool,
    /// How many columns a tab character takes.
    pub tab_size: u8,
    /// How many columns one level of indentation takes.
    pub indent: u8,
    /// How many columns a continued line is indented by, when a source sets it.
    pub continuation_indent: Option<u8>,
    /// The column of the right margin, when a source sets one.
    pub right_margin: Option<u16>,
    /// The columns of the extra vertical guides.
    pub visual_guides: Vec<u16>,
}

impl From<CodeStyle> for FileCodeStyle {
    fn from(style: CodeStyle) -> Self {
        Self { use_tabs: style.use_tabs, tab_size: style.tab_size, indent: style.indent, ..Self::default() }
    }
}

impl FileCodeStyle {
    pub fn code_style(&self) -> CodeStyle {
        CodeStyle { use_tabs: self.use_tabs, tab_size: self.tab_size, indent: self.indent }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_table() {
        let style = |use_tabs, tab_size, indent| CodeStyle { use_tabs, tab_size, indent };
        assert_eq!(CodeStyle::default_for("go"), style(true, 4, 4));
        assert_eq!(CodeStyle::default_for(DEFAULT_LANGUAGE), style(false, 4, 4));
        for language in ["json", "yaml", "toml"] {
            assert_eq!(CodeStyle::default_for(language), style(false, 2, 2), "{language}");
        }
        assert_eq!(CodeStyle::default_for("markdown"), style(false, 4, 4));
        assert_eq!(CodeStyle::default_for("rust"), CodeStyle::default(), "an unknown language takes the default");
        assert_eq!(CodeStyle::default(), CodeStyle::default_for(DEFAULT_LANGUAGE));
        let mut ids: Vec<&str> = CodeStyle::BUILT_IN.iter().map(|(id, _)| *id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "every language is listed once");
    }

    #[test]
    fn it_is_written_in_camel_case_and_fills_gaps() {
        let json = serde_json::to_value(CodeStyle::default_for("go")).unwrap();
        assert_eq!(json, serde_json::json!({ "useTabs": true, "tabSize": 4, "indent": 4 }));
        let partial: CodeStyle = serde_json::from_str(r#"{"indent": 2}"#).unwrap();
        assert_eq!(partial, CodeStyle { indent: 2, ..CodeStyle::default() });
    }
}

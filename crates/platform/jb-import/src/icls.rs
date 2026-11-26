use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use roxmltree::Node;
use zen_theme::{
    EditorKey, ImportError, ImportReport, ResolvedTheme, Rgba, Style, SyntaxKey, Theme, ThemeImporter, UiKey,
    Underline, base_id, format,
};

use crate::color;

const MAX_BASE_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct IclsImporter;

#[derive(Clone, Copy, Debug)]
enum Field {
    Foreground,
    Background,
    Effect,
    Stripe,
}

const COLORS: &[(&str, EditorKey)] = &[
    ("CARET_COLOR", EditorKey::Caret),
    ("CARET_ROW_COLOR", EditorKey::CurrentLine),
    ("SELECTION_BACKGROUND", EditorKey::Selection),
    ("LINE_NUMBERS_COLOR", EditorKey::LineNumber),
    ("TEARLINE_COLOR", EditorKey::GutterSeparator),
    ("METHOD_SEPARATORS_COLOR", EditorKey::MethodSeparator),
    ("STICKY_LINES_BACKGROUND", EditorKey::StickyBackground),
    ("ADDED_LINES_COLOR", EditorKey::GutterAdded),
    ("MODIFIED_LINES_COLOR", EditorKey::GutterModified),
    ("DELETED_LINES_COLOR", EditorKey::GutterDeleted),
    ("ANNOTATIONS_LAST_COMMIT_COLOR", EditorKey::AnnotationRecent),
    ("ANNOTATIONS_COLOR", EditorKey::AnnotationOld),
    ("WHITESPACES", EditorKey::Whitespace),
    ("INDENT_GUIDE", EditorKey::IndentGuide),
    ("SELECTED_INDENT_GUIDE", EditorKey::IndentGuideSelected),
    ("RIGHT_MARGIN_COLOR", EditorKey::RightMargin),
];

const UI_COLORS: &[(&str, UiKey)] = &[
    ("FILESTATUS_MODIFIED", UiKey::VcsModified),
    ("FILESTATUS_ADDED", UiKey::VcsAdded),
    ("FILESTATUS_DELETED", UiKey::VcsDeleted),
    ("FILESTATUS_UNKNOWN", UiKey::VcsUnversioned),
    ("FILESTATUS_IDEA_FILESTATUS_IGNORED", UiKey::VcsIgnored),
    ("FILESTATUS_IDEA_FILESTATUS_MERGED_WITH_CONFLICTS", UiKey::VcsConflict),
];

const DIAGNOSTIC: &[Field] = &[Field::Effect, Field::Foreground, Field::Stripe];

const EDITOR_ATTRIBUTES: &[(&str, EditorKey, &[Field])] = &[
    ("TEXT", EditorKey::Foreground, &[Field::Foreground]),
    ("TEXT", EditorKey::Background, &[Field::Background]),
    ("TEXT_SEARCH_RESULT_ATTRIBUTES", EditorKey::SearchMatch, &[Field::Background]),
    ("TEXT_SEARCH_RESULT_ATTRIBUTES", EditorKey::SearchMark, &[Field::Stripe]),
    ("FOLDED_TEXT_ATTRIBUTES", EditorKey::FoldMarker, &[Field::Background]),
    ("FOLDED_TEXT_ATTRIBUTES", EditorKey::FoldText, &[Field::Foreground]),
    ("INLAY_DEFAULT", EditorKey::InlayHintBackground, &[Field::Background]),
    ("INLAY_DEFAULT", EditorKey::InlayHintForeground, &[Field::Foreground]),
    ("INLINE_PARAMETER_HINT", EditorKey::InlayHintBackground, &[Field::Background]),
    ("INLINE_PARAMETER_HINT", EditorKey::InlayHintForeground, &[Field::Foreground]),
    ("IDENTIFIER_UNDER_CARET_ATTRIBUTES", EditorKey::SymbolHighlight, &[Field::Background]),
    ("WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES", EditorKey::SymbolHighlightWrite, &[Field::Background]),
    ("MATCHED_BRACE_ATTRIBUTES", EditorKey::BracketMatchedBackground, &[Field::Background]),
    ("MATCHED_BRACE_ATTRIBUTES", EditorKey::BracketMatchedForeground, &[Field::Foreground]),
    ("UNMATCHED_BRACE_ATTRIBUTES", EditorKey::BracketUnmatchedBackground, &[Field::Background]),
    ("UNMATCHED_BRACE_ATTRIBUTES", EditorKey::BracketUnmatchedForeground, &[Field::Foreground]),
    ("ERRORS_ATTRIBUTES", EditorKey::DiagnosticError, DIAGNOSTIC),
    ("WARNING_ATTRIBUTES", EditorKey::DiagnosticWarning, DIAGNOSTIC),
    ("INFO_ATTRIBUTES", EditorKey::DiagnosticInfo, DIAGNOSTIC),
    ("WEAK_WARNING_ATTRIBUTES", EditorKey::DiagnosticHint, DIAGNOSTIC),
    ("TYPO", EditorKey::DiagnosticTypo, DIAGNOSTIC),
    ("NOT_USED_ELEMENT_ATTRIBUTES", EditorKey::Unused, &[Field::Foreground]),
    ("DIFF_INSERTED", EditorKey::DiffInserted, &[Field::Background]),
    ("DIFF_DELETED", EditorKey::DiffDeleted, &[Field::Background]),
    ("DIFF_MODIFIED", EditorKey::DiffModified, &[Field::Background]),
];

const SYNTAX_ATTRIBUTES: &[(&str, SyntaxKey)] = &[
    ("TEXT", SyntaxKey::Default),
    ("DEFAULT_KEYWORD", SyntaxKey::Keyword),
    ("DEFAULT_STRING", SyntaxKey::String),
    ("DEFAULT_NUMBER", SyntaxKey::Number),
    ("DEFAULT_LINE_COMMENT", SyntaxKey::Comment),
    ("DEFAULT_BLOCK_COMMENT", SyntaxKey::Comment),
    ("DEFAULT_DOC_COMMENT", SyntaxKey::DocComment),
    ("DEFAULT_FUNCTION_DECLARATION", SyntaxKey::Function),
    ("DEFAULT_FUNCTION_CALL", SyntaxKey::SemanticCall),
    ("CTRL_CLICKABLE", SyntaxKey::NavigationLink),
    ("DEFAULT_CLASS_NAME", SyntaxKey::Type),
    ("DEFAULT_INTERFACE_NAME", SyntaxKey::Type),
    ("DEFAULT_CONSTANT", SyntaxKey::Constant),
    ("DEFAULT_INSTANCE_FIELD", SyntaxKey::Property),
    ("DEFAULT_GLOBAL_VARIABLE", SyntaxKey::GlobalVariable),
    ("DEFAULT_METADATA", SyntaxKey::Attribute),
    ("DEFAULT_LABEL", SyntaxKey::Lifetime),
    ("DEFAULT_OPERATION_SIGN", SyntaxKey::Punctuation),
    ("DEFAULT_BRACES", SyntaxKey::Punctuation),
    ("DEFAULT_PARENTHS", SyntaxKey::Punctuation),
    ("DEFAULT_BRACKETS", SyntaxKey::Punctuation),
    ("DEFAULT_COMMA", SyntaxKey::Punctuation),
    ("DEFAULT_SEMICOLON", SyntaxKey::Punctuation),
    ("DEFAULT_DOT", SyntaxKey::Punctuation),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Attributes {
    foreground: Option<Rgba>,
    background: Option<Rgba>,
    font_type: u8,
    effect_type: Option<u8>,
    effect_color: Option<Rgba>,
    stripe: Option<Rgba>,
}

impl Attributes {
    fn get(&self, field: Field) -> Option<Rgba> {
        match field {
            Field::Foreground => self.foreground,
            Field::Background => self.background,
            Field::Effect => self.effect_color,
            Field::Stripe => self.stripe,
        }
    }

    fn is_empty(&self) -> bool {
        *self == Attributes::default()
    }

    fn style(&self, fallback: Rgba) -> Style {
        let effect = self.effect_color.and(self.effect_type);
        Style {
            color: self.foreground.unwrap_or(fallback),
            background: self.background,
            bold: self.font_type & 1 != 0,
            italic: self.font_type & 2 != 0,
            underline: match effect {
                Some(1 | 4 | 5) => Some(Underline::Line),
                Some(2) => Some(Underline::Wave),
                _ => None,
            },
            strikethrough: effect == Some(3),
        }
    }
}

#[derive(Debug, Default)]
struct Entry {
    value: Option<Attributes>,
    base: Option<String>,
}

#[derive(Debug, Default)]
struct Scheme {
    name: String,
    parent: Option<String>,
    sources: Vec<String>,
    invalid: BTreeSet<String>,
    colors: BTreeMap<String, Rgba>,
    attributes: BTreeMap<String, Entry>,
}

impl Scheme {
    fn parse(text: &str) -> Result<Scheme, ImportError> {
        let document =
            crate::idea::xml::parse(text).map_err(|error| ImportError(format!("not a colour scheme: {error}")))?;
        let root = document.root_element();
        if root.tag_name().name() != "scheme" {
            return Err(ImportError(format!(
                "not a colour scheme: the root element is <{}>, not <scheme>",
                root.tag_name().name()
            )));
        }
        let mut scheme = Scheme {
            name: root.attribute("name").unwrap_or("Imported scheme").to_owned(),
            parent: root.attribute("parent_scheme").map(str::to_owned),
            ..Scheme::default()
        };
        for section in root.children().filter(Node::is_element) {
            match section.tag_name().name() {
                "colors" => scheme.read_colors(section),
                "attributes" => scheme.read_attributes(section),
                "option" => scheme.note(section.attribute("name").unwrap_or("option")),
                _ => {}
            }
        }
        Ok(scheme)
    }

    fn note(&mut self, name: &str) {
        if !self.sources.iter().any(|source| source == name) {
            self.sources.push(name.to_owned());
        }
    }

    fn read_colors(&mut self, section: Node<'_, '_>) {
        for option in options(section) {
            let Some(name) = option.attribute("name") else { continue };
            self.note(name);
            match option.attribute("value").map(str::trim) {
                None | Some("") => {}
                Some(value) => match color::parse(value) {
                    Some(rgba) => {
                        self.colors.insert(name.to_owned(), rgba);
                    }
                    None => {
                        self.invalid.insert(name.to_owned());
                    }
                },
            }
        }
    }

    fn read_attributes(&mut self, section: Node<'_, '_>) {
        for option in options(section) {
            let Some(name) = option.attribute("name") else { continue };
            self.note(name);
            let base = option.attribute("baseAttributes").map(str::to_owned);
            let value = option.children().find(|child| child.has_tag_name("value"));
            let value = value.map(|value| {
                let mut attributes = Attributes::default();
                for field in options(value) {
                    let text = field.attribute("value").unwrap_or("").trim();
                    let mut rgba = || {
                        let parsed = color::parse(text);
                        if parsed.is_none() && !text.is_empty() {
                            self.invalid.insert(name.to_owned());
                        }
                        parsed
                    };
                    match field.attribute("name") {
                        Some("FOREGROUND") => attributes.foreground = rgba(),
                        Some("BACKGROUND") => attributes.background = rgba(),
                        Some("EFFECT_COLOR") => attributes.effect_color = rgba(),
                        Some("ERROR_STRIPE_COLOR") => attributes.stripe = rgba(),
                        Some("FONT_TYPE") => attributes.font_type = text.parse().unwrap_or(0),
                        Some("EFFECT_TYPE") => attributes.effect_type = text.parse().ok(),
                        _ => {}
                    }
                }
                attributes
            });
            self.attributes.insert(name.to_owned(), Entry { value, base });
        }
    }

    fn resolve(&self, name: &str) -> Option<Attributes> {
        let mut current = name;
        for _ in 0..MAX_BASE_DEPTH {
            let entry = self.attributes.get(current)?;
            match (&entry.value, &entry.base) {
                (Some(value), _) => return Some(*value),
                (None, Some(base)) => current = base,
                (None, None) => return None,
            }
        }
        None
    }

    fn dark(&self) -> bool {
        match &self.parent {
            Some(parent) => {
                let parent = parent.to_ascii_lowercase();
                parent.contains("darcula") || parent.contains("dark")
            }
            None => self.resolve("TEXT").and_then(|text| text.background).is_none_or(|background| {
                let luma =
                    299 * u32::from(background.r()) + 587 * u32::from(background.g()) + 114 * u32::from(background.b());
                luma < 128_000
            }),
        }
    }

    fn into_report(self) -> ImportReport {
        let mut theme = Theme::new(format::id_from_name(&self.name), self.name.clone(), self.dark());
        theme.parent = Some(base_id(theme.dark).to_owned());
        let mut mapped = BTreeSet::new();

        for (name, key) in COLORS {
            if let Some(rgba) = self.colors.get(*name) {
                theme.editor.entry(*key).or_insert(*rgba);
                mapped.insert(*name);
            }
        }
        for (name, key) in UI_COLORS {
            if let Some(rgba) = self.colors.get(*name) {
                theme.ui.entry(*key).or_insert(*rgba);
                mapped.insert(*name);
            }
        }
        for (name, key, fields) in EDITOR_ATTRIBUTES {
            let Some(attributes) = self.resolve(name) else { continue };
            if let Some(rgba) = fields.iter().find_map(|field| attributes.get(*field)) {
                theme.editor.entry(*key).or_insert(rgba);
                mapped.insert(*name);
            }
        }
        let text = self.resolve("TEXT").and_then(|text| text.foreground);
        let base = ResolvedTheme::base(theme.dark);
        for (name, key) in SYNTAX_ATTRIBUTES {
            let Some(attributes) = self.resolve(name).filter(|attributes| !attributes.is_empty()) else { continue };
            let fallback = text.unwrap_or(base.syntax(*key).color);
            let mut style = attributes.style(fallback);
            if *key == SyntaxKey::Default {
                style.background = None;
            }
            theme.syntax.entry(*key).or_insert(style);
            mapped.insert(*name);
        }

        let unmapped = self
            .sources
            .iter()
            .filter(|source| !mapped.contains(source.as_str()) || self.invalid.contains(source.as_str()))
            .cloned()
            .collect();
        ImportReport { theme, mapped: mapped.len(), unmapped }
    }
}

fn options<'a, 'input>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(|child| child.has_tag_name("option"))
}

impl IclsImporter {
    pub fn detects_path(&self, path: &Path, text: &str) -> bool {
        let extension = path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase);
        matches!(extension.as_deref(), Some("icls" | "xml")) && self.detects(text)
    }
}

impl ThemeImporter for IclsImporter {
    fn format_name(&self) -> &'static str {
        "JetBrains colour scheme"
    }

    fn detects(&self, text: &str) -> bool {
        text.contains("<scheme")
            && crate::idea::xml::parse(text).is_ok_and(|document| document.root_element().has_tag_name("scheme"))
    }

    fn import(&self, text: &str) -> Result<ImportReport, ImportError> {
        Ok(Scheme::parse(text)?.into_report())
    }

    fn detects_at(&self, path: &str, text: &str) -> bool {
        self.detects_path(Path::new(path), text)
    }
}

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{EditorKey, Key, Rgba, Style, SyntaxKey, TerminalKey, Theme, UiKey, Underline, jsonc};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for FormatError {}

impl From<serde_json::Error> for FormatError {
    fn from(error: serde_json::Error) -> FormatError {
        let (line, column) = (error.line(), error.column());
        let text = error.to_string();
        let suffix = format!(" at line {line} column {column}");
        let message = text.strip_suffix(&suffix).unwrap_or(&text).to_owned();
        FormatError { message, line, column }
    }
}

pub fn parse(text: &str) -> Result<Theme, FormatError> {
    let file: ThemeFile = serde_json::from_str(&jsonc::strip(text))?;
    Ok(file.into_theme())
}

pub fn to_json(theme: &Theme) -> String {
    let file = ThemeFile::from_theme(theme);
    let mut text = serde_json::to_string_pretty(&file).unwrap_or_default();
    text.push('\n');
    text
}

pub fn schema_json() -> String {
    let mut text = serde_json::to_string_pretty(&schemars::schema_for!(ThemeFile)).unwrap_or_default();
    text.push('\n');
    text
}

pub fn id_from_name(name: &str) -> String {
    let mut id = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            id.extend(c.to_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    while id.ends_with('-') {
        id.pop();
    }
    if id.is_empty() { "theme".to_owned() } else { id }
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(description = "A Zen IDE colour theme (docs/themes.md).")]
#[serde(rename_all = "camelCase")]
#[schemars(title = "Zen IDE theme")]
struct ThemeFile {
    /// A link to this schema, for editors; ignored by the IDE.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    /// The version of the format: always 1.
    format_version: FormatVersion,
    /// The stable id other themes name as their parent; made from the name when it is left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    /// The name shown in the theme list.
    name: String,
    /// Whether the theme is dark.
    dark: bool,
    /// The id of the theme this one changes; Zen Dark when it is left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    /// Window colours.
    #[serde(default, skip_serializing_if = "Section::is_empty")]
    ui: Section<UiKey, Rgba>,
    /// Editor colours.
    #[serde(default, skip_serializing_if = "Section::is_empty")]
    editor: Section<EditorKey, Rgba>,
    /// Code styles: a colour, or a colour with a background and font style.
    #[serde(default, skip_serializing_if = "Section::is_empty")]
    syntax: Section<SyntaxKey, StyleValue>,
    /// Terminal colours.
    #[serde(default, skip_serializing_if = "TerminalSection::is_empty")]
    terminal: TerminalSection,
    /// Fields this version does not know.
    #[serde(flatten, skip_serializing)]
    #[schemars(skip)]
    other: BTreeMap<String, IgnoredValue>,
}

impl ThemeFile {
    fn into_theme(self) -> Theme {
        let mut unknown_keys: Vec<String> = self.other.into_keys().collect();
        unknown_keys.extend(self.ui.unknown.iter().map(|key| format!("ui.{key}")));
        unknown_keys.extend(self.editor.unknown.iter().map(|key| format!("editor.{key}")));
        unknown_keys.extend(self.syntax.unknown.iter().map(|key| format!("syntax.{key}")));
        let mut syntax = BTreeMap::new();
        for (key, value) in self.syntax.entries {
            let style = match value {
                StyleValue::Color(color) => Style::color(color),
                StyleValue::Style(object) => {
                    unknown_keys.extend(object.other.keys().map(|field| format!("syntax.{}.{field}", key.id())));
                    object.style()
                }
            };
            syntax.insert(key, style);
        }
        let terminal = self.terminal;
        unknown_keys.extend(terminal.other.keys().map(|field| format!("terminal.{field}")));
        let mut terminal_colors = BTreeMap::new();
        for (key, color) in [
            (TerminalKey::Background, terminal.background),
            (TerminalKey::Foreground, terminal.foreground),
            (TerminalKey::Cursor, terminal.cursor),
            (TerminalKey::Selection, terminal.selection),
        ] {
            if let Some(color) = color {
                terminal_colors.insert(key, color);
            }
        }
        if let Some(ansi) = terminal.ansi {
            for (key, color) in TerminalKey::ANSI.into_iter().zip(ansi) {
                if let Some(color) = color {
                    terminal_colors.insert(key, color);
                }
            }
        }
        Theme {
            id: self.id.unwrap_or_else(|| id_from_name(&self.name)),
            name: self.name,
            dark: self.dark,
            parent: self.parent,
            ui: self.ui.entries.into_iter().collect(),
            editor: self.editor.entries.into_iter().collect(),
            syntax,
            terminal: terminal_colors,
            unknown_keys,
        }
    }

    fn from_theme(theme: &Theme) -> ThemeFile {
        let ansi = TerminalKey::ANSI.map(|key| theme.terminal.get(&key).copied());
        ThemeFile {
            schema: None,
            format_version: FormatVersion,
            id: Some(theme.id.clone()),
            name: theme.name.clone(),
            dark: theme.dark,
            parent: theme.parent.clone(),
            ui: Section::from_map(&theme.ui, |color| *color),
            editor: Section::from_map(&theme.editor, |color| *color),
            syntax: Section::from_map(&theme.syntax, |style| {
                if style.is_plain() {
                    StyleValue::Color(style.color)
                } else {
                    StyleValue::Style(StyleObject::from(*style))
                }
            }),
            terminal: TerminalSection {
                background: theme.terminal.get(&TerminalKey::Background).copied(),
                foreground: theme.terminal.get(&TerminalKey::Foreground).copied(),
                cursor: theme.terminal.get(&TerminalKey::Cursor).copied(),
                selection: theme.terminal.get(&TerminalKey::Selection).copied(),
                ansi: ansi.iter().any(Option::is_some).then_some(ansi),
                other: BTreeMap::new(),
            },
            other: BTreeMap::new(),
        }
    }
}

struct IgnoredValue;

impl<'de> Deserialize<'de> for IgnoredValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<IgnoredValue, D::Error> {
        IgnoredAny::deserialize(deserializer).map(|_| IgnoredValue)
    }
}

struct FormatVersion;

impl Serialize for FormatVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(FORMAT_VERSION)
    }
}

impl<'de> Deserialize<'de> for FormatVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<FormatVersion, D::Error> {
        let version = u64::deserialize(deserializer)?;
        if version == u64::from(FORMAT_VERSION) {
            Ok(FormatVersion)
        } else {
            Err(de::Error::custom(format!(
                "formatVersion {version} is not supported; this version reads formatVersion {FORMAT_VERSION}"
            )))
        }
    }
}

impl JsonSchema for FormatVersion {
    fn schema_name() -> Cow<'static, str> {
        "FormatVersion".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "const": FORMAT_VERSION })
    }
}

struct Section<K, V> {
    entries: Vec<(K, V)>,
    unknown: Vec<String>,
}

impl<K: Key, V> Section<K, V> {
    fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.unknown.is_empty()
    }

    fn from_map<T>(map: &BTreeMap<K, T>, value: impl Fn(&T) -> V) -> Section<K, V> {
        Section { entries: map.iter().map(|(key, item)| (*key, value(item))).collect(), unknown: Vec::new() }
    }
}

impl<K, V> Default for Section<K, V> {
    fn default() -> Section<K, V> {
        Section { entries: Vec::new(), unknown: Vec::new() }
    }
}

impl<K: Key, V: Serialize> Serialize for Section<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.entries.iter().map(|(key, value)| (key.id(), value)))
    }
}

impl<'de, K: Key, V: Deserialize<'de>> Deserialize<'de> for Section<K, V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Section<K, V>, D::Error> {
        struct SectionVisitor<K, V>(PhantomData<(K, V)>);

        impl<'de, K: Key, V: Deserialize<'de>> Visitor<'de> for SectionVisitor<K, V> {
            type Value = Section<K, V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "an object of `{}` keys", K::SECTION)
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Section<K, V>, A::Error> {
                let mut section = Section::default();
                while let Some(id) = map.next_key::<Cow<'de, str>>()? {
                    match K::from_id(&id) {
                        Some(key) => {
                            let value = map.next_value::<V>()?;
                            section.entries.retain(|(other, _)| *other != key);
                            section.entries.push((key, value));
                        }
                        None => {
                            map.next_value::<IgnoredAny>()?;
                            section.unknown.push(id.into_owned());
                        }
                    }
                }
                Ok(section)
            }
        }

        deserializer.deserialize_map(SectionVisitor(PhantomData))
    }
}

impl<K: Key, V: JsonSchema> JsonSchema for Section<K, V> {
    fn schema_name() -> Cow<'static, str> {
        let mut name: String = K::SECTION[..1].to_uppercase();
        name.push_str(&K::SECTION[1..]);
        name.push_str("Section");
        name.into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let value = generator.subschema_for::<V>().to_value();
        let mut properties = serde_json::Map::new();
        for key in K::ALL {
            let mut property = serde_json::Map::new();
            property.insert("description".to_owned(), key.doc().into());
            if let serde_json::Value::Object(fields) = &value {
                property.extend(fields.clone());
            }
            properties.insert(key.id().to_owned(), property.into());
        }
        json_schema!({ "type": "object", "properties": properties })
    }
}

#[derive(Serialize, JsonSchema)]
#[schemars(description = "A code style: a colour, or an object with the colour and a font style.")]
#[serde(untagged)]
enum StyleValue {
    /// Plain text in this colour.
    Color(Rgba),
    /// A colour with a background or a font style.
    Style(StyleObject),
}

impl<'de> Deserialize<'de> for StyleValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<StyleValue, D::Error> {
        struct StyleVisitor;

        impl<'de> Visitor<'de> for StyleVisitor {
            type Value = StyleValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a colour such as \"#CF8E6D\" or a style object such as {\"color\": \"#CF8E6D\", \"italic\": true}",
                )
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<StyleValue, E> {
                text.parse().map(StyleValue::Color).map_err(E::custom)
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<StyleValue, A::Error> {
                StyleObject::deserialize(de::value::MapAccessDeserializer::new(map)).map(StyleValue::Style)
            }
        }

        deserializer.deserialize_any(StyleVisitor)
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(description = "A code style with more than a colour.")]
struct StyleObject {
    /// The text colour.
    color: Rgba,
    /// The background behind the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<Rgba>,
    /// Bold text.
    #[serde(default, skip_serializing_if = "is_false")]
    bold: bool,
    /// Italic text.
    #[serde(default, skip_serializing_if = "is_false")]
    italic: bool,
    /// The underline: `line` or `wave`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    underline: Option<Underline>,
    /// Struck-through text.
    #[serde(default, skip_serializing_if = "is_false")]
    strikethrough: bool,
    /// Fields this version does not know.
    #[serde(flatten, skip_serializing)]
    #[schemars(skip)]
    other: BTreeMap<String, IgnoredValue>,
}

impl StyleObject {
    fn style(&self) -> Style {
        Style {
            color: self.color,
            background: self.background,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strikethrough: self.strikethrough,
        }
    }
}

impl From<Style> for StyleObject {
    fn from(style: Style) -> StyleObject {
        StyleObject {
            color: style.color,
            background: style.background,
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            strikethrough: style.strikethrough,
            other: BTreeMap::new(),
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Default, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "The terminal's colours.")]
struct TerminalSection {
    /// The default background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<Rgba>,
    /// The default text colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    foreground: Option<Rgba>,
    /// The block cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<Rgba>,
    /// Selected text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selection: Option<Rgba>,
    /// The 16 ANSI colours: black, red, green, yellow, blue, magenta, cyan, white, then the bright ones; `null` keeps the parent's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ansi: Option<[Option<Rgba>; 16]>,
    /// Fields this version does not know.
    #[serde(flatten, skip_serializing)]
    #[schemars(skip)]
    other: BTreeMap<String, IgnoredValue>,
}

impl TerminalSection {
    fn is_empty(&self) -> bool {
        self.background.is_none()
            && self.foreground.is_none()
            && self.cursor.is_none()
            && self.ansi.is_none()
            && self.other.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zen_dark;

    fn error(text: &str) -> FormatError {
        parse(text).expect_err("parsed")
    }

    #[test]
    fn a_minimal_file_is_an_empty_theme() {
        let theme = parse(r#"{"formatVersion": 1, "name": "My Theme!", "dark": true}"#).unwrap();
        assert_eq!(theme, Theme::new("my-theme", "My Theme!", true));
    }

    #[test]
    fn every_section_is_read() {
        let theme = parse(
            r##"{
                // A child of Zen Dark.
                "formatVersion": 1,
                "id": "dusk",
                "name": "Dusk",
                "dark": true,
                "parent": "zen-dark",
                "ui": { "accent": "#F00", "link": "#00FF0080", },
                "editor": { "caret": "#123456" },
                "syntax": {
                    "keyword": "#CF8E6D",
                    "comment": { "color": "#7A7E85", "italic": true, "underline": "wave", "background": "#000" },
                },
                "terminal": { "cursor": "#FFF", "ansi": ["#000", null, null, null, null, null, null, null,
                                                         null, null, null, null, null, null, null, "#FFF"] },
            }"##,
        )
        .unwrap();
        assert_eq!(theme.id, "dusk");
        assert_eq!(theme.parent.as_deref(), Some("zen-dark"));
        assert_eq!(theme.ui[&UiKey::Accent], Rgba::rgb(0xFF, 0, 0));
        assert_eq!(theme.ui[&UiKey::Link], Rgba::new(0, 0xFF, 0, 0x80));
        assert_eq!(theme.editor[&EditorKey::Caret], Rgba::rgb(0x12, 0x34, 0x56));
        assert_eq!(theme.syntax[&SyntaxKey::Keyword], Style::color(Rgba::rgb(0xCF, 0x8E, 0x6D)));
        assert_eq!(
            theme.syntax[&SyntaxKey::Comment],
            Style {
                background: Some(Rgba::rgb(0, 0, 0)),
                underline: Some(Underline::Wave),
                ..Style::color(Rgba::rgb(0x7A, 0x7E, 0x85)).italic()
            }
        );
        assert_eq!(theme.terminal.len(), 3);
        assert_eq!(theme.terminal[&TerminalKey::Ansi15], Rgba::rgb(0xFF, 0xFF, 0xFF));
        assert!(theme.unknown_keys.is_empty());
    }

    #[test]
    fn unknown_keys_are_kept_not_rejected() {
        let theme = parse(
            r##"{"$schema": "theme.schema.json", "formatVersion": 1, "name": "N", "dark": false, "author": "Ada",
                "ui": {"accent": "#FFF", "no.such.key": 42},
                "editor": {"glow": "#FFF"},
                "syntax": {"keyword": {"color": "#FFF", "weight": 700}, "sparkle": "not even a colour"},
                "terminal": {"bell": "#FFF"}}"##,
        )
        .unwrap();
        assert_eq!(
            theme.unknown_keys,
            ["author", "ui.no.such.key", "editor.glow", "syntax.sparkle", "syntax.keyword.weight", "terminal.bell"]
        );
        assert_eq!(theme.ui.len(), 1);
        assert_eq!(theme.syntax.len(), 1);
    }

    #[test]
    fn errors_have_lines_and_columns() {
        let found = error(
            "{\n  \"formatVersion\": 1,\n  \"name\": \"N\",\n  \"dark\": true,\n  \"ui\": {\"accent\": \"#12345\"}\n}",
        );
        assert_eq!(found.line, 5);
        assert!(found.message.contains("`#12345` is not a colour"), "{found}");
        assert_eq!(found.to_string(), format!("line 5, column {}: {}", found.column, found.message));

        let found = error("{\"formatVersion\": 2, \"name\": \"N\", \"dark\": true}");
        assert!(found.message.contains("formatVersion 2 is not supported"), "{found}");
        assert_eq!((found.line, found.column), (1, 19));

        let found = error("/* a comment\n */ {\"formatVersion\": 1, \"name\": \"N\"}");
        assert!(found.message.contains("missing field `dark`"), "{found}");
        assert_eq!(found.line, 2);

        let found =
            error("{\"formatVersion\": 1, \"name\": \"N\", \"dark\": true, \"terminal\": {\"ansi\": [\"#000\"]}}");
        assert!(found.message.contains("invalid length 1"), "{found}");

        let found = error("{\"formatVersion\": 1, \"name\": \"N\", \"dark\": true, \"syntax\": {\"keyword\": 7}}");
        assert!(found.message.contains("a colour such as"), "{found}");

        assert_eq!(error("").line, 1);
    }

    #[test]
    fn writing_then_reading_gives_the_same_theme() {
        let mut theme = Theme::new("sparse", "Sparse", false);
        theme.parent = Some("zen-dark".to_owned());
        theme.ui.insert(UiKey::PopupShadow, Rgba::new(0, 0, 0, 0x6E));
        theme
            .syntax
            .insert(SyntaxKey::Strong, Style { bold: true, strikethrough: true, ..Style::color(Rgba::rgb(1, 2, 3)) });
        theme.terminal.insert(TerminalKey::Ansi3, Rgba::rgb(4, 5, 6));
        assert_eq!(parse(&to_json(&theme)), Ok(theme));
        assert_eq!(parse(&to_json(&zen_dark())), Ok(zen_dark()));
    }

    #[test]
    fn an_empty_theme_writes_only_its_header() {
        assert_eq!(
            to_json(&Theme::new("e", "E", true)),
            "{\n  \"formatVersion\": 1,\n  \"id\": \"e\",\n  \"name\": \"E\",\n  \"dark\": true\n}\n"
        );
    }

    #[test]
    fn ids_are_made_from_names() {
        assert_eq!(id_from_name("Zen Dark"), "zen-dark");
        assert_eq!(id_from_name("  Solar -- Flare 2 "), "solar-flare-2");
        assert_eq!(id_from_name("!!!"), "theme");
    }
}

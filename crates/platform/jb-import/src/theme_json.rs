use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use zen_theme::{ImportError, ImportReport, Rgba, Theme, ThemeImporter, UiKey, base_id, format, jsonc};

use crate::{IclsImporter, color};

#[derive(Clone, Copy, Debug, Default)]
pub struct ThemeJsonImporter;

const UI: &[(UiKey, &[&str])] = &[
    (UiKey::WindowBackground, &["Panel.background"]),
    (UiKey::IslandBackground, &["ToolWindow.background", "Editor.background"]),
    (UiKey::TextPrimary, &["Label.foreground"]),
    (UiKey::TextSecondary, &["Component.infoForeground", "Label.infoForeground"]),
    (UiKey::TextError, &["Label.errorForeground"]),
    (UiKey::Link, &["Link.activeForeground"]),
    (UiKey::Accent, &["Button.default.startBackground"]),
    (UiKey::AccentForeground, &["Button.default.foreground"]),
    (UiKey::ListHover, &["List.hoverBackground", "Tree.hoverBackground"]),
    (UiKey::ListSelection, &["Tree.selectionBackground", "List.selectionBackground"]),
    (UiKey::PopupBackground, &["Popup.background"]),
    (UiKey::PopupBorder, &["Popup.borderColor"]),
    (UiKey::PopupHover, &["MenuItem.selectionBackground"]),
    (UiKey::TextFieldBackground, &["TextField.background"]),
    (UiKey::TextFieldBorder, &["Component.borderColor"]),
    (UiKey::TextFieldSelection, &["TextField.selectionBackground"]),
    (UiKey::TextFieldCaret, &["TextField.caretForeground"]),
    (UiKey::ButtonSecondaryBorder, &["Button.startBorderColor"]),
    (UiKey::TabBorder, &["Borders.color"]),
    (UiKey::TabActiveBackground, &["EditorTabs.underlinedTabBackground"]),
    (UiKey::TabActiveBorder, &["EditorTabs.underlineColor"]),
];

const KNOWN: &[&str] = &["name", "dark", "author", "colors", "ui", "editorScheme"];

const MAX_NAMED_DEPTH: usize = 8;

struct UiTheme {
    name: String,
    dark: bool,
    editor_scheme: Option<String>,
    colors: Map<String, Value>,
    flat: BTreeMap<String, Value>,
    extra: Vec<String>,
}

impl UiTheme {
    fn parse(text: &str) -> Result<UiTheme, ImportError> {
        let value: Value = serde_json::from_str(&jsonc::strip(text))
            .map_err(|error| ImportError(format!("not a UI theme: {error}")))?;
        let Value::Object(object) = value else {
            return Err(ImportError("not a UI theme: the file is not a JSON object".to_owned()));
        };
        let mut flat = BTreeMap::new();
        if let Some(Value::Object(ui)) = object.get("ui") {
            flatten("", ui, &mut flat);
        }
        Ok(UiTheme {
            name: object.get("name").and_then(Value::as_str).unwrap_or("Imported theme").to_owned(),
            dark: object.get("dark").and_then(Value::as_bool).unwrap_or(true),
            editor_scheme: object.get("editorScheme").and_then(Value::as_str).map(str::to_owned),
            colors: match object.get("colors") {
                Some(Value::Object(colors)) => colors.clone(),
                _ => Map::new(),
            },
            flat,
            extra: object.keys().filter(|key| !KNOWN.contains(&key.as_str())).cloned().collect(),
        })
    }

    fn lookup(&self, key: &str) -> Option<(&str, &Value)> {
        if let Some((name, value)) = self.flat.get_key_value(key) {
            return Some((name, value));
        }
        self.flat
            .iter()
            .filter_map(|(name, value)| {
                let suffix = name.strip_prefix("*.")?;
                key.ends_with(&format!(".{suffix}")).then_some((suffix.len(), name.as_str(), value))
            })
            .max_by_key(|(length, _, _)| *length)
            .map(|(_, name, value)| (name, value))
    }

    fn color(&self, value: &Value) -> Option<Rgba> {
        let mut text = value.as_str()?;
        for _ in 0..MAX_NAMED_DEPTH {
            if text.starts_with('#') {
                return color::parse(text);
            }
            match self.colors.get(text).and_then(Value::as_str) {
                Some(next) => text = next,
                None => return color::parse(text),
            }
        }
        None
    }

    fn into_report(self) -> ImportReport {
        let mut theme = Theme::new(format::id_from_name(&self.name), self.name.clone(), self.dark);
        theme.parent = Some(base_id(theme.dark).to_owned());
        let mut used = BTreeSet::new();
        let mut mapped = 0;
        for (key, sources) in UI {
            let found = sources.iter().find_map(|source| {
                let (name, value) = self.lookup(source)?;
                Some((name.to_owned(), self.color(value)?))
            });
            if let Some((name, rgba)) = found {
                theme.ui.insert(*key, rgba);
                mapped += 1;
                used.insert(name);
            }
        }
        let mut unmapped = self.extra.clone();
        unmapped.extend(self.flat.keys().filter(|name| !used.contains(*name)).cloned());
        if self.editor_scheme.is_some() {
            unmapped.push("editorScheme".to_owned());
        }
        ImportReport { theme, mapped, unmapped }
    }
}

fn flatten(prefix: &str, object: &Map<String, Value>, out: &mut BTreeMap<String, Value>) {
    for (key, value) in object {
        let name = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        match value {
            Value::Object(inner) => flatten(&name, inner, out),
            other => {
                out.insert(name, other.clone());
            }
        }
    }
}

impl ThemeJsonImporter {
    pub fn detects_path(&self, path: &Path, text: &str) -> bool {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("").to_ascii_lowercase();
        name.ends_with(".json") && self.detects(text)
    }

    pub fn import_with_scheme(&self, text: &str, scheme: Option<&str>) -> Result<ImportReport, ImportError> {
        let ui = UiTheme::parse(text)?;
        let has_scheme = ui.editor_scheme.is_some();
        let mut report = ui.into_report();
        let Some(scheme) = scheme.filter(|_| has_scheme) else { return Ok(report) };
        let editor = IclsImporter.import(scheme).map_err(|error| ImportError(format!("editor scheme: {error}")))?;
        report.unmapped.retain(|name| name != "editorScheme");
        report.theme.editor = editor.theme.editor;
        report.theme.syntax = editor.theme.syntax;
        report.theme.terminal = editor.theme.terminal;
        report.mapped += editor.mapped;
        report.unmapped.extend(editor.unmapped);
        Ok(report)
    }

    pub fn scheme_path(&self, path: &Path, text: &str) -> Option<PathBuf> {
        scheme_candidates(path, text).into_iter().find(|candidate| candidate.is_file())
    }

    pub fn import_file(&self, path: &Path) -> Result<ImportReport, ImportError> {
        let read = |path: &Path| {
            std::fs::read_to_string(path).map_err(|error| ImportError(format!("{}: {error}", path.display())))
        };
        let text = read(path)?;
        let scheme = self.scheme_path(path, &text).map(|scheme| read(&scheme)).transpose()?;
        self.import_with_scheme(&text, scheme.as_deref())
    }
}

fn scheme_candidates(path: &Path, text: &str) -> Vec<PathBuf> {
    let Some(scheme) = UiTheme::parse(text).ok().and_then(|theme| theme.editor_scheme) else { return Vec::new() };
    let (Some(folder), Some(file)) = (path.parent(), Path::new(&scheme).file_name()) else { return Vec::new() };
    let Some(stem) = Path::new(file).file_stem().and_then(|stem| stem.to_str()) else { return Vec::new() };
    vec![folder.join(file), folder.join(format!("{stem}.icls")), folder.join(format!("{stem}.xml"))]
}

impl ThemeImporter for ThemeJsonImporter {
    fn format_name(&self) -> &'static str {
        "JetBrains UI theme"
    }

    fn detects(&self, text: &str) -> bool {
        let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&jsonc::strip(text)) else { return false };
        let jetbrains =
            object.get("ui").is_some_and(Value::is_object) || object.get("editorScheme").is_some_and(Value::is_string);
        jetbrains && !object.contains_key("tokenColors") && !object.contains_key("semanticTokenColors")
    }

    fn import(&self, text: &str) -> Result<ImportReport, ImportError> {
        self.import_with_scheme(text, None)
    }

    fn detects_at(&self, path: &str, text: &str) -> bool {
        self.detects_path(Path::new(path), text)
    }

    fn import_at(
        &self,
        path: &str,
        text: &str,
        read: &dyn Fn(&str) -> Option<String>,
    ) -> Result<ImportReport, ImportError> {
        let scheme = scheme_candidates(Path::new(path), text)
            .into_iter()
            .find_map(|candidate| read(&candidate.to_string_lossy()));
        self.import_with_scheme(text, scheme.as_deref())
    }
}

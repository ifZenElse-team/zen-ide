use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RESERVED_IDS: &[&str] = &["useTabs", "tabSize", "indent", "arrangement"];

/// Text shown to people: a message id of the IDE's own translations, or the text by language code (`en`, `ru`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Label {
    Message(String),
    Texts(BTreeMap<String, String>),
}

impl Default for Label {
    fn default() -> Self {
        Label::Texts(BTreeMap::new())
    }
}

impl Label {
    pub fn resolve(&self, language: &str, translate: impl Fn(&str) -> String) -> String {
        match self {
            Label::Message(id) => translate(id),
            Label::Texts(texts) => {
                let base = language.split(['-', '_']).next().unwrap_or(language);
                [language, base, "en"]
                    .iter()
                    .find_map(|code| texts.get(*code))
                    .or_else(|| texts.values().next())
                    .cloned()
                    .unwrap_or_default()
            }
        }
    }
}

/// A tab of a language's Code Style page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StyleTab {
    TabsAndIndents,
    Spaces,
    WrappingAndBraces,
    BlankLines,
    Imports,
    Arrangement,
}

impl StyleTab {
    pub const ALL: [StyleTab; 6] = [
        StyleTab::TabsAndIndents,
        StyleTab::Spaces,
        StyleTab::WrappingAndBraces,
        StyleTab::BlankLines,
        StyleTab::Imports,
        StyleTab::Arrangement,
    ];

    pub fn id(self) -> &'static str {
        match self {
            StyleTab::TabsAndIndents => "tabsAndIndents",
            StyleTab::Spaces => "spaces",
            StyleTab::WrappingAndBraces => "wrappingAndBraces",
            StyleTab::BlankLines => "blankLines",
            StyleTab::Imports => "imports",
            StyleTab::Arrangement => "arrangement",
        }
    }
}

/// One value a choice option takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// The value written to the formatter, the server or the file.
    pub value: String,
    pub label: Label,
}

/// What values an option takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OptionKind {
    Boolean,
    Integer { min: i64, max: i64 },
    Choice { choices: Vec<Choice> },
    Text,
}

/// A setting of a language server's configuration section that an option sets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerSetting {
    /// The configuration section, as `gopls`.
    pub section: String,
    /// The setting inside the section, dot-separated, as `formatting.gofumpt`.
    pub path: String,
    /// Sends a text option as a list: its items split at this separator, each trimmed, as `;`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub separator: Option<String>,
}

/// One option of a language's code style.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StyleOption {
    /// Stable id: the setting `editor.codeStyle.<language>.<id>` holds its value.
    pub id: String,
    pub label: Label,
    pub tab: StyleTab,
    /// The group on its tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Label>,
    pub kind: OptionKind,
    pub default: Value,
    /// The key of `textDocument/formatting` options it is sent as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatting_option: Option<String>,
    /// The language server setting it is sent as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_setting: Option<ServerSetting>,
    /// The key of the language's configuration file it is written as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_key: Option<String>,
    /// More keys of the configuration file that are written with the same value, as one option sets several settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more_config_keys: Vec<String>,
    /// How a value is written in the configuration file, by the value's plain text, as `true` → `insert`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_values: BTreeMap<String, String>,
    /// The EditorConfig property it is exported as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_config: Option<String>,
}

/// A server setting that carries the configuration file's keys on a command line when the IDE's scheme is used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigArguments {
    pub section: String,
    pub path: String,
    /// The flag before the `key=value,key=value` list, as `--config`.
    pub flag: String,
}

/// How the formatter's configuration file is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConfigFormat {
    /// A flat TOML file of `key = value` lines.
    #[default]
    Toml,
    /// An Eclipse formatter profile: `<setting id="key" value="value"/>` lines in one `<profile>`.
    EclipseProfile,
    /// An `.editorconfig` file: `key = value` lines in the section `section` names.
    EditorConfig,
}

/// The formatter's own configuration file in the project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    /// The file names the formatter reads, relative to the project's root folder; the first is written when none exists.
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<ConfigArguments>,
    #[serde(default, skip_serializing_if = "is_toml")]
    pub format: ConfigFormat,
    /// A server setting given the file's path: the project's file with the Project scheme, a file the IDE writes from its own values otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<ServerSetting>,
    /// The section an EditorConfig file keeps the keys in, as `*.{kt,kts}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

fn is_toml(format: &ConfigFormat) -> bool {
    *format == ConfigFormat::Toml
}

/// A kind of declaration Rearrange Code sorts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementEntry {
    pub id: String,
    pub label: Label,
}

/// The code style a language declares: its options, the text its page previews and its arrangement rules.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeStyleDeclaration {
    /// The language id, as `go` or `rust`.
    pub language: String,
    /// The page's name in the Settings tree.
    pub title: Label,
    #[serde(default)]
    pub preview: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<StyleOption>,
    /// What a tab says about the formatter, as why it has no options there.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<StyleTab, Label>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file: Option<ConfigFile>,
    /// The kinds of declarations Rearrange Code sorts, in their default order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrangement: Vec<ArrangementEntry>,
}

pub type Values = BTreeMap<String, Value>;

impl CodeStyleDeclaration {
    pub fn parse(json: &str) -> Result<Self, String> {
        let declaration: Self =
            serde_json::from_str(json).map_err(|error| format!("the code style declaration is invalid: {error}"))?;
        declaration.check()?;
        Ok(declaration)
    }

    pub fn parse_list(json: &str) -> Result<Vec<Self>, String> {
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        let declarations: Vec<Self> =
            serde_json::from_str(json).map_err(|error| format!("the code style declarations are invalid: {error}"))?;
        for declaration in &declarations {
            declaration.check()?;
        }
        Ok(declarations)
    }

    pub fn check(&self) -> Result<(), String> {
        if self.language.is_empty() {
            return Err("a code style declaration needs a language".to_owned());
        }
        for name in self.config_file.iter().flat_map(|file| &file.names) {
            let path = std::path::Path::new(name);
            let inside = path.components().all(|part| matches!(part, std::path::Component::Normal(_)));
            if name.is_empty() || !inside {
                return Err(format!("{}: `{name}` is not a file of the project", self.language));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for option in &self.options {
            let valid = !option.id.is_empty()
                && option.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !RESERVED_IDS.contains(&option.id.as_str());
            if !valid {
                return Err(format!("{}: `{}` is not a valid option id", self.language, option.id));
            }
            if !seen.insert(option.id.as_str()) {
                return Err(format!("{}: the option `{}` is declared twice", self.language, option.id));
            }
            if !option.accepts(&option.default) {
                return Err(format!("{}: the default of `{}` is not one of its values", self.language, option.id));
            }
        }
        Ok(())
    }

    pub fn message_ids(&self) -> Vec<&str> {
        let mut labels: Vec<&Label> = vec![&self.title];
        for option in &self.options {
            labels.push(&option.label);
            labels.extend(&option.group);
            if let OptionKind::Choice { choices } = &option.kind {
                labels.extend(choices.iter().map(|choice| &choice.label));
            }
        }
        labels.extend(self.notes.values());
        labels.extend(self.arrangement.iter().map(|entry| &entry.label));
        labels
            .into_iter()
            .filter_map(|label| match label {
                Label::Message(id) => Some(id.as_str()),
                Label::Texts(_) => None,
            })
            .collect()
    }

    pub fn option(&self, id: &str) -> Option<&StyleOption> {
        self.options.iter().find(|option| option.id == id)
    }

    pub fn defaults(&self) -> Values {
        self.options.iter().map(|option| (option.id.clone(), option.default.clone())).collect()
    }

    pub fn complete(&self, given: &Values) -> Values {
        self.options
            .iter()
            .map(|option| {
                let value = given.get(&option.id).filter(|value| option.accepts(value)).unwrap_or(&option.default);
                (option.id.clone(), value.clone())
            })
            .collect()
    }

    pub fn formatting_options(&self, values: &Values) -> serde_json::Map<String, Value> {
        let values = self.complete(values);
        self.options
            .iter()
            .filter_map(|option| Some((option.formatting_option.clone()?, values.get(&option.id)?.clone())))
            .collect()
    }

    pub fn server_settings(&self, values: &Values) -> BTreeMap<String, Value> {
        let values = self.complete(values);
        let mut sections: BTreeMap<String, Value> = BTreeMap::new();
        for option in &self.options {
            let (Some(setting), Some(value)) = (&option.server_setting, values.get(&option.id)) else { continue };
            let section = sections.entry(setting.section.clone()).or_insert_with(|| Value::Object(Default::default()));
            if *value != option.default {
                let sent = match (&setting.separator, value.as_str()) {
                    (Some(separator), Some(text)) => Value::from(
                        text.split(separator.as_str()).map(|item| item.trim().to_owned()).collect::<Vec<_>>(),
                    ),
                    _ => value.clone(),
                };
                set_path(section, &setting.path, sent);
            }
        }
        sections
    }

    pub fn config_arguments(&self, values: &Values) -> Option<(String, Value)> {
        let arguments = self.config_file.as_ref()?.arguments.as_ref()?;
        let values = self.complete(values);
        let pairs: Vec<String> = self
            .options
            .iter()
            .filter_map(|option| {
                let key = option.config_key.as_ref()?;
                let value = values.get(&option.id)?;
                (*value != option.default).then(|| format!("{key}={}", plain(value)))
            })
            .collect();
        if pairs.is_empty() {
            return None;
        }
        let mut section = Value::Object(Default::default());
        set_path(&mut section, &arguments.path, Value::from(vec![arguments.flag.clone(), pairs.join(",")]));
        Some((arguments.section.clone(), section))
    }

    pub fn editor_config_pairs(&self, values: &Values) -> Vec<(String, String)> {
        let values = self.complete(values);
        self.options
            .iter()
            .filter_map(|option| {
                let key = option.editor_config.as_ref()?;
                let value = values.get(&option.id)?;
                (*value != option.default).then(|| (key.clone(), plain(value)))
            })
            .collect()
    }

    pub fn with_config_values(&self, base: &Values, file: &Values) -> Values {
        let mut values = self.complete(base);
        let file = self.complete(file);
        for option in self.options.iter().filter(|option| option.config_key.is_some()) {
            if let Some(value) = file.get(&option.id) {
                values.insert(option.id.clone(), value.clone());
            }
        }
        values
    }

    pub fn config_format(&self) -> ConfigFormat {
        self.config_file.as_ref().map(|file| file.format).unwrap_or_default()
    }

    pub fn differs_in_config(&self, values: &Values) -> bool {
        let values = self.complete(values);
        self.options.iter().any(|option| option.config_key.is_some() && values.get(&option.id) != Some(&option.default))
    }

    pub fn read_config(&self, text: &str) -> Values {
        let format = self.config_format();
        let mapped = format != ConfigFormat::Toml;
        let found = match format {
            ConfigFormat::Toml => top_level_pairs(text),
            ConfigFormat::EclipseProfile => profile_settings(text),
            ConfigFormat::EditorConfig => section_pairs(text, self.editor_config_section()),
        };
        self.options
            .iter()
            .filter_map(|option| {
                let key = option.config_key.as_ref()?;
                let (_, raw) = found.iter().find(|(name, _)| name == key)?;
                let value = if mapped { option.read_written(raw)? } else { parse_value(raw, &option.kind)? };
                option.accepts(&value).then(|| (option.id.clone(), value))
            })
            .collect()
    }

    pub fn write_config(&self, text: &str, values: &Values) -> String {
        match self.config_format() {
            ConfigFormat::EclipseProfile => return self.write_profile(text, values),
            ConfigFormat::EditorConfig => return self.write_editor_config(text, values),
            ConfigFormat::Toml => {}
        }
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let header = lines.iter().position(|line| line.trim_start().starts_with('[')).unwrap_or(lines.len());
        let mut added = Vec::new();
        for option in &self.options {
            let (Some(key), Some(value)) = (&option.config_key, values.get(&option.id)) else { continue };
            if !option.accepts(value) {
                continue;
            }
            let line = format!("{key} = {}", toml_value(value));
            match lines[..header].iter().position(|existing| line_key(existing).as_deref() == Some(key.as_str())) {
                Some(at) => lines[at] = line,
                None if *value != option.default => added.push(line),
                None => {}
            }
        }
        let mut at = header;
        while at > 0 && lines[at - 1].trim().is_empty() && header < lines.len() {
            at -= 1;
        }
        for (offset, line) in added.into_iter().enumerate() {
            lines.insert(at + offset, line);
        }
        let mut written = lines.join("\n");
        if !written.is_empty() {
            written.push('\n');
        }
        written
    }
}

impl CodeStyleDeclaration {
    fn write_profile(&self, text: &str, values: &Values) -> String {
        let mut given = self.read_config(text);
        given.extend(values.iter().map(|(id, value)| (id.clone(), value.clone())));
        let values = self.complete(&given);
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if !lines.iter().any(|line| line.trim() == PROFILE_END) {
            lines = PROFILE_SKELETON.lines().map(str::to_owned).collect();
        }
        for option in &self.options {
            let (Some(key), Some(value)) = (&option.config_key, values.get(&option.id)) else { continue };
            let written = xml_escape(&option.written(value));
            for key in std::iter::once(key).chain(&option.more_config_keys) {
                let line = format!("    <setting id=\"{}\" value=\"{written}\"/>", xml_escape(key));
                match lines.iter().position(|existing| setting_pair(existing).is_some_and(|(id, _)| id == *key)) {
                    Some(at) => lines[at] = line,
                    None => {
                        let end =
                            lines.iter().position(|existing| existing.trim() == PROFILE_END).unwrap_or(lines.len());
                        lines.insert(end, line);
                    }
                }
            }
        }
        let mut written = lines.join("\n");
        written.push('\n');
        written
    }
}

impl CodeStyleDeclaration {
    fn editor_config_section(&self) -> &str {
        self.config_file.as_ref().and_then(|file| file.section.as_deref()).unwrap_or("*")
    }

    fn write_editor_config(&self, text: &str, values: &Values) -> String {
        let header = format!("[{}]", self.editor_config_section());
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let start = lines.iter().position(|line| line.trim() == header).map(|at| at + 1);
        let mut end = match start {
            Some(start) => lines[start..]
                .iter()
                .position(|line| line.trim_start().starts_with('['))
                .map_or(lines.len(), |at| start + at),
            None => lines.len(),
        };
        let mut added = Vec::new();
        for option in &self.options {
            let (Some(key), Some(value)) = (&option.config_key, values.get(&option.id)) else { continue };
            if !option.accepts(value) {
                continue;
            }
            let line = format!("{key} = {}", option.written(value));
            let existing = start.and_then(|start| {
                lines[start..end]
                    .iter()
                    .position(|existing| editor_config_key(existing).as_deref() == Some(key.as_str()))
            });
            match (existing, start) {
                (Some(at), Some(start)) => lines[start + at] = line,
                _ if *value != option.default => added.push(line),
                _ => {}
            }
        }
        if !added.is_empty() {
            if start.is_none() {
                if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(header);
                end = lines.len();
            }
            while start.is_some() && end > 0 && lines[end - 1].trim().is_empty() {
                end -= 1;
            }
            for (offset, line) in added.into_iter().enumerate() {
                lines.insert(end + offset, line);
            }
        }
        let mut written = lines.join("\n");
        if !written.is_empty() {
            written.push('\n');
        }
        written
    }
}

fn editor_config_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with(['#', ';', '[']) {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_ascii_lowercase())
}

fn section_pairs(text: &str, section: &str) -> Vec<(String, String)> {
    let header = format!("[{section}]");
    text.lines()
        .skip_while(|line| line.trim() != header)
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter_map(|line| {
            let key = editor_config_key(line)?;
            let (_, raw) = line.split_once('=')?;
            Some((key, raw.trim().to_owned()))
        })
        .collect()
}

const PROFILE_END: &str = "</profile>";

const PROFILE_SKELETON: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n<profiles version=\"23\">\n<profile kind=\"CodeFormatterProfile\" name=\"Zen IDE\" version=\"23\">\n</profile>\n</profiles>";

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        rest = &rest[at + name.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(after) = after.strip_prefix('=') else { continue };
        let after = after.trim_start();
        let quote = after.chars().next().filter(|quote| *quote == '"' || *quote == '\'')?;
        let (value, _) = after[1..].split_once(quote)?;
        return Some(xml_unescape(value));
    }
    None
}

fn setting_pair(line: &str) -> Option<(String, String)> {
    let start = line.find("<setting")?;
    let tag = &line[start + "<setting".len()..];
    let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
    Some((attribute(tag, "id")?, attribute(tag, "value")?))
}

fn profile_settings(text: &str) -> Vec<(String, String)> {
    text.split_inclusive('>').filter_map(setting_pair).collect()
}

impl StyleOption {
    fn written(&self, value: &Value) -> String {
        let text = plain(value);
        self.config_values.get(&text).cloned().unwrap_or(text)
    }

    fn read_written(&self, raw: &str) -> Option<Value> {
        let plain = self.config_values.iter().find(|(_, written)| *written == raw).map_or(raw, |(plain, _)| plain);
        match &self.kind {
            OptionKind::Boolean => plain.trim().parse::<bool>().ok().map(Value::Bool),
            OptionKind::Integer { .. } => plain.trim().parse::<i64>().ok().map(Value::from),
            OptionKind::Choice { .. } | OptionKind::Text => Some(Value::String(plain.to_owned())),
        }
    }

    pub fn accepts(&self, value: &Value) -> bool {
        match &self.kind {
            OptionKind::Boolean => value.is_boolean(),
            OptionKind::Integer { min, max } => value.as_i64().is_some_and(|number| (*min..=*max).contains(&number)),
            OptionKind::Choice { choices } => {
                value.as_str().is_some_and(|text| choices.iter().any(|choice| choice.value == text))
            }
            OptionKind::Text => value.as_str().is_some_and(|text| !text.contains(['\n', '\r'])),
        }
    }
}

pub fn set_path(target: &mut Value, path: &str, value: Value) {
    let mut node = target;
    let mut names = path.split('.').peekable();
    while let Some(name) = names.next() {
        if !node.is_object() {
            *node = Value::Object(serde_json::Map::new());
        }
        let Some(object) = node.as_object_mut() else { return };
        if names.peek().is_none() {
            object.insert(name.to_owned(), value);
            return;
        }
        node = object.entry(name.to_owned()).or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
}

pub fn merge(target: &mut Value, over: &Value) {
    match (target, over) {
        (Value::Object(target), Value::Object(over)) => {
            for (key, value) in over {
                match target.get_mut(key) {
                    Some(existing) if existing.is_object() && value.is_object() => merge(existing, value),
                    _ => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, over) => *target = over.clone(),
    }
}

fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn toml_value(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        }
        other => other.to_string(),
    }
}

fn line_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim().trim_matches('"');
    (!key.is_empty()).then(|| key.to_owned())
}

fn top_level_pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter_map(|line| {
            let key = line_key(line)?;
            let (_, raw) = line.split_once('=')?;
            Some((key, raw.trim().to_owned()))
        })
        .collect()
}

fn parse_value(raw: &str, kind: &OptionKind) -> Option<Value> {
    if let Some(rest) = raw.strip_prefix('"') {
        let mut text = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => text.push(chars.next()?),
                '"' => return Some(Value::String(text)),
                c => text.push(c),
            }
        }
        return None;
    }
    if let Some(rest) = raw.strip_prefix('\'') {
        return rest.split_once('\'').map(|(text, _)| Value::String(text.to_owned()));
    }
    let bare = raw.split('#').next().unwrap_or_default().trim();
    match kind {
        OptionKind::Boolean => bare.parse::<bool>().ok().map(Value::Bool),
        OptionKind::Integer { .. } => bare.replace('_', "").parse::<i64>().ok().map(Value::from),
        OptionKind::Choice { .. } | OptionKind::Text => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn declaration() -> CodeStyleDeclaration {
        CodeStyleDeclaration::parse(
            r#"{
                "language": "rust",
                "title": { "en": "Rust", "ru": "Rust" },
                "preview": "fn main() {}\n",
                "options": [
                    { "id": "max_width", "label": "settings-x", "tab": "wrappingAndBraces",
                      "kind": { "type": "integer", "min": 1, "max": 1000 }, "default": 100,
                      "configKey": "max_width", "editorConfig": "max_line_length" },
                    { "id": "hard_tabs", "label": { "en": "Hard tabs" }, "tab": "tabsAndIndents",
                      "kind": { "type": "boolean" }, "default": false, "configKey": "hard_tabs",
                      "formattingOption": "zenHardTabs" },
                    { "id": "newline_style", "label": { "en": "Newlines" }, "tab": "wrappingAndBraces",
                      "kind": { "type": "choice", "choices": [
                          { "value": "Auto", "label": { "en": "Auto" } },
                          { "value": "Unix", "label": { "en": "Unix" } } ] },
                      "default": "Auto", "configKey": "newline_style" },
                    { "id": "local", "label": { "en": "Local" }, "tab": "imports", "kind": { "type": "text" },
                      "default": "", "serverSetting": { "section": "gopls", "path": "formatting.local" } }
                ],
                "configFile": { "names": ["rustfmt.toml", ".rustfmt.toml"],
                    "arguments": { "section": "rust-analyzer", "path": "rustfmt.extraArgs", "flag": "--config" } }
            }"#,
        )
        .unwrap()
    }

    fn values(pairs: &[(&str, Value)]) -> Values {
        pairs.iter().map(|(id, value)| ((*id).to_owned(), value.clone())).collect()
    }

    #[test]
    fn a_configuration_file_stays_in_the_project() {
        let declared = |name: &str| {
            format!(
                r#"[{{ "language": "x", "title": {{ "en": "X" }}, "options": [], "configFile": {{ "names": ["{name}"] }} }}]"#
            )
        };
        assert!(CodeStyleDeclaration::parse_list(&declared(".zen/style.xml")).is_ok());
        for name in ["../../.ssh/config", "/etc/hosts", ""] {
            assert!(CodeStyleDeclaration::parse_list(&declared(name)).is_err(), "{name}");
        }
    }

    #[test]
    fn a_declaration_with_a_reserved_twice_declared_or_invalid_default_option_is_refused() {
        let base = r#"{"language":"go","title":"t","options":[OPTIONS]}"#;
        let option = |id: &str, default: &str| {
            format!(r#"{{"id":"{id}","label":"l","tab":"spaces","kind":{{"type":"boolean"}},"default":{default}}}"#)
        };
        let parse = |options: String| CodeStyleDeclaration::parse(&base.replace("OPTIONS", &options));
        assert!(parse(option("ok", "true")).is_ok());
        assert!(parse(option("indent", "true")).is_err());
        assert!(parse(option("a.b", "true")).is_err());
        assert!(parse(option("ok", "3")).is_err());
        assert!(parse(format!("{},{}", option("ok", "true"), option("ok", "false"))).is_err());
    }

    #[test]
    fn labels_take_the_interface_language_then_english() {
        let label =
            Label::Texts([("en", "Imports"), ("ru", "Импорты")].map(|(k, v)| (k.to_owned(), v.to_owned())).into());
        assert_eq!(label.resolve("ru-RU", |id| id.to_owned()), "Импорты");
        assert_eq!(label.resolve("de", |id| id.to_owned()), "Imports");
        assert_eq!(Label::Message("x".to_owned()).resolve("ru", |id| format!("<{id}>")), "<x>");
    }

    #[test]
    fn unknown_and_invalid_values_fall_back_to_the_defaults() {
        let style = declaration();
        let complete = style.complete(&values(&[("max_width", json!(5000)), ("hard_tabs", json!(true))]));
        assert_eq!(complete["max_width"], json!(100));
        assert_eq!(complete["hard_tabs"], json!(true));
        assert_eq!(complete["newline_style"], json!("Auto"));
    }

    #[test]
    fn values_reach_formatting_options_server_settings_arguments_and_editorconfig() {
        let style = declaration();
        let given = values(&[("max_width", json!(80)), ("local", json!("example.com")), ("hard_tabs", json!(true))]);
        assert_eq!(style.formatting_options(&given).get("zenHardTabs"), Some(&json!(true)));
        assert_eq!(style.server_settings(&given)["gopls"], json!({ "formatting": { "local": "example.com" } }));
        let (section, arguments) = style.config_arguments(&given).unwrap();
        assert_eq!(section, "rust-analyzer");
        assert_eq!(arguments, json!({ "rustfmt": { "extraArgs": ["--config", "max_width=80,hard_tabs=true"] } }));
        assert_eq!(style.config_arguments(&Values::new()), None, "nothing to pass at the defaults");
        assert_eq!(style.editor_config_pairs(&given), [("max_line_length".to_owned(), "80".to_owned())]);
    }

    fn profile() -> CodeStyleDeclaration {
        CodeStyleDeclaration::parse(
            r##"{
                "language": "java",
                "title": { "en": "Java" },
                "options": [
                    { "id": "braces", "label": { "en": "Braces" }, "tab": "wrappingAndBraces",
                      "kind": { "type": "choice", "choices": [
                          { "value": "end_of_line", "label": { "en": "End of line" } },
                          { "value": "next_line", "label": { "en": "Next line" } } ] },
                      "default": "end_of_line", "configKey": "f.brace_method", "moreConfigKeys": ["f.brace_constructor"] },
                    { "id": "spaceIf", "label": { "en": "if" }, "tab": "spaces", "kind": { "type": "boolean" },
                      "default": true, "configKey": "f.space_if",
                      "configValues": { "true": "insert", "false": "do not insert" } },
                    { "id": "lineSplit", "label": { "en": "Wrap at" }, "tab": "wrappingAndBraces",
                      "kind": { "type": "integer", "min": 10, "max": 500 }, "default": 120, "configKey": "f.line" },
                    { "id": "order", "label": { "en": "Order" }, "tab": "imports", "kind": { "type": "text" },
                      "default": "#;java;", "serverSetting": { "section": "java", "path": "importOrder", "separator": ";" } }
                ],
                "configFile": { "names": ["java-formatter.xml"], "format": "eclipseProfile",
                    "location": { "section": "java", "path": "format.settings.url" } }
            }"##,
        )
        .unwrap()
    }

    #[test]
    fn a_text_option_with_a_separator_is_sent_as_a_list() {
        let style = profile();
        assert_eq!(style.server_settings(&Values::new())["java"], json!({}), "nothing at the default");
        let given = values(&[("order", json!("#; com ;org;"))]);
        assert_eq!(style.server_settings(&given)["java"], json!({ "importOrder": ["#", "com", "org", ""] }));
    }

    #[test]
    fn an_eclipse_profile_is_written_with_every_declared_setting_and_read_back() {
        let style = profile();
        assert!(!style.differs_in_config(&Values::new()));
        let given = values(&[("braces", json!("next_line")), ("spaceIf", json!(false))]);
        assert!(style.differs_in_config(&given));
        let written = style.write_config("", &given);
        assert_eq!(
            written,
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n<profiles version=\"23\">\n\
             <profile kind=\"CodeFormatterProfile\" name=\"Zen IDE\" version=\"23\">\n\
             \x20   <setting id=\"f.brace_method\" value=\"next_line\"/>\n\
             \x20   <setting id=\"f.brace_constructor\" value=\"next_line\"/>\n\
             \x20   <setting id=\"f.space_if\" value=\"do not insert\"/>\n\
             \x20   <setting id=\"f.line\" value=\"120\"/>\n\
             </profile>\n</profiles>\n"
        );
        assert_eq!(
            style.read_config(&written),
            values(&[("braces", json!("next_line")), ("spaceIf", json!(false)), ("lineSplit", json!(120))])
        );
    }

    #[test]
    fn an_editorconfig_section_is_read_and_written_keeping_the_rest_of_the_file() {
        let style = CodeStyleDeclaration::parse(
            r#"{ "language": "kotlin", "title": "t", "options": [
                { "id": "trailing", "label": "l", "tab": "wrappingAndBraces", "kind": { "type": "boolean" },
                  "default": false, "configKey": "ij_kotlin_allow_trailing_comma" },
                { "id": "stars", "label": "l", "tab": "imports", "kind": { "type": "integer", "min": 1, "max": 99 },
                  "default": 5, "configKey": "ij_kotlin_name_count_to_use_star_import" } ],
              "configFile": { "names": [".editorconfig"], "format": "editorConfig", "section": "*.{kt,kts}" } }"#,
        )
        .unwrap();
        let text = "root = true\n\n[*]\nij_kotlin_allow_trailing_comma = true\n\n[*.{kt,kts}]\nindent_size = 4\nij_kotlin_name_count_to_use_star_import = 7\n\n[*.md]\nmax_line_length = off\n";
        assert_eq!(style.read_config(text), values(&[("stars", json!(7))]), "only the declared section counts");
        let written = style.write_config(text, &values(&[("stars", json!(9)), ("trailing", json!(true))]));
        assert_eq!(
            written,
            "root = true\n\n[*]\nij_kotlin_allow_trailing_comma = true\n\n[*.{kt,kts}]\nindent_size = 4\nij_kotlin_name_count_to_use_star_import = 9\nij_kotlin_allow_trailing_comma = true\n\n[*.md]\nmax_line_length = off\n"
        );
        assert_eq!(
            style.write_config("root = true\n", &values(&[("trailing", json!(true))])),
            "root = true\n\n[*.{kt,kts}]\nij_kotlin_allow_trailing_comma = true\n"
        );
        assert_eq!(style.write_config("root = true\n", &values(&[("trailing", json!(false))])), "root = true\n");
    }

    #[test]
    fn the_file_gives_only_the_options_it_keeps() {
        let style = profile();
        let base = values(&[("order", json!("java;")), ("braces", json!("next_line"))]);
        let merged =
            style.with_config_values(&base, &values(&[("braces", json!("end_of_line")), ("order", json!("x;"))]));
        assert_eq!((merged["braces"].clone(), merged["order"].clone()), (json!("end_of_line"), json!("java;")));
    }

    #[test]
    fn an_eclipse_profile_keeps_the_settings_it_does_not_declare() {
        let style = profile();
        let text = "<?xml version=\"1.0\"?>\n<profiles version=\"13\">\n<profile kind=\"CodeFormatterProfile\" name=\"Team\" version=\"13\">\n<setting id=\"f.other\" value=\"x &amp; y\"/>\n<setting value=\"80\" id=\"f.line\"/>\n</profile>\n</profiles>\n";
        assert_eq!(style.read_config(text), values(&[("lineSplit", json!(80))]));
        let written = style.write_config(text, &values(&[("spaceIf", json!(false))]));
        assert!(
            written.contains("name=\"Team\"") && written.contains("<setting id=\"f.other\" value=\"x &amp; y\"/>"),
            "{written}"
        );
        assert!(written.contains("<setting id=\"f.line\" value=\"80\"/>"), "the file's own value stays: {written}");
        assert!(written.contains("<setting id=\"f.space_if\" value=\"do not insert\"/>"), "{written}");
        assert!(written.contains("<setting id=\"f.brace_constructor\" value=\"end_of_line\"/>"), "{written}");
        assert_eq!(written.matches("f.line").count(), 1);
    }

    #[test]
    fn the_config_file_is_read_and_written_keeping_what_it_does_not_declare() {
        let style = declaration();
        let text = "# ours\nedition = \"2024\"\nmax_width = 120 # wide\nnewline_style = \"Unix\"\n\n[ignore]\nhard_tabs = true\n";
        let read = style.read_config(text);
        assert_eq!(read, values(&[("max_width", json!(120)), ("newline_style", json!("Unix"))]));
        let written = style.write_config(text, &values(&[("max_width", json!(90)), ("hard_tabs", json!(true))]));
        assert_eq!(
            written,
            "# ours\nedition = \"2024\"\nmax_width = 90\nnewline_style = \"Unix\"\nhard_tabs = true\n\n[ignore]\nhard_tabs = true\n"
        );
        assert_eq!(style.write_config("", &values(&[("hard_tabs", json!(false))])), "", "a default is not added");
        assert_eq!(style.write_config("", &values(&[("newline_style", json!("Unix"))])), "newline_style = \"Unix\"\n");
    }
}

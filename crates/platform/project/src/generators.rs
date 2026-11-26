use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::sync::atomic::AtomicBool;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_vfs::zip::{Archive, Limits};

pub type Values = BTreeMap<String, Value>;

pub const NAME_PLACEHOLDER: &str = "${name}";

const JDK_RELEASE_SUFFIX: &str = "Release";

pub fn jdk_release_key(field: &str) -> String {
    format!("{field}{JDK_RELEASE_SUFFIX}")
}

/// Where a project type stands on the left of the New Project wizard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GeneratorSection {
    /// The project types of the IDE and its language modules, at the top.
    #[default]
    Types,
    /// The types plug-ins add, under the heading Generators.
    Generators,
}

/// A project type the New Project wizard offers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorInfo {
    /// Its id, for the `project.new.*` commands.
    pub id: String,
    /// Its name in the interface's language.
    pub name: String,
    /// The name of a file whose icon stands for the type, e.g. `main.go`.
    pub icon: String,
    /// Where it stands in the wizard.
    pub section: GeneratorSection,
    /// Who gives it: the plug-in's id, or nothing for the IDE and its modules.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub plugin: String,
    /// A line above Name and Location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether it needs the network, which the wizard says.
    pub network: bool,
    /// Its settings, in System Settings under Project Generators.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<GeneratorSetting>,
    /// The fields whose values change the form; the wizard loads it again when they do.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub form_depends_on: Vec<String>,
    /// The build tool it runs to generate; an agent cannot run one (ADR-0205, ADR-0244).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The address selecting it asks; the wizard shows it first and asks only when told to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// A text setting of a project type, shown in System Settings under Project Generators.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorSetting {
    /// Its key in `generatorSettings`: the type's id, a dot and the setting's id.
    pub key: String,
    /// Its label.
    pub label: String,
    /// A note under it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Its value until the person changes it.
    pub default: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratorEnvironment {
    pub network: bool,
    pub settings: BTreeMap<String, String>,
}

impl Default for GeneratorEnvironment {
    fn default() -> Self {
        Self { network: true, settings: BTreeMap::new() }
    }
}

/// The fields of a project type, shown under Name and Location.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorForm {
    /// The fields, from top to bottom.
    pub fields: Vec<Field>,
    /// A line above the fields, for example that they come from saved data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// The service that gives the form, shown above Name with the way to change it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<FormServer>,
    /// Why Create cannot run with the values given, shown in its place.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
}

/// The service behind a form.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FormServer {
    /// Its label.
    pub label: String,
    /// Its web address.
    pub url: String,
}

/// One field of a project type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// Its id, the key of its value.
    pub id: String,
    /// Its label.
    pub label: String,
    /// What it edits.
    #[serde(flatten)]
    pub kind: FieldKind,
    /// Whether Create waits for a value.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// A note under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Shown, and its value used, only while this holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<Condition>,
    /// Shown under Advanced Settings, folded until the person opens them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub advanced: bool,
    /// A choice drawn as a row of buttons, not a drop-down list.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub segmented: bool,
    /// The wizard's page it stands on: 0 beside the list of types, and Next leads to the others.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub page: u8,
    /// What the question mark beside it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// A list the IDE keeps for this field, edited from a link beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manage: Option<ManagedList>,
}

fn is_zero(page: &u8) -> bool {
    *page == 0
}

/// A list of rows the IDE keeps for a project type and gives its form under `lists`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ManagedList {
    /// Its id within the project type.
    pub id: String,
    /// The link's text.
    pub label: String,
    /// The title of its dialog.
    pub title: String,
    /// The columns of a row.
    pub columns: Vec<Column>,
    /// The dialog adds one row instead of showing the list.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub add_one: bool,
    /// The field's value after a row is added; `${column}` stands for a column's value.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub value: String,
}

/// One column of a table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Column {
    /// Its id, the key of its value in a row.
    pub id: String,
    /// Its title.
    pub label: String,
}

/// A link in a note that selects another project type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteLink {
    /// Its text.
    pub label: String,
    /// The project type it selects.
    pub generator: String,
}

/// What a field edits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FieldKind {
    /// One line of text; in the default `${name}` stands for the project's name and `${id}` for the
    /// value of a field above it.
    Text {
        /// Its value until the person changes it.
        #[serde(default)]
        default: String,
    },
    /// A path, with Browse.
    Path {
        /// Its value until the person changes it.
        #[serde(default)]
        default: String,
    },
    /// One of a few options, a drop-down list.
    Choice {
        /// The options.
        options: Vec<FieldOption>,
        /// The value chosen until the person changes it; the first enabled option without it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    /// A check box.
    Check {
        /// Whether it is ticked until the person changes it.
        #[serde(default)]
        default: bool,
    },
    /// Any number of options in groups, with a search field; its value is a list.
    Select {
        /// The groups, each with its options.
        groups: Vec<OptionGroup>,
        /// The values ticked until the person changes them.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        default: Vec<String>,
    },
    /// A JDK of this computer, with Download; its value is the JDK's home, and the generator also
    /// gets `<id>Release`, its feature release.
    Jdk {
        /// The JDKs found, filled in by the IDE: a generator leaves them out.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<FieldOption>,
        /// The IDE's default JDK.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    /// Text the person reads, with an optional link between its two parts; it has no value.
    Note {
        /// The text before the link.
        #[serde(default)]
        before: String,
        /// The link.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<NoteLink>,
        /// The text after the link.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        after: String,
    },
    /// Rows the person adds and removes; its value is a list of objects by column id.
    Table {
        /// The columns.
        columns: Vec<Column>,
        /// What it shows without rows.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        empty: String,
    },
}

/// One option of a choice or a multi-select.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FieldOption {
    /// The value it gives.
    pub value: String,
    /// What the list shows.
    pub label: String,
    /// A line under the label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A web page about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// It can be chosen only while this holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_when: Option<Condition>,
    /// Why it cannot be chosen, shown while `enabledWhen` does not hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_note: Option<String>,
}

/// A named group of options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OptionGroup {
    /// Its title.
    pub name: String,
    /// Its options.
    pub options: Vec<FieldOption>,
}

/// A rule on another field's value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Condition {
    /// The other field's id.
    pub field: String,
    /// The values for which it holds; `true` and `false` for a check box.
    pub any_of: Vec<String>,
}

impl Condition {
    fn holds(&self, values: &Values) -> bool {
        let text = match values.get(&self.field) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Bool(on)) => on.to_string(),
            _ => return false,
        };
        self.any_of.contains(&text)
    }
}

impl FieldOption {
    pub fn web_link(&self) -> Option<&str> {
        self.url.as_deref().filter(|url| url.starts_with("https://") || url.starts_with("http://"))
    }

    pub fn enabled(&self, values: &Values) -> bool {
        self.enabled_when.as_ref().is_none_or(|condition| condition.holds(values))
    }
}

impl Field {
    pub fn visible(&self, values: &Values) -> bool {
        self.visible_when.as_ref().is_none_or(|condition| condition.holds(values))
    }

    fn default_value(&self, name: &str, chosen: &Values) -> Value {
        match &self.kind {
            FieldKind::Text { default } | FieldKind::Path { default } => {
                let text = default.replace(NAME_PLACEHOLDER, name);
                let filled = chosen.iter().fold(text, |text, (id, value)| match value {
                    Value::String(value) => text.replace(&format!("${{{id}}}"), value),
                    _ => text,
                });
                Value::String(filled)
            }
            FieldKind::Check { default } => Value::Bool(*default),
            FieldKind::Select { default, .. } => Value::from(default.clone()),
            FieldKind::Note { .. } => Value::Null,
            FieldKind::Table { .. } => Value::Array(Vec::new()),
            FieldKind::Choice { options, default } | FieldKind::Jdk { options, default } => {
                let enabled = |option: &&FieldOption| option.enabled(chosen);
                let first = options.iter().find(enabled).map(|option| option.value.clone());
                let wanted =
                    default.clone().filter(|value| options.iter().filter(enabled).any(|option| &option.value == value));
                wanted.or(first).map_or(Value::Null, Value::String)
            }
        }
    }

    fn enabled_ticks(&self, values: &Values) -> Option<(String, Value)> {
        let FieldKind::Select { groups, .. } = &self.kind else { return None };
        let ticked = values.get(&self.id)?.as_array()?;
        let options = groups.iter().flat_map(|group| &group.options);
        let usable = |item: &&Value| {
            let option = options.clone().find(|option| Some(option.value.as_str()) == item.as_str());
            option.is_none_or(|option| option.enabled(values))
        };
        let kept = ticked.iter().filter(usable).cloned().collect();
        Some((self.id.clone(), Value::Array(kept)))
    }

    fn is_empty(value: &Value) -> bool {
        match value {
            Value::Null => true,
            Value::String(text) => text.trim().is_empty(),
            Value::Array(items) => items.is_empty(),
            _ => false,
        }
    }
}

impl GeneratorForm {
    pub fn last_page(&self) -> u8 {
        self.fields.iter().map(|field| field.page).max().unwrap_or(0)
    }

    pub fn resolved(&self, name: &str, given: &Values) -> Values {
        let mut values = given.clone();
        for field in &self.fields {
            let keeps = values.get(&field.id).is_some_and(|value| match &field.kind {
                FieldKind::Choice { options, .. } | FieldKind::Jdk { options, .. } => {
                    options.iter().any(|option| Some(option.value.as_str()) == value.as_str() && option.enabled(given))
                }
                _ => true,
            });
            if !keeps {
                let value = field.default_value(name, &values);
                values.insert(field.id.clone(), value);
            }
        }
        let kept: Vec<_> = self.fields.iter().filter_map(|field| field.enabled_ticks(&values)).collect();
        values.extend(kept);
        for field in &self.fields {
            if !field.visible(&values) {
                values.remove(&field.id);
            }
        }
        values
    }

    pub fn missing(&self, values: &Values) -> Option<&Field> {
        self.fields
            .iter()
            .filter(|field| field.required && field.visible(values))
            .find(|field| values.get(&field.id).is_none_or(Field::is_empty))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneratorRequest {
    pub name: String,
    pub values: Values,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolRun {
    pub tool: String,
    pub args: Vec<String>,
    pub java_home: Option<String>,
    pub subfolder: Option<String>,
}

pub trait ToolRunner: Send + Sync {
    fn run(&self, run: &ToolRun, folder: &Path, stop: &AtomicBool, output: &dyn Fn(&str)) -> Result<(), String>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleEntry {
    pub file: String,
    pub text: String,
    pub path: String,
    pub project: String,
    pub has_build: bool,
    pub must_build: bool,
}

pub trait ModuleBuild: Send + Sync {
    fn plugin(&self) -> String;

    fn module_files(&self) -> Vec<String>;

    fn reload(&self) -> String;

    fn add_module(&self, entry: &ModuleEntry) -> Result<Vec<GeneratedFile>, String>;
}

impl GeneratedFile {
    pub fn text(path: &str, text: impl Into<String>) -> Self {
        Self { path: path.to_owned(), bytes: text.into().into_bytes(), executable: false }
    }

    pub fn is_folder(&self) -> bool {
        self.path.ends_with('/')
    }
}

const ZIP_LIMITS: Limits = Limits { archive: 16 << 20, entries: 4096, entry: 16 << 20, total: 64 << 20 };

fn shared_root(files: &[GeneratedFile]) -> Option<String> {
    let root = files.first()?.path.split_once('/')?.0;
    let prefix = format!("{root}/");
    files.iter().all(|file| file.path.starts_with(&prefix)).then_some(prefix)
}

pub fn unzipped(bytes: Vec<u8>) -> Result<Vec<GeneratedFile>, String> {
    let archive = Archive::parse(bytes, ZIP_LIMITS).map_err(|error| error.to_string())?;
    let mut files = Vec::with_capacity(archive.entries().len());
    for entry in archive.entries() {
        if entry.link {
            return Err(format!("{}: the archive holds a link", entry.name));
        }
        if entry.folder {
            files.push(GeneratedFile { path: format!("{}/", entry.name), bytes: Vec::new(), executable: false });
            continue;
        }
        let bytes = archive.read(entry).map_err(|error| error.to_string())?;
        files.push(GeneratedFile { path: entry.name.clone(), bytes, executable: entry.executable });
    }
    if let Some(root) = shared_root(&files) {
        files.retain(|file| file.path != root);
        for file in &mut files {
            file.path.drain(..root.len());
        }
    }
    Ok(files)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepAnswer {
    pub answer: Value,
    pub from_saved: bool,
}

pub trait ProjectGenerator: Send + Sync {
    fn info(&self) -> GeneratorInfo;

    fn form(&self) -> Result<GeneratorForm, String>;

    fn files(&self, request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String>;

    fn generate(&self, request: &GeneratorRequest, folder: &Path, _stop: &AtomicBool) -> Result<(), String> {
        write_files(folder, &self.files(request)?)
    }

    fn step(&self, step: &str, _values: Values) -> Result<StepAnswer, String> {
        Err(format!("{}: no step {step}", self.info().id))
    }

    fn form_with(&self, _values: &Values) -> Result<GeneratorForm, String> {
        self.form()
    }

    fn generate_showing(
        &self,
        request: &GeneratorRequest,
        folder: &Path,
        stop: &AtomicBool,
        _output: &dyn Fn(&str),
    ) -> Result<(), String> {
        self.generate(request, folder, stop)
    }
}

pub fn hoist(folder: &Path, subfolder: &str) -> Result<(), String> {
    if !is_inside(subfolder) {
        return Err(format!("{subfolder}: the path leaves the project"));
    }
    let moved = folder.join(".zen-hoist");
    let describe = |error: std::io::Error| format!("{subfolder}: {error}");
    std::fs::rename(folder.join(subfolder), &moved).map_err(describe)?;
    for entry in std::fs::read_dir(&moved).map_err(describe)? {
        let entry = entry.map_err(describe)?;
        std::fs::rename(entry.path(), folder.join(entry.file_name())).map_err(describe)?;
    }
    std::fs::remove_dir(&moved).map_err(describe)
}

fn in_git_folder(path: &str) -> bool {
    path.split('/').any(|part| part.eq_ignore_ascii_case(".git"))
}

pub fn write_files(folder: &Path, files: &[GeneratedFile]) -> Result<(), String> {
    for file in files {
        if !is_inside(&file.path) {
            return Err(format!("{}: the path leaves the project", file.path));
        }
        if in_git_folder(&file.path) {
            return Err(format!("{}: a generator writes nothing into Git's folder", file.path));
        }
        let path = folder.join(&file.path);
        if file.is_folder() {
            std::fs::create_dir_all(&path).map_err(|error| format!("{}: {error}", file.path))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", file.path))?;
        }
        std::fs::write(&path, &file.bytes).map_err(|error| format!("{}: {error}", file.path))?;
        #[cfg(unix)]
        if file.executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|error| format!("{}: {error}", file.path))?;
        }
    }
    Ok(())
}

fn is_inside(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && Path::new(path).components().all(|component| matches!(component, Component::Normal(_)))
}

/// A project generator a plug-in declares in its `plugin.json`. Its name and the texts of its fields
/// are strings or objects by language (`{"en": "…", "ru": "…"}`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorDeclaration {
    /// Its id within the plug-in.
    pub id: String,
    /// Its name.
    pub name: Value,
    /// The name of a file whose icon stands for the type, e.g. `package.json`.
    pub icon: String,
    /// The fields, as `GeneratorForm`'s; none when the module gives the form.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Value>,
    /// Whether the module's `zen_project` export gives the form instead (a form that needs the network).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dynamic_form: bool,
    /// Whether the module asks the IDE to fetch web addresses for it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub network: bool,
    /// The fields whose values the module's form depends on, given to it as `values`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub form_depends_on: Vec<String>,
    /// The build tool the module may ask the IDE to run in its `generate` step, e.g. `maven`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The id of the setting that holds the address its form asks; the wizard shows it first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Whether it is a language's own type, listed above Empty Project and not under Generators.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub language: bool,
    /// Its text settings: each an `id`, `label`, optional `help` and `default`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Value>,
}

const TEXT_KEYS: [&str; 10] =
    ["name", "label", "description", "help", "disabledNote", "title", "tooltip", "before", "after", "empty"];

fn in_language(text: &Value, language: &str) -> Option<Value> {
    let texts = text.as_object()?;
    Some(texts.get(language).or_else(|| texts.get("en")).cloned().unwrap_or_else(|| Value::String(String::new())))
}

fn localized(value: &Value, language: &str) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|item| localized(item, language)).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let text = TEXT_KEYS.contains(&key.as_str()).then(|| in_language(value, language)).flatten();
                    (key.clone(), text.unwrap_or_else(|| localized(value, language)))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn localized_form(value: &Value, language: &str) -> Result<GeneratorForm, String> {
    serde_json::from_value(localized(value, language)).map_err(|error| format!("the form is invalid: {error}"))
}

impl GeneratorDeclaration {
    pub fn parse_list(json: &str) -> Result<Vec<Self>, String> {
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        let list: Vec<Self> =
            serde_json::from_str(json).map_err(|error| format!("the project generators are invalid: {error}"))?;
        for generator in &list {
            if generator.id.is_empty() {
                return Err("a project generator has no id".to_owned());
            }
            generator.form("en")?;
        }
        Ok(list)
    }

    pub fn display_name(&self, language: &str) -> String {
        let name = in_language(&self.name, language).unwrap_or_else(|| self.name.clone());
        name.as_str().unwrap_or(&self.id).to_owned()
    }

    pub fn form(&self, language: &str) -> Result<GeneratorForm, String> {
        localized_form(&serde_json::json!({ "fields": self.fields }), language)
    }

    pub fn resolved_settings(&self, language: &str, owner: &str) -> Vec<GeneratorSetting> {
        let setting = |value: &Value| {
            let mut setting: GeneratorSetting = serde_json::from_value(localized(value, language)).ok()?;
            let id = value.get("id")?.as_str()?;
            setting.key = format!("{owner}.{id}");
            Some(setting)
        };
        self.settings.iter().filter_map(setting).collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_vfs::zip::ZipWriter;

    #[test]
    fn a_link_a_service_gives_opens_only_as_a_web_page() {
        let option = |url: &str| FieldOption { url: Some(url.to_owned()), ..FieldOption::default() };
        assert_eq!(option("https://spring.io/guides").web_link(), Some("https://spring.io/guides"));
        for url in ["file:///etc/passwd", "javascript:alert(1)", "smb://host/share", "x-app://run"] {
            assert_eq!(option(url).web_link(), None, "{url}");
        }
    }

    #[test]
    fn a_generator_writes_nothing_into_git_s_folder() {
        let dir = tempfile::tempdir().unwrap();
        for path in [".git/hooks/post-commit", "sub/.GIT/config"] {
            let refused = write_files(dir.path(), &[GeneratedFile::text(path, "x")]).unwrap_err();
            assert!(refused.contains("Git's folder"), "{refused}");
        }
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
        write_files(dir.path(), &[GeneratedFile::text(".gitignore", "x")]).unwrap();
    }

    use super::*;

    fn form() -> GeneratorForm {
        serde_json::from_value(json!({ "fields": [
            { "id": "language", "label": "Language", "type": "choice", "default": "kotlin", "options": [
                { "value": "java", "label": "Java" }, { "value": "kotlin", "label": "Kotlin" }] },
            { "id": "jvm", "label": "JVM", "type": "text", "default": "${name}-jvm",
              "visibleWhen": { "field": "language", "anyOf": ["kotlin"] } },
            { "id": "group", "label": "Group", "type": "text", "required": true },
            { "id": "sample", "label": "Sample", "type": "check", "default": true },
            { "id": "starters", "label": "Starters", "type": "select", "default": ["web"], "groups": [
                { "name": "Web", "options": [{ "value": "web", "label": "Web" }] }] },
        ] }))
        .unwrap()
    }

    #[test]
    fn defaults_fill_the_values_the_person_left_alone_and_follow_the_project_s_name() {
        let values = form().resolved("shop", &Values::new());
        assert_eq!(values["language"], "kotlin");
        assert_eq!(values["jvm"], "shop-jvm");
        assert_eq!(values["sample"], true);
        assert_eq!(values["starters"], json!(["web"]));
    }

    #[test]
    fn a_field_depending_on_another_goes_with_its_value() {
        let values = form().resolved("shop", &Values::from([("language".to_owned(), json!("java"))]));
        assert!(!values.contains_key("jvm"), "{values:?}");
    }

    #[test]
    fn a_choice_that_became_disabled_falls_back_to_an_enabled_option() {
        let form: GeneratorForm = serde_json::from_value(json!({ "fields": [
            { "id": "boot", "label": "Boot", "type": "choice", "options": [
                { "value": "3", "label": "3" }, { "value": "4", "label": "4" }] },
            { "id": "java", "label": "Java", "type": "choice", "options": [
                { "value": "8", "label": "8", "enabledWhen": { "field": "boot", "anyOf": ["3"] } },
                { "value": "21", "label": "21" }] },
        ] }))
        .unwrap();
        let chosen = Values::from([("boot".to_owned(), json!("4")), ("java".to_owned(), json!("8"))]);
        assert_eq!(form.resolved("x", &chosen)["java"], "21");
    }

    #[test]
    fn a_required_field_without_a_value_is_named() {
        let form = form();
        let values = form.resolved("shop", &Values::new());
        assert_eq!(form.missing(&values).map(|field| field.id.as_str()), Some("group"));
        let filled = form.resolved("shop", &Values::from([("group".to_owned(), json!("com.acme"))]));
        assert_eq!(form.missing(&filled), None);
    }

    #[test]
    fn a_jdk_field_starts_on_the_default_and_keeps_only_a_jdk_it_lists() {
        let form: GeneratorForm = serde_json::from_value(json!({ "fields": [
            { "id": "jdk", "label": "JDK", "type": "jdk", "required": true,
              "options": [{ "value": "/jdk/17", "label": "17" }, { "value": "/jdk/21", "label": "21" }],
              "default": "/jdk/21" },
        ] }))
        .unwrap();
        assert_eq!(form.resolved("x", &Values::new())["jdk"], "/jdk/21");
        let given = Values::from([("jdk".to_owned(), json!("/jdk/17"))]);
        assert_eq!(form.resolved("x", &given)["jdk"], "/jdk/17");
        let gone = Values::from([("jdk".to_owned(), json!("/jdk/8"))]);
        assert_eq!(form.resolved("x", &gone)["jdk"], "/jdk/21");
    }

    #[test]
    fn a_jdk_field_a_manifest_declares_has_no_value_until_the_ide_fills_it_in() {
        let form: GeneratorForm = serde_json::from_value(json!({ "fields": [
            { "id": "jdk", "label": "JDK", "type": "jdk", "required": true },
        ] }))
        .unwrap();
        let values = form.resolved("x", &Values::new());
        assert_eq!(form.missing(&values).map(|field| field.id.as_str()), Some("jdk"));
    }

    #[test]
    fn a_note_has_no_value_and_a_table_starts_without_rows() {
        let form: GeneratorForm = serde_json::from_value(json!({ "fields": [
            { "id": "about", "label": "", "type": "note", "before": "Go to the ",
              "link": { "label": "Java", "generator": "java.java" }, "after": " page." },
            { "id": "properties", "label": "Additional Properties", "type": "table",
              "columns": [{ "id": "name", "label": "Name" }, { "id": "value", "label": "Value" }] },
        ] }))
        .unwrap();
        let values = form.resolved("x", &Values::new());
        assert_eq!(values["about"], Value::Null);
        assert_eq!(values["properties"], json!([]));
        assert_eq!(form.missing(&values), None);
    }

    #[test]
    fn a_manifest_s_note_table_and_managed_list_read_in_the_language() {
        let form = json!({ "fields": [
            { "id": "catalog", "label": { "en": "Catalog:", "ru": "Каталог:" }, "type": "choice", "options": [],
              "manage": { "id": "catalogs", "label": { "en": "Manage", "ru": "Управлять" },
                          "title": { "en": "Catalogs", "ru": "Каталоги" },
                          "columns": [{ "id": "url", "label": { "en": "URL", "ru": "Адрес" } }] } },
            { "id": "about", "label": "", "type": "note", "before": { "en": "Go", "ru": "Иди" },
              "link": { "label": { "en": "Java", "ru": "Джава" }, "generator": "java.java" } },
            { "id": "rows", "label": "", "type": "table", "columns": [], "empty": { "en": "None", "ru": "Нет" } },
        ] });
        let form = localized_form(&form, "ru").unwrap();
        let manage = form.fields[0].manage.as_ref().unwrap();
        assert_eq!((manage.label.as_str(), manage.columns[0].label.as_str()), ("Управлять", "Адрес"));
        let FieldKind::Note { before, link, .. } = &form.fields[1].kind else { panic!("{:?}", form.fields[1]) };
        assert_eq!((before.as_str(), link.as_ref().unwrap().label.as_str()), ("Иди", "Джава"));
        let FieldKind::Table { empty, .. } = &form.fields[2].kind else { panic!("{:?}", form.fields[2]) };
        assert_eq!(empty, "Нет");
    }

    #[test]
    fn hoisting_moves_a_subfolder_s_files_up_also_one_with_the_same_name() {
        let folder = tempfile::tempdir().unwrap();
        write_files(
            folder.path(),
            &[GeneratedFile::text("shop/pom.xml", "x"), GeneratedFile::text("shop/shop/a", "y")],
        )
        .unwrap();
        hoist(folder.path(), "shop").unwrap();
        assert_eq!(std::fs::read_to_string(folder.path().join("pom.xml")).unwrap(), "x");
        assert_eq!(std::fs::read_to_string(folder.path().join("shop/a")).unwrap(), "y");
        assert!(hoist(folder.path(), "../up").is_err());
    }

    #[test]
    fn files_leaving_the_project_are_refused() {
        let folder = tempfile::tempdir().unwrap();
        for path in ["../up.txt", "/etc/hosts", "a/../../b", "", "a\\b"] {
            let refused = write_files(folder.path(), &[GeneratedFile::text(path, "x")]);
            assert!(refused.is_err(), "{path}");
        }
        write_files(folder.path(), &[GeneratedFile::text("src/a.txt", "x")]).unwrap();
        assert_eq!(std::fs::read_to_string(folder.path().join("src/a.txt")).unwrap(), "x");
    }

    #[test]
    fn a_declaration_reads_its_texts_in_the_language_and_falls_back_to_english() {
        let list = GeneratorDeclaration::parse_list(
            &json!([{ "id": "vite", "name": { "en": "Vite", "ru": "Вайт" }, "icon": "package.json", "fields": [
                { "id": "ts", "label": { "en": "TypeScript", "ru": "ТайпСкрипт" }, "type": "check" },
                { "id": "out", "label": { "en": "Output" }, "type": "path" },
            ] }])
            .to_string(),
        )
        .unwrap();
        assert_eq!(list[0].display_name("ru"), "Вайт");
        let form = list[0].form("ru").unwrap();
        assert_eq!(form.fields[0].label, "ТайпСкрипт");
        assert_eq!(form.fields[1].label, "Output");
    }

    #[test]
    fn a_declaration_with_an_unknown_field_type_is_refused() {
        let list = json!([{ "id": "x", "name": "X", "fields": [{ "id": "a", "label": "A", "type": "dial" }] }]);
        assert!(GeneratorDeclaration::parse_list(&list.to_string()).is_err());
        assert!(GeneratorDeclaration::parse_list(r#"[{ "name": "X" }]"#).is_err(), "no id");
    }

    fn zipped(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(1_792_000_000);
        for (name, data) in files {
            writer.add(name, data).unwrap();
        }
        writer.finish()
    }

    fn with_bytes_replaced(mut archive: Vec<u8>, from: &[u8], to: &[u8]) -> Vec<u8> {
        let places: Vec<usize> =
            archive.windows(from.len()).enumerate().filter(|(_, bytes)| *bytes == from).map(|(at, _)| at).collect();
        assert!(!places.is_empty());
        for at in places {
            archive[at..at + to.len()].copy_from_slice(to);
        }
        archive
    }

    #[test]
    fn a_zip_without_a_root_folder_unpacks_as_it_is() {
        let files = unzipped(zipped(&[("pom.xml", b"<project/>"), ("src/App.java", b"class App {}")])).unwrap();
        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["pom.xml", "src/App.java"]);
        assert_eq!(files[0].bytes, b"<project/>");
    }

    #[test]
    fn a_zip_with_one_root_folder_loses_it() {
        let files = unzipped(zipped(&[("demo/pom.xml", b"a"), ("demo/src/App.java", b"b")])).unwrap();
        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["pom.xml", "src/App.java"]);
    }

    #[test]
    fn a_zip_leaving_the_project_is_refused() {
        let parent = with_bytes_replaced(zipped(&[("aa/evil", b"x")]), b"aa/evil", b"../evil");
        assert!(unzipped(parent).unwrap_err().contains("not a safe relative path"));
        let inner = with_bytes_replaced(zipped(&[("src/aa/evil", b"x")]), b"src/aa/evil", b"src/../evil");
        assert!(unzipped(inner).is_err());
        let absolute = with_bytes_replaced(zipped(&[("xtmp/evil", b"x")]), b"xtmp/evil", b"/tmp/evil");
        assert!(unzipped(absolute).unwrap_err().contains("not a safe relative path"));
    }

    fn with_mode(archive: Vec<u8>, name: &str, mode: u32) -> Vec<u8> {
        let mut archive = archive;
        let central = archive.windows(4).position(|bytes| bytes == [0x50, 0x4b, 1, 2]).unwrap();
        let mut at = central;
        loop {
            let length = usize::from(u16::from_le_bytes([archive[at + 28], archive[at + 29]]));
            let extra = usize::from(u16::from_le_bytes([archive[at + 30], archive[at + 31]]));
            if &archive[at + 46..at + 46 + length] == name.as_bytes() {
                archive[at + 5] = 3;
                archive[at + 38..at + 42].copy_from_slice(&(mode << 16).to_le_bytes());
                return archive;
            }
            at += 46 + length + extra;
        }
    }

    #[test]
    fn scripts_stay_executable_and_links_are_refused() {
        let archive = zipped(&[("mvnw", b"#!/bin/sh"), ("pom.xml", b"a")]);
        let files = unzipped(with_mode(archive.clone(), "mvnw", 0o100_755)).unwrap();
        assert!(files[0].executable);
        assert!(!files[1].executable);
        assert!(unzipped(with_mode(archive, "mvnw", 0o120_777)).unwrap_err().contains("link"));
    }

    #[test]
    fn empty_folders_of_a_zip_are_written_too() {
        let archive = zipped(&[("pom.xml", b"a"), ("src/staticx", b"")]);
        let files = unzipped(with_bytes_replaced(archive, b"src/staticx", b"src/static/")).unwrap();
        assert!(files[1].is_folder());
        let folder = tempfile::tempdir().unwrap();
        write_files(folder.path(), &files).unwrap();
        assert!(folder.path().join("src/static").is_dir());
        assert!(folder.path().join("pom.xml").is_file());
    }

    #[test]
    fn a_text_default_follows_the_fields_above_it_until_it_is_changed() {
        let form: GeneratorForm = serde_json::from_value(json!({ "fields": [
            { "id": "group", "label": "Group", "type": "text", "default": "com.example" },
            { "id": "artifact", "label": "Artifact", "type": "text", "default": "${name}" },
            { "id": "package", "label": "Package", "type": "text", "default": "${group}.${artifact}" },
        ] }))
        .unwrap();
        let mut given = Values::new();
        given.insert("group".to_owned(), json!("org.acme"));
        assert_eq!(form.resolved("shop", &given)["package"], "org.acme.shop");
        given.insert("package".to_owned(), json!("my.own"));
        assert_eq!(form.resolved("shop", &given)["package"], "my.own");
    }

    #[test]
    fn a_ticked_option_that_became_disabled_leaves_the_values_and_comes_back() {
        let form: GeneratorForm = serde_json::from_value(json!({ "fields": [
            { "id": "boot", "label": "Boot", "type": "choice", "default": "4", "options": [
                { "value": "3", "label": "3" }, { "value": "4", "label": "4" }] },
            { "id": "starters", "label": "Starters", "type": "select", "groups": [{ "name": "Web", "options": [
                { "value": "web", "label": "Web" },
                { "value": "old", "label": "Old", "enabledWhen": { "field": "boot", "anyOf": ["3"] } }] }] },
        ] }))
        .unwrap();
        let mut given = Values::new();
        given.insert("starters".to_owned(), json!(["web", "old"]));
        assert_eq!(form.resolved("x", &given)["starters"], json!(["web"]), "4 is the default");
        given.insert("boot".to_owned(), json!("3"));
        assert_eq!(form.resolved("x", &given)["starters"], json!(["web", "old"]));
    }

    #[test]
    fn a_declaration_s_settings_read_in_the_language_with_the_owner_s_key() {
        let declaration: GeneratorDeclaration = serde_json::from_value(json!({
            "id": "app", "name": "App",
            "settings": [{ "id": "url", "label": { "en": "URL", "ru": "Адрес" }, "default": "https://x" }],
        }))
        .unwrap();
        let settings = declaration.resolved_settings("ru", "sample.app");
        assert_eq!(settings[0].key, "sample.app.url");
        assert_eq!(settings[0].label, "Адрес");
        assert_eq!(settings[0].default, "https://x");
    }
}

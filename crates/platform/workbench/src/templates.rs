pub mod doc_comments;
mod expansion;
pub mod expressions;
pub mod postfix;
mod session;
pub mod statement;
pub mod surround;

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::{HighlightKind, Highlighter};

pub use doc_comments::DocComment;
pub use expansion::{Around, END, Expanded, SELECTION, expand, expand_around, surrounds, values_of, variables_of};
pub use expressions::Place;
pub use postfix::PostfixTemplate;
pub use session::TemplateSession;
pub use statement::StatementRules;
pub use surround::Surrounder;

use crate::{Document, Workbench, code_style, project_settings};

pub const OTHER: &str = "other";

pub const FORMAT_VERSION: u32 = 1;

/// A live template.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    /// The language id of the files it is for, e.g. `go`; `other` for every file.
    pub language: String,
    /// What is typed before Tab, e.g. `for`.
    pub abbreviation: String,
    /// What it is for: a message id of the module that declares it, or the text itself.
    #[serde(default)]
    pub description: String,
    /// The text: `$NAME$` is a variable (every occurrence is filled in at once), `$END$` is where
    /// the caret goes at the end, `$SELECTION$` is the code it surrounds (Surround With), `$$` is a
    /// dollar sign; tabs that start a line are the file's indentation.
    pub text: String,
    /// The variables' default values, in the order Tab visits them; the others follow in the order they appear.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// The packages the text uses, imported into the file when it is put in, e.g. `fmt` in Go.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
    /// Where in a file it expands: `code`, `comment`, `string`; in code only when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<Context>,
    /// Whether it is on.
    #[serde(default = "enabled")]
    pub enabled: bool,
}

impl Default for Template {
    fn default() -> Self {
        Self {
            language: OTHER.to_owned(),
            abbreviation: String::new(),
            description: String::new(),
            text: String::new(),
            variables: Vec::new(),
            imports: Vec::new(),
            contexts: Vec::new(),
            enabled: true,
        }
    }
}

fn enabled() -> bool {
    true
}

/// A place in a file where a template can expand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Context {
    /// Code, outside strings and comments.
    Code,
    /// A comment.
    Comment,
    /// A string literal.
    String,
}

/// A variable of a template.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    /// The name, as between the dollar signs.
    pub name: String,
    /// The text it starts with when it has no expression or the expression gives nothing.
    #[serde(default)]
    pub default: String,
    /// What its value is computed from: a quoted text, another variable's name, or a function such as
    /// `capitalize(NAME)`, `camelCase(fileNameWithoutExtension())`, `date()`; one computed from another
    /// variable follows it while the template is filled in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expression: String,
    /// Tab passes it by when its value is not empty.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip_if_defined: bool,
}

impl Template {
    pub fn parse_list(json: &str) -> Result<Vec<Template>, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }

    pub fn default_of(&self, name: &str) -> &str {
        self.variables.iter().find(|variable| variable.name == name).map_or("", |variable| variable.default.as_str())
    }

    pub fn description_text(&self) -> String {
        if self.description.is_empty() { String::new() } else { zen_i18n::text(&self.description, &[]) }
    }

    pub fn applies_to(&self, language: &str) -> bool {
        self.language == language || self.language == OTHER
    }

    pub fn expands_in(&self, place: Context) -> bool {
        if self.contexts.is_empty() { place == Context::Code } else { self.contexts.contains(&place) }
    }

    fn same_key(&self, language: &str, abbreviation: &str) -> bool {
        self.language == language && self.abbreviation == abbreviation
    }

    fn validate(&self) -> Result<(), String> {
        if self.language.trim().is_empty() {
            return Err(t!("workbench-templates-invalid-language"));
        }
        if self.abbreviation.is_empty() || self.abbreviation.chars().any(char::is_whitespace) {
            return Err(t!("workbench-templates-invalid-abbreviation"));
        }
        if self.text.is_empty() {
            return Err(t!("workbench-templates-invalid-text"));
        }
        let name_ok = |name: &str| {
            let mut chars = name.chars();
            name != END
                && name != SELECTION
                && chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if let Some(bad) = self.variables.iter().find(|variable| !name_ok(&variable.name)) {
            return Err(t!("workbench-templates-invalid-variable", name = bad.name.as_str()));
        }
        for variable in &self.variables {
            expressions::check(&variable.expression).map_err(|error| {
                t!("workbench-templates-invalid-expression", name = variable.name.as_str(), detail = error)
            })?;
        }
        let import_ok = |import: &str| !import.is_empty() && !import.contains(|c: char| c.is_whitespace() || c == '"');
        if let Some(bad) = self.imports.iter().find(|import| !import_ok(import)) {
            return Err(t!("workbench-templates-invalid-import", name = bad.as_str()));
        }
        Ok(())
    }
}

/// A template as `editor.templates.list` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    /// The template as it is in use.
    pub template: Template,
    /// Its description in the interface's language.
    pub description_text: String,
    /// Whether a language module declares it.
    pub builtin: bool,
    /// Whether it is built in and the user changed it.
    pub modified: bool,
    /// The built-in template as declared, for a built-in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Template>,
    /// The plug-in that declares it: only switching it off is kept, duplicate it to change it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

pub type AddImport = fn(&str, &str) -> Option<(Range<usize>, String)>;

pub type HighlighterSource = Arc<dyn Fn(&str) -> Option<Arc<dyn Highlighter>> + Send + Sync>;

#[derive(Clone, Default)]
pub struct Builtin {
    pub language: String,
    pub templates: Vec<Template>,
    pub add_import: Option<AddImport>,
    pub surrounders: Vec<Surrounder>,
    pub postfix: Vec<PostfixTemplate>,
    pub doc_comments: Vec<DocComment>,
    pub statements: Vec<StatementRules>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginTemplates {
    pub plugin: String,
    pub templates: Vec<Template>,
    pub postfix: Vec<PostfixTemplate>,
    pub doc_comments: Vec<DocComment>,
    pub statements: Vec<StatementRules>,
    pub surrounders: Vec<Surrounder>,
}

fn localised(value: &serde_json::Value, field: &str, language: &str) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(texts) = value.get(field).and_then(serde_json::Value::as_object) {
        let base = language.split(['-', '_']).next().unwrap_or(language);
        let text = [language, base, "en"]
            .iter()
            .find_map(|code| texts.get(*code))
            .or_else(|| texts.values().next())
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        value[field] = serde_json::Value::String(text);
    }
    value
}

fn each<T: serde::de::DeserializeOwned>(values: &[serde_json::Value]) -> Vec<T> {
    values.iter().filter_map(|value| serde_json::from_value(value.clone()).ok()).collect()
}

fn each_in<T: serde::de::DeserializeOwned>(values: &[serde_json::Value], field: &str) -> Vec<T> {
    let language = zen_i18n::language();
    values.iter().filter_map(|value| serde_json::from_value(localised(value, field, &language)).ok()).collect()
}

impl PluginTemplates {
    pub fn parse(plugin: &str, declared: &[serde_json::Value], postfix: &[serde_json::Value]) -> Self {
        Self {
            plugin: plugin.to_owned(),
            templates: each_in::<Template>(declared, "description")
                .into_iter()
                .filter(|template| template.validate().is_ok())
                .collect(),
            postfix: each_in(postfix, "description"),
            doc_comments: Vec::new(),
            statements: Vec::new(),
            surrounders: Vec::new(),
        }
    }

    pub fn with_assists(mut self, doc_comments: &[serde_json::Value], statements: &[serde_json::Value]) -> Self {
        self.doc_comments = each(doc_comments);
        self.statements = each(statements);
        self
    }

    pub fn with_surrounders(mut self, surrounders: &[serde_json::Value]) -> Self {
        self.surrounders = each_in(surrounders, "title");
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    version: u32,
    templates: Vec<Template>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    postfix_off: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    postfix_disabled: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawSaved {
    version: u32,
    templates: Vec<serde_json::Value>,
    postfix_off: Vec<String>,
    postfix_disabled: bool,
}

struct Inner {
    file: Option<PathBuf>,
    builtin: Vec<Template>,
    plugins: Vec<PluginTemplates>,
    surrounders: Vec<Surrounder>,
    postfix: Vec<PostfixTemplate>,
    doc_comments: Vec<DocComment>,
    statements: Vec<StatementRules>,
    saved: Saved,
    generation: u64,
    importers: Vec<(String, AddImport)>,
    highlighters: Option<HighlighterSource>,
    notice: Option<String>,
}

enum Origin<'a> {
    Module(&'a Template),
    Plugin(&'a str, &'a Template),
    User,
}

impl Inner {
    fn plugin_templates(&self) -> impl Iterator<Item = (&str, &Template)> {
        self.plugins
            .iter()
            .flat_map(|set| set.templates.iter().map(move |template| (set.plugin.as_str(), template)))
            .filter(|(_, template)| {
                !self.builtin.iter().any(|builtin| builtin.same_key(&template.language, &template.abbreviation))
            })
    }

    fn templates(&self) -> Vec<(Template, Origin<'_>)> {
        let saved = |language: &str, abbreviation: &str| {
            self.saved.templates.iter().find(|user| user.same_key(language, abbreviation))
        };
        let mut all: Vec<(Template, Origin<'_>)> = Vec::new();
        for builtin in &self.builtin {
            let template = saved(&builtin.language, &builtin.abbreviation).unwrap_or(builtin).clone();
            all.push((template, Origin::Module(builtin)));
        }
        for (plugin, declared) in self.plugin_templates() {
            if all.iter().any(|(known, _)| known.same_key(&declared.language, &declared.abbreviation)) {
                continue;
            }
            let mut template = declared.clone();
            if let Some(user) = saved(&declared.language, &declared.abbreviation) {
                template.enabled = user.enabled;
            }
            all.push((template, Origin::Plugin(plugin, declared)));
        }
        for user in &self.saved.templates {
            if !self.is_builtin(&user.language, &user.abbreviation) {
                all.push((user.clone(), Origin::User));
            }
        }
        all
    }

    fn effective(&self) -> Vec<TemplateInfo> {
        self.templates()
            .into_iter()
            .map(|(template, origin)| {
                let (default, plugin) = match origin {
                    Origin::Module(builtin) => (Some(builtin.clone()), None),
                    Origin::Plugin(plugin, declared) => (Some(declared.clone()), Some(plugin.to_owned())),
                    Origin::User => (None, None),
                };
                TemplateInfo {
                    description_text: template.description_text(),
                    builtin: default.is_some(),
                    modified: default.as_ref().is_some_and(|default| *default != template),
                    default,
                    plugin,
                    template,
                }
            })
            .collect()
    }

    fn is_builtin(&self, language: &str, abbreviation: &str) -> bool {
        self.builtin.iter().any(|builtin| builtin.same_key(language, abbreviation))
            || self.plugin_templates().any(|(_, template)| template.same_key(language, abbreviation))
    }

    fn plugin_template(&self, language: &str, abbreviation: &str) -> Option<Template> {
        if self.builtin.iter().any(|builtin| builtin.same_key(language, abbreviation)) {
            return None;
        }
        self.plugin_templates().find(|(_, template)| template.same_key(language, abbreviation)).map(|(_, t)| t.clone())
    }

    fn all_postfix(&self) -> impl Iterator<Item = (&PostfixTemplate, Option<&str>)> {
        let plugins = self
            .plugins
            .iter()
            .flat_map(|set| set.postfix.iter().map(move |postfix| (postfix, Some(set.plugin.as_str()))));
        self.postfix.iter().map(|postfix| (postfix, None)).chain(plugins)
    }

    fn all_doc_comments(&self) -> impl Iterator<Item = &DocComment> {
        self.doc_comments.iter().chain(self.plugins.iter().flat_map(|set| set.doc_comments.iter()))
    }

    fn all_statements(&self) -> impl Iterator<Item = &StatementRules> {
        self.statements.iter().chain(self.plugins.iter().flat_map(|set| set.statements.iter()))
    }

    fn save(&mut self) -> Result<(), String> {
        self.generation += 1;
        let Some(file) = &self.file else { return Ok(()) };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        self.saved.version = FORMAT_VERSION;
        let json = serde_json::to_vec_pretty(&self.saved).unwrap_or_default();
        zen_vfs::write_atomic(file, &json)
            .map_err(|error| t!("workbench-templates-save-failed", detail = error.to_string()))
    }
}

fn read_saved(file: &Path) -> (Saved, Option<String>) {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Saved::default(), None),
        Err(error) => {
            return (Saved::default(), Some(t!("workbench-templates-unreadable", detail = error.to_string())));
        }
    };
    let raw: RawSaved = match serde_json::from_slice(&bytes) {
        Ok(raw) => raw,
        Err(error) => {
            let kept = file.with_extension("json.corrupt");
            let kept = if std::fs::rename(file, &kept).is_ok() { kept } else { file.to_owned() };
            let notice =
                t!("workbench-templates-corrupt", detail = error.to_string(), file = kept.display().to_string());
            return (Saved::default(), Some(notice));
        }
    };
    let total = raw.templates.len();
    let templates: Vec<Template> = raw
        .templates
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Template>(value).ok())
        .filter(|template| template.validate().is_ok())
        .collect();
    let skipped = total - templates.len();
    let notice = (skipped > 0).then(|| t!("workbench-templates-skipped", count = skipped));
    let saved =
        Saved { version: raw.version, templates, postfix_off: raw.postfix_off, postfix_disabled: raw.postfix_disabled };
    (saved, notice)
}

#[derive(Clone)]
pub struct LiveTemplates {
    inner: Arc<Mutex<Inner>>,
}

impl Default for LiveTemplates {
    fn default() -> Self {
        Self::load(None)
    }
}

impl LiveTemplates {
    pub fn load(file: Option<PathBuf>) -> Self {
        let (saved, notice) = file.as_deref().map_or_else(|| (Saved::default(), None), read_saved);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                file,
                builtin: Vec::new(),
                plugins: Vec::new(),
                surrounders: Vec::new(),
                postfix: Vec::new(),
                doc_comments: Vec::new(),
                statements: Vec::new(),
                saved,
                generation: 0,
                importers: Vec::new(),
                highlighters: None,
                notice,
            })),
        }
    }

    pub fn take_notice(&self) -> Option<String> {
        self.inner().notice.take()
    }

    pub fn set_highlighters(&self, source: HighlighterSource) {
        self.inner().highlighters = Some(source);
    }

    pub fn highlighter(&self, name: &str) -> Option<Arc<dyn Highlighter>> {
        let source = self.inner().highlighters.clone()?;
        source(name)
    }

    pub fn declare_language(&self, builtin: Builtin) {
        {
            let mut inner = self.inner();
            if let Some(add_import) = builtin.add_import {
                inner.importers.retain(|(language, _)| *language != builtin.language);
                inner.importers.push((builtin.language.clone(), add_import));
            }
            for surrounder in builtin.surrounders {
                let known = inner
                    .surrounders
                    .iter()
                    .any(|known| known.language == surrounder.language && known.id == surrounder.id);
                if !known {
                    inner.surrounders.push(surrounder);
                }
            }
            for postfix in builtin.postfix {
                if !inner.postfix.iter().any(|known| known.same_key(&postfix.language, &postfix.key)) {
                    inner.postfix.push(postfix);
                }
            }
            for stub in builtin.doc_comments {
                if !inner.doc_comments.contains(&stub) {
                    inner.doc_comments.push(stub);
                }
            }
            for rules in builtin.statements {
                inner.statements.retain(|known| known.language != rules.language);
                inner.statements.push(rules);
            }
        }
        self.declare(builtin.templates);
    }

    pub fn set_plugin_templates(&self, plugins: Vec<PluginTemplates>) {
        let mut inner = self.inner();
        if inner.plugins != plugins {
            inner.plugins = plugins;
            inner.generation += 1;
        }
    }

    pub fn doc_comments_for(&self, language: &str) -> Vec<DocComment> {
        self.inner().all_doc_comments().filter(|stub| stub.language == language).cloned().collect()
    }

    pub fn statements_for(&self, language: &str) -> Option<StatementRules> {
        self.inner().all_statements().find(|rules| rules.language == language).cloned()
    }

    pub fn postfix_enabled(&self) -> bool {
        !self.inner().saved.postfix_disabled
    }

    pub fn postfix_list(&self) -> Vec<postfix::PostfixInfo> {
        let inner = self.inner();
        inner
            .all_postfix()
            .map(|(template, plugin)| postfix::PostfixInfo {
                template: template.clone(),
                description_text: template.description_text(),
                enabled: !inner.saved.postfix_off.contains(&template.id()),
                plugin: plugin.map(str::to_owned),
            })
            .collect()
    }

    pub fn postfix_for(&self, language: &str) -> Vec<PostfixTemplate> {
        let inner = self.inner();
        if inner.saved.postfix_disabled {
            return Vec::new();
        }
        let mut found: Vec<PostfixTemplate> = Vec::new();
        for (template, _) in inner.all_postfix() {
            if template.language == language
                && !inner.saved.postfix_off.contains(&template.id())
                && !found.iter().any(|known| known.key == template.key)
            {
                found.push(template.clone());
            }
        }
        found
    }

    pub fn set_postfix(&self, language: Option<&str>, key: Option<&str>, enabled: bool) -> Result<(), String> {
        let mut inner = self.inner();
        match (language, key) {
            (None, None) => inner.saved.postfix_disabled = !enabled,
            (language, key) => {
                let chosen: Vec<String> = inner
                    .all_postfix()
                    .map(|(template, _)| template)
                    .filter(|template| language.is_none_or(|language| template.language == language))
                    .filter(|template| key.is_none_or(|key| template.key == key))
                    .map(PostfixTemplate::id)
                    .collect();
                if chosen.is_empty() {
                    return Err(t!(
                        "workbench-postfix-unknown",
                        name = key.unwrap_or_default(),
                        language = language.unwrap_or_default()
                    ));
                }
                for id in chosen {
                    inner.saved.postfix_off.retain(|off| *off != id);
                    if !enabled {
                        inner.saved.postfix_off.push(id);
                    }
                }
            }
        }
        inner.save()
    }

    pub fn surrounders_for(&self, language: &str) -> Vec<Surrounder> {
        let inner = self.inner();
        let mut found: Vec<Surrounder> = Vec::new();
        let declared = inner.plugins.iter().flat_map(|set| set.surrounders.iter());
        for surrounder in inner.surrounders.iter().chain(declared).filter(|surrounder| surrounder.language == language)
        {
            if !found.iter().any(|known| known.id == surrounder.id) {
                found.push(surrounder.clone());
            }
        }
        found
    }

    pub fn importer(&self, language: &str) -> Option<AddImport> {
        self.inner().importers.iter().find(|(known, _)| known == language).map(|(_, add_import)| *add_import)
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn declare(&self, templates: impl IntoIterator<Item = Template>) {
        let mut inner = self.inner();
        for template in templates {
            if !inner.is_builtin(&template.language, &template.abbreviation) {
                inner.builtin.push(template);
            }
        }
        inner.generation += 1;
    }

    pub fn generation(&self) -> u64 {
        self.inner().generation
    }

    pub fn list(&self) -> Vec<TemplateInfo> {
        self.inner().effective()
    }

    pub fn for_language(&self, language: &str) -> Vec<Template> {
        let inner = self.inner();
        inner
            .templates()
            .into_iter()
            .map(|(template, _)| template)
            .filter(|template| template.enabled && template.applies_to(language))
            .collect()
    }

    pub fn any_for(&self, language: &str) -> bool {
        self.inner().templates().iter().any(|(template, _)| template.enabled && template.applies_to(language))
    }

    pub fn set(&self, template: Template, previous: Option<&str>) -> Result<(), String> {
        template.validate()?;
        let mut inner = self.inner();
        if let Some(declared) = inner.plugin_template(&template.language, &template.abbreviation) {
            if (Template { enabled: declared.enabled, ..template.clone() }) != declared {
                return Err(t!("workbench-templates-plugin", name = template.abbreviation.as_str()));
            }
            inner.saved.templates.retain(|user| !user.same_key(&template.language, &template.abbreviation));
            if template.enabled != declared.enabled {
                inner.saved.templates.push(template);
            }
            return inner.save();
        }
        if let Some(previous) = previous.filter(|previous| *previous != template.abbreviation) {
            if inner.is_builtin(&template.language, previous) {
                return Err(t!("workbench-templates-builtin", name = previous));
            }
            if inner.effective().iter().any(|info| info.template.same_key(&template.language, &template.abbreviation)) {
                return Err(t!(
                    "workbench-templates-exists",
                    name = template.abbreviation.as_str(),
                    language = template.language.as_str()
                ));
            }
            inner.saved.templates.retain(|user| !user.same_key(&template.language, previous));
        }
        let builtin =
            inner.builtin.iter().find(|builtin| builtin.same_key(&template.language, &template.abbreviation)).cloned();
        let index =
            inner.saved.templates.iter().position(|user| user.same_key(&template.language, &template.abbreviation));
        match (index, builtin) {
            (Some(index), Some(builtin)) if builtin == template => {
                inner.saved.templates.remove(index);
            }
            (Some(index), _) => inner.saved.templates[index] = template,
            (None, Some(builtin)) if builtin == template => {}
            (None, _) => inner.saved.templates.push(template),
        }
        inner.save()
    }

    pub fn remove(&self, language: &str, abbreviation: &str) -> Result<(), String> {
        let mut inner = self.inner();
        if inner.is_builtin(language, abbreviation) {
            return Err(t!("workbench-templates-builtin", name = abbreviation));
        }
        let before = inner.saved.templates.len();
        inner.saved.templates.retain(|user| !user.same_key(language, abbreviation));
        if inner.saved.templates.len() == before {
            return Err(t!("workbench-templates-unknown", name = abbreviation, language = language));
        }
        inner.save()
    }

    pub fn reset(&self, language: Option<&str>, abbreviation: Option<&str>) -> Result<usize, String> {
        let mut inner = self.inner();
        let declared: Vec<(String, String)> = inner
            .saved
            .templates
            .iter()
            .filter(|user| inner.is_builtin(&user.language, &user.abbreviation))
            .map(|user| (user.language.clone(), user.abbreviation.clone()))
            .collect();
        let before = inner.saved.templates.len();
        inner.saved.templates.retain(|user| {
            let chosen = language.is_none_or(|language| user.language == language)
                && abbreviation.is_none_or(|abbreviation| user.abbreviation == abbreviation);
            !(chosen && declared.iter().any(|(language, abbreviation)| user.same_key(language, abbreviation)))
        });
        let reset = before - inner.saved.templates.len();
        if reset > 0 {
            inner.save()?;
        }
        Ok(reset)
    }
}

/// What `editor.templates.next`, `previous`, `end` and `expand` report about the template being filled in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// Whether a template is being filled in.
    pub active: bool,
    /// Its abbreviation.
    pub template: Option<String>,
    /// The variable Tab is on.
    pub variable: Option<String>,
    /// That variable's index.
    pub index: usize,
    /// How many variables the template has.
    pub count: usize,
    /// The occurrences of that variable, as character ranges `[start, end]`.
    pub ranges: Vec<[usize; 2]>,
}

fn describe(document: &Document) -> SessionInfo {
    let Some(session) = document.template_session() else { return SessionInfo::default() };
    let (index, name) = session.current();
    SessionInfo {
        active: true,
        template: Some(session.template().to_owned()),
        variable: Some(name.to_owned()),
        index,
        count: session.len(),
        ranges: session.current_ranges().iter().map(|range| [range.start, range.end]).collect(),
    }
}

pub(crate) fn select_current(document: &mut Document) {
    let Some(session) = document.template_session() else { return };
    let selections: Vec<zen_editor::Selection> =
        session.current_ranges().iter().map(|range| zen_editor::Selection::new(range.start, range.end)).collect();
    document.set_selections(zen_editor::Selections::from_vec(selections, 0));
}

fn document_at<'a>(session: &'a mut Session, path: Option<&Path>) -> Result<&'a mut Document, CommandError> {
    let workbench = session.state_mut::<Workbench>();
    let document = match path {
        Some(path) => workbench.documents.iter_mut().find(|document| document.path() == path),
        None => workbench.active_document_mut(),
    };
    document
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

pub(crate) fn applicable(session: &Session, path: &Path) -> Vec<Template> {
    session
        .state::<LiveTemplates>()
        .map(|templates| templates.for_language(&code_style::language_of(path)))
        .unwrap_or_default()
}

pub(crate) fn any_applicable(session: &Session, path: &Path) -> bool {
    session.state::<LiveTemplates>().is_some_and(|templates| templates.any_for(&code_style::language_of(path)))
}

pub(crate) fn context_at(session: &Session, document: &Document, at: usize, caret: usize) -> Context {
    let text = document.text();
    let (at, caret) = (crate::language::byte_offset(text, at), crate::language::byte_offset(text, caret));
    let cached = session
        .state::<crate::brackets::Highlighters>()
        .and_then(|highlighters| highlighters.highlighting_of(document))
        .map(|highlighting| highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner).kind_at(text, at));
    let kind = match cached {
        Some(kind) => kind,
        None => {
            let Some(highlighter) =
                session.state::<LiveTemplates>().and_then(|templates| templates.highlighter(&document.name()))
            else {
                return Context::Code;
            };
            let spans = highlighter.highlight(&text[..caret]);
            spans.iter().find(|span| span.range.contains(&at)).map(|span| span.kind)
        }
    };
    match kind {
        Some(HighlightKind::String) => Context::String,
        Some(HighlightKind::Comment | HighlightKind::DocComment) => Context::Comment,
        _ => Context::Code,
    }
}

pub(crate) fn place_of(document: &Document, offset: usize) -> Place {
    let line = document.snapshot().offset_to_point(offset).map_or(0, |point| point.line);
    Place { file_name: document.name(), line, now: crate::file_templates::variables::Moment::now() }
}

fn import_edit(
    session: &Session,
    path: &Path,
    text: &str,
    template: &Template,
    around: &Range<usize>,
) -> Option<(Range<usize>, String)> {
    if template.imports.is_empty() {
        return None;
    }
    let add_import = session.state::<LiveTemplates>()?.importer(&code_style::language_of(path))?;
    let mut changed = text.to_owned();
    for import in &template.imports {
        if let Some((range, insert)) = add_import(&changed, import) {
            changed.replace_range(range, &insert);
        }
    }
    let prefix = text.chars().zip(changed.chars()).take_while(|(old, new)| old == new).count();
    let (old_len, new_len) = (text.chars().count(), changed.chars().count());
    if prefix == old_len && old_len == new_len {
        return None;
    }
    let suffix = text
        .chars()
        .rev()
        .zip(changed.chars().rev())
        .take_while(|(old, new)| old == new)
        .count()
        .min(old_len - prefix)
        .min(new_len - prefix);
    let range = prefix..old_len - suffix;
    let insert: String = changed.chars().skip(prefix).take(new_len - suffix - prefix).collect();
    (range.end <= around.start || range.start >= around.end).then_some((range, insert))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn abbreviation_before<'a>(text: &str, caret: usize, templates: &'a [Template]) -> Option<(usize, &'a Template)> {
    let at = crate::language::byte_offset(text, caret);
    let (before, after) = text.split_at(at);
    if after.chars().next().is_some_and(is_word) {
        return None;
    }
    templates
        .iter()
        .filter(|template| {
            before
                .strip_suffix(template.abbreviation.as_str())
                .is_some_and(|rest| !rest.chars().next_back().is_some_and(is_word))
        })
        .max_by_key(|template| template.abbreviation.len())
        .map(|template| (caret - template.abbreviation.chars().count(), template))
}

pub(crate) fn insert(
    session: &mut Session,
    path: Option<&Path>,
    range: Range<usize>,
    template: &Template,
) -> Result<SessionInfo, CommandError> {
    let (path, indent) = {
        let document = document_at(session, path)?;
        let text = document.text();
        let start = crate::language::byte_offset(text, range.start);
        let line_start = text[..start].rfind(['\n', '\r']).map_or(0, |at| at + 1);
        let indent: String = text[line_start..start].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        (document.path().to_owned(), indent)
    };
    let unit = {
        let document = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
        document.map_or_else(|| "    ".to_owned(), |document| project_settings::indent_unit_of(session, document))
    };
    let document = document_at(session, Some(&path))?;
    let separator = document.line_separator();
    let around = Around { place: place_of(document, range.start), ..Around::default() };
    let expanded = expand_around(template, &indent, &unit, &around).with_line_separator(separator);
    insert_expanded(session, &path, range, template, &expanded)
}

pub(crate) fn insert_expanded(
    session: &mut Session,
    path: &Path,
    range: Range<usize>,
    template: &Template,
    expanded: &Expanded,
) -> Result<SessionInfo, CommandError> {
    let document = document_at(session, Some(path))?;
    let (text, separator) = (document.text().to_owned(), document.line_separator());
    let import = import_edit(session, path, &text, template, &range);
    let mut edits = vec![(range.clone(), expanded.text.clone())];
    let mut at = range.start;
    if let Some((import_range, import_text)) = import {
        let import_text = zen_text::line_separator::convert(&import_text, separator).into_owned();
        if import_range.end <= range.start {
            at = (range.start + import_text.chars().count()).saturating_sub(import_range.len());
            edits.insert(0, (import_range, import_text));
        } else {
            edits.push((import_range, import_text));
        }
    }
    let document = document_at(session, Some(path))?;
    document.template = None;
    if !document.replace_ranges(edits) {
        return Ok(SessionInfo::default());
    }
    document.template = TemplateSession::new(&template.abbreviation, expanded, at);
    if document.template.is_some() {
        select_current(document);
    } else {
        let end = at + expanded.end;
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(end)));
    }
    Ok(describe(document))
}

pub(crate) fn expand_at_caret(session: &mut Session) -> Result<bool, CommandError> {
    let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return Ok(false) };
    let selections = document.selections();
    if selections.iter().any(|selection| !selection.is_empty()) || document.image().is_some() {
        return Ok(false);
    }
    let caret = selections.primary().head;
    let templates = applicable(session, document.path());
    let word_start = document.text()[..crate::language::byte_offset(document.text(), caret)]
        .chars()
        .rev()
        .take_while(|c| is_word(*c))
        .count();
    let place = context_at(session, document, caret - word_start, caret);
    let templates: Vec<Template> = templates.into_iter().filter(|template| template.expands_in(place)).collect();
    let Some((start, template)) = abbreviation_before(document.text(), caret, &templates) else {
        return postfix::expand_at_caret(session, place);
    };
    let template = template.clone();
    insert(session, None, start..caret, &template)?;
    Ok(true)
}

fn live_session(session: &mut Session) -> Option<&mut Document> {
    let document = session.state_mut::<Workbench>().active_document_mut()?;
    let inside =
        document.template_session().is_some_and(|template| template.contains(document.selections().primary().head));
    if !inside {
        document.template = None;
        return None;
    }
    Some(document)
}

fn refresh_dependents(document: &mut Document) {
    let Some(mut template) = document.template.take() else { return };
    let replacements = template.dependent_values(document.text());
    if !replacements.is_empty() && document.replace_ranges(replacements.clone()) {
        template.replaced(&replacements);
    }
    document.template = Some(template);
}

pub(crate) fn step(session: &mut Session, forward: bool) -> bool {
    let Some(document) = live_session(session) else { return false };
    refresh_dependents(document);
    let Some(template) = document.template.as_mut() else { return false };
    let (index, _) = template.current();
    let moved = if forward { template.go_to(index + 1) } else { index > 0 && template.go_to(index - 1) };
    if forward && !moved {
        finish(document, false);
    } else {
        select_current(document);
    }
    true
}

fn finish(document: &mut Document, keep_caret: bool) {
    let Some(template) = document.template.take() else { return };
    let mut selections = document.selections().clone();
    if keep_caret {
        selections.collapse_to_primary();
    } else {
        selections = zen_editor::Selections::single(zen_editor::Selection::caret(template.end()));
    }
    document.set_selections(selections);
}

pub(crate) fn end(session: &mut Session, keep_caret: bool) -> bool {
    let Some(document) = live_session(session) else { return false };
    finish(document, keep_caret);
    true
}

fn has_text_document(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

fn active_session(session: &Session) -> SessionInfo {
    session.state::<Workbench>().and_then(Workbench::active_document).map(describe).unwrap_or_default()
}

/// The parameters of `editor.templates.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ListParams {
    /// Only the templates that work in files of this language id (its own and Other), e.g. `go`.
    pub language: Option<String>,
}

pub struct ListTemplates;

impl Command for ListTemplates {
    type Params = ListParams;
    type Output = Vec<TemplateInfo>;
    const ID: &'static str = "editor.templates.list";
    const TITLE: &'static str = "Live Templates";
    const DESCRIPTION: &'static str = "The live templates: the built-in ones of the language modules as the user left them, \
         the plug-ins' ones, then the user's own, each with whether it is on and changed.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<Vec<TemplateInfo>, CommandError> {
        let listed = session.state::<LiveTemplates>().map(LiveTemplates::list).unwrap_or_default();
        Ok(match params.language {
            Some(language) => listed.into_iter().filter(|info| info.template.applies_to(&language)).collect(),
            None => listed,
        })
    }
}

/// The parameters of `editor.templates.expand`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ExpandParams {
    /// The template's abbreviation, e.g. `for`.
    pub name: String,
    /// The file, open in the editor; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Where, a character offset; the primary caret when absent. The abbreviation before it, if
    /// it is there, is replaced.
    #[serde(default)]
    pub offset: Option<usize>,
}

pub struct ExpandTemplate;

impl Command for ExpandTemplate {
    type Params = ExpandParams;
    type Output = SessionInfo;
    const ID: &'static str = "editor.templates.expand";
    const TITLE: &'static str = "Expand Live Template";
    const DESCRIPTION: &'static str = "Inserts a live template of the file's language at an offset (replacing its \
         abbreviation there) and starts filling it in; reports the variable Tab is on.";

    fn run(&self, session: &mut Session, params: ExpandParams) -> Result<SessionInfo, CommandError> {
        let document = document_at(session, params.path.as_deref())?;
        let path = document.path().to_owned();
        let offset =
            params.offset.unwrap_or_else(|| document.selections().primary().head).min(document.snapshot().len_chars());
        let text = document.text().to_owned();
        let templates = applicable(session, &path);
        let template = templates
            .iter()
            .find(|template| template.abbreviation == params.name)
            .cloned()
            .ok_or_else(|| CommandError::new(t!("workbench-templates-none", name = params.name.as_str())))?;
        let only = std::slice::from_ref(&template);
        let start = abbreviation_before(&text, offset, only).map_or(offset, |(start, _)| start);
        insert(session, Some(&path), start..offset, &template)
    }
}

pub struct SessionStep<const K: u8>;

impl<const K: u8> Command for SessionStep<K> {
    type Params = NoParams;
    type Output = SessionInfo;
    const ID: &'static str = match K {
        0 => "editor.templates.next",
        1 => "editor.templates.previous",
        _ => "editor.templates.end",
    };
    const TITLE: &'static str = match K {
        0 => "Next Template Variable",
        1 => "Previous Template Variable",
        _ => "Finish Live Template",
    };
    const DESCRIPTION: &'static str = match K {
        0 => {
            "In a live template being filled in, selects the next variable, or ends it at its end after the last (Tab, Enter)."
        }
        1 => "In a live template being filled in, selects the previous variable (⇧Tab).",
        _ => "Ends the live template being filled in, the caret at the template's end.",
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SessionInfo, CommandError> {
        match K {
            0 => step(session, true),
            1 => step(session, false),
            _ => end(session, false),
        };
        Ok(active_session(session))
    }
}

/// The parameters of `editor.templates.set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SetParams {
    /// The template, added or replacing the one with the same language and abbreviation.
    pub template: Template,
    /// The abbreviation the user's template had, when it is renamed.
    #[serde(default)]
    pub previous: Option<String>,
}

pub struct SetTemplate;

impl Command for SetTemplate {
    type Params = SetParams;
    type Output = ();
    const ID: &'static str = "editor.templates.set";
    const TITLE: &'static str = "Save Live Template";
    const DESCRIPTION: &'static str = "Adds a live template or changes one (a language module's too: its text, \
         variables, contexts, description, or `enabled` to switch it off; a plug-in's only `enabled`), saved in \
         templates.json.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<(), CommandError> {
        let templates = session.state_mut::<LiveTemplates>().clone();
        templates.set(params.template, params.previous.as_deref()).map_err(CommandError::new)
    }
}

/// A template by its language and abbreviation.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct TemplateKey {
    /// The language id, e.g. `go`, or `other`.
    pub language: String,
    /// The abbreviation.
    pub abbreviation: String,
}

pub struct RemoveTemplate;

impl Command for RemoveTemplate {
    type Params = TemplateKey;
    type Output = ();
    const ID: &'static str = "editor.templates.remove";
    const TITLE: &'static str = "Remove Live Template";
    const DESCRIPTION: &'static str = "Removes a user's live template; a built-in one can only be switched off.";

    fn run(&self, session: &mut Session, params: TemplateKey) -> Result<(), CommandError> {
        let templates = session.state_mut::<LiveTemplates>().clone();
        templates.remove(&params.language, &params.abbreviation).map_err(CommandError::new)
    }
}

/// The parameters of `editor.templates.reset`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ResetParams {
    /// Only this language's templates.
    pub language: Option<String>,
    /// Only the template with this abbreviation.
    pub abbreviation: Option<String>,
}

/// What `editor.templates.reset` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ResetResult {
    /// How many built-in templates had been changed and are back as declared.
    pub reset: usize,
}

pub struct ResetTemplates;

impl Command for ResetTemplates {
    type Params = ResetParams;
    type Output = ResetResult;
    const ID: &'static str = "editor.templates.reset";
    const TITLE: &'static str = "Restore Default Live Templates";
    const DESCRIPTION: &'static str = "Takes changed built-in live templates back to how their language module declares \
         them: all, a language's, or one; the user's own templates stay.";

    fn run(&self, session: &mut Session, params: ResetParams) -> Result<ResetResult, CommandError> {
        let templates = session.state_mut::<LiveTemplates>().clone();
        let reset =
            templates.reset(params.language.as_deref(), params.abbreviation.as_deref()).map_err(CommandError::new)?;
        Ok(ResetResult { reset })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTemplates)?;
    registry.register(ExpandTemplate)?;
    registry.register(SessionStep::<0>)?;
    registry.register(SessionStep::<1>)?;
    registry.register(SessionStep::<2>)?;
    registry.register(SetTemplate)?;
    registry.register(RemoveTemplate)?;
    registry.register(ResetTemplates)?;
    surround::register(registry)?;
    postfix::register(registry)?;
    statement::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const GO: &str = r#"[
        { "language": "go", "abbreviation": "fori", "description": "go-fori", "text": "for $INDEX$ := 0; $INDEX$ < $LIMIT$; $INDEX$++ {\n\t$END$\n}", "variables": [{ "name": "INDEX", "default": "i" }, { "name": "LIMIT", "default": "n" }] },
        { "language": "go", "abbreviation": "main", "description": "go-main", "text": "func main() {\n\t$END$\n}" }
    ]"#;

    fn declared(file: Option<PathBuf>) -> LiveTemplates {
        let templates = LiveTemplates::load(file);
        templates.declare(Template::parse_list(GO).unwrap());
        templates
    }

    fn open(name: &str, text: &str, caret: usize) -> (Registry, Session, tempfile::TempDir) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(name), text).unwrap();
        let mut session = Session::new();
        session.set_state(declared(None));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join(name) })).unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] })).unwrap();
        (registry, session, dir)
    }

    fn text(session: &Session) -> String {
        session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn selections(session: &Session) -> Vec<(usize, usize)> {
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        document.selections().iter().map(|selection| (selection.anchor, selection.head)).collect()
    }

    fn run(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Value {
        registry.dispatch(session, id, params).unwrap()
    }

    const BODY: &str = "package main\n\nfunc f() {\n\tfori\n}\n";

    #[test]
    fn tab_expands_and_the_variables_are_filled_in_together() {
        let caret = BODY.find("fori").unwrap() + 4;
        let (registry, mut session, _dir) = open("main.go", BODY, caret);
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "package main\n\nfunc f() {\n\tfor i := 0; i < n; i++ {\n\t\t\n\t}\n}\n");
        let start = caret - 4;
        assert_eq!(selections(&session), [(start + 4, start + 5), (start + 12, start + 13), (start + 19, start + 20)]);
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "k" }));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "j" }));
        assert!(text(&session).contains("for kj := 0; kj < n; kj++ {"), "{}", text(&session));
        let next = run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(next.as_array().map(Vec::len), Some(1));
        let info = run(&registry, &mut session, "editor.templates.previous", json!({}));
        assert_eq!((info["variable"].as_str(), info["ranges"].as_array().map(Vec::len)), (Some("INDEX"), Some(3)));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "len(xs)" }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        let done = text(&session);
        assert_eq!(done, "package main\n\nfunc f() {\n\tfor kj := 0; kj < len(xs); kj++ {\n\t\t\n\t}\n}\n");
        let end = done.find("\t\t\n").unwrap() + 2;
        assert_eq!(selections(&session), [(end, end)], "after the last variable the caret goes to $END$");
        assert_eq!(run(&registry, &mut session, "editor.templates.next", json!({}))["active"], false);
        let mut undone = Vec::new();
        while text(&session) != BODY && undone.len() < 6 {
            run(&registry, &mut session, "editor.edit.undo", json!({}));
            undone.push(text(&session));
        }
        assert_eq!(text(&session), BODY, "the expansion and the typing undo step by step");
        assert_eq!(undone[undone.len() - 2], "package main\n\nfunc f() {\n\tfor i := 0; i < n; i++ {\n\t\t\n\t}\n}\n");
    }

    #[test]
    fn escape_keeps_the_caret_and_enter_goes_on_like_tab() {
        let caret = BODY.find("fori").unwrap() + 4;
        let (registry, mut session, _dir) = open("main.go", BODY, caret);
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        run(&registry, &mut session, "editor.caret.keepPrimary", json!({}));
        let start = caret - 4;
        assert_eq!(selections(&session), [(start + 4, start + 5)], "Escape keeps the primary selection");
        assert_eq!(run(&registry, &mut session, "editor.templates.next", json!({}))["active"], false);
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        run(&registry, &mut session, "editor.edit.newline", json!({}));
        assert_eq!(selections(&session), [(start + 16, start + 17)], "Enter goes to the next variable");
        run(&registry, &mut session, "editor.edit.newline", json!({}));
        let done = text(&session);
        assert_eq!(
            done, "package main\n\nfunc f() {\n\tfor i := 0; i < n; i++ {\n\t\t\n\t}\n}\n",
            "Enter types nothing"
        );
        let end = done.find("\t\t\n").unwrap() + 2;
        assert_eq!(selections(&session), [(end, end)], "after the last variable Enter goes to the end");
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(text(&session).contains("\t\t\t\n"), "Tab indents again once the template is done");
    }

    struct Literals;

    impl Highlighter for Literals {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<zen_syntax::Span> {
            let mut spans = Vec::new();
            let mut at = 0;
            while let Some(rest) = text.get(at..).filter(|rest| !rest.is_empty()) {
                let (kind, len) = if rest.starts_with("//") {
                    (HighlightKind::Comment, rest.find('\n').unwrap_or(rest.len()))
                } else if let Some(body) = rest.strip_prefix('"') {
                    (HighlightKind::String, body.find(['"', '\n']).map_or(rest.len(), |end| end + 2))
                } else {
                    at += rest.chars().next().map_or(1, char::len_utf8);
                    continue;
                };
                spans.push(zen_syntax::Span { range: at..at + len, kind });
                at += len;
            }
            spans
        }
    }

    #[test]
    fn tab_expands_in_code_but_not_in_a_string_or_a_comment() {
        let body = "package main\n\nfunc f() {\n\ts := \"a fori\n\t// fori\n\tfori\n}\n";
        let (registry, mut session, _dir) = open("main.go", body, 0);
        session.state::<LiveTemplates>().unwrap().set_highlighters(Arc::new(|name: &str| {
            Literals.handles(name).then(|| Arc::new(Literals) as Arc<dyn Highlighter>)
        }));
        for (needle, typed) in [("fori\n\t//", "\"a fori\t\n"), ("fori\n\tfori", "// fori\t\n")] {
            let caret = text(&session).find(needle).unwrap() + 4;
            run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] }));
            run(&registry, &mut session, "editor.edit.tab", json!({}));
            assert!(text(&session).contains(typed), "{}", text(&session));
        }
        let caret = text(&session).rfind("fori").unwrap() + 4;
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(text(&session).contains("\tfor i := 0; i < n; i++ {"), "{}", text(&session));
    }

    #[test]
    fn with_several_carets_the_primary_one_expands() {
        let caret = BODY.find("fori").unwrap() + 4;
        let (registry, mut session, _dir) = open("main.go", BODY, caret);
        let carets = json!({ "selections": [{ "head": 0 }, { "head": caret }] });
        run(&registry, &mut session, "editor.selection.set", carets);
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "package main\n\nfunc f() {\n\tfor i := 0; i < n; i++ {\n\t\t\n\t}\n}\n");
        assert_eq!(selections(&session).len(), 3, "the other caret goes, the variable's occurrences are selected");
        let carets = json!({ "selections": [{ "head": 0, "anchor": 1 }, { "head": caret }] });
        let (registry, mut session, _dir) = open("main.go", BODY, caret);
        run(&registry, &mut session, "editor.selection.set", carets);
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(!text(&session).contains("for i"), "with a selection Tab indents");
    }

    #[test]
    fn undo_while_filling_in_ends_the_template() {
        let caret = BODY.find("fori").unwrap() + 4;
        let (registry, mut session, _dir) = open("main.go", BODY, caret);
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "k" }));
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        assert_eq!(run(&registry, &mut session, "editor.templates.next", json!({}))["active"], false);
        assert!(text(&session).contains("for i := 0; i < n; i++"), "{}", text(&session));
        run(&registry, &mut session, "editor.edit.redo", json!({}));
        assert!(text(&session).contains("for k := 0; k < n; k++"), "{}", text(&session));
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), BODY);
    }

    fn add_import(text: &str, path: &str) -> Option<(Range<usize>, String)> {
        let line = format!("\nimport \"{path}\"");
        let at = text.find('\n')?;
        (!text.contains(&line)).then_some((at..at, line))
    }

    #[test]
    fn a_template_brings_its_imports_in_the_same_undo_step() {
        let body = "package main\n\nfunc f() {\n\tprint\n}\n";
        let (registry, mut session, _dir) = open("main.go", body, body.find("print").unwrap() + 5);
        let print = Template {
            language: "go".to_owned(),
            abbreviation: "print".to_owned(),
            description: String::new(),
            text: "fmt.Println($VALUE$)$END$".to_owned(),
            variables: Vec::new(),
            imports: vec!["fmt".to_owned()],
            contexts: Vec::new(),
            enabled: true,
        };
        session.state::<LiveTemplates>().unwrap().declare_language(Builtin {
            language: "go".to_owned(),
            templates: vec![print],
            add_import: Some(add_import),
            surrounders: Vec::new(),
            ..Default::default()
        });
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        let done = text(&session);
        assert_eq!(done, "package main\nimport \"fmt\"\n\nfunc f() {\n\tfmt.Println()\n}\n");
        let inside = done.find("Println(").unwrap() + 8;
        assert_eq!(selections(&session), [(inside, inside)]);
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "x" }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(text(&session).contains("fmt.Println(x)\n"));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "\n\tprint" }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session).matches("import").count(), 1, "{}", text(&session));
        for _ in 0..6 {
            if text(&session) == body {
                break;
            }
            run(&registry, &mut session, "editor.edit.undo", json!({}));
            let now = text(&session);
            assert_eq!(now.contains("import"), now.contains("fmt."), "the import goes with the expansion: {now}");
        }
        assert_eq!(text(&session), body);
    }

    #[test]
    fn a_broken_file_leaves_the_built_in_templates_and_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("templates.json");
        std::fs::write(&file, "{ not json").unwrap();
        let templates = declared(Some(file.clone()));
        assert_eq!(templates.list().len(), 2);
        assert!(templates.take_notice().is_some_and(|notice| notice.contains("templates.json.corrupt")));
        assert_eq!(templates.take_notice(), None, "once");
        assert_eq!(std::fs::read_to_string(dir.path().join("templates.json.corrupt")).unwrap(), "{ not json");
        assert!(!file.exists());
        let good = json!({ "language": "go", "abbreviation": "pkg", "text": "package $NAME$" });
        let saved = json!({ "version": 1, "templates": [good, { "language": "go" }, { "language": "go", "abbreviation": "a b", "text": "x" }] });
        std::fs::write(&file, saved.to_string()).unwrap();
        let templates = declared(Some(file.clone()));
        assert_eq!(templates.list().len(), 3, "the valid one is kept");
        assert!(templates.take_notice().is_some_and(|notice| notice.contains('2')));
        assert_eq!(declared(Some(dir.path().join("none.json"))).take_notice(), None);
    }

    #[test]
    fn a_caret_that_leaves_the_template_ends_it() {
        let caret = BODY.find("fori").unwrap() + 4;
        let (registry, mut session, _dir) = open("main.go", BODY, caret);
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(text(&session).starts_with("\tpackage"), "{}", text(&session));
        let (registry, mut session, _dir) = open("main.go", "package main\n\nvar formain = 1\n", 19);
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "package main\n\nvar f\tormain = 1\n", "only a whole word expands");
    }

    #[test]
    fn tab_tries_the_plugin_first_then_the_templates_then_indents() {
        let (registry, mut session, folder, _plugin) = crate::plugins::testing::session(&[("page.web", "x ul>li\n")]);
        let templates = declared(None);
        let other = Template {
            language: OTHER.to_owned(),
            abbreviation: "li>".to_owned(),
            description: "An item".to_owned(),
            text: "[$TEXT$]".to_owned(),
            variables: Vec::new(),
            imports: Vec::new(),
            contexts: Vec::new(),
            enabled: true,
        };
        templates.set(other, None).unwrap();
        session.set_state(templates);
        registry
            .dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("page.web") }))
            .unwrap();
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": 7 }] }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "x <ul><li></li></ul>\n", "Emmet first");
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "x <ul><[]</li></ul>\n", "then a template of the Other group");
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "x <ul><[]    </li></ul>\n", "then the indentation");
    }

    #[test]
    fn the_settings_are_saved_reset_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("templates.json");
        let templates = declared(Some(file.clone()));
        let mut main = templates.list().into_iter().find(|info| info.template.abbreviation == "main").unwrap().template;
        main.enabled = false;
        templates.set(main.clone(), None).unwrap();
        let mine = Template { abbreviation: "pkg".to_owned(), text: "package $NAME$".to_owned(), ..main.clone() };
        templates.set(Template { enabled: true, ..mine.clone() }, None).unwrap();
        assert!(templates.set(Template { abbreviation: "a b".to_owned(), ..mine.clone() }, None).is_err());
        assert!(templates.remove("go", "main").is_err(), "a built-in one is switched off, not removed");

        let again = declared(Some(file.clone()));
        let listed = again.list();
        let main_info = listed.iter().find(|info| info.template.abbreviation == "main").unwrap();
        assert!(main_info.builtin && main_info.modified && !main_info.template.enabled);
        let pkg = listed.iter().find(|info| info.template.abbreviation == "pkg").unwrap();
        assert!(!pkg.builtin && pkg.default.is_none());
        let names: Vec<String> = again.for_language("go").into_iter().map(|template| template.abbreviation).collect();
        assert_eq!(names, ["fori", "pkg"]);

        again.set(Template { abbreviation: "package".to_owned(), ..mine.clone() }, Some("pkg")).unwrap();
        assert_eq!(again.reset(Some("go"), None).unwrap(), 1);
        assert!(again.list().iter().all(|info| !info.modified));
        again.remove("go", "package").unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved, json!({ "version": 1, "templates": [] }));
    }

    #[test]
    fn the_commands_list_expand_and_edit() {
        let (registry, mut session, dir) = open("main.go", "package main\n\n", 14);
        let listed = run(&registry, &mut session, "editor.templates.list", json!({ "language": "go" }));
        assert_eq!(listed.as_array().map(Vec::len), Some(2));
        assert_eq!(listed[0]["template"]["abbreviation"], "fori");
        assert_eq!(listed[0]["descriptionText"], "go-fori", "a description that is no message shows as it is");
        assert_eq!(run(&registry, &mut session, "editor.templates.list", json!({ "language": "yaml" })), json!([]));
        let path = dir.path().join("main.go");
        let info = run(&registry, &mut session, "editor.templates.expand", json!({ "name": "main", "path": path }));
        assert_eq!(info["active"], false, "a template without variables is done at once");
        assert_eq!(text(&session), "package main\n\nfunc main() {\n\t\n}");
        assert_eq!(selections(&session), [(29, 29)]);
        let info = run(&registry, &mut session, "editor.templates.expand", json!({ "name": "fori", "offset": 14 }));
        assert_eq!(
            (info["active"].as_bool(), info["variable"].as_str(), info["count"].as_u64()),
            (Some(true), Some("INDEX"), Some(2))
        );
        assert_eq!(run(&registry, &mut session, "editor.templates.end", json!({}))["active"], false);
        assert!(registry.dispatch(&mut session, "editor.templates.expand", json!({ "name": "nope" })).is_err());

        let template = json!({ "language": "go", "abbreviation": "fori", "text": "for $I$ {}", "enabled": false });
        run(&registry, &mut session, "editor.templates.set", json!({ "template": template }));
        let listed = run(&registry, &mut session, "editor.templates.list", json!({}));
        assert_eq!(
            (listed[0]["modified"].as_bool(), listed[0]["template"]["enabled"].as_bool()),
            (Some(true), Some(false))
        );
        assert_eq!(
            run(&registry, &mut session, "editor.templates.reset", json!({ "abbreviation": "fori" })),
            json!({ "reset": 1 })
        );
        assert!(
            registry
                .dispatch(&mut session, "editor.templates.remove", json!({ "language": "go", "abbreviation": "x" }))
                .is_err()
        );
    }

    fn declare(session: &Session, builtin: Builtin) {
        session.state::<LiveTemplates>().unwrap().declare_language(builtin);
    }

    #[test]
    fn a_template_expands_only_in_the_places_it_is_applicable_in() {
        let body = "package main\n\n// todo\nfunc f() {\n\ttodo\n}\n";
        let (registry, mut session, _dir) = open("main.go", body, 0);
        session.state::<LiveTemplates>().unwrap().set_highlighters(Arc::new(|name: &str| {
            Literals.handles(name).then(|| Arc::new(Literals) as Arc<dyn Highlighter>)
        }));
        let todo = Template {
            language: "go".to_owned(),
            abbreviation: "todo".to_owned(),
            text: "TODO(me): $END$".to_owned(),
            contexts: vec![Context::Comment],
            ..Template::default()
        };
        session.state::<LiveTemplates>().unwrap().set(todo, None).unwrap();
        let in_code = text(&session).rfind("todo").unwrap() + 4;
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": in_code }] }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(text(&session).contains("\ttodo\t\n"), "not in code: {}", text(&session));
        let in_comment = text(&session).find("todo").unwrap() + 4;
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": in_comment }] }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(text(&session).starts_with("package main\n\n// TODO(me): \n"), "{}", text(&session));
    }

    #[test]
    fn a_variable_follows_the_expression_it_is_computed_from_and_tab_skips_a_defined_one() {
        let body = "package main\n\nvar x = ctor\n";
        let (registry, mut session, _dir) = open("main.go", body, body.find("ctor").unwrap() + 4);
        let ctor = Template {
            language: "go".to_owned(),
            abbreviation: "ctor".to_owned(),
            text: "$TYPE${$NAME$} // $FILE$ $END$".to_owned(),
            variables: vec![
                Variable { name: "TYPE".to_owned(), default: "T".to_owned(), ..Variable::default() },
                Variable {
                    name: "NAME".to_owned(),
                    expression: "decapitalize(TYPE)".to_owned(),
                    ..Variable::default()
                },
                Variable {
                    name: "FILE".to_owned(),
                    expression: "fileNameWithoutExtension()".to_owned(),
                    skip_if_defined: true,
                    ..Variable::default()
                },
            ],
            ..Template::default()
        };
        assert!(
            session
                .state::<LiveTemplates>()
                .unwrap()
                .set(
                    Template {
                        variables: vec![Variable { expression: "nope(".to_owned(), ..ctor.variables[0].clone() }],
                        ..ctor.clone()
                    },
                    None
                )
                .is_err()
        );
        session.state::<LiveTemplates>().unwrap().set(ctor, None).unwrap();
        let info = run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(info.as_array().map(Vec::len), Some(1));
        assert!(text(&session).contains("var x = T{t} // main "), "{}", text(&session));
        let state = run(&registry, &mut session, "editor.templates.next", json!({}));
        assert_eq!(state["count"], 2, "the file name is defined, so Tab passes it by");
        run(&registry, &mut session, "editor.templates.previous", json!({}));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "Server" }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(text(&session).contains("var x = Server{server} // main "), "{}", text(&session));
    }

    #[test]
    fn plugin_templates_are_listed_read_only_and_can_only_be_switched_off() {
        let dir = tempfile::tempdir().unwrap();
        let templates = declared(Some(dir.path().join("templates.json")));
        let sout = json!({ "language": "java", "abbreviation": "sout", "text": "System.out.println($END$);" });
        let fori = json!({ "language": "go", "abbreviation": "fori", "text": "shadowed" });
        let postfix = json!({ "language": "java", "key": "nn", "text": "if ($EXPR$ != null) {\n\t$END$\n}" });
        templates.set_plugin_templates(vec![PluginTemplates::parse("java", &[sout, fori], &[postfix])]);
        let listed = templates.list();
        let info = listed.iter().find(|info| info.template.abbreviation == "sout").unwrap();
        assert_eq!((info.plugin.as_deref(), info.builtin, info.modified), (Some("java"), true, false));
        assert_eq!(
            listed.iter().filter(|info| info.template.abbreviation == "fori").count(),
            1,
            "a module's comes first"
        );
        let mut changed = info.template.clone();
        changed.text = "mine".to_owned();
        assert!(templates.set(changed, None).is_err(), "duplicate it to change it");
        assert!(templates.remove("java", "sout").is_err());
        templates.set(Template { enabled: false, ..info.template.clone() }, None).unwrap();
        assert!(templates.for_language("java").is_empty());
        let again = declared(Some(dir.path().join("templates.json")));
        again.set_plugin_templates(vec![PluginTemplates::parse(
            "java",
            &[json!({ "language": "java", "abbreviation": "sout", "text": "System.out.println($END$); // new" })],
            &[],
        )]);
        let info = again.list().into_iter().find(|info| info.template.abbreviation == "sout").unwrap();
        assert!(
            !info.template.enabled && info.template.text.ends_with("// new"),
            "the plug-in's text, the user's switch"
        );
        assert_eq!(again.reset(None, None).unwrap(), 1);
        assert_eq!(templates.postfix_for("java").len(), 1);
        templates.set_plugin_templates(Vec::new());
        assert!(templates.postfix_for("java").is_empty() && !templates.list().iter().any(|info| info.plugin.is_some()));
    }

    fn go_postfix() -> Vec<PostfixTemplate> {
        PostfixTemplate::parse_list(
            r#"[{ "language": "go", "key": "if", "description": "If", "text": "if $EXPR$ {\n\t$END$\n}" },
                { "language": "go", "key": "nn", "text": "if $EXPR$ != nil {\n\t$END$\n}" }]"#,
        )
        .unwrap()
    }

    #[test]
    fn postfix_templates_expand_with_tab_and_from_the_completion_and_can_be_switched_off() {
        let body = "package main\n\nfunc f() {\n\tok(x).if\n}\n";
        let (registry, mut session, _dir) = open("main.go", body, body.find(".if").unwrap() + 3);
        declare(&session, Builtin { language: "go".to_owned(), postfix: go_postfix(), ..Builtin::default() });
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "package main\n\nfunc f() {\n\tif ok(x) {\n\t\t\n\t}\n}\n");
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        let caret = body.find(".if").unwrap() + 3;
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] }));
        run(&registry, &mut session, "editor.edit.backspace", json!({}));
        run(&registry, &mut session, "editor.edit.backspace", json!({}));
        let listed = run(&registry, &mut session, "editor.completion.start", json!({}));
        let items = listed["items"].as_array().unwrap();
        let nn = items.iter().find(|item| item["label"] == "nn").unwrap();
        assert_eq!(nn["kind"], "postfix");
        run(&registry, &mut session, "editor.completion.accept", json!({ "index": nn["index"] }));
        assert_eq!(text(&session), "package main\n\nfunc f() {\n\tif ok(x) != nil {\n\t\t\n\t}\n}\n");
        let listed = run(&registry, &mut session, "editor.postfix.list", json!({ "language": "go" }));
        assert_eq!((listed["enabled"].as_bool(), listed["templates"].as_array().map(Vec::len)), (Some(true), Some(2)));
        run(&registry, &mut session, "editor.postfix.set", json!({ "language": "go", "key": "nn", "enabled": false }));
        let names: Vec<String> = session
            .state::<LiveTemplates>()
            .unwrap()
            .postfix_for("go")
            .into_iter()
            .map(|postfix| postfix.key)
            .collect();
        assert_eq!(names, ["if"]);
        run(&registry, &mut session, "editor.postfix.set", json!({ "enabled": false }));
        assert!(session.state::<LiveTemplates>().unwrap().postfix_for("go").is_empty());
        let (registry, mut session, dir) = open("main.go", "package main\n\nvar v = a.b.\n", 0);
        declare(&session, Builtin { language: "go".to_owned(), postfix: go_postfix(), ..Builtin::default() });
        let at = "package main\n\nvar v = a.b.".len();
        let path = dir.path().join("main.go");
        run(&registry, &mut session, "editor.postfix.expand", json!({ "key": "if", "path": path, "offset": at }));
        assert_eq!(text(&session), "package main\n\nvar v = if a.b {\n\t\n}\n");
    }

    #[test]
    fn complete_current_statement_follows_the_language_rules() {
        let body = "package main\n\nfunc f() {\n\tif ok(x\n}\n";
        let (registry, mut session, _dir) = open("main.go", body, body.find("ok(x").unwrap() + 4);
        let rules =
            StatementRules { language: "go".to_owned(), blocks: vec!["if".to_owned()], ..StatementRules::default() };
        declare(&session, Builtin { language: "go".to_owned(), statements: vec![rules], ..Builtin::default() });
        run(&registry, &mut session, "editor.statement.complete", json!({}));
        let done = text(&session);
        assert_eq!(done, "package main\n\nfunc f() {\n\tif ok(x) {\n\t\t\n\t}\n}\n");
        let caret = done.find("\t\t\n").unwrap() + 2;
        assert_eq!(selections(&session), [(caret, caret)]);
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), body, "one undo step");
    }

    #[test]
    fn typing_the_opening_of_a_doc_comment_above_a_declaration_puts_its_stub() {
        let body = "\npub fn load(path: &str) -> Result<(), Error> {\n}\n";
        let (registry, mut session, _dir) = open("lib.rs", body, 0);
        let stub = DocComment::parse_list(
            r#"[{ "language": "rust", "opening": "///", "keywords": ["fn"], "resultContains": "Result",
                 "text": "/// $END$\n$RESULT$", "result": "///\n/// # Errors" }]"#,
        )
        .unwrap();
        declare(&session, Builtin { language: "rust".to_owned(), doc_comments: stub, ..Builtin::default() });
        for slash in ["/", "/", "/"] {
            run(&registry, &mut session, "editor.edit.type", json!({ "text": slash }));
        }
        assert_eq!(text(&session), "/// \n///\n/// # Errors\npub fn load(path: &str) -> Result<(), Error> {\n}\n");
        assert_eq!(selections(&session), [(4, 4)]);
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), format!("///{body}"), "the stub is its own undo step");
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "\n" }));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "/" }));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "/" }));
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "/" }));
        assert!(!text(&session).contains("# Errors"), "under a doc comment line no stub: {}", text(&session));
    }

    fn plugin_declarations(plugin: &str) -> PluginTemplates {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../plugins/{plugin}/plugin.json"));
        let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        let live = &manifest["liveTemplates"];
        let list = |key: &str| live[key].as_array().cloned().unwrap_or_default();
        PluginTemplates::parse(plugin, &list("templates"), &list("postfix"))
            .with_assists(&list("docComments"), &list("statements"))
            .with_surrounders(&list("surrounders"))
    }

    fn open_with_plugin(plugin: &str, name: &str, text: &str, caret: usize) -> (Registry, Session, tempfile::TempDir) {
        let opened = open(name, text, caret);
        opened.1.state::<LiveTemplates>().unwrap().set_plugin_templates(vec![plugin_declarations(plugin)]);
        opened
    }

    #[test]
    fn a_plugin_text_in_several_languages_is_shown_in_the_interface_language() {
        let declared = json!({ "description": { "en": "Prints", "ru": "Печатает" }, "text": "x" });
        assert_eq!(localised(&declared, "description", "ru-RU")["description"], "Печатает");
        assert_eq!(localised(&declared, "description", "de")["description"], "Prints");
        assert_eq!(localised(&json!({ "description": "Plain" }), "description", "ru")["description"], "Plain");
    }

    fn type_text(registry: &Registry, session: &mut Session, typed: &str) {
        for c in typed.chars() {
            run(registry, session, "editor.edit.type", json!({ "text": c.to_string() }));
        }
    }

    #[test]
    fn the_java_plugin_brings_its_templates_postfix_surrounders_javadoc_and_statements() {
        let body = "class Shop {\n    void ship() {\n        \n    }\n}\n";
        let inside = body.find("        \n").unwrap() + 8;
        let (registry, mut session, _dir) = open_with_plugin("java", "Shop.java", body, inside);
        let listed = session.state::<LiveTemplates>().unwrap().list();
        let sout = listed.iter().find(|info| info.template.abbreviation == "sout").unwrap();
        assert_eq!(sout.plugin.as_deref(), Some("java"));
        assert_eq!(sout.template.description, "Prints a string to System.out", "the English text of the manifest");
        type_text(&registry, &mut session, "sout");
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), body.replace("        \n", "        System.out.println();\n"));
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        let at = text(&session).find("        \n").unwrap() + 8;
        run(&registry, &mut session, "editor.selection.set", json!({ "selections": [{ "head": at }] }));
        type_text(&registry, &mut session, "name.nn");
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert!(
            text(&session).contains("        if (name != null) {\n            \n        }\n"),
            "{}",
            text(&session)
        );

        let body = "class Shop {\n    void ship() {\n        send(order);\n    }\n}\n";
        let (registry, mut session, _dir) = open_with_plugin("java", "Shop.java", body, body.find("send").unwrap());
        let list = run(&registry, &mut session, "editor.surround.list", json!({}));
        let names: Vec<&str> =
            list["items"].as_array().unwrap().iter().filter_map(|item| item["name"].as_str()).collect();
        for name in ["if", "ifElse", "while", "for", "tryCatch", "tryFinally", "synchronized", "runnable"] {
            assert!(names.contains(&name), "{name} in {names:?}");
        }
        run(&registry, &mut session, "editor.surround.apply", json!({ "name": "tryCatch" }));
        assert_eq!(
            text(&session),
            "class Shop {\n    void ship() {\n        try {\n            send(order);\n        } catch (Exception e) {\n            \n        }\n    }\n}\n"
        );

        let body = "class Shop {\n    /**\n    @Override\n    public Map<String, Integer> read(Path path, int limit) throws IOException {\n    }\n}\n";
        let (registry, mut session, _dir) = open_with_plugin("java", "Shop.java", body, body.find("/**").unwrap() + 3);
        run(&registry, &mut session, "editor.edit.newline", json!({}));
        assert_eq!(
            text(&session),
            body.replace(
                "    /**\n",
                "    /**\n     * \n     * @param path\n     * @param limit\n     * @return\n     * @throws IOException\n     */\n"
            )
        );

        let body = "class Shop {\n    public List<String> names(int count\n}\n";
        let (registry, mut session, _dir) =
            open_with_plugin("java", "Shop.java", body, body.find("count").unwrap() + 5);
        run(&registry, &mut session, "editor.statement.complete", json!({}));
        assert_eq!(text(&session), "class Shop {\n    public List<String> names(int count) {\n        \n    }\n}\n");
        let body = "class Shop {\n    void f() {\n        send(order\n    }\n}\n";
        let (registry, mut session, _dir) =
            open_with_plugin("java", "Shop.java", body, body.find("order").unwrap() + 5);
        run(&registry, &mut session, "editor.statement.complete", json!({}));
        assert!(text(&session).contains("        send(order);\n"), "{}", text(&session));
    }

    #[test]
    fn the_kotlin_plugin_brings_its_templates_postfix_kdoc_and_statements() {
        let body = "fun main() {\n    \n}\n";
        let (registry, mut session, _dir) =
            open_with_plugin("kotlin", "Main.kt", body, body.find("    \n").unwrap() + 4);
        type_text(&registry, &mut session, "items.for");
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        assert_eq!(text(&session), "fun main() {\n    for (item in items) {\n        \n    }\n}\n");

        let body = "/**\n@Throws(IOException::class)\nfun <T : Comparable<T>> List<T>.biggest(limit: Int): T? {\n}\n";
        let (registry, mut session, _dir) = open_with_plugin("kotlin", "Main.kt", body, 3);
        run(&registry, &mut session, "editor.edit.newline", json!({}));
        assert_eq!(
            text(&session),
            body.replace("/**\n", "/**\n * \n * @param limit\n * @return\n * @throws IOException\n */\n")
        );
        assert_eq!(selections(&session), [(7, 7)]);

        let body = "fun main() {\n    if x > 0\n}\n";
        let (registry, mut session, _dir) = open_with_plugin("kotlin", "Main.kt", body, body.find("0").unwrap() + 1);
        run(&registry, &mut session, "editor.statement.complete", json!({}));
        assert_eq!(text(&session), "fun main() {\n    if (x > 0) {\n        \n    }\n}\n");
        let body = "fun main() {\n    println(x\n}\n";
        let (registry, mut session, _dir) = open_with_plugin("kotlin", "Main.kt", body, body.find("(x").unwrap() + 2);
        run(&registry, &mut session, "editor.statement.complete", json!({}));
        assert_eq!(text(&session), "fun main() {\n    println(x)\n}\n", "no terminator in Kotlin");
    }
}

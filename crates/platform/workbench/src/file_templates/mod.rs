pub mod commands;
pub mod storage;
pub mod variables;
pub mod velocity;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_i18n::t;

pub use commands::register;
use storage::Folder;
use velocity::Problem;

/// The tab of File and Code Templates a template is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TemplateKind {
    /// Files: a template of new files.
    #[default]
    File,
    /// Includes: a part other templates take in with `#parse`.
    Include,
    /// Code: a piece of code the IDE generates.
    Code,
}

/// A file template.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileTemplate {
    /// The name, e.g. `Go Test File`; unique on its tab.
    pub name: String,
    /// The extension of the files it creates, without the dot, e.g. `go`; may be empty.
    #[serde(default)]
    pub extension: String,
    /// Its tab: `file`, `include` or `code`.
    #[serde(default)]
    pub kind: TemplateKind,
    /// The new file's name without the extension, with variables, e.g. `${NAME}_test`; empty to ask for it.
    #[serde(default)]
    pub file_name: String,
    /// The text, in the Velocity subset.
    pub text: String,
    /// Reformat according to style: the new file gets the indentation of its code style and is reformatted.
    #[serde(default = "yes")]
    pub reformat: bool,
    /// Enable Live Templates: `$VAR$` and `$END$` (written as `#[[$VAR$]]#`) are filled in with Tab after creation.
    #[serde(default)]
    pub live_templates: bool,
    /// Child templates: more files created beside the main one, on the Files tab only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ChildTemplate>,
}

/// A child template: one more file a template creates beside its main file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChildTemplate {
    /// The extension of the file it creates, without the dot; may be empty.
    #[serde(default)]
    pub extension: String,
    /// The file's name without the extension, with variables, e.g. `${NAME}_test`; empty for the main file's name.
    #[serde(default)]
    pub file_name: String,
    /// The text, in the Velocity subset.
    #[serde(default)]
    pub text: String,
    /// Reformat according to style.
    #[serde(default = "yes")]
    pub reformat: bool,
    /// Enable Live Templates.
    #[serde(default)]
    pub live_templates: bool,
}

impl Default for ChildTemplate {
    fn default() -> Self {
        Self {
            extension: String::new(),
            file_name: String::new(),
            text: String::new(),
            reformat: true,
            live_templates: false,
        }
    }
}

fn yes() -> bool {
    true
}

pub const CHILD_INFIX: &str = ".child.";

impl FileTemplate {
    pub fn parse_list(json: &str) -> Result<Vec<FileTemplate>, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }

    pub fn full_name(&self) -> String {
        storage::full_name(&self.name, &self.extension)
    }

    fn validate(&self) -> Result<(), String> {
        let bad = |text: &str| text.contains(['/', '\\', '\n', '\0']);
        if self.name.trim().is_empty() || bad(&self.name) || self.name.starts_with('.') {
            return Err(t!("workbench-file-templates-invalid-name", name = self.name.as_str()));
        }
        if bad(&self.extension) || self.extension.starts_with('.') || self.extension.contains(char::is_whitespace) {
            return Err(t!("workbench-file-templates-invalid-extension", extension = self.extension.as_str()));
        }
        if self.file_name.contains(['\n', '\0']) {
            return Err(t!("workbench-file-templates-invalid-file-name"));
        }
        if self.name.contains(CHILD_INFIX) {
            return Err(t!("workbench-file-templates-invalid-name", name = self.name.as_str()));
        }
        if !self.children.is_empty() && self.kind != TemplateKind::File {
            return Err(t!("workbench-file-templates-children-files-only"));
        }
        for child in &self.children {
            if bad(&child.extension)
                || child.extension.starts_with('.')
                || child.extension.contains(char::is_whitespace)
            {
                return Err(t!("workbench-file-templates-invalid-extension", extension = child.extension.as_str()));
            }
            if child.file_name.contains(['\n', '\0']) {
                return Err(t!("workbench-file-templates-invalid-file-name"));
            }
        }
        Ok(())
    }

    fn same(&self, kind: TemplateKind, name: &str) -> bool {
        self.kind == kind && self.name == name
    }
}

/// A variable a module or a plug-in gives file templates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredVariable {
    /// The name, e.g. `GO_PACKAGE_NAME`.
    pub name: String,
    /// What it holds: a message id of the module that declares it, or the text itself.
    #[serde(default)]
    pub description: String,
    /// Its value when it is always the same; a module's variable gets its value for the new file's folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Clone, Default)]
pub struct Declared {
    pub origin: String,
    pub templates: Vec<FileTemplate>,
    pub variables: Vec<DeclaredVariable>,
    pub values: Option<zen_modules::TemplateValues>,
    pub offers: Vec<(String, Offer)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    pub markers: Vec<String>,
    pub source_folder: bool,
    pub group: Option<String>,
    pub kind_of: Option<String>,
}

fn offers_of(values: &[serde_json::Value]) -> Vec<(String, Offer)> {
    values
        .iter()
        .filter_map(|value| {
            let name = value.get("name")?.as_str()?.to_owned();
            let markers: Vec<String> = value
                .get("markers")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|marker| marker.as_str())
                .filter(|marker| !matches!(*marker, "" | "." | "..") && !marker.contains(['/', '\\']))
                .map(str::to_owned)
                .collect();
            let source_folder = value.get("sourceFolder").and_then(serde_json::Value::as_bool).unwrap_or(false);
            let group = value.get("group").and_then(serde_json::Value::as_str).map(str::to_owned);
            let kind_of = value.get("kindOf").and_then(serde_json::Value::as_str).map(str::to_owned);
            let offer = Offer { markers, source_folder, group, kind_of };
            (offer != Offer::default()).then_some((name, offer))
        })
        .collect()
}

impl Declared {
    pub fn of_module(set: &zen_modules::FileTemplateSet) -> Self {
        let raw: Vec<serde_json::Value> = serde_json::from_str(set.templates).unwrap_or_default();
        Self {
            offers: offers_of(&raw),
            origin: set.origin.to_owned(),
            templates: FileTemplate::parse_list(set.templates).unwrap_or_default(),
            variables: set
                .variables
                .iter()
                .map(|variable| DeclaredVariable {
                    name: variable.name.to_owned(),
                    description: variable.description.to_owned(),
                    value: None,
                })
                .collect(),
            values: set.values,
        }
    }

    pub fn of_plugin(origin: &str, templates: &[serde_json::Value], variables: &[serde_json::Value]) -> Self {
        let read = |value: &serde_json::Value| serde_json::from_value::<FileTemplate>(value.clone()).ok();
        Self {
            origin: origin.to_owned(),
            templates: templates.iter().filter_map(read).filter(|template| template.validate().is_ok()).collect(),
            variables: variables.iter().filter_map(|value| serde_json::from_value(value.clone()).ok()).collect(),
            values: None,
            offers: offers_of(templates),
        }
    }
}

pub type PluginSource = Arc<dyn Fn() -> Vec<Declared> + Send + Sync>;

/// A scheme: where changed and own templates are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Scheme {
    /// Default: the IDE's, in its data folder.
    Default,
    /// Project: the project's, in `.idea/fileTemplates`, shared through version control.
    Project,
}

/// Where a template in use comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// A module or a plug-in declares it, unchanged.
    Builtin,
    /// The IDE's templates.
    Default,
    /// The project's templates.
    Project,
}

/// A template as `file.templates.list` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    /// The template as it is in use.
    pub template: FileTemplate,
    /// Where it comes from.
    pub source: Source,
    /// Whether a module or a plug-in declares it.
    pub builtin: bool,
    /// Whether it is built in and changed.
    pub modified: bool,
    /// The module or plug-in that declares it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// The built-in template as declared, for a built-in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<FileTemplate>,
    /// The variables asked for when a file is created from it, in order; `NAME` is the file name.
    pub asked: Vec<String>,
    /// What its text or file name has that cannot be rendered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
    /// The files one of which must be in the folder or above it for New to offer the template; `*.go` is any such file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<String>,
    /// Whether New offers it only in a source folder, and File → New never.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub source_folder: bool,
    /// The one row of New that stands for it and the other templates of the group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The template whose New dialog offers this one as a kind, instead of New listing it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_of: Option<String>,
}

fn has_marker(folder: &Path, marker: &str) -> bool {
    match marker.strip_prefix('*') {
        Some(suffix) => std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(suffix)),
        None => folder.join(marker).exists(),
    }
}

impl TemplateInfo {
    pub fn fits(&self, directory: &Path, project: &Path) -> bool {
        if self.markers.is_empty() {
            return true;
        }
        let mut folder = Some(directory);
        while let Some(current) = folder.filter(|folder| folder.starts_with(project)) {
            if self.markers.iter().any(|marker| has_marker(current, marker)) {
                return true;
            }
            folder = current.parent();
        }
        false
    }
}

/// A variable as `file.templates.variables` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VariableInfo {
    /// The name.
    pub name: String,
    /// What it holds, in the interface's language.
    pub description: String,
    /// Its value for a new file in the folder asked about (`NAME` and `FILE_NAME` for the name asked about).
    pub value: String,
    /// The module or plug-in that gives it; none for a predefined one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

struct Inner {
    data: Option<PathBuf>,
    product: String,
    declared: Vec<Declared>,
    plugins: Option<PluginSource>,
    ide: Vec<FileTemplate>,
    schemes: BTreeMap<String, Scheme>,
    generation: u64,
}

const SCHEMES: &str = "file-template-schemes.json";

fn scheme_key(project: &Path) -> String {
    project.to_string_lossy().into_owned()
}

fn read_schemes(data: Option<&Path>) -> BTreeMap<String, Scheme> {
    data.and_then(|data| std::fs::read_to_string(data.join(SCHEMES)).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct FileTemplates {
    inner: Arc<Mutex<Inner>>,
}

impl Default for FileTemplates {
    fn default() -> Self {
        Self::load(None, "")
    }
}

impl FileTemplates {
    pub fn load(data: Option<PathBuf>, product: &str) -> Self {
        let ide = data.as_deref().map(|data| storage::read(&Folder::ide(data))).unwrap_or_default();
        let schemes = read_schemes(data.as_deref());
        Self {
            inner: Arc::new(Mutex::new(Inner {
                data,
                product: product.to_owned(),
                declared: Vec::new(),
                plugins: None,
                ide,
                schemes,
                generation: 0,
            })),
        }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn declare(&self, declared: Declared) {
        let mut inner = self.inner();
        inner.declared.push(declared);
        inner.generation += 1;
    }

    pub fn set_plugins(&self, source: PluginSource) {
        let mut inner = self.inner();
        inner.plugins = Some(source);
        inner.generation += 1;
    }

    pub fn generation(&self) -> u64 {
        self.inner().generation
    }

    pub fn product(&self) -> String {
        self.inner().product.clone()
    }

    fn declarations(&self) -> Vec<Declared> {
        let (mut declared, plugins) = {
            let inner = self.inner();
            (inner.declared.clone(), inner.plugins.clone())
        };
        if let Some(plugins) = plugins {
            declared.extend(plugins());
        }
        declared
    }

    fn builtin(&self) -> Vec<(FileTemplate, String)> {
        self.builtin_with_offers().into_iter().map(|(template, origin, _)| (template, origin)).collect()
    }

    fn builtin_with_offers(&self) -> Vec<(FileTemplate, String, Offer)> {
        let mut builtin: Vec<(FileTemplate, String, Offer)> = Vec::new();
        for declared in self.declarations() {
            for template in declared.templates {
                if !builtin.iter().any(|(known, _, _)| known.same(template.kind, &template.name)) {
                    let offer = declared
                        .offers
                        .iter()
                        .find(|(name, _)| *name == template.name && template.kind == TemplateKind::File)
                        .map(|(_, offer)| offer.clone())
                        .unwrap_or_default();
                    builtin.push((template, declared.origin.clone(), offer));
                }
            }
        }
        builtin
    }

    pub fn scheme_of(&self, project: Option<&Path>) -> Scheme {
        let Some(project) = project else { return Scheme::Default };
        if let Some(chosen) = self.inner().schemes.get(&scheme_key(project)) {
            return *chosen;
        }
        if Folder::project(project).templates.is_dir() { Scheme::Project } else { Scheme::Default }
    }

    fn stored(&self, scheme: Scheme, project: Option<&Path>) -> Vec<FileTemplate> {
        match (scheme, project) {
            (Scheme::Default, _) => self.inner().ide.clone(),
            (Scheme::Project, Some(project)) => storage::read(&Folder::project(project)),
            (Scheme::Project, None) => Vec::new(),
        }
    }

    pub fn list(&self, project: Option<&Path>) -> Vec<TemplateInfo> {
        self.list_in(self.scheme_of(project), project)
    }

    pub fn list_in(&self, scheme: Scheme, project: Option<&Path>) -> Vec<TemplateInfo> {
        let with_offers = self.builtin_with_offers();
        let builtin: Vec<(FileTemplate, String)> =
            with_offers.iter().map(|(template, origin, _)| (template.clone(), origin.clone())).collect();
        let ide = self.stored(Scheme::Default, project);
        let own = if scheme == Scheme::Project { self.stored(Scheme::Project, project) } else { Vec::new() };
        let mut chosen: Vec<(FileTemplate, Source)> = Vec::new();
        for (template, source) in own
            .into_iter()
            .map(|template| (template, Source::Project))
            .chain(ide.into_iter().map(|template| (template, Source::Default)))
            .chain(builtin.iter().map(|(template, _)| (template.clone(), Source::Builtin)))
        {
            if !chosen.iter().any(|(known, _)| known.same(template.kind, &template.name)) {
                chosen.push((template, source));
            }
        }
        let rank = |template: &FileTemplate| {
            let declared = builtin.iter().position(|(known, _)| known.same(template.kind, &template.name));
            (declared.is_none(), declared, template.kind as u8, template.name.to_lowercase())
        };
        chosen.sort_by_key(|(template, _)| rank(template));
        let includes = includes_of(chosen.iter().map(|(template, _)| template));
        let declared_names = self.variable_names();
        chosen
            .into_iter()
            .map(|(template, source)| {
                let default = builtin.iter().find(|(known, _)| known.same(template.kind, &template.name));
                let offer = with_offers
                    .iter()
                    .find(|(known, _, _)| known.same(template.kind, &template.name))
                    .map(|(_, _, offer)| offer.clone())
                    .unwrap_or_default();
                let (asked, problems) = analyse(&template, &includes, &declared_names);
                TemplateInfo {
                    markers: offer.markers,
                    source_folder: offer.source_folder,
                    group: offer.group,
                    kind_of: offer.kind_of,
                    builtin: default.is_some(),
                    modified: default.is_some_and(|(known, _)| *known != template),
                    origin: default.map(|(_, origin)| origin.clone()),
                    default: default.map(|(known, _)| known.clone()),
                    template,
                    source,
                    asked,
                    problems,
                }
            })
            .collect()
    }

    pub fn find(&self, project: Option<&Path>, kind: TemplateKind, name: &str) -> Option<TemplateInfo> {
        self.list(project).into_iter().find(|info| info.template.same(kind, name))
    }

    fn variable_names(&self) -> Vec<String> {
        self.declarations().into_iter().flat_map(|declared| declared.variables).map(|variable| variable.name).collect()
    }

    pub fn declared_variables(&self, directory: &Path) -> Vec<(DeclaredVariable, String)> {
        let mut found: Vec<(DeclaredVariable, String)> = Vec::new();
        for declared in self.declarations() {
            let values = declared.values.map(|values| values(directory)).unwrap_or_default();
            for mut variable in declared.variables {
                if found.iter().any(|(known, _)| known.name == variable.name) {
                    continue;
                }
                if let Some((_, value)) = values.iter().find(|(name, _)| *name == variable.name) {
                    variable.value = Some(value.clone());
                }
                found.push((variable, declared.origin.clone()));
            }
        }
        found
    }

    pub fn set(
        &self,
        scheme: Scheme,
        project: Option<&Path>,
        template: FileTemplate,
        previous: Option<&str>,
    ) -> Result<(), String> {
        template.validate()?;
        let builtin = self.builtin();
        let mut stored = self.stored(scheme, project);
        if let Some(previous) = previous.filter(|previous| *previous != template.name) {
            if builtin.iter().any(|(known, _)| known.same(template.kind, previous)) {
                return Err(t!("workbench-file-templates-builtin", name = previous));
            }
            if self.list(project).iter().any(|info| info.template.same(template.kind, &template.name)) {
                return Err(t!("workbench-file-templates-exists", name = template.name.as_str()));
            }
            stored.retain(|known| !known.same(template.kind, previous));
        }
        let same_as_builtin = builtin.iter().any(|(known, _)| *known == template);
        stored.retain(|known| !known.same(template.kind, &template.name));
        if !same_as_builtin {
            stored.push(template);
        }
        self.store(scheme, project, stored)
    }

    fn store(&self, scheme: Scheme, project: Option<&Path>, templates: Vec<FileTemplate>) -> Result<(), String> {
        let folder = match (scheme, project) {
            (Scheme::Default, _) => self.inner().data.as_deref().map(Folder::ide),
            (Scheme::Project, Some(project)) => Some(Folder::project(project)),
            (Scheme::Project, None) => return Err(t!("workbench-error-no-project")),
        };
        if let Some(folder) = &folder {
            storage::write(folder, &templates)
                .map_err(|error| t!("workbench-file-templates-save-failed", detail = error.to_string()))?;
            if scheme == Scheme::Project {
                std::fs::create_dir_all(&folder.templates)
                    .map_err(|error| t!("workbench-file-templates-save-failed", detail = error.to_string()))?;
            }
        }
        let mut inner = self.inner();
        if scheme == Scheme::Default {
            inner.ide = templates;
        }
        inner.generation += 1;
        Ok(())
    }

    pub fn use_scheme(&self, project: &Path, scheme: Scheme) -> Result<(), String> {
        let failed = |error: std::io::Error| t!("workbench-file-templates-save-failed", detail = error.to_string());
        if scheme == Scheme::Project
            && std::fs::symlink_metadata(project.join(".idea")).is_ok_and(|meta| meta.file_type().is_symlink())
        {
            let path = project.join(".idea").display().to_string();
            return Err(t!("workbench-file-templates-linked", path = path));
        }
        let (data, json) = {
            let mut inner = self.inner();
            inner.schemes.insert(scheme_key(project), scheme);
            inner.generation += 1;
            (inner.data.clone(), serde_json::to_string_pretty(&inner.schemes).unwrap_or_default())
        };
        if let Some(data) = data {
            std::fs::create_dir_all(&data).map_err(failed)?;
            zen_vfs::write_atomic(&data.join(SCHEMES), json.as_bytes()).map_err(|error| failed(error.source))?;
        }
        Ok(())
    }

    pub fn remove(&self, scheme: Scheme, project: Option<&Path>, kind: TemplateKind, name: &str) -> Result<(), String> {
        if self.builtin().iter().any(|(known, _)| known.same(kind, name)) {
            return Err(t!("workbench-file-templates-builtin", name = name));
        }
        let mut stored = self.stored(scheme, project);
        let before = stored.len();
        stored.retain(|known| !known.same(kind, name));
        if stored.len() == before {
            return Err(t!("workbench-file-templates-unknown", name = name));
        }
        self.store(scheme, project, stored)
    }

    pub fn reset(&self, scheme: Scheme, project: Option<&Path>, name: Option<&str>) -> Result<usize, String> {
        let builtin = self.builtin();
        let mut stored = self.stored(scheme, project);
        let before = stored.len();
        stored.retain(|known| {
            let chosen = name.is_none_or(|name| known.name == name);
            !(chosen && builtin.iter().any(|(declared, _)| declared.same(known.kind, &known.name)))
        });
        let reset = before - stored.len();
        if reset > 0 {
            self.store(scheme, project, stored)?;
        }
        Ok(reset)
    }
}

fn includes_of<'a>(templates: impl Iterator<Item = &'a FileTemplate>) -> HashMap<String, String> {
    let mut includes = HashMap::new();
    for template in templates.filter(|template| template.kind == TemplateKind::Include) {
        includes.insert(template.full_name(), template.text.clone());
        includes.entry(template.name.clone()).or_insert_with(|| template.text.clone());
    }
    includes
}

fn analyse(
    template: &FileTemplate,
    includes: &HashMap<String, String>,
    declared: &[String],
) -> (Vec<String>, Vec<Problem>) {
    let find = |name: &str| includes.get(name).cloned();
    let mut asked = Vec::new();
    let pattern = velocity::references(&template.file_name, &find);
    if template.kind == TemplateKind::File
        && (template.file_name.trim().is_empty() || pattern.iter().any(|name| name == "NAME"))
    {
        asked.push("NAME".to_owned());
    }
    let mut read = pattern;
    read.extend(velocity::references(&template.text, &find));
    for child in &template.children {
        read.extend(velocity::references(&child.file_name, &find));
        read.extend(velocity::references(&child.text, &find));
    }
    for name in read {
        if !variables::is_predefined(&name) && !declared.contains(&name) && !asked.contains(&name) {
            asked.push(name);
        }
    }
    let empty = HashMap::new();
    let mut problems: Vec<Problem> = velocity::render(&template.file_name, &empty, &find).problems;
    problems.extend(velocity::render(&template.text, &empty, &find).problems);
    for child in &template.children {
        problems.extend(velocity::render(&child.file_name, &empty, &find).problems);
        problems.extend(velocity::render(&child.text, &empty, &find).problems);
    }
    (asked, problems)
}

#[cfg(test)]
mod tests;

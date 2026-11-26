use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session, Settings};
use zen_i18n::t;
pub use zen_project::code_style::{CodeStyleDeclaration, Values};
use zen_syntax::Arranger;

use crate::{Workbench, code_style, project_settings};

pub const REARRANGE_ON_REFORMAT: &str = "editor.code.reformat.rearrange";

pub const ARRANGEMENT: &str = "arrangement";

pub type PluginStyles = Arc<dyn Fn() -> Vec<CodeStyleDeclaration> + Send + Sync>;

#[derive(Clone, Default)]
pub struct CodeStyles {
    modules: Vec<CodeStyleDeclaration>,
    plugins: Option<PluginStyles>,
    arrangers: Vec<Arc<dyn Arranger>>,
}

impl CodeStyles {
    pub fn new(modules: Vec<CodeStyleDeclaration>, arrangers: Vec<Arc<dyn Arranger>>) -> Self {
        Self { modules, plugins: None, arrangers }
    }

    pub fn with_plugins(mut self, plugins: PluginStyles) -> Self {
        self.plugins = Some(plugins);
        self
    }

    pub fn declarations(&self) -> Vec<CodeStyleDeclaration> {
        let mut all = self.modules.clone();
        for declaration in self.plugins.as_ref().map(|plugins| plugins()).unwrap_or_default() {
            if declaration.check().is_ok() && !all.iter().any(|known| known.language == declaration.language) {
                all.push(declaration);
            }
        }
        all
    }

    pub fn declaration(&self, language: &str) -> Option<CodeStyleDeclaration> {
        self.declarations().into_iter().find(|declaration| declaration.language == language)
    }

    pub fn arranger_for(&self, path: &Path) -> Option<Arc<dyn Arranger>> {
        let name = path.file_name()?.to_string_lossy();
        self.arrangers.iter().find(|arranger| arranger.handles(&name)).cloned()
    }
}

pub fn key(language: &str, id: &str) -> String {
    code_style::key(language, id)
}

pub fn apply(settings: &mut Settings, values: &BTreeMap<String, Values>) {
    for (language, values) in values {
        for (id, value) in values {
            let _ = settings.set(&key(language, id), value);
        }
    }
}

pub fn ide_values(settings: &Settings, declaration: &CodeStyleDeclaration) -> Values {
    let given: Values = declaration
        .options
        .iter()
        .filter_map(|option| Some((option.id.clone(), settings.get::<Value>(&key(&declaration.language, &option.id))?)))
        .collect();
    declaration.complete(&given)
}

pub fn arrangement_order(settings: &Settings, declaration: &CodeStyleDeclaration) -> Vec<String> {
    let declared: Vec<String> = declaration.arrangement.iter().map(|entry| entry.id.clone()).collect();
    let chosen = settings.get::<Vec<String>>(&key(&declaration.language, ARRANGEMENT)).unwrap_or_default();
    let mut order: Vec<String> = chosen.into_iter().filter(|id| declared.contains(id)).collect();
    order.dedup();
    for id in declared {
        if !order.contains(&id) {
            order.push(id);
        }
    }
    order
}

fn project_root(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

pub fn config_path(root: &Path, declaration: &CodeStyleDeclaration) -> Option<PathBuf> {
    let file = declaration.config_file.as_ref()?;
    let found = file.names.iter().map(|name| root.join(name)).find(|path| path.is_file());
    found.or_else(|| file.names.first().map(|name| root.join(name)))
}

fn uses_project_file(session: &Session, declaration: &CodeStyleDeclaration) -> bool {
    declaration.config_file.is_some() && project_settings::uses_project_code_style(session)
}

pub fn project_values(session: &Session, declaration: &CodeStyleDeclaration) -> Option<(PathBuf, Values)> {
    let path = config_path(&project_root(session)?, declaration)?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    Some((path, declaration.complete(&declaration.read_config(&text))))
}

pub fn effective_values(session: &Session, declaration: &CodeStyleDeclaration) -> Values {
    let ide = ide_values(session.settings(), declaration);
    if uses_project_file(session, declaration)
        && let Some((_, project)) = project_values(session, declaration)
    {
        return declaration.with_config_values(&ide, &project);
    }
    ide
}

fn declarations(session: &Session) -> Vec<CodeStyleDeclaration> {
    session.state::<CodeStyles>().map(CodeStyles::declarations).unwrap_or_default()
}

pub fn formatting_options(session: &Session, path: &Path) -> serde_json::Map<String, Value> {
    let language = code_style::language_of(path);
    declarations(session)
        .into_iter()
        .find(|declaration| declaration.language == language)
        .map(|declaration| declaration.formatting_options(&effective_values(session, &declaration)))
        .unwrap_or_default()
}

pub type ServerStyles = BTreeMap<String, BTreeMap<String, Value>>;

pub fn server_sections(session: &Session) -> ServerStyles {
    let mut styles = ServerStyles::new();
    for declaration in declarations(session) {
        let project = uses_project_file(session, &declaration);
        let values = if project {
            effective_values(session, &declaration)
        } else {
            ide_values(session.settings(), &declaration)
        };
        let mut sections = declaration.server_settings(&values);
        if !project && let Some((section, over)) = declaration.config_arguments(&values) {
            let target = sections.entry(section).or_insert_with(|| Value::Object(Default::default()));
            zen_project::code_style::merge(target, &over);
        }
        if let Some(location) = declaration.config_file.as_ref().and_then(|file| file.location.as_ref())
            && let Some(file) = located_file(session, &declaration, project, &values)
        {
            let target = sections.entry(location.section.clone()).or_insert_with(|| Value::Object(Default::default()));
            zen_project::code_style::set_path(target, &location.path, Value::from(file.to_string_lossy().into_owned()));
        }
        if !sections.is_empty() {
            styles.insert(declaration.language.clone(), sections);
        }
    }
    styles
}

fn ide_folder(session: &Session, declaration: &CodeStyleDeclaration) -> Option<PathBuf> {
    let data = session.state::<crate::workspaces::Workspaces>()?.data_dir()?;
    Some(data.join("code-style").join(&declaration.language))
}

fn stored_copy(folder: &Path, name: &Path, text: &str) -> Option<PathBuf> {
    use sha2::Digest;
    let stem = name.file_stem()?.to_string_lossy();
    let extension = name.extension().map(|extension| format!(".{}", extension.to_string_lossy())).unwrap_or_default();
    let digest = hex::encode(sha2::Sha256::digest(text.as_bytes()));
    let path = folder.join(format!("{stem}-{}{extension}", &digest[..16]));
    if !path.is_file() {
        std::fs::create_dir_all(folder).ok()?;
        zen_vfs::write_atomic(&path, text.as_bytes()).ok()?;
    }
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let other = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let ours = file_name.starts_with(&format!("{stem}-")) && file_name.ends_with(&extension);
        if ours && other != path {
            let _ = std::fs::remove_file(other);
        }
    }
    Some(path)
}

fn located_file(
    session: &Session,
    declaration: &CodeStyleDeclaration,
    project: bool,
    values: &Values,
) -> Option<PathBuf> {
    let name = PathBuf::from(declaration.config_file.as_ref()?.names.first()?);
    let folder = ide_folder(session, declaration);
    if project {
        let path = config_path(&project_root(session)?, declaration).filter(|path| path.is_file())?;
        let text = std::fs::read_to_string(&path).ok()?;
        let own = |folder: PathBuf| {
            use sha2::Digest;
            let root = path.to_string_lossy();
            folder.join(&hex::encode(sha2::Sha256::digest(root.as_bytes()))[..16])
        };
        return Some(folder.map(own).and_then(|folder| stored_copy(&folder, &name, &text)).unwrap_or(path));
    }
    if !declaration.differs_in_config(values) {
        return None;
    }
    stored_copy(&folder?, &name, &declaration.write_config("", values))
}

pub fn editor_config_pairs(session: &Session, language: &str) -> Vec<(String, String)> {
    declarations(session)
        .into_iter()
        .find(|declaration| declaration.language == language)
        .map(|declaration| declaration.editor_config_pairs(&effective_values(session, &declaration)))
        .unwrap_or_default()
}

pub fn declared_languages(session: &Session) -> Vec<String> {
    declarations(session).into_iter().map(|declaration| declaration.language).collect()
}

fn char_edit(old: &str, new: &str) -> Option<(std::ops::Range<usize>, String)> {
    if old == new {
        return None;
    }
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let prefix = old_chars.iter().zip(&new_chars).take_while(|(a, b)| a == b).count();
    let room = old_chars.len().min(new_chars.len()) - prefix;
    let suffix = old_chars.iter().rev().zip(new_chars.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let inserted: String = new_chars[prefix..new_chars.len() - suffix].iter().collect();
    Some((prefix..old_chars.len() - suffix, inserted))
}

pub(crate) fn rearrange(session: &mut Session, path: &Path) -> Result<bool, String> {
    let styles = session.state::<CodeStyles>().cloned().unwrap_or_default();
    let arranger = styles.arranger_for(path).ok_or_else(|| t!("workbench-rearrange-unsupported"))?;
    let order = styles
        .declaration(arranger.language())
        .map(|declaration| arrangement_order(session.settings(), &declaration))
        .unwrap_or_default();
    let workbench = session.state_mut::<Workbench>();
    let document = workbench
        .documents
        .iter_mut()
        .find(|document| document.path() == path)
        .ok_or_else(|| t!("workbench-error-no-document"))?;
    if let Some(reason) = document.read_only() {
        return Err(reason.to_owned());
    }
    let text = document.text().to_owned();
    let Some(arranged) = arranger.arrange(&text, &order) else { return Ok(false) };
    let Some(edit) = char_edit(&text, &arranged) else { return Ok(false) };
    Ok(document.replace_ranges(vec![edit]))
}

pub(crate) fn before_reformat(session: &mut Session, path: &Path) {
    let styles = session.state::<CodeStyles>().cloned().unwrap_or_default();
    let Some(arranger) = styles.arranger_for(path) else { return };
    let values = styles
        .declaration(arranger.language())
        .map(|declaration| effective_values(session, &declaration))
        .unwrap_or_default();
    let Some(document) = session.state_mut::<Workbench>().documents.iter_mut().find(|document| document.path() == path)
    else {
        return;
    };
    if document.read_only().is_some() {
        return;
    }
    let text = document.text().to_owned();
    let option = |id: &str| values.get(id).and_then(Value::as_bool).unwrap_or(false);
    if let Some(edit) = arranger.before_reformat(&text, &option).and_then(|tidied| char_edit(&text, &tidied)) {
        document.replace_ranges(vec![edit]);
    }
}

/// A language's code style: what it declares and the values in use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LanguageStyle {
    pub declaration: CodeStyleDeclaration,
    /// The values Editor → Code Style keeps for the IDE (the Default scheme).
    pub ide: Values,
    /// The values of the project's configuration file, when the language has one and a project is open.
    pub project: Option<Values>,
    /// The project's configuration file, which may not exist yet.
    pub project_file: Option<PathBuf>,
    /// Whether formatting uses the project's file (the Project scheme) instead of the IDE's values.
    pub uses_project: bool,
    /// The order Rearrange Code puts declarations in, by arrangement entry id.
    pub arrangement: Vec<String>,
}

pub struct ListStyles;

impl Command for ListStyles {
    type Params = NoParams;
    type Output = Vec<LanguageStyle>;
    const ID: &'static str = "editor.codeStyle.list";
    const TITLE: &'static str = "List Code Styles";
    const DESCRIPTION: &'static str = "Lists the languages with a Code Style page of their own (Settings → Editor → \
         Code Style → <language>): the options a language module or plug-in declares (id, tab, kind, default, the \
         formatting option, server setting, configuration file key and EditorConfig property each maps to), the \
         preview text, the IDE's values (set_settings codeStyleOptions changes them), the project's configuration \
         file and its values (editor.codeStyle.writeProject changes them), and the arrangement order of Rearrange \
         Code.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<LanguageStyle>, CommandError> {
        Ok(declarations(session)
            .into_iter()
            .map(|declaration| {
                let project = project_values(session, &declaration)
                    .filter(|(path, _)| crate::hidden_files::guard_read(session, &[path]).is_ok());
                LanguageStyle {
                    ide: ide_values(session.settings(), &declaration),
                    project_file: project.as_ref().map(|(path, _)| path.clone()),
                    project: project.map(|(_, values)| values),
                    uses_project: uses_project_file(session, &declaration),
                    arrangement: arrangement_order(session.settings(), &declaration),
                    declaration,
                }
            })
            .collect())
    }
}

/// The parameters of `editor.codeStyle.writeProject`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteProjectParams {
    /// The language, as `rust`.
    pub language: String,
    /// The values by option id; options left out keep what the file has.
    pub values: Values,
}

/// What `editor.codeStyle.writeProject` wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteProjectResult {
    pub file: PathBuf,
    /// Whether the file's text changed.
    pub changed: bool,
}

pub struct WriteProjectStyle;

impl Command for WriteProjectStyle {
    type Params = WriteProjectParams;
    type Output = WriteProjectResult;
    const ID: &'static str = "editor.codeStyle.writeProject";
    const TITLE: &'static str = "Write Project Code Style";
    const DESCRIPTION: &'static str = "Writes code style values of a language that keeps them in a configuration \
         file of its formatter (rustfmt.toml for Rust, the Eclipse formatter profile .zen/java-formatter.xml for \
         Java, the [*.{kt,kts}] section of .editorconfig for Kotlin) into that file in the project, creating it when \
         it has none. Only the keys the language declares change: other lines, comments, tables, sections and \
         settings stay; in a TOML or EditorConfig file a value at its default is added only when the file already \
         sets the key, while a formatter profile gets every declared setting. Values that are not valid are \
         refused.";

    fn run(&self, session: &mut Session, params: WriteProjectParams) -> Result<WriteProjectResult, CommandError> {
        let declaration =
            declarations(session).into_iter().find(|declaration| declaration.language == params.language).ok_or_else(
                || CommandError::new(t!("workbench-code-style-unknown", language = params.language.as_str())),
            )?;
        let root = project_root(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let path = config_path(&root, &declaration).ok_or_else(|| {
            CommandError::new(t!("workbench-code-style-no-file", language = params.language.as_str()))
        })?;
        for (id, value) in &params.values {
            let valid =
                declaration.option(id).is_some_and(|option| option.config_key.is_some() && option.accepts(value));
            if !valid {
                return Err(CommandError::new(t!("workbench-code-style-invalid", option = id.as_str())));
            }
        }
        if crate::guard::refuses_in(Some(&root), &path) {
            return Err(CommandError::new(crate::guard::refusal(&path)));
        }
        crate::hidden_files::guard_change(session, &root, &[&path])?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| CommandError::new(error.to_string()))?;
        }
        let changed =
            crate::disk::edit_file(session, &path, |text| Some(declaration.write_config(text, &params.values)))?;
        Ok(WriteProjectResult { file: path, changed })
    }
}

/// The parameters of `editor.code.rearrange`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RearrangeParams {
    /// The open file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// What Rearrange Code did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RearrangeResult {
    /// Whether the file's text changed.
    pub changed: bool,
    /// Why nothing was done, in words for the user.
    pub error: Option<String>,
}

pub struct RearrangeCode;

impl Command for RearrangeCode {
    type Params = RearrangeParams;
    type Output = RearrangeResult;
    const ID: &'static str = "editor.code.rearrange";
    const TITLE: &'static str = "Rearrange Code";
    const DESCRIPTION: &'static str = "Reorders the declarations of an open file (the active one by default) by its \
         language's arrangement rules, as one undo step: in Go the top-level constants, variables, types and \
         functions, in the order Code Style → Go → Arrangement sets, keeping the order of declarations of the same \
         kind and the comments above each. A file it cannot read safely is left as it is. Reformat Code does it \
         first when editor.code.reformat.rearrange is on or its rearrange parameter is true.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| !workbench.documents().is_empty())
    }

    fn run(&self, session: &mut Session, params: RearrangeParams) -> Result<RearrangeResult, CommandError> {
        let path = params
            .path
            .or_else(|| session.state::<Workbench>()?.active_document().map(|document| document.path().to_owned()))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let path = session.state_mut::<Workbench>().resolve(&path);
        crate::hidden_files::guard_read(session, &[&path])?;
        Ok(match rearrange(session, &path) {
            Ok(changed) => RearrangeResult { changed, error: None },
            Err(error) => RearrangeResult { changed: false, error: Some(error) },
        })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListStyles)?;
    registry.register(WriteProjectStyle)?;
    registry.register(RearrangeCode)?;
    Ok(())
}

#[cfg(test)]
mod tests;

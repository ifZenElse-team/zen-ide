use crate::project_settings::ProjectSettingsState;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::variables::{self, Context, Moment};
use super::velocity::{self, Problem};
use super::{FileTemplate, FileTemplates, Scheme, TemplateInfo, TemplateKind, VariableInfo, includes_of};
use crate::{Workbench, code_style, display, history, project_settings, templates};

fn store(session: &Session) -> FileTemplates {
    session.state::<FileTemplates>().cloned().unwrap_or_default()
}

fn project(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

fn scheme_or_active(session: &Session, scheme: Option<Scheme>, project: Option<&Path>) -> Scheme {
    scheme.unwrap_or_else(|| store(session).scheme_of(project))
}

/// The parameters of `file.templates.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ListParams {
    /// Only the templates of this tab: `file`, `include` or `code`.
    pub kind: Option<TemplateKind>,
    /// The scheme whose templates are listed; the scheme in use by default.
    pub scheme: Option<Scheme>,
}

/// What `file.templates.list` gives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateList {
    /// The scheme in use: the one chosen last, else `project` when the project has `.idea/fileTemplates`.
    pub scheme: Scheme,
    /// The templates in use.
    pub templates: Vec<TemplateInfo>,
}

pub struct ListTemplates;

impl Command for ListTemplates {
    type Params = ListParams;
    type Output = TemplateList;
    const ID: &'static str = "file.templates.list";
    const TITLE: &'static str = "File Templates";
    const DESCRIPTION: &'static str = "The file templates in use: the project's over the IDE's over the built-in ones of the \
         modules and plug-ins, each with where it comes from, whether it is changed, the variables a new file asks for \
         and what in it cannot be rendered.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<TemplateList, CommandError> {
        let project = project(session);
        let scheme = scheme_or_active(session, params.scheme, project.as_deref());
        let templates = store(session)
            .list_in(scheme, project.as_deref())
            .into_iter()
            .filter(|info| params.kind.is_none_or(|kind| info.template.kind == kind))
            .collect();
        Ok(TemplateList { scheme: store(session).scheme_of(project.as_deref()), templates })
    }
}

/// The parameters of `file.templates.variables`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VariablesParams {
    /// The folder of the new file, absolute or relative to the project; the project folder by default.
    pub directory: Option<PathBuf>,
    /// The new file's name, for `NAME` and `FILE_NAME`.
    pub name: Option<String>,
}

pub struct ListVariables;

impl Command for ListVariables {
    type Params = VariablesParams;
    type Output = Vec<VariableInfo>;
    const ID: &'static str = "file.templates.variables";
    const TITLE: &'static str = "File Template Variables";
    const DESCRIPTION: &'static str = "The variables every file template can use: the predefined ones (NAME, FILE_NAME, \
         DIR_PATH, PROJECT_NAME, PRODUCT_NAME, USER, DATE, TIME and the parts of the date and time, DS) and those \
         modules and plug-ins give, with their values for a new file in a folder.";

    fn run(&self, session: &mut Session, params: VariablesParams) -> Result<Vec<VariableInfo>, CommandError> {
        let project = project(session);
        let directory = match (&params.directory, session.state::<Workbench>()) {
            (Some(directory), Some(workbench)) => workbench.resolve(directory),
            (Some(directory), None) => directory.clone(),
            (None, _) => project.clone().unwrap_or_default(),
        };
        let name = params.name.unwrap_or_default();
        let templates = store(session);
        let context = Context {
            name: &name,
            file_name: &name,
            directory: &directory,
            project: project.as_deref(),
            product: &templates.product(),
            now: Moment::now(),
        };
        let values = variables::predefined(&context);
        let mut listed: Vec<VariableInfo> = variables::PREDEFINED
            .iter()
            .map(|(name, description)| VariableInfo {
                name: (*name).to_owned(),
                description: zen_i18n::text(description, &[]),
                value: values.get(*name).cloned().unwrap_or_default(),
                origin: None,
            })
            .collect();
        listed.extend(templates.declared_variables(&directory).into_iter().map(|(variable, origin)| VariableInfo {
            description: if variable.description.is_empty() {
                String::new()
            } else {
                zen_i18n::text(&variable.description, &[])
            },
            name: variable.name,
            value: variable.value.unwrap_or_default(),
            origin: Some(origin),
        }));
        Ok(listed)
    }
}

/// The parameters of `file.templates.render` and `file.templates.create`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderParams {
    /// The template's name, on the Files tab.
    pub template: String,
    /// The folder of the new file, absolute or relative to the project; the project folder by default.
    #[serde(default)]
    pub directory: Option<PathBuf>,
    /// The file name typed for it (`NAME`), with or without the template's extension; may start with folders.
    #[serde(default)]
    pub name: Option<String>,
    /// Values of the variables the template asks for, by name.
    #[serde(default)]
    pub variables: HashMap<String, String>,
    /// For `file.templates.create`: whether to open the new file in the editor; true by default.
    #[serde(default)]
    pub open: Option<bool>,
}

/// What `file.templates.render` gives: the file a template would create, without creating it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Rendered {
    /// The template's name.
    pub template: String,
    /// The new file.
    pub path: PathBuf,
    /// Its text, the variables without a value left as written.
    pub text: String,
    /// The variables the template asks for, in order; `NAME` is the file name.
    pub asked: Vec<String>,
    /// Those of them still without a value.
    pub missing: Vec<String>,
    /// What cannot be rendered, in the text or the file name.
    pub problems: Vec<Problem>,
    /// The files of the child templates, created beside it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<RenderedChild>,
}

/// A file a child template would create.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderedChild {
    /// The new file.
    pub path: PathBuf,
    /// Its text.
    pub text: String,
}

struct Plan {
    template: FileTemplate,
    rendered: Rendered,
}

fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other.as_os_str()),
        }
    }
    normal
}

fn plan(session: &Session, params: &RenderParams) -> Result<Plan, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let project = workbench.project().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?.to_owned();
    let directory = crate::project_folder(workbench, params.directory.as_deref().unwrap_or(Path::new("")))?;
    let templates = store(session);
    let all = templates.list(Some(&project));
    let info = all
        .iter()
        .find(|info| info.template.kind == TemplateKind::File && info.template.name == params.template)
        .ok_or_else(|| CommandError::new(t!("workbench-file-templates-unknown", name = params.template.as_str())))?;
    let template = info.template.clone();
    let includes = includes_of(all.iter().map(|info| &info.template));
    let find = |name: &str| includes.get(name).cloned();

    let typed = params.name.clone().or_else(|| params.variables.get("NAME").cloned()).unwrap_or_default();
    let typed = typed.trim();
    let typed_path = Path::new(typed);
    if !typed.is_empty() && !typed_path.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err(CommandError::new(t!("workbench-error-bad-name", name = typed)));
    }
    let (folders, last) = match typed_path.parent() {
        Some(parent) => {
            (parent.to_owned(), typed_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        }
        None => (PathBuf::new(), typed.to_owned()),
    };
    let dotted = format!(".{}", template.extension);
    let name = match last.strip_suffix(&dotted) {
        Some(stem) if !template.extension.is_empty() && !stem.is_empty() => stem.to_owned(),
        _ => last,
    };
    let base = directory.join(folders);
    let with_extension = |stem: &str| {
        if template.extension.is_empty() { stem.to_owned() } else { format!("{stem}.{}", template.extension) }
    };
    let now = Moment::now();
    let product = templates.product();
    let values_for = |file_name: &str, folder: &Path| {
        let context =
            Context { name: &name, file_name, directory: folder, project: Some(&project), product: &product, now };
        let mut values = variables::predefined(&context);
        for (variable, _) in templates.declared_variables(folder) {
            if let Some(value) = variable.value {
                values.entry(variable.name).or_insert(value);
            }
        }
        for (key, value) in &params.variables {
            if !matches!(key.as_str(), "NAME" | "FILE_NAME" | "DIR_PATH") {
                values.insert(key.clone(), value.clone());
            }
        }
        values
    };

    let mut problems = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let stem = if template.file_name.trim().is_empty() {
        name.clone()
    } else {
        let rendered = velocity::render(&template.file_name, &values_for(&with_extension(&name), &base), &find);
        problems.extend(rendered.problems);
        missing.extend(rendered.missing);
        let stem = rendered.text.trim().to_owned();
        if stem.is_empty() || !Path::new(&stem).components().all(|part| matches!(part, Component::Normal(_))) {
            return Err(CommandError::new(t!("workbench-file-templates-bad-file-name", name = stem.as_str())));
        }
        stem
    };
    let path = normalize(&base.join(with_extension(&stem)));
    if !crate::inside_project(&project, &path) || path == project {
        return Err(CommandError::new(t!("workbench-error-outside-project", path = display(&path))));
    }
    let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = path.parent().unwrap_or(&project).to_owned();
    let values = values_for(&file_name, &folder);
    let text = velocity::render(&template.text, &values, &find);
    problems.extend(text.problems);
    missing.extend(text.missing);
    let mut children = Vec::new();
    let mut child_values = Vec::new();
    for child in &template.children {
        let with_child_extension = |stem: &str| {
            if child.extension.is_empty() { stem.to_owned() } else { format!("{stem}.{}", child.extension) }
        };
        let child_stem = if child.file_name.trim().is_empty() {
            stem.clone()
        } else {
            let rendered = velocity::render(&child.file_name, &values_for(&with_child_extension(&name), &base), &find);
            problems.extend(rendered.problems);
            missing.extend(rendered.missing);
            let child_stem = rendered.text.trim().to_owned();
            if child_stem.is_empty()
                || !Path::new(&child_stem).components().all(|part| matches!(part, Component::Normal(_)))
            {
                return Err(CommandError::new(t!(
                    "workbench-file-templates-bad-file-name",
                    name = child_stem.as_str()
                )));
            }
            child_stem
        };
        let child_path = normalize(&base.join(with_child_extension(&child_stem)));
        let taken = child_path == path || children.iter().any(|known: &RenderedChild| known.path == child_path);
        if !crate::inside_project(&project, &child_path) || child_path == project || taken {
            return Err(CommandError::new(t!("workbench-error-outside-project", path = display(&child_path))));
        }
        let child_file = child_path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let child_folder = child_path.parent().unwrap_or(&project).to_owned();
        let values = values_for(&child_file, &child_folder);
        let rendered = velocity::render(&child.text, &values, &find);
        problems.extend(rendered.problems);
        missing.extend(rendered.missing);
        child_values.push(values);
        children.push(RenderedChild { path: child_path, text: rendered.text });
    }
    let mut seen = Vec::new();
    missing.retain(|name| {
        let first = !seen.contains(name);
        seen.push(name.clone());
        first
    });
    let asked = info.asked.clone();
    let known =
        |asked: &String| values.contains_key(asked) || child_values.iter().any(|values| values.contains_key(asked));
    let mut still: Vec<String> =
        asked.iter().filter(|asked| if *asked == "NAME" { name.is_empty() } else { !known(asked) }).cloned().collect();
    for name in missing {
        if !still.contains(&name) && !variables::is_predefined(&name) {
            still.push(name);
        }
    }
    let rendered =
        Rendered { template: template.name.clone(), path, text: text.text, asked, missing: still, problems, children };
    Ok(Plan { template, rendered })
}

pub struct RenderTemplate;

impl Command for RenderTemplate {
    type Params = RenderParams;
    type Output = Rendered;
    const ID: &'static str = "file.templates.render";
    const TITLE: &'static str = "Preview File Template";
    const DESCRIPTION: &'static str = "Renders a file template for a new file in a project folder without creating it: \
         the path, the text, the variables it asks for, those still missing and what cannot be rendered.";

    fn run(&self, session: &mut Session, params: RenderParams) -> Result<Rendered, CommandError> {
        Ok(plan(session, &params)?.rendered)
    }
}

/// What `file.templates.create` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    /// The new file.
    pub path: PathBuf,
    /// Its text as written.
    pub text: String,
    /// With Enable Live Templates, the variables Tab goes through in the open file, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub live_variables: Vec<String>,
    /// Whether its language server reformatted it.
    pub reformatted: bool,
    /// The files of the child templates, created beside it and opened behind it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<PathBuf>,
}

fn reindent(text: &str, unit: &str) -> String {
    if unit == "\t" {
        return text.to_owned();
    }
    text.split('\n')
        .map(|line| {
            let tabs = line.chars().take_while(|c| *c == '\t').count();
            format!("{}{}", unit.repeat(tabs), &line[tabs..])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn problems_message(problems: &[Problem]) -> String {
    problems
        .iter()
        .take(3)
        .map(|problem| match &problem.include {
            Some(include) => format!("{include} {}:{}: {}", problem.line, problem.column, problem.message),
            None => format!("{}:{}: {}", problem.line, problem.column, problem.message),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn write_new(session: &Session, file: &Path, text: &str) -> Result<(), CommandError> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            CommandError::new(t!("workbench-error-create", path = display(parent), error = error.to_string()))
        })?;
    }
    zen_vfs::create_file(file).map_err(|error| {
        CommandError::new(t!("workbench-error-create", path = display(file), error = error.source.to_string()))
    })?;
    let bytes = crate::file_encodings::new_file_bytes(session, file, text);
    zen_vfs::write_atomic(file, &bytes).map_err(|error| {
        let _ = std::fs::remove_file(file);
        CommandError::new(t!("workbench-error-create", path = display(file), error = error.source.to_string()))
    })
}

pub struct CreateFromTemplate;

impl Command for CreateFromTemplate {
    type Params = RenderParams;
    type Output = Created;
    const ID: &'static str = "file.templates.create";
    const TITLE: &'static str = "New File from Template";
    const DESCRIPTION: &'static str = "Creates a file from a file template in a project folder and opens it: `name` is the \
         file name asked for (NAME), `variables` the values of the variables the template asks for. Fails, creating \
         nothing, when one is missing, when the template cannot be rendered or when the file exists.";

    fn run(&self, session: &mut Session, params: RenderParams) -> Result<Created, CommandError> {
        let Plan { template, rendered } = plan(session, &params)?;
        if !rendered.problems.is_empty() {
            return Err(CommandError::new(t!(
                "workbench-file-templates-cannot-render",
                name = template.name.as_str(),
                problems = problems_message(&rendered.problems)
            )));
        }
        if !rendered.missing.is_empty() {
            return Err(CommandError::new(t!(
                "workbench-file-templates-missing",
                names = rendered.missing.join(", "),
                count = rendered.missing.len()
            )));
        }
        let path = rendered.path;
        let unit = if template.reformat {
            code_style::indent_unit(project_settings::code_style_for_path(session, &path))
        } else {
            "\t".to_owned()
        };
        let live = template.live_templates.then(|| {
            let variables = templates::variables_of(&rendered.text)
                .into_iter()
                .map(|name| templates::Variable { default: name.clone(), name, ..Default::default() })
                .collect();
            let live = templates::Template {
                language: templates::OTHER.to_owned(),
                abbreviation: template.name.clone(),
                description: String::new(),
                text: rendered.text.clone(),
                variables,
                imports: Vec::new(),
                contexts: Vec::new(),
                enabled: true,
            };
            templates::expand(&live, "", &unit)
        });
        let text = match &live {
            Some(expanded) => expanded.text.clone(),
            None if template.reformat => reindent(&rendered.text, &unit),
            None => rendered.text.clone(),
        };
        let mut files = Vec::with_capacity(1 + rendered.children.len());
        for (child, shaped) in template.children.iter().zip(&rendered.children) {
            let unit = if child.reformat {
                code_style::indent_unit(project_settings::code_style_for_path(session, &shaped.path))
            } else {
                "\t".to_owned()
            };
            let text = if child.live_templates {
                let live = templates::Template {
                    language: templates::OTHER.to_owned(),
                    abbreviation: template.name.clone(),
                    description: String::new(),
                    text: shaped.text.clone(),
                    variables: Vec::new(),
                    imports: Vec::new(),
                    contexts: Vec::new(),
                    enabled: true,
                };
                templates::expand(&live, "", &unit).text
            } else if child.reformat {
                reindent(&shaped.text, &unit)
            } else {
                shaped.text.clone()
            };
            files.push((shaped.path.clone(), text));
        }
        files.push((path.clone(), text.clone()));
        if let Some((taken, _)) = files.iter().find(|(path, _)| std::fs::symlink_metadata(path).is_ok()) {
            let error = std::io::Error::from(std::io::ErrorKind::AlreadyExists).to_string();
            return Err(CommandError::new(t!("workbench-error-create", path = display(taken), error = error)));
        }
        let mut written: Vec<&Path> = Vec::with_capacity(files.len());
        for (file, text) in &files {
            if let Err(error) = write_new(session, file, text) {
                for done in written {
                    let _ = std::fs::remove_file(done);
                }
                return Err(error);
            }
            written.push(file);
        }
        let children: Vec<PathBuf> = rendered.children.iter().map(|child| child.path.clone()).collect();
        let mut created =
            Created { path: path.clone(), text, live_variables: Vec::new(), reformatted: false, children };
        if !params.open.unwrap_or(true) {
            session.state_mut::<Workbench>().revision += 1;
            return Ok(created);
        }
        let origin = history::caret(session);
        for child in &created.children {
            crate::open_created(session, child, None)?;
        }
        crate::open_created(session, &path, origin)?;
        if let Some(expanded) = &live
            && let Some(document) = session.state_mut::<Workbench>().active_document_mut()
        {
            document.template = templates::TemplateSession::new(&template.name, expanded, 0);
            if document.template.is_some() {
                templates::select_current(document);
                created.live_variables = expanded.variables.iter().map(|(name, _)| name.clone()).collect();
            } else {
                document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(expanded.end)));
            }
        }
        if template.reformat && created.live_variables.is_empty() {
            let result = crate::language::format::reformat(session, &path, 500);
            created.reformatted = result.is_ok_and(|result| result.changed);
            if created.reformatted
                && let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document)
            {
                created.text = document.text().to_owned();
            }
        }
        Ok(created)
    }
}

/// The parameters of `file.templates.set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SetParams {
    /// The template, added or replacing the one with the same tab and name.
    pub template: FileTemplate,
    /// Where to keep it: `default` (the IDE) or `project` (`.idea/fileTemplates`); the scheme in use by default.
    #[serde(default)]
    pub scheme: Option<Scheme>,
    /// The name the template had, when it is renamed.
    #[serde(default)]
    pub previous: Option<String>,
}

pub struct SetTemplate;

impl Command for SetTemplate {
    type Params = SetParams;
    type Output = TemplateInfo;
    const ID: &'static str = "file.templates.set";
    const TITLE: &'static str = "Save File Template";
    const DESCRIPTION: &'static str = "Adds a file template or changes one (a built-in one too), in the IDE's templates or \
         the project's .idea/fileTemplates; answers the template as it is now in use.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<TemplateInfo, CommandError> {
        let project = project(session);
        let scheme = scheme_or_active(session, params.scheme, project.as_deref());
        let templates = store(session);
        let (kind, name) = (params.template.kind, params.template.name.clone());
        templates
            .set(scheme, project.as_deref(), params.template, params.previous.as_deref())
            .map_err(CommandError::new)?;
        templates
            .find(project.as_deref(), kind, &name)
            .ok_or_else(|| CommandError::new(t!("workbench-file-templates-unknown", name = name.as_str())))
    }
}

/// A template by its tab and name, in a scheme.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct TemplateKey {
    /// The name.
    pub name: String,
    /// The tab; `file` by default.
    #[serde(default)]
    pub kind: TemplateKind,
    /// The scheme; the one in use by default.
    #[serde(default)]
    pub scheme: Option<Scheme>,
}

pub struct RemoveTemplate;

impl Command for RemoveTemplate {
    type Params = TemplateKey;
    type Output = ();
    const ID: &'static str = "file.templates.remove";
    const TITLE: &'static str = "Remove File Template";
    const DESCRIPTION: &'static str =
        "Removes a file template the user made from a scheme; a built-in one can only be reset.";

    fn run(&self, session: &mut Session, params: TemplateKey) -> Result<(), CommandError> {
        let project = project(session);
        let scheme = scheme_or_active(session, params.scheme, project.as_deref());
        store(session).remove(scheme, project.as_deref(), params.kind, &params.name).map_err(CommandError::new)
    }
}

/// The parameters of `file.templates.reset`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ResetParams {
    /// Only the template with this name.
    pub name: Option<String>,
    /// The scheme; the one in use by default.
    pub scheme: Option<Scheme>,
}

/// What `file.templates.reset` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ResetResult {
    /// How many changed built-in templates are back as declared.
    pub reset: usize,
}

pub struct ResetTemplates;

impl Command for ResetTemplates {
    type Params = ResetParams;
    type Output = ResetResult;
    const ID: &'static str = "file.templates.reset";
    const TITLE: &'static str = "Reset File Templates to Default";
    const DESCRIPTION: &'static str = "Takes changed built-in file templates of a scheme back to how their module or \
         plug-in declares them: all, or one by name; the user's own templates stay.";

    fn run(&self, session: &mut Session, params: ResetParams) -> Result<ResetResult, CommandError> {
        let project = project(session);
        let scheme = scheme_or_active(session, params.scheme, project.as_deref());
        let reset =
            store(session).reset(scheme, project.as_deref(), params.name.as_deref()).map_err(CommandError::new)?;
        Ok(ResetResult { reset })
    }
}

/// The parameters of `file.templates.saveFileAs`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SaveAsParams {
    /// The file, open in the editor or not; the active one by default.
    pub path: Option<PathBuf>,
    /// The template's name; the file's name without its extension by default.
    pub name: Option<String>,
    /// The scheme; the one in use by default.
    pub scheme: Option<Scheme>,
    /// Whether to replace a template with that name.
    pub replace: bool,
}

pub fn template_of_file(path: &Path, text: &str, name: Option<&str>) -> FileTemplate {
    let file = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, extension) = match file.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_owned(), extension.to_owned()),
        _ => (file.clone(), String::new()),
    };
    FileTemplate {
        name: name.map_or(stem, str::to_owned),
        extension,
        kind: TemplateKind::File,
        file_name: String::new(),
        text: text.to_owned(),
        reformat: true,
        live_templates: false,
        children: Vec::new(),
    }
}

pub struct SaveFileAsTemplate;

impl Command for SaveFileAsTemplate {
    type Params = SaveAsParams;
    type Output = TemplateInfo;
    const ID: &'static str = "file.templates.saveFileAs";
    const TITLE: &'static str = "Save File as Template";
    const DESCRIPTION: &'static str = "Makes a file template of a file's text (unsaved changes included), named after the \
         file and with its extension, in the scheme in use; fails when a template has that name unless `replace`.";

    fn run(&self, session: &mut Session, params: SaveAsParams) -> Result<TemplateInfo, CommandError> {
        let workbench = session.state::<Workbench>();
        let document = match (&params.path, workbench) {
            (Some(path), Some(workbench)) => {
                let path = workbench.resolve(path);
                workbench.documents().iter().find(|document| document.path() == path)
            }
            (None, Some(workbench)) => workbench.active_document(),
            (_, None) => None,
        };
        let (path, text) = match (document, &params.path) {
            (Some(document), _) if document.image().is_none() => {
                (document.path().to_owned(), document.text().to_owned())
            }
            (None, Some(path)) => {
                let path = workbench.map_or_else(|| path.clone(), |workbench| workbench.resolve(path));
                let text = std::fs::read_to_string(&path).map_err(|error| {
                    CommandError::new(t!("workbench-error-open", path = display(&path), error = error.to_string()))
                })?;
                (path, text)
            }
            _ => return Err(CommandError::new(t!("workbench-error-no-document"))),
        };
        let template = template_of_file(&path, &text, params.name.as_deref());
        let project = project(session);
        let templates = store(session);
        if !params.replace && templates.find(project.as_deref(), TemplateKind::File, &template.name).is_some() {
            return Err(CommandError::new(t!("workbench-file-templates-exists", name = template.name.as_str())));
        }
        let scheme = scheme_or_active(session, params.scheme, project.as_deref());
        let name = template.name.clone();
        templates.set(scheme, project.as_deref(), template, None).map_err(CommandError::new)?;
        templates
            .find(project.as_deref(), TemplateKind::File, &name)
            .ok_or_else(|| CommandError::new(t!("workbench-file-templates-unknown", name = name.as_str())))
    }
}

/// The parameters of `file.templates.useScheme`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct UseSchemeParams {
    /// `project` uses the project's templates in its `.idea/fileTemplates` over the IDE's; `default` uses the IDE's.
    pub scheme: Scheme,
}

/// What `file.templates.useScheme` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SchemeResult {
    /// The scheme in use now.
    pub scheme: Scheme,
}

pub struct UseScheme;

impl Command for UseScheme {
    type Params = UseSchemeParams;
    type Output = SchemeResult;
    const ID: &'static str = "file.templates.useScheme";
    const TITLE: &'static str = "Use File Template Scheme";
    const DESCRIPTION: &'static str = "The Scheme of Settings → Editor → File and Code Templates for the project: \
         `project` uses the project's templates in .idea/fileTemplates over the IDE's, `default` the IDE's. The choice \
         is kept in the IDE's data folder; no template file is written or deleted, so the project's templates stay \
         for a later switch back.";

    fn run(&self, session: &mut Session, params: UseSchemeParams) -> Result<SchemeResult, CommandError> {
        let project = project(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let templates = store(session);
        templates.use_scheme(&project, params.scheme).map_err(CommandError::new)?;
        session.state_mut::<Workbench>().revision += 1;
        Ok(SchemeResult { scheme: templates.scheme_of(Some(&project)) })
    }
}

/// The parameters of `file.new.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct NewListParams {
    /// The folder of the new file, absolute or relative to the project; the project folder by default.
    pub directory: Option<PathBuf>,
    /// What File → New offers instead of the New popup: the same for every folder, nothing that makes a class.
    pub menu: bool,
}

/// What an item of the New list makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NewItemKind {
    /// File: an empty file, its name asked for (`file.fs.newFile`).
    File,
    /// Directory: a folder (`file.fs.newDirectory`).
    Directory,
    /// A file from a template of the Files tab (`file.templates.create`).
    Template,
    /// Package: a folder in a source folder, its dots making nested folders (`file.fs.newDirectory`).
    Package,
}

/// An item of the New list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewItem {
    /// What it makes.
    pub kind: NewItemKind,
    /// Its name in the list, in the interface's language for the fixed items.
    pub title: String,
    /// For a template: its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// For a template: the extension of the file it makes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// For a template: the variables asked for when the file is made; `NAME` is the file name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asked: Vec<String>,
    /// For a row that stands for several templates: their group; the name dialog chooses among them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// For a template: the other templates its dialog offers as kinds of the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
}

/// What `file.new.list` gives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewList {
    /// The folder the items make files in.
    pub directory: PathBuf,
    /// The items, in the order of the list.
    pub items: Vec<NewItem>,
}

const SOURCE_DEPTH: usize = 6;

fn template_item(info: TemplateInfo, listed: &[TemplateInfo]) -> NewItem {
    let group = info.group.clone();
    let kinds = listed.iter().filter(|other| other.kind_of.as_deref() == Some(info.template.name.as_str()));
    let kinds = kinds.map(|other| other.template.name.clone()).collect();
    NewItem {
        kind: NewItemKind::Template,
        title: group.clone().unwrap_or_else(|| info.template.name.clone()),
        extension: Some(info.template.extension.clone()),
        template: Some(info.template.name),
        asked: info.asked,
        group,
        kinds,
    }
}

fn fixed_item(kind: NewItemKind, title: String) -> NewItem {
    NewItem { kind, title, template: None, extension: None, asked: Vec::new(), group: None, kinds: Vec::new() }
}

pub fn menu_items(templates: &FileTemplates, project: &Path) -> Vec<NewItem> {
    let listed = templates.list(Some(project));
    let plain: Vec<TemplateInfo> = listed
        .iter()
        .filter(|info| info.template.kind == TemplateKind::File && !info.source_folder && info.kind_of.is_none())
        .filter(|info| info.fits(project, project))
        .cloned()
        .collect();
    let mut items = vec![
        fixed_item(NewItemKind::File, t!("workbench-new-file")),
        fixed_item(NewItemKind::Directory, t!("workbench-new-directory")),
    ];
    items.extend(plain.into_iter().map(|info| template_item(info, &listed)));
    items
}

fn holds_sources(folder: &Path, depth: usize) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else { return false };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            return depth > 0 && holds_sources(&path, depth - 1);
        }
        path.extension().is_some_and(|extension| extension == "java" || extension == "kt")
    })
}

pub fn marked_source_folders(session: &Session) -> Vec<PathBuf> {
    let state = session.state::<ProjectSettingsState>();
    let folders = state.iter().flat_map(|state| &state.settings().source_folders);
    folders.filter(|folder| !folder.kind.is_resource()).map(|folder| folder.path.clone()).collect()
}

pub fn is_source_folder(project: &Path, directory: &Path, marked: &[PathBuf]) -> bool {
    let conventional = |folder: &Path| {
        matches!(folder.file_name().and_then(|name| name.to_str()), Some("java" | "kotlin"))
            && folder.parent().and_then(Path::parent).is_some_and(|src| src.ends_with("src"))
    };
    let plain = project.join("src");
    directory.ancestors().take_while(|folder| folder.starts_with(project) && *folder != project).any(|folder| {
        marked.iter().any(|root| root == folder)
            || conventional(folder)
            || (folder == plain && !plain.join("main").exists() && holds_sources(&plain, SOURCE_DEPTH))
    })
}

pub fn new_items(templates: &FileTemplates, project: &Path, directory: &Path, source_folder: bool) -> Vec<NewItem> {
    let listed: Vec<TemplateInfo> = templates
        .list(Some(project))
        .into_iter()
        .filter(|info| info.template.kind == TemplateKind::File && info.fits(directory, project))
        .collect();
    let file = fixed_item(NewItemKind::File, t!("workbench-new-file"));
    if !source_folder || !listed.iter().any(|info| info.source_folder) {
        let mut items = vec![file, fixed_item(NewItemKind::Directory, t!("workbench-new-directory"))];
        let plain = listed.iter().filter(|info| !info.source_folder && info.kind_of.is_none());
        items.extend(plain.cloned().map(|info| template_item(info, &listed)));
        return items;
    }
    let mut groups: Vec<String> = Vec::new();
    let mut items = Vec::new();
    for info in listed.iter().filter(|info| info.source_folder) {
        if let Some(group) = info.group.as_ref().filter(|group| !groups.contains(group)) {
            groups.push(group.clone());
            items.push(template_item(info.clone(), &listed));
        }
    }
    items.push(file);
    items.push(fixed_item(NewItemKind::Package, t!("workbench-new-package")));
    let single = listed.iter().filter(|info| info.source_folder && info.group.is_none());
    items.extend(single.cloned().map(|info| template_item(info, &listed)));
    let own = listed.iter().filter(|info| !info.source_folder && !info.builtin && info.kind_of.is_none());
    items.extend(own.cloned().map(|info| template_item(info, &listed)));
    items
}

pub struct ListNew;

impl Command for ListNew {
    type Params = NewListParams;
    type Output = NewList;
    const ID: &'static str = "file.new.list";
    const TITLE: &'static str = "New";
    const DESCRIPTION: &'static str = "What the New popup of the Project tool window (⌘N) offers for a project folder: \
         File, Directory and the file templates that fit it; in a source folder the class templates, File and \
         Package. With `menu`, what File → New offers: the same for every folder, nothing that makes a class. A template's `kinds` are more templates for \
         file.templates.create, which its dialog offers to choose from.";

    fn run(&self, session: &mut Session, params: NewListParams) -> Result<NewList, CommandError> {
        let workbench =
            session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let project =
            workbench.project().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?.to_owned();
        let directory = crate::project_folder(workbench, params.directory.as_deref().unwrap_or(Path::new("")))?;
        let templates = store(session);
        let items = if params.menu {
            menu_items(&templates, &project)
        } else {
            let marked = marked_source_folders(session);
            new_items(&templates, &project, &directory, is_source_folder(&project, &directory, &marked))
        };
        Ok(NewList { directory, items })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTemplates)?;
    registry.register(ListVariables)?;
    registry.register(RenderTemplate)?;
    registry.register(CreateFromTemplate)?;
    registry.register(SetTemplate)?;
    registry.register(RemoveTemplate)?;
    registry.register(ResetTemplates)?;
    registry.register(SaveFileAsTemplate)?;
    registry.register(UseScheme)?;
    registry.register(ListNew)?;
    Ok(())
}

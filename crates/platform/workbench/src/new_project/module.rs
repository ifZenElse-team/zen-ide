use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Session};
use zen_i18n::t;
use zen_project::generators::{
    FieldKind, FieldOption, GeneratedFile, GeneratorForm, GeneratorInfo, ModuleBuild, ModuleEntry, Values, write_files,
};

use super::{
    CreateAnswer, EMPTY, FOLDER_NOT_EMPTY, INVALID_LOCATION, NewProjects, answer, check_location, create_project,
    expanded, refuse_tool, shared,
};
use crate::run::project_root;

pub const NO_BUILD: &str = "noBuild";

pub const BUILD_FILE_UNSAVED: &str = "buildFileUnsaved";

const MODULE_VALUE: &str = "module";

const BUILD_VALUE: &str = "build";

pub(super) struct Registration {
    build: Arc<dyn ModuleBuild>,
    root: PathBuf,
    pub(super) file: String,
    text: String,
    pub(super) reload: String,
}

fn canonical(path: &Path) -> PathBuf {
    if let Ok(path) = std::fs::canonicalize(path) {
        return path;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => canonical(parent).join(name),
        _ => path.to_owned(),
    }
}

pub(super) fn relative_path(root: &Path, module: &Path) -> String {
    let (root, module) = (canonical(root), canonical(module));
    let shared = root.components().zip(module.components()).take_while(|(a, b)| a == b).count();
    let up = root.components().skip(shared).filter(|part| matches!(part, Component::Normal(_))).map(|_| "..");
    let down = module.components().skip(shared).map(|part| part.as_os_str().to_string_lossy().into_owned());
    up.map(str::to_owned).chain(down).collect::<Vec<_>>().join("/")
}

impl Registration {
    pub(super) fn edits(&self, contents: &Path, module: &Path, must_build: bool) -> Result<Vec<GeneratedFile>, String> {
        let project = self.root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.build.add_module(&ModuleEntry {
            file: self.file.clone(),
            text: self.text.clone(),
            path: relative_path(&self.root, module),
            project,
            has_build: self.build.module_files().iter().any(|file| contents.join(file).is_file()),
            must_build,
        })
    }

    pub(super) fn write(&self, edits: &[GeneratedFile]) -> Result<(), String> {
        write_files(&self.root, edits)
    }
}

impl NewProjects {
    fn builds(&self) -> Vec<Arc<dyn ModuleBuild>> {
        let source = self.lock().builds.clone();
        source.map(|source| source()).unwrap_or_default()
    }

    fn registration(&self, root: &Path) -> Option<Registration> {
        self.builds().into_iter().find_map(|build| {
            let file = build.module_files().into_iter().find(|file| root.join(file).is_file())?;
            let text = std::fs::read_to_string(root.join(&file)).ok()?;
            let reload = build.reload();
            Some(Registration { build, root: root.to_owned(), file, text, reload })
        })
    }
}

fn root_of(session: &Session) -> Result<PathBuf, CommandError> {
    project_root(session)
}

fn refuse_unsaved(session: &Session, registration: Option<&Registration>) -> Result<(), CommandError> {
    let Some(registration) = registration else { return Ok(()) };
    let path = registration.root.join(&registration.file);
    let unsaved = session
        .state::<crate::Workbench>()
        .is_some_and(|workbench| workbench.documents().iter().any(|doc| doc.path() == path && doc.is_dirty()));
    if unsaved {
        let file = registration.file.as_str();
        return Err(CommandError::with_code(BUILD_FILE_UNSAVED, t!("workbench-module-unsaved", file = file)));
    }
    Ok(())
}

pub(super) fn with_project_jdk(session: &Session, mut form: GeneratorForm) -> GeneratorForm {
    let Ok(root) = project_root(session) else { return form };
    let Some(project) = crate::jdk::project_jdk(&root) else { return form };
    let home = project.jdk.home.to_string_lossy().into_owned();
    let label = t!("workbench-module-project-jdk", jdk = project.jdk.label());
    for field in &mut form.fields {
        if let FieldKind::Jdk { options, default } = &mut field.kind {
            options.retain(|option| option.value != home);
            let own = FieldOption { value: home.clone(), label: label.clone(), ..FieldOption::default() };
            options.insert(0, own);
            *default = Some(home.clone());
        }
    }
    form
}

/// The build a project's modules are listed in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBuild {
    /// The plug-in that reads it, e.g. `maven`.
    pub plugin: String,
    /// The file a new module is entered in, relative to the project.
    pub file: String,
}

/// What New Module starts with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleOptionsAnswer {
    /// The types in the wizard's order: the same as New Project's without Empty Project.
    pub generators: Vec<GeneratorInfo>,
    /// The project's root: Location starts there.
    pub location: PathBuf,
    /// The project's build; a project without one takes the module as a plain folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<ProjectBuild>,
    /// Values the fields start with, as the project's build system for `build`.
    pub values: Values,
}

pub struct ModuleOptions;

impl Command for ModuleOptions {
    type Params = NoParams;
    type Output = ModuleOptionsAnswer;
    const ID: &'static str = "project.module.options";
    const TITLE: &'static str = "List New Module Types";
    const DESCRIPTION: &'static str = "What New Module offers in this project: the types without Empty Project, Location at the project's root, the build that will list the module, and values the fields start with. project.new.form with `module` gives a type's form.";

    fn run(&self, session: &mut Session, _params: NoParams) -> Result<ModuleOptionsAnswer, CommandError> {
        let root = root_of(session)?;
        let projects = shared(session);
        let generators =
            projects.generators().iter().map(|generator| generator.info()).filter(|info| info.id != EMPTY).collect();
        let registration = projects.registration(&root);
        let build = registration
            .map(|registration| ProjectBuild { plugin: registration.build.plugin(), file: registration.file });
        let values = build
            .iter()
            .map(|build| (BUILD_VALUE.to_owned(), serde_json::Value::String(build.plugin.clone())))
            .collect();
        Ok(ModuleOptionsAnswer { generators, location: root, build, values })
    }
}

/// What module to create.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NewModuleParams {
    /// The type's id.
    pub generator: String,
    /// The module's name, its folder's.
    pub name: String,
    /// The folder it goes in; the project's root without it.
    pub location: Option<String>,
    /// The fields' values by id; a field left out has its default.
    pub values: Values,
    /// Use a folder that holds something; existing files are kept.
    pub reuse: bool,
    /// How long to wait for it to end, in milliseconds (at most 60 000); answers at once without it.
    pub wait_ms: Option<u64>,
}

pub struct NewModule;

impl Command for NewModule {
    type Params = NewModuleParams;
    type Output = CreateAnswer;
    const ID: &'static str = "project.module.new";
    const TITLE: &'static str = "New Module";
    const DESCRIPTION: &'static str = "Creates a module of type `generator` called `name` in the open project, in the background; a failure leaves nothing. The project's build lists it: the Maven parent's `<modules>`, Gradle's `include`; `registered` names the file and `reload` the command that loads the project again, which the caller runs (for an agent, only in a trusted project). Refused while that file has unsaved changes (code `buildFileUnsaved`). Follow it with project.new.status, or give `waitMs`.";

    fn run(&self, session: &mut Session, params: NewModuleParams) -> Result<CreateAnswer, CommandError> {
        let root = root_of(session)?;
        let projects = shared(session);
        if params.generator == EMPTY {
            let generator = params.generator.as_str();
            return Err(CommandError::with_code(
                super::UNKNOWN_GENERATOR,
                t!("workbench-new-project-unknown", generator = generator),
            ));
        }
        let generator = projects.generator(&params.generator)?;
        refuse_tool(session, generator.as_ref())?;
        let location = params.location.as_deref().filter(|location| !location.trim().is_empty());
        let folder = location.map_or_else(|| root.clone(), expanded).join(params.name.trim());
        let check = check_location(&folder.to_string_lossy());
        if let Some(problem) = check.problem {
            return Err(CommandError::with_code(INVALID_LOCATION, problem));
        }
        if check.not_empty && !params.reuse {
            let path = check.path.display().to_string();
            return Err(CommandError::with_code(FOLDER_NOT_EMPTY, t!("workbench-new-project-not-empty", path = path)));
        }
        let registration = projects.registration(&root);
        refuse_unsaved(session, registration.as_ref())?;
        let mut values = params.values;
        values.insert(MODULE_VALUE.to_owned(), serde_json::Value::Bool(true));
        let (working, target, name) = (projects.clone(), check.path, check.name);
        let job = projects.start(false, move |stop, output, _| {
            let (request, _) = working.resolved(&generator, &name, &values)?;
            create_project(generator.as_ref(), &request, &target, false, stop, output, registration.as_ref())
        });
        answer(session, &projects, job, params.wait_ms)
    }
}

/// Which folder to enter in the project's build.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ModuleSourcesParams {
    /// A folder, or a build file in it.
    pub path: String,
}

/// A folder entered in the project's build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Registered {
    /// The module's folder.
    pub path: PathBuf,
    /// The build file that lists it.
    pub registered: String,
    /// Whether it had to change: false when it listed the module already.
    pub changed: bool,
    /// The command that loads the project again with the module.
    pub reload: String,
}

pub struct ModuleFromSources;

impl Command for ModuleFromSources {
    type Params = ModuleSourcesParams;
    type Output = Registered;
    const ID: &'static str = "project.module.fromSources";
    const TITLE: &'static str = "Module from Existing Sources";
    const DESCRIPTION: &'static str = "Enters an existing folder (or the folder of a build file) in the open project's build: the Maven parent's `<modules>`, Gradle's `include`. `reload` is the command that loads the project again, which the caller runs (for an agent, only in a trusted project). Refused without a build that lists modules (code `noBuild`), and while its file has unsaved changes (code `buildFileUnsaved`).";

    fn run(&self, session: &mut Session, params: ModuleSourcesParams) -> Result<Registered, CommandError> {
        let root = root_of(session)?;
        let path = expanded(&params.path);
        let found = super::detect_sources(&path).ok_or_else(|| {
            let path = path.display().to_string();
            CommandError::with_code(INVALID_LOCATION, t!("workbench-sources-not-found", path = path))
        })?;
        if canonical(&found.root) == canonical(&root) {
            return Err(CommandError::with_code(INVALID_LOCATION, t!("workbench-module-is-root")));
        }
        let registration = shared(session).registration(&root);
        let registration =
            registration.ok_or_else(|| CommandError::with_code(NO_BUILD, t!("workbench-module-no-build")))?;
        refuse_unsaved(session, Some(&registration))?;
        let edits = registration.edits(&found.root, &found.root, true).map_err(CommandError::new)?;
        registration.write(&edits).map_err(CommandError::new)?;
        Ok(Registered {
            path: found.root,
            registered: registration.file.clone(),
            changed: !edits.is_empty(),
            reload: registration.reload.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_s_path_is_relative_to_the_project_also_above_it() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("shop");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(relative_path(&root, &root.join("api")), "api");
        assert_eq!(relative_path(&root, &root.join("services/api")), "services/api");
        assert_eq!(relative_path(&root, &folder.path().join("api")), "../api");
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::settings::{SchemeFile, SettingsProvider};

use crate::Workbench;
use crate::project_settings::ProjectSettingsState;

const PROJECT: &str = "Project";

fn providers(session: &Session) -> Vec<Arc<dyn SettingsProvider>> {
    session.state::<ProjectSettingsState>().map(|state| state.providers().to_vec()).unwrap_or_default()
}

fn root(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-code-style-no-project")))
}

fn no_provider() -> CommandError {
    CommandError::new(t!("workbench-code-style-no-provider"))
}

pub fn project_scheme(session: &Session) -> Option<SchemeFile> {
    let settings = session.state::<ProjectSettingsState>()?.settings();
    let scheme = SchemeFile {
        name: PROJECT.to_owned(),
        styles: settings.code_style.clone(),
        options: settings.code_style_options.clone(),
    };
    (!scheme.styles.is_empty() || !scheme.options.is_empty()).then_some(scheme)
}

/// The code style scheme a project keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScheme {
    /// Whether the project keeps one and turns it on.
    pub exists: bool,
    /// What it sets.
    pub scheme: SchemeFile,
}

pub struct GetProjectScheme;

impl Command for GetProjectScheme {
    type Params = NoParams;
    type Output = ProjectScheme;
    const ID: &'static str = "project.codeStyle.get";
    const TITLE: &'static str = "Code Style Scheme of Project";
    const DESCRIPTION: &'static str = "Tells the Project code style scheme the project keeps in \
         .idea/codeStyles: the styles of its languages (`default` holds the scheme-wide right margin and guides) and \
         its options. Nothing to read gives exists: false.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ProjectScheme, CommandError> {
        let scheme = project_scheme(session);
        Ok(ProjectScheme { exists: scheme.is_some(), scheme: scheme.unwrap_or_default() })
    }
}

/// A scheme to keep in the project.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetProjectSchemeParams {
    /// The scheme; project.codeStyle.get gives its shape.
    pub scheme: SchemeFile,
}

/// What writing the Project scheme changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WrittenFiles {
    /// The files written, as absolute paths; empty when they already said this.
    pub files: Vec<PathBuf>,
}

pub struct SetProjectScheme;

impl Command for SetProjectScheme {
    type Params = SetProjectSchemeParams;
    type Output = WrittenFiles;
    const ID: &'static str = "project.codeStyle.set";
    const TITLE: &'static str = "Write Code Style Scheme to Project";
    const DESCRIPTION: &'static str = "Writes the Project code style scheme to .idea/codeStyles/Project.xml and turns \
         it on in codeStyleConfig.xml (USE_PER_PROJECT_SETTINGS). What the IDE does not manage in the file stays.";

    fn run(&self, session: &mut Session, params: SetProjectSchemeParams) -> Result<WrittenFiles, CommandError> {
        let root = root(session)?;
        crate::trust::guard_agent_build(session, &root)?;
        let files = [root.join(".idea/codeStyles/Project.xml"), root.join(".idea/codeStyles/codeStyleConfig.xml")];
        crate::hidden_files::guard_change(session, &root, &files)?;
        let files = providers(session)
            .iter()
            .find_map(|provider| provider.write_code_style(&root, &params.scheme))
            .ok_or_else(no_provider)?
            .map_err(CommandError::new)?;
        session.state_mut::<ProjectSettingsState>().refresh(Some(&root));
        session.state_mut::<Workbench>().revision += 1;
        Ok(WrittenFiles { files })
    }
}

/// Where to export a scheme.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportSchemeParams {
    /// The file to write, as an absolute path.
    pub path: PathBuf,
    /// The scheme; the project's when absent.
    #[serde(default)]
    pub scheme: Option<SchemeFile>,
    /// Whether to replace a file that exists.
    #[serde(default)]
    pub overwrite: bool,
}

fn guard_agent_file(session: &Session, path: &Path) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    let root = root(session)?;
    if !crate::inside_project(&root, path) {
        let shown = path.display().to_string();
        let message = t!("workbench-code-style-agent-outside", path = shown);
        return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, message));
    }
    crate::hidden_files::guard_change(session, &root, &[path])
}

pub struct ExportScheme;

impl Command for ExportScheme {
    type Params = ExportSchemeParams;
    type Output = WrittenFiles;
    const ID: &'static str = "project.codeStyle.export";
    const TITLE: &'static str = "Export Code Style Scheme";
    const DESCRIPTION: &'static str =
        "Writes a code style scheme to a file as IntelliJ code style XML (a code_scheme element).";

    fn run(&self, session: &mut Session, params: ExportSchemeParams) -> Result<WrittenFiles, CommandError> {
        guard_agent_file(session, &params.path)?;
        if params.path.exists() && !params.overwrite {
            return Err(CommandError::new(t!("workbench-code-style-exists", path = params.path.display().to_string())));
        }
        let scheme = params.scheme.or_else(|| project_scheme(session)).unwrap_or_default();
        let text =
            providers(session).iter().find_map(|provider| provider.code_style_text(&scheme)).ok_or_else(no_provider)?;
        zen_vfs::write_atomic(&params.path, text.as_bytes()).map_err(|error| CommandError::new(error.to_string()))?;
        Ok(WrittenFiles { files: vec![params.path] })
    }
}

/// The file to read a scheme from.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportSchemeParams {
    /// The IntelliJ code style XML file, as an absolute path.
    pub path: PathBuf,
}

pub struct ImportScheme;

impl Command for ImportScheme {
    type Params = ImportSchemeParams;
    type Output = SchemeFile;
    const ID: &'static str = "project.codeStyle.import";
    const TITLE: &'static str = "Read Code Style Scheme File";
    const DESCRIPTION: &'static str = "Reads a scheme from an IntelliJ code style XML file and answers it; nothing \
         changes until it is given to project.codeStyle.set or set_settings (codeStyleSchemes).";

    fn run(&self, session: &mut Session, params: ImportSchemeParams) -> Result<SchemeFile, CommandError> {
        guard_agent_file(session, &params.path)?;
        let text = std::fs::read_to_string(&params.path).map_err(|error| CommandError::new(error.to_string()))?;
        providers(session)
            .iter()
            .find_map(|provider| provider.parse_code_style(&text))
            .ok_or_else(no_provider)?
            .map_err(CommandError::new)
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetProjectScheme)?;
    registry.register(SetProjectScheme)?;
    registry.register(ExportScheme)?;
    registry.register(ImportScheme)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_project::settings::{CodeStyle, CodeStyleOptions, ProjectSettings, ReadFiles};

    use super::*;

    #[test]
    fn an_agent_reads_and_writes_schemes_only_inside_the_project_and_never_hidden_files() {
        zen_i18n::register(crate::LOCALES);
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret.xml".to_owned()]);
        let target = outside.path().join("victim.xml");
        std::fs::write(&target, "keep").unwrap();
        std::fs::write(dir.path().join("secret.xml"), "<code_scheme/>").unwrap();
        for (command, path, code) in [
            ("project.codeStyle.export", target.clone(), crate::trust::AGENT_NOT_ALLOWED),
            ("project.codeStyle.import", target.clone(), crate::trust::AGENT_NOT_ALLOWED),
            ("project.codeStyle.import", dir.path().join("secret.xml"), crate::hidden_files::REDACTED_CODE),
        ] {
            let params = json!({ "path": path, "overwrite": true });
            let answer =
                zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, command, params));
            match answer {
                Err(zen_core::DispatchError::Failed { error, .. }) => {
                    assert_eq!(error.code.as_deref(), Some(code), "{command}: {error:?}")
                }
                other => panic!("{command}: not refused: {other:?}"),
            }
        }
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    }

    #[derive(Default)]
    struct Fake {
        scheme: std::sync::Mutex<Option<SchemeFile>>,
    }

    impl SettingsProvider for Fake {
        fn id(&self) -> &str {
            "fake"
        }

        fn watches(&self, _: &Path) -> bool {
            false
        }

        fn load(&self, _: &Path, _: &dyn ReadFiles) -> Option<ProjectSettings> {
            let scheme = self.scheme.lock().unwrap().clone()?;
            Some(ProjectSettings {
                code_style: scheme.styles,
                code_style_options: scheme.options,
                ..ProjectSettings::default()
            })
        }

        fn write_code_style(&self, root: &Path, scheme: &SchemeFile) -> Option<Result<Vec<PathBuf>, String>> {
            *self.scheme.lock().unwrap() = Some(scheme.clone());
            Some(Ok(vec![root.join("written.xml")]))
        }

        fn code_style_text(&self, scheme: &SchemeFile) -> Option<String> {
            Some(scheme.name.clone())
        }

        fn parse_code_style(&self, text: &str) -> Option<Result<SchemeFile, String>> {
            Some(Ok(SchemeFile { name: text.to_owned(), ..SchemeFile::default() }))
        }
    }

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Fake::default())]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        (registry, session, dir)
    }

    #[test]
    fn a_written_scheme_is_read_back_and_turns_the_projects_options_on() {
        let (registry, mut session, _dir) = setup();
        let answer = registry.dispatch(&mut session, "project.codeStyle.get", json!({})).unwrap();
        assert_eq!(answer["exists"], false);
        let mut scheme = SchemeFile { name: PROJECT.to_owned(), ..SchemeFile::default() };
        scheme.set_style("go", CodeStyle { use_tabs: false, tab_size: 2, indent: 2 });
        scheme.options = CodeStyleOptions { wrap_on_typing: Some(true), ..CodeStyleOptions::default() };
        let written = registry.dispatch(&mut session, "project.codeStyle.set", json!({ "scheme": scheme })).unwrap();
        assert_eq!(written["files"].as_array().unwrap().len(), 1);
        let answer = registry.dispatch(&mut session, "project.codeStyle.get", json!({})).unwrap();
        assert_eq!(answer["exists"], true);
        assert_eq!(serde_json::from_value::<SchemeFile>(answer["scheme"].clone()).unwrap(), scheme);
        assert!(crate::project_settings::wrap_on_typing(&session));
    }

    #[test]
    fn export_refuses_a_file_that_exists_unless_told_and_import_reads_one() {
        let (registry, mut session, dir) = setup();
        let path = dir.path().join("scheme.xml");
        let scheme = SchemeFile { name: "Team".to_owned(), ..SchemeFile::default() };
        let params = json!({ "path": path, "scheme": scheme });
        registry.dispatch(&mut session, "project.codeStyle.export", params.clone()).unwrap();
        assert!(registry.dispatch(&mut session, "project.codeStyle.export", params).is_err());
        let params = json!({ "path": path, "scheme": scheme, "overwrite": true });
        registry.dispatch(&mut session, "project.codeStyle.export", params).unwrap();
        let read = registry.dispatch(&mut session, "project.codeStyle.import", json!({ "path": path })).unwrap();
        assert_eq!(read["name"], "Team");
    }
}

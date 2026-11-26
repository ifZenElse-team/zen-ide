use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::editorconfig::Style;
use zen_project::settings::{
    CodeStyle, CodeStyleOptions, CodeStyleOverride, DEFAULT_LANGUAGE, ExclusionChange, FileCodeStyle, PLAIN_TEXT,
    ProjectSettings, ReadFiles, SettingsProvider,
};

use crate::project_model::{RefreshAnswer, RefreshParams};
use crate::{Document, Workbench, code_style, editor_config};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskFiles {
    root: PathBuf,
}

impl DiskFiles {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_owned() }
    }

    fn resolve(&self, relative: &Path) -> Option<PathBuf> {
        let inside =
            relative.components().all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
        if !inside {
            return None;
        }
        let mut path = self.root.clone();
        for component in relative.components() {
            path.push(component);
            if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                return None;
            }
        }
        Some(path)
    }
}

const MAX_SETTINGS_FILE: u64 = 4 * 1024 * 1024;

impl ReadFiles for DiskFiles {
    fn read(&self, relative: &Path) -> Option<String> {
        let path = self.resolve(relative)?;
        if std::fs::metadata(&path).ok()?.len() > MAX_SETTINGS_FILE {
            return None;
        }
        zen_vfs::read_text(&path).ok()
    }

    fn list(&self, relative_dir: &Path) -> Vec<PathBuf> {
        let Some(dir) = self.resolve(relative_dir) else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut listed: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_symlink()))
            .map(|entry| relative_dir.join(entry.file_name()))
            .collect();
        listed.sort();
        listed
    }
}

#[derive(Default)]
pub struct ProjectSettingsState {
    providers: Vec<Arc<dyn SettingsProvider>>,
    settings: ProjectSettings,
    revision: u64,
    built_for: Option<Option<PathBuf>>,
}

impl ProjectSettingsState {
    pub fn set_providers(&mut self, providers: Vec<Arc<dyn SettingsProvider>>) {
        self.providers = providers;
        self.built_for = None;
    }

    pub fn providers(&self) -> &[Arc<dyn SettingsProvider>] {
        &self.providers
    }

    pub fn settings(&self) -> &ProjectSettings {
        &self.settings
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn watches(&self, root: Option<&Path>, path: &Path) -> bool {
        let relative = root.and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path);
        self.providers.iter().any(|provider| provider.watches(relative))
    }

    pub fn ensure(&mut self, root: Option<&Path>) -> bool {
        let built = self.built_for.as_ref().is_some_and(|built| built.as_deref() == root);
        !built && self.refresh(root)
    }

    pub fn refresh_after(&mut self, root: Option<&Path>, changed: Option<&[PathBuf]>) -> bool {
        let wanted = changed.is_none_or(|paths| paths.iter().any(|path| self.watches(root, path)));
        wanted && self.refresh(root)
    }

    pub fn refresh(&mut self, root: Option<&Path>) -> bool {
        let mut settings = ProjectSettings::default();
        if let Some(root) = root {
            let files = DiskFiles::new(root);
            for provider in &self.providers {
                if let Some(part) = provider.load(root, &files) {
                    settings.merge(part);
                }
            }
        }
        self.built_for = Some(root.map(Path::to_owned));
        if settings == self.settings {
            return false;
        }
        self.settings = settings;
        self.revision += 1;
        true
    }
}

pub const PROJECT_CODE_STYLE: &str = "editor.projectCodeStyle";

pub fn uses_project_code_style(session: &Session) -> bool {
    session.settings().get::<bool>(PROJECT_CODE_STYLE).unwrap_or(true)
}

pub fn code_style_override(session: &Session, language: &str) -> Option<CodeStyleOverride> {
    if !uses_project_code_style(session) {
        return None;
    }
    code_style_override_in(session.state::<ProjectSettingsState>()?.settings(), language)
}

pub fn code_style_override_in(settings: &ProjectSettings, language: &str) -> Option<CodeStyleOverride> {
    let own = settings.code_style.get(language).cloned();
    let other = settings.code_style.get(DEFAULT_LANGUAGE).cloned().unwrap_or_default();
    let other_file_type = language == PLAIN_TEXT || (own.is_none() && !code_style::has_own_style(language));
    let own = own.unwrap_or_default();
    let style = if other_file_type {
        own.or(other)
    } else {
        CodeStyleOverride {
            right_margin: own.right_margin.or(other.right_margin),
            visual_guides: own.visual_guides.clone().or(other.visual_guides),
            ..own
        }
    };
    (style != CodeStyleOverride::default()).then_some(style)
}

pub fn code_style_options(session: &Session) -> CodeStyleOptions {
    if !uses_project_code_style(session) {
        return CodeStyleOptions::default();
    }
    session.state::<ProjectSettingsState>().map(|state| state.settings().code_style_options.clone()).unwrap_or_default()
}

pub fn formatter_markers(session: &Session) -> bool {
    code_style_options(session)
        .formatter_markers
        .unwrap_or_else(|| session.settings().get_or(code_style::FORMATTER_MARKERS, false))
}

pub fn wrap_on_typing(session: &Session) -> bool {
    code_style_options(session)
        .wrap_on_typing
        .unwrap_or_else(|| session.settings().get_or(code_style::WRAP_ON_TYPING, false))
}

pub fn formatting_excluded(session: &Session, path: &Path) -> bool {
    let patterns = code_style_options(session)
        .do_not_format
        .unwrap_or_else(|| code_style::general_of(session.settings()).do_not_format);
    let root = project_root(session);
    let relative = root.as_deref().and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path);
    let relative = relative.to_string_lossy().replace('\\', "/");
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| crate::soft_wrap::glob(pattern, if pattern.contains('/') { &relative } else { &name }))
}

pub fn code_style_of(session: &Session, document: &Document) -> CodeStyle {
    crate::file_indent::effective_style(session, document)
}

pub fn indent_unit_of(session: &Session, document: &Document) -> String {
    code_style::indent_unit(code_style_of(session, document))
}

pub fn code_style_for_path(session: &Session, path: &Path) -> CodeStyle {
    with_override(session, path, code_style::for_path(session.settings(), path))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleLayers {
    pub project: Option<CodeStyleOverride>,
    pub editor_config: Option<Style>,
}

impl StyleLayers {
    pub fn of(session: &Session, path: &Path) -> StyleLayers {
        StyleLayers {
            project: code_style_override(session, &code_style::language_of(path)),
            editor_config: editor_config::style_for(session, path),
        }
    }

    pub fn apply_to_file(&self, base: FileCodeStyle) -> FileCodeStyle {
        let base = match &self.project {
            Some(project) => project.apply_to_file(base),
            None => base,
        };
        match &self.editor_config {
            Some(editor_config) => editor_config.apply_to_file(base),
            None => base,
        }
    }

    pub fn apply_to(&self, base: CodeStyle) -> CodeStyle {
        self.apply_to_file(FileCodeStyle::from(base)).code_style()
    }
}

fn with_override(session: &Session, path: &Path, style: CodeStyle) -> CodeStyle {
    StyleLayers::of(session, path).apply_to(style)
}

pub fn file_code_style(session: &Session, path: &Path) -> FileCodeStyle {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
    let base = match open {
        Some(document) => document.code_style(session.settings()),
        None => code_style::for_path(session.settings(), path),
    };
    StyleLayers::of(session, path).apply_to_file(FileCodeStyle::from(base))
}

pub const GLOBAL_ENCODING: &str = "editor.globalEncoding";

pub fn global_encoding(session: &Session) -> Option<String> {
    session.settings().get::<String>(GLOBAL_ENCODING).filter(|charset| !charset.trim().is_empty())
}

pub fn charset_for(session: &Session, path: &Path) -> Option<String> {
    let project = session.state::<ProjectSettingsState>().map(|state| &state.settings().encodings);
    project.and_then(|encodings| encodings.charset_for(path)).map(str::to_owned).or_else(|| global_encoding(session))
}

fn project_root(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

pub(crate) fn project_opened(session: &mut Session, root: &Path) {
    session.state_mut::<editor_config::EditorConfigs>().forget();
    session.state_mut::<ProjectSettingsState>().refresh(Some(root));
}

/// What `project.settings.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsAnswer {
    /// The settings' revision.
    pub revision: u64,
    /// The settings the project's own files give.
    pub settings: ProjectSettings,
}

pub struct GetSettings;

impl Command for GetSettings {
    type Params = NoParams;
    type Output = SettingsAnswer;
    const ID: &'static str = "project.settings.get";
    const TITLE: &'static str = "Project Settings";
    const DESCRIPTION: &'static str = "The settings the project's own files give (the .idea folder of a JetBrains IDE...), as last read, with the files they come from and what could not be read, the project's .editorconfig files and their parse errors included.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SettingsAnswer, CommandError> {
        let root = project_root(session);
        if session.state_mut::<ProjectSettingsState>().ensure(root.as_deref()) {
            crate::index_project(session);
        }
        let (problems, sources) = editor_config::problems_and_sources(session);
        let state = session.state_mut::<ProjectSettingsState>();
        let mut settings = state.settings().clone();
        settings.problems.extend(problems);
        settings.sources.extend(sources);
        Ok(SettingsAnswer { revision: state.revision(), settings })
    }
}

pub struct RefreshSettings;

impl Command for RefreshSettings {
    type Params = RefreshParams;
    type Output = RefreshAnswer;
    const ID: &'static str = "project.settings.refresh";
    const TITLE: &'static str = "Reload Project Settings";
    const DESCRIPTION: &'static str =
        "Rereads the project's settings files (.idea, .run, *.iml...) when one of the given files changed, or always.";

    fn run(&self, session: &mut Session, params: RefreshParams) -> Result<RefreshAnswer, CommandError> {
        let root = project_root(session);
        editor_config::files_changed(session, params.paths.as_deref());
        let state = session.state_mut::<ProjectSettingsState>();
        let changed = state.refresh_after(root.as_deref(), params.paths.as_deref());
        let revision = state.revision();
        if changed {
            crate::index_project(session);
        }
        Ok(RefreshAnswer { revision, changed })
    }
}

/// A folder of the project.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct FolderParams {
    /// The folder: absolute, or relative to the project.
    pub path: PathBuf,
}

/// What the `project.directory.*` commands and `project.structure.setExcludePatterns` answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionAnswer {
    /// Whether a file of the project was written.
    pub changed: bool,
    /// The files written or created.
    pub files: Vec<PathBuf>,
    /// The folders the project excludes now, besides the built-in ones.
    pub excluded: Vec<PathBuf>,
    /// The folders marked as resource roots now.
    pub resource_roots: Vec<PathBuf>,
    /// The name patterns the project excludes now.
    pub exclude_patterns: Vec<String>,
    /// The settings' revision.
    pub revision: u64,
}

pub struct MarkExcluded;

impl Command for MarkExcluded {
    type Params = FolderParams;
    type Output = ExclusionAnswer;
    const ID: &'static str = "project.directory.markExcluded";
    const TITLE: &'static str = "Mark Directory as Excluded";
    const DESCRIPTION: &'static str = "Excludes a folder from the project in its .idea module files: adds <excludeFolder> to the module file that holds it (creating .idea/modules.xml and a module file when the project has none), leaving the rest of the file as it was; a resource root mark of the folder is removed. The index, Go to File, Search Everywhere and Find in Files drop the folder at once.";

    fn run(&self, session: &mut Session, params: FolderParams) -> Result<ExclusionAnswer, CommandError> {
        mark_folder(session, &params.path, Mark::Excluded, true)
    }
}

pub struct CancelExclusion;

impl Command for CancelExclusion {
    type Params = FolderParams;
    type Output = ExclusionAnswer;
    const ID: &'static str = "project.directory.cancelExclusion";
    const TITLE: &'static str = "Cancel Exclusion";
    const DESCRIPTION: &'static str = "Includes an excluded folder in the project again: removes its <excludeFolder> from the module files. The index, Go to File, Search Everywhere and Find in Files find its files again at once.";

    fn run(&self, session: &mut Session, params: FolderParams) -> Result<ExclusionAnswer, CommandError> {
        mark_folder(session, &params.path, Mark::Excluded, false)
    }
}

pub struct MarkResourceRoot;

impl Command for MarkResourceRoot {
    type Params = FolderParams;
    type Output = ExclusionAnswer;
    const ID: &'static str = "project.directory.markResourceRoot";
    const TITLE: &'static str = "Mark Directory as Resource Root";
    const DESCRIPTION: &'static str = "Marks a folder as a resource root in its .idea module files: adds <sourceFolder type=\"java-resource\"> to the module file that holds it (creating .idea/modules.xml and a module file when the project has none), leaving the rest of the file as it was; an exclusion or another root mark of the folder is removed.";

    fn run(&self, session: &mut Session, params: FolderParams) -> Result<ExclusionAnswer, CommandError> {
        mark_folder(session, &params.path, Mark::ResourceRoot, true)
    }
}

pub struct UnmarkResourceRoot;

impl Command for UnmarkResourceRoot {
    type Params = FolderParams;
    type Output = ExclusionAnswer;
    const ID: &'static str = "project.directory.unmarkResourceRoot";
    const TITLE: &'static str = "Unmark as Resource Root";
    const DESCRIPTION: &'static str =
        "Takes the resource root mark off a folder: removes its resource <sourceFolder> from the module files.";

    fn run(&self, session: &mut Session, params: FolderParams) -> Result<ExclusionAnswer, CommandError> {
        mark_folder(session, &params.path, Mark::ResourceRoot, false)
    }
}

/// The name patterns of files and folders the project excludes.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ExcludePatternsParams {
    /// The patterns, names only with `*` and `?` (e.g. `*.log`); an entry may hold several separated by `;`.
    pub patterns: Vec<String>,
}

pub struct SetExcludePatterns;

impl Command for SetExcludePatterns {
    type Params = ExcludePatternsParams;
    type Output = ExclusionAnswer;
    const ID: &'static str = "project.structure.setExcludePatterns";
    const TITLE: &'static str = "Set Exclude Files Patterns";
    const DESCRIPTION: &'static str = "Replaces the project's Exclude files patterns (names only, with * and ?; a folder whose name matches is excluded with its content): writes one <excludePattern> per pattern into the module file of the project's content root, leaving the rest of the file as it was. The index, Go to File, Search Everywhere and Find in Files apply them at once.";

    fn run(&self, session: &mut Session, params: ExcludePatternsParams) -> Result<ExclusionAnswer, CommandError> {
        let patterns: Vec<String> = params
            .patterns
            .iter()
            .flat_map(|entry| entry.split(';'))
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
            .map(str::to_owned)
            .collect();
        let root = require_project(session)?;
        let failed = |error: String| CommandError::new(t!("workbench-error-exclude-patterns", error = error));
        let change = providers(session)
            .iter()
            .find_map(|provider| provider.set_exclude_patterns(&root, &patterns))
            .ok_or_else(|| failed("no settings provider keeps excluded name patterns".to_owned()))?
            .map_err(failed)?;
        Ok(after_change(session, &root, change))
    }
}

/// What `project.structure.get` answers: the content of Settings → Project Structure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructureAnswer {
    /// The project's root folder.
    pub root: PathBuf,
    /// The content roots of the project's modules; the project's root when it has no module file.
    pub content_roots: Vec<PathBuf>,
    /// The folders marked excluded.
    pub excluded: Vec<PathBuf>,
    /// The folders marked as resource roots.
    pub resource_roots: Vec<PathBuf>,
    /// The Exclude files name patterns.
    pub exclude_patterns: Vec<String>,
    /// Whether the project's settings files can be written (a provider keeps folder marks).
    pub editable: bool,
    /// The settings' revision.
    pub revision: u64,
}

pub struct GetStructure;

impl Command for GetStructure {
    type Params = NoParams;
    type Output = StructureAnswer;
    const ID: &'static str = "project.structure.get";
    const TITLE: &'static str = "Project Structure";
    const DESCRIPTION: &'static str = "What Settings → Project Structure shows: the project's content roots, the folders marked excluded or as resource roots, and the Exclude files name patterns, as the module files (*.iml) give them.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<StructureAnswer, CommandError> {
        let root = require_project(session)?;
        if session.state_mut::<ProjectSettingsState>().ensure(Some(&root)) {
            crate::index_project(session);
        }
        let editable = !providers(session).is_empty();
        let state = session.state_mut::<ProjectSettingsState>();
        let settings = state.settings();
        let mut content_roots = settings.content_roots.clone();
        if content_roots.is_empty() {
            content_roots.push(root.clone());
        }
        Ok(StructureAnswer {
            root,
            content_roots,
            excluded: settings.excluded.clone(),
            resource_roots: settings.resource_roots().map(Path::to_owned).collect(),
            exclude_patterns: settings.exclude_patterns.clone(),
            editable,
            revision: state.revision(),
        })
    }
}

#[derive(Clone, Copy)]
enum Mark {
    Excluded,
    ResourceRoot,
}

fn require_project(session: &Session) -> Result<PathBuf, CommandError> {
    project_root(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn providers(session: &Session) -> Vec<Arc<dyn SettingsProvider>> {
    session.state::<ProjectSettingsState>().map(|state| state.providers().to_vec()).unwrap_or_default()
}

fn mark_folder(session: &mut Session, path: &Path, mark: Mark, on: bool) -> Result<ExclusionAnswer, CommandError> {
    let root = require_project(session)?;
    let folder = session.state::<Workbench>().map_or_else(|| root.join(path), |workbench| workbench.resolve(path));
    let failed = |error: String| {
        CommandError::new(t!("workbench-error-exclusion", path = folder.display().to_string(), error = error))
    };
    let change = providers(session)
        .iter()
        .find_map(|provider| match mark {
            Mark::Excluded => provider.set_excluded(&root, &folder, on),
            Mark::ResourceRoot => provider.set_resource_root(&root, &folder, on),
        })
        .ok_or_else(|| failed("no settings provider keeps folder marks".to_owned()))?
        .map_err(failed)?;
    Ok(after_change(session, &root, change))
}

fn after_change(session: &mut Session, root: &Path, change: ExclusionChange) -> ExclusionAnswer {
    if change.changed {
        session.state_mut::<Workbench>().revision += 1;
    }
    if session.state_mut::<ProjectSettingsState>().refresh(Some(root)) {
        crate::index_project(session);
    }
    let state = session.state_mut::<ProjectSettingsState>();
    let settings = state.settings();
    ExclusionAnswer {
        changed: change.changed,
        files: change.files,
        excluded: settings.excluded.clone(),
        resource_roots: settings.resource_roots().map(Path::to_owned).collect(),
        exclude_patterns: settings.exclude_patterns.clone(),
        revision: state.revision(),
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetSettings)?;
    registry.register(RefreshSettings)?;
    registry.register(GetStructure)?;
    registry.register(SetExcludePatterns)?;
    registry.register(MarkExcluded)?;
    registry.register(CancelExclusion)?;
    registry.register(MarkResourceRoot)?;
    registry.register(UnmarkResourceRoot)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;
    use zen_project::settings::Scope;

    use super::*;

    #[derive(Default)]
    struct Fake {
        loads: AtomicUsize,
        read: Mutex<Vec<Option<String>>>,
    }

    impl SettingsProvider for Fake {
        fn id(&self) -> &str {
            "fake"
        }

        fn watches(&self, relative: &Path) -> bool {
            relative.starts_with("conf")
        }

        fn load(&self, root: &Path, files: &dyn ReadFiles) -> Option<ProjectSettings> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            let listed = files.list(Path::new("conf"));
            *self.read.lock().unwrap() = vec![
                files.read(Path::new("conf/scope")),
                files.read(Path::new("../outside")),
                files.read(Path::new("/etc/hosts")),
            ];
            let pattern = files.read(Path::new("conf/scope"))?;
            Some(ProjectSettings {
                scopes: vec![Scope { name: "s".to_owned(), pattern: pattern.trim().to_owned() }],
                sources: listed.iter().map(|path| root.join(path)).collect(),
                ..ProjectSettings::default()
            })
        }
    }

    fn setup(files: &[(&str, &str)]) -> (Registry, Session, tempfile::TempDir, Arc<Fake>) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let fake = Arc::new(Fake::default());
        let mut session = Session::new();
        session.state_mut::<ProjectSettingsState>().set_providers(vec![fake.clone()]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        (registry, session, dir, fake)
    }

    struct TabsInGo;

    impl SettingsProvider for TabsInGo {
        fn id(&self) -> &str {
            "tabs"
        }

        fn watches(&self, _relative: &Path) -> bool {
            false
        }

        fn load(&self, _root: &Path, _files: &dyn ReadFiles) -> Option<ProjectSettings> {
            let mut settings = ProjectSettings::default();
            let style = CodeStyleOverride { use_tabs: Some(false), tab_size: Some(8), ..CodeStyleOverride::default() };
            settings.code_style.insert("go".to_owned(), style);
            Some(settings)
        }
    }

    #[test]
    fn the_default_scheme_leaves_the_projects_code_style_out() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(TabsInGo)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let main = dir.path().join("main.go");
        assert!(uses_project_code_style(&session), "the project's scheme until the user chooses another");
        assert_eq!(code_style_override(&session, "go").and_then(|style| style.tab_size), Some(8));
        assert_eq!(code_style_for_path(&session, &main).tab_size, 8);
        session.settings_mut().set(PROJECT_CODE_STYLE, false).unwrap();
        assert!(!uses_project_code_style(&session));
        assert_eq!(code_style_override(&session, "go"), None);
        assert_eq!(code_style_for_path(&session, &main), code_style::for_path(session.settings(), &main));
        session.settings_mut().set(PROJECT_CODE_STYLE, true).unwrap();
        assert_eq!(code_style_for_path(&session, &main).tab_size, 8);
    }

    #[test]
    fn without_providers_the_settings_are_empty() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let got = registry.dispatch(&mut session, "project.settings.get", json!({})).unwrap();
        assert_eq!(got["revision"], 0);
        assert_eq!(got["settings"], serde_json::to_value(ProjectSettings::default()).unwrap());
    }

    #[test]
    fn the_settings_are_read_when_the_project_opens_and_only_inside_it() {
        let (registry, mut session, dir, fake) = setup(&[("conf/scope", "file:*.go\n"), ("main.go", "")]);
        assert_eq!(fake.loads.load(Ordering::SeqCst), 1);
        let got = registry.dispatch(&mut session, "project.settings.get", json!({})).unwrap();
        assert_eq!(got["revision"], 1);
        assert_eq!(got["settings"]["scopes"], json!([{ "name": "s", "pattern": "file:*.go" }]));
        assert_eq!(got["settings"]["sources"], json!([dir.path().join("conf/scope")]));
        assert_eq!(fake.loads.load(Ordering::SeqCst), 1);
        assert_eq!(*fake.read.lock().unwrap(), vec![Some("file:*.go\n".to_owned()), None, None]);
    }

    #[test]
    fn only_a_watched_change_reads_them_again() {
        let (registry, mut session, dir, fake) = setup(&[("conf/scope", "a")]);
        let refresh = |registry: &Registry, session: &mut Session, paths: serde_json::Value| {
            registry.dispatch(session, "project.settings.refresh", json!({ "paths": paths })).unwrap()
        };
        std::fs::write(dir.path().join("conf/scope"), "b").unwrap();
        assert_eq!(
            refresh(&registry, &mut session, json!([dir.path().join("main.go")])),
            json!({ "revision": 1, "changed": false })
        );
        assert_eq!(fake.loads.load(Ordering::SeqCst), 1);
        let watched = json!([dir.path().join("conf/scope")]);
        assert_eq!(refresh(&registry, &mut session, watched.clone()), json!({ "revision": 2, "changed": true }));
        assert_eq!(refresh(&registry, &mut session, watched), json!({ "revision": 2, "changed": false }));
        std::fs::remove_file(dir.path().join("conf/scope")).unwrap();
        let all = registry.dispatch(&mut session, "project.settings.refresh", json!({})).unwrap();
        assert_eq!(all, json!({ "revision": 3, "changed": true }));
        let got = registry.dispatch(&mut session, "project.settings.get", json!({})).unwrap();
        assert_eq!(got["settings"]["scopes"], json!([]));
    }

    struct Excluding;

    impl SettingsProvider for Excluding {
        fn id(&self) -> &str {
            "excluding"
        }

        fn watches(&self, relative: &Path) -> bool {
            relative == Path::new("exclude.conf")
        }

        fn load(&self, root: &Path, files: &dyn ReadFiles) -> Option<ProjectSettings> {
            let text = files.read(Path::new("exclude.conf"))?;
            let (folders, patterns): (Vec<&str>, Vec<&str>) =
                text.split_whitespace().partition(|word| !word.contains('*'));
            Some(ProjectSettings {
                excluded: folders.iter().map(|folder| root.join(folder)).collect(),
                exclude_patterns: patterns.iter().map(|pattern| (*pattern).to_owned()).collect(),
                ..ProjectSettings::default()
            })
        }
    }

    fn indexed(session: &Session) -> Vec<String> {
        let index = session.state::<Workbench>().unwrap().index.clone().unwrap();
        while !index.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut files: Vec<String> = index.file_list().into_iter().map(|(_, relative)| relative).collect();
        files.sort();
        files
    }

    #[test]
    fn the_index_leaves_out_what_the_settings_exclude_and_follows_their_changes() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in
            [("exclude.conf", "gen *.log"), ("main.go", "needle"), ("gen/out.go", "needle"), ("run.log", "needle")]
        {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut session = Session::new();
        session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Excluding)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        assert_eq!(indexed(&session), ["exclude.conf", "main.go"]);
        let found = registry.dispatch(&mut session, "navigate.file.goTo", json!({ "query": "out" })).unwrap();
        assert_eq!(found["files"], json!([]));
        let matches =
            registry.dispatch(&mut session, "search.text.find", json!({ "query": { "text": "needle" } })).unwrap();
        assert_eq!(matches["matches"].as_array().unwrap().len(), 1);
        assert_eq!(matches["matches"][0]["path"], "main.go");

        let before = session.state::<Workbench>().unwrap().index.clone().unwrap();
        let unrelated = json!({ "paths": [dir.path().join("main.go")] });
        registry.dispatch(&mut session, "project.settings.refresh", unrelated).unwrap();
        let after = session.state::<Workbench>().unwrap().index.clone().unwrap();
        assert_eq!(before.exclusions(), after.exclusions());

        std::fs::write(dir.path().join("exclude.conf"), "*.go").unwrap();
        let changed = json!({ "paths": [dir.path().join("exclude.conf")] });
        registry.dispatch(&mut session, "project.settings.refresh", changed).unwrap();
        assert_eq!(indexed(&session), ["exclude.conf", "run.log"]);
    }

    struct Marking;

    impl SettingsProvider for Marking {
        fn id(&self) -> &str {
            "marking"
        }

        fn watches(&self, relative: &Path) -> bool {
            relative == Path::new("exclude.conf")
        }

        fn load(&self, root: &Path, files: &dyn ReadFiles) -> Option<ProjectSettings> {
            let mut settings = Excluding.load(root, files).unwrap_or_default();
            let resources = files.read(Path::new("resource.conf")).unwrap_or_default();
            settings.source_folders = resources
                .split_whitespace()
                .map(|folder| zen_project::settings::SourceFolder {
                    path: root.join(folder),
                    kind: zen_project::settings::SourceFolderKind::Resources,
                })
                .collect();
            Some(settings)
        }

        fn set_resource_root(
            &self,
            root: &Path,
            folder: &Path,
            marked: bool,
        ) -> Option<Result<ExclusionChange, String>> {
            let file = root.join("resource.conf");
            let relative = folder.strip_prefix(root).ok()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let mut words: Vec<&str> = text.split_whitespace().filter(|word| *word != relative).collect();
            if marked {
                words.push(&relative);
            }
            std::fs::write(&file, words.join(" ")).unwrap();
            Some(Ok(ExclusionChange { changed: true, files: vec![file] }))
        }

        fn set_exclude_patterns(&self, root: &Path, patterns: &[String]) -> Option<Result<ExclusionChange, String>> {
            let file = root.join("exclude.conf");
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let mut words: Vec<&str> = text.split_whitespace().filter(|word| !word.contains('*')).collect();
            words.extend(patterns.iter().map(String::as_str));
            std::fs::write(&file, words.join(" ")).unwrap();
            Some(Ok(ExclusionChange { changed: true, files: vec![file] }))
        }

        fn set_excluded(&self, root: &Path, folder: &Path, excluded: bool) -> Option<Result<ExclusionChange, String>> {
            let file = root.join("exclude.conf");
            let relative = folder.strip_prefix(root).ok()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let mut words: Vec<&str> = text.split_whitespace().filter(|word| *word != relative).collect();
            if excluded {
                words.push(&relative);
            }
            std::fs::write(&file, words.join(" ")).unwrap();
            Some(Ok(ExclusionChange { changed: true, files: vec![file] }))
        }
    }

    fn found(registry: &Registry, session: &mut Session, query: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let index = session.state::<Workbench>().unwrap().index.clone().unwrap();
        while !index.is_ready() || !index.text_status().ready {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let names = |value: &serde_json::Value, key: &str| -> Vec<String> {
            let mut names: Vec<String> =
                value.as_array().unwrap().iter().map(|item| item[key].as_str().unwrap().to_owned()).collect();
            names.sort();
            names
        };
        let files = registry.dispatch(session, "navigate.file.goTo", json!({ "query": query })).unwrap();
        let everywhere = registry
            .dispatch(session, "search.everywhere.query", json!({ "tab": "files", "query": query, "waitMs": 2000 }))
            .unwrap();
        let shown: Vec<serde_json::Value> = everywhere["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["items"].as_array().unwrap().clone())
            .collect();
        let text = registry.dispatch(session, "search.text.find", json!({ "query": { "text": "needle" } })).unwrap();
        (names(&files["files"], "relative"), names(&json!(shown), "name"), names(&text["matches"], "path"))
    }

    #[test]
    fn a_marked_folder_leaves_every_search_at_once_and_comes_back_when_cancelled() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for (name, text) in [
            ("go.mod", "module shop\n"),
            ("main.go", "needle"),
            ("gen/needle_gen.go", "needle"),
            ("vendor/example.com/m/needle_vendor.go", "needle"),
        ] {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut session = Session::new();
        session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Marking)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        let (files, everywhere, text) = found(&registry, &mut session, "needle");
        assert_eq!(files, ["gen/needle_gen.go", "vendor/example.com/m/needle_vendor.go"], "vendor is searched");
        assert_eq!(everywhere, ["needle_gen.go", "needle_vendor.go"]);
        assert_eq!(text, ["gen/needle_gen.go", "main.go", "vendor/example.com/m/needle_vendor.go"]);

        let marked =
            registry.dispatch(&mut session, "project.directory.markExcluded", json!({ "path": "gen" })).unwrap();
        assert_eq!(marked["changed"], true);
        assert_eq!(marked["excluded"], json!([root.join("gen")]));
        let (files, everywhere, text) = found(&registry, &mut session, "needle");
        assert_eq!(files, ["vendor/example.com/m/needle_vendor.go"]);
        assert_eq!(everywhere, ["needle_vendor.go"]);
        assert_eq!(text, ["main.go", "vendor/example.com/m/needle_vendor.go"]);

        let path = root.join("gen");
        let cancelled =
            registry.dispatch(&mut session, "project.directory.cancelExclusion", json!({ "path": path })).unwrap();
        assert_eq!(cancelled["excluded"], json!([]));
        let (files, everywhere, text) = found(&registry, &mut session, "needle");
        assert_eq!(files, ["gen/needle_gen.go", "vendor/example.com/m/needle_vendor.go"]);
        assert_eq!(everywhere, ["needle_gen.go", "needle_vendor.go"]);
        assert_eq!(text, ["gen/needle_gen.go", "main.go", "vendor/example.com/m/needle_vendor.go"]);
    }

    #[test]
    fn project_structure_lists_the_marks_and_its_patterns_reach_the_index_at_once() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for (name, text) in [("main.go", ""), ("run.log", ""), ("logs/today.txt", ""), ("static/app.css", "")] {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut session = Session::new();
        session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Marking)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        let structure = registry.dispatch(&mut session, "project.structure.get", json!({})).unwrap();
        assert_eq!(structure["contentRoots"], json!([root]), "the project's root without module files");
        assert_eq!(structure["editable"], true);

        let marked =
            registry.dispatch(&mut session, "project.directory.markResourceRoot", json!({ "path": "static" })).unwrap();
        assert_eq!(marked["resourceRoots"], json!([root.join("static")]));
        let set = registry
            .dispatch(&mut session, "project.structure.setExcludePatterns", json!({ "patterns": ["*.log; log*"] }))
            .unwrap();
        assert_eq!(set["excludePatterns"], json!(["*.log", "log*"]));
        assert_eq!(indexed(&session), ["exclude.conf", "main.go", "resource.conf", "static/app.css"]);
        let structure = registry.dispatch(&mut session, "project.structure.get", json!({})).unwrap();
        assert_eq!(structure["resourceRoots"], json!([root.join("static")]));
        assert_eq!(structure["excludePatterns"], json!(["*.log", "log*"]));

        registry.dispatch(&mut session, "project.directory.unmarkResourceRoot", json!({ "path": "static" })).unwrap();
        registry.dispatch(&mut session, "project.structure.setExcludePatterns", json!({ "patterns": [] })).unwrap();
        let structure = registry.dispatch(&mut session, "project.structure.get", json!({})).unwrap();
        assert_eq!(structure["resourceRoots"], json!([]));
        assert_eq!(structure["excludePatterns"], json!([]));
        assert!(indexed(&session).contains(&"logs/today.txt".to_owned()));
    }

    #[test]
    fn marking_needs_a_project_and_a_provider_that_keeps_exclusions() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let error =
            registry.dispatch(&mut session, "project.directory.markExcluded", json!({ "path": "/x" })).unwrap_err();
        assert!(error.to_string().contains(&zen_i18n::t!("workbench-error-no-project")), "{error}");
        let dir = tempfile::tempdir().unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let error =
            registry.dispatch(&mut session, "project.directory.markExcluded", json!({ "path": "gen" })).unwrap_err();
        assert!(error.to_string().contains("no settings provider"), "{error}");
    }

    #[test]
    fn a_language_takes_its_own_override_and_the_project_right_margin() {
        use zen_project::settings::CodeStyleOverride;
        let mut settings = ProjectSettings::default();
        assert_eq!(code_style_override_in(&settings, "go"), None);
        settings.code_style.insert(
            DEFAULT_LANGUAGE.to_owned(),
            CodeStyleOverride { indent: Some(3), right_margin: Some(110), ..Default::default() },
        );
        settings.code_style.insert("go".to_owned(), CodeStyleOverride { tab_size: Some(6), ..Default::default() });
        settings
            .code_style
            .insert("json".to_owned(), CodeStyleOverride { right_margin: Some(80), ..Default::default() });
        let only_margin = CodeStyleOverride { right_margin: Some(110), ..Default::default() };
        assert_eq!(
            code_style_override_in(&settings, "go"),
            Some(CodeStyleOverride { tab_size: Some(6), ..only_margin.clone() })
        );
        assert_eq!(code_style_override_in(&settings, "json").and_then(|style| style.right_margin), Some(80));
        assert_eq!(code_style_override_in(&settings, "yaml"), Some(only_margin), "the other indent options stay out");
        assert_eq!(code_style_override_in(&settings, DEFAULT_LANGUAGE).and_then(|style| style.indent), Some(3));
    }

    #[test]
    fn plain_text_and_unknown_file_types_take_the_other_indent_options() {
        let mut settings = ProjectSettings::default();
        let other =
            CodeStyleOverride { indent: Some(3), use_tabs: Some(false), right_margin: Some(110), ..Default::default() };
        settings.code_style.insert(DEFAULT_LANGUAGE.to_owned(), other.clone());
        assert_eq!(code_style_override_in(&settings, PLAIN_TEXT), Some(other.clone()));
        assert_eq!(code_style_override_in(&settings, "zzz"), Some(other.clone()));
        assert_eq!(
            code_style_override_in(&settings, "go"),
            Some(CodeStyleOverride { right_margin: Some(110), ..Default::default() }),
            "Go has a style of its own"
        );
        let text = CodeStyleOverride { use_tabs: Some(true), tab_size: Some(8), ..Default::default() };
        settings.code_style.insert(PLAIN_TEXT.to_owned(), text);
        assert_eq!(
            code_style_override_in(&settings, PLAIN_TEXT),
            Some(CodeStyleOverride {
                use_tabs: Some(true),
                tab_size: Some(8),
                indent: Some(3),
                right_margin: Some(110),
                ..Default::default()
            }),
            "TEXT first, then the other options"
        );
        assert_eq!(code_style_override_in(&settings, "zzz"), Some(other), "TEXT is for plain text only");
        let groovy = CodeStyleOverride { indent: Some(2), ..Default::default() };
        settings.code_style.insert("groovy".to_owned(), groovy.clone());
        assert_eq!(
            code_style_override_in(&settings, "groovy"),
            Some(CodeStyleOverride { right_margin: Some(110), ..groovy }),
            "a language the project styles has a style of its own"
        );
    }

    #[test]
    fn disk_files_stay_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/one.xml"), "1").unwrap();
        let files = DiskFiles::new(&dir.path().join("a"));
        assert_eq!(files.list(Path::new("")), vec![PathBuf::from("b"), PathBuf::from("one.xml")]);
        assert_eq!(files.read(Path::new("one.xml")).as_deref(), Some("1"));
        assert_eq!(files.read(Path::new("b")), None);
        assert!(files.list(Path::new("..")).is_empty());
        assert!(files.list(Path::new("missing")).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn disk_files_do_not_follow_symbolic_links_out_of_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.xml"), "<x/>").unwrap();
        std::fs::create_dir_all(dir.path().join(".idea")).unwrap();
        std::fs::write(dir.path().join(".idea/misc.xml"), "<m/>").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(".idea/linked")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.xml"), dir.path().join(".idea/file.xml")).unwrap();
        let files = DiskFiles::new(dir.path());
        assert_eq!(files.list(Path::new(".idea")), vec![PathBuf::from(".idea/misc.xml")]);
        assert_eq!(files.read(Path::new(".idea/linked/secret.xml")), None);
        assert_eq!(files.read(Path::new(".idea/file.xml")), None);
        assert!(files.list(Path::new(".idea/linked")).is_empty());
        assert_eq!(files.read(Path::new(".idea/misc.xml")).as_deref(), Some("<m/>"));
    }
}

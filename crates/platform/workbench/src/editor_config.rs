use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::editorconfig::{FILE_NAME, File, Found, ProblemKind, RightMargin, Style};
use zen_project::settings::{FileCodeStyle, SettingsProblem};

use crate::{Workbench, code_style, project_settings};

mod files;
mod save;
mod syntax;
mod write;

pub use files::{
    CreateEditorConfig, CreateParams, EXISTS, EditorConfigFile, ExportEditorConfig, ExportParams, Exported,
    ListEditorConfigs, OpenEditorConfig, OpenParams,
};
pub use save::{SaveProperties, before_write, charset, save_properties};
pub use syntax::EditorConfigSyntax;
pub use write::{DEFAULT_RIGHT_MARGIN, Stubs, scheme_text, stub_text};

pub const ENABLED: &str = "editor.editorConfig.enabled";

const MAX_FILE: u64 = 1024 * 1024;

pub fn enabled(settings: &zen_core::Settings) -> bool {
    settings.get::<bool>(ENABLED).unwrap_or(true)
}

const RECHECK: Duration = Duration::from_secs(1);

type Stamp = (Option<SystemTime>, u64);

#[derive(Clone)]
struct Cached {
    stamp: Option<Stamp>,
    file: Option<Arc<File>>,
}

#[derive(Default)]
struct Cache {
    root: PathBuf,
    folders: HashMap<PathBuf, Cached>,
    found: HashMap<PathBuf, Found>,
    checked: Option<Instant>,
}

fn stamp_of(folder: &Path) -> Option<Stamp> {
    std::fs::metadata(folder.join(FILE_NAME))
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| (metadata.modified().ok(), metadata.len()))
}

fn read(folder: &Path, stamp: Option<Stamp>) -> Cached {
    let file = stamp
        .filter(|(_, size)| *size <= MAX_FILE)
        .and_then(|_| zen_vfs::read_text(&folder.join(FILE_NAME)).ok())
        .map(|text| Arc::new(File::parse(&text)));
    Cached { stamp, file }
}

impl Cache {
    fn file_in(&mut self, folder: &Path) -> Option<Arc<File>> {
        if let Some(cached) = self.folders.get(folder) {
            return cached.file.clone();
        }
        let cached = read(folder, stamp_of(folder));
        let file = cached.file.clone();
        self.folders.insert(folder.to_owned(), cached);
        file
    }

    fn recheck(&mut self) {
        let changed: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|(folder, cached)| stamp_of(folder) != cached.stamp)
            .map(|(folder, _)| folder.clone())
            .collect();
        if !changed.is_empty() {
            for folder in changed {
                self.folders.remove(&folder);
            }
            self.found.clear();
        }
        self.checked = Some(Instant::now());
    }
}

pub struct EditorConfigs {
    recheck: Duration,
    cache: Mutex<Cache>,
}

impl Default for EditorConfigs {
    fn default() -> Self {
        Self::with_recheck(RECHECK)
    }
}

impl EditorConfigs {
    pub fn with_recheck(recheck: Duration) -> Self {
        Self { recheck, cache: Mutex::default() }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn file_in(&self, folder: &Path) -> Option<Arc<File>> {
        let stamp = stamp_of(folder);
        let mut cache = self.cache();
        if let Some(cached) = cache.folders.get(folder).filter(|cached| cached.stamp == stamp) {
            return cached.file.clone();
        }
        let cached = read(folder, stamp);
        let file = cached.file.clone();
        if cache.folders.insert(folder.to_owned(), cached).is_some() {
            cache.found.clear();
        }
        file
    }

    pub fn lookup(&self, root: &Path, path: &Path) -> Found {
        let mut cache = self.cache();
        if cache.root != root {
            *cache = Cache { root: root.to_owned(), ..Cache::default() };
        }
        if cache.checked.is_none_or(|checked| checked.elapsed() >= self.recheck) {
            cache.recheck();
        }
        if let Some(found) = cache.found.get(path) {
            return found.clone();
        }
        let found = zen_project::editorconfig::lookup(root, path, |folder| cache.file_in(folder));
        cache.found.insert(path.to_owned(), found.clone());
        found
    }

    pub fn forget(&self) {
        let mut cache = self.cache();
        cache.folders.clear();
        cache.found.clear();
    }

    fn recheck_now(&self) {
        self.cache().recheck();
    }

    fn known(&self) -> Vec<(PathBuf, Arc<File>)> {
        let cache = self.cache();
        let mut known: Vec<(PathBuf, Arc<File>)> = cache
            .folders
            .iter()
            .filter_map(|(folder, cached)| Some((folder.join(FILE_NAME), Arc::clone(cached.file.as_ref()?))))
            .collect();
        known.sort_by(|a, b| a.0.cmp(&b.0));
        known
    }
}

fn project_root(session: &Session) -> Option<&Path> {
    session.state::<Workbench>().and_then(Workbench::project)
}

pub fn found_for(session: &Session, path: &Path) -> Option<Found> {
    if !enabled(session.settings()) {
        return None;
    }
    let root = project_root(session)?;
    if !path.starts_with(root) {
        return None;
    }
    Some(match session.state::<EditorConfigs>() {
        Some(configs) => configs.lookup(root, path),
        None => EditorConfigs::default().lookup(root, path),
    })
}

pub fn style_for(session: &Session, path: &Path) -> Option<Style> {
    let found = found_for(session, path)?;
    Some(Style::from_properties(&found.properties, &code_style::language_of(path))).filter(|style| !style.is_empty())
}

pub fn margins_over(session: &Session, path: &Path, margins: (u16, Vec<u16>)) -> (u16, Vec<u16>) {
    let Some(style) = style_for(session, path) else { return margins };
    let (margin, guides) = margins;
    let margin = match style.right_margin {
        Some(RightMargin::Column(column)) => column,
        Some(RightMargin::Off) | None => margin,
    };
    (margin, style.visual_guides.unwrap_or(guides))
}

pub fn end_of_line(session: &Session, path: &Path) -> Option<zen_text::LineSeparator> {
    found_for(session, path)?.properties.get("end_of_line").and_then(|value| zen_text::LineSeparator::parse(value))
}

fn message(kind: &ProblemKind) -> String {
    match kind {
        ProblemKind::NotAProperty(text) => t!("workbench-editorconfig-not-a-property", text = text.clone()),
        ProblemKind::UnclosedSection(text) => t!("workbench-editorconfig-unclosed-section", text = text.clone()),
        ProblemKind::OutsideSection(name) => t!("workbench-editorconfig-outside-section", name = name.clone()),
        ProblemKind::NoName => t!("workbench-editorconfig-no-name"),
        ProblemKind::InvalidValue { name, value } => {
            t!("workbench-editorconfig-invalid-value", name = name.clone(), value = value.clone())
        }
    }
}

pub(crate) fn problems_and_sources(session: &mut Session) -> (Vec<SettingsProblem>, Vec<PathBuf>) {
    if !enabled(session.settings()) {
        return (Vec::new(), Vec::new());
    }
    let Some(root) = project_root(session).map(Path::to_owned) else { return (Vec::new(), Vec::new()) };
    let listed: Vec<PathBuf> = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.index.as_ref())
        .filter(|index| index.is_ready())
        .map(|index| {
            index.file_list().into_iter().map(|(path, _)| path).filter(|path| path.ends_with(FILE_NAME)).collect()
        })
        .unwrap_or_default();
    let configs = session.state_mut::<EditorConfigs>();
    configs.recheck_now();
    for path in listed {
        if let Some(folder) = path.parent() {
            configs.file_in(folder);
        }
    }
    let mut problems = Vec::new();
    let mut sources = Vec::new();
    for (path, file) in configs.known().into_iter().filter(|(path, _)| path.starts_with(&root)) {
        for problem in &file.problems {
            let message = t!("workbench-editorconfig-problem", line = problem.line, message = message(&problem.kind));
            problems.push(SettingsProblem { file: path.clone(), message });
        }
        sources.push(path);
    }
    (problems, sources)
}

pub(crate) fn files_changed(session: &mut Session, changed: Option<&[PathBuf]>) {
    let ours =
        changed.is_none_or(|paths| paths.iter().any(|path| path.file_name().is_some_and(|name| name == FILE_NAME)));
    if ours && let Some(configs) = session.state::<EditorConfigs>() {
        configs.forget();
    }
}

/// The parameters of `project.editorConfig.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EditorConfigParams {
    /// A file of the project, absolute or relative to it; the active editor's file when absent.
    pub path: Option<PathBuf>,
}

/// What `project.editorConfig.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditorConfigAnswer {
    /// The file.
    pub path: PathBuf,
    /// Whether EditorConfig support is on (Settings → Editor → Code Style → General).
    pub enabled: bool,
    /// The `.editorconfig` files with a section that matches the file, the nearest first.
    pub files: Vec<PathBuf>,
    /// The EditorConfig properties of the file, by name; `unset` ones are left out.
    pub properties: BTreeMap<String, String>,
    /// The code style those properties set; `null` fields are left to the project and the IDE.
    pub style: Style,
    /// The code style the file is edited with: the IDE's, the project's and EditorConfig's in that
    /// order, as Tab, Enter, ⇧Tab, the status bar and Reformat Code use it.
    pub code_style: FileCodeStyle,
    /// What the properties do when the file is saved (`trim_trailing_whitespace`,
    /// `insert_final_newline`, `end_of_line`) and opened (`charset`).
    pub save: SaveProperties,
}

pub struct GetEditorConfig;

impl Command for GetEditorConfig {
    type Params = EditorConfigParams;
    type Output = EditorConfigAnswer;
    const ID: &'static str = "project.editorConfig.get";
    const TITLE: &'static str = "EditorConfig of File";
    const DESCRIPTION: &'static str = "Tells what the project's .editorconfig files give a file (the active editor's \
         by default): the files that match it, its properties, the code style they set (indent_style, indent_size, \
         tab_width, max_line_length, ij_continuation_indent_size, ij_visual_guides) and the code style the file is \
         edited with, EditorConfig above the project's and the IDE's, and save: trim_trailing_whitespace and \
         insert_final_newline (which win over Editor → General → On Save), end_of_line (applied to a modified file \
         on save) and charset (the encoding it is opened and saved in; a character it cannot hold refuses the \
         save). Parse errors are in project.settings.get's \
         problems; Settings → Editor → Code Style → General → Enable EditorConfig support (the editorConfig \
         setting) turns it off.";

    fn run(&self, session: &mut Session, params: EditorConfigParams) -> Result<EditorConfigAnswer, CommandError> {
        let workbench =
            session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let path = match params.path {
            Some(path) => workbench.resolve(&path),
            None => workbench
                .active_document()
                .map(|document| document.path().to_owned())
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        };
        if workbench.project().is_none_or(|root| !path.starts_with(root)) {
            return Err(CommandError::new(t!("workbench-editorconfig-outside", path = crate::display(&path))));
        }
        session.state_mut::<EditorConfigs>();
        let found = found_for(session, &path).unwrap_or_default();
        let style = Style::from_properties(&found.properties, &code_style::language_of(&path));
        let save = SaveProperties::from_properties(&found.properties);
        Ok(EditorConfigAnswer {
            save,
            enabled: enabled(session.settings()),
            files: found.files,
            properties: found.properties,
            style,
            code_style: project_settings::file_code_style(session, &path),
            path,
        })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetEditorConfig)?;
    registry.register(OpenEditorConfig)?;
    registry.register(ListEditorConfigs)?;
    registry.register(CreateEditorConfig)?;
    registry.register(ExportEditorConfig)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod save_tests;

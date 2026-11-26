use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Session};
use zen_i18n::t;
use zen_project::editorconfig::FILE_NAME;

use super::write::{Stubs, scheme_text, stub_text};
use crate::{Workbench, display, open_document};

pub const EXISTS: &str = "editorConfigExists";

fn project_root(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn inside(session: &Session, path: Option<&PathBuf>) -> Result<PathBuf, CommandError> {
    let root = project_root(session)?;
    let path = match path {
        Some(path) => session.state::<Workbench>().map_or_else(|| path.clone(), |workbench| workbench.resolve(path)),
        None => root.clone(),
    };
    if !crate::inside_project(&root, &path) {
        return Err(CommandError::new(t!("workbench-editorconfig-outside", path = display(&path))));
    }
    Ok(path)
}

fn write_new(session: &mut Session, path: &Path, text: &str, overwrite: bool) -> Result<(), CommandError> {
    if path.exists() && !overwrite {
        return Err(CommandError::with_code(EXISTS, t!("workbench-editorconfig-exists", path = display(path))));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            CommandError::new(t!("workbench-error-create", path = display(parent), error = error.to_string()))
        })?;
    }
    let before = std::fs::read(path).ok();
    zen_vfs::replace_unchanged(path, before.as_deref(), text.as_bytes()).map_err(|error| {
        CommandError::new(t!("workbench-error-create", path = display(path), error = error.source.to_string()))
    })?;
    if before.is_some() {
        let written = vec![(path.to_owned(), before, Some(text.as_bytes().to_vec()))];
        crate::local_history::changed_by(session, crate::local_history::EDITORCONFIG_EXPORT, written);
    }
    let workbench = session.state_mut::<Workbench>();
    workbench.revision += 1;
    let open = workbench.documents.iter_mut().find(|document| document.path() == path);
    if let Some(document) = open {
        document.sync_with_disk();
    }
    super::files_changed(session, Some(&[path.to_owned()]));
    Ok(())
}

/// The parameters of `project.editorConfig.open`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct OpenParams {
    /// A file of the project, absolute or relative to it; the active editor's file when absent.
    pub path: Option<PathBuf>,
}

pub struct OpenEditorConfig;

impl Command for OpenEditorConfig {
    type Params = OpenParams;
    type Output = PathBuf;
    const ID: &'static str = "project.editorConfig.open";
    const TITLE: &'static str = "Open .editorconfig";
    const DESCRIPTION: &'static str = "Opens in the editor the nearest .editorconfig file with a section that matches a \
         file (the active editor's by default), as the status bar's indentation widget does; answers its path.";

    fn is_available(&self, session: &Session) -> bool {
        let active = session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path());
        active.is_some_and(|path| super::found_for(session, path).is_some_and(|found| !found.files.is_empty()))
    }

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<PathBuf, CommandError> {
        let path = match params.path {
            Some(path) => inside(session, Some(&path))?,
            None => session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .map(|document| document.path().to_owned())
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        };
        session.state_mut::<super::EditorConfigs>();
        let nearest = super::found_for(session, &path).and_then(|found| found.files.into_iter().next());
        let nearest =
            nearest.ok_or_else(|| CommandError::new(t!("workbench-editorconfig-none", path = display(&path))))?;
        open_document(session, &nearest)?;
        Ok(nearest)
    }
}

/// One `.editorconfig` file of the project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditorConfigFile {
    /// The file.
    pub path: PathBuf,
    /// Its path relative to the project, with `/`.
    pub relative: String,
    /// Whether it has `root = true`.
    pub root: bool,
    /// How many of its lines could not be read (`project.settings.get` lists them).
    pub problems: usize,
}

pub struct ListEditorConfigs;

impl Command for ListEditorConfigs {
    type Params = NoParams;
    type Output = Vec<EditorConfigFile>;
    const ID: &'static str = "project.editorConfig.list";
    const TITLE: &'static str = "Show Files Related to Project";
    const DESCRIPTION: &'static str = "Lists the project's .editorconfig files (the indentation widget's Show Files \
         Related to Project…), by path, whether each is a root file and how many of its lines cannot be read, \
         whether EditorConfig support is on or off.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<EditorConfigFile>, CommandError> {
        let root = project_root(session)?;
        let indexed = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.index.as_ref())
            .filter(|index| index.is_ready())
            .map(zen_index::FileIndex::file_list);
        let listed = indexed.unwrap_or_else(|| zen_index::list_files(&root));
        let mut paths: Vec<PathBuf> =
            listed.into_iter().map(|(path, _)| path).filter(|path| path.ends_with(FILE_NAME)).collect();
        if root.join(FILE_NAME).is_file() && !paths.contains(&root.join(FILE_NAME)) {
            paths.push(root.join(FILE_NAME));
        }
        paths.sort();
        let configs = session.state_mut::<super::EditorConfigs>();
        Ok(paths
            .into_iter()
            .filter_map(|path| {
                let file = configs.file_in(path.parent()?)?;
                let relative = path.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/");
                Some(EditorConfigFile { relative, root: file.root, problems: file.problems.len(), path })
            })
            .collect())
    }
}

/// The parameters of `project.editorConfig.create`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateParams {
    /// The folder of the new file, absolute or relative to the project; the project's root when absent.
    pub folder: Option<PathBuf>,
    /// The property stubs; `root`, the standard properties and no IntelliJ ones when absent.
    pub stubs: Option<Stubs>,
}

pub struct CreateEditorConfig;

impl Command for CreateEditorConfig {
    type Params = CreateParams;
    type Output = PathBuf;
    const ID: &'static str = "project.editorConfig.create";
    const TITLE: &'static str = "EditorConfig File";
    const DESCRIPTION: &'static str = "Creates an .editorconfig file in a project folder (the root by default) with the \
         chosen property stubs (stubs.root: root = true; stubs.standard: charset, end_of_line, indent_size, \
         indent_style, insert_final_newline, max_line_length, tab_width, trim_trailing_whitespace; stubs.intellij: \
         ij_continuation_indent_size, ij_visual_guides), each set to what the folder's files get now, and opens it. \
         Refused with code editorConfigExists when the folder has one.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<PathBuf, CommandError> {
        let folder = inside(session, params.folder.as_ref())?;
        if !folder.is_dir() {
            return Err(CommandError::new(t!("workbench-error-outside-project", path = display(&folder))));
        }
        let path = folder.join(FILE_NAME);
        let text = stub_text(session, &folder, params.stubs.unwrap_or_default());
        write_new(session, &path, &text, false)?;
        open_document(session, &path)?;
        Ok(path)
    }
}

/// The parameters of `project.editorConfig.export`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportParams {
    /// Where to write the file, absolute or relative to the project; `.editorconfig` at the
    /// project's root when absent.
    pub path: Option<PathBuf>,
    /// Replace the file when it exists; otherwise that is refused with code editorConfigExists.
    pub overwrite: bool,
}

/// What `project.editorConfig.export` wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    /// The file written.
    pub path: PathBuf,
    /// Its text.
    pub text: String,
}

pub struct ExportEditorConfig;

impl Command for ExportEditorConfig {
    type Params = ExportParams;
    type Output = Exported;
    const ID: &'static str = "project.editorConfig.export";
    const TITLE: &'static str = "Export Code Style as EditorConfig File";
    const DESCRIPTION: &'static str = "Writes the current code style scheme (Editor → Code Style with the project's own \
         .idea code style on top) as an .editorconfig file: root = true, [*] with end_of_line (unless the line \
         separator is System-Dependent), indent_size, indent_style, max_line_length, tab_width, \
         ij_continuation_indent_size and ij_visual_guides, then a section per language whose indentation differs. \
         Writes <project>/.editorconfig by default; an existing file is replaced only with overwrite.";

    fn run(&self, session: &mut Session, params: ExportParams) -> Result<Exported, CommandError> {
        let path = match params.path {
            Some(path) => inside(session, Some(&path))?,
            None => project_root(session)?.join(FILE_NAME),
        };
        let text = scheme_text(session, &path);
        write_new(session, &path, &text, params.overwrite)?;
        Ok(Exported { path, text })
    }
}

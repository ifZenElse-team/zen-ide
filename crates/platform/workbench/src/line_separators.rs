use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_text::LineSeparator;

use crate::{Document, Workbench, display};

pub const NEW_FILES: &str = "editor.lineSeparator.newFiles";

pub const SYSTEM: &str = "system";

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetLineSeparator)?;
    registry.register(SetLineSeparator)?;
    Ok(())
}

pub fn for_new_file(session: &Session, path: &Path) -> LineSeparator {
    crate::editor_config::end_of_line(session, path).unwrap_or_else(|| chosen_separator(&chosen(session)))
}

pub(crate) fn chosen(session: &Session) -> String {
    session.settings().get_or(NEW_FILES, SYSTEM.to_owned())
}

fn chosen_separator(chosen: &str) -> LineSeparator {
    LineSeparator::parse(chosen).unwrap_or_else(LineSeparator::system)
}

pub(crate) fn settle(session: &Session, document: &mut Document) {
    if !document.line_separator_found() {
        let separator = for_new_file(session, document.path());
        document.default_line_separator(separator);
    }
}

/// A file whose line separator is wanted.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct GetParams {
    /// The file; the active editor's when absent.
    pub path: Option<PathBuf>,
}

/// A file's line separator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeparatorInfo {
    /// The file.
    pub path: PathBuf,
    /// The separator: the one of its first line break, else the one new files get.
    pub separator: LineSeparator,
    /// How the status bar shows it: `LF`, `CRLF` or `CR`.
    pub label: String,
    /// Whether the file has a line break the separator was read from.
    pub found: bool,
    /// Whether the file uses more than one kind of line separator.
    pub mixed: bool,
}

pub struct GetLineSeparator;

impl Command for GetLineSeparator {
    type Params = GetParams;
    type Output = SeparatorInfo;
    const ID: &'static str = "file.lineSeparator.get";
    const TITLE: &'static str = "Line Separator";
    const DESCRIPTION: &'static str = "The line separator of a file (the active editor's by default) as the status bar \
         shows it: the separator of its first line break (lf, crlf or cr), else the one new files get (Code Style or \
         EditorConfig end_of_line), and whether the file mixes separators. An open file is read from its editor.";

    fn run(&self, session: &mut Session, params: GetParams) -> Result<SeparatorInfo, CommandError> {
        let workbench = session.state::<Workbench>();
        let path = match params.path {
            Some(path) => workbench.map_or(path.clone(), |workbench| workbench.resolve(&path)),
            None => workbench
                .and_then(Workbench::active_document)
                .map(|document| document.path().to_owned())
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        };
        let open = workbench.and_then(|workbench| workbench.documents.iter().find(|document| document.path() == path));
        let (separator, found, mixed) = match open {
            Some(document) => (
                document.line_separator(),
                document.line_separator_found(),
                zen_text::line_separator::is_mixed(document.text()),
            ),
            None => {
                let text = zen_vfs::read_text(&path).map_err(|error| {
                    CommandError::new(t!("workbench-error-open", path = display(&path), error = error.to_string()))
                })?;
                let detected = zen_text::line_separator::detect(&text);
                let separator = detected.unwrap_or_else(|| for_new_file(session, &path));
                (separator, detected.is_some(), zen_text::line_separator::is_mixed(&text))
            }
        };
        Ok(SeparatorInfo { path, separator, label: separator.label().to_owned(), found, mixed })
    }
}

/// Files or folders whose line separators to change.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SetParams {
    /// The separator to write: `lf`, `crlf` or `cr`.
    pub separator: LineSeparator,
    /// Files and folders of the project (a folder: all its text files, recursively); the active
    /// editor's file when absent or empty.
    #[serde(default)]
    pub paths: Vec<PathBuf>,
}

/// What changing line separators did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeparatorsChanged {
    /// The files written with the new separator.
    pub changed: Vec<PathBuf>,
    /// The files left as they were because they cannot be changed: not text, not UTF-8, or read-only.
    pub skipped: Vec<PathBuf>,
}

pub struct SetLineSeparator;

impl Command for SetLineSeparator {
    type Params = SetParams;
    type Output = SeparatorsChanged;
    const ID: &'static str = "file.lineSeparator.set";
    const TITLE: &'static str = "Line Separators";
    const DESCRIPTION: &'static str = "Writes every line break of files with another separator (lf, crlf or cr), as the \
         status bar's Line Separator widget and File → File Properties → Line Separators do: the active editor's file by \
         default, or the given files and folders of the project (a folder: all its text files, recursively, without \
         .git and excluded folders inside it). An open file changes as one undo step and is saved at once; every file \
         written gets a Local History entry. Returns the files changed and those skipped.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| {
            workbench.project().is_some()
                || workbench.active_document().is_some_and(|document| document.image().is_none())
        })
    }

    fn run(&self, session: &mut Session, params: SetParams) -> Result<SeparatorsChanged, CommandError> {
        let targets = checked_targets(session, &params.paths)?;
        let project = project_of(session);
        let hidden = crate::hidden_files::for_caller(session);
        let mut done = SeparatorsChanged::default();
        let mut written = Vec::new();
        let mut failure = None;
        for target in targets {
            let files = files_of(session, &target, hidden.as_deref(), project.as_deref());
            let single = files.len() == 1 && files[0] == target;
            for file in files {
                match convert_file(session, &file, params.separator) {
                    Ok(Some((before, after))) => {
                        written.push((file.clone(), before, Some(after)));
                        done.changed.push(file);
                    }
                    Ok(None) => {}
                    Err(Skip::NotText(reason)) if single => {
                        failure = Some(CommandError::new(reason));
                    }
                    Err(Skip::NotText(_)) => done.skipped.push(file),
                    Err(Skip::Failed(error)) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            if failure.is_some() {
                break;
            }
        }
        if !written.is_empty() {
            session.state_mut::<Workbench>().revision += 1;
        }
        crate::local_history::changed_by(session, crate::local_history::LINE_SEPARATORS, written);
        match failure {
            Some(error) => Err(error),
            None => Ok(done),
        }
    }
}

fn project_of(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

pub(crate) fn checked_targets(session: &Session, paths: &[PathBuf]) -> Result<Vec<PathBuf>, CommandError> {
    let workbench = session.state::<Workbench>();
    let targets: Vec<PathBuf> = if paths.is_empty() {
        let active = workbench.and_then(Workbench::active_document);
        vec![
            active
                .map(|document| document.path().to_owned())
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        ]
    } else {
        paths.iter().map(|path| workbench.map_or(path.clone(), |workbench| workbench.resolve(path))).collect()
    };
    let project = project_of(session);
    for target in &targets {
        let open =
            workbench.is_some_and(|workbench| workbench.documents.iter().any(|document| document.path() == target));
        if !open && project.as_deref().is_none_or(|root| !crate::inside_project(root, target)) {
            return Err(CommandError::new(t!("workbench-error-separators-outside", path = display(target))));
        }
        if crate::guard::refuses_in(project.as_deref(), target) {
            return Err(CommandError::new(crate::guard::refusal(target)));
        }
    }
    let hidden = crate::hidden_files::for_caller(session);
    if let (Some(patterns), Some(root)) = (&hidden, project.as_deref())
        && let Some(file) = targets.iter().find(|target| crate::hidden_files::is_hidden(patterns, Some(root), target))
    {
        return Err(crate::hidden_files::refused(&display(file)));
    }
    Ok(targets)
}

pub(crate) fn files_of(
    session: &Session,
    target: &Path,
    hidden: Option<&[String]>,
    project: Option<&Path>,
) -> Vec<PathBuf> {
    if !target.is_dir() {
        return vec![target.to_owned()];
    }
    let exclusions = session
        .state::<crate::project_settings::ProjectSettingsState>()
        .map(|state| zen_index::Exclusions::new(&state.settings().excluded, &state.settings().exclude_patterns))
        .unwrap_or_default();
    let mut files = text_files(target, project, &exclusions);
    if let Some(patterns) = hidden {
        files.retain(|file| !crate::hidden_files::is_hidden(patterns, project, file));
    }
    files
}

const MAX_FILE_BYTES: u64 = 16 << 20;

enum Skip {
    NotText(String),
    Failed(CommandError),
}

pub(crate) type Rewritten = (Option<Vec<u8>>, Vec<u8>);

fn convert_file(session: &mut Session, file: &Path, separator: LineSeparator) -> Result<Option<Rewritten>, Skip> {
    let failed = |error: zen_vfs::VfsError| {
        Skip::Failed(CommandError::new(t!(
            "workbench-error-save",
            file = display(file),
            error = error.source.to_string()
        )))
    };
    let charset = crate::project_settings::charset_for(session, file);
    let foreign = crate::editor_config::charset(session, file).is_some_and(|charset| !charset.is_utf8());
    let workbench = session.state_mut::<Workbench>();
    if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == file) {
        if document.image().is_some() {
            return Err(Skip::NotText(t!("workbench-error-separators-not-text", file = display(file))));
        }
        if let Some(reason) = document.read_only() {
            return Err(Skip::NotText(reason.to_owned()));
        }
        if std::fs::metadata(file).is_ok_and(|metadata| metadata.permissions().readonly()) {
            return Err(Skip::NotText(t!("workbench-error-separators-read-only", file = display(file))));
        }
        return document.convert_and_write(separator).map_err(failed);
    }
    let not_text = || Skip::NotText(t!("workbench-error-separators-not-text", file = display(file)));
    if zen_image::opens_as_picture(file)
        || foreign
        || charset.is_some_and(|charset| !zen_project::settings::is_utf8_compatible(&charset))
    {
        return Err(not_text());
    }
    let metadata = std::fs::symlink_metadata(file).map_err(|error| {
        Skip::Failed(CommandError::new(t!("workbench-error-open", path = display(file), error = error.to_string())))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(Skip::NotText(t!("workbench-error-separators-link-or-large", file = display(file))));
    }
    if metadata.permissions().readonly() {
        return Err(Skip::NotText(t!("workbench-error-separators-read-only", file = display(file))));
    }
    let bytes = std::fs::read(file).map_err(|error| {
        Skip::Failed(CommandError::new(t!("workbench-error-open", path = display(file), error = error.to_string())))
    })?;
    if bytes.contains(&0) {
        return Err(not_text());
    }
    let Ok(text) = std::str::from_utf8(&bytes) else { return Err(not_text()) };
    let std::borrow::Cow::Owned(converted) = zen_text::line_separator::convert(text, separator) else {
        return Ok(None);
    };
    zen_vfs::replace_unchanged(file, Some(&bytes), converted.as_bytes()).map_err(failed)?;
    Ok(Some((Some(bytes), converted.into_bytes())))
}

fn text_files(folder: &Path, project: Option<&Path>, exclusions: &zen_index::Exclusions) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![folder.to_owned()];
    while let Some(current) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if zen_vfs::HIDDEN.contains(&name.as_str())
                || crate::guard::refuses_in(project, &path)
                || exclusions.excludes_entry(&path)
            {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests;

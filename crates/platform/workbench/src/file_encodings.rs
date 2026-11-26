use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::settings::{Encodings, FileEncoding, NewFileBom};

use crate::charsets::Charset;
use crate::project_settings::{self, ProjectSettingsState};
use crate::{Document, Workbench, display, editor_config, file_types, line_separators, local_history};

const MAX_FILE_BYTES: u64 = 16 << 20;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetFileEncoding)?;
    registry.register(ListCharsets)?;
    registry.register(SetFileEncoding)?;
    registry.register(AddBom)?;
    registry.register(RemoveBom)?;
    registry.register(GetEncodings)?;
    registry.register(SetEncodings)?;
    Ok(())
}

pub(crate) fn new_file_bytes(session: &Session, path: &Path, text: &str) -> Vec<u8> {
    let named = editor_config::charset(session, path)
        .or_else(|| project_settings::charset_for(session, path).and_then(|charset| Charset::named(&charset)));
    match named {
        Some(Charset::Utf8) | None if adds_bom_to_new_files(session) => {
            Charset::Utf8Bom.encode(text, true).unwrap_or_else(|_| text.as_bytes().to_vec())
        }
        Some(charset) => charset.encode(text, false).unwrap_or_else(|_| text.as_bytes().to_vec()),
        None => text.as_bytes().to_vec(),
    }
}

fn adds_bom_to_new_files(session: &Session) -> bool {
    session
        .state::<ProjectSettingsState>()
        .and_then(|state| state.settings().encodings.bom_for_new_files)
        .is_some_and(NewFileBom::adds)
}

fn document_at<'a>(session: &'a Session, path: Option<&Path>) -> Result<&'a Document, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let document = match path {
        Some(path) => {
            let path = workbench.resolve(path);
            workbench.documents().iter().find(|document| document.path() == path)
        }
        None => workbench.active_document(),
    };
    document.filter(|document| document.image().is_none()).ok_or_else(|| {
        CommandError::new(match path {
            Some(path) => t!("workbench-error-encoding-not-open", path = display(path)),
            None => t!("workbench-error-no-document"),
        })
    })
}

/// A file whose encoding is wanted.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FileParams {
    /// An open file; the active editor's when absent.
    pub path: Option<PathBuf>,
}

/// The encoding of an open file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileEncodingInfo {
    /// The file.
    pub path: PathBuf,
    /// The encoding, as the status bar shows it, e.g. `windows-1251` or `UTF-8 BOM`.
    pub charset: String,
    /// Whether the file starts with a byte order mark.
    pub bom: bool,
}

pub struct GetFileEncoding;

impl Command for GetFileEncoding {
    type Params = FileParams;
    type Output = FileEncodingInfo;
    const ID: &'static str = "file.encoding.get";
    const TITLE: &'static str = "File Encoding";
    const DESCRIPTION: &'static str = "The encoding of an open file (the active editor's by default), as the status \
         bar shows it, and whether it has a byte order mark. A file's encoding comes from its byte order mark, then \
         EditorConfig's charset, then the nearest entry of the Path/Encoding table, then Project Encoding, then \
         Global Encoding, then UTF-8.";

    fn run(&self, session: &mut Session, params: FileParams) -> Result<FileEncodingInfo, CommandError> {
        let document = document_at(session, params.path.as_deref())?;
        Ok(FileEncodingInfo {
            path: document.path().to_owned(),
            charset: document.charset().to_owned(),
            bom: document.encoding().1,
        })
    }
}

/// The encodings a file can be read and written in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CharsetList {
    /// Their names, in the order the status bar's list shows them.
    pub charsets: Vec<String>,
}

pub struct ListCharsets;

impl Command for ListCharsets {
    type Params = NoParams;
    type Output = CharsetList;
    const ID: &'static str = "file.encoding.list";
    const TITLE: &'static str = "List File Encodings";
    const DESCRIPTION: &'static str =
        "The encodings a file can be read and written in, as the status bar's encoding list shows them.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<CharsetList, CommandError> {
        Ok(CharsetList { charsets: Charset::choices().into_iter().map(|charset| charset.label().to_owned()).collect() })
    }
}

/// How an open file takes another encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EncodingAction {
    /// Read the file's bytes again in the encoding; the file stays as it is.
    Reload,
    /// Write the file's text in the encoding.
    Convert,
}

/// A new encoding for an open file.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetEncodingParams {
    /// The encoding's name, one file.encoding.list gives.
    pub charset: String,
    /// An open file; the active editor's when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// `reload` or `convert`. When absent, a change that loses nothing is made, and any other
    /// answers `applied: false` with what is possible, changing nothing.
    #[serde(default)]
    pub action: Option<EncodingAction>,
}

/// What asking for another encoding did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncodingChange {
    /// Whether the file now has the encoding; false means a choice is needed and nothing changed.
    pub applied: bool,
    /// The file's encoding after the call.
    pub charset: String,
    /// Whether `reload` is possible: the bytes on disk are text in the encoding and nothing is unsaved.
    pub can_reload: bool,
    /// Whether `convert` is possible: the file can be written and the encoding holds all of its text.
    pub can_convert: bool,
    /// Why `convert` is not possible, when a character is what stops it.
    pub unmappable: Option<String>,
    /// Whether the choice was kept in the Path/Encoding table.
    pub remembered: bool,
}

struct Plan {
    path: PathBuf,
    current: (Charset, bool),
    target: (Charset, bool),
    reloaded: Option<String>,
    converted: Result<Vec<u8>, String>,
    disk: Option<Vec<u8>>,
    dirty: bool,
}

impl Plan {
    fn of(document: &Document, charset: Charset) -> Plan {
        let current = document.encoding();
        let keeps_bom = current.1 && charset.holds_a_bom();
        let target = (charset, keeps_bom || charset == Charset::Utf8Bom);
        let disk = std::fs::read(document.path()).ok();
        let reloaded = disk.as_deref().and_then(|bytes| charset.decode(bytes)).map(|(text, _)| text);
        let converted = charset.encode(document.text(), target.1).map_err(|unmappable| {
            t!(
                "workbench-error-unmappable",
                charset = charset.label(),
                character = unmappable.character.to_string(),
                code = format!("U+{:04X}", u32::from(unmappable.character)),
                line = unmappable.line,
                column = unmappable.column
            )
        });
        Plan {
            path: document.path().to_owned(),
            current,
            target,
            reloaded,
            converted,
            disk,
            dirty: document.is_dirty(),
        }
    }

    fn can_reload(&self) -> bool {
        self.reloaded.is_some() && !self.dirty
    }

    fn loses_nothing(&self, document: &Document) -> bool {
        self.can_reload()
            && document.read_only().is_none()
            && self.reloaded.as_deref() == Some(document.text())
            && matches!((&self.converted, &self.disk), (Ok(written), Some(disk)) if written == disk)
    }

    fn answer(&self, document: &Document) -> EncodingChange {
        EncodingChange {
            applied: false,
            charset: document.charset().to_owned(),
            can_reload: self.can_reload(),
            can_convert: self.converted.is_ok() && document.read_only().is_none(),
            unmappable: self.converted.as_ref().err().cloned(),
            remembered: false,
        }
    }
}

fn same_encoding(current: Charset, chosen: Charset) -> bool {
    current == chosen || (current.is_utf8() && chosen.is_utf8())
}

pub struct SetFileEncoding;

impl Command for SetFileEncoding {
    type Params = SetEncodingParams;
    type Output = EncodingChange;
    const ID: &'static str = "file.encoding.set";
    const TITLE: &'static str = "Change File Encoding";
    const DESCRIPTION: &'static str = "Changes an open file's encoding, as the status bar's encoding list does: \
         `reload` reads the file's bytes again in the encoding and leaves the file as it is; `convert` writes the \
         file's text in the encoding. Without `action` a change that loses nothing is made at once, and any other \
         answers `applied: false` with canReload, canConvert and the first character the encoding cannot hold, \
         changing nothing. A change is kept in the Path/Encoding table of .idea/encodings.xml.";

    fn run(&self, session: &mut Session, params: SetEncodingParams) -> Result<EncodingChange, CommandError> {
        let charset = Charset::named(&params.charset).ok_or_else(|| {
            CommandError::new(t!("workbench-error-encoding-unknown", charset = params.charset.as_str()))
        })?;
        let document = document_at(session, params.path.as_deref())?;
        let path = document.path().to_owned();
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        if params.action == Some(EncodingAction::Convert) && crate::guard::refuses_in(root.as_deref(), &path) {
            return Err(CommandError::new(crate::guard::refusal(&path)));
        }
        if let Some(root) = root.as_deref().or(path.parent()) {
            crate::hidden_files::guard_change(session, root, &[&path])?;
        }
        let document = document_at(session, params.path.as_deref())?;
        let plan = Plan::of(document, charset);
        let mut answer = plan.answer(document);
        if same_encoding(plan.current.0, charset) {
            answer.applied = true;
            return Ok(answer);
        }
        let action = match params.action {
            Some(action) => Some(action),
            None if plan.loses_nothing(document) => None,
            None => return Ok(answer),
        };
        if action == Some(EncodingAction::Reload) && !answer.can_reload {
            return Err(CommandError::new(t!("workbench-error-encoding-reload", charset = charset.label())));
        }
        if action == Some(EncodingAction::Convert) && !answer.can_convert {
            let reason = answer.unmappable.clone().or_else(|| document.read_only().map(str::to_owned));
            return Err(CommandError::new(
                reason.unwrap_or_else(|| t!("workbench-error-encoding-reload", charset = charset.label())),
            ));
        }
        let workbench = session.state_mut::<Workbench>();
        let found = workbench.documents.iter_mut().find(|document| document.path() == plan.path);
        let document = found.ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        match action {
            Some(EncodingAction::Reload) => {
                document.set_encoding(plan.target.0, plan.target.1);
                document.reinterpret(plan.reloaded.clone().unwrap_or_default());
            }
            Some(EncodingAction::Convert) => document.convert(plan.target.0, plan.target.1).map_err(|error| {
                CommandError::new(t!(
                    "workbench-error-save",
                    file = display(&plan.path),
                    error = error.source.to_string()
                ))
            })?,
            None => document.set_encoding(plan.target.0, plan.target.1),
        }
        answer.charset = document.charset().to_owned();
        answer.applied = true;
        workbench.revision += 1;
        if action == Some(EncodingAction::Convert) {
            let written = (plan.path.clone(), plan.disk.clone(), std::fs::read(&plan.path).ok());
            local_history::changed_by(session, local_history::FILE_ENCODING, vec![written]);
        }
        answer.remembered = remember(session, &plan.path, charset);
        Ok(answer)
    }
}

fn remember(session: &mut Session, path: &Path, charset: Charset) -> bool {
    let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
        return false;
    };
    if !crate::inside_project(&root, path) {
        return false;
    }
    let Some(state) = session.state::<ProjectSettingsState>() else { return false };
    let encodings = state.settings().encodings.clone().with_file(path, None);
    let inherited = encodings
        .charset_for(path)
        .and_then(Charset::named)
        .or_else(|| project_settings::global_encoding(session).as_deref().and_then(Charset::named));
    let same = inherited.unwrap_or(Charset::Utf8) == charset;
    let encodings = if same { encodings } else { encodings.with_file(path, Some(charset.label())) };
    write_encodings(session, &root, &encodings).is_ok()
}

fn write_encodings(session: &mut Session, root: &Path, encodings: &Encodings) -> Result<(), String> {
    let state = session.state::<ProjectSettingsState>().ok_or("the project keeps no settings")?;
    let written = state.providers().iter().find_map(|provider| provider.set_encodings(root, encodings));
    written.unwrap_or_else(|| Err("no settings provider keeps encodings".to_owned()))?;
    session.state_mut::<ProjectSettingsState>().refresh(Some(root));
    Ok(())
}

/// Files or folders whose byte order mark to change.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct BomParams {
    /// Files and folders of the project (a folder: all its text files, recursively); the active
    /// editor's file when absent or empty.
    pub paths: Vec<PathBuf>,
}

/// What changing byte order marks did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BomChanged {
    /// The files written.
    pub changed: Vec<PathBuf>,
    /// The files left as they were: already so, not text, read-only, or in an encoding without a mark.
    pub skipped: Vec<PathBuf>,
}

fn charset_of_file(session: &Session, path: &Path, bytes: &[u8]) -> Option<Charset> {
    let named = project_settings::charset_for(session, path);
    Charset::of_bom(bytes)
        .or_else(|| editor_config::charset(session, path))
        .or(named.as_deref().map(Charset::named).unwrap_or(Some(Charset::Utf8)))
}

fn with_bom(charset: Charset, bytes: &[u8]) -> Option<Vec<u8>> {
    if !charset.holds_a_bom() || Charset::of_bom(bytes).is_some() || charset.decode(bytes).is_none() {
        return None;
    }
    let mark = charset.bom_bytes();
    let mut written = Vec::with_capacity(bytes.len() + mark.len());
    written.extend_from_slice(mark);
    written.extend_from_slice(bytes);
    Some(written)
}

fn without_bom(bytes: &[u8]) -> Option<Vec<u8>> {
    Charset::of_bom(bytes).map(|charset| bytes[charset.bom_bytes().len()..].to_vec())
}

fn change_bom(session: &mut Session, paths: &[PathBuf], add: bool) -> Result<BomChanged, CommandError> {
    let targets = line_separators::checked_targets(session, paths)?;
    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let hidden = crate::hidden_files::for_caller(session);
    let mut done = BomChanged::default();
    let mut written = Vec::new();
    for target in targets {
        for file in line_separators::files_of(session, &target, hidden.as_deref(), project.as_deref()) {
            match change_file_bom(session, &file, add) {
                Some(result) => {
                    written.push((file.clone(), Some(result.0), Some(result.1)));
                    done.changed.push(file);
                }
                None => done.skipped.push(file),
            }
        }
    }
    if !written.is_empty() {
        session.state_mut::<Workbench>().revision += 1;
    }
    local_history::changed_by(session, local_history::BYTE_ORDER_MARK, written);
    Ok(done)
}

fn change_file_bom(session: &mut Session, file: &Path, add: bool) -> Option<(Vec<u8>, Vec<u8>)> {
    let metadata = std::fs::symlink_metadata(file).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES || metadata.permissions().readonly() {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    let head = &bytes[..bytes.len().min(file_types::HEAD)];
    let charset = charset_of_file(session, file, &bytes)?;
    if charset.is_utf8() && file_types::is_binary(head) && Charset::of_bom(&bytes).is_none() {
        return None;
    }
    let open = session.state::<Workbench>()?.documents().iter().find(|document| document.path() == file);
    if open.is_some_and(|document| document.read_only().is_some()) {
        return None;
    }
    let after = if add {
        with_bom(charset, &bytes)?
    } else {
        charset.decode(&bytes)?;
        without_bom(&bytes)?
    };
    zen_vfs::write_atomic(file, &after).ok()?;
    let workbench = session.state_mut::<Workbench>();
    if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == file) {
        match (charset, add) {
            (Charset::Utf8 | Charset::Utf8Bom, true) => document.set_encoding(Charset::Utf8Bom, true),
            (Charset::Utf8 | Charset::Utf8Bom, false) => document.set_encoding(Charset::Utf8, false),
            (other, add) => document.set_encoding(other, add),
        }
    }
    Some((bytes, after))
}

pub struct AddBom;

impl Command for AddBom {
    type Params = BomParams;
    type Output = BomChanged;
    const ID: &'static str = "file.bom.add";
    const TITLE: &'static str = "Add BOM";
    const DESCRIPTION: &'static str = "Adds a byte order mark to UTF-8 and UTF-16 files, as File → File Properties → \
         Add BOM does: the active editor's file by default, or the given files and folders of the project (a folder: \
         all its text files, recursively). The file's text does not change, and an open file's unsaved edits stay \
         unsaved. Returns the files changed and those skipped.";

    fn run(&self, session: &mut Session, params: BomParams) -> Result<BomChanged, CommandError> {
        change_bom(session, &params.paths, true)
    }
}

pub struct RemoveBom;

impl Command for RemoveBom {
    type Params = BomParams;
    type Output = BomChanged;
    const ID: &'static str = "file.bom.remove";
    const TITLE: &'static str = "Remove BOM";
    const DESCRIPTION: &'static str = "Removes the byte order mark of files, as File → File Properties → Remove BOM \
         does: the active editor's file by default, or the given files and folders of the project (a folder: all its \
         text files, recursively). The file's text does not change, and an open file's unsaved edits stay unsaved. \
         Returns the files changed and those skipped.";

    fn run(&self, session: &mut Session, params: BomParams) -> Result<BomChanged, CommandError> {
        change_bom(session, &params.paths, false)
    }
}

/// Settings → Editor → File Encodings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EncodingSettings {
    /// Global Encoding, kept in the IDE's settings; absent means UTF-8.
    pub global: Option<String>,
    /// Project Encoding; absent means Global Encoding.
    pub project: Option<String>,
    /// The Path/Encoding table: files and folders with an encoding of their own.
    pub files: Vec<FileEncoding>,
    /// Create UTF-8 files; absent means without a byte order mark.
    pub bom_for_new_files: Option<NewFileBom>,
}

fn require_project(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

pub struct GetEncodings;

impl Command for GetEncodings {
    type Params = NoParams;
    type Output = EncodingSettings;
    const ID: &'static str = "project.encodings.get";
    const TITLE: &'static str = "Get File Encodings Settings";
    const DESCRIPTION: &'static str = "The content of Settings → Editor → File Encodings: Global Encoding, Project \
         Encoding, the Path/Encoding table and Create UTF-8 files.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EncodingSettings, CommandError> {
        let encodings =
            session.state::<ProjectSettingsState>().map(|state| state.settings().encodings.clone()).unwrap_or_default();
        Ok(EncodingSettings {
            global: project_settings::global_encoding(session),
            project: encodings.default,
            files: encodings.files,
            bom_for_new_files: encodings.bom_for_new_files,
        })
    }
}

/// The project's part of Settings → Editor → File Encodings.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SetEncodingsParams {
    /// Project Encoding; absent means Global Encoding.
    pub project: Option<String>,
    /// The Path/Encoding table: files and folders with an encoding of their own.
    pub files: Vec<FileEncoding>,
    /// Create UTF-8 files; absent means without a byte order mark.
    pub bom_for_new_files: Option<NewFileBom>,
}

pub struct SetEncodings;

impl Command for SetEncodings {
    type Params = SetEncodingsParams;
    type Output = EncodingSettings;
    const ID: &'static str = "project.encodings.set";
    const TITLE: &'static str = "Set File Encodings Settings";
    const DESCRIPTION: &'static str = "Replaces Project Encoding, the Path/Encoding table and Create UTF-8 files in \
         .idea/encodings.xml, leaving the rest of the file as it was. Relative paths are the project's. Files already \
         open keep their encoding until they are opened again. Global Encoding is the IDE setting globalEncoding.";

    fn run(&self, session: &mut Session, params: SetEncodingsParams) -> Result<EncodingSettings, CommandError> {
        let root = require_project(session)?;
        let names = params.files.iter().map(|file| file.charset.as_str());
        for name in names.chain(params.project.as_deref()) {
            if Charset::named(name).is_none() {
                return Err(CommandError::new(t!("workbench-error-encoding-unknown", charset = name)));
            }
        }
        let files = params
            .files
            .iter()
            .map(|file| FileEncoding { path: root.join(&file.path), charset: file.charset.trim().to_owned() })
            .collect();
        let encodings =
            Encodings { default: params.project.clone(), files, bom_for_new_files: params.bom_for_new_files };
        write_encodings(session, &root, &encodings)
            .map_err(|error| CommandError::new(t!("workbench-error-encodings-write", error = error)))?;
        GetEncodings.run(session, NoParams::default())
    }
}

#[cfg(test)]
mod tests;

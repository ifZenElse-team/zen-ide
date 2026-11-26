use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::{Workbench, display, history, inside_project, open_document};

pub const BUFFERS: usize = 5;

pub const PLAIN_TEXT: &str = "txt";

/// A language a scratch file can be written in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScratchLanguage {
    /// A stable id, such as `go` or `json`.
    pub id: String,
    /// The name shown in the list, such as `Go` or `JSON`.
    pub name: String,
    /// The extension a scratch file in it gets, without the dot.
    pub extension: String,
}

pub type LanguageSource = Arc<dyn Fn() -> Vec<ScratchLanguage> + Send + Sync>;

pub type OpenElsewhere = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct Scratches {
    data: Option<PathBuf>,
    languages: Option<LanguageSource>,
    elsewhere: Option<OpenElsewhere>,
}

impl std::fmt::Debug for Scratches {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Scratches").field("data", &self.data).finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Memo {
    recent_languages: Vec<String>,
    last_buffer: usize,
}

const RECENT_KEPT: usize = 20;

impl Scratches {
    pub fn set_data_dir(&mut self, data: PathBuf) {
        self.data = Some(data);
    }

    pub fn set_languages(&mut self, source: LanguageSource) {
        self.languages = Some(source);
    }

    pub fn set_open_elsewhere(&mut self, elsewhere: OpenElsewhere) {
        self.elsewhere = Some(elsewhere);
    }

    pub fn scratches_folder(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|data| data.join("scratches"))
    }

    pub fn buffers_folder(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|data| data.join("buffers"))
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        [self.scratches_folder(), self.buffers_folder()].into_iter().flatten().collect()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.folders()
            .iter()
            .any(|folder| path != folder && inside_project(folder, path) && resolves_inside(folder, path))
    }

    pub fn languages(&self) -> Vec<ScratchLanguage> {
        let mut languages = built_in_languages();
        if let Some(source) = &self.languages {
            for language in source() {
                let taken = languages
                    .iter()
                    .any(|known| known.extension.eq_ignore_ascii_case(&language.extension) || known.id == language.id);
                if !taken && !language.extension.is_empty() {
                    languages.push(language);
                }
            }
        }
        languages.sort_by_key(|language| language.name.to_lowercase());
        let recent = self.memo().recent_languages;
        languages.sort_by_key(|language| recent.iter().position(|id| *id == language.id).unwrap_or(usize::MAX));
        languages
    }

    fn memo_file(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|data| data.join("scratches.json"))
    }

    fn memo(&self) -> Memo {
        self.memo_file()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save_memo(&self, memo: &Memo) {
        if let (Some(file), Ok(bytes)) = (self.memo_file(), serde_json::to_vec_pretty(memo)) {
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(file, bytes);
        }
    }

    fn used_language(&self, id: &str) {
        let mut memo = self.memo();
        memo.recent_languages.retain(|recent| recent != id);
        memo.recent_languages.insert(0, id.to_owned());
        memo.recent_languages.truncate(RECENT_KEPT);
        self.save_memo(&memo);
    }
}

fn resolves_inside(folder: &Path, path: &Path) -> bool {
    let (Some(parent), Ok(folder)) = (path.parent(), std::fs::canonicalize(folder)) else { return false };
    std::fs::canonicalize(parent).is_ok_and(|parent| parent.starts_with(folder))
}

fn built_in_languages() -> Vec<ScratchLanguage> {
    let language = |id: &str, name: String, extension: &str| ScratchLanguage {
        id: id.to_owned(),
        name,
        extension: extension.to_owned(),
    };
    vec![
        language("text", t!("workbench-scratch-plain-text"), PLAIN_TEXT),
        language("go", "Go".to_owned(), "go"),
        language("rust", "Rust".to_owned(), "rs"),
    ]
}

pub fn find_language(languages: &[ScratchLanguage], key: &str) -> Option<ScratchLanguage> {
    let key = key.trim().trim_start_matches('.');
    if key.is_empty() {
        return None;
    }
    let found = languages.iter().find(|language| {
        language.id.eq_ignore_ascii_case(key)
            || language.name.eq_ignore_ascii_case(key)
            || language.extension.eq_ignore_ascii_case(key)
    });
    if let Some(found) = found {
        return Some(found.clone());
    }
    let plain = key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'));
    plain.then(|| ScratchLanguage { id: key.to_lowercase(), name: key.to_owned(), extension: key.to_owned() })
}

pub fn language_of(languages: &[ScratchLanguage], path: &Path) -> ScratchLanguage {
    let extension = path.extension().map(|extension| extension.to_string_lossy().into_owned());
    extension
        .and_then(|extension| find_language(languages, &extension))
        .or_else(|| find_language(languages, PLAIN_TEXT))
        .unwrap_or_else(|| ScratchLanguage {
            id: "text".to_owned(),
            name: PLAIN_TEXT.to_owned(),
            extension: PLAIN_TEXT.to_owned(),
        })
}

pub fn free_scratch_name(folder: &Path, stem: &str, extension: &str) -> PathBuf {
    let name = |suffix: String| folder.join(format!("{stem}{suffix}.{extension}"));
    let first = name(String::new());
    if !first.exists() {
        return first;
    }
    (1..).map(|n| name(format!("_{n}"))).find(|path| !path.exists()).unwrap_or(first)
}

pub(crate) fn create_new(folder: &Path, stem: &str, extension: &str, text: &str) -> std::io::Result<PathBuf> {
    loop {
        let path = free_scratch_name(folder, stem, extension);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                std::io::Write::write_all(&mut file, text.as_bytes())?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

pub fn buffer_name(slot: usize) -> String {
    format!("buffer{slot}.txt")
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListLanguages)?;
    registry.register(NewScratch)?;
    registry.register(ScratchFromSelection)?;
    registry.register(NewBuffer)?;
    registry.register(ListScratches)?;
    registry.register(ChangeLanguage)?;
    registry.register(MoveScratch)?;
    Ok(())
}

fn scratches(session: &Session) -> Result<Scratches, CommandError> {
    session
        .state::<Scratches>()
        .filter(|scratches| scratches.data.is_some())
        .cloned()
        .ok_or_else(|| CommandError::new(t!("workbench-scratch-no-folder")))
}

fn create_folder(folder: &Path) -> Result<(), CommandError> {
    std::fs::create_dir_all(folder).map_err(|error| {
        CommandError::new(t!("workbench-error-create", path = display(folder), error = error.to_string()))
    })
}

pub(crate) fn create_scratch(
    session: &mut Session,
    language: &ScratchLanguage,
    text: &str,
) -> Result<PathBuf, CommandError> {
    let scratches = scratches(session)?;
    let folder = scratches.scratches_folder().ok_or_else(|| CommandError::new(t!("workbench-scratch-no-folder")))?;
    create_folder(&folder)?;
    let path = create_new(&folder, "scratch", &language.extension, text).map_err(|error| {
        CommandError::new(t!("workbench-error-create", path = display(&folder), error = error.to_string()))
    })?;
    scratches.used_language(&language.id);
    let origin = history::caret(session);
    open_document(session, &path)?;
    session.state_mut::<Workbench>().revision += 1;
    history::left_file(session, origin);
    Ok(path)
}

/// The parameters of `file.scratch.languages`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct LanguagesParams {
    /// Only the languages whose name, id or extension contains this text, in any case.
    #[serde(default)]
    pub query: Option<String>,
}

pub struct ListLanguages;

impl Command for ListLanguages {
    type Params = LanguagesParams;
    type Output = Vec<ScratchLanguage>;
    const ID: &'static str = "file.scratch.languages";
    const TITLE: &'static str = "Scratch File Languages";
    const DESCRIPTION: &'static str = "Lists the languages a scratch file can be created in (file.scratch.new), the most \
         recently used first, then by name; `query` keeps those whose name, id or extension contains it.";

    fn run(&self, session: &mut Session, params: LanguagesParams) -> Result<Vec<ScratchLanguage>, CommandError> {
        let mut languages = scratches(session)?.languages();
        if let Some(query) = params.query.map(|query| query.trim().to_lowercase()).filter(|query| !query.is_empty()) {
            languages.retain(|language| {
                [&language.name, &language.id, &language.extension]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&query))
            });
        }
        Ok(languages)
    }
}

/// The parameters of `file.scratch.new`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct NewScratchParams {
    /// The language: an id, a name or an extension from `file.scratch.languages`; any other plain
    /// extension is taken as it is.
    pub language: String,
    /// The text the file starts with; by default what its language starts a scratch with, or nothing.
    #[serde(default)]
    pub text: Option<String>,
}

pub struct NewScratch;

impl Command for NewScratch {
    type Params = NewScratchParams;
    type Output = PathBuf;
    const ID: &'static str = "file.scratch.new";
    const TITLE: &'static str = "Scratch File";
    const DESCRIPTION: &'static str = "Creates a scratch file in a language in the IDE's scratches folder, shared by every \
         project (`scratch.<ext>`, then `scratch_1.<ext>`, ...), opens it and returns its path. Without `text` a Go \
         scratch starts as a runnable `main`, the caret inside. Closing it while it is empty deletes it.";

    fn run(&self, session: &mut Session, params: NewScratchParams) -> Result<PathBuf, CommandError> {
        let languages = scratches(session)?.languages();
        let language = find_language(&languages, &params.language).ok_or_else(|| {
            CommandError::new(t!("workbench-scratch-no-language", language = params.language.as_str()))
        })?;
        let starter = params.text.is_none().then(|| crate::run::scratch_starter(session, &language.id)).flatten();
        let text = params.text.as_deref().or(starter.as_ref().map(|starter| starter.text.as_str()));
        let path = create_scratch(session, &language, text.unwrap_or_default())?;
        if let Some(starter) = starter {
            history::place_caret(session, starter.caret, None)?;
        }
        Ok(path)
    }
}

fn selection_text(session: &Session) -> Option<(PathBuf, String)> {
    let document = session.state::<Workbench>()?.active_document()?;
    if document.image().is_some() {
        return None;
    }
    let text = document.text();
    let parts: Vec<String> = document
        .selections()
        .iter()
        .filter(|selection| !selection.is_empty())
        .map(|selection| {
            let range = selection.range();
            text.chars().skip(range.start).take(range.len()).collect()
        })
        .collect();
    (!parts.is_empty()).then(|| (document.path().to_owned(), parts.join("\n")))
}

pub fn has_selection(session: &Session) -> bool {
    selection_text(session).is_some()
}

pub(crate) fn scratch_from_selection(session: &mut Session) -> Result<PathBuf, CommandError> {
    let (path, text) =
        selection_text(session).ok_or_else(|| CommandError::new(t!("workbench-error-nothing-selected")))?;
    let languages = scratches(session)?.languages();
    create_scratch(session, &language_of(&languages, &path), &text)
}

pub struct ScratchFromSelection;

impl Command for ScratchFromSelection {
    type Params = NoParams;
    type Output = PathBuf;
    const ID: &'static str = "file.scratch.fromSelection";
    const TITLE: &'static str = "Create New Scratch File from Selection";
    const DESCRIPTION: &'static str = "Creates a scratch file holding the active editor's selected text (every selection, \
         one per line), in the language of that file, opens it and returns its path.";

    fn is_available(&self, session: &Session) -> bool {
        has_selection(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<PathBuf, CommandError> {
        scratch_from_selection(session)
    }
}

/// The parameters of `file.scratch.newBuffer`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct NewBufferParams {
    /// When the buffer to reuse has text: `true` clears it, `false` keeps it; when absent, nothing
    /// is opened and the answer asks for this.
    #[serde(default)]
    pub clear: Option<bool>,
}

/// What `file.scratch.newBuffer` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BufferOpened {
    /// The buffer file.
    pub path: PathBuf,
    /// Whether an existing buffer is reused: all five exist.
    pub reused: bool,
    /// Whether its text was cleared.
    pub cleared: bool,
    /// Whether the reused buffer has text and nothing was opened: call again with `clear`.
    pub needs_answer: bool,
}

pub struct NewBuffer;

impl Command for NewBuffer {
    type Params = NewBufferParams;
    type Output = BufferOpened;
    const ID: &'static str = "file.scratch.newBuffer";
    const TITLE: &'static str = "New Scratch Buffer";
    const DESCRIPTION: &'static str = "Opens the next scratch buffer, `buffer1.txt` to `buffer5.txt` in the IDE's buffers \
         folder: the first missing one is created; when all five exist, the one after the last opened is reused. A \
         reused buffer with text answers `needsAnswer` and opens nothing until called again with `clear` (true clears \
         it, false keeps it). A renamed buffer is never reused.";

    fn run(&self, session: &mut Session, params: NewBufferParams) -> Result<BufferOpened, CommandError> {
        let scratches = scratches(session)?;
        let folder = scratches.buffers_folder().ok_or_else(|| CommandError::new(t!("workbench-scratch-no-folder")))?;
        create_folder(&folder)?;
        let mut memo = scratches.memo();
        let missing = (1..=BUFFERS).find(|slot| !folder.join(buffer_name(*slot)).exists());
        let slot = missing.unwrap_or(memo.last_buffer % BUFFERS + 1);
        let path = folder.join(buffer_name(slot));
        let mut opened = BufferOpened { path: path.clone(), reused: missing.is_none(), ..BufferOpened::default() };
        if opened.reused {
            let open_text = session
                .state::<Workbench>()
                .and_then(|workbench| workbench.documents.iter().find(|document| document.path == path))
                .map(|document| document.text().to_owned());
            let text = open_text.or_else(|| std::fs::read_to_string(&path).ok()).unwrap_or_default();
            if !text.is_empty() {
                match params.clear {
                    None => return Ok(BufferOpened { needs_answer: true, ..opened }),
                    Some(true) => {
                        clear_buffer(session, &path)?;
                        opened.cleared = true;
                    }
                    Some(false) => {}
                }
            }
        } else {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    let error = error.to_string();
                    return Err(CommandError::new(t!("workbench-error-create", path = display(&path), error = error)));
                }
            }
        }
        memo.last_buffer = slot;
        scratches.save_memo(&memo);
        let origin = history::caret(session);
        open_document(session, &path)?;
        session.state_mut::<Workbench>().revision += 1;
        history::left_file(session, origin);
        Ok(opened)
    }
}

fn clear_buffer(session: &mut Session, path: &Path) -> Result<(), CommandError> {
    let workbench = session.state_mut::<Workbench>();
    if let Some(document) = workbench.documents.iter_mut().find(|document| document.path == path) {
        let length = document.text().chars().count();
        document.replace_ranges(vec![(0..length, String::new())]);
        let name = document.name();
        return document.save().map_err(|error| CommandError::new(crate::closing::save_failed(&name, &error)));
    }
    std::fs::write(path, "")
        .map_err(|error| CommandError::new(t!("workbench-error-save", file = display(path), error = error.to_string())))
}

/// A scratch file or buffer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScratchEntry {
    /// The file.
    pub path: PathBuf,
    /// Its name.
    pub name: String,
    /// The id of its language.
    pub language: String,
}

/// What `file.scratch.list` returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScratchList {
    /// The folder of scratch files.
    pub folder: Option<PathBuf>,
    /// The folder of scratch buffers.
    pub buffers_folder: Option<PathBuf>,
    /// The scratch files, by name.
    pub scratches: Vec<ScratchEntry>,
    /// The buffers, by name.
    pub buffers: Vec<ScratchEntry>,
}

fn entries(folder: Option<&Path>, languages: &[ScratchLanguage]) -> Vec<ScratchEntry> {
    let Some(read) = folder.and_then(|folder| std::fs::read_dir(folder).ok()) else { return Vec::new() };
    let mut entries: Vec<ScratchEntry> = read
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .map(|path| ScratchEntry {
            name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            language: language_of(languages, &path).id,
            path,
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

pub fn list(session: &Session) -> ScratchList {
    let Some(scratches) = session.state::<Scratches>() else { return ScratchList::default() };
    let languages = scratches.languages();
    let (folder, buffers_folder) = (scratches.scratches_folder(), scratches.buffers_folder());
    ScratchList {
        scratches: entries(folder.as_deref(), &languages),
        buffers: entries(buffers_folder.as_deref(), &languages),
        folder,
        buffers_folder,
    }
}

pub struct ListScratches;

impl Command for ListScratches {
    type Params = NoParams;
    type Output = ScratchList;
    const ID: &'static str = "file.scratch.list";
    const TITLE: &'static str = "Show Scratch Files";
    const DESCRIPTION: &'static str = "Lists the scratch files and buffers with their folders and languages. Rename and \
         delete them with file.fs.rename and file.fs.delete, open them with file.document.open.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ScratchList, CommandError> {
        scratches(session)?;
        Ok(list(session))
    }
}

fn scratch_path(session: &Session, scratches: &Scratches, path: &Path) -> Result<PathBuf, CommandError> {
    let path = session.state::<Workbench>().map_or_else(|| path.to_owned(), |workbench| workbench.spelled(path));
    if !scratches.contains(&path) || !path.is_file() {
        return Err(CommandError::new(t!("workbench-scratch-not-scratch", path = display(&path))));
    }
    Ok(path)
}

/// The parameters of `file.scratch.changeLanguage`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ChangeLanguageParams {
    /// The scratch file or buffer, absolute.
    pub path: PathBuf,
    /// The new language: an id, a name or an extension from `file.scratch.languages`.
    pub language: String,
}

pub struct ChangeLanguage;

impl Command for ChangeLanguage {
    type Params = ChangeLanguageParams;
    type Output = PathBuf;
    const ID: &'static str = "file.scratch.changeLanguage";
    const TITLE: &'static str = "Change Language";
    const DESCRIPTION: &'static str = "Changes the language of a scratch file or buffer by giving it that language's \
         extension (a free name when it is taken); its open tab follows. Returns the new path.";

    fn run(&self, session: &mut Session, params: ChangeLanguageParams) -> Result<PathBuf, CommandError> {
        let scratches = scratches(session)?;
        let path = scratch_path(session, &scratches, &params.path)?;
        let languages = scratches.languages();
        let language = find_language(&languages, &params.language).ok_or_else(|| {
            CommandError::new(t!("workbench-scratch-no-language", language = params.language.as_str()))
        })?;
        if path.extension().is_some_and(|extension| extension.to_string_lossy() == language.extension) {
            return Ok(path);
        }
        let folder = path.parent().map(Path::to_owned).unwrap_or_default();
        let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        let target = free_scratch_name(&folder, &stem, &language.extension);
        std::fs::rename(&path, &target).map_err(|error| {
            CommandError::new(t!("workbench-error-rename", path = display(&path), error = error.to_string()))
        })?;
        scratches.used_language(&language.id);
        session.state_mut::<Workbench>().relocated(&path, &target);
        Ok(target)
    }
}

/// The parameters of `file.scratch.move`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct MoveScratchParams {
    /// The scratch file or buffer, absolute.
    pub path: PathBuf,
    /// The folder of the project to move it into: absolute, or relative to the project (`.` is
    /// the project folder).
    pub directory: PathBuf,
}

pub struct MoveScratch;

impl Command for MoveScratch {
    type Params = MoveScratchParams;
    type Output = PathBuf;
    const ID: &'static str = "file.scratch.move";
    const TITLE: &'static str = "Move";
    const DESCRIPTION: &'static str = "Moves a scratch file or buffer into a folder of the project (a copy's name when its \
         name is taken there); its open tab follows. Returns the new path.";

    fn run(&self, session: &mut Session, params: MoveScratchParams) -> Result<PathBuf, CommandError> {
        let scratches = scratches(session)?;
        let path = scratch_path(session, &scratches, &params.path)?;
        let workbench = session.state_mut::<Workbench>();
        let directory = crate::project_folder(workbench, &params.directory)?;
        if let Some(root) = workbench.project().map(Path::to_owned) {
            let target = directory.join(path.file_name().unwrap_or_default());
            crate::hidden_files::guard_change(session, &root, &[target])?;
        }
        let workbench = session.state_mut::<Workbench>();
        let moved = zen_vfs::move_into(&path, &directory).map_err(|error| {
            CommandError::new(t!("workbench-error-move", path = display(&error.path), error = error.source.to_string()))
        })?;
        workbench.relocated(&path, &moved);
        workbench.messages.push(t!("workbench-moved", count = 1));
        Ok(moved)
    }
}

pub(crate) fn closed(session: &mut Session, path: &Path, text: &str) {
    let Some(scratches) = session.state::<Scratches>().filter(|scratches| scratches.contains(path)) else { return };
    let empty_on_disk = std::fs::read_to_string(path).is_ok_and(|saved| saved.trim().is_empty());
    if !text.trim().is_empty() || !empty_on_disk {
        return;
    }
    if scratches.elsewhere.as_ref().is_some_and(|elsewhere| elsewhere(path)) {
        return;
    }
    let workbench = session.state_mut::<Workbench>();
    if workbench.documents.iter().any(|document| document.path == path) {
        return;
    }
    if std::fs::remove_file(path).is_ok() {
        workbench.recent.retain(|recent| recent != path);
        workbench.revision += 1;
    }
}

pub(crate) mod homes;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Session};
use zen_i18n::t;
use zen_spell::Dictionary;

use super::Spelling;
use crate::Workbench;

pub const CUSTOM_DICTIONARIES: &str = "spelling.customDictionaries";
pub const SINGLE_DICTIONARY: &str = "spelling.useSingleDictionary";
pub const SAVING_DICTIONARY: &str = "spelling.savingDictionary";

pub const PROJECT_FOLDER: &str = ".idea/dictionaries";
pub const PROJECT_FILE: &str = "project.xml";

const PROJECT_NAME: &str = "project";

const RECHECK: Duration = Duration::from_secs(1);

const LARGEST_PROJECT_FILE: u64 = 4 << 20;

const LARGEST_DICTIONARY: u64 = 64 << 20;

/// A dictionary of the user's words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DictionaryLevel {
    /// The IDE's own, for every project: a word list in the IDE's data folder.
    Application,
    /// The project's, in `.idea/dictionaries/`, shared with JetBrains IDEs and the team.
    #[default]
    Project,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Application,
    Project,
    Plain,
    Hunspell,
}

type Stamp = Vec<Option<(u64, SystemTime)>>;

struct Source {
    kind: Kind,
    stamp: Stamp,
    dictionary: Arc<Dictionary>,
    words: Vec<String>,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct UserDictionaries {
    application: Option<PathBuf>,
    sources: Vec<(PathBuf, Source)>,
    generation: u64,
    checked: Option<Instant>,
}

impl UserDictionaries {
    pub(super) fn loaded(&self) -> Vec<Arc<Dictionary>> {
        self.sources
            .iter()
            .filter(|(_, source)| !source.dictionary.is_empty())
            .map(|(_, source)| Arc::clone(&source.dictionary))
            .collect()
    }

    fn words_of(&self, kind: Kind) -> Vec<String> {
        let mut words: Vec<String> = self
            .sources
            .iter()
            .filter(|(_, source)| source.kind == kind)
            .flat_map(|(_, source)| source.words.iter().map(|word| word.to_ascii_lowercase()))
            .collect();
        words.sort();
        words.dedup();
        words
    }

    fn files_of(&self, kind: Kind) -> Vec<PathBuf> {
        self.sources.iter().filter(|(_, source)| source.kind == kind).map(|(path, _)| path.clone()).collect()
    }
}

impl Spelling {
    pub fn set_application_dictionary(&mut self, file: PathBuf) {
        self.dictionaries.application = Some(file);
        self.dictionaries.checked = None;
    }
}

fn stamp(paths: &[PathBuf]) -> Stamp {
    paths
        .iter()
        .map(|path| std::fs::metadata(path).ok().and_then(|meta| Some((meta.len(), meta.modified().ok()?))))
        .collect()
}

fn hunspell_affixes(path: &Path) -> Option<PathBuf> {
    let affixes = path.with_extension("aff");
    (path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("dic")) && affixes.is_file())
        .then_some(affixes)
}

fn custom_paths(session: &Session) -> Vec<PathBuf> {
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    session
        .settings()
        .get::<Vec<String>>(CUSTOM_DICTIONARIES)
        .unwrap_or_default()
        .into_iter()
        .filter(|path| !path.trim().is_empty())
        .map(|path| {
            let path = PathBuf::from(path.trim());
            match &root {
                Some(root) if path.is_relative() => root.join(path),
                _ => path,
            }
        })
        .collect()
}

fn linked(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn project_folder(root: &Path) -> Option<PathBuf> {
    let idea = root.join(".idea");
    let folder = root.join(PROJECT_FOLDER);
    (!linked(&idea) && !linked(&folder)).then_some(folder)
}

fn project_files(session: &Session) -> Vec<PathBuf> {
    let Some(root) = session.state::<Workbench>().and_then(Workbench::project) else { return Vec::new() };
    let Some(Ok(entries)) = project_folder(root).map(std::fs::read_dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry.metadata().is_ok_and(|meta| meta.len() <= LARGEST_PROJECT_FILE)
        })
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "xml"))
        .collect();
    files.sort();
    files
}

fn wanted(session: &Session) -> Vec<(PathBuf, Kind)> {
    let mut wanted = Vec::new();
    if let Some(file) = session.state::<Spelling>().and_then(|spelling| spelling.dictionaries.application.clone()) {
        wanted.push((file, Kind::Application));
    }
    wanted.extend(project_files(session).into_iter().map(|file| (file, Kind::Project)));
    for path in custom_paths(session) {
        let kind = if hunspell_affixes(&path).is_some() { Kind::Hunspell } else { Kind::Plain };
        if !wanted.iter().any(|(kept, _)| *kept == path) {
            wanted.push((path, kind));
        }
    }
    wanted
}

fn inputs(path: &Path, kind: Kind) -> Vec<PathBuf> {
    match (kind, hunspell_affixes(path)) {
        (Kind::Hunspell, Some(affixes)) => vec![path.to_owned(), affixes],
        _ => vec![path.to_owned()],
    }
}

fn load(path: &Path, kind: Kind, stamp: Stamp) -> Source {
    let read = |path: &Path| {
        let size = std::fs::metadata(path).map_err(|error| error.to_string())?.len();
        if size > LARGEST_DICTIONARY {
            return Err(t!("workbench-spelling-too-large", megabytes = LARGEST_DICTIONARY >> 20));
        }
        std::fs::read(path).map_err(|error| error.to_string())
    };
    let words: Result<Vec<String>, String> = match kind {
        Kind::Application | Kind::Plain => {
            read(path).map(|bytes| zen_spell::word_list(&String::from_utf8_lossy(&bytes)))
        }
        Kind::Project => {
            read(path).and_then(|bytes| zen_spell::idea_dictionary::words(&String::from_utf8_lossy(&bytes)))
        }
        Kind::Hunspell => match hunspell_affixes(path) {
            Some(affixes) => {
                read(&affixes).and_then(|aff| read(path).and_then(|dic| zen_spell::hunspell::words(&aff, &dic)))
            }
            None => Err(t!("workbench-spelling-no-affixes")),
        },
    };
    let missing = kind == Kind::Application && !path.exists();
    match words {
        Ok(words) => Source { kind, stamp, dictionary: Arc::new(Dictionary::from_words(&words)), words, error: None },
        Err(error) => Source {
            kind,
            stamp,
            dictionary: Arc::new(Dictionary::from_words(Vec::<String>::new())),
            words: Vec::new(),
            error: (!missing).then_some(error),
        },
    }
}

pub(super) fn refresh(session: &mut Session, now: bool) -> (u64, Vec<Arc<Dictionary>>) {
    let fresh = session
        .state::<Spelling>()
        .and_then(|spelling| spelling.dictionaries.checked)
        .is_some_and(|checked| checked.elapsed() < RECHECK);
    if fresh && !now {
        let dictionaries = &session.state_mut::<Spelling>().dictionaries;
        return (dictionaries.generation, dictionaries.loaded());
    }
    let wanted = wanted(session);
    let stamps: Vec<Stamp> = wanted.iter().map(|(path, kind)| stamp(&inputs(path, *kind))).collect();
    let dictionaries = &mut session.state_mut::<Spelling>().dictionaries;
    let mut kept = std::mem::take(&mut dictionaries.sources);
    let mut changed = kept.len() != wanted.len();
    for ((path, kind), stamp) in wanted.into_iter().zip(stamps) {
        let at = kept.iter().position(|(known, source)| *known == path && source.kind == kind && source.stamp == stamp);
        match at {
            Some(at) => dictionaries.sources.push(kept.remove(at)),
            None => {
                changed = true;
                let source = load(&path, kind, stamp);
                dictionaries.sources.push((path, source));
            }
        }
    }
    if changed || !kept.is_empty() {
        dictionaries.generation += 1;
    }
    dictionaries.checked = Some(Instant::now());
    (dictionaries.generation, dictionaries.loaded())
}

pub fn saving_level(settings: &zen_core::Settings) -> DictionaryLevel {
    if single_dictionary(settings) {
        settings.get_or(SAVING_DICTIONARY, DictionaryLevel::Project)
    } else {
        DictionaryLevel::Application
    }
}

pub fn single_dictionary(settings: &zen_core::Settings) -> bool {
    settings.get_or(SINGLE_DICTIONARY, false)
}

fn normalized(word: &str) -> Result<String, CommandError> {
    let word = word.trim().replace('\u{2019}', "'").to_ascii_lowercase();
    if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'\'') {
        return Err(CommandError::new(t!("workbench-spelling-bad-word", word = word.as_str())));
    }
    Ok(word)
}

fn write(path: &Path, text: &str) -> Result<(), CommandError> {
    let failed = |error: std::io::Error| {
        CommandError::new(t!(
            "workbench-spelling-write-failed",
            file = path.display().to_string(),
            error = error.to_string()
        ))
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(failed)?;
    }
    let temporary = path.with_extension("zen-tmp");
    std::fs::write(&temporary, text).map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(failed)
}

fn read_text(path: &Path) -> Result<Option<String>, CommandError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CommandError::new(t!(
            "workbench-spelling-write-failed",
            file = path.display().to_string(),
            error = error.to_string()
        ))),
    }
}

fn application_file(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Spelling>()
        .and_then(|spelling| spelling.dictionaries.application.clone())
        .ok_or_else(|| CommandError::new(t!("workbench-spelling-no-application-dictionary")))
}

fn project_root(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-spelling-no-project")))
}

fn save_to_application(file: &Path, word: &str) -> Result<bool, CommandError> {
    let mut words = read_text(file)?.map(|text| zen_spell::word_list(&text)).unwrap_or_default();
    if words.iter().any(|kept| kept.eq_ignore_ascii_case(word)) {
        return Ok(false);
    }
    words.push(word.to_owned());
    words.sort_by_key(|word| word.to_ascii_lowercase());
    write(file, &(words.join("\n") + "\n"))?;
    Ok(true)
}

fn remove_from_application(file: &Path, word: &str) -> Result<bool, CommandError> {
    let Some(text) = read_text(file)? else { return Ok(false) };
    let words = zen_spell::word_list(&text);
    let kept: Vec<String> = words.iter().filter(|kept| !kept.eq_ignore_ascii_case(word)).cloned().collect();
    if kept.len() == words.len() {
        return Ok(false);
    }
    write(file, &kept.iter().map(|word| format!("{word}\n")).collect::<String>())?;
    Ok(true)
}

fn edit_project_file(file: &Path, add: &[&str], remove: &[&str]) -> Result<bool, CommandError> {
    let invalid = |error: String| {
        CommandError::new(t!("workbench-spelling-bad-file", file = file.display().to_string(), error = error))
    };
    match read_text(file)? {
        Some(text) => match zen_spell::idea_dictionary::edited(&text, add, remove).map_err(invalid)? {
            Some(edited) => write(file, &edited).map(|()| true),
            None => Ok(false),
        },
        None if add.is_empty() => Ok(false),
        None => {
            let words: Vec<String> = add.iter().map(|word| (*word).to_owned()).collect();
            write(file, &zen_spell::idea_dictionary::new_file(PROJECT_NAME, &words)).map(|()| true)
        }
    }
}

pub(super) fn save(session: &mut Session, word: &str, level: DictionaryLevel) -> Result<(PathBuf, bool), CommandError> {
    let word = normalized(word)?;
    let (file, added) = match level {
        DictionaryLevel::Application => {
            let file = application_file(session)?;
            let added = save_to_application(&file, &word)?;
            (file, added)
        }
        DictionaryLevel::Project => {
            let root = project_root(session)?;
            let target = project_folder(&root)
                .map(|folder| folder.join(PROJECT_FILE))
                .filter(|target| !linked(target))
                .ok_or_else(|| CommandError::new(t!("workbench-spelling-linked", folder = PROJECT_FOLDER)))?;
            let present = project_files(session).iter().any(|file| {
                read_text(file)
                    .ok()
                    .flatten()
                    .and_then(|text| zen_spell::idea_dictionary::words(&text).ok())
                    .is_some_and(|words| words.iter().any(|kept| kept.eq_ignore_ascii_case(&word)))
            });
            let added = !present && edit_project_file(&target, &[&word], &[])?;
            (target, added)
        }
    };
    refresh(session, true);
    Ok((file, added))
}

pub(super) fn remove(
    session: &mut Session,
    word: &str,
    level: Option<DictionaryLevel>,
) -> Result<Vec<PathBuf>, CommandError> {
    let word = normalized(word)?;
    let mut changed = Vec::new();
    if level != Some(DictionaryLevel::Project)
        && let Ok(file) = application_file(session)
        && remove_from_application(&file, &word)?
    {
        changed.push(file);
    }
    if level != Some(DictionaryLevel::Application) {
        for file in project_files(session) {
            if edit_project_file(&file, &[], &[&word])? {
                changed.push(file);
            }
        }
    }
    refresh(session, true);
    Ok(changed)
}

/// The parameters of `spelling.word.save`.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveParams {
    /// The word, saved in lower case.
    pub word: String,
    /// The dictionary; when absent, the one of Use single dictionary for saving words, or application-level while it is off.
    #[serde(default)]
    pub level: Option<DictionaryLevel>,
    /// The file the word was found in: ⌘Z there undoes the save until the file changes.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// What `spelling.word.save` did.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WordSaved {
    /// The word, as saved.
    pub word: String,
    /// The dictionary it was saved to.
    pub level: DictionaryLevel,
    /// That dictionary's file.
    pub file: PathBuf,
    /// Whether it was not there yet.
    pub added: bool,
}

pub struct SaveWord;

impl Command for SaveWord {
    type Params = SaveParams;
    type Output = WordSaved;
    const ID: &'static str = "spelling.word.save";
    const TITLE: &'static str = "Save Word to Dictionary";
    const DESCRIPTION: &'static str = "Saves a word to a dictionary so that it is no longer a typo: level application (the \
         IDE's own word list, for every project) or project (.idea/dictionaries/project.xml, shared with JetBrains IDEs); \
         by default the one Settings → Editor → Natural Languages → Spelling saves to. With path, ⌘Z in that file undoes \
         the save until the file changes. spelling.word.remove takes it out again.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<WordSaved, CommandError> {
        let level = params.level.unwrap_or_else(|| saving_level(session.settings()));
        let (file, added) = save(session, &params.word, level)?;
        let word = normalized(&params.word)?;
        if added && let Some(path) = params.path {
            let path = session.state::<Workbench>().map_or_else(|| path.clone(), |workbench| workbench.resolve(&path));
            let version = session
                .state::<Workbench>()
                .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
                .map(crate::Document::version);
            if let Some(version) = version {
                session.state_mut::<Spelling>().saved.push(super::SavedWord {
                    path,
                    version,
                    word: word.clone(),
                    level,
                });
            }
        }
        Ok(WordSaved { word, level, file, added })
    }
}

/// The parameters of `spelling.word.remove`.
#[derive(Deserialize, JsonSchema)]
pub struct RemoveParams {
    /// The word, whatever its case.
    pub word: String,
    /// The dictionary; both the application-level and the project-level ones when absent.
    #[serde(default)]
    pub level: Option<DictionaryLevel>,
}

pub struct RemoveWord;

impl Command for RemoveWord {
    type Params = RemoveParams;
    type Output = Vec<PathBuf>;
    const ID: &'static str = "spelling.word.remove";
    const TITLE: &'static str = "Remove Word from Dictionary";
    const DESCRIPTION: &'static str = "Takes a word out of the user's dictionaries (application, project, or both when level \
         is absent), so that it is a typo again; returns the files changed. Custom dictionaries are not changed.";

    fn run(&self, session: &mut Session, params: RemoveParams) -> Result<Vec<PathBuf>, CommandError> {
        remove(session, &params.word, params.level)
    }
}

/// A user's dictionary as `spelling.dictionaries.get` describes it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserDictionary {
    /// The file words are saved to; absent when there is none (no project, no data folder).
    pub file: Option<PathBuf>,
    /// The files read.
    pub files: Vec<PathBuf>,
    /// Its words, in lower case and in order.
    pub words: Vec<String>,
}

/// A custom dictionary as `spelling.dictionaries.get` describes it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomDictionary {
    /// Its file.
    pub path: PathBuf,
    /// `plain` (a word list) or `hunspell` (a `.dic` with its `.aff` beside it).
    pub format: String,
    /// How many words it spells, with the forms a Hunspell dictionary's affixes make.
    pub words: usize,
    /// Why it could not be read.
    pub error: Option<String>,
}

/// What `spelling.dictionaries.get` answers.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Dictionaries {
    /// Use single dictionary for saving words.
    pub use_single_dictionary: bool,
    /// The dictionary words are saved to without asking.
    pub saving_dictionary: DictionaryLevel,
    /// The application-level dictionary.
    pub application: UserDictionary,
    /// The project-level dictionary.
    pub project: UserDictionary,
    /// The custom dictionaries, in the order of the settings.
    pub custom: Vec<CustomDictionary>,
}

pub struct GetDictionaries;

impl Command for GetDictionaries {
    type Params = NoParams;
    type Output = Dictionaries;
    const ID: &'static str = "spelling.dictionaries.get";
    const TITLE: &'static str = "Spelling Dictionaries";
    const DESCRIPTION: &'static str = "The dictionaries the spelling check uses besides the bundled English one: the \
         application-level and project-level words (Accepted words), the custom dictionaries with how many words each \
         spells or why it cannot be read, and where words are saved. set_settings spelling changes the custom \
         dictionaries and the saving dictionary; spelling.word.save and spelling.word.remove change the words.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Dictionaries, CommandError> {
        refresh(session, true);
        let project = session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .map(|root| root.join(PROJECT_FOLDER).join(PROJECT_FILE));
        let settings = session.settings();
        let (single, level) =
            (single_dictionary(settings), settings.get_or(SAVING_DICTIONARY, DictionaryLevel::Project));
        let spelling = session.state_mut::<Spelling>();
        let dictionaries = &spelling.dictionaries;
        let custom = dictionaries
            .sources
            .iter()
            .filter(|(_, source)| matches!(source.kind, Kind::Plain | Kind::Hunspell))
            .map(|(path, source)| CustomDictionary {
                path: path.clone(),
                format: if source.kind == Kind::Hunspell { "hunspell" } else { "plain" }.to_owned(),
                words: source.dictionary.len(),
                error: source.error.clone(),
            })
            .collect();
        Ok(Dictionaries {
            use_single_dictionary: single,
            saving_dictionary: level,
            application: UserDictionary {
                file: dictionaries.application.clone(),
                files: dictionaries.files_of(Kind::Application),
                words: dictionaries.words_of(Kind::Application),
            },
            project: UserDictionary {
                file: project,
                files: dictionaries.files_of(Kind::Project),
                words: dictionaries.words_of(Kind::Project),
            },
            custom,
        })
    }
}

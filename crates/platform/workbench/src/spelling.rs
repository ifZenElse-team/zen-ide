use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::inspect::Wake;
use zen_spell::{Context, Region};
use zen_syntax::{HighlightKind, Highlighter, Span};

use crate::Workbench;
use crate::language::{Problem, Severity};

mod dictionaries;
mod fixes;

pub use dictionaries::{
    CUSTOM_DICTIONARIES, CustomDictionary, Dictionaries, DictionaryLevel, GetDictionaries, PROJECT_FILE,
    PROJECT_FOLDER, RemoveParams, RemoveWord, SAVING_DICTIONARY, SINGLE_DICTIONARY, SaveParams, SaveWord,
    UserDictionary, WordSaved, saving_level, single_dictionary,
};
pub use fixes::{
    ChangeParams, ChangeTypo, GetTypo, ListSuggestions, SAVE_TO, SuggestionsParams, TypoAt, TypoChanged, TypoName,
    TypoParams,
};
pub(crate) use fixes::{undo, with_suggestions};

pub const ENABLED: &str = "inspections.spelling.enabled";
pub const PROCESS_CODE: &str = "inspections.spelling.processCode";
pub const PROCESS_LITERALS: &str = "inspections.spelling.processLiterals";
pub const PROCESS_COMMENTS: &str = "inspections.spelling.processComments";

pub const SOURCE: &str = "spelling";

const LARGEST: usize = 4 << 20;

const WAIT: Duration = Duration::from_secs(5);

const PROSE: &[&str] = &["md", "markdown", "txt", "text", "rst", "adoc", "asciidoc"];

const DECLARING: &[&str] = &[
    "func",
    "type",
    "var",
    "package",
    "fn",
    "let",
    "struct",
    "enum",
    "trait",
    "mod",
    "static",
    "union",
    "class",
    "interface",
    "function",
    "def",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Options {
    pub code: bool,
    pub literals: bool,
    pub comments: bool,
}

impl Options {
    pub fn of(settings: &zen_core::Settings) -> Option<Options> {
        settings.get_or(ENABLED, true).then(|| Options {
            code: settings.get_or(PROCESS_CODE, true),
            literals: settings.get_or(PROCESS_LITERALS, true),
            comments: settings.get_or(PROCESS_COMMENTS, true),
        })
    }
}

/// Where a typo is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TypoContext {
    /// A name declared in code.
    Code,
    /// A string literal.
    Literal,
    /// A comment.
    Comment,
    /// Plain text or Markdown.
    Text,
}

impl From<Context> for TypoContext {
    fn from(context: Context) -> Self {
        match context {
            Context::Code => TypoContext::Code,
            Context::Literal => TypoContext::Literal,
            Context::Comment => TypoContext::Comment,
            Context::Text => TypoContext::Text,
        }
    }
}

/// A misspelt word of a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoundTypo {
    /// The first character it covers (a character offset of the file's text).
    pub start: usize,
    /// The character after the last.
    pub end: usize,
    /// The line it is on, one-based.
    pub line: u32,
    /// The column it starts at, one-based, in characters.
    pub column: u32,
    /// The word as it is written.
    pub word: String,
    /// Where it is.
    pub context: TypoContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    version: u64,
    declarations: Option<u64>,
    options: Options,
    highlighter: usize,
    words: u64,
}

struct Job {
    path: PathBuf,
    key: Key,
    text: String,
    highlighter: Option<Arc<dyn Highlighter>>,
    spans: Option<Vec<Span>>,
    prose: bool,
    declarations: Option<Vec<(usize, usize)>>,
    user: Vec<Arc<zen_spell::Dictionary>>,
    wake: Option<Wake>,
}

struct Done {
    path: PathBuf,
    key: Key,
    typos: Vec<FoundTypo>,
}

struct Worker {
    jobs: Sender<Job>,
    done: Receiver<Done>,
}

struct Shown {
    version: u64,
    checked: Key,
    language: String,
    single: bool,
    problems: Vec<Problem>,
}

struct SavedWord {
    path: PathBuf,
    version: u64,
    word: String,
    level: DictionaryLevel,
}

#[derive(Default)]
struct FileTypos {
    asked: Option<Key>,
    checked: Option<(Key, Vec<FoundTypo>)>,
    shown: Option<Shown>,
}

#[derive(Default)]
pub struct Spelling {
    active: bool,
    worker: Option<Worker>,
    files: HashMap<PathBuf, FileTypos>,
    dictionaries: dictionaries::UserDictionaries,
    saved: Vec<SavedWord>,
}

impl Spelling {
    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    fn send(&mut self, job: Job) {
        let worker = self.worker.get_or_insert_with(|| {
            let (jobs, waiting) = std::sync::mpsc::channel::<Job>();
            let (finished, done) = std::sync::mpsc::channel::<Done>();
            std::thread::Builder::new().name("zen-spelling".to_owned()).spawn(move || work(&waiting, &finished)).ok();
            Worker { jobs, done }
        });
        if let Err(failed) = worker.jobs.send(job) {
            self.worker = None;
            if let Some(file) = self.files.get_mut(&failed.0.path) {
                file.asked = None;
            }
        }
    }

    fn receive(&mut self, wait: Option<Duration>) {
        let Some(worker) = &self.worker else { return };
        let mut answers = Vec::new();
        if let Some(wait) = wait {
            match worker.done.recv_timeout(wait) {
                Ok(done) => answers.push(done),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.worker = None;
                    return;
                }
            }
        }
        answers.extend(worker.done.try_iter());
        for done in answers {
            let file = self.files.entry(done.path).or_default();
            let newer = file.checked.as_ref().is_none_or(|(key, _)| key.version <= done.key.version);
            if newer {
                file.checked = Some((done.key, done.typos));
            }
        }
    }
}

fn work(jobs: &Receiver<Job>, done: &Sender<Done>) {
    while let Ok(first) = jobs.recv() {
        let mut latest: Vec<Job> = vec![first];
        for job in jobs.try_iter() {
            latest.retain(|kept| kept.path != job.path);
            latest.push(job);
        }
        let dictionary = zen_spell::english();
        let mut wakes = Vec::new();
        for job in latest {
            let typos = check(dictionary, &job);
            wakes.extend(job.wake.clone());
            if done.send(Done { path: job.path, key: job.key, typos }).is_err() {
                return;
            }
        }
        if let Some(wake) = wakes.last() {
            wake();
        }
    }
}

fn check(dictionary: &zen_spell::Dictionary, job: &Job) -> Vec<FoundTypo> {
    let text = job.text.as_str();
    let highlighted: Vec<Span>;
    let spans = match &job.spans {
        Some(spans) => spans.as_slice(),
        None => {
            highlighted = job.highlighter.as_ref().map(|highlighter| highlighter.highlight(text)).unwrap_or_default();
            highlighted.as_slice()
        }
    };
    let declarations = job.declarations.as_ref().map(|ranges| byte_ranges(text, ranges));
    let regions = regions(text, spans, declarations.as_deref(), job.prose, job.key.options);
    let mut all: Vec<&zen_spell::Dictionary> = vec![dictionary];
    all.extend(job.user.iter().map(AsRef::as_ref));
    let mut typos = zen_spell::typos_with(&all, text, &regions);
    typos.sort_by_key(|typo| typo.range.start);
    placed(text, typos)
}

fn byte_ranges(text: &str, ranges: &[(usize, usize)]) -> Vec<Range<usize>> {
    let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
    sorted.sort_unstable();
    let mut bytes = Vec::with_capacity(sorted.len());
    let mut chars = text.char_indices().map(|(byte, _)| byte).chain(std::iter::once(text.len())).enumerate();
    let mut at = |wanted: usize| chars.find(|(index, _)| *index >= wanted).map_or(text.len(), |(_, byte)| byte);
    for (start, end) in sorted {
        let from = at(start);
        let to = if end == start { from } else { at(end) };
        bytes.push(from..to);
    }
    bytes
}

fn placed(text: &str, typos: Vec<zen_spell::Typo>) -> Vec<FoundTypo> {
    let (mut byte, mut char_index, mut line, mut line_start) = (0usize, 0usize, 1u32, 0usize);
    let mut advance = |to: usize| {
        for c in text[byte..to].chars() {
            char_index += 1;
            if c == '\n' {
                line += 1;
                line_start = char_index;
            }
        }
        byte = to;
        (char_index, line, line_start)
    };
    typos
        .into_iter()
        .map(|typo| {
            let (start, line, line_start) = advance(typo.range.start);
            let length = typo.word.chars().count();
            FoundTypo {
                start,
                end: start + length,
                line,
                column: (start - line_start) as u32 + 1,
                word: typo.word,
                context: typo.context.into(),
            }
        })
        .collect()
}

fn regions(
    text: &str,
    spans: &[Span],
    declarations: Option<&[Range<usize>]>,
    prose: bool,
    options: Options,
) -> Vec<Region> {
    let mut regions = Vec::new();
    let gap = |range: Range<usize>, regions: &mut Vec<Region>| {
        if range.is_empty() {
            return;
        }
        if prose {
            regions.push(Region { range, context: Context::Text });
        } else if options.code {
            names(text, range, declarations, regions);
        }
    };
    let mut at = 0;
    for span in spans {
        let range = span.range.start.min(text.len())..span.range.end.min(text.len());
        if range.start < at {
            continue;
        }
        gap(at..range.start, &mut regions);
        match span.kind {
            HighlightKind::Comment | HighlightKind::DocComment if options.comments => {
                regions.push(Region { range: range.clone(), context: Context::Comment });
            }
            HighlightKind::String if options.literals && !prose => {
                regions.push(Region { range: range.clone(), context: Context::Literal });
            }
            HighlightKind::Type | HighlightKind::Function | HighlightKind::Constant if !prose && options.code => {
                names(text, range.clone(), declarations, &mut regions);
            }
            HighlightKind::Heading | HighlightKind::Strong | HighlightKind::Emphasis | HighlightKind::Link if prose => {
                regions.push(Region { range: range.clone(), context: Context::Text });
            }
            _ => {}
        }
        at = range.end;
    }
    gap(at..text.len(), &mut regions);
    regions
}

fn names(text: &str, range: Range<usize>, declarations: Option<&[Range<usize>]>, regions: &mut Vec<Region>) {
    let bytes = text.as_bytes();
    let mut at = range.start;
    while at < range.end {
        let byte = bytes[at];
        if !(byte.is_ascii_alphabetic() || byte == b'_') || (at > 0 && is_name_byte(bytes[at - 1])) {
            at += 1;
            continue;
        }
        let start = at;
        while at < range.end && is_name_byte(bytes[at]) {
            at += 1;
        }
        let declared = match declarations {
            Some(declared) => {
                let index = declared.partition_point(|range| range.end <= start);
                declared.get(index).is_some_and(|range| range.start <= start)
            }
            None => declared_by_keyword(text, start..at),
        };
        if declared {
            regions.push(Region { range: start..at, context: Context::Code });
        }
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn declared_by_keyword(text: &str, range: Range<usize>) -> bool {
    let before = text[..range.start].trim_end_matches([' ', '\t']);
    let keyword_start = before.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |at| at + 1);
    let keyword = &before[keyword_start..];
    if DECLARING.contains(&keyword) {
        return true;
    }
    if matches!(keyword, "mut" | "const") {
        let ahead = before[..keyword_start].trim_end_matches([' ', '\t']);
        return !ahead.ends_with(['&', '*']);
    }
    text[range.end..].trim_start_matches([' ', '\t']).starts_with(":=")
}

fn is_prose(name: &str, highlighted: bool) -> bool {
    let extension = name.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase());
    !highlighted || extension.is_some_and(|extension| PROSE.contains(&extension.as_str()))
}

fn kept_spans(session: &Session, document: &crate::Document, highlighter: &Arc<dyn Highlighter>) -> Option<Vec<Span>> {
    let highlighting = session.state::<crate::brackets::Highlighters>()?.highlighting(document, highlighter);
    let highlighting = highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    (!highlighting.is_incremental()).then(|| highlighting.spans(document.text(), 0..document.text().len()))
}

fn highlighter_key(highlighter: Option<&Arc<dyn Highlighter>>) -> usize {
    highlighter.map_or(0, |highlighter| Arc::as_ptr(highlighter).cast::<()>() as usize)
}

fn schedule(session: &mut Session, path: &Path) -> Option<Key> {
    if !session.state::<Spelling>().is_some_and(Spelling::is_active) {
        return None;
    }
    let options = Options::of(session.settings())?;
    let document = session.state::<Workbench>()?.documents().iter().find(|document| document.path() == path)?;
    if document.image().is_some() || document.text().len() > LARGEST {
        return None;
    }
    let highlighter =
        session.state::<crate::brackets::Highlighters>().and_then(|found| found.for_name(&document.name()));
    let version = document.version();
    let (words, user) = dictionaries::refresh(session, false);
    let document = session.state::<Workbench>()?.documents().iter().find(|document| document.path() == path)?;
    let key = Key {
        version,
        declarations: crate::language::semantic::declarations_generation(session, path),
        options,
        highlighter: highlighter_key(highlighter.as_ref()),
        words,
    };
    if session.state::<Spelling>().and_then(|spelling| spelling.files.get(path)).and_then(|file| file.asked)
        == Some(key)
    {
        return Some(key);
    }
    let prose = is_prose(&document.name(), highlighter.is_some());
    let spans = highlighter.as_ref().and_then(|highlighter| kept_spans(session, document, highlighter));
    let job = Job {
        path: path.to_owned(),
        key,
        text: document.text().to_owned(),
        prose,
        highlighter: if spans.is_some() { None } else { highlighter },
        spans,
        declarations: key.declarations.and_then(|_| crate::language::semantic::declarations(session, path)),
        user,
        wake: session.state::<crate::inspections::Inspections>().and_then(crate::inspections::Inspections::wake),
    };
    let spelling = session.state_mut::<Spelling>();
    spelling.files.entry(path.to_owned()).or_default().asked = Some(key);
    spelling.send(job);
    Some(key)
}

pub(crate) fn problems(session: &mut Session, path: &Path) -> Vec<Problem> {
    let Some(key) = schedule(session, path) else {
        if session.state::<Spelling>().is_some_and(|spelling| spelling.files.contains_key(path)) {
            session.state_mut::<Spelling>().files.remove(path);
        }
        return Vec::new();
    };
    session.state_mut::<Spelling>().receive(None);
    let language = zen_i18n::language();
    let single = single_dictionary(session.settings());
    let Some(file) = session.state::<Spelling>().and_then(|spelling| spelling.files.get(path)) else {
        return Vec::new();
    };
    let Some((checked, typos)) = &file.checked else { return Vec::new() };
    if let Some(shown) = &file.shown
        && shown.version == key.version
        && shown.checked == *checked
        && shown.language == language
        && shown.single == single
    {
        return shown.problems.clone();
    }
    let checked = *checked;
    let Some(document) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
    else {
        return Vec::new();
    };
    let moved = if checked.version == document.version() {
        typos.clone()
    } else {
        match document.char_edits_since(checked.version) {
            Some(edits) => moved_typos(typos, &edits, document.snapshot()),
            None => Vec::new(),
        }
    };
    let mut messages: HashMap<&str, String> = HashMap::new();
    let problems: Vec<Problem> = moved
        .iter()
        .map(|typo| {
            let message = messages.entry(typo.word.as_str()).or_insert_with(|| message(&typo.word)).clone();
            Problem { message, ..problem(path, typo, single) }
        })
        .collect();
    let version = document.version();
    if let Some(file) = session.state_mut::<Spelling>().files.get_mut(path) {
        file.shown = Some(Shown { version, checked, language, single, problems: problems.clone() });
    }
    problems
}

pub(crate) fn known(session: &Session, path: &Path) -> Vec<Problem> {
    let Some(file) = session.state::<Spelling>().and_then(|spelling| spelling.files.get(path)) else {
        return Vec::new();
    };
    let version = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(crate::Document::version);
    match (&file.checked, &file.shown) {
        (Some((checked, typos)), shown)
            if Some(checked.version) == version && shown.as_ref().is_none_or(|shown| shown.checked != *checked) =>
        {
            let single = single_dictionary(session.settings());
            typos.iter().map(|typo| Problem { message: message(&typo.word), ..problem(path, typo, single) }).collect()
        }
        (_, Some(shown)) => shown.problems.clone(),
        _ => Vec::new(),
    }
}

pub(crate) fn checks(session: &Session, path: &Path) -> bool {
    session.state::<Spelling>().is_some_and(Spelling::is_active)
        && Options::of(session.settings()).is_some()
        && session.state::<Workbench>().is_some_and(|workbench| {
            workbench.documents().iter().any(|document| document.path() == path && document.image().is_none())
        })
}

fn moved_typos(typos: &[FoundTypo], edits: &[crate::CharEdit], text: &zen_text::Text) -> Vec<FoundTypo> {
    typos
        .iter()
        .filter_map(|typo| {
            let (mut start, mut end) = (typo.start, typo.end);
            for edit in edits {
                if edit.range.start < end && start < edit.range.end.max(edit.range.start + 1) {
                    return None;
                }
                (start, end) = (edit.map(start), edit.map(end));
            }
            let point = text.offset_to_point(start).ok()?;
            Some(FoundTypo { start, end, line: point.line as u32 + 1, column: point.column as u32 + 1, ..typo.clone() })
        })
        .collect()
}

fn message(word: &str) -> String {
    t!("workbench-spelling-typo", word = word)
}

fn problem(path: &Path, typo: &FoundTypo, single: bool) -> Problem {
    Problem {
        start: typo.start,
        end: typo.end,
        line: typo.line,
        column: typo.column,
        severity: Severity::Typo,
        message: String::new(),
        source: Some(SOURCE.to_owned()),
        code: Some("typo".to_owned()),
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: fixes::of(path, typo, single),
        deprecated: false,
        unused: false,
    }
}

pub(crate) fn is_typo(problem: &Problem) -> bool {
    problem.source.as_deref() == Some(SOURCE)
}

fn current_typos(session: &mut Session, path: &Path, wait: Duration) -> Option<Vec<FoundTypo>> {
    let deadline = Instant::now() + wait;
    loop {
        let key = schedule(session, path)?;
        let spelling = session.state_mut::<Spelling>();
        spelling.receive(None);
        if let Some((checked, typos)) = spelling.files.get(path).and_then(|file| file.checked.as_ref())
            && *checked == key
        {
            return Some(typos.clone());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        spelling.receive(Some(left.min(Duration::from_millis(50))));
    }
}

/// The parameters of `spelling.typos.list`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TyposParams {
    /// The file, open in the editor; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// How long to wait for the check of the file as it is now, in milliseconds (5000 by default).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `spelling.typos.list` answers.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TyposAnswer {
    /// Whether the file was checked as it is now: false when the inspection is off, the file is
    /// not open or too large, or the check did not end in time.
    pub checked: bool,
    /// Its typos, in order.
    pub typos: Vec<FoundTypo>,
}

pub struct ListTypos;

impl Command for ListTypos {
    type Params = TyposParams;
    type Output = TyposAnswer;
    const ID: &'static str = "spelling.typos.list";
    const TITLE: &'static str = "Spelling Typos";
    const DESCRIPTION: &'static str = "The misspelt words of an open file (the active one by default), once the background \
         spelling check of its text as it is now is done (waits up to waitMs, 5000 by default): each with its character \
         range, one-based line and column, the word, and where it is (code: a declared name, literal, comment, text). \
         The same words are problems of severity typo in problems.file.list. Settings → Editor → Inspections → \
         Proofreading → Spelling (set_settings spelling) turns it off and picks code, literals and comments.";

    fn run(&self, session: &mut Session, params: TyposParams) -> Result<TyposAnswer, CommandError> {
        let path = params
            .path
            .map(|path| session.state::<Workbench>().map_or_else(|| path.clone(), |workbench| workbench.resolve(&path)))
            .or_else(|| {
                session
                    .state::<Workbench>()
                    .and_then(Workbench::active_document)
                    .map(|document| document.path().to_owned())
            })
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        Ok(match current_typos(session, &path, params.wait_ms.map_or(WAIT, Duration::from_millis)) {
            Some(typos) => TyposAnswer { checked: true, typos },
            None => TyposAnswer { checked: false, typos: Vec::new() },
        })
    }
}

/// The parameters of `spelling.words.check`.
#[derive(Deserialize, JsonSchema)]
pub struct WordsParams {
    /// The words, each judged on its own, whatever its case.
    pub words: Vec<String>,
}

pub struct CheckWords;

impl Command for CheckWords {
    type Params = WordsParams;
    type Output = Vec<String>;
    const ID: &'static str = "spelling.words.check";
    const TITLE: &'static str = "Check Spelling of Words";
    const DESCRIPTION: &'static str = "The words of `words` that neither the bundled English dictionary nor the user's \
         dictionaries (spelling.dictionaries.get) know, in order: an empty list means every word is spelt right. \
         Inflections (files, parsed), prefixes (unmarshal), two words written as one (filename) and British spellings \
         (colour) are known.";

    fn run(&self, session: &mut Session, params: WordsParams) -> Result<Vec<String>, CommandError> {
        let (_, user) = dictionaries::refresh(session, false);
        let dictionary = zen_spell::english();
        Ok(params
            .words
            .into_iter()
            .filter(|word| !dictionary.knows(word) && !user.iter().any(|mine| mine.knows(word)))
            .collect())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTypos)?;
    registry.register(CheckWords)?;
    registry.register(ListSuggestions)?;
    registry.register(GetTypo)?;
    registry.register(ChangeTypo)?;
    registry.register(SaveWord)?;
    registry.register(RemoveWord)?;
    registry.register(GetDictionaries)
}

#[cfg(test)]
mod tests;

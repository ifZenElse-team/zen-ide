use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_index::{Search, TextMatch};
use zen_text::search::{Query, SearchContext};

use crate::Workbench;
use crate::brackets::{HighlighterSource, Highlighters};
use crate::search_scopes::ResolvedScope;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(StartSearch)?;
    registry.register(SearchResults)?;
    registry.register(CancelSearch)?;
    registry.register(FindInFiles)?;
    registry.register(ReplaceInFiles)?;
    registry.register(StartReplace)?;
    registry.register(FinishReplace)?;
    registry.register(PreviewMatch)?;
    registry.register(EditPreview)?;
    registry.register(IndexStatus)?;
    Ok(())
}

pub const POPUP_LIMIT: &str = "search.findInFiles.popupLimit";
pub const DEFAULT_POPUP_LIMIT: usize = 100;
pub const TOO_MANY: &str = "search.findInFiles.tooManyResults";
pub const DEFAULT_TOO_MANY: usize = 1000;
pub const MAX_POPUP_LIMIT: usize = 10_000;
pub const MAX_TOO_MANY: usize = 1_000_000;

#[derive(Clone, Debug, Default)]
pub struct TextSearch {
    matches: Arc<Mutex<Vec<TextMatch>>>,
    done: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

/// What and where to search.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchParams {
    /// The text or regular expression, with match case, whole words and the filter (`context`).
    pub query: Query,
    /// File masks, comma-separated, e.g. `*.go, !*_test.go`.
    pub mask: String,
    /// A folder of the project, relative to it (`store/model`) or absolute; the whole project when
    /// empty. A folder outside the project or missing from it is an error.
    pub directory: String,
    /// Whether the folder's subfolders are searched too; true when absent.
    pub recursive: Option<bool>,
    /// A scope id from search.scope.list (`projectFiles`, `module:<name>`, `named:<name>`...); Project Files when absent.
    pub scope: Option<String>,
    /// At most this many matches; 2000 when absent.
    pub limit: Option<usize>,
    /// Find: `files`, each path once.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub group: bool,
    /// Find: `rows` of text.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compact: bool,
}

impl SearchParams {
    pub(crate) fn search(&self, session: &mut Session, default_limit: usize) -> Result<Search, CommandError> {
        let (mut search, scope) = self.scoped(session, default_limit)?;
        search.extra = scope.extra_files();
        Ok(search)
    }

    fn scoped(&self, session: &mut Session, default_limit: usize) -> Result<(Search, ResolvedScope), CommandError> {
        let scope = crate::search_scopes::resolve_in(
            session,
            self.scope.as_deref(),
            &self.directory,
            self.recursive.unwrap_or(true),
        )?;
        let search = Search {
            query: self.query.clone(),
            mask: self.mask.clone(),
            directory: String::new(),
            limit: self.limit.unwrap_or(default_limit),
            filter: Some(scope.filter.clone()),
            extra: Vec::new(),
        };
        Ok((search, scope))
    }
}

/// One match, as commands report it (ADR-0090): one path, one-based line and column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MatchInfo {
    /// The file: relative to the project (with `/`) when it is in it, otherwise absolute (a
    /// library's file a scope adds). Commands that take a path take it as it is.
    pub path: String,
    /// The line, one-based.
    pub line: usize,
    /// The column, one-based, in characters.
    pub column: usize,
    /// The length of the match in characters.
    pub length: usize,
    /// The match's first character as a character offset in the file.
    pub offset: usize,
    /// The line's text.
    pub text: String,
}

impl MatchInfo {
    fn new(found: &TextMatch, root: Option<&Path>) -> Self {
        let inside = root.is_some_and(|root| crate::inside_project(root, &found.path));
        MatchInfo {
            path: if inside { found.relative.clone() } else { found.path.display().to_string() },
            line: found.line + 1,
            column: found.column + 1,
            length: found.range.len(),
            offset: found.range.start,
            text: found.line_text.clone(),
        }
    }
}

/// The matches of a search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SearchResult {
    /// The matches, sorted by file and line; absent with `group` or `compact`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matches: Option<Vec<MatchInfo>>,
    /// With `group`: each file once, its matches under it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileMatches>>,
    /// With `compact`: one row of text each.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<String>>,
    /// Whether the search is finished.
    pub done: bool,
    /// Why the query cannot be used, if it cannot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When there are no matches, why: `searching` while the search runs, `noMatch` once it has
    /// read every file of the scope and folder without finding the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<crate::language::Reason>,
    /// For an agent: how many of the project's files are hidden from agents and so never searched
    /// (Settings → Tools → MCP Server), whatever the query; absent when none are.
    #[serde(rename = "hiddenFiles", skip_serializing_if = "Option::is_none")]
    pub hidden_files: Option<usize>,
}

impl SearchResult {
    fn new(matches: &[TextMatch], done: bool, error: Option<String>, root: Option<&Path>) -> Self {
        let reason = match (matches.is_empty() && error.is_none(), done) {
            (false, _) => None,
            (true, false) => Some(crate::language::Reason::Searching),
            (true, true) => Some(crate::language::Reason::NoMatch),
        };
        let matches = matches.iter().map(|found| MatchInfo::new(found, root)).collect();
        SearchResult { matches: Some(matches), files: None, rows: None, done, error, reason, hidden_files: None }
    }

    fn shaped(mut self, group: bool, compact: bool) -> Self {
        let matches = self.matches.take().unwrap_or_default();
        match (group, compact) {
            (false, false) => self.matches = Some(matches),
            (false, true) => {
                let rows = matches.iter().map(|found| {
                    crate::language::compact::row(
                        &found.path,
                        found.line as u32,
                        found.column as u32,
                        found.text.trim(),
                    )
                });
                self.rows = Some(rows.collect());
            }
            (true, compact) => {
                let mut files: Vec<FileMatches> = Vec::new();
                for found in matches {
                    let line = LineMatch {
                        line: found.line,
                        column: found.column,
                        length: found.length,
                        offset: found.offset,
                        text: found.text,
                    };
                    match files.last_mut() {
                        Some(file) if file.path == found.path => file.matches.push(line),
                        _ => files.push(FileMatches { path: found.path, matches: vec![line] }),
                    }
                }
                if compact {
                    let mut rows = Vec::new();
                    for file in files {
                        rows.push(file.path);
                        rows.extend(file.matches.iter().map(|found| {
                            format!("{}:{} {}", found.line, found.column, found.text.trim()).trim_end().to_owned()
                        }));
                    }
                    self.rows = Some(rows);
                } else {
                    self.files = Some(files);
                }
            }
        }
        self
    }
}

/// One file's matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FileMatches {
    /// The file, as `MatchInfo::path` gives it.
    pub path: String,
    /// Its matches, by line.
    pub matches: Vec<LineMatch>,
}

/// A match in a file whose path is given once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LineMatch {
    /// The line, one-based.
    pub line: usize,
    /// The column, one-based, in characters.
    pub column: usize,
    /// The length of the match in characters.
    pub length: usize,
    /// The match's first character as a character offset in the file.
    pub offset: usize,
    /// The line's text.
    pub text: String,
}

pub(crate) fn open_texts(workbench: &Workbench) -> HashMap<PathBuf, String> {
    workbench
        .documents()
        .iter()
        .filter(|document| document.image().is_none())
        .map(|document| (document.path().to_owned(), document.text().to_owned()))
        .collect()
}

pub(crate) fn keeping(
    source: Option<HighlighterSource>,
    context: SearchContext,
) -> impl Fn(&Path, &str, Vec<Range<usize>>) -> Vec<Range<usize>> + Send + Sync {
    move |path, text, ranges| {
        if context == SearchContext::Anywhere {
            return ranges;
        }
        let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
        let spans = source.as_ref().and_then(|source| source(&name)).map(|found| found.highlight(text));
        let spans = spans.unwrap_or_default();
        zen_syntax::keep_in_context(text, ranges, context, |byte| zen_syntax::kind_at(&spans, byte))
    }
}

pub(crate) fn highlighters(session: &Session) -> Option<HighlighterSource> {
    session.state::<Highlighters>().and_then(Highlighters::source)
}

pub(crate) fn index(workbench: &Workbench) -> Result<zen_index::FileIndex, CommandError> {
    workbench.index.clone().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

pub(crate) fn sorted(mut matches: Vec<TextMatch>) -> Vec<TextMatch> {
    matches.sort_by(|a, b| (&a.relative, a.line, a.column).cmp(&(&b.relative, b.line, b.column)));
    matches
}

pub struct StartSearch;

impl Command for StartSearch {
    type Params = SearchParams;
    type Output = ();
    const ID: &'static str = "search.text.start";
    const TITLE: &'static str = "Find in Files";
    const DESCRIPTION: &'static str = "Starts searching the project's files in the background (⇧⌘F), in a scope from \
         search.scope.list (`scope`) and a folder (`directory`, `recursive`); read the matches with \
         search.text.results. A search already running is cancelled.";

    fn run(&self, session: &mut Session, params: SearchParams) -> Result<(), CommandError> {
        session.state_mut::<Workbench>().text_search.cancel.store(true, Ordering::Relaxed);
        let search = spawn(session, &params, 2000)?;
        session.state_mut::<Workbench>().text_search = search;
        Ok(())
    }
}

impl TextSearch {
    pub(crate) fn matches(&self) -> Vec<TextMatch> {
        sorted(self.matches.lock().map(|matches| matches.clone()).unwrap_or_default())
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub(crate) fn spawn(
    session: &mut Session,
    params: &SearchParams,
    default_limit: usize,
) -> Result<TextSearch, CommandError> {
    let keep = keeping(highlighters(session), params.query.context);
    let (scoped, scope) = params.scoped(session, default_limit)?;
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let search = TextSearch::default();
    let index = index(workbench)?;
    let open = open_texts(workbench);
    let running = search.clone();
    let run = move || {
        let mut scoped = scoped;
        scoped.extra = scope.extra_files();
        let sink = |matches: Vec<TextMatch>| {
            if let Ok(mut all) = running.matches.lock() {
                all.extend(matches);
            }
        };
        if let Err(error) = index.search_text_keeping(&scoped, &open, &running.cancel, &sink, &keep)
            && let Ok(mut slot) = running.error.lock()
        {
            *slot = Some(error);
        }
        running.done.store(true, Ordering::Relaxed);
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(run);
    #[cfg(target_arch = "wasm32")]
    run();
    Ok(search)
}

pub struct SearchResults;

impl Command for SearchResults {
    type Params = NoParams;
    type Output = SearchResult;
    const ID: &'static str = "search.text.results";
    const TITLE: &'static str = "Find in Files Results";
    const DESCRIPTION: &'static str =
        "Returns the matches found so far by the search started last, and whether it is done.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SearchResult, CommandError> {
        let workbench = session.state::<Workbench>();
        let search = &workbench.map(|workbench| workbench.text_search.clone()).unwrap_or_default();
        let matches = sorted(search.matches.lock().map(|matches| matches.clone()).unwrap_or_default());
        let mut result = SearchResult::new(
            &matches,
            search.done.load(Ordering::Relaxed),
            search.error.lock().ok().and_then(|error| error.clone()),
            workbench.and_then(Workbench::project),
        );
        result.hidden_files = crate::hidden_files::count(session).filter(|count| *count > 0);
        Ok(result)
    }
}

pub struct CancelSearch;

impl Command for CancelSearch {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "search.text.cancel";
    const TITLE: &'static str = "Stop Find in Files";
    const DESCRIPTION: &'static str = "Stops the running Find in Files search; the matches found so far stay.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        if let Some(workbench) = session.state::<Workbench>() {
            workbench.text_search.cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

const LISTING_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

pub struct FindInFiles;

impl Command for FindInFiles {
    type Params = SearchParams;
    type Output = SearchResult;
    const ID: &'static str = "search.text.find";
    const TITLE: &'static str = "Find in Files Now";
    const DESCRIPTION: &'static str = "Searches the project's files (open files in their unsaved text) and returns the \
         matches with the file (relative to the project), the line and column (one-based) and the line's text; \
         for agents, which want the answer at once. `group`: each path once.";

    fn run(&self, session: &mut Session, params: SearchParams) -> Result<SearchResult, CommandError> {
        let keep = keeping(highlighters(session), params.query.context);
        let scoped = params.search(session, 2000)?;
        let workbench =
            session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let index = index(workbench)?;
        let listed_by = std::time::Instant::now() + LISTING_WAIT;
        while !index.is_ready() && std::time::Instant::now() < listed_by {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let listed = index.is_ready();
        let found = Mutex::new(Vec::new());
        let sink = |matches: Vec<TextMatch>| {
            if let Ok(mut all) = found.lock() {
                all.extend(matches);
            }
        };
        let open = open_texts(workbench);
        let result = index.search_text_keeping(&scoped, &open, &AtomicBool::new(false), &sink, &keep);
        let matches = sorted(found.into_inner().unwrap_or_default());
        let mut answer =
            SearchResult::new(&matches, listed, result.err(), workbench.project()).shaped(params.group, params.compact);
        answer.hidden_files = crate::hidden_files::count(session).filter(|count| *count > 0);
        Ok(answer)
    }
}

/// The state of the project's text index.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatusInfo {
    /// Whether a project is open.
    pub project: bool,
    /// Whether the file names are listed.
    pub names_ready: bool,
    /// Whether the text index is built; until then the files it has not read are searched in full.
    pub ready: bool,
    /// Whether its first build is paused (`tasks.background.pause`); it keeps what it has read.
    pub paused: bool,
    /// The files the first build has read.
    pub indexed: usize,
    /// The files the first build reads.
    pub total: usize,
    /// The files whose text the index holds.
    pub files: usize,
    /// The files left out as binary, not UTF-8 or larger than 2.5 MB.
    pub skipped: usize,
    /// The distinct trigrams it holds.
    pub trigrams: usize,
}

pub struct IndexStatus;

impl Command for IndexStatus {
    type Params = NoParams;
    type Output = IndexStatusInfo;
    const ID: &'static str = "search.index.status";
    const TITLE: &'static str = "Find in Files Index Status";
    const DESCRIPTION: &'static str = "Tells whether the project's text index is built and how many files it has read, \
         holds and left out (binary, not UTF-8, larger than 2.5 MB); Find in Files reads only the files it names.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<IndexStatusInfo, CommandError> {
        let Some(index) = session.state::<Workbench>().and_then(|workbench| workbench.index.as_ref()) else {
            return Ok(IndexStatusInfo::default());
        };
        let status = index.text_status();
        Ok(IndexStatusInfo {
            project: true,
            names_ready: index.is_ready(),
            ready: status.ready,
            paused: status.paused,
            indexed: status.indexed,
            total: status.total,
            files: status.files,
            skipped: status.skipped,
            trigrams: status.trigrams,
        })
    }
}

/// A replacement over the project.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ReplaceParams {
    /// What and where.
    #[serde(flatten)]
    pub search: SearchParams,
    /// The replacement; `$1` and `${name}` refer to groups of a regular expression, and its case
    /// escapes convert case; with the query's Preserve Case it takes each match's case.
    pub replacement: String,
}

/// What Replace in Files changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Replaced {
    /// How many files changed.
    pub files: usize,
    /// How many matches were replaced.
    pub replacements: usize,
    /// For an agent: how many of the project's files are hidden from agents and were left alone
    /// (ADR-0143), whatever matched in them.
    #[serde(rename = "hiddenFiles", skip_serializing_if = "Option::is_none")]
    pub hidden_files: Option<usize>,
}

pub struct ReplaceInFiles;

impl Command for ReplaceInFiles {
    type Params = ReplaceParams;
    type Output = Replaced;
    const ID: &'static str = "search.text.replaceAll";
    const TITLE: &'static str = "Replace in Files";
    const DESCRIPTION: &'static str = "Replaces every match in the project's files (⇧⌘R) with the search bar's engine \
         (groups, case conversion, Preserve Case): open files change in the editor, one undo step per \
         file; other files are written on disk with a Local History version from before, keeping their CRLF line \
         breaks. A read-only file, or one in an encoding other than UTF-8, refuses the whole replacement.";

    fn run(&self, session: &mut Session, params: ReplaceParams) -> Result<Replaced, CommandError> {
        let keep = keeping(highlighters(session), params.search.query.context);
        let scoped = params.search.search(session, 0)?;
        let workbench = session.state_mut::<Workbench>();
        let index = index(workbench)?;
        let found = Mutex::new(Vec::new());
        let sink = |matches: Vec<TextMatch>| {
            if let Ok(mut all) = found.lock() {
                all.extend(matches);
            }
        };
        index
            .search_text_keeping(&scoped, &open_texts(workbench), &AtomicBool::new(false), &sink, &keep)
            .map_err(|error| CommandError::new(t!("workbench-error-search", error = error)))?;
        let matches = found.into_inner().unwrap_or_default();
        replace_found(session, matches, &params)
    }
}

#[derive(Debug)]
pub(crate) struct ReplaceSearch {
    search: TextSearch,
    open: HashMap<PathBuf, String>,
    params: ReplaceParams,
}

fn start_replace_search(session: &mut Session, params: ReplaceParams) -> Result<ReplaceSearch, CommandError> {
    let search = spawn(session, &params.search, 0)?;
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    Ok(ReplaceSearch { search, open: open_texts(workbench), params })
}

pub struct StartReplace;

impl Command for StartReplace {
    type Params = ReplaceParams;
    type Output = ();
    const ID: &'static str = "search.replace.start";
    const TITLE: &'static str = "Start Replace in Files";
    const DESCRIPTION: &'static str = "Starts Replace in Files with its search in the background, so the window stays \
         responsive; search.replace.finish applies it. A replacement already waiting is dropped.";

    fn run(&self, session: &mut Session, params: ReplaceParams) -> Result<(), CommandError> {
        let replacing = start_replace_search(session, params)?;
        let workbench = session.state_mut::<Workbench>();
        if let Some(earlier) = workbench.replace_search.replace(replacing) {
            earlier.search.cancel();
        }
        Ok(())
    }
}

pub struct FinishReplace;

impl Command for FinishReplace {
    type Params = NoParams;
    type Output = Option<Replaced>;
    const ID: &'static str = "search.replace.finish";
    const TITLE: &'static str = "Finish Replace in Files";
    const DESCRIPTION: &'static str = "Applies the replacement search.replace.start began once its search is done, and \
         answers what changed; null while it is still searching or when none is waiting. When an open file changed \
         during the search, the search runs again first.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<Replaced>, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        if !workbench.replace_search.as_ref().is_some_and(|replacing| replacing.search.is_done()) {
            return Ok(None);
        }
        let Some(done) = workbench.replace_search.take() else { return Ok(None) };
        if done.open != open_texts(workbench) {
            let replacing = start_replace_search(session, done.params)?;
            session.state_mut::<Workbench>().replace_search = Some(replacing);
            return Ok(None);
        }
        let failed = done.search.error.lock().ok().and_then(|error| error.clone());
        if let Some(error) = failed {
            return Err(CommandError::new(t!("workbench-error-search", error = error)));
        }
        replace_found(session, done.search.matches(), &done.params).map(Some)
    }
}

fn replace_found(
    session: &mut Session,
    matches: Vec<TextMatch>,
    params: &ReplaceParams,
) -> Result<Replaced, CommandError> {
    let mut by_file: HashMap<PathBuf, Vec<TextMatch>> = HashMap::new();
    for found in matches {
        by_file.entry(found.path.clone()).or_default().push(found);
    }
    let files: Vec<&PathBuf> = by_file.keys().collect();
    crate::trust::guard_agent_data_folder(session, &files)?;
    replace_matches(session, by_file.into_iter().collect(), &params.search.query, &params.replacement)
        .map(|(replaced, _)| replaced)
}

type Plan = (PathBuf, bool, String, Vec<(Range<usize>, String)>);

pub(crate) fn line_text_of(text: &str, line: usize) -> Option<&str> {
    let mut rest = text;
    for _ in 0..line {
        let end = rest.find(['\n', '\r'])?;
        rest = &rest[end + if rest[end..].starts_with("\r\n") { 2 } else { 1 }..];
    }
    rest.split(['\n', '\r']).next()
}

fn refused(error: String) -> CommandError {
    CommandError::new(error)
}

pub(crate) fn replace_matches(
    session: &mut Session,
    mut matches: Vec<(PathBuf, Vec<TextMatch>)>,
    query: &Query,
    replacement: &str,
) -> Result<(Replaced, Vec<Rewritten>), CommandError> {
    matches.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        matches.retain(|(path, _)| !crate::hidden_files::is_hidden(&patterns, root.as_deref(), path));
    }
    let mut plans: Vec<Plan> = Vec::new();
    for (path, found) in matches {
        if found.is_empty() {
            continue;
        }
        let open =
            session.state::<Workbench>().is_some_and(|workbench| workbench.documents.iter().any(|d| d.path == path));
        if !open && crate::editor_config::charset(session, &path).is_some_and(|charset| !charset.is_utf8()) {
            continue;
        }
        let workbench = session.state_mut::<Workbench>();
        let root = workbench.project().map(Path::to_owned);
        if root.as_deref().is_none_or(|root| !crate::inside_project(root, &path)) {
            return Err(refused(t!("workbench-error-replace-outside", file = path.display().to_string())));
        }
        if crate::guard::refuses_in(root.as_deref(), &path) {
            return Err(refused(crate::guard::refusal(&path)));
        }
        if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) {
            document.locked = crate::read_only::lock_reason(&path);
        }
        let open = workbench.documents.iter().find(|document| document.image().is_none() && document.path() == path);
        let (text, in_editor) = match open {
            Some(document) => {
                if let Some(reason) = document.read_only() {
                    return Err(refused(reason.to_owned()));
                }
                if !writable(&path) {
                    return Err(refused(t!("workbench-error-replace-read-only", file = path.display().to_string())));
                }
                (document.text().to_owned(), true)
            }
            None => (read_for_replace(session, &path)?, false),
        };
        let file = path.display().to_string();
        let changed = || refused(t!("workbench-error-replace-changed", file = file.as_str()));
        if !found.iter().all(|one| line_text_of(&text, one.line) == Some(one.line_text.as_str())) {
            return Err(changed());
        }
        let wanted: std::collections::HashSet<Range<usize>> = found.iter().map(|one| one.range.clone()).collect();
        let replacements: Vec<(Range<usize>, String)> = zen_text::search::replacements(&text, query, replacement)
            .map_err(|error| refused(t!("workbench-error-search", error = error)))?
            .into_iter()
            .filter(|(range, _)| wanted.contains(range))
            .map(|(range, new)| (range, with_line_breaks_of(&text, &new)))
            .collect();
        if replacements.len() != wanted.len() {
            return Err(changed());
        }
        plans.push((path, in_editor, text, replacements));
    }
    let mut replaced = Replaced { files: 0, replacements: 0, hidden_files: crate::hidden_files::count(session) };
    let mut rewritten = Vec::new();
    let mut written: Vec<(PathBuf, String, String)> = Vec::new();
    let mut in_editors = Vec::new();
    for (path, in_editor, text, replacements) in plans {
        replaced.files += 1;
        replaced.replacements += replacements.len();
        let mut chars: Vec<char> = text.chars().collect();
        for (range, new) in replacements.iter().rev() {
            chars.splice(range.clone(), new.chars());
        }
        let new_text: String = chars.into_iter().collect();
        let edits = replacements.iter().map(|(range, new)| (range.clone(), new.chars().count())).collect();
        rewritten.push(Rewritten { path: path.clone(), edits, text: new_text.clone() });
        if in_editor {
            in_editors.push((path, replacements));
            continue;
        }
        if let Err(error) = zen_vfs::replace_unchanged(&path, Some(text.as_bytes()), new_text.as_bytes()) {
            let mut message = if error.is_changed_on_disk() {
                t!("workbench-error-replace-changed", file = path.display().to_string())
            } else {
                t!("workbench-error-save", file = path.display().to_string(), error = error.to_string())
            };
            for (done, before, after) in written.iter().rev() {
                if let Err(error) = zen_vfs::replace_unchanged(done, Some(after.as_bytes()), before.as_bytes()) {
                    message.push('\n');
                    message.push_str(&t!(
                        "workbench-error-replace-not-restored",
                        file = done.display().to_string(),
                        error = error.to_string()
                    ));
                }
            }
            return Err(refused(message));
        }
        written.push((path, text, new_text));
    }
    let workbench = session.state_mut::<Workbench>();
    for (path, replacements) in in_editors {
        if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) {
            document.replace_ranges(replacements);
        }
    }
    let written = written
        .into_iter()
        .map(|(path, before, after)| (path, Some(before.into_bytes()), Some(after.into_bytes())))
        .collect();
    crate::local_history::changed_by(session, crate::local_history::REPLACE_IN_FILES, written);
    Ok((replaced, rewritten))
}

pub(crate) struct Rewritten {
    pub(crate) path: PathBuf,
    pub(crate) edits: Vec<(Range<usize>, usize)>,
    pub(crate) text: String,
}

fn writable(path: &Path) -> bool {
    !path.exists() || std::fs::OpenOptions::new().write(true).open(path).is_ok()
}

fn read_for_replace(session: &Session, path: &Path) -> Result<String, CommandError> {
    let file = path.display().to_string();
    if let Some(charset) = crate::project_settings::charset_for(session, path)
        .filter(|charset| !zen_project::settings::is_utf8_compatible(charset))
    {
        return Err(refused(t!("workbench-error-replace-encoding", file = file.as_str(), charset = charset.as_str())));
    }
    if !writable(path) {
        return Err(refused(t!("workbench-error-replace-read-only", file = file.as_str())));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| refused(t!("workbench-error-open", path = file.as_str(), error = error.to_string())))?;
    String::from_utf8(bytes)
        .map_err(|_| refused(t!("workbench-error-replace-encoding", file = file.as_str(), charset = "UTF-8")))
}

/// Which lines of a file to preview.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewParams {
    /// The file, as a match reports it: relative to the project or absolute.
    pub path: PathBuf,
    /// The line of the match, one-based.
    pub line: usize,
    /// How many lines around it; 9 when absent.
    #[serde(default)]
    pub lines: Option<usize>,
}

/// The lines around a match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The number of the first line, one-based.
    pub first: usize,
    /// The lines, without their line breaks.
    pub lines: Vec<String>,
    /// Whether the text is the open editor's, with any unsaved changes, rather than the disk's.
    pub from_editor: bool,
}

pub struct PreviewMatch;

impl Command for PreviewMatch {
    type Params = PreviewParams;
    type Output = Preview;
    const ID: &'static str = "search.text.preview";
    const TITLE: &'static str = "Preview Find in Files Match";
    const DESCRIPTION: &'static str = "Returns the lines around a line of a file, as the Find in Files preview shows \
         them: an open file in its editor text with unsaved changes, any other from the disk.";

    fn run(&self, session: &mut Session, params: PreviewParams) -> Result<Preview, CommandError> {
        let workbench = session.state::<Workbench>();
        let path = workbench.map_or_else(|| params.path.clone(), |workbench| workbench.resolve(&params.path));
        let open = workbench.and_then(|workbench| {
            workbench.documents().iter().find(|document| document.image().is_none() && document.path() == path)
        });
        let disk;
        let (text, from_editor) = match open {
            Some(document) => (document.text(), true),
            None => {
                disk = zen_vfs::read_text(&path).map_err(|error| {
                    CommandError::new(t!(
                        "workbench-error-open",
                        path = path.display().to_string(),
                        error = error.to_string()
                    ))
                })?;
                (disk.as_str(), false)
            }
        };
        let count = params.lines.unwrap_or(9).max(1);
        let first = params.line.saturating_sub(1).saturating_sub(count / 2);
        let lines = text.lines().skip(first).take(count).map(str::to_owned).collect();
        Ok(Preview { first: first + 1, lines, from_editor })
    }
}

/// A change of a file made in the Find in Files preview: the text between two places replaced.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditParams {
    /// The file, as a match reports it.
    pub path: PathBuf,
    /// The line of the first character replaced, from 1.
    pub line: usize,
    /// Its column in characters, from 1.
    pub column: usize,
    /// The line of the character after the replaced text, from 1.
    pub end_line: usize,
    /// Its column in characters, from 1.
    pub end_column: usize,
    /// The text put in its place.
    pub text: String,
    /// The lines the preview showed from `expected_from`; the edit is refused when the file differs.
    #[serde(default)]
    pub expected: Option<Vec<String>>,
    /// The line of the first of `expected`, from 1, as `search.text.preview`'s `first`.
    #[serde(default = "first_line")]
    pub expected_from: usize,
}

/// What `search.text.edit` changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Edited {
    /// Whether the file's open editor took the change (one undo step) rather than the disk.
    pub from_editor: bool,
}

fn first_line() -> usize {
    1
}

fn from_one(params: &mut EditParams) -> Result<(), CommandError> {
    for place in
        [&mut params.line, &mut params.column, &mut params.end_line, &mut params.end_column, &mut params.expected_from]
    {
        *place = place.checked_sub(1).ok_or_else(|| CommandError::new(t!("workbench-error-places-from-one")))?;
    }
    Ok(())
}

fn still_shows(text: &str, params: &EditParams) -> bool {
    params.expected.as_ref().is_none_or(|expected| {
        let lines: Vec<&str> = text.lines().skip(params.expected_from).take(expected.len()).collect();
        lines.len() == expected.len() && lines.iter().zip(expected).all(|(line, shown)| line == shown)
    })
}

fn with_line_breaks_of(text: &str, inserted: &str) -> String {
    let crlf = text.find('\n').is_some_and(|at| text[..at].ends_with('\r'));
    if !crlf {
        return inserted.to_owned();
    }
    inserted.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn char_offset_of(text: &str, line: usize, column: usize) -> Option<usize> {
    let mut offset = 0;
    for (index, row) in text.split('\n').enumerate() {
        let whole = row.chars().count();
        if index == line {
            let length = row.strip_suffix('\r').map_or(whole, |row| row.chars().count());
            return Some(offset + column.min(length));
        }
        offset += whole + 1;
    }
    None
}

pub struct EditPreview;

impl Command for EditPreview {
    type Params = EditParams;
    type Output = Edited;
    const ID: &'static str = "search.text.edit";
    const TITLE: &'static str = "Edit in Find in Files Preview";
    const DESCRIPTION: &'static str = "Replaces the text between two places of a project file, as typing in the Find in \
         Files preview does: an open file changes in its editor (one undo step), any other on disk, with a Local \
         History version from before, keeping its CRLF line breaks. With `expected` (the lines the preview showed, \
         from `expectedFrom`) it is refused when the file changed since; a read-only file, or one in an encoding \
         other than UTF-8, is refused too.";

    fn run(&self, session: &mut Session, mut params: EditParams) -> Result<Edited, CommandError> {
        from_one(&mut params)?;
        let workbench = session.state_mut::<Workbench>();
        params.path = workbench.resolve(&params.path);
        let root = workbench.project().map(Path::to_owned);
        if root.as_deref().is_none_or(|root| !crate::inside_project(root, &params.path)) {
            return Err(CommandError::new(t!(
                "workbench-error-replace-outside",
                file = params.path.display().to_string()
            )));
        }
        if crate::guard::refuses_in(root.as_deref(), &params.path) {
            return Err(CommandError::new(crate::guard::refusal(&params.path)));
        }
        let range = |text: &str| -> Result<Range<usize>, CommandError> {
            if !still_shows(text, &params) {
                return Err(CommandError::new(t!("workbench-error-preview-place")));
            }
            let start = char_offset_of(text, params.line, params.column);
            let end = char_offset_of(text, params.end_line, params.end_column);
            match (start, end) {
                (Some(start), Some(end)) if start <= end => Ok(start..end),
                _ => Err(CommandError::new(t!("workbench-error-preview-place"))),
            }
        };
        let open = workbench
            .documents
            .iter_mut()
            .find(|document| document.image().is_none() && document.path() == params.path);
        if let Some(document) = open {
            if let Some(reason) = document.read_only() {
                return Err(CommandError::new(reason.to_owned()));
            }
            let range = range(document.text())?;
            let inserted = with_line_breaks_of(document.text(), &params.text);
            document.replace_ranges(vec![(range, inserted)]);
            return Ok(Edited { from_editor: true });
        }
        if let Some(charset) = crate::project_settings::charset_for(session, &params.path)
            .filter(|charset| !zen_project::settings::is_utf8_compatible(charset))
        {
            let file = params.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(CommandError::new(t!(
                "workbench-error-encoding",
                file = file.as_str(),
                charset = charset.as_str(),
                kind = "charset"
            )));
        }
        if std::fs::metadata(&params.path).is_ok_and(|metadata| metadata.permissions().readonly()) {
            return Err(CommandError::new(t!(
                "workbench-error-preview-read-only",
                file = params.path.display().to_string()
            )));
        }
        let text = zen_vfs::read_text(&params.path).map_err(|error| {
            CommandError::new(t!(
                "workbench-error-open",
                path = params.path.display().to_string(),
                error = error.to_string()
            ))
        })?;
        let range = range(&text)?;
        let mut chars: Vec<char> = text.chars().collect();
        chars.splice(range, with_line_breaks_of(&text, &params.text).chars());
        let new_text: String = chars.into_iter().collect();
        zen_vfs::replace_unchanged(&params.path, Some(text.as_bytes()), new_text.as_bytes()).map_err(|error| {
            CommandError::new(t!(
                "workbench-error-save",
                file = params.path.display().to_string(),
                error = error.to_string()
            ))
        })?;
        let written = vec![(params.path, Some(text.into_bytes()), Some(new_text.into_bytes()))];
        crate::local_history::changed_by(session, crate::local_history::PREVIEW_EDIT, written);
        Ok(Edited { from_editor: false })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, Registry, Session) {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["ready"] != true {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        (dir, registry, session)
    }

    #[test]
    fn find_in_files_right_after_opening_waits_for_the_files_to_be_listed() {
        zen_i18n::register(crate::LOCALES);
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            for index in 0..200 {
                std::fs::write(dir.path().join(format!("f{index}.go")), "package main\n").unwrap();
            }
            std::fs::write(dir.path().join("main.go"), "package main\n\nfunc a() {}\n").unwrap();
            let mut registry = Registry::new();
            crate::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            let found =
                registry.dispatch(&mut session, "search.text.find", json!({ "query": { "text": "func a" } })).unwrap();
            assert_eq!(found["matches"][0]["line"], 3, "no noMatch while the files are still being listed: {found}");
        }
    }

    #[test]
    fn find_in_files_can_name_each_file_once() {
        let (_dir, registry, mut session) = project(&[("a.go", "Total\nx := Total\n"), ("b.go", "Total\n")]);
        let mut find = |extra: serde_json::Value| {
            let mut params = json!({ "query": { "text": "Total" } });
            params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            registry.dispatch(&mut session, "search.text.find", params).unwrap()
        };
        let plain = find(json!({}));
        assert_eq!(plain["matches"].as_array().unwrap().len(), 3, "the default stays: {plain}");
        assert!(plain.get("files").is_none() && plain.get("rows").is_none());
        let grouped = find(json!({ "group": true }));
        assert!(grouped.get("matches").is_none(), "{grouped}");
        assert_eq!(grouped["files"][0]["path"], "a.go");
        assert_eq!(
            grouped["files"][0]["matches"][1],
            json!({ "line": 2, "column": 6, "length": 5, "offset": 11, "text": "x := Total" })
        );
        assert!(grouped["files"][0]["matches"][0].get("path").is_none(), "the path is said once");
        assert_eq!(grouped["files"][1]["path"], "b.go");
        assert_eq!(
            find(json!({ "group": true, "compact": true }))["rows"],
            json!(["a.go", "1:1 Total", "2:6 x := Total", "b.go", "1:1 Total"])
        );
        assert_eq!(find(json!({ "compact": true }))["rows"][2], "b.go:1:1 Total");
    }

    #[test]
    fn replace_in_files_searches_in_the_background_and_starts_over_when_an_open_file_changed() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("a.go"), "Total\n").unwrap();
        std::fs::write(root.join("b.go"), "Total\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["ready"] != true {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("a.go") })).unwrap();
        let waiting = registry.dispatch(&mut session, "search.replace.finish", json!({})).unwrap();
        assert!(waiting.is_null(), "nothing is waiting");
        let params = json!({ "query": { "text": "Total" }, "replacement": "Sum" });
        registry.dispatch(&mut session, "search.replace.start", params).unwrap();
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "Total " })).unwrap();
        let start = std::time::Instant::now();
        let replaced = loop {
            let answer = registry.dispatch(&mut session, "search.replace.finish", json!({})).unwrap();
            if !answer.is_null() {
                break answer;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "the replacement never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(replaced["replacements"], 3, "the typing during the search is counted: {replaced}");
        assert_eq!(std::fs::read_to_string(root.join("b.go")).unwrap(), "Sum\n");
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "Sum Sum\n");
    }

    #[test]
    fn the_preview_edits_a_closed_file_on_disk_and_an_open_one_in_its_editor() {
        let (dir, registry, mut session) = project(&[("a.go", "package a\r\n\r\nvar Version = 1\r\n")]);
        let found =
            registry.dispatch(&mut session, "search.text.find", json!({ "query": { "text": "Version" } })).unwrap();
        let path = found["matches"][0]["path"].clone();
        let file = dir.path().join(path.as_str().unwrap());
        let edit = |line: usize, column: usize, end_column: usize, text: &str| json!({ "path": path, "line": line, "column": column, "endLine": line, "endColumn": end_column, "text": text });
        let edited = registry.dispatch(&mut session, "search.text.edit", edit(3, 5, 12, "Release")).unwrap();
        assert_eq!(edited["fromEditor"], false);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package a\r\n\r\nvar Release = 1\r\n");
        let joined = json!({ "path": path, "line": 1, "column": 10, "endLine": 2, "endColumn": 1, "text": "" });
        registry.dispatch(&mut session, "search.text.edit", joined).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "package a\r\nvar Release = 1\r\n",
            "the line break, \\r and all"
        );
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let edited = registry.dispatch(&mut session, "search.text.edit", edit(2, 16, 16, " // one")).unwrap();
        assert_eq!(edited["fromEditor"], true);
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "package a\r\nvar Release = 1 // one\r\n");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package a\r\nvar Release = 1\r\n", "not saved by itself");
        let outside =
            json!({ "path": "/elsewhere.go", "line": 1, "column": 1, "endLine": 1, "endColumn": 1, "text": "x" });
        assert!(registry.dispatch(&mut session, "search.text.edit", outside).is_err());
        assert!(registry.dispatch(&mut session, "search.text.edit", edit(9, 1, 1, "x")).is_err(), "no such line");
    }

    #[test]
    fn a_preview_edit_takes_a_match_place_as_find_gives_it() {
        let (dir, registry, mut session) = project(&[("a.go", "package a\n\nvar Version = 1\n")]);
        let found =
            registry.dispatch(&mut session, "search.text.find", json!({ "query": { "text": "Version" } })).unwrap();
        let found = &found["matches"][0];
        let (line, column) = (found["line"].as_u64().unwrap(), found["column"].as_u64().unwrap());
        let edit = json!({ "path": found["path"], "line": line, "column": column, "endLine": line,
            "endColumn": column + 7, "text": "Release", "expected": ["var Version = 1"], "expectedFrom": line });
        registry.dispatch(&mut session, "search.text.edit", edit).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.go")).unwrap(), "package a\n\nvar Release = 1\n");
        let zero = json!({ "path": "a.go", "line": 0, "column": 1, "endLine": 1, "endColumn": 1, "text": "x" });
        assert!(registry.dispatch(&mut session, "search.text.edit", zero).is_err(), "lines count from 1");
    }

    #[test]
    fn the_preview_refuses_a_changed_or_read_only_file_and_keeps_crlf() {
        let (dir, registry, mut session) = project(&[("a.go", "package a\r\n\r\nvar Version = 1\r\n")]);
        let file = dir.path().join("a.go");
        let shown = ["package a", "", "var Version = 1"];
        let edit = |line: usize, column: usize, text: &str| {
            json!({ "path": file, "line": line, "column": column, "endLine": line, "endColumn": column,
                "text": text, "expected": shown, "expectedFrom": 1 })
        };
        registry.dispatch(&mut session, "search.text.edit", edit(1, 10, "\n// a")).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package a\r\n// a\r\n\r\nvar Version = 1\r\n");
        let stale = registry.dispatch(&mut session, "search.text.edit", edit(3, 1, "x"));
        assert!(stale.is_err(), "the file changed since the preview read it");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package a\r\n// a\r\n\r\nvar Version = 1\r\n");
        std::fs::write(&file, "package a\r\n\r\nvar Version = 1\r\n").unwrap();
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions.clone()).unwrap();
        let refused = registry.dispatch(&mut session, "search.text.edit", edit(3, 1, "x"));
        assert!(refused.is_err(), "a read-only file is not written");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package a\r\n\r\nvar Version = 1\r\n");
        #[allow(clippy::permissions_set_readonly_false, reason = "the temporary folder is removed afterwards")]
        permissions.set_readonly(false);
        std::fs::set_permissions(&file, permissions).unwrap();
    }

    #[test]
    fn an_edit_cannot_climb_out_of_the_project_with_dots() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(base.join("outside.txt"), "secret\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        let path = root.join("../outside.txt");
        let edit = json!({ "path": path, "line": 1, "column": 1, "endLine": 1, "endColumn": 7, "text": "public" });
        assert!(registry.dispatch(&mut session, "search.text.edit", edit).is_err());
        assert_eq!(std::fs::read_to_string(base.join("outside.txt")).unwrap(), "secret\n");
    }

    #[test]
    fn line_texts_follow_every_kind_of_line_break() {
        let text = "one\rtwo\r\nthree\nfour";
        let lines: Vec<_> = (0..5).map(|line| line_text_of(text, line)).collect();
        assert_eq!(lines, [Some("one"), Some("two"), Some("three"), Some("four"), None]);
        assert_eq!(line_text_of("a\n", 1), Some(""));
        assert_eq!(line_text_of("a\n", 2), None);
    }

    #[test]
    fn a_folder_is_relative_or_absolute_and_a_wrong_one_is_an_error_not_no_matches() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("store")).unwrap();
        std::fs::write(root.join("store/a.go"), "package store\n\nfunc Total() int { return 1 }\n").unwrap();
        std::fs::write(root.join("main.go"), "package main\n\n// Total of all.\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["ready"] != true {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut find = |directory: &str, text: &str| {
            let params = json!({ "query": { "text": text }, "directory": directory });
            registry.dispatch(&mut session, "search.text.find", params)
        };
        let relative = find("store", "Total").unwrap();
        let absolute = find(&root.join("store").display().to_string(), "Total").unwrap();
        assert_eq!(relative["matches"].as_array().unwrap().len(), 1, "{relative}");
        assert_eq!(absolute, relative, "an absolute folder of the project is the same folder");
        assert!(relative.get("reason").is_none());
        let whole = find(&root.display().to_string(), "Total").unwrap();
        assert_eq!(whole["matches"].as_array().unwrap().len(), 2, "{whole}");
        let none = find("store", "Nothing").unwrap();
        assert_eq!((&none["matches"], &none["reason"]), (&json!([]), &json!("noMatch")), "{none}");
        let missing = find("nosuch", "Total").unwrap_err().to_string();
        assert!(
            missing.contains(&t!(
                "workbench-scope-directory-missing",
                directory = "nosuch",
                project = root.display().to_string()
            )),
            "{missing}"
        );
        let outside = find("/elsewhere", "Total").unwrap_err().to_string();
        let expected =
            t!("workbench-scope-directory-outside", directory = "/elsewhere", project = root.display().to_string());
        assert!(outside.contains(&expected), "{outside}");
        assert!(find("../x", "Total").is_err());
    }

    #[test]
    fn find_and_replace_in_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.go"), "package a\n\nfunc Total() int { return total }\n").unwrap();
        std::fs::write(dir.path().join("b.go"), "package b\n\n// Total of all.\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["ready"] != true {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let status = registry.dispatch(&mut session, "search.index.status", json!({})).unwrap();
        assert_eq!(
            (status["files"].as_u64(), status["indexed"].as_u64(), status["namesReady"].as_bool()),
            (Some(2), Some(2), Some(true))
        );
        let tasks = session.state::<crate::tasks::Tasks>().unwrap().list();
        let task = tasks.iter().find(|task| task.source == "index").unwrap();
        assert_eq!(task.state, crate::tasks::TaskState::Succeeded);
        let found = registry
            .dispatch(&mut session, "search.text.find", json!({ "query": { "text": "Total", "matchCase": true } }))
            .unwrap();
        assert_eq!(found["matches"].as_array().unwrap().len(), 2);
        assert!(found["matches"][0].get("relative").is_none() && found.get("error").is_none(), "{found}");
        assert_eq!(
            (found["matches"][0]["path"].as_str(), found["matches"][0]["line"].as_u64()),
            (Some("a.go"), Some(3))
        );

        registry.dispatch(&mut session, "search.text.start", json!({ "query": { "text": "total" } })).unwrap();
        let mut results = json!(null);
        for _ in 0..400 {
            results = registry.dispatch(&mut session, "search.text.results", json!({})).unwrap();
            if results["done"] == true {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(results["matches"].as_array().unwrap().len(), 3, "case-insensitive: Total twice, total once");

        registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join("a.go") })).unwrap();
        let replaced = registry
            .dispatch(
                &mut session,
                "search.text.replaceAll",
                json!({ "query": { "text": "Total", "matchCase": true }, "replacement": "Sum" }),
            )
            .unwrap();
        assert_eq!((replaced["files"].as_u64(), replaced["replacements"].as_u64()), (Some(2), Some(2)));
        let a = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(a, "package a\n\nfunc Sum() int { return total }\n");
        assert_eq!(std::fs::read_to_string(dir.path().join("b.go")).unwrap(), "package b\n\n// Sum of all.\n");
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        assert!(session.state::<Workbench>().unwrap().active_document().unwrap().text().contains("Total"));
    }

    struct Latin1;

    impl zen_project::settings::SettingsProvider for Latin1 {
        fn id(&self) -> &str {
            "latin1"
        }

        fn watches(&self, _: &Path) -> bool {
            false
        }

        fn load(
            &self,
            root: &Path,
            _: &dyn zen_project::settings::ReadFiles,
        ) -> Option<zen_project::settings::ProjectSettings> {
            let file = zen_project::settings::FileEncoding { path: root.to_owned(), charset: "ISO-8859-1".to_owned() };
            let encodings =
                zen_project::settings::Encodings { default: None, files: vec![file], bom_for_new_files: None };
            Some(zen_project::settings::ProjectSettings { encodings, ..Default::default() })
        }
    }

    #[test]
    fn replace_in_files_never_writes_a_closed_file_of_another_charset_as_utf8() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "total\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        let settings = session.state_mut::<crate::project_settings::ProjectSettingsState>();
        settings.set_providers(vec![Arc::new(Latin1)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["ready"] != true {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let replaced = registry.dispatch(
            &mut session,
            "search.text.replaceAll",
            json!({ "query": { "text": "total" }, "replacement": "summe" }),
        );
        assert!(format!("{replaced:?}").contains("ISO-8859-1"), "{replaced:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "total\n");
    }

    #[test]
    fn preview_shows_the_unsaved_text_of_an_open_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.go");
        std::fs::write(&path, "package a\n\nfunc Total() int { return 0 }\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        while !session.state::<Workbench>().unwrap().index.as_ref().unwrap().is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let preview =
            registry.dispatch(&mut session, "search.text.preview", json!({ "path": path, "line": 3 })).unwrap();
        assert_eq!(preview["lines"][2], "func Total() int { return 0 }");
        assert_eq!(preview["fromEditor"], false);

        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        registry
            .dispatch(
                &mut session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": { "line": 0, "column": 0 }, "head": { "line": 0, "column": 0 } }] }),
            )
            .unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "// Package a.\n" })).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("package a"));

        let found = registry
            .dispatch(&mut session, "search.text.find", json!({ "query": { "text": "Total", "matchCase": true } }))
            .unwrap();
        let row = &found["matches"][0];
        assert_eq!(row["path"], "a.go", "relative to the project");
        assert_eq!((row["line"].as_u64(), row["column"].as_u64(), row["length"].as_u64()), (Some(4), Some(6), Some(5)));
        let preview = registry
            .dispatch(
                &mut session,
                "search.text.preview",
                json!({ "path": row["path"], "line": row["line"], "lines": 4 }),
            )
            .unwrap();
        assert_eq!(preview["fromEditor"], true);
        assert_eq!(preview["first"], 2, "one-based, as the match's line");
        assert_eq!(preview["lines"], json!(["package a", "", "func Total() int { return 0 }"]));
        let shown: Vec<char> = preview["lines"][2].as_str().unwrap().chars().collect();
        assert_eq!(shown.iter().collect::<String>(), row["text"].as_str().unwrap());
        assert_eq!(shown[5..10].iter().collect::<String>(), "Total");
    }

    #[test]
    fn find_and_replace_in_files_keep_to_the_filter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.go"), "total := \"total\" // total\n").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "total\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        crate::find::tests::literals(&mut session);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        while !session.state::<Workbench>().unwrap().index.as_ref().unwrap().is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut found = |context: &str| {
            let params = json!({ "query": { "text": "total", "context": context } });
            let result = registry.dispatch(&mut session, "search.text.find", params).unwrap();
            result["matches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| (m["path"].as_str().unwrap().to_owned(), m["column"].as_u64().unwrap()))
                .collect::<Vec<_>>()
        };
        let at = |file: &str, column: u64| (file.to_owned(), column);
        assert_eq!(found("anywhere").len(), 4);
        assert_eq!(found("inComments"), [at("a.go", 21)]);
        assert_eq!(found("inStringLiterals"), [at("a.go", 11)]);
        assert_eq!(found("exceptCommentsAndStringLiterals"), [at("a.go", 1), at("notes.txt", 1)]);

        registry
            .dispatch(
                &mut session,
                "search.text.start",
                json!({ "query": { "text": "total", "context": "inComments" } }),
            )
            .unwrap();
        let mut results = json!(null);
        for _ in 0..400 {
            results = registry.dispatch(&mut session, "search.text.results", json!({})).unwrap();
            if results["done"] == true {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(results["matches"].as_array().unwrap().len(), 1, "in the background too");

        let replaced = registry
            .dispatch(
                &mut session,
                "search.text.replaceAll",
                json!({ "query": { "text": "total", "context": "inStringLiterals" }, "replacement": "sum" }),
            )
            .unwrap();
        assert_eq!(replaced["replacements"], 1);
        let a = std::fs::read_to_string(dir.path().join("a.go")).unwrap();
        assert_eq!(a, "total := \"sum\" // total\n");
    }

    #[test]
    fn replace_in_files_converts_case_and_preserves_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.go"), "fooBar := FooBar(FOOBAR)\n").unwrap();
        std::fs::write(dir.path().join("b.go"), "get_name get_id\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        while !session.state::<Workbench>().unwrap().index.as_ref().unwrap().is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let preserve =
            json!({ "query": { "text": "fooBar", "preserveCase": true }, "replacement": "bazQux", "mask": "a.go" });
        let replaced = registry.dispatch(&mut session, "search.text.replaceAll", preserve).unwrap();
        assert_eq!(replaced["replacements"], 3);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.go")).unwrap(), "bazQux := BazQux(BAZQUX)\n");
        let regex = json!({
            "query": { "text": r"get_(\w)(\w*)", "regex": true, "matchCase": true },
            "replacement": r"Get\U$1\E$2",
            "mask": "b.go"
        });
        registry.dispatch(&mut session, "search.text.replaceAll", regex).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("b.go")).unwrap(), "GetName GetId\n");
    }
}

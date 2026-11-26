use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_index::{FileFilter, Search, SearchMemory, TextMatch};
use zen_syntax::Span;
use zen_text::search::Query;

use crate::Workbench;
use crate::brackets::{HighlighterSource, Highlighters};
use crate::search_scopes::ResolvedScope;
use crate::todo_patterns::{CompiledPatterns, TodoPatterns};
use crate::vcs::changelists::{ListChangelists, ListParams};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTodo)?;
    registry.register(TodoTabs)?;
    registry.register(StartTodoScan)?;
    registry.register(TodoScanResults)?;
    registry.register(DocumentTodos)?;
    registry.register(CommitCheck)?;
    Ok(())
}

const LIMIT: usize = 10_000;

/// A tab of the TODO tool window: which files its items come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TodoTab {
    /// Every file of the project.
    #[default]
    Project,
    /// The file in the editor.
    CurrentFile,
    /// The files of a search scope.
    ScopeBased,
    /// The files of the active changelist; only for a project in Git.
    CurrentChangelist,
}

impl TodoTab {
    pub const ALL: [TodoTab; 4] = [Self::Project, Self::CurrentFile, Self::ScopeBased, Self::CurrentChangelist];

    pub fn id(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::CurrentFile => "currentFile",
            Self::ScopeBased => "scopeBased",
            Self::CurrentChangelist => "currentChangelist",
        }
    }
}

/// Which TODO items to list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoParams {
    /// The tab: `project` (the default), `currentFile`, `scopeBased` or `currentChangelist`.
    pub tab: TodoTab,
    /// For `scopeBased`: a scope id from search.scope.list (`named:<name>` for the project's `.idea/scopes`); Project Files when absent.
    pub scope: Option<String>,
    /// For `currentFile`: the file; the file in front in the editor when absent.
    pub path: Option<PathBuf>,
    /// At most this many items; 10000 when absent.
    pub limit: Option<usize>,
    /// The name of a filter of Settings → Editor → TODO; every item when absent.
    pub filter: Option<String>,
}

/// One TODO item: a line of a comment that matches a pattern.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    /// The line, from 0.
    pub line: usize,
    /// The column of the TODO's first character, in characters from 0.
    pub column: usize,
    /// The TODO's first character as an offset in the file's characters, as navigate.place.goTo takes it.
    pub offset: usize,
    /// The TODO's length in characters.
    pub length: usize,
    /// The TODO's text: from its keyword to the end of the comment on that line.
    pub text: String,
    /// The whole line, without its line break.
    pub line_text: String,
    /// The indented lines below that belong to it, each as its text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub continued: Vec<String>,
}

/// The TODO items of one file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoFile {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the project, with `/`.
    pub relative: String,
    /// Its items, by line.
    pub items: Vec<TodoItem>,
}

/// The TODO items of a tab.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoList {
    /// The tab they are of.
    pub tab: TodoTab,
    /// The scope of `scopeBased`.
    pub scope: Option<String>,
    /// The files with items, by path.
    pub files: Vec<TodoFile>,
    /// How many items there are in all.
    pub total: usize,
    /// Whether the scan is finished.
    pub done: bool,
    /// The number of the scan (todo.scan.start); 0 for todo.items.list and before the first scan.
    pub scan: u64,
    /// Why the tab has no items to list, when it cannot have any (no file in the editor, no Git, an unknown scope).
    pub error: Option<String>,
}

/// The tabs the TODO tool window has for the open project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoTabList {
    /// The tabs, in order: Current Changelist only when the project is in Git.
    pub tabs: Vec<TodoTab>,
    /// The patterns the items are found by.
    pub patterns: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TodoScan {
    id: u64,
    params: TodoParams,
    matches: Arc<Mutex<Vec<TextMatch>>>,
    done: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

type Remembered = Arc<Mutex<HashMap<PathBuf, (u64, Vec<TextMatch>)>>>;

#[derive(Debug, Default)]
pub struct TodoState {
    scan: TodoScan,
    remembered: Remembered,
}

struct Memory {
    remembered: Remembered,
    source: Option<HighlighterSource>,
    patterns: String,
}

impl SearchMemory for Memory {
    fn stamp(&self, path: &Path, open: Option<&str>) -> Option<u64> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let highlighter = self.source.as_ref().and_then(|source| source(&name));
        self.patterns.hash(&mut hasher);
        highlighter.map(|highlighter| Arc::as_ptr(&highlighter).cast::<()>() as usize).hash(&mut hasher);
        match open {
            Some(text) => text.hash(&mut hasher),
            None => {
                let metadata = std::fs::metadata(path).ok()?;
                (metadata.len(), metadata.modified().ok()?).hash(&mut hasher);
            }
        }
        Some(hasher.finish())
    }

    fn recall(&self, path: &Path, stamp: u64) -> Option<Vec<TextMatch>> {
        let remembered = self.remembered.lock().ok()?;
        remembered.get(path).filter(|(seen, _)| *seen == stamp).map(|(_, matches)| matches.clone())
    }

    fn remember(&self, path: &Path, stamp: u64, matches: &[TextMatch]) {
        if let Ok(mut remembered) = self.remembered.lock() {
            remembered.insert(path.to_owned(), (stamp, matches.to_vec()));
        }
    }
}

fn memory(session: &mut Session) -> Memory {
    let remembered = Arc::clone(&session.state_mut::<TodoState>().remembered);
    let settings = TodoPatterns::of(session);
    let patterns = format!("{}{}", settings.query().text, settings.settings().multiline);
    Memory { remembered, source: highlighters(session), patterns }
}

impl TodoState {
    pub fn scan_id(&self) -> u64 {
        self.scan.id
    }

    pub fn is_done(&self) -> bool {
        self.scan.done.load(Ordering::Relaxed)
    }
}

fn comment_end(name: &str) -> Option<&'static str> {
    zen_syntax::comment_syntax(name).and_then(|syntax| syntax.block).map(|(_, end)| end)
}

pub fn todo_ranges(text: &str, ranges: &[Range<usize>], spans: &[Span], end: Option<&str>) -> Vec<Range<usize>> {
    let mut kept = Vec::new();
    let mut chars = text.char_indices().enumerate().peekable();
    let mut line_end = 0;
    for range in ranges {
        let mut start = text.len();
        while let Some(&(index, (byte, _))) = chars.peek() {
            if index >= range.start {
                if index == range.start {
                    start = byte;
                }
                break;
            }
            chars.next();
        }
        if start >= text.len() || (start < line_end && !kept.is_empty()) {
            continue;
        }
        let at = spans.partition_point(|span| span.range.end <= start);
        let Some(span) = spans.get(at).filter(|span| span.range.start <= start && span.kind.is_comment()) else {
            continue;
        };
        line_end = text[start..].find('\n').map_or(text.len(), |offset| start + offset);
        let stop = line_end.min(span.range.end);
        let mut body = text[start..stop].trim_end();
        if let Some(end) = end.filter(|_| span.range.end <= line_end) {
            body = body.strip_suffix(end).unwrap_or(body).trim_end();
        }
        let length = body.chars().count();
        if length > 0 {
            kept.push(range.start..range.start + length);
        }
    }
    kept
}

fn continuation_of(line: &str, column: usize, end: Option<&str>) -> Option<(usize, usize)> {
    let leading = line.chars().take_while(|c| c.is_whitespace()).count();
    let rest: String = line.chars().skip(leading).collect();
    let marker = rest.chars().take_while(|c| "/#*;-".contains(*c)).count();
    let after: String = rest.chars().skip(marker).collect();
    let gap = after.chars().take_while(|c| c.is_whitespace()).count();
    let start = leading + marker + gap;
    let body = after.trim();
    let body = end.and_then(|end| body.strip_suffix(end)).unwrap_or(body).trim_end();
    (!body.is_empty() && start > column).then(|| (start, body.chars().count()))
}

pub fn continuation_ranges(
    text: &str,
    ranges: &[Range<usize>],
    spans: &[Span],
    end: Option<&str>,
    is_todo: impl Fn(&str) -> bool,
) -> Vec<Range<usize>> {
    let mut lines: Vec<(usize, usize, &str)> = Vec::new();
    let (mut byte, mut char_at) = (0, 0);
    for line in text.split_inclusive('\n') {
        lines.push((byte, char_at, line.trim_end_matches(['\n', '\r'])));
        byte += line.len();
        char_at += line.chars().count();
    }
    let in_comment = |byte: usize| {
        let at = spans.partition_point(|span| span.range.end <= byte);
        spans.get(at).is_some_and(|span| span.range.start <= byte && span.kind.is_comment())
    };
    let mut found = Vec::new();
    for range in ranges {
        let line = lines.partition_point(|(_, start, _)| *start <= range.start).saturating_sub(1);
        let column = range.start - lines[line].1;
        for (byte, start, shown) in lines.iter().skip(line + 1) {
            let first = shown.len() - shown.trim_start().len();
            if shown.trim().is_empty() || !in_comment(byte + first) || is_todo(shown) {
                break;
            }
            let Some((from, length)) = continuation_of(shown, column, end) else { break };
            found.push(start + from..start + from + length);
        }
    }
    found
}

fn comments_only(
    source: Option<HighlighterSource>,
    patterns: &TodoPatterns,
) -> impl Fn(&Path, &str, Vec<Range<usize>>) -> Vec<Range<usize>> + Send + Sync + use<> {
    let compiled = patterns.compiled();
    let multiline = patterns.settings().multiline;
    move |path, text, ranges| {
        if ranges.is_empty() {
            return ranges;
        }
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(highlighter) = source.as_ref().and_then(|source| source(&name)) else { return Vec::new() };
        let spans = highlighter.highlight(text);
        let end = comment_end(&name);
        let mut kept = todo_ranges(text, &ranges, &spans, end);
        if multiline {
            kept.extend(continuation_ranges(text, &kept, &spans, end, |line| compiled.has_match(line)));
            kept.sort_by_key(|range| range.start);
        }
        kept
    }
}

fn item(found: &TextMatch) -> TodoItem {
    let length = found.range.len();
    let text: String = found.line_text.chars().skip(found.column).take(length).collect();
    TodoItem {
        line: found.line,
        column: found.column,
        offset: found.range.start,
        length,
        text,
        line_text: found.line_text.clone(),
        continued: Vec::new(),
    }
}

fn joined(items: Vec<TodoItem>, compiled: &CompiledPatterns) -> Vec<TodoItem> {
    let mut joined: Vec<TodoItem> = Vec::with_capacity(items.len());
    for item in items {
        match joined.last_mut() {
            Some(last) if item.line == last.line + last.continued.len() + 1 && !compiled.has_match(&item.text) => {
                last.continued.push(item.text);
            }
            _ => joined.push(item),
        }
    }
    joined
}

fn grouped(matches: &[TextMatch], compiled: &CompiledPatterns, filter: Option<&str>) -> (Vec<TodoFile>, usize) {
    let mut files: BTreeMap<&str, TodoFile> = BTreeMap::new();
    for found in matches {
        files
            .entry(found.relative.as_str())
            .or_insert_with(|| TodoFile {
                path: found.path.clone(),
                relative: found.relative.clone(),
                items: Vec::new(),
            })
            .items
            .push(item(found));
    }
    let mut files: Vec<TodoFile> = files.into_values().collect();
    for file in &mut files {
        file.items.sort_by_key(|item| (item.line, item.column));
        file.items.dedup_by_key(|item| item.line);
        file.items = joined(std::mem::take(&mut file.items), compiled);
        file.items.retain(|item| compiled.passes(filter, &item.text));
    }
    files.retain(|file| !file.items.is_empty());
    let total = files.iter().map(|file| file.items.len()).sum();
    (files, total)
}

fn project(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
}

fn in_git(session: &mut Session) -> bool {
    crate::vcs::repository(session).is_ok()
}

enum Files {
    Scope(ResolvedScope),
    One(PathBuf, String),
    Nothing(String),
}

fn files_of(session: &mut Session, params: &TodoParams, wait: bool) -> Result<Files, CommandError> {
    let root = project(session)?;
    match params.tab {
        TodoTab::Project => Ok(Files::Scope(crate::search_scopes::resolve_in(session, None, "", true)?)),
        TodoTab::ScopeBased => {
            Ok(Files::Scope(crate::search_scopes::resolve_in(session, params.scope.as_deref(), "", true)?))
        }
        TodoTab::CurrentFile => {
            let workbench = session.state::<Workbench>();
            let path = match &params.path {
                Some(path) => Some(workbench.map_or_else(|| path.clone(), |workbench| workbench.spelled(path))),
                None => workbench.and_then(Workbench::active_document).map(|document| document.path().to_owned()),
            };
            Ok(match path {
                Some(path) => {
                    let relative = relative_to(&root, &path).unwrap_or_else(|| path.display().to_string());
                    Files::One(path, relative)
                }
                None => Files::Nothing(t!("workbench-todo-no-file")),
            })
        }
        TodoTab::CurrentChangelist => {
            if !in_git(session) {
                return Ok(Files::Nothing(t!("vcs-error-no-repository")));
            }
            let report = ListChangelists.run(session, ListParams { wait })?;
            let files: HashSet<String> = report
                .lists
                .into_iter()
                .filter(|list| list.active)
                .flat_map(|list| list.files)
                .filter(|change| change.status != "deleted")
                .filter_map(|change| relative_to(&root, &change.path))
                .collect();
            Ok(Files::Scope(ResolvedScope {
                filter: FileFilter::new(move |relative| files.contains(relative)),
                outside: Vec::new(),
            }))
        }
    }
}

fn open_texts(session: &Session) -> HashMap<PathBuf, String> {
    session
        .state::<Workbench>()
        .map(|workbench| {
            workbench
                .documents()
                .iter()
                .filter(|document| document.image().is_none())
                .map(|document| (document.path().to_owned(), document.text().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn highlighters(session: &Session) -> Option<HighlighterSource> {
    session.state::<Highlighters>().and_then(Highlighters::source)
}

fn one_file(session: &Session, path: &Path, relative: &str, limit: usize) -> Vec<TextMatch> {
    let open = session.state::<Workbench>().and_then(|workbench| {
        workbench.documents().iter().find(|document| document.image().is_none() && document.path() == path)
    });
    let text = match open {
        Some(document) => document.text().to_owned(),
        None => match zen_vfs::read_text(path) {
            Ok(text) => text,
            Err(_) => return Vec::new(),
        },
    };
    let ranges = file_ranges(session, path, &text);
    located(&text, &ranges[..ranges.len().min(limit)], path, relative)
}

fn one_file_text(session: &Session, path: &Path, text: &str) -> Vec<TextMatch> {
    let ranges = file_ranges(session, path, text);
    located(text, &ranges, path, &path.display().to_string())
}

fn file_ranges(session: &Session, path: &Path, text: &str) -> Vec<Range<usize>> {
    let patterns = TodoPatterns::of(session);
    let Ok(ranges) = zen_text::search::find_all(text, &patterns.query()) else { return Vec::new() };
    comments_only(highlighters(session), &patterns)(path, text, ranges)
}

fn located(text: &str, ranges: &[Range<usize>], path: &Path, relative: &str) -> Vec<TextMatch> {
    let mut found = Vec::with_capacity(ranges.len());
    let (mut line, mut line_start_char, mut line_start_byte) = (0, 0, 0);
    let mut chars = text.char_indices().enumerate().peekable();
    for range in ranges {
        while let Some(&(index, (byte, c))) = chars.peek() {
            if index >= range.start {
                break;
            }
            chars.next();
            if c == '\n' {
                line += 1;
                line_start_char = index + 1;
                line_start_byte = byte + 1;
            }
        }
        let rest = &text[line_start_byte..];
        let line_text = rest.split('\n').next().unwrap_or(rest).trim_end_matches('\r').to_owned();
        found.push(TextMatch {
            path: path.to_owned(),
            relative: relative.to_owned(),
            line,
            column: range.start - line_start_char,
            range: range.clone(),
            line_text,
        });
    }
    found
}

fn search(scope: &ResolvedScope, limit: usize, query: Query) -> Search {
    Search {
        query,
        mask: String::new(),
        directory: String::new(),
        limit,
        filter: Some(scope.filter.clone()),
        extra: scope.extra_files(),
    }
}

fn index(session: &Session) -> Result<zen_index::FileIndex, CommandError> {
    session
        .state::<Workbench>()
        .and_then(|workbench| workbench.index.clone())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn list(
    session: &Session,
    params: &TodoParams,
    matches: &[TextMatch],
    done: bool,
    scan: u64,
    error: Option<String>,
) -> TodoList {
    let compiled = TodoPatterns::of(session).compiled();
    let (files, total) = grouped(matches, &compiled, params.filter.as_deref());
    TodoList {
        tab: params.tab,
        scope: (params.tab == TodoTab::ScopeBased).then(|| params.scope.clone()).flatten(),
        files,
        total,
        done,
        scan,
        error,
    }
}

pub struct ListTodo;

impl Command for ListTodo {
    type Params = TodoParams;
    type Output = TodoList;
    const ID: &'static str = "todo.items.list";
    const TITLE: &'static str = "TODO Items";
    const DESCRIPTION: &'static str = "Lists the TODO and FIXME comments of a tab of the TODO tool window at once: the \
         project's files (`project`), a file (`currentFile`, the file in the editor by default), a search scope \
         (`scopeBased` with `scope` from search.scope.list) or the active changelist (`currentChangelist`). Only \
         comments count, by each file's language; excluded folders are left out and open files are read with their \
         unsaved changes. Each item has its line and column from 0, its offset for navigate.place.goTo and its text.";

    fn run(&self, session: &mut Session, params: TodoParams) -> Result<TodoList, CommandError> {
        let limit = params.limit.unwrap_or(LIMIT);
        match files_of(session, &params, true)? {
            Files::Nothing(reason) => Ok(list(session, &params, &[], true, 0, Some(reason))),
            Files::One(path, relative) => {
                let matches = one_file(session, &path, &relative, limit);
                Ok(list(session, &params, &matches, true, 0, None))
            }
            Files::Scope(scope) => {
                let index = index(session)?;
                let patterns = TodoPatterns::of(session);
                let keep = comments_only(highlighters(session), &patterns);
                let memory = memory(session);
                let found = Mutex::new(Vec::new());
                let sink = |matches: Vec<TextMatch>| {
                    if let Ok(mut all) = found.lock() {
                        all.extend(matches);
                    }
                };
                let result = index.search_text_remembering(
                    &search(&scope, limit, patterns.query()),
                    &open_texts(session),
                    &AtomicBool::new(false),
                    &sink,
                    &keep,
                    Some(&memory),
                );
                let matches = found.into_inner().unwrap_or_default();
                Ok(list(session, &params, &matches, true, 0, result.err()))
            }
        }
    }
}

pub struct TodoTabs;

impl Command for TodoTabs {
    type Params = NoParams;
    type Output = TodoTabList;
    const ID: &'static str = "todo.tabs.list";
    const TITLE: &'static str = "TODO Tabs";
    const DESCRIPTION: &'static str = "Lists the tabs of the TODO tool window (Project, Current File, Scope Based, and \
         Current Changelist when the project is in Git) and the patterns TODO items are found by.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<TodoTabList, CommandError> {
        project(session)?;
        let git = in_git(session);
        Ok(TodoTabList {
            tabs: TodoTab::ALL.into_iter().filter(|tab| git || *tab != TodoTab::CurrentChangelist).collect(),
            patterns: TodoPatterns::of(session).patterns(),
        })
    }
}

/// The number of a scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ScanStarted {
    /// The scan's number, which todo.scan.results answers with.
    pub scan: u64,
}

pub struct StartTodoScan;

impl Command for StartTodoScan {
    type Params = TodoParams;
    type Output = ScanStarted;
    const ID: &'static str = "todo.scan.start";
    const TITLE: &'static str = "Scan TODO Items";
    const DESCRIPTION: &'static str = "Starts listing the TODO items of a tab in the background, as the TODO tool window \
         does after edits and changes on disk; read them with todo.scan.results. A scan already running is cancelled.";

    fn run(&self, session: &mut Session, params: TodoParams) -> Result<ScanStarted, CommandError> {
        let limit = params.limit.unwrap_or(LIMIT);
        session.state_mut::<TodoState>().scan.cancel.store(true, Ordering::Relaxed);
        let files = files_of(session, &params, false);
        let state = session.state_mut::<TodoState>();
        let scan = TodoScan { id: state.scan.id + 1, params: params.clone(), ..TodoScan::default() };
        state.scan = scan.clone();
        let finish = |matches: Vec<TextMatch>, error: Option<String>| {
            if let Ok(mut all) = scan.matches.lock() {
                *all = matches;
            }
            if let Ok(mut slot) = scan.error.lock() {
                *slot = error;
            }
            scan.done.store(true, Ordering::Relaxed);
        };
        match files {
            Err(error) => finish(Vec::new(), Some(error.to_string())),
            Ok(Files::Nothing(reason)) => finish(Vec::new(), Some(reason)),
            Ok(Files::One(path, relative)) => finish(one_file(session, &path, &relative, limit), None),
            Ok(Files::Scope(scope)) => {
                let index = match index(session) {
                    Ok(index) => index,
                    Err(error) => {
                        finish(Vec::new(), Some(error.to_string()));
                        return Ok(ScanStarted { scan: scan.id });
                    }
                };
                let patterns = TodoPatterns::of(session);
                let keep = comments_only(highlighters(session), &patterns);
                let query = patterns.query();
                let memory = memory(session);
                let open = open_texts(session);
                let running = scan.clone();
                let run = move || {
                    let sink = |matches: Vec<TextMatch>| {
                        if let Ok(mut all) = running.matches.lock() {
                            all.extend(matches);
                        }
                    };
                    if let Err(error) = index.search_text_remembering(
                        &search(&scope, limit, query),
                        &open,
                        &running.cancel,
                        &sink,
                        &keep,
                        Some(&memory),
                    ) && let Ok(mut slot) = running.error.lock()
                    {
                        *slot = Some(error);
                    }
                    running.done.store(true, Ordering::Relaxed);
                };
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::spawn(run);
                #[cfg(target_arch = "wasm32")]
                run();
            }
        }
        Ok(ScanStarted { scan: scan.id })
    }
}

pub struct TodoScanResults;

impl Command for TodoScanResults {
    type Params = NoParams;
    type Output = TodoList;
    const ID: &'static str = "todo.scan.results";
    const TITLE: &'static str = "TODO Scan Results";
    const DESCRIPTION: &'static str = "Returns the TODO items found so far by the scan started last (todo.scan.start), \
         with its tab, its number and whether it is done.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<TodoList, CommandError> {
        let scan = session.state::<TodoState>().map(|state| state.scan.clone()).unwrap_or_default();
        let matches = scan.matches.lock().map(|matches| matches.clone()).unwrap_or_default();
        let error = scan.error.lock().ok().and_then(|error| error.clone());
        Ok(list(session, &scan.params, &matches, scan.id == 0 || scan.done.load(Ordering::Relaxed), scan.id, error))
    }
}

/// The file whose TODO text to mark.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DocumentTodoParams {
    /// An open file; the active one when absent.
    pub path: Option<PathBuf>,
}

/// One stretch of TODO text in a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoRange {
    /// Its first character, from 0.
    pub start: usize,
    /// The character after its last one.
    pub end: usize,
    /// The pattern's own colour as `#rrggbb`; the colour scheme's TODO colour when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

pub struct DocumentTodos;

impl Command for DocumentTodos {
    type Params = DocumentTodoParams;
    type Output = Vec<TodoRange>;
    const ID: &'static str = "todo.document.ranges";
    const TITLE: &'static str = "TODO Text of a File";
    const DESCRIPTION: &'static str = "The stretches of TODO text of an open file (the active one by default), with its \
         unsaved changes, as the editor highlights them and marks them on the scroll bar: their character offsets \
         and the colour of the pattern, when it has its own.";

    fn run(&self, session: &mut Session, params: DocumentTodoParams) -> Result<Vec<TodoRange>, CommandError> {
        let workbench =
            session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let document = match &params.path {
            Some(path) => workbench.documents().iter().find(|document| document.path() == path),
            None => workbench.active_document(),
        };
        let document = document.ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let (path, text) = (document.path().to_owned(), document.text().to_owned());
        let root = workbench.project().map(Path::to_owned).unwrap_or_default();
        crate::hidden_files::guard_change(session, &root, std::slice::from_ref(&path))?;
        let compiled = TodoPatterns::of(session).compiled();
        let mut color = None;
        let chars: Vec<char> = text.chars().collect();
        let ranges = file_ranges(session, &path, &text);
        Ok(ranges
            .into_iter()
            .map(|range| {
                let shown: String = chars[range.clone()].iter().collect();
                if let Some(pattern) = compiled.pattern_at_start(&shown) {
                    color.clone_from(&pattern.color);
                }
                TodoRange { start: range.start, end: range.end, color: color.clone() }
            })
            .collect())
    }
}

/// What to check before a commit.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CommitCheckParams {
    /// The files of the commit.
    pub paths: Vec<PathBuf>,
    /// A filter of Settings → Editor → TODO; the one Check TODO is set to when absent.
    pub filter: Option<String>,
}

/// One TODO found in the changed lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommitTodo {
    /// The file.
    pub path: PathBuf,
    /// The line, from 0.
    pub line: usize,
    /// The TODO's text.
    pub text: String,
}

pub struct CommitCheck;

impl Command for CommitCheck {
    type Params = CommitCheckParams;
    type Output = Vec<CommitTodo>;
    const ID: &'static str = "todo.commit.check";
    const TITLE: &'static str = "Check TODO";
    const DESCRIPTION: &'static str = "Finds the TODO items in the lines a commit of the given files changes against HEAD, \
         through a filter; a new file counts as changed everywhere. The Commit tool window asks this before committing \
         while Check TODO is on.";

    fn run(&self, session: &mut Session, params: CommitCheckParams) -> Result<Vec<CommitTodo>, CommandError> {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        crate::hidden_files::guard_change(session, &root, &params.paths)?;
        let patterns = TodoPatterns::of(session);
        let filter = params.filter.or_else(|| patterns.settings().commit_filter.clone());
        let compiled = patterns.compiled();
        let mut found = Vec::new();
        for path in &params.paths {
            let open = session.state::<Workbench>().and_then(|workbench| {
                workbench
                    .documents()
                    .iter()
                    .find(|document| document.path() == path)
                    .map(|document| document.text().to_owned())
            });
            let Some(text) = open.or_else(|| zen_vfs::read_text(path).ok()) else { continue };
            let changed = crate::vcs::commit::changed_lines(session, path, &text);
            let matches = one_file_text(session, path, &text);
            let (files, _) = grouped(&matches, &compiled, filter.as_deref());
            let items = files.into_iter().flat_map(|file| file.items);
            found.extend(
                items
                    .filter(|item| {
                        changed.as_ref().is_none_or(|lines| lines.iter().any(|range| range.contains(&item.line)))
                    })
                    .map(|item| CommitTodo { path: path.clone(), line: item.line, text: item.text }),
            );
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::places::{Place, TextCache, places};
use super::show_usages::{DeclarationKind, UsagesScope};
use super::{
    Languages, LookupKind, LookupParams, Reason, WaitParams, WorkProgress, byte_offset, file_text, with_languages,
};
use crate::Workbench;
use crate::find_results::{FindUsagesOptions, UsagesTarget};

/// The parameters of `navigate.usages.find`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindUsagesParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in the file; `line` and `column` instead, else the primary caret, when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// The line, one-based as answers count them: with `column`, the place instead of `offset`.
    #[serde(default)]
    pub line: Option<u32>,
    /// The column on `line`, one-based, in characters; 1 when absent.
    #[serde(default)]
    pub column: Option<u32>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// Whether the declaration is listed among the usages; it is not by default.
    #[serde(default)]
    pub include_declaration: Option<bool>,
    /// Where to look; the Find Usages options' scope (Project Files) when absent.
    #[serde(default)]
    pub scope: Option<UsagesScope>,
    /// Whether the usages open on a tab of their own; the Find Usages options' Open in new tab when absent.
    #[serde(default)]
    pub new_tab: Option<bool>,
    /// A declaration by name instead of a place, as navigate.symbol.goTo takes it (`Workbench`,
    /// `Store.get`, `model.Store.get`): its best match whose name is the last part, waiting up to
    /// `waitMs` for the language servers to find it.
    #[serde(default)]
    pub symbol: Option<String>,
    /// The name at the place instead of a column: its first occurrence as a whole word on `line`
    /// (in the whole file without `line`).
    #[serde(default)]
    pub name: Option<String>,
    /// Whether to add `textMatches`: the places where the name occurs as a whole word that the
    /// server does not count as usages (comments, strings, macros, other languages), as rows
    /// `path:line:column text`; once the usages have come.
    #[serde(default)]
    pub include_text_matches: Option<bool>,
    /// Whether the usages are `rows`, `path:line:column text` each, instead of `files` with offsets.
    #[serde(default)]
    pub compact: Option<bool>,
}

impl FindUsagesParams {
    pub fn lookup(&self) -> LookupParams {
        LookupParams {
            path: self.path.clone(),
            offset: self.offset,
            line: self.line,
            column: self.column,
            wait_ms: self.wait_ms,
        }
    }
}

/// The usages in one file. Agents get each path once (card ZneOu428): the file's `path` relative
/// to the project (absolute outside it), and its usages without a path of their own.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsagesFile {
    /// The file.
    #[serde(skip)]
    pub path: PathBuf,
    /// The file's path relative to the project, with `/`, or the whole path outside it.
    #[serde(rename = "path")]
    pub relative: String,
    /// Its usages, in order.
    #[serde(serialize_with = "in_file")]
    #[schemars(with = "Vec<PlaceInFile>")]
    pub usages: Vec<Place>,
}

/// A usage as [`UsagesFile`] lists it: a [`Place`] without the file, which its group names.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlaceInFile {
    /// The first character of the range, a character offset in the file's text.
    pub start: usize,
    /// The character after the range.
    pub end: usize,
    /// The line the range starts on, one-based.
    pub line: u32,
    /// The column the range starts at, one-based, in characters.
    pub column: u32,
    /// The line's text without its leading and trailing white space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// The range within the preview, in characters, cut at the line's end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<[usize; 2]>,
}

fn in_file<S: serde::Serializer>(places: &[Place], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(places.iter().map(|place| PlaceInFile {
        start: place.start,
        end: place.end,
        line: place.line,
        column: place.column,
        preview: place.preview.clone(),
        highlight: place.highlight,
    }))
}

/// The usages found so far.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsagesState {
    /// The number of the search, raised by each Find Usages; 0 before the first.
    pub search: u64,
    /// The symbol searched for, as written at its place, or the text of a search's occurrences.
    pub symbol: Option<String>,
    /// Whether these are the occurrences of the search bar's query (Open Results in Find Window)
    /// rather than a symbol's usages.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub occurrences: bool,
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// The files with usages, in path order.
    pub files: Vec<UsagesFile>,
    /// How many usages there are in all.
    pub total: usize,
    /// Why nothing was found, in words for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// While `pending`, why the answer has not come (`starting`, `indexing`: the server reports
    /// work under way, `searching`); `noMatch` when the server found none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// The work the server reports under way while `reason` is `indexing`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<WorkProgress>,
    /// The tab of the Find tool window that lists the usages (search.results.tree takes it), once
    /// the server answered; none when Skip results tab with one usage opened the one usage instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u64>,
    /// With `compact`: the usages as rows, `path:line:column text`, and `files` empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<String>,
    /// With `includeTextMatches`: where the name occurs as a whole word but the server sees no
    /// usage, as rows `path:line:column text` (at most 200).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub text_matches: Vec<String>,
}

pub struct FindUsages;

impl Command for FindUsages {
    type Params = FindUsagesParams;
    type Output = UsagesState;
    const ID: &'static str = "navigate.usages.find";
    const TITLE: &'static str = "Find Usages";
    const DESCRIPTION: &'static str = "Asks the language server for the usages of the symbol at a place (the caret by default) \
         in a scope (the Find Usages options', Project Files) and lists them on a tab \"Usages of …\" of the Find \
         tool window, in place of the last tab that is not pinned unless `newTab`; with Skip results tab with one \
         usage a single usage opens in the editor instead. The place is `path` with `offset`, `line` and `column`, or `line` and `name`; or `symbol`, a declaration by name (`Store.get`). With `waitMs` the usages come in the answer; `compact` makes them rows `path:line:column text`, and `includeTextMatches` adds `textMatches`, the whole-word occurrences the server does not count (comments, strings). The answer's `tab` is the tab once the server answered \
         (search.results.tree reads it, grouped as the text search's tabs are); navigate.usages.list waits. When the file's \
         server is not there, the error is {error: serverMissing|serverNotRunning|starting, server, install, message}.";

    fn is_available(&self, session: &Session) -> bool {
        !session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .is_some_and(|document| document.image().is_some())
    }

    fn run(&self, session: &mut Session, params: FindUsagesParams) -> Result<UsagesState, CommandError> {
        let lookup = params.lookup();
        #[cfg(not(target_arch = "wasm32"))]
        if params.symbol.is_none()
            && params.name.is_none()
            && let Ok((path, offset)) = lookup.place(session.state_mut::<Workbench>())
            && let Some(found) =
                crate::database::usages::find_at(session, &path, offset, params.new_tab, params.wait_ms)
        {
            let found = found?;
            return Ok(UsagesState {
                symbol: Some(found.label),
                total: found.total,
                tab: Some(found.tab),
                rows: found
                    .usages
                    .iter()
                    .filter(|_| params.compact == Some(true))
                    .map(|row| {
                        let place = row.path.clone().or_else(|| row.holder.clone()).unwrap_or_default();
                        let line = u32::try_from(row.line).unwrap_or(u32::MAX);
                        let column = u32::try_from(row.column).unwrap_or(u32::MAX);
                        super::compact::row(&place, line, column, &row.text)
                    })
                    .collect(),
                ..UsagesState::default()
            });
        }
        if params.symbol.is_none()
            && params.name.is_none()
            && let Ok((path, offset)) = lookup.place(session.state_mut::<Workbench>())
            && let Some(found) = crate::project_index::find_at(session, &path, offset, params.new_tab, params.wait_ms)
        {
            let found = found?;
            return Ok(UsagesState {
                symbol: Some(found.label),
                total: found.total,
                tab: Some(found.tab),
                rows: found
                    .usages
                    .iter()
                    .filter(|_| params.compact == Some(true))
                    .map(|row| {
                        let place = row.path.clone().unwrap_or_default();
                        let line = u32::try_from(row.line).unwrap_or(u32::MAX);
                        let column = u32::try_from(row.column).unwrap_or(u32::MAX);
                        super::compact::row(&place, line, column, &row.text)
                    })
                    .collect(),
                ..UsagesState::default()
            });
        }
        let (path, offset) = match (&params.symbol, &params.name) {
            (Some(symbol), _) => declaration_named(session, symbol, lookup.wait())?,
            (None, Some(name)) => name_place(session, &lookup, name)?,
            (None, None) => lookup.place(session.state_mut::<Workbench>())?,
        };
        let options = FindUsagesOptions::of(session);
        let search = start_lookup(session, &path, offset, params.include_declaration.unwrap_or(false))?;
        let symbol = session.state::<UsagesState>().and_then(|state| state.symbol.clone()).unwrap_or_default();
        let (line, column) = file_text(session.state_mut::<Workbench>(), &path)
            .map(|text| crate::find_results::usages::line_context(&text, offset))
            .unwrap_or_default();
        let target = UsagesTarget {
            path: path.clone(),
            offset,
            symbol,
            kind: DeclarationKind::Symbol,
            scope: params.scope.unwrap_or(options.scope),
            line,
            column,
        };
        let new_tab = params.new_tab.unwrap_or(options.new_tab);
        crate::find_results::usages::begin(session, search, target, new_tab, options.skip_single);
        let mut state = poll(session, lookup.wait());
        if params.include_text_matches == Some(true) && !state.pending {
            state.text_matches = text_matches(session, &state, &path, offset);
        }
        if params.compact == Some(true) {
            state.rows = std::mem::take(&mut state.files)
                .iter()
                .flat_map(|file| {
                    file.usages.iter().map(|usage| {
                        super::compact::row(
                            &file.relative,
                            usage.line,
                            usage.column,
                            usage.preview.as_deref().unwrap_or(""),
                        )
                    })
                })
                .collect();
        }
        Ok(state)
    }
}

const MAX_TEXT_MATCHES: usize = 200;

fn declaration_named(session: &mut Session, symbol: &str, wait: Duration) -> Result<(PathBuf, usize), CommandError> {
    let query = symbol.trim();
    let last = query.rsplit(['.', ':']).next().unwrap_or(query);
    let found = with_languages(session, |languages, workbench| {
        let searched = languages.search_waiting(workbench, query, wait.max(Duration::from_millis(1)), false);
        let matched = languages.matched_symbols(workbench, query, |_| true, 50);
        let best = matched.iter().find(|found| found.name == last).or_else(|| matched.first());
        best.map(|found| (found.path.clone(), found.offset)).ok_or(searched.pending)
    });
    found.map_err(|pending| {
        let code = if pending { "pending" } else { "noMatch" };
        CommandError::with_code(code, t!("workbench-usages-no-declaration", symbol = query))
            .with_data(json!({ "symbol": query }))
    })
}

fn name_place(session: &mut Session, lookup: &LookupParams, name: &str) -> Result<(PathBuf, usize), CommandError> {
    let workbench = session.state_mut::<Workbench>();
    let path = lookup
        .path
        .as_deref()
        .map(|path| workbench.resolve(path))
        .or_else(|| workbench.active_document().map(|document| document.path().to_owned()))
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let text = file_text(workbench, &path).map_err(CommandError::new)?;
    let line_start = match lookup.line {
        Some(line) => text.split('\n').take(line.saturating_sub(1) as usize).map(|line| line.chars().count() + 1).sum(),
        None => 0,
    };
    let chars: Vec<char> = text.chars().collect();
    let wanted: Vec<char> = name.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut at = line_start;
    while at + wanted.len() <= chars.len() {
        if lookup.line.is_some() && chars[at] == '\n' {
            break;
        }
        let before = at.checked_sub(1).map(|before| chars[before]);
        let after = chars.get(at + wanted.len()).copied();
        if chars[at..at + wanted.len()] == wanted[..] && !before.is_some_and(word) && !after.is_some_and(word) {
            return Ok((path, at));
        }
        at += 1;
    }
    let relative = super::project_relative(&path, workbench.project());
    Err(CommandError::with_code("not_found", t!("workbench-usages-no-name", name = name, path = relative.as_str()))
        .with_data(json!({ "name": name, "path": relative })))
}

fn text_matches(session: &mut Session, state: &UsagesState, path: &Path, offset: usize) -> Vec<String> {
    let Some(word) = state.symbol.clone().filter(|word| !word.is_empty()) else { return Vec::new() };
    let params = crate::search::SearchParams {
        query: zen_text::search::Query {
            text: word,
            match_case: true,
            whole_words: true,
            ..zen_text::search::Query::default()
        },
        limit: Some(2000),
        ..crate::search::SearchParams::default()
    };
    let Ok(found) = Command::run(&crate::search::FindInFiles, session, params) else { return Vec::new() };
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let here = super::project_relative(path, root.as_deref());
    let known: std::collections::BTreeSet<(String, usize)> = state
        .files
        .iter()
        .flat_map(|file| file.usages.iter().map(|usage| (file.relative.clone(), usage.start)))
        .chain(std::iter::once((here, offset)))
        .collect();
    found
        .matches
        .iter()
        .flatten()
        .filter(|found| {
            let start = found.offset;
            !known.contains(&(found.path.clone(), start))
                && !(known.iter().any(|(file, at)| *file == found.path && (start..start + found.length).contains(at)))
        })
        .take(MAX_TEXT_MATCHES)
        .map(|found| super::compact::row(&found.path, found.line as u32, found.column as u32, found.text.trim()))
        .collect()
}

pub(crate) fn start_lookup(
    session: &mut Session,
    path: &Path,
    offset: usize,
    include_declaration: bool,
) -> Result<u64, CommandError> {
    let symbol = {
        let text = file_text(session.state_mut::<Workbench>(), path).map_err(CommandError::new)?;
        word_at(&text, offset).ok_or_else(|| CommandError::new(t!("workbench-nav-no-symbol")))?
    };
    let context = json!({ "context": { "includeDeclaration": include_declaration } });
    with_languages(session, |languages, workbench| {
        let started = languages
            .start_lookup(workbench, LookupKind::Usages, path, offset, "textDocument/references", context)
            .map(|_| ());
        started.map_err(|message| languages.lookup_error(path, message))
    })?;
    let state = session.state_mut::<UsagesState>();
    *state = UsagesState { search: state.search + 1, symbol: Some(symbol), pending: true, ..UsagesState::default() };
    Ok(state.search)
}

pub(crate) fn poll_usages(session: &mut Session, wait: Duration) {
    let pending = session.state::<Languages>().is_some_and(|languages| languages.lookup(LookupKind::Usages).is_some());
    if pending {
        let _ = poll(session, wait);
    } else {
        crate::find_results::usages::poll_access(session);
    }
}

pub struct ListUsages;

impl Command for ListUsages {
    type Params = WaitParams;
    type Output = UsagesState;
    const ID: &'static str = "navigate.usages.list";
    const TITLE: &'static str = "Usages";
    const DESCRIPTION: &'static str =
        "The usages of the last Find Usages, grouped by file, waiting up to waitMs for the server's answer.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<UsagesState, CommandError> {
        Ok(poll(session, params.wait()))
    }
}

pub struct ClearUsages;

impl Command for ClearUsages {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "navigate.usages.clear";
    const TITLE: &'static str = "Clear Usages";
    const DESCRIPTION: &'static str =
        "Forgets the usages of the last Find Usages, cancelling the search if it still runs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        session.state_mut::<Languages>().end_lookup(LookupKind::Usages);
        crate::find_results::usages::forget(session);
        let state = session.state_mut::<UsagesState>();
        *state = UsagesState { search: state.search, ..UsagesState::default() };
        Ok(())
    }
}

fn poll(session: &mut Session, wait: Duration) -> UsagesState {
    let until = std::time::Instant::now() + wait;
    crate::find_results::usages::poll_access(session);
    if crate::find_results::usages::cancelled(session) {
        session.state_mut::<Languages>().end_lookup(LookupKind::Usages);
        crate::find_results::usages::forget(session);
        let state = session.state_mut::<UsagesState>();
        state.pending = false;
        state.error = Some(t!("workbench-usages-task-stopped"));
        return state.clone();
    }
    let languages = session.state_mut::<Languages>();
    let answer = if languages.lookup(LookupKind::Usages).is_none() {
        None
    } else if languages.is_outdated(LookupKind::Usages) {
        languages.end_lookup(LookupKind::Usages);
        Some(Err(t!("workbench-nav-usages-outdated")))
    } else if languages.lookup_mut(LookupKind::Usages).is_some_and(|lookup| lookup.poll(wait)) {
        let again = with_languages(session, |languages, workbench| languages.ask_again(workbench, LookupKind::Usages));
        let languages = session.state_mut::<Languages>();
        if again {
            let left = until.saturating_duration_since(std::time::Instant::now());
            if let Some(lookup) = languages.lookup_mut(LookupKind::Usages) {
                lookup.poll(left);
            }
        }
        match languages.lookup(LookupKind::Usages) {
            Some(lookup) if lookup.is_pending() => None,
            _ => languages.end_lookup(LookupKind::Usages).and_then(|lookup| lookup.answer),
        }
    } else {
        None
    };
    if let Some(answer) = answer {
        let found = answer.map(|answer| {
            with_languages(session, |languages, workbench| {
                group(places(&answer, languages, &mut TextCache::new()), workbench.project())
            })
        });
        let state = session.state_mut::<UsagesState>();
        state.pending = false;
        let search = state.search;
        let answered = found.clone();
        match found {
            Ok(files) if !files.is_empty() => {
                state.total = files.iter().map(|file| file.usages.len()).sum();
                state.files = files;
            }
            Ok(_) => {
                state.error = Some(t!("workbench-nav-no-usages"));
                state.reason = Some(Reason::NoMatch);
            }
            Err(error) => state.error = Some(error),
        }
        crate::find_results::usages::answered(session, search, answered);
    }
    let languages = session.state::<Languages>();
    let pending = languages.is_some_and(|languages| languages.lookup(LookupKind::Usages).is_some());
    let busy = languages.and_then(Languages::busy).filter(|_| pending);
    if !pending && crate::find_results::usages::waiting(session).is_some() {
        crate::find_results::usages::forget(session);
    }
    let state = session.state_mut::<UsagesState>();
    state.pending = pending;
    if pending {
        state.reason = Some(busy.as_ref().map_or(Reason::Searching, |(reason, _)| *reason));
        state.progress = busy.and_then(|(_, progress)| progress);
    } else if state.reason != Some(Reason::NoMatch) {
        state.reason = None;
        state.progress = None;
    }
    let tab = state.tab.filter(|_| !pending);
    if let Some(tab) = tab {
        crate::find_results::usages::settle(session, tab, until);
    }
    session.state_mut::<UsagesState>().clone()
}

pub fn group(found: Vec<Place>, project: Option<&Path>) -> Vec<UsagesFile> {
    let mut files: BTreeMap<(bool, String), UsagesFile> = BTreeMap::new();
    for place in found {
        let inside = project.and_then(|root| place.path.strip_prefix(root).ok());
        let relative = match inside {
            Some(inside) => {
                inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
            }
            None => place.path.display().to_string(),
        };
        let file = files.entry((inside.is_none(), relative.clone())).or_insert_with(|| UsagesFile {
            path: place.path.clone(),
            relative,
            usages: Vec::new(),
        });
        file.usages.push(place);
    }
    files
        .into_values()
        .map(|mut file| {
            file.usages.sort_by_key(|place| (place.start, place.end));
            file.usages.dedup_by_key(|place| (place.start, place.end));
            file
        })
        .collect()
}

pub fn word_at(text: &str, offset: usize) -> Option<String> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let at = byte_offset(text, offset);
    let start = text[..at].char_indices().rev().take_while(|&(_, c)| is_word(c)).last().map_or(at, |(index, _)| index);
    let end = text[at..].char_indices().find(|&(_, c)| !is_word(c)).map_or(text.len(), |(index, _)| at + index);
    (start < end).then(|| text[start..end].to_owned())
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(FindUsages)?;
    registry.register(ListUsages)?;
    registry.register(ClearUsages)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use zen_lsp::fake::Reply;

    use crate::language::testing::{Fixture, fixture};

    const MAIN: &str = "package main\n\n// Привет, мир\nfunc main() {\n\tПривет(\"ё😀\")\n\tПривет(\"x\")\n}\n";
    const LIB: &str = "package main\n\nfunc Привет(s string) {}\n";
    const UTIL: &str = "package util\n\n  /* 😀 */ var _ = main.Привет\n";

    fn location(fixture: &Fixture, name: &str, text: &str, needle: &str, nth: usize) -> Value {
        let at = text.match_indices(needle).nth(nth).unwrap().0;
        let start = zen_lsp::position::position(text, at);
        let end = zen_lsp::position::position(text, at + needle.len());
        json!({ "uri": zen_lsp::position::file_uri(&fixture.path(name)), "range": { "start": start, "end": end } })
    }

    fn chars(text: &str, needle: &str, nth: usize) -> usize {
        text[..text.match_indices(needle).nth(nth).unwrap().0].chars().count()
    }

    fn project() -> Fixture {
        fixture(&[("main.go", MAIN), ("lib.go", LIB), ("util/util.go", UTIL)])
    }

    #[test]
    fn a_search_the_server_dropped_for_a_change_is_asked_again() {
        let mut fixture = project();
        let answer = json!([location(&fixture, "lib.go", LIB, "Привет", 0)]);
        fixture.server.reply(
            "textDocument/references",
            Reply::ErrorOnce { code: zen_lsp::connection::CONTENT_MODIFIED, then: answer },
        );
        fixture.open("main.go");
        let at = chars(MAIN, "Привет", 1) + 2;
        let state = fixture.dispatch("navigate.usages.find", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(fixture.server.requests("textDocument/references").len(), 2, "{state}");
        assert_eq!(state["total"], 1, "content modified is asked again, not answered: {state}");
        assert_eq!(state["error"], Value::Null);
    }

    #[test]
    fn usages_across_three_files_are_grouped_sorted_and_previewed() {
        let mut fixture = project();
        let answer = json!([
            location(&fixture, "util/util.go", UTIL, "Привет", 0),
            location(&fixture, "main.go", MAIN, "Привет", 2),
            location(&fixture, "lib.go", LIB, "Привет", 0),
            location(&fixture, "main.go", MAIN, "Привет", 1),
            location(&fixture, "main.go", MAIN, "Привет", 1),
        ]);
        fixture.server.on("textDocument/references", answer);
        fixture.open("lib.go");
        fixture.open("main.go");
        let at = chars(MAIN, "Привет", 1) + 2;
        let state = fixture.dispatch("navigate.usages.find", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        let sent = &fixture.server.requests("textDocument/references")[0];
        assert_eq!(sent["context"], json!({ "includeDeclaration": false }));
        assert_eq!(sent["position"]["line"], 4);
        assert_eq!(state["search"], 1);
        assert_eq!(state["symbol"], "Привет");
        assert_eq!(state["pending"], false);
        assert_eq!(state["total"], 4, "the duplicate is listed once: {state}");
        let files = state["files"].as_array().unwrap();
        let paths: Vec<&str> = files.iter().map(|file| file["path"].as_str().unwrap()).collect();
        assert_eq!(paths, ["lib.go", "main.go", "util/util.go"], "each file once, relative to the project");
        assert!(files.iter().all(|file| file.get("relative").is_none()), "{state}");
        let main = &files[1]["usages"];
        assert_eq!(main.as_array().unwrap().len(), 2);
        assert_eq!(main[0]["line"], 5);
        assert_eq!(main[0]["start"], chars(MAIN, "Привет", 1));
        assert_eq!(main[0]["end"], chars(MAIN, "Привет", 1) + 6);
        assert_eq!(main[0]["preview"], "Привет(\"ё😀\")");
        assert_eq!(main[0]["highlight"], json!([0, 6]));
        assert_eq!(main[1]["line"], 6);
        let util = &files[2]["usages"][0];
        assert!(util.get("path").is_none(), "a usage does not repeat its file: {util}");
        assert_eq!(util["preview"], "/* 😀 */ var _ = main.Привет", "read from disk: util.go is not open");
        assert_eq!(util["highlight"], json!([21, 27]));
        assert_eq!(util["column"], 24);
        assert_eq!(files[0]["usages"][0]["preview"], "func Привет(s string) {}");
        assert_eq!(files[0]["usages"][0]["highlight"], json!([5, 11]));

        let listed = fixture.dispatch("navigate.usages.list", json!({})).unwrap();
        assert_eq!(listed, state);
        fixture.dispatch("navigate.usages.clear", json!({})).unwrap();
        let cleared = fixture.dispatch("navigate.usages.list", json!({})).unwrap();
        assert_eq!(
            (cleared["search"].clone(), cleared["total"].clone(), cleared["files"].clone()),
            (json!(1), json!(0), json!([]))
        );
    }

    #[test]
    fn a_place_is_taken_as_a_line_and_column_too() {
        let mut fixture = project();
        fixture.server.on("textDocument/references", json!([location(&fixture, "lib.go", LIB, "Привет", 0)]));
        fixture.open("main.go");
        let at = json!({ "path": "main.go", "line": 6, "column": 4, "waitMs": 5000 });
        let state = fixture.dispatch("navigate.usages.find", at).unwrap();
        assert_eq!(state["symbol"], "Привет", "{state}");
        let sent = &fixture.server.requests("textDocument/references")[0];
        assert_eq!(sent["position"], json!({ "line": 5, "character": 3 }));
        let line = crate::language::line_column_offset;
        assert_eq!(line(MAIN, 6, 99), line(MAIN, 6, 13));
        assert_eq!(line(MAIN, 6, 1), Some(chars(MAIN, "\tПривет(\"x\")", 0)));
        assert_eq!(line(MAIN, 6, 0), line(MAIN, 6, 1));
        assert_eq!(line("a\r\nbc\r\n", 2, 9), Some(5), "a line's end is before its \\r");
        assert_eq!(line(MAIN, 0, 1), None);
        let past = fixture.dispatch("navigate.usages.find", json!({ "path": "main.go", "line": 40 }));
        let message = format!("{past:?}");
        assert!(message.contains("main.go ") && message.contains(" 40") && message.contains(" 8"), "{message}");
        let shown = fixture
            .dispatch("navigate.usages.show", json!({ "path": "main.go", "line": 5, "column": 2, "waitMs": 5000 }));
        assert!(shown.is_ok(), "{shown:?}");
        let sent = fixture.server.requests("textDocument/references");
        assert_eq!(sent.last().unwrap()["position"], json!({ "line": 4, "character": 1 }));
    }

    #[test]
    fn include_declaration_is_passed_through_and_nothing_found_is_an_error() {
        let mut fixture = project();
        fixture.server.on("textDocument/references", json!([]));
        fixture.open("main.go");
        let at = chars(MAIN, "Привет", 1);
        let state = fixture
            .dispatch("navigate.usages.find", json!({ "offset": at, "waitMs": 5000, "includeDeclaration": true }))
            .unwrap();
        assert_eq!(fixture.server.requests("textDocument/references")[0]["context"]["includeDeclaration"], true);
        assert_eq!(state["pending"], false);
        assert_eq!(state["total"], 0);
        assert_eq!(state["error"], zen_i18n::t!("workbench-nav-no-usages"));

        fixture.server.on("textDocument/references", json!(null));
        let state = fixture.dispatch("navigate.usages.find", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(
            (state["search"].clone(), state["error"].clone()),
            (json!(2), json!(zen_i18n::t!("workbench-nav-no-usages")))
        );

        let blank = chars(MAIN, "(\"x", 0) + 1;
        assert!(
            fixture.dispatch("navigate.usages.find", json!({ "offset": blank })).is_err(),
            "no symbol in a string's quote"
        );
    }

    #[test]
    fn a_replaced_search_and_an_outdated_answer_are_dropped() {
        let mut fixture = project();
        let first = json!([location(&fixture, "lib.go", LIB, "Привет", 0)]);
        fixture.server.reply("textDocument/references", Reply::Delayed(first, Duration::from_millis(300)));
        fixture.open("main.go");
        let at = chars(MAIN, "Привет", 1);
        let asked = fixture.server.requests("textDocument/references").len();
        let state = fixture.dispatch("navigate.usages.find", json!({ "offset": at })).unwrap();
        assert_eq!((state["pending"].clone(), state["search"].clone()), (json!(true), json!(1)));
        fixture.server.wait_for("textDocument/references", asked + 1, Duration::from_secs(5));
        fixture.server.on("textDocument/references", json!([location(&fixture, "main.go", MAIN, "Привет", 2)]));
        let state = fixture.dispatch("navigate.usages.find", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(state["search"], 2);
        std::thread::sleep(Duration::from_millis(400));
        let state = fixture.dispatch("navigate.usages.list", json!({ "waitMs": 100 })).unwrap();
        assert_eq!(state["total"], 1);
        assert_eq!(state["files"][0]["path"], "main.go", "the first answer does not land: {state}");

        fixture.server.reply("textDocument/references", Reply::Hold);
        let asked = fixture.server.requests("textDocument/references").len();
        let state = fixture.dispatch("navigate.usages.find", json!({ "offset": at })).unwrap();
        assert_eq!(state["pending"], true);
        fixture.server.wait_for("textDocument/references", asked + 1, Duration::from_secs(5));
        fixture.dispatch("editor.edit.insert", json!({ "text": "x" })).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let state = fixture.dispatch("navigate.usages.list", json!({})).unwrap();
        assert_eq!(state["pending"], false);
        assert_eq!(state["error"], zen_i18n::t!("workbench-nav-usages-outdated"));
        let cancelled = fixture.server.wait_for("$/cancelRequest", 2, Duration::from_secs(5));
        assert_eq!(cancelled.len(), 2, "the replaced search and the outdated one are cancelled");
    }

    #[test]
    fn usages_of_a_closed_file_are_found_without_opening_it() {
        let mut fixture = project();
        let answer =
            json!([location(&fixture, "main.go", MAIN, "Привет", 1), location(&fixture, "lib.go", LIB, "Привет", 0)]);
        fixture.server.on("textDocument/references", answer);
        let main = fixture.path("main.go");
        let at = chars(MAIN, "Привет", 1) + 3;
        let state = fixture
            .dispatch_once_running("navigate.usages.find", json!({ "path": main, "offset": at, "waitMs": 5000 }))
            .unwrap();
        assert_eq!(state["symbol"], "Привет");
        assert_eq!(state["total"], 2, "{state}");
        assert_eq!(state["files"][1]["usages"][0]["preview"], "Привет(\"ё😀\")");
        let opened = fixture.server.requests("textDocument/didOpen");
        let uris: Vec<&Value> = opened.iter().map(|open| &open["textDocument"]["uri"]).collect();
        let lib = zen_lsp::position::file_uri(&fixture.path("lib.go"));
        assert_eq!(uris, [&json!(zen_lsp::position::file_uri(&main)), &json!(lib)], "each lent once: {opened:?}");
        assert_eq!(opened[0]["textDocument"]["text"], MAIN, "the text on disk is sent");
        assert_eq!(fixture.server.requests("textDocument/references")[0]["position"]["line"], 4);
        assert!(fixture.open_files().is_empty(), "no editor tab is opened");

        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let closed = fixture.server.wait_for("textDocument/didClose", 2, Duration::from_secs(5));
        let closed: Vec<&Value> = closed.iter().map(|close| &close["textDocument"]["uri"]).collect();
        assert!(closed.contains(&&json!(zen_lsp::position::file_uri(&main))), "closed once answered: {closed:?}");
        assert!(closed.contains(&&json!(lib)), "and the file lent to tell reads from writes: {closed:?}");
    }

    #[test]
    fn a_missing_file_is_a_clear_error() {
        let mut fixture = project();
        fixture.open("lib.go");
        let missing = fixture.path("gone.go");
        let error =
            fixture.dispatch("navigate.usages.find", json!({ "path": missing, "offset": 0 })).unwrap_err().to_string();
        let expected = zen_i18n::t!("workbench-nav-no-file", path = missing.display().to_string());
        assert!(error.contains(&expected), "{error}");
        let error = fixture
            .dispatch("navigate.declaration.goTo", json!({ "path": missing, "offset": 0 }))
            .unwrap_err()
            .to_string();
        assert!(error.contains(&expected), "{error}");
    }

    #[test]
    fn the_word_at_an_offset_is_found_on_either_side() {
        assert_eq!(super::word_at("a Привет(b)", 2).as_deref(), Some("Привет"));
        assert_eq!(super::word_at("a Привет(b)", 8).as_deref(), Some("Привет"));
        assert_eq!(super::word_at("a Привет(b)", 5).as_deref(), Some("Привет"));
        assert_eq!(super::word_at("a ( b", 3), None);
        assert_eq!(super::word_at("", 0), None);
    }
}

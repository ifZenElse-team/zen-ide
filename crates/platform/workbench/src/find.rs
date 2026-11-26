use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::sync::PoisonError;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_editor::{Query, SearchContext, Selection, Selections};
use zen_i18n::t;

use crate::brackets::Highlighters;
use crate::find_history::FindHistory;
use crate::language::places::Place;
use crate::language::usages::{UsagesState, group, word_at};
use crate::{Document, Workbench};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(OpenFind)?;
    registry.register(SetQuery)?;
    registry.register(Step::<true>)?;
    registry.register(Step::<false>)?;
    registry.register(SelectAllMatches)?;
    registry.register(Replace::<false>)?;
    registry.register(Replace::<true>)?;
    registry.register(Exclude)?;
    registry.register(Preview)?;
    registry.register(CloseFind)?;
    registry.register(FindStatus)?;
    registry.register(SearchInSelection)?;
    registry.register(ScrollToResults)?;
    registry.register(OpenInFindWindow)?;
    registry.register(WordAtCaret::<true>)?;
    registry.register(WordAtCaret::<false>)?;
    Ok(())
}

/// The search bar's state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindState {
    /// Whether the bar is shown.
    pub open: bool,
    /// Whether the replace row is shown too.
    pub replace_open: bool,
    /// What is searched for.
    pub query: Query,
    /// The replacement.
    pub replacement: String,
    /// Whether typing a query selects the first match from the caret and scrolls to it (Scroll to
    /// Results During Typing).
    pub scroll_to_results: bool,
}

impl Default for FindState {
    fn default() -> Self {
        Self {
            open: false,
            replace_open: false,
            query: Query::default(),
            replacement: String::new(),
            scroll_to_results: true,
        }
    }
}

/// What the find commands report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindInfo {
    /// The search bar's state.
    pub state: FindState,
    /// How many matches the active file has.
    pub count: usize,
    /// Which match the primary selection is, from 1, if it is one.
    pub current: Option<usize>,
    /// The matches of the active file Exclude skipped, as character ranges.
    pub excluded: Vec<Range<usize>>,
    /// Whether that match is excluded from replacing.
    pub current_excluded: bool,
    /// Why the query cannot be used (an invalid regular expression).
    pub error: Option<String>,
    /// Whether the search keeps to the text chosen with In Selection in the active file.
    pub in_selection: bool,
    /// That text, as a character range.
    pub scope: Option<Range<usize>>,
}

pub fn find_matches(
    highlighters: Option<&Highlighters>,
    document: &Document,
    query: &Query,
) -> Result<Vec<Range<usize>>, String> {
    let mut found = zen_editor::find_all(document.text(), query)?;
    if let Some(scope) = document.find_scope() {
        found.retain(|range| range.start >= scope.start && range.end <= scope.end);
    }
    if query.context == SearchContext::Anywhere || found.is_empty() {
        return Ok(found);
    }
    let highlighting = highlighters.and_then(|highlighters| highlighters.highlighting_of(document));
    let highlighting =
        highlighting.as_ref().map(|highlighting| highlighting.lock().unwrap_or_else(PoisonError::into_inner));
    let text = document.text();
    Ok(zen_syntax::keep_in_context(text, found, query.context, |byte| {
        highlighting.as_ref().and_then(|highlighting| highlighting.kind_at(text, byte))
    }))
}

fn matches(session: &Session, query: &Query) -> (Vec<Range<usize>>, Option<String>) {
    let Some(document) =
        session.state::<Workbench>().and_then(Workbench::active_document).filter(|document| document.image().is_none())
    else {
        return (Vec::new(), None);
    };
    match find_matches(session.state::<Highlighters>(), document, query) {
        Ok(found) => (found, None),
        Err(error) => (Vec::new(), Some(error)),
    }
}

fn forget_excluded(workbench: &mut Workbench) {
    for document in &mut workbench.documents {
        document.set_find_excluded(Vec::new());
    }
}

fn next_open<'a>(found: &'a [Range<usize>], excluded: &[Range<usize>], from: usize) -> Option<&'a Range<usize>> {
    let mut open = found.iter().filter(|range| !excluded.contains(range));
    open.clone().find(|range| range.start >= from).or_else(|| open.next())
}

fn info(session: &Session) -> FindInfo {
    let empty = |state| FindInfo {
        state,
        count: 0,
        current: None,
        excluded: Vec::new(),
        current_excluded: false,
        error: None,
        in_selection: false,
        scope: None,
    };
    let Some(workbench) = session.state::<Workbench>() else { return empty(FindState::default()) };
    let state = workbench.find.clone();
    let Some(document) = workbench.active_document().filter(|document| document.image().is_none()) else {
        return empty(state);
    };
    let (found, error) = matches(session, &state.query);
    let primary = document.selections().primary().range();
    let current = found.iter().position(|range| *range == primary).map(|index| index + 1);
    let excluded = document.find_excluded().to_vec();
    let current_excluded = current.is_some() && excluded.contains(&primary);
    let scope = document.find_scope();
    FindInfo {
        state,
        count: found.len(),
        current,
        excluded,
        current_excluded,
        error,
        in_selection: scope.is_some(),
        scope,
    }
}

fn forget_scopes(workbench: &mut Workbench) {
    for document in &mut workbench.documents {
        document.set_find_scope(None);
    }
}

fn remember(session: &mut Session, query: &str, replacement: Option<&str>) {
    session.state_mut::<FindHistory>().remember(Some(query), replacement, None);
}

fn has_text_document(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

fn workbench(session: &mut Session) -> Result<&mut Workbench, CommandError> {
    let workbench = session.state_mut::<Workbench>();
    if workbench.active_document().is_some_and(|document| document.image().is_none()) {
        Ok(workbench)
    } else {
        Err(CommandError::new(t!("workbench-error-no-document")))
    }
}

/// Whether to show the replace row too.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    /// Show the replace row (⌘R) as well as the search field (⌘F).
    pub replace: bool,
    /// Leave the query and In Selection as they are, whatever is selected (the bar's own replace
    /// toggle).
    pub keep_query: bool,
}

pub struct OpenFind;

impl Command for OpenFind {
    type Params = OpenParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.open";
    const TITLE: &'static str = "Find";
    const DESCRIPTION: &'static str = "Shows the search bar over the active file (⌘F), or the search and replace bar (⌘R); \
         a selection on one line becomes the query (escaped while Regex is on), a selection of several lines turns \
         on In Selection, and a caret alone leaves the query as it was.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<FindInfo, CommandError> {
        let workbench = workbench(session)?;
        let regex = workbench.find.query.regex;
        let Some(document) = workbench.active_document_mut().filter(|_| !params.keep_query) else {
            workbench.find.open = true;
            workbench.find.replace_open = params.replace;
            return Ok(info(session));
        };
        let range = document.selections().primary().range();
        let selected = document.snapshot().slice(range.clone()).map(|text| text.into_owned()).unwrap_or_default();
        if selected.contains('\n') {
            document.set_find_scope(Some(range));
        } else if !selected.is_empty() {
            document.set_find_scope(None);
            let text = if regex { zen_editor::escape_regex(&selected) } else { selected };
            if text != workbench.find.query.text {
                workbench.find.query.text = text;
                forget_excluded(workbench);
            }
        }
        workbench.find.open = true;
        workbench.find.replace_open = params.replace;
        Ok(info(session))
    }
}

/// The new query and replacement.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SetQueryParams {
    /// What to search for and how.
    pub query: Query,
    /// The replacement; unchanged when absent.
    pub replacement: Option<String>,
}

pub struct SetQuery;

impl Command for SetQuery {
    type Params = SetQueryParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.set";
    const TITLE: &'static str = "Set Search";
    const DESCRIPTION: &'static str = "Sets what the search bar looks for (text or a regular expression, with match case, \
         whole words, Preserve Case and the filter `context`: anywhere, inComments, inStringLiterals, exceptComments, \
         exceptStringLiterals or exceptCommentsAndStringLiterals) and the replacement; selects the first match from \
         the caret unless Scroll to Results During Typing is off; returns the count. A new query forgets the excluded \
         matches of every file.";

    fn run(&self, session: &mut Session, params: SetQueryParams) -> Result<FindInfo, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        if workbench.find.query != params.query {
            forget_excluded(workbench);
        }
        workbench.find.query = params.query;
        if let Some(replacement) = params.replacement {
            workbench.find.replacement = replacement;
        }
        let (query, scroll) = (workbench.find.query.clone(), workbench.find.scroll_to_results);
        let (found, _) = matches(session, &query);
        let workbench = session.state_mut::<Workbench>();
        if let Some(document) = workbench.active_document_mut().filter(|document| scroll && document.image().is_none())
        {
            let from = document.selections().primary().range().start;
            if let Some(range) = found.iter().find(|range| range.start >= from).or(found.first()) {
                document.set_selections(Selections::single(Selection::new(range.start, range.end)));
            }
        }
        Ok(info(session))
    }
}

pub struct Step<const NEXT: bool>;

impl<const NEXT: bool> Command for Step<NEXT> {
    type Params = NoParams;
    type Output = FindInfo;
    const ID: &'static str = if NEXT { "editor.find.next" } else { "editor.find.previous" };
    const TITLE: &'static str = if NEXT { "Find Next" } else { "Find Previous" };
    const DESCRIPTION: &'static str = if NEXT {
        "Selects the next match of the search in the active file, wrapping around (Enter, ⌘G)."
    } else {
        "Selects the previous match of the search in the active file, wrapping around (⇧Enter, ⇧⌘G)."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindInfo, CommandError> {
        let query = workbench(session)?.find.query.clone();
        remember(session, &query.text, None);
        step(session, &query, NEXT)
    }
}

fn step(session: &mut Session, query: &Query, next: bool) -> Result<FindInfo, CommandError> {
    let (found, _) = matches(session, query);
    let Some(document) = workbench(session)?.active_document_mut() else { return Ok(info(session)) };
    let primary = document.selections().primary().range();
    let target = if next {
        found.iter().find(|range| range.start >= primary.end && **range != primary).or(found.first())
    } else {
        found.iter().rev().find(|range| range.end <= primary.start).or(found.last())
    };
    if let Some(range) = target {
        document.set_selections(Selections::single(Selection::new(range.start, range.end)));
    }
    Ok(info(session))
}

pub struct SelectAllMatches;

impl Command for SelectAllMatches {
    type Params = NoParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.selectAll";
    const TITLE: &'static str = "Select All Matches";
    const DESCRIPTION: &'static str =
        "Selects every match of the search in the active file, each with a caret (⌥Enter).";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindInfo, CommandError> {
        let query = workbench(session)?.find.query.clone();
        remember(session, &query.text, None);
        let (found, _) = matches(session, &query);
        let Some(document) = workbench(session)?.active_document_mut() else { return Ok(info(session)) };
        if !found.is_empty() {
            let items = found.iter().map(|range| Selection::new(range.start, range.end)).collect();
            document.set_selections(Selections::from_vec(items, 0));
        }
        Ok(info(session))
    }
}

/// The replacement to use; the search bar's when absent.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ReplaceParams {
    /// The replacement text; `$1` and `${name}` refer to groups of a regular expression, and its
    /// case escapes convert case.
    pub replacement: Option<String>,
}

pub struct Replace<const ALL: bool>;

impl<const ALL: bool> Command for Replace<ALL> {
    type Params = ReplaceParams;
    type Output = FindInfo;
    const ID: &'static str = if ALL { "editor.find.replaceAll" } else { "editor.find.replace" };
    const TITLE: &'static str = if ALL { "Replace All" } else { "Replace" };
    const DESCRIPTION: &'static str = if ALL {
        "Replaces every match of the search in the active file but the excluded ones, as one undo step."
    } else {
        "Replaces the selected match of the search in the active file (unless it is excluded) and selects the next \
         match that is not."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: ReplaceParams) -> Result<FindInfo, CommandError> {
        let bar = workbench(session)?;
        if let Some(replacement) = params.replacement {
            bar.find.replacement = replacement;
        }
        let (query, replace) = (bar.find.query.clone(), bar.find.replacement.clone());
        remember(session, &query.text, Some(&replace));
        let (found, error) = matches(session, &query);
        if let Some(error) = error {
            return Err(CommandError::new(t!("workbench-error-search", error = error)));
        }
        let Some(document) = workbench(session)?.active_document_mut() else { return Ok(info(session)) };
        let excluded = document.find_excluded().to_vec();
        let text = document.text().to_owned();
        let primary = document.selections().primary().range();
        let replaced: Vec<(Range<usize>, String)> = if ALL {
            let counted: HashSet<&Range<usize>> = found.iter().collect();
            zen_editor::replacements(&text, &query, &replace)
                .map_err(|error| CommandError::new(t!("workbench-error-search", error = error)))?
                .into_iter()
                .filter(|(range, _)| counted.contains(range) && !excluded.contains(range))
                .collect()
        } else {
            found
                .iter()
                .filter(|range| **range == primary && !excluded.contains(range))
                .map(|range| {
                    let new = zen_editor::replacement(&text, range.clone(), &query, &replace).unwrap_or_default();
                    (range.clone(), new)
                })
                .collect()
        };
        if let [(range, new)] = replaced.as_slice()
            && !ALL
        {
            let end = range.start + new.chars().count();
            document.replace_ranges(replaced.clone());
            document.set_selections(Selections::single(Selection::caret(end)));
        } else {
            document.replace_ranges(replaced);
        }
        if !ALL {
            let (found, _) = matches(session, &query);
            let Some(document) = workbench(session)?.active_document_mut() else { return Ok(info(session)) };
            let from = document.selections().primary().range().end;
            if let Some(range) = next_open(&found, document.find_excluded(), from).cloned() {
                document.set_selections(Selections::single(Selection::new(range.start, range.end)));
            }
        }
        Ok(info(session))
    }
}

pub struct Exclude;

impl Command for Exclude {
    type Params = NoParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.exclude";
    const TITLE: &'static str = "Exclude";
    const DESCRIPTION: &'static str = "Excludes the selected match of the search from Replace and Replace All in the active \
         file and selects the next match that is not excluded; on an excluded match it includes it again.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindInfo, CommandError> {
        let query = workbench(session)?.find.query.clone();
        let (found, _) = matches(session, &query);
        let Some(document) = workbench(session)?.active_document_mut() else { return Ok(info(session)) };
        let mut excluded = document.find_excluded().to_vec();
        let primary = document.selections().primary().range();
        let Some(current) = found.iter().find(|range| **range == primary).cloned() else { return Ok(info(session)) };
        let next = if let Some(index) = excluded.iter().position(|range| *range == current) {
            excluded.remove(index);
            None
        } else {
            excluded.push(current.clone());
            next_open(&found, &excluded, current.end).cloned()
        };
        if let Some(range) = next {
            document.set_selections(Selections::single(Selection::new(range.start, range.end)));
        }
        document.set_find_excluded(excluded);
        Ok(info(session))
    }
}

/// One match and what it becomes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    /// The match's first character.
    pub start: usize,
    /// The character after the match.
    pub end: usize,
    /// What Replace puts in its place.
    pub replacement: String,
    /// Whether it is excluded from replacing.
    pub excluded: bool,
}

/// Which lines to preview.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct PreviewParams {
    /// The first line wanted, one-based; the first of the file when absent.
    pub from_line: Option<usize>,
    /// The last line wanted, one-based; the last of the file when absent.
    pub to_line: Option<usize>,
}

/// What every match of the active file becomes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindPreview {
    /// The matches in order, each with its replacement.
    pub matches: Vec<PreviewItem>,
    /// Why the query cannot be used (an invalid regular expression).
    pub error: Option<String>,
}

pub struct Preview;

impl Command for Preview {
    type Params = PreviewParams;
    type Output = FindPreview;
    const ID: &'static str = "editor.find.preview";
    const TITLE: &'static str = "Preview Replacement";
    const DESCRIPTION: &'static str = "Returns every match of the search in the active file with what the search bar's \
         replacement turns it into (groups, case conversion and Preserve Case applied) and whether it is excluded, \
         for the matches that start on some lines (fromLine and toLine, one-based) or in the whole file; changes nothing.";

    fn run(&self, session: &mut Session, params: PreviewParams) -> Result<FindPreview, CommandError> {
        let Some(workbench) = session.state::<Workbench>() else {
            return Ok(FindPreview { matches: Vec::new(), error: None });
        };
        let Some(document) = workbench.active_document().filter(|document| document.image().is_none()) else {
            return Ok(FindPreview { matches: Vec::new(), error: None });
        };
        let excluded = document.find_excluded();
        let text = document.snapshot();
        let line_start = |line: usize| {
            if line >= text.len_lines() {
                text.len_chars()
            } else {
                text.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default()
            }
        };
        let within =
            line_start(params.from_line.unwrap_or(1).saturating_sub(1))..params.to_line.map_or(usize::MAX, line_start);
        let (query, replace) = (&workbench.find.query, &workbench.find.replacement);
        let filtered = query.context != SearchContext::Anywhere || document.find_scope().is_some();
        let counted: Option<HashSet<Range<usize>>> = filtered.then(|| {
            find_matches(session.state::<Highlighters>(), document, query).unwrap_or_default().into_iter().collect()
        });
        Ok(match zen_editor::replacements_within(document.text(), query, replace, within) {
            Ok(all) => FindPreview {
                matches: all
                    .into_iter()
                    .filter(|(range, _)| counted.as_ref().is_none_or(|counted| counted.contains(range)))
                    .map(|(range, replacement)| PreviewItem {
                        start: range.start,
                        end: range.end,
                        replacement,
                        excluded: excluded.contains(&range),
                    })
                    .collect(),
                error: None,
            },
            Err(error) => FindPreview { matches: Vec::new(), error: Some(error) },
        })
    }
}

pub struct FindStatus;

impl Command for FindStatus {
    type Params = NoParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.info";
    const TITLE: &'static str = "Search Status";
    const DESCRIPTION: &'static str = "Returns the search bar's query and replacement, the number of matches in the active file and which one is selected.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindInfo, CommandError> {
        Ok(info(session))
    }
}

pub struct CloseFind;

impl Command for CloseFind {
    type Params = NoParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.close";
    const TITLE: &'static str = "Close Search";
    const DESCRIPTION: &'static str = "Hides the search bar of the editor (Escape), keeps its query in the history and \
         forgets the excluded matches and In Selection.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindInfo, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let (query, open) = (workbench.find.query.text.clone(), workbench.find.open);
        workbench.find.open = false;
        workbench.find.replace_open = false;
        forget_excluded(workbench);
        forget_scopes(workbench);
        if open {
            remember(session, &query, None);
        }
        Ok(info(session))
    }
}

/// Whether to turn an option on or off; it is toggled when absent.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SwitchParams {
    /// On (`true`) or off (`false`); toggled when absent.
    pub on: Option<bool>,
}

pub struct SearchInSelection;

impl Command for SearchInSelection {
    type Params = SwitchParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.searchInSelection";
    const TITLE: &'static str = "Search In Selection";
    const DESCRIPTION: &'static str = "Turns In Selection of the search bar on (the search keeps to the text selected now, \
         followed through edits) or off (the whole file), toggling when `on` is absent; shows the bar.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: SwitchParams) -> Result<FindInfo, CommandError> {
        let workbench = workbench(session)?;
        workbench.find.open = true;
        let Some(document) = workbench.active_document_mut() else { return Ok(info(session)) };
        let on = params.on.unwrap_or(document.find_scope().is_none());
        if !on {
            document.set_find_scope(None);
        } else if document.find_scope().is_none() {
            let range = document.selections().primary().range();
            if range.is_empty() {
                return Err(CommandError::new(t!("workbench-error-find-no-selection")));
            }
            document.set_find_scope(Some(range));
        }
        forget_excluded(workbench);
        Ok(info(session))
    }
}

pub struct ScrollToResults;

impl Command for ScrollToResults {
    type Params = SwitchParams;
    type Output = FindInfo;
    const ID: &'static str = "editor.find.scrollToResults";
    const TITLE: &'static str = "Scroll to Results During Typing";
    const DESCRIPTION: &'static str = "Turns Scroll to Results During Typing on or off, toggling when `on` is absent: while \
         it is on, each change of the query selects the first match from the caret and scrolls to it.";

    fn run(&self, session: &mut Session, params: SwitchParams) -> Result<FindInfo, CommandError> {
        let find = &mut session.state_mut::<Workbench>().find;
        find.scroll_to_results = params.on.unwrap_or(!find.scroll_to_results);
        Ok(info(session))
    }
}

pub struct OpenInFindWindow;

impl Command for OpenInFindWindow {
    type Params = NoParams;
    type Output = UsagesState;
    const ID: &'static str = "editor.find.openInFindWindow";
    const TITLE: &'static str = "Open Results in Find Window";
    const DESCRIPTION: &'static str = "Lists the matches of the search bar's query in the active file on a tab \
         \"Occurrences of '…'\" of the Find tool window (`tab`, which search.results.tree reads), grouped as the \
         Find in Files tabs are; navigate.usages.list reads them too.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<UsagesState, CommandError> {
        let query = workbench(session)?.find.query.clone();
        if query.text.is_empty() {
            return Err(CommandError::new(t!("workbench-error-find-no-query")));
        }
        remember(session, &query.text, None);
        let (found, error) = matches(session, &query);
        if let Some(error) = error {
            return Err(CommandError::new(t!("workbench-error-search", error = error)));
        }
        let workbench = workbench(session)?;
        let Some(document) = workbench.active_document() else {
            return Err(CommandError::new(t!("workbench-error-no-document")));
        };
        let path = document.path().to_owned();
        let places = found.iter().map(|range| place(document, &path, range.clone())).collect();
        let files = group(places, workbench.project());
        session.state_mut::<crate::language::Languages>().end_lookup(crate::language::LookupKind::Usages);
        crate::find_results::usages::forget(session);
        let tab = crate::find_results::usages::open_file_tab(session, &path, query.clone())?;
        let state = session.state_mut::<UsagesState>();
        let total = files.iter().map(|file| file.usages.len()).sum();
        *state = UsagesState {
            search: state.search + 1,
            symbol: Some(query.text),
            occurrences: true,
            error: (total == 0).then(|| t!("workbench-find-no-occurrences")),
            files,
            total,
            tab: Some(tab),
            ..UsagesState::default()
        };
        Ok(state.clone())
    }
}

fn place(document: &Document, path: &Path, range: Range<usize>) -> Place {
    let text = document.snapshot();
    let start = text.offset_to_point(range.start).unwrap_or_default();
    let line_start = range.start - start.column;
    let line_end = text
        .point_to_offset(zen_text::Point::new(start.line + 1, 0))
        .map_or(text.len_chars(), |next| next.saturating_sub(1))
        .max(range.start);
    let line = text.slice(line_start..line_end).map(|line| line.into_owned()).unwrap_or_default();
    let line = line.trim_end_matches(['\n', '\r']);
    let lead = line.chars().take_while(|c| c.is_whitespace()).count();
    let preview = line.trim();
    let length = preview.chars().count();
    let from = (range.start - line_start).saturating_sub(lead).min(length);
    let to = (from + range.len()).min(length);
    Place {
        path: path.to_owned(),
        start: range.start,
        end: range.end,
        line: start.line as u32 + 1,
        column: start.column as u32 + 1,
        preview: Some(preview.to_owned()),
        highlight: Some([from, to]),
    }
}

pub struct WordAtCaret<const NEXT: bool>;

impl<const NEXT: bool> Command for WordAtCaret<NEXT> {
    type Params = NoParams;
    type Output = FindInfo;
    const ID: &'static str = if NEXT { "editor.find.nextWordAtCaret" } else { "editor.find.previousWordAtCaret" };
    const TITLE: &'static str =
        if NEXT { "Next Occurrence of the Word at Caret" } else { "Previous Occurrence of the Word at Caret" };
    const DESCRIPTION: &'static str = if NEXT {
        "Makes the selected text, or the word at the caret, the search's query (whole words, match case) and selects \
         its next occurrence, wrapping around."
    } else {
        "Makes the selected text, or the word at the caret, the search's query (whole words, match case) and selects \
         its previous occurrence, wrapping around."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindInfo, CommandError> {
        let workbench = workbench(session)?;
        let Some(document) = workbench.active_document() else { return Ok(info(session)) };
        let primary = document.selections().primary();
        let word = if primary.is_empty() {
            word_at(document.text(), primary.head)
        } else {
            document.snapshot().slice(primary.range()).ok().map(|text| text.into_owned())
        };
        let Some(word) = word.filter(|word| !word.is_empty() && !word.contains('\n')) else {
            return Err(CommandError::new(t!("workbench-nav-no-symbol")));
        };
        let query = Query { text: word, match_case: true, whole_words: true, ..Query::default() };
        if workbench.find.query != query {
            forget_excluded(workbench);
            forget_scopes(workbench);
            workbench.find.query = query.clone();
        }
        remember(session, &query.text, None);
        step(session, &query, NEXT)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use zen_syntax::{HighlightKind, Highlighter, Span};

    use super::*;

    pub(crate) struct Literals;

    impl Highlighter for Literals {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<Span> {
            let mut spans = Vec::new();
            let mut offset = 0;
            for line in text.split_inclusive('\n') {
                let comment = line.find("//").unwrap_or(line.len());
                let quotes: Vec<usize> = line[..comment].match_indices('"').map(|(at, _)| at).collect();
                for pair in quotes.chunks(2).filter(|pair| pair.len() == 2) {
                    spans.push(Span { range: offset + pair[0]..offset + pair[1] + 1, kind: HighlightKind::String });
                }
                if comment < line.len() {
                    let end = offset + line.trim_end_matches('\n').len();
                    spans.push(Span { range: offset + comment..end, kind: HighlightKind::Comment });
                }
                offset += line.len();
            }
            spans
        }
    }

    pub(crate) fn literals(session: &mut Session) {
        let literals: Arc<dyn Highlighter> = Arc::new(Literals);
        session
            .state_mut::<Highlighters>()
            .set_source(Arc::new(move |name| Literals.handles(name).then(|| Arc::clone(&literals))));
    }

    #[test]
    fn the_filter_keeps_matches_by_the_token_they_start_in() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "total := \"total\" // total\nuse(total)\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        literals(&mut session);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let mut count = |context: &str| {
            let query = json!({ "query": { "text": "total", "context": context } });
            let found = registry.dispatch(&mut session, "editor.find.set", query).unwrap();
            (found["count"].as_u64().unwrap(), found["state"]["query"]["context"].as_str().unwrap().to_owned())
        };
        assert_eq!(count("anywhere"), (4, "anywhere".to_owned()));
        assert_eq!(count("inComments").0, 1);
        assert_eq!(count("inStringLiterals").0, 1);
        assert_eq!(count("exceptComments").0, 3);
        assert_eq!(count("exceptStringLiterals").0, 3);
        assert_eq!(count("exceptCommentsAndStringLiterals").0, 2);

        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        run(&mut session, "editor.find.set", json!({ "query": { "text": "total", "context": "inStringLiterals" } }));
        let selected = |session: &Session| {
            session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().range()
        };
        assert_eq!(selected(&session), 10..15, "the first match in a string");
        run(&mut session, "editor.find.next", json!({}));
        assert_eq!(selected(&session), 10..15, "the only one");
        run(
            &mut session,
            "editor.find.set",
            json!({ "query": { "text": "total", "context": "exceptCommentsAndStringLiterals" } }),
        );
        run(&mut session, "editor.find.replaceAll", json!({ "replacement": "sum" }));
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "sum := \"total\" // total\nuse(sum)\n");
        let preview = run(&mut session, "editor.find.preview", json!({}));
        assert_eq!(preview["matches"], json!([]), "the string's and the comment's matches are not counted");
        run(&mut session, "editor.find.set", json!({ "query": { "text": "total", "context": "inComments" } }));
        let preview = run(&mut session, "editor.find.preview", json!({}));
        assert_eq!(preview["matches"].as_array().map(Vec::len), Some(1), "{preview}");
        let bad = registry.dispatch(&mut session, "editor.find.set", json!({ "query": { "context": "nowhere" } }));
        assert!(bad.is_err(), "an unknown filter is refused");
    }

    #[test]
    fn find_step_replace_and_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "item := items[0]\nuse(item)\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        let text =
            |session: &Session| session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();

        run(&mut session, "editor.find.open", json!({}));
        let found = run(&mut session, "editor.find.set", json!({ "query": { "text": "item", "wholeWords": true } }));
        assert_eq!((found["count"].as_u64(), found["current"].as_u64()), (Some(2), Some(1)));
        let next = run(&mut session, "editor.find.next", json!({}));
        assert_eq!(next["current"], 2);
        let wrapped = run(&mut session, "editor.find.next", json!({}));
        assert_eq!(wrapped["current"], 1, "wraps around");
        let replaced = run(&mut session, "editor.find.replace", json!({ "replacement": "entry" }));
        assert_eq!(text(&session), "entry := items[0]\nuse(item)\n");
        assert_eq!(
            (replaced["count"].as_u64(), replaced["current"].as_u64()),
            (Some(1), Some(1)),
            "the next match is selected"
        );

        run(&mut session, "editor.find.set", json!({ "query": { "text": r"(\w+)\[(\d)\]", "regex": true } }));
        run(&mut session, "editor.find.replaceAll", json!({ "replacement": "$1.At($2)" }));
        assert_eq!(text(&session), "entry := items.At(0)\nuse(item)\n");
        run(&mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), "entry := items[0]\nuse(item)\n");
        let bad = registry
            .dispatch(&mut session, "editor.find.set", json!({ "query": { "text": "(", "regex": true } }))
            .unwrap();
        assert!(bad["error"].is_string());
        assert!(registry.dispatch(&mut session, "editor.find.replaceAll", json!({})).is_err());
        let closed = run(&mut session, "editor.find.close", json!({}));
        assert_eq!(closed["state"]["open"], false);
    }

    #[test]
    fn exclude_skips_a_match_and_preview_shows_each_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "fooBar FooBar FOOBAR fooBar\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        let text =
            |session: &Session| session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();

        run(&mut session, "editor.find.open", json!({ "replace": true }));
        let query = json!({ "text": "fooBar", "preserveCase": true });
        let found = run(&mut session, "editor.find.set", json!({ "query": query, "replacement": "bazQux" }));
        assert_eq!((found["count"].as_u64(), found["current"].as_u64()), (Some(4), Some(1)));
        let preview = run(&mut session, "editor.find.preview", json!({}));
        let shown: Vec<&str> =
            preview["matches"].as_array().unwrap().iter().map(|item| item["replacement"].as_str().unwrap()).collect();
        assert_eq!(shown, ["bazQux", "BazQux", "BAZQUX", "bazQux"]);
        assert_eq!(
            (preview["matches"][1]["start"].as_u64(), preview["matches"][1]["end"].as_u64()),
            (Some(7), Some(13))
        );

        let skipped = run(&mut session, "editor.find.exclude", json!({}));
        assert_eq!(skipped["current"], 2, "the next match is selected");
        assert_eq!(skipped["excluded"], json!([{ "start": 0, "end": 6 }]));
        run(&mut session, "editor.find.next", json!({}));
        run(&mut session, "editor.find.exclude", json!({}));
        run(&mut session, "editor.find.previous", json!({}));
        let back = run(&mut session, "editor.find.exclude", json!({}));
        assert_eq!((back["current"].as_u64(), back["currentExcluded"].as_bool()), (Some(3), Some(false)), "taken back");
        let excluded = run(&mut session, "editor.find.next", json!({}));
        assert_eq!(excluded["currentExcluded"], false);
        run(&mut session, "editor.find.previous", json!({}));
        run(&mut session, "editor.find.previous", json!({}));
        let first = run(&mut session, "editor.find.previous", json!({}));
        assert_eq!((first["current"].as_u64(), first["currentExcluded"].as_bool()), (Some(1), Some(true)));
        let preview = run(&mut session, "editor.find.preview", json!({}));
        assert_eq!(preview["matches"][0]["excluded"], true);

        let replaced = run(&mut session, "editor.find.replace", json!({}));
        assert_eq!(text(&session), "fooBar FooBar FOOBAR fooBar\n", "an excluded match is not replaced");
        assert_eq!(replaced["current"], 2);
        run(&mut session, "editor.find.replace", json!({}));
        assert_eq!(text(&session), "fooBar BazQux FOOBAR fooBar\n");
        let after = run(&mut session, "editor.find.info", json!({}));
        assert_eq!(after["excluded"], json!([{ "start": 0, "end": 6 }]), "kept after a replace");
        run(&mut session, "editor.find.replaceAll", json!({}));
        assert_eq!(text(&session), "fooBar BazQux BAZQUX bazQux\n", "Replace All skips the excluded match");

        run(&mut session, "editor.find.set", json!({ "query": { "text": "fooBar" } }));
        let fresh = run(&mut session, "editor.find.info", json!({}));
        assert_eq!(fresh["excluded"], json!([]), "a new query forgets the exclusions");
    }

    #[test]
    fn excluded_matches_follow_edits_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "ab ab ab\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let mut run = |id: &str, params: serde_json::Value| registry.dispatch(&mut session, id, params).unwrap();
        run("editor.find.open", json!({ "replace": true }));
        run("editor.find.set", json!({ "query": { "text": "ab" }, "replacement": "xyz" }));
        run("editor.find.next", json!({}));
        let skipped = run("editor.find.exclude", json!({}));
        assert_eq!(skipped["excluded"], json!([{ "start": 3, "end": 5 }]));
        run("editor.find.replaceAll", json!({}));
        let after = run("editor.find.info", json!({}));
        assert_eq!(after["excluded"], json!([{ "start": 4, "end": 6 }]), "moved by the replacement before it");
        run("editor.edit.undo", json!({}));
        let undone = run("editor.find.info", json!({}));
        assert_eq!(undone["excluded"], json!([{ "start": 3, "end": 5 }]), "back where it was after undo");
        run("editor.find.replaceAll", json!({}));
        let text = run("editor.find.info", json!({}));
        assert_eq!(text["excluded"], json!([{ "start": 4, "end": 6 }]));
        run("editor.edit.undo", json!({}));
        run("editor.find.close", json!({}));
        let closed = run("editor.find.info", json!({}));
        assert_eq!(closed["excluded"], json!([]), "closing the bar forgets them");

        let next = dir.path().join("next.go");
        std::fs::write(&next, "abab\n").unwrap();
        run("file.document.open", json!({ "path": next }));
        run("editor.find.open", json!({ "replace": true }));
        run("editor.find.set", json!({ "query": { "text": "ab" }, "replacement": "xyz" }));
        run("editor.find.next", json!({}));
        let skipped = run("editor.find.exclude", json!({}));
        assert_eq!(skipped["excluded"], json!([{ "start": 2, "end": 4 }]));
        assert_eq!(skipped["current"], 1, "the match before it is selected");
        run("editor.find.replace", json!({}));
        let after = run("editor.find.info", json!({}));
        assert_eq!(after["excluded"], json!([{ "start": 3, "end": 5 }]), "a replacement right before it moves it");
        assert_eq!(after["currentExcluded"], false);
        run("editor.edit.undo", json!({}));
        let undone = run("editor.find.info", json!({}));
        assert_eq!(undone["excluded"], json!([{ "start": 2, "end": 4 }]));
        run("editor.find.replaceAll", json!({}));
        let all = run("editor.find.info", json!({}));
        assert_eq!(all["excluded"], json!([{ "start": 3, "end": 5 }]), "Replace All after undo still skips it");
    }

    fn open(text: &str) -> (tempfile::TempDir, Registry, Session) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        (dir, registry, session)
    }

    fn select(registry: &Registry, session: &mut Session, anchor: (u32, u32), head: (u32, u32)) {
        let selection = json!({ "selections": [{
            "anchor": { "line": anchor.0, "column": anchor.1 },
            "head": { "line": head.0, "column": head.1 },
        }] });
        registry.dispatch(session, "editor.selection.set", selection).unwrap();
    }

    #[test]
    fn find_takes_a_selection_on_one_line_and_turns_in_selection_on_for_several() {
        let (_dir, registry, mut session) = open("a.b := 1\nuse(a.b)\nuse(a.b)\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        select(&registry, &mut session, (0, 1), (0, 1));
        let caret = run(&mut session, "editor.find.open", json!({}));
        assert_eq!(caret["state"]["query"]["text"], "", "a caret on a word leaves the field empty");
        select(&registry, &mut session, (0, 0), (0, 3));
        let taken = run(&mut session, "editor.find.open", json!({}));
        assert_eq!(taken["state"]["query"]["text"], "a.b");
        assert_eq!((taken["count"].as_u64(), taken["inSelection"].as_bool()), (Some(3), Some(false)));
        run(&mut session, "editor.find.set", json!({ "query": { "text": "x", "regex": true } }));
        select(&registry, &mut session, (0, 0), (0, 3));
        let escaped = run(&mut session, "editor.find.open", json!({}));
        assert_eq!(escaped["state"]["query"]["text"], r"a\.b", "escaped while Regex is on");
        assert_eq!(escaped["count"], 3);

        run(&mut session, "editor.find.set", json!({ "query": { "text": "a.b" } }));
        select(&registry, &mut session, (1, 0), (2, 8));
        let scoped = run(&mut session, "editor.find.open", json!({}));
        assert_eq!(scoped["state"]["query"]["text"], "a.b", "the query stays");
        assert_eq!((scoped["inSelection"].as_bool(), scoped["count"].as_u64()), (Some(true), Some(2)));
        assert_eq!(scoped["scope"], json!({ "start": 9, "end": 26 }));
        let first = run(&mut session, "editor.find.next", json!({}));
        assert_eq!((first["current"].as_u64(), first["inSelection"].as_bool()), (Some(1), Some(true)));
        run(&mut session, "editor.find.replaceAll", json!({ "replacement": "c" }));
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "a.b := 1\nuse(c)\nuse(c)\n", "Replace All keeps to the selection");
        let off = run(&mut session, "editor.find.searchInSelection", json!({}));
        assert_eq!((off["inSelection"].as_bool(), off["count"].as_u64()), (Some(false), Some(1)));
        select(&registry, &mut session, (0, 0), (0, 0));
        let refused = registry.dispatch(&mut session, "editor.find.searchInSelection", json!({ "on": true }));
        assert!(refused.is_err(), "nothing selected to search in");
        select(&registry, &mut session, (0, 0), (1, 6));
        let on = run(&mut session, "editor.find.searchInSelection", json!({}));
        assert_eq!((on["inSelection"].as_bool(), on["state"]["open"].as_bool()), (Some(true), Some(true)));
        let closed = run(&mut session, "editor.find.close", json!({}));
        assert_eq!(closed["inSelection"], false, "closing the bar ends In Selection");
    }

    #[test]
    fn scroll_to_results_during_typing_can_be_turned_off() {
        let (_dir, registry, mut session) = open("one two one\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        let off = run(&mut session, "editor.find.scrollToResults", json!({}));
        assert_eq!(off["state"]["scrollToResults"], false);
        let typed = run(&mut session, "editor.find.set", json!({ "query": { "text": "two" } }));
        assert_eq!((typed["count"].as_u64(), typed["current"].as_u64()), (Some(1), None), "the caret stays");
        run(&mut session, "editor.find.scrollToResults", json!({ "on": true }));
        let typed = run(&mut session, "editor.find.set", json!({ "query": { "text": "one" } }));
        assert_eq!(typed["current"], 1);
    }

    #[test]
    fn the_results_open_in_the_find_window_and_the_word_at_the_caret_is_stepped_through() {
        let (_dir, registry, mut session) = open("total := 1\n\ttotal++ // total\nsubtotal(total)\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        assert!(registry.dispatch(&mut session, "editor.find.openInFindWindow", json!({})).is_err(), "no query");
        run(&mut session, "editor.find.set", json!({ "query": { "text": "total", "wholeWords": true } }));
        let found = run(&mut session, "editor.find.openInFindWindow", json!({}));
        assert_eq!((found["occurrences"].as_bool(), found["total"].as_u64()), (Some(true), Some(4)));
        assert_eq!(found["symbol"], "total");
        let second = &found["files"][0]["usages"][1];
        assert_eq!((second["line"].as_u64(), second["column"].as_u64()), (Some(2), Some(2)));
        assert_eq!(
            (second["preview"].as_str(), second["highlight"].clone()),
            (Some("total++ // total"), json!([0, 5]))
        );
        let listed = run(&mut session, "navigate.usages.list", json!({}));
        assert_eq!(listed["total"], 4);
        let tree = run(&mut session, "search.results.tree", json!({ "tab": found["tab"] }));
        assert_eq!(
            (&tree["title"], &tree["kind"], &tree["total"], &tree["files"]),
            (&json!(t!("workbench-results-title", text = "total")), &json!("file"), &json!(4), &json!(1)),
            "a tab of the Find tool window like Find in Files': {tree}"
        );
        let rerun = run(&mut session, "search.results.rerun", json!({ "tab": found["tab"] }));
        assert_eq!((rerun["total"].clone(), rerun["done"].clone()), (json!(4), json!(true)));

        select(&registry, &mut session, (1, 3), (1, 3));
        let next = run(&mut session, "editor.find.nextWordAtCaret", json!({}));
        assert_eq!((next["current"].as_u64(), next["state"]["query"]["wholeWords"].as_bool()), (Some(3), Some(true)));
        let previous = run(&mut session, "editor.find.previousWordAtCaret", json!({}));
        assert_eq!(previous["current"], 2);
        let previous = run(&mut session, "editor.find.previousWordAtCaret", json!({}));
        assert_eq!(previous["current"], 1);
        let wrapped = run(&mut session, "editor.find.previousWordAtCaret", json!({}));
        assert_eq!(wrapped["current"], 4, "wraps around");
    }

    #[test]
    fn the_find_history_is_kept_newest_first_in_its_file() {
        let (dir, registry, mut session) = open("ab cd ab\n");
        let file = dir.path().join("data").join("find-history.json");
        session.set_state(FindHistory::load(&file));
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        run(&mut session, "editor.find.open", json!({ "replace": true }));
        run(&mut session, "editor.find.set", json!({ "query": { "text": "a" } }));
        run(&mut session, "editor.find.set", json!({ "query": { "text": "ab" }, "replacement": "x" }));
        assert_eq!(run(&mut session, "search.findHistory.list", json!({}))["queries"], json!([]), "typing is not kept");
        run(&mut session, "editor.find.next", json!({}));
        run(&mut session, "editor.find.replace", json!({}));
        run(&mut session, "search.findHistory.add", json!({ "query": "cd" }));
        run(&mut session, "editor.find.close", json!({}));
        let history = run(&mut session, "search.findHistory.list", json!({}));
        assert_eq!(history, json!({ "queries": ["ab", "cd"], "replacements": ["x"], "masks": [] }));
        assert_eq!(FindHistory::load(&file).data().queries, ["ab", "cd"], "kept in the file");
        run(&mut session, "search.findHistory.clear", json!({}));
        assert_eq!(FindHistory::load(&file).data(), crate::find_history::FindHistoryData::default());
    }
}

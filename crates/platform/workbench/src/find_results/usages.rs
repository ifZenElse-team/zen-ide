use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_index::TextMatch;

use super::{FindResults, Found, Infos, Progress, ResultsTab, Source, UsageType, UsagesTarget, classify};
use crate::Workbench;
use crate::language::highlights::{HighlightKind, server_ranges};
use crate::language::show_usages::{DeclarationKind, UsageAccess, UsagesScope};
use crate::language::structure::Outliners;
use crate::language::usages::{FindUsages, FindUsagesParams, UsagesFile, UsagesState, word_at};
use crate::language::{Languages, file_text, with_languages};
use crate::search::SearchParams;
use crate::tasks::{TaskHandle, Tasks};

pub const OPTIONS: &str = "navigate.usages.options";
const RECENT: usize = 15;
const ACCESS_FILES: usize = 500;
const ACCESS_AT_ONCE: usize = 8;
const ACCESS_WAIT: Duration = Duration::from_secs(5);
const HOLDER: &str = "find-usages-kind";
const ACCESS_HOLDER: &str = "find-usages-access";

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(RecentFindUsages)?;
    registry.register(OpenRecentFindUsages)?;
    Ok(())
}

/// The Find Usages options, as the Find Usages Settings dialog (⌥⇧⌘F7) keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FindUsagesOptions {
    /// Where Find Usages looks.
    pub scope: UsagesScope,
    /// Open in new tab: the usages open on a tab of their own rather than in place of the last tab
    /// that is not pinned.
    pub new_tab: bool,
    /// Skip results tab with one usage: a single usage is opened in the editor, with no tab.
    pub skip_single: bool,
}

impl Default for FindUsagesOptions {
    fn default() -> Self {
        Self { scope: UsagesScope::ProjectFiles, new_tab: false, skip_single: false }
    }
}

impl FindUsagesOptions {
    pub fn of(session: &Session) -> Self {
        session.settings().get_or(OPTIONS, Self::default())
    }
}

pub(crate) struct Waiting {
    search: u64,
    target: UsagesTarget,
    new_tab: bool,
    skip_single: bool,
    task: TaskHandle,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

pub(crate) fn scope_label(scope: UsagesScope) -> String {
    match scope {
        UsagesScope::CurrentFile => t!("workbench-usages-scope-current-file"),
        UsagesScope::OpenFiles => t!("workbench-usages-scope-open-files"),
        UsagesScope::ProjectFiles => t!("workbench-usages-scope-project-files"),
        UsagesScope::AllPlaces => t!("workbench-usages-scope-all-places"),
    }
}

pub(crate) fn kind_label(kind: DeclarationKind) -> String {
    match kind {
        DeclarationKind::Variable => t!("workbench-usages-kind-variable"),
        DeclarationKind::Parameter => t!("workbench-usages-kind-parameter"),
        DeclarationKind::Constant => t!("workbench-usages-kind-constant"),
        DeclarationKind::Function => t!("workbench-usages-kind-function"),
        DeclarationKind::Method => t!("workbench-usages-kind-method"),
        DeclarationKind::Type => t!("workbench-usages-kind-type"),
        DeclarationKind::Struct => t!("workbench-usages-kind-struct"),
        DeclarationKind::Interface => t!("workbench-usages-kind-interface"),
        DeclarationKind::Class => t!("workbench-usages-kind-class"),
        DeclarationKind::Field => t!("workbench-usages-kind-field"),
        DeclarationKind::Property => t!("workbench-usages-kind-property"),
        DeclarationKind::Package => t!("workbench-usages-kind-package"),
        DeclarationKind::Module => t!("workbench-usages-kind-module"),
        DeclarationKind::Symbol => t!("workbench-usages-kind-symbol"),
    }
}

pub(crate) fn target_label(target: &UsagesTarget) -> String {
    t!("workbench-results-usages-target", kind = kind_label(target.kind), symbol = target.symbol.as_str())
}

pub(crate) fn begin(session: &mut Session, search: u64, target: UsagesTarget, new_tab: bool, skip_single: bool) {
    let title = t!("workbench-usages-task", symbol = target.symbol.as_str());
    let task = session.state_mut::<Tasks>().start(&title, super::TASK_SOURCE, false);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    task.cancel_with(Arc::clone(&cancel));
    let results = session.state_mut::<FindResults>();
    if let Some(old) = results.waiting.replace(Waiting { search, target, new_tab, skip_single, task, cancel }) {
        old.task.finish(Ok(t!("workbench-usages-task-replaced")));
    }
    results.generation += 1;
}

pub(crate) fn waiting(session: &Session) -> Option<u64> {
    session.state::<FindResults>().and_then(|results| results.waiting.as_ref()).map(|waiting| waiting.search)
}

pub(crate) fn cancelled(session: &Session) -> bool {
    session
        .state::<FindResults>()
        .and_then(|results| results.waiting.as_ref())
        .is_some_and(|waiting| waiting.cancel.load(Ordering::Relaxed))
}

pub(crate) fn forget(session: &mut Session) {
    let results = session.state_mut::<FindResults>();
    if let Some(waiting) = results.waiting.take() {
        waiting.task.finish(Ok(t!("workbench-usages-task-stopped")));
        results.generation += 1;
    }
    for tab in results.tabs.iter().filter(|tab| matches!(tab.source, Source::Usages(_)) && !tab.progress.done()) {
        tab.progress.done.store(true, Ordering::Relaxed);
    }
}

pub(super) fn restart(session: &mut Session, tab: &mut ResultsTab, target: &UsagesTarget) -> Result<(), CommandError> {
    let target = relocated(session, target)?;
    let search = crate::language::usages::start_lookup(session, &target.path, target.offset, false)?;
    tab.source = Source::Usages(target);
    {
        let results = session.state_mut::<FindResults>();
        if let Some(old) = results.waiting.take() {
            old.task.finish(Ok(t!("workbench-usages-task-replaced")));
        }
    }
    tab.lookup = search;
    tab.progress = Arc::new(Progress::default());
    Ok(())
}

pub(crate) fn answered(session: &mut Session, search: u64, answer: Result<Vec<UsagesFile>, String>) {
    let waiting = {
        let results = session.state_mut::<FindResults>();
        match results.waiting.take_if(|waiting| waiting.search == search) {
            Some(waiting) => Some(waiting),
            None if results.tabs.iter().any(|tab| tab.lookup == search) => None,
            None => return,
        }
    };
    let target = match &waiting {
        Some(waiting) => waiting.target.clone(),
        None => {
            let results = session.state_mut::<FindResults>();
            let Some(Source::Usages(target)) =
                results.tabs.iter().find(|tab| tab.lookup == search).map(|tab| tab.source.clone())
            else {
                return;
            };
            target
        }
    };
    let (target, found, error) = match answer {
        Ok(files) => {
            let mut target = target;
            let paths: Vec<PathBuf> = files.iter().map(|file| file.path.clone()).collect();
            let held = paths.iter().take(crate::language::show_usages::DECLARATION_FILES).cloned().collect();
            session.state_mut::<Languages>().hold(HOLDER, held);
            target.kind =
                crate::language::show_usages::kind_at(session, &target.path, target.offset, &target.symbol, &paths);
            let found = collect(session, &target, &files);
            let error = found.is_empty().then(|| t!("workbench-usages-none", scope = scope_label(target.scope)));
            (target, found, error)
        }
        Err(error) => (target, Vec::new(), Some(error)),
    };
    if let Some(waiting) = &waiting {
        waiting.task.finish(Ok(t!("workbench-usages-task-done", count = found.len())));
        if waiting.skip_single && found.len() == 1 {
            let one = &found[0].found;
            let params =
                crate::history::NavigateToParams { path: one.path.clone(), offset: one.range.start, end: None };
            let _ = crate::history::NavigateTo.run(session, params);
            remember(session, &target);
            session.state_mut::<FindResults>().generation += 1;
            session.state_mut::<Languages>().hold(HOLDER, BTreeSet::new());
            return;
        }
    }
    let queue = access_queue(&found);
    let count = found.len() as u64;
    let progress = Arc::new(Progress::default());
    *progress.found.lock().unwrap_or_else(PoisonError::into_inner) = found;
    progress.count.store(count, Ordering::Relaxed);
    *progress.error.lock().unwrap_or_else(PoisonError::into_inner) = error;
    progress.done.store(true, Ordering::Relaxed);
    let id = match waiting {
        Some(waiting) => {
            let id = {
                let results = session.state_mut::<FindResults>();
                results.next += 1;
                results.next
            };
            let tab = ResultsTab {
                id,
                replaces: None,
                source: Source::Usages(target.clone()),
                lookup: search,
                params: params_of(&target),
                replacement: None,
                scope: scope_label(target.scope),
                progress,
                excluded: HashSet::new(),
                collapsed: HashSet::new(),
                selected: None,
                pinned: false,
                number: 1,
            };
            super::place(session.state_mut::<FindResults>(), tab, waiting.new_tab);
            remember(session, &target);
            id
        }
        None => {
            let results = session.state_mut::<FindResults>();
            let Some(tab) = results.tabs.iter_mut().find(|tab| tab.lookup == search) else {
                session.state_mut::<Languages>().hold(HOLDER, BTreeSet::new());
                return;
            };
            tab.source = Source::Usages(target.clone());
            tab.scope = scope_label(target.scope);
            tab.progress = progress;
            results.generation += 1;
            tab.id
        }
    };
    session.state_mut::<UsagesState>().tab = Some(id);
    if !queue.is_empty() {
        let results = session.state_mut::<FindResults>();
        results.access.retain(|access| access.tab != id);
        if let Some(tab) = results.tabs.iter().find(|tab| tab.id == id) {
            tab.progress.classifying.store(true, Ordering::Relaxed);
        }
        results.access.push(Access { tab: id, lookup: search, queue, asked: Vec::new() });
        poll_access(session);
    }
    session.state_mut::<Languages>().hold(HOLDER, BTreeSet::new());
}

fn access_queue(found: &[Found]) -> VecDeque<(PathBuf, usize)> {
    let mut seen = HashSet::new();
    found
        .iter()
        .filter(|one| one.usage_type == UsageType::Unclassified && seen.insert(one.found.path.clone()))
        .map(|one| (one.found.path.clone(), one.found.range.start))
        .take(ACCESS_FILES)
        .collect()
}

pub(crate) struct Access {
    tab: u64,
    lookup: u64,
    queue: VecDeque<(PathBuf, usize)>,
    asked: Vec<Asked>,
}

struct Asked {
    path: PathBuf,
    request: zen_lsp::DocumentRequest,
    at: Instant,
}

pub(crate) fn classifying(session: &Session, id: u64) -> bool {
    session.state::<FindResults>().is_some_and(|results| results.access.iter().any(|access| access.tab == id))
}

pub(crate) fn settle(session: &mut Session, id: u64, until: Instant) {
    loop {
        poll_access(session);
        if !classifying(session, id) || Instant::now() >= until {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn stop_access(session: &mut Session, id: u64) {
    let results = session.state_mut::<FindResults>();
    let mut stopped = Vec::new();
    results.access.retain_mut(|access| {
        if access.tab == id {
            stopped.append(&mut access.asked);
        }
        access.tab != id
    });
    if let Some(tab) = results.tabs.iter().find(|tab| tab.id == id) {
        tab.progress.classifying.store(false, Ordering::Relaxed);
        results.generation += 1;
    }
    cancel(session, &stopped);
    hold_asked(session);
}

pub(crate) fn poll_access(session: &mut Session) {
    if session.state::<FindResults>().is_none_or(|results| results.access.is_empty()) {
        return;
    }
    let results = session.state_mut::<FindResults>();
    let mut accesses = std::mem::take(&mut results.access);
    let live: HashSet<(u64, u64)> = results.tabs.iter().map(|tab| (tab.id, tab.lookup)).collect();
    let now = Instant::now();
    let mut dropped = Vec::new();
    let mut answers = Vec::new();
    for access in &mut accesses {
        if !live.contains(&(access.tab, access.lookup)) {
            access.queue.clear();
            dropped.append(&mut access.asked);
            continue;
        }
        for asked in std::mem::take(&mut access.asked) {
            match asked.request.pending.try_answer() {
                Some(Ok(answer)) => answers.push((access.tab, asked, answer)),
                Some(Err(_)) => {}
                None if now.duration_since(asked.at) >= ACCESS_WAIT => dropped.push(asked),
                None => access.asked.push(asked),
            }
        }
    }
    cancel(session, &dropped);
    let learned = with_languages(session, |languages, _| {
        answers.into_iter().filter_map(|(tab, asked, answer)| told(languages, tab, asked, &answer)).collect::<Vec<_>>()
    });
    let changed = !learned.is_empty();
    let results = session.state_mut::<FindResults>();
    for (id, path, told) in learned {
        let Some(tab) = results.tabs.iter().find(|tab| tab.id == id) else { continue };
        let mut found = tab.progress.found.lock().unwrap_or_else(PoisonError::into_inner);
        for one in found.iter_mut().filter(|one| one.found.path == path && one.usage_type == UsageType::Unclassified) {
            if let Some(access) = told.get(&one.found.range.start) {
                one.access = Some(*access);
                one.usage_type = match access {
                    UsageAccess::Read => UsageType::Read,
                    UsageAccess::Write => UsageType::Write,
                };
            }
        }
    }
    let mut held: BTreeSet<PathBuf> =
        accesses.iter().flat_map(|access| access.asked.iter().map(|asked| asked.path.clone())).collect();
    let mut busy: usize = accesses.iter().map(|access| access.asked.len()).sum();
    for access in &mut accesses {
        while busy < ACCESS_AT_ONCE {
            let Some((path, offset)) = access.queue.pop_front() else { break };
            held.insert(path.clone());
            session.state_mut::<Languages>().hold(ACCESS_HOLDER, held.clone());
            let request = with_languages(session, |languages, workbench| {
                let highlights = languages.client_for(&path).is_some_and(|client| client.document_highlights());
                if !highlights {
                    return None;
                }
                let method = "textDocument/documentHighlight";
                crate::language::request_at(languages, workbench, &path, offset, method, json!({})).ok()
            });
            match request {
                Some(request) => {
                    access.asked.push(Asked { path, request, at: Instant::now() });
                    busy += 1;
                }
                None => {
                    held.remove(&path);
                }
            }
        }
    }
    let (going, over): (Vec<Access>, Vec<Access>) =
        accesses.into_iter().partition(|access| !access.queue.is_empty() || !access.asked.is_empty());
    let results = session.state_mut::<FindResults>();
    for access in &over {
        if let Some(tab) = results.tabs.iter().find(|tab| tab.id == access.tab && tab.lookup == access.lookup) {
            tab.progress.classifying.store(false, Ordering::Relaxed);
        }
    }
    results.access = going;
    if changed || !over.is_empty() {
        results.generation += 1;
    }
    hold_asked(session);
}

fn told(
    languages: &Languages,
    tab: u64,
    asked: Asked,
    answer: &Value,
) -> Option<(u64, PathBuf, HashMap<usize, UsageAccess>)> {
    if languages.server_version(&asked.path) != Some(asked.request.version) {
        return None;
    }
    let text = languages.synced_text(&asked.path)?;
    let told = server_ranges(text, answer)
        .into_iter()
        .filter_map(|range| match range.kind {
            HighlightKind::Read => Some((range.start, UsageAccess::Read)),
            HighlightKind::Write => Some((range.start, UsageAccess::Write)),
            HighlightKind::Text => None,
        })
        .collect();
    Some((tab, asked.path, told))
}

fn cancel(session: &mut Session, asked: &[Asked]) {
    if asked.is_empty() {
        return;
    }
    with_languages(session, |languages, _| {
        for one in asked {
            if let Some(client) = languages.client_for(&one.path) {
                client.cancel(&one.request.pending);
            }
        }
    });
}

fn hold_asked(session: &mut Session) {
    let held = session
        .state::<FindResults>()
        .map(|results| {
            results.access.iter().flat_map(|access| access.asked.iter().map(|asked| asked.path.clone())).collect()
        })
        .unwrap_or_default();
    session.state_mut::<Languages>().hold(ACCESS_HOLDER, held);
}

fn params_of(target: &UsagesTarget) -> SearchParams {
    let mut params = SearchParams::default();
    params.query.text = target.symbol.clone();
    params.query.match_case = true;
    params.query.whole_words = true;
    params
}

fn remember(session: &mut Session, target: &UsagesTarget) {
    let recent = &mut session.state_mut::<FindResults>().recent;
    recent.retain(|old| old != target);
    recent.insert(0, target.clone());
    recent.truncate(RECENT);
}

fn collect(session: &mut Session, target: &UsagesTarget, files: &[UsagesFile]) -> Vec<Found> {
    let workbench = session.state::<Workbench>();
    let project = workbench.and_then(Workbench::project).map(Path::to_owned);
    let open: Vec<PathBuf> = workbench
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default();
    let files: Vec<&UsagesFile> = files
        .iter()
        .filter(|file| {
            crate::language::show_usages::in_scope(
                target.scope,
                &file.path,
                Some(&target.path),
                &open,
                project.as_deref(),
            )
        })
        .collect();
    let source = crate::search::highlighters(session);
    let outliners = session.state::<Outliners>().map(Outliners::all).unwrap_or_default();
    let infos: Infos = Mutex::new(BTreeMap::new());
    let mut found = Vec::new();
    for file in files {
        let Ok(text) = file_text(session.state_mut::<Workbench>(), &file.path) else { continue };
        let ranges: Vec<std::ops::Range<usize>> = file.usages.iter().map(|place| place.start..place.end).collect();
        classify(&file.path, &text, &ranges, source.as_ref(), &outliners, &infos);
        let commented = crate::language::show_usages::comment_ranges(&text);
        let bytes = byte_offsets(&text, &file.usages.iter().map(|place| place.start).collect::<Vec<_>>());
        for (place, byte) in file.usages.iter().zip(bytes) {
            let line = place.line.saturating_sub(1) as usize;
            let one = TextMatch {
                path: file.path.clone(),
                relative: file.relative.clone(),
                line,
                column: place.column.saturating_sub(1) as usize,
                range: place.start..place.end,
                line_text: crate::search::line_text_of(&text, line).unwrap_or_default().to_owned(),
            };
            let (classified, members) = infos
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&(file.path.clone(), place.start))
                .unwrap_or((UsageType::Unclassified, Vec::new()));
            let after = commented.partition_point(|range| range.end <= byte);
            let in_comment = commented.get(after).is_some_and(|range| range.contains(&byte));
            let usage_type = match classified {
                UsageType::Comment | UsageType::String => classified,
                _ if in_comment => UsageType::Comment,
                _ if crate::language::show_usages::in_import(&text, byte) => UsageType::Import,
                _ => UsageType::Unclassified,
            };
            found.push(Found { found: one, usage_type, members, access: None, holder: None });
        }
    }
    found
}

fn byte_offsets(text: &str, chars: &[usize]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..chars.len()).collect();
    order.sort_by_key(|&index| chars[index]);
    let mut bytes = vec![text.len(); chars.len()];
    let mut next = 0;
    for (count, (byte, _)) in text.char_indices().enumerate() {
        while next < order.len() && chars[order[next]] <= count {
            bytes[order[next]] = byte;
            next += 1;
        }
        if next == order.len() {
            break;
        }
    }
    bytes
}

pub(super) fn file_occurrences(session: &mut Session, tab: &mut ResultsTab, path: &Path) -> Result<(), CommandError> {
    let query = tab.params.query.clone();
    let (text, ranges) = {
        let workbench =
            session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let highlighters = session.state::<crate::brackets::Highlighters>();
        match workbench.documents().iter().find(|document| document.path() == path) {
            Some(document) => {
                let ranges = crate::find::find_matches(highlighters, document, &query)
                    .map_err(|error| CommandError::new(t!("workbench-error-search", error = error)))?;
                (document.text().to_owned(), ranges)
            }
            None => {
                let text = file_text(workbench, path).map_err(CommandError::new)?;
                let ranges = zen_text::search::find_all(&text, &query)
                    .map_err(|error| CommandError::new(t!("workbench-error-search", error = error)))?;
                (text, ranges)
            }
        }
    };
    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let relative = match project.as_deref().and_then(|root| path.strip_prefix(root).ok()) {
        Some(inside) => {
            inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
        }
        None => path.display().to_string(),
    };
    let source = crate::search::highlighters(session);
    let outliners = session.state::<Outliners>().map(Outliners::all).unwrap_or_default();
    let infos: Infos = Mutex::new(BTreeMap::new());
    classify(path, &text, &ranges, source.as_ref(), &outliners, &infos);
    let starts = super::line_starts(&text);
    let mut infos = infos.lock().unwrap_or_else(PoisonError::into_inner);
    let found: Vec<Found> = ranges
        .into_iter()
        .map(|range| {
            let line = starts.partition_point(|at| *at <= range.start).saturating_sub(1);
            let (usage_type, members) =
                infos.remove(&(path.to_owned(), range.start)).unwrap_or((UsageType::Unclassified, Vec::new()));
            Found {
                found: TextMatch {
                    path: path.to_owned(),
                    relative: relative.clone(),
                    line,
                    column: range.start - starts[line],
                    range,
                    line_text: crate::search::line_text_of(&text, line).unwrap_or_default().to_owned(),
                },
                usage_type,
                members,
                access: None,
                holder: None,
            }
        })
        .collect();
    let progress = Arc::new(Progress::default());
    progress.count.store(found.len() as u64, Ordering::Relaxed);
    if found.is_empty() {
        *progress.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(t!("workbench-find-no-occurrences"));
    }
    *progress.found.lock().unwrap_or_else(PoisonError::into_inner) = found;
    progress.done.store(true, Ordering::Relaxed);
    tab.progress = progress;
    Ok(())
}

pub(crate) fn open_file_tab(
    session: &mut Session,
    path: &Path,
    query: zen_text::search::Query,
) -> Result<u64, CommandError> {
    let id = {
        let results = session.state_mut::<FindResults>();
        results.next += 1;
        results.next
    };
    let params = SearchParams { query, ..SearchParams::default() };
    let mut tab = ResultsTab {
        id,
        replaces: None,
        source: Source::File(path.to_owned()),
        lookup: 0,
        params,
        replacement: None,
        scope: String::new(),
        progress: Arc::new(Progress::default()),
        excluded: HashSet::new(),
        collapsed: HashSet::new(),
        selected: None,
        pinned: false,
        number: 1,
    };
    file_occurrences(session, &mut tab, path)?;
    super::place(session.state_mut::<FindResults>(), tab, false);
    Ok(id)
}

/// A Find Usages done before, as Recent Find Usages lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecentUsage {
    /// Its place in the list, newest first, which `navigate.usages.openRecent` takes.
    pub index: usize,
    /// What is shown: "Variable Version".
    pub title: String,
    /// The file of the search, relative to the project.
    pub relative: String,
    /// The scope's name.
    pub scope_name: String,
    /// What Find Usages looked for.
    #[serde(flatten)]
    pub target: UsagesTarget,
}

/// What `navigate.usages.recent` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecentUsages {
    /// The searches, newest first.
    pub searches: Vec<RecentUsage>,
}

pub struct RecentFindUsages;

impl Command for RecentFindUsages {
    type Params = NoParams;
    type Output = RecentUsages;
    const ID: &'static str = "navigate.usages.recent";
    const TITLE: &'static str = "Recent Find Usages";
    const DESCRIPTION: &'static str = "The Find Usages done in this window, newest first, as Edit → Find Usages → Recent \
         Find Usages lists them; navigate.usages.openRecent runs one again.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RecentUsages, CommandError> {
        let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let recent = session.state::<FindResults>().map(|results| results.recent.clone()).unwrap_or_default();
        let searches = recent
            .into_iter()
            .enumerate()
            .map(|(index, target)| RecentUsage {
                index,
                title: target_label(&target),
                relative: match project.as_deref().and_then(|root| target.path.strip_prefix(root).ok()) {
                    Some(inside) => inside.to_string_lossy().replace('\\', "/"),
                    None => target.path.display().to_string(),
                },
                scope_name: scope_label(target.scope),
                target,
            })
            .collect();
        Ok(RecentUsages { searches })
    }
}

/// The parameters of `navigate.usages.openRecent`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenRecentParams {
    /// The search, by its index in navigate.usages.recent; the newest when absent.
    pub index: usize,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    pub wait_ms: Option<u64>,
}

pub struct OpenRecentFindUsages;

impl Command for OpenRecentFindUsages {
    type Params = OpenRecentParams;
    type Output = UsagesState;
    const ID: &'static str = "navigate.usages.openRecent";
    const TITLE: &'static str = "Open Recent Find Usages";
    const DESCRIPTION: &'static str = "Runs a Find Usages of Recent Find Usages again (by `index`, newest 0) and lists \
         its usages in the Find tool window; if the file changed, the symbol is looked for near where it was.";

    fn run(&self, session: &mut Session, params: OpenRecentParams) -> Result<UsagesState, CommandError> {
        let target = session
            .state::<FindResults>()
            .and_then(|results| results.recent.get(params.index).cloned())
            .ok_or_else(|| CommandError::new(t!("workbench-usages-no-recent")))?;
        let target = relocated(session, &target)?;
        let find = FindUsagesParams {
            path: Some(target.path),
            offset: Some(target.offset),
            wait_ms: params.wait_ms,
            scope: Some(target.scope),
            ..FindUsagesParams::default()
        };
        FindUsages.run(session, find)
    }
}

pub(crate) fn line_context(text: &str, offset: usize) -> (String, usize) {
    let at = crate::language::byte_offset(text, offset);
    let start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let end = text[at..].find('\n').map_or(text.len(), |newline| at + newline);
    let line = &text[start..end];
    let indent = start + (line.len() - line.trim_start().len());
    let column = if at > indent { text[indent..at].chars().count() } else { 0 };
    (line.trim().to_owned(), column)
}

fn relocated(session: &mut Session, target: &UsagesTarget) -> Result<UsagesTarget, CommandError> {
    let text = file_text(session.state_mut::<Workbench>(), &target.path).map_err(CommandError::new)?;
    let offset = nearest(&text, target)
        .ok_or_else(|| CommandError::new(t!("workbench-usages-recent-gone", symbol = target.symbol.as_str())))?;
    let (line, column) = line_context(&text, offset);
    Ok(UsagesTarget { offset, line, column, ..target.clone() })
}

pub(crate) fn nearest(text: &str, target: &UsagesTarget) -> Option<usize> {
    let (symbol, offset) = (target.symbol.as_str(), target.offset);
    let here = word_at(text, offset).as_deref() == Some(symbol);
    let same_line = |at: usize| !target.line.is_empty() && line_context(text, at).0 == target.line;
    if here && (target.line.is_empty() || same_line(offset)) {
        return Some(offset);
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let found: Vec<usize> = text
        .match_indices(symbol)
        .filter(|(at, _)| {
            let open = text[..*at].chars().next_back().is_none_or(|c| !is_word(c));
            let close = text[at + symbol.len()..].chars().next().is_none_or(|c| !is_word(c));
            open && close
        })
        .map(|(at, _)| crate::language::char_offset(text, at))
        .collect();
    let on_its_line = found
        .iter()
        .copied()
        .filter(|at| same_line(*at))
        .min_by_key(|at| (line_context(text, *at).1.abs_diff(target.column), at.abs_diff(offset)));
    if on_its_line.is_some() {
        return on_its_line;
    }
    if here {
        return Some(offset);
    }
    found.into_iter().min_by_key(|at| at.abs_diff(offset))
}

#[cfg(test)]
mod tests;

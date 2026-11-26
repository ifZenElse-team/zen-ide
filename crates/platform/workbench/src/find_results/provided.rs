use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{CommandError, Session};
use zen_i18n::t;
use zen_index::TextMatch;

use super::{FindResults, Found, Key, Progress, ResultsTab, Source, TASK_SOURCE, UsageType};
use crate::Workbench;
use crate::language::show_usages::UsageAccess;
use crate::search::SearchParams;
use crate::tasks::Tasks;

const PROGRESS_EVERY: Duration = Duration::from_millis(80);

/// A command a node of the results tree runs when it is opened, for a usage that is in no file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeCommand {
    /// The command's id.
    pub command: String,
    /// Its parameters.
    pub params: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Elsewhere {
    pub group: String,
    pub holder: String,
    pub key: String,
    pub text: String,
    pub range: Range<usize>,
    pub access: Option<UsageAccess>,
    pub open: NodeCommand,
}

pub trait UsageFinder: Send + Sync {
    fn title(&self) -> String;

    fn target(&self) -> String;

    fn scope(&self) -> String;

    fn request(&self) -> serde_json::Value;

    fn search(&self) -> SearchParams;

    fn extra(&self) -> Vec<(PathBuf, String)>;

    fn keep(&self, path: &Path, text: &str, ranges: Vec<Range<usize>>) -> Vec<(Range<usize>, Option<UsageAccess>)>;

    fn elsewhere(&self, cancel: &AtomicBool) -> Vec<Elsewhere>;
}

#[derive(Clone)]
pub(super) struct Finder(pub Arc<dyn UsageFinder>);

impl std::fmt::Debug for Finder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Finder").field(&self.0.title()).finish()
    }
}

impl PartialEq for Finder {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Holder {
    pub group: String,
    pub holder: String,
    pub key: String,
    pub open: NodeCommand,
}

fn usage_type(access: Option<UsageAccess>) -> UsageType {
    match access {
        Some(UsageAccess::Read) => UsageType::Read,
        Some(UsageAccess::Write) => UsageType::Write,
        None => UsageType::Unclassified,
    }
}

fn elsewhere_found(one: Elsewhere) -> Found {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(one.text.chars().enumerate().filter(|(_, c)| *c == '\n').map(|(at, _)| at + 1))
        .collect();
    let line = starts.partition_point(|start| *start <= one.range.start).saturating_sub(1);
    let line_text = one.text.split('\n').nth(line).unwrap_or_default().trim_end_matches('\r').to_owned();
    Found {
        found: TextMatch {
            path: PathBuf::from(&one.key),
            relative: one.key.clone(),
            line,
            column: one.range.start - starts[line],
            range: one.range,
            line_text,
        },
        usage_type: usage_type(one.access),
        members: Vec::new(),
        access: one.access,
        holder: Some(Arc::new(Holder { group: one.group, holder: one.holder, key: one.key, open: one.open })),
    }
}

pub(super) fn start(
    session: &mut Session,
    tab: &mut ResultsTab,
    finder: &Arc<dyn UsageFinder>,
) -> Result<(), CommandError> {
    let params = finder.search();
    let mut search = params.search(session, 0)?;
    search.limit = 0;
    search.extra.extend(finder.extra());
    tab.params = params;
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let index = crate::search::index(workbench)?;
    let open = crate::search::open_texts(workbench);
    let progress = Arc::new(Progress::default());
    tab.progress = Arc::clone(&progress);
    let task = session.state_mut::<Tasks>().start(&finder.title(), TASK_SOURCE, false);
    task.cancel_with(Arc::clone(&progress.cancel));
    let finder = Arc::clone(finder);
    let run = move || {
        let kinds: Arc<Mutex<BTreeMap<Key, Option<UsageAccess>>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let kept_kinds = Arc::clone(&kinds);
        let keeper = Arc::clone(&finder);
        let keeping = move |path: &Path, text: &str, ranges: Vec<Range<usize>>| {
            let kept = keeper.keep(path, text, ranges);
            let mut kinds = kept_kinds.lock().unwrap_or_else(PoisonError::into_inner);
            kept.into_iter()
                .map(|(range, access)| {
                    kinds.insert((path.to_owned(), range.start), access);
                    range
                })
                .collect::<Vec<_>>()
        };
        let shown = Mutex::new(Instant::now() - PROGRESS_EVERY);
        let visit = |relative: &str| {
            *progress.current.lock().unwrap_or_else(PoisonError::into_inner) = relative.to_owned();
            let mut shown = shown.lock().unwrap_or_else(PoisonError::into_inner);
            if shown.elapsed() >= PROGRESS_EVERY {
                *shown = Instant::now();
                task.progress(None, &t!("workbench-results-task-in", file = relative));
            }
        };
        let sink = |matches: Vec<TextMatch>| {
            let count = {
                let mut kinds = kinds.lock().unwrap_or_else(PoisonError::into_inner);
                let mut found = progress.found.lock().unwrap_or_else(PoisonError::into_inner);
                for one in matches {
                    let access = kinds.remove(&(one.path.clone(), one.range.start)).flatten();
                    found.push(Found {
                        found: one,
                        usage_type: usage_type(access),
                        members: Vec::new(),
                        access,
                        holder: None,
                    });
                }
                found.len()
            };
            progress.count.fetch_max(count as u64, Ordering::Relaxed);
        };
        let result = index.search_text_visiting(&search, &open, &progress.cancel, &sink, &keeping, &visit);
        if result.is_ok() && !progress.cancel.load(Ordering::Relaxed) {
            *progress.current.lock().unwrap_or_else(PoisonError::into_inner) = String::new();
            let more: Vec<Found> = finder.elsewhere(&progress.cancel).into_iter().map(elsewhere_found).collect();
            let mut found = progress.found.lock().unwrap_or_else(PoisonError::into_inner);
            found.extend(more);
            progress.count.store(found.len() as u64, Ordering::Relaxed);
        }
        {
            let mut found = progress.found.lock().unwrap_or_else(PoisonError::into_inner);
            found.sort_by(|a, b| {
                (a.holder.is_some(), &a.found.relative, a.found.line, a.found.column).cmp(&(
                    b.holder.is_some(),
                    &b.found.relative,
                    b.found.line,
                    b.found.column,
                ))
            });
        }
        let count = progress.count.load(Ordering::Relaxed);
        match result {
            Err(error) => {
                *progress.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(error.clone());
                task.finish(Err(error));
            }
            Ok(_) if progress.cancel.load(Ordering::Relaxed) => {
                task.finish(Ok(t!("workbench-results-task-stopped", count = count)));
            }
            Ok(_) => task.finish(Ok(t!("workbench-results-task-done", count = count))),
        }
        progress.done.store(true, Ordering::Relaxed);
        progress.current.lock().unwrap_or_else(PoisonError::into_inner).clear();
        task.progress(None, "");
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(run);
    #[cfg(target_arch = "wasm32")]
    run();
    Ok(())
}

pub fn open_tab(
    session: &mut Session,
    finder: Arc<dyn UsageFinder>,
    replacement: Option<String>,
    new_tab: bool,
) -> Result<u64, CommandError> {
    let id = {
        let results = session.state_mut::<FindResults>();
        results.next += 1;
        results.next
    };
    let mut tab = ResultsTab {
        id,
        replaces: None,
        scope: finder.scope(),
        source: Source::Provided(Finder(Arc::clone(&finder))),
        lookup: 0,
        params: SearchParams::default(),
        replacement,
        progress: Arc::new(Progress::default()),
        excluded: HashSet::new(),
        collapsed: HashSet::new(),
        selected: None,
        pinned: false,
        number: 1,
    };
    start(session, &mut tab, &finder)?;
    super::place(session.state_mut::<FindResults>(), tab, new_tab);
    Ok(id)
}

pub fn wait(session: &Session, id: u64, wait: Duration) -> bool {
    let Some(progress) = session
        .state::<FindResults>()
        .and_then(|results| results.tabs.iter().find(|tab| tab.id == id))
        .map(|tab| Arc::clone(&tab.progress))
    else {
        return false;
    };
    let until = Instant::now() + wait;
    while !progress.done() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    progress.done()
}

/// A usage of a tab, as an answer for agents lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    /// The file, relative to the project when in it; none for a usage in no file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// What holds a usage in no file, e.g. `VIEW public.active_orders`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    /// The line, one-based.
    pub line: usize,
    /// The column, one-based, in characters.
    pub column: usize,
    /// The line's text, trimmed, at most 200 characters.
    pub text: String,
    /// `read`, `write`, or none when not known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<UsageAccess>,
}

pub fn rows(session: &Session, id: u64, limit: usize) -> Option<(Vec<UsageRow>, usize, bool)> {
    let tab = session.state::<FindResults>()?.tabs.iter().find(|tab| tab.id == id)?;
    let project = session.state::<Workbench>().and_then(Workbench::project);
    let found = tab.progress.found();
    let rows = found
        .iter()
        .take(limit)
        .map(|one| {
            let shown = match (&one.holder, project.and_then(|root| one.found.path.strip_prefix(root).ok())) {
                (Some(_), _) => None,
                (None, Some(inside)) => Some(inside.to_string_lossy().replace('\\', "/")),
                (None, None) => Some(one.found.relative.clone()),
            };
            UsageRow {
                path: shown,
                holder: one.holder.as_ref().map(|holder| holder.holder.clone()),
                line: one.found.line + 1,
                column: one.found.column + 1,
                text: one.found.line_text.trim().chars().take(200).collect(),
                access: one.access,
            }
        })
        .collect();
    Some((rows, found.len(), tab.progress.done()))
}

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Session};
use zen_i18n::t;
use zen_project::inspect::{FixKind, FixState, InspectionFix};

use super::{InspectCode, ResultItem, SOURCE, run_of};
use crate::inspections::{FixStatus, Inspections, start_fixes};
use crate::language::edits::Applied;
use crate::language::intentions::{
    ApplyIntention, IntentionApplyParams, IntentionList, IntentionsListParams, ListIntentions,
};
use crate::language::refactor::{JournalEntry, Refactoring, merge_undo, undo_recorded};
use crate::language::{Languages, Problem, Reason, with_languages};
use crate::notifications::{Action, Group, Kind, NewNotification};
use crate::suppression::SuppressScope;
use crate::tasks::{TaskHandle, Tasks};

const HOLD: &str = "inspectFixes";
const CHECK_LIMIT: Duration = Duration::from_secs(10);
const ANSWER_LIMIT: Duration = Duration::from_secs(10);
const REPORT_MARGIN: Duration = Duration::from_secs(1);
const APPLY_WAIT_MS: u64 = 3000;
const CLEANUP_RUN: u64 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Signature {
    source: Option<String>,
    code: Option<String>,
    message: String,
}

impl Signature {
    fn of(problem: &Problem) -> Self {
        Self { source: problem.source.clone(), code: problem.code.clone(), message: problem.message.clone() }
    }

    fn is(&self, problem: &Problem) -> bool {
        self.source == problem.source && self.code == problem.code && self.message == problem.message
    }
}

#[derive(Clone, Debug)]
struct ServerFix {
    path: PathBuf,
    signature: Signature,
    wanted: Option<String>,
    run: u64,
    item: u64,
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Next,
    Checking(Instant),
    Asking(Instant, usize),
}

#[derive(Default)]
pub(super) struct Fixing {
    queue: VecDeque<ServerFix>,
    phase: Phase,
    task: Option<TaskHandle>,
    cancel: Option<Arc<AtomicBool>>,
    fixed: usize,
    skipped: usize,
    jobs: Vec<(u64, u64, Vec<u64>)>,
    cleaning: Option<BTreeMap<PathBuf, Vec<Applied>>>,
    outcome: Option<CleanupOutcome>,
    undoable: BTreeMap<PathBuf, JournalEntry>,
    job_files: usize,
}

impl Fixing {
    pub(super) fn running(&self) -> bool {
        self.task.is_some() || !self.jobs.is_empty()
    }

    pub(super) fn outcome(&self) -> Option<CleanupOutcome> {
        self.outcome.clone()
    }
}

/// What a Code Cleanup did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    /// How many problems were fixed.
    pub fixed: usize,
    /// How many were left, each with why in the task's log.
    pub skipped: usize,
    /// How many files changed.
    pub files: usize,
}

/// A fix offered for an item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemFix {
    /// What inspect.results.fix takes as `fix`.
    pub id: String,
    /// What it says.
    pub title: String,
    /// The window action it runs, for a fix that is one (agents use run_action).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<serde_json::Value>,
}

/// What `inspect.results.fixes` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemFixList {
    /// Whether the language server has not answered yet: ask again.
    pub pending: bool,
    /// The fixes, the main one first.
    pub fixes: Vec<ItemFix>,
}

/// The parameters of `inspect.results.fixes`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemFixesParams {
    /// The run.
    pub run: u64,
    /// The item.
    pub item: u64,
    /// How long to wait for the language server in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

fn item_of(session: &Session, run: u64, id: u64) -> Result<ResultItem, CommandError> {
    let item = run_of(session, run)?
        .items
        .iter()
        .find(|item| item.id == id)
        .cloned()
        .ok_or_else(|| CommandError::new(t!("workbench-inspect-unknown-item", item = id.to_string())))?;
    crate::hidden_files::guard_read(session, std::slice::from_ref(&item.path))?;
    Ok(item)
}

fn offered(fix: &InspectionFix) -> Option<ItemFix> {
    match fix.kind {
        FixKind::Apply => Some(ItemFix { id: fix.id.clone(), title: fix.title.clone(), action: None }),
        FixKind::Action => {
            Some(ItemFix { id: fix.id.clone(), title: fix.title.clone(), action: Some(fix.args.clone()) })
        }
        FixKind::Batch | FixKind::Details | FixKind::Heading => None,
    }
}

fn server_fixes(list: IntentionList) -> ItemFixList {
    let mut fixes: Vec<ItemFix> = Vec::new();
    for action in list.actions.into_iter().filter(|action| action.quick_fix && action.disabled.is_none()) {
        if !fixes.iter().any(|fix| fix.title == action.title) {
            fixes.push(ItemFix { id: action.title.clone(), title: action.title, action: None });
        }
    }
    ItemFixList { pending: list.pending, fixes }
}

pub struct ItemFixes;

impl Command for ItemFixes {
    type Params = ItemFixesParams;
    type Output = ItemFixList;
    const ID: &'static str = "inspect.results.fixes";
    const TITLE: &'static str = "Inspection Item Fixes";
    const DESCRIPTION: &'static str = "The quick-fixes of an item of a run: the inspector's own, or those its language server \
         offers at the problem, which it opens the file for. Call again with waitMs while pending; apply one with \
         inspect.results.fix.";

    fn run(&self, session: &mut Session, params: ItemFixesParams) -> Result<ItemFixList, CommandError> {
        let item = item_of(session, params.run, params.item)?;
        let mut list = own_fixes(session, &item, params.wait_ms)?;
        if !list.pending {
            list.fixes.extend(suppressions(session, params.run, &item));
        }
        Ok(list)
    }
}

fn own_fixes(session: &mut Session, item: &ResultItem, wait_ms: Option<u64>) -> Result<ItemFixList, CommandError> {
    if !item.problem.fixes.is_empty() {
        let fixes = item.problem.fixes.iter().filter_map(offered).collect();
        return Ok(ItemFixList { pending: false, fixes });
    }
    let serves = session.state::<Languages>().is_some_and(|languages| languages.serves(&item.path));
    if !serves {
        return Ok(ItemFixList::default());
    }
    let list = ListIntentions.run(
        session,
        IntentionsListParams { path: Some(item.path.clone()), offset: Some(item.problem.start), wait_ms },
    )?;
    Ok(server_fixes(list))
}

fn suppressions(session: &mut Session, run: u64, item: &ResultItem) -> Vec<ItemFix> {
    crate::suppression::item_options(session, item)
        .into_iter()
        .map(|option| {
            let params = json!({ "run": run, "item": item.id, "scope": option.scope });
            ItemFix {
                id: option.title.clone(),
                title: option.title,
                action: Some(json!({ "action": SuppressItem::ID, "params": params })),
            }
        })
        .collect()
}

/// The parameters of `inspect.results.fix`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FixParams {
    /// The run.
    pub run: u64,
    /// The items to fix, e.g. all of a file's for Fix All in File.
    pub items: Vec<u64>,
    /// The fix to apply (an id from inspect.results.fixes), for one item; each item's main fix when absent.
    #[serde(default)]
    pub fix: Option<String>,
}

/// What `inspect.results.fix` started.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FixStarted {
    /// The inspectors' jobs started, for problems.fix.status.
    pub jobs: Vec<u64>,
    /// How many items wait for their language server's fix.
    pub server_fixes: usize,
    /// How many items have no fix to apply.
    pub skipped: usize,
}

fn main_fix(item: &ResultItem, wanted: Option<&str>) -> Option<InspectionFix> {
    let mut applied = item.problem.fixes.iter().filter(|fix| fix.kind == FixKind::Apply);
    match wanted {
        Some(wanted) => applied.find(|fix| fix.id == wanted).cloned(),
        None => applied.next().cloned(),
    }
}

fn queue_server_fixes(session: &mut Session, fixes: Vec<ServerFix>) {
    if fixes.is_empty() {
        return;
    }
    let count = fixes.len();
    let task = session.state_mut::<Tasks>().start(&t!("workbench-inspect-fix-task", count = count), SOURCE, false);
    let cancel = Arc::new(AtomicBool::new(false));
    task.cancel_with(Arc::clone(&cancel));
    let fixing = &mut session.state_mut::<InspectCode>().fixing;
    fixing.queue.extend(fixes);
    fixing.task = Some(task);
    fixing.cancel = Some(cancel);
}

pub struct FixItems;

impl Command for FixItems {
    type Params = FixParams;
    type Output = FixStarted;
    const ID: &'static str = "inspect.results.fix";
    const TITLE: &'static str = "Fix Inspection Items";
    const DESCRIPTION: &'static str = "Applies a quick-fix to items of a run in the background: the one named, or each item's \
         main fix (Fix All in File). Inspectors' fixes are jobs of problems.fix.status; language servers' fixes are \
         made one by one and undone like any context action; inspect.code.status moves them along. Fixed items leave \
         the results; items with no fix stay and are counted as skipped.";

    fn run(&self, session: &mut Session, params: FixParams) -> Result<FixStarted, CommandError> {
        let mut items = Vec::new();
        for id in &params.items {
            items.push(item_of(session, params.run, *id)?);
        }
        fix_items(session, params.run, items, params.fix.as_deref())
    }
}

fn fix_items(
    session: &mut Session,
    run: u64,
    items: Vec<ResultItem>,
    wanted: Option<&str>,
) -> Result<FixStarted, CommandError> {
    let mut by_file: BTreeMap<PathBuf, (Vec<InspectionFix>, Vec<u64>)> = BTreeMap::new();
    let mut server = Vec::new();
    let mut started = FixStarted::default();
    for item in items {
        if let Some(fix) = main_fix(&item, wanted) {
            let (fixes, ids) = by_file.entry(item.path.clone()).or_default();
            if !fixes.iter().any(|kept| kept.id == fix.id) {
                fixes.push(fix);
            }
            ids.push(item.id);
        } else if item.problem.fixes.is_empty()
            && session.state::<Languages>().is_some_and(|languages| languages.serves(&item.path))
        {
            server.push(ServerFix {
                path: item.path.clone(),
                signature: Signature::of(&item.problem),
                wanted: wanted.map(str::to_owned),
                run,
                item: item.id,
            });
        } else {
            started.skipped += 1;
        }
    }
    for (path, (fixes, items)) in by_file {
        let job = start_fixes(session, path, fixes)?;
        session.state_mut::<InspectCode>().fixing.jobs.push((job.job, run, items));
        started.jobs.push(job.job);
    }
    started.server_fixes = server.len();
    queue_server_fixes(session, server);
    Ok(started)
}

pub(super) fn clean(session: &mut Session, items: Vec<ResultItem>) {
    let total = items.len();
    let fixing = &mut session.state_mut::<InspectCode>().fixing;
    fixing.outcome = None;
    fixing.cleaning.get_or_insert_default();
    let started = fix_items(session, CLEANUP_RUN, items, None);
    let fixing = &mut session.state_mut::<InspectCode>().fixing;
    match started {
        Ok(started) => {
            fixing.skipped += started.skipped;
            fixing.fixed += total - started.server_fixes - started.skipped;
            fixing.job_files += started.jobs.len();
        }
        Err(error) => {
            fixing.skipped += total;
            let text = error.message;
            crate::notifications::post(session, NewNotification::message(Group::Errors, Kind::Error, text));
        }
    }
    if !session.state::<InspectCode>().is_some_and(|state| state.fixing.task.is_some()) {
        end_cleanup(session);
    }
}

fn end_cleanup(session: &mut Session) {
    let fixing = &mut session.state_mut::<InspectCode>().fixing;
    let Some(logs) = fixing.cleaning.take() else { return };
    let outcome = CleanupOutcome { fixed: fixing.fixed, skipped: fixing.skipped, files: logs.len() + fixing.job_files };
    fixing.fixed = 0;
    fixing.skipped = 0;
    fixing.job_files = 0;
    let undoes = !logs.is_empty();
    fixing.outcome = Some(outcome.clone());
    let undoable = logs.into_iter().map(|(path, log)| (path, merge_undo(session, log))).collect();
    session.state_mut::<InspectCode>().fixing.undoable = undoable;
    let text = t!("workbench-cleanup-done", fixed = outcome.fixed, skipped = outcome.skipped, files = outcome.files);
    let mut note = NewNotification::message(Group::General, Kind::Info, text);
    if undoes {
        note = note.action(Action::new("undo", t!("workbench-cleanup-undo"), UndoCleanup::ID, json!({})));
    }
    crate::notifications::post(session, note);
}

/// What `inspect.cleanup.undo` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UndoCleanupParams {
    /// The file whose change to undo; every file of the last Code Cleanup when absent.
    pub path: Option<PathBuf>,
}

pub struct UndoCleanup;

impl Command for UndoCleanup {
    type Params = UndoCleanupParams;
    type Output = usize;
    const ID: &'static str = "inspect.cleanup.undo";
    const TITLE: &'static str = "Undo Code Cleanup";
    const DESCRIPTION: &'static str = "Undoes the change the last Code Cleanup made to a file, or to every file it changed, \
         which also reaches files that are not open. A file changed since is left alone and named in a message. Returns \
         how many files went back.";

    fn run(&self, session: &mut Session, params: UndoCleanupParams) -> Result<usize, CommandError> {
        let undoable = &mut session.state_mut::<InspectCode>().fixing.undoable;
        let entries: Vec<JournalEntry> = match &params.path {
            Some(path) => undoable.remove(path).into_iter().collect(),
            None => std::mem::take(undoable).into_values().collect(),
        };
        if entries.is_empty() {
            return Err(CommandError::new(t!("workbench-cleanup-nothing-to-undo")));
        }
        Ok(entries.iter().filter(|entry| undo_recorded(session, entry)).count())
    }
}

/// The parameters of `inspect.results.suppress`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuppressItemParams {
    /// The run.
    pub run: u64,
    /// The item.
    pub item: u64,
    /// How far it reaches.
    pub scope: SuppressScope,
}

pub struct SuppressItem;

impl Command for SuppressItem {
    type Params = SuppressItemParams;
    type Output = usize;
    const ID: &'static str = "inspect.results.suppress";
    const TITLE: &'static str = "Suppress Inspection Item";
    const DESCRIPTION: &'static str = "Suppresses an item of a run as inspect.suppress.apply does, for its statement, function \
         or file. It and the items of the same inspection the suppression covers leave the results. Returns how many left.";

    fn run(&self, session: &mut Session, params: SuppressItemParams) -> Result<usize, CommandError> {
        let item = item_of(session, params.run, params.item)?;
        let written = crate::suppression::write(session, &item.path, &item.problem, params.scope)?;
        let state = session.state_mut::<InspectCode>();
        let results = state.run_mut(params.run)?;
        let before = results.items.len();
        let covered = written.as_ref().map(|written| written.covered.clone());
        results.items.retain(|kept| {
            kept.id != item.id
                && !(kept.path == item.path
                    && kept.inspection == item.inspection
                    && covered.as_ref().is_some_and(|covered| covered.contains(&kept.problem.line)))
        });
        if let Some(written) = &written {
            for kept in results.items.iter_mut().filter(|kept| kept.path == item.path) {
                if kept.problem.start >= written.at_char {
                    kept.problem.start += written.chars;
                    kept.problem.end += written.chars;
                    kept.problem.line += written.lines;
                }
            }
        }
        let left = before - results.items.len();
        state.revision += 1;
        Ok(left)
    }
}

fn forget_items(session: &mut Session, run: u64, items: &[u64]) {
    let state = session.state_mut::<InspectCode>();
    if let Ok(results) = state.run_mut(run) {
        results.items.retain(|item| !items.contains(&item.id));
    }
    state.revision += 1;
}

fn step_jobs(session: &mut Session) {
    if session.state::<InspectCode>().is_none_or(|state| state.fixing.jobs.is_empty()) {
        return;
    }
    let _ = FixStatus.run(session, NoParams::default());
    let jobs = std::mem::take(&mut session.state_mut::<InspectCode>().fixing.jobs);
    let mut kept = Vec::new();
    for (job, run, items) in jobs {
        match session.state::<Inspections>().and_then(|inspections| inspections.job_state(job)) {
            Some(FixState::Running { .. }) => kept.push((job, run, items)),
            Some(FixState::Done { .. }) => forget_items(session, run, &items),
            _ => {}
        }
    }
    session.state_mut::<InspectCode>().fixing.jobs = kept;
}

fn end_batch(session: &mut Session) {
    let fixing = &mut session.state_mut::<InspectCode>().fixing;
    fixing.queue.clear();
    fixing.phase = Phase::Next;
    fixing.cancel = None;
    let (task, fixed, skipped) = (fixing.task.take(), fixing.fixed, fixing.skipped);
    if fixing.cleaning.is_none() {
        fixing.fixed = 0;
        fixing.skipped = 0;
    }
    if let Some(task) = task {
        task.finish(Ok(t!("workbench-inspect-fix-done", fixed = fixed, skipped = skipped)));
    }
    end_cleanup(session);
    with_languages(session, |languages, workbench| {
        languages.hold(HOLD, BTreeSet::new());
        languages.sync(workbench);
    });
}

fn checked(session: &mut Session, path: &std::path::Path) -> bool {
    with_languages(session, |languages, workbench| {
        languages.sync(workbench);
        let version =
            workbench.documents().iter().find(|document| document.path() == path).map(crate::Document::version);
        !matches!(languages.analysis(path, version), Some(Reason::Checking | Reason::NotChecked))
    })
}

fn settle_time(session: &Session, fix: &ServerFix) -> Duration {
    Duration::from_millis(session.state::<Languages>().map_or(0, |languages| languages.settle_ms(&fix.path)))
}

fn offset_of(session: &Session, fix: &ServerFix) -> Option<usize> {
    let languages = session.state::<Languages>()?;
    languages.problems(&fix.path).iter().find(|problem| fix.signature.is(problem)).map(|problem| problem.start)
}

fn held(session: &mut Session) {
    let paths: BTreeSet<PathBuf> = session
        .state::<InspectCode>()
        .map(|state| state.fixing.queue.iter().map(|fix| fix.path.clone()).collect())
        .unwrap_or_default();
    with_languages(session, |languages, workbench| {
        languages.sync(workbench);
        for path in &paths {
            if workbench.documents().iter().all(|document| document.path() != path) {
                let _ = languages.lend(path);
            }
        }
        languages.hold(HOLD, paths.clone());
    });
}

fn skip(session: &mut Session, why: &str) {
    let fixing = &mut session.state_mut::<InspectCode>().fixing;
    if let (Some(fix), Some(task)) = (fixing.queue.pop_front(), &fixing.task) {
        task.log(&format!("{}: {why}", fix.signature.message));
    }
    fixing.skipped += 1;
    fixing.phase = Phase::Next;
}

fn ask(session: &mut Session, fix: &ServerFix, offset: usize) {
    let params = IntentionsListParams { path: Some(fix.path.clone()), offset: Some(offset), wait_ms: None };
    let list = match ListIntentions.run(session, params) {
        Ok(list) => list,
        Err(error) => return skip(session, &error.message),
    };
    if list.pending {
        return;
    }
    let quick: Vec<_> =
        list.actions.into_iter().filter(|action| action.quick_fix && action.disabled.is_none()).collect();
    let chosen = match &fix.wanted {
        Some(title) => quick.iter().find(|action| action.title == *title),
        None => quick.iter().find(|action| action.preferred).or_else(|| quick.first()),
    };
    let Some(chosen) = chosen else { return skip(session, &t!("workbench-inspect-fix-none")) };
    let params = IntentionApplyParams { id: chosen.id.clone(), wait_ms: Some(APPLY_WAIT_MS) };
    let before = session.state::<Refactoring>().map_or(0, Refactoring::recorded);
    if let Err(error) = ApplyIntention.run(session, params) {
        return skip(session, &error.message);
    }
    forget_items(session, fix.run, &[fix.item]);
    let made = session.state::<Refactoring>().is_some_and(|refactoring| refactoring.recorded() != before);
    if made && session.state::<InspectCode>().is_some_and(|state| state.fixing.cleaning.is_some()) {
        let entry = session.state_mut::<Refactoring>().take_latest(|_| true);
        if let (Some(entry), Some(logs)) = (entry, session.state_mut::<InspectCode>().fixing.cleaning.as_mut()) {
            logs.entry(fix.path.clone()).or_default().extend(entry.files);
        }
    }
    let fixing = &mut session.state_mut::<InspectCode>().fixing;
    fixing.queue.pop_front();
    fixing.fixed += 1;
    fixing.phase = Phase::Next;
}

fn step_server(session: &mut Session) {
    let Some(state) = session.state::<InspectCode>() else { return };
    if state.fixing.task.is_none() {
        return;
    }
    if state.fixing.cancel.as_ref().is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return end_batch(session);
    }
    let Some(fix) = state.fixing.queue.front().cloned() else { return end_batch(session) };
    match state.fixing.phase {
        Phase::Next => {
            held(session);
            session.state_mut::<InspectCode>().fixing.phase = Phase::Checking(Instant::now());
        }
        Phase::Checking(since) => {
            let settle = settle_time(session, &fix);
            if (!checked(session, &fix.path) && since.elapsed() < CHECK_LIMIT) || since.elapsed() < settle {
                return;
            }
            match offset_of(session, &fix) {
                Some(offset) => {
                    session.state_mut::<InspectCode>().fixing.phase = Phase::Asking(Instant::now(), offset);
                }
                None if since.elapsed() < settle + REPORT_MARGIN => {}
                None => skip(session, &t!("workbench-inspect-fix-gone")),
            }
        }
        Phase::Asking(since, offset) => {
            if since.elapsed() > ANSWER_LIMIT {
                return skip(session, &t!("workbench-inspect-no-answer"));
            }
            ask(session, &fix, offset);
        }
    }
}

pub(super) fn step(session: &mut Session) {
    step_jobs(session);
    step_server(session);
}

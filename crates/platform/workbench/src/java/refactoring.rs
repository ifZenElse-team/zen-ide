use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{CommandError, Session};
use zen_i18n::t;
use zen_lsp::{DocumentRequest, FileOperation, LspError};

use super::scan::tokens;
use crate::Workbench;
use crate::language::edits::{self, Applied, Step};
use crate::language::file_operations::{self, Touched};
use crate::language::places::{TextCache, places};
use crate::language::refactor::{Refactoring, is_identifier, preview_file, preview_operation, relative};
use crate::language::refactoring_dialog::{
    ColumnKind, FieldKind, FieldOption, RefactoringApplied, RefactoringConflict, RefactoringField, RefactoringForm,
    RefactoringPlan, RefactoringProvider, TableColumn, Values,
};
use crate::language::{
    Languages, Lookup, LookupParams, byte_offset, char_offset, request_at, request_in, with_languages,
};

const OWNER: &str = "javaRefactoring";
pub const MOVE: &str = "javaMove";
pub const CHANGE_SIGNATURE: &str = "javaChangeSignature";
const VISIBILITIES: [&str; 4] = ["public", "protected", "", "private"];

pub use fields::{ENCAPSULATE_FIELDS, INTRODUCE_PARAMETER_OBJECT};
pub use hierarchy::{EXTRACT_INTERFACE, EXTRACT_SUPERCLASS, PULL_UP, PUSH_DOWN};

pub(crate) fn providers() -> Vec<Arc<dyn RefactoringProvider>> {
    let mut all: Vec<Arc<dyn RefactoringProvider>> = vec![Arc::new(Move), Arc::new(ChangeSignature)];
    all.extend(hierarchy::providers());
    all.extend(fields::providers());
    all
}

#[derive(Default)]
pub struct JavaRefactorings {
    destinations: Option<Job<Vec<Value>>>,
    moving: Option<Job<Planned>>,
    signature: Option<Job<Signature>>,
    changing: Option<Job<Planned>>,
    asked: std::collections::HashMap<&'static str, Job<()>>,
}

impl JavaRefactorings {
    fn waiting(&self) -> BTreeSet<PathBuf> {
        let mut paths = BTreeSet::new();
        if let Some(job) = &self.destinations {
            paths.extend(job.waiting());
        }
        if let Some(job) = &self.moving {
            paths.extend(job.waiting());
        }
        if let Some(job) = &self.signature {
            paths.extend(job.waiting());
        }
        if let Some(job) = &self.changing {
            paths.extend(job.waiting());
        }
        for job in self.asked.values() {
            paths.extend(job.waiting());
        }
        paths
    }
}

struct Job<T> {
    key: String,
    lookups: Vec<Lookup>,
    notes: Vec<String>,
    step: usize,
    done: Option<Result<T, String>>,
}

impl<T> Job<T> {
    fn new(key: String, lookups: Vec<Lookup>) -> Self {
        Self { key, lookups, notes: Vec::new(), step: 0, done: None }
    }

    fn waiting(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.lookups.iter().filter(|lookup| lookup.request.is_some()).map(|lookup| lookup.path.clone())
    }

    fn poll(&mut self, deadline: Instant) -> bool {
        let mut done = true;
        for lookup in &mut self.lookups {
            done &= lookup.poll(deadline.saturating_duration_since(Instant::now()));
        }
        done
    }

    fn answer(&self, index: usize) -> Result<Value, String> {
        match self.lookups.get(index).and_then(|lookup| lookup.answer.clone()) {
            Some(answer) => answer,
            None => Err(LspError::Timeout.to_string()),
        }
    }
}

fn drop_job<T>(languages: &mut Languages, job: Option<Job<T>>) {
    let Some(job) = job else { return };
    for mut lookup in job.lookups {
        if let Some(request) = lookup.request.take()
            && let Some(client) = languages.client_for(&lookup.path)
        {
            client.cancel(&request.pending);
        }
    }
}

struct Planned {
    steps: Vec<Step>,
    conflicts: Vec<RefactoringConflict>,
    renamed: Option<(PathBuf, PathBuf)>,
}

#[derive(Clone)]
struct Signature {
    info: Value,
    path: PathBuf,
    offset: usize,
}

fn with_state<T>(
    session: &mut Session,
    f: impl FnOnce(&mut JavaRefactorings, &mut Languages, &mut Workbench) -> T,
) -> T {
    let mut state = std::mem::take(session.state_mut::<JavaRefactorings>());
    let result = with_languages(session, |languages, workbench| {
        let result = f(&mut state, languages, workbench);
        languages.hold(OWNER, state.waiting());
        result
    });
    session.set_state(state);
    result
}

fn lookup(request: DocumentRequest, method: &str, path: &Path, offset: usize) -> Lookup {
    Lookup {
        method: method.to_owned(),
        path: path.to_owned(),
        offset,
        uri: request.uri.clone(),
        version: request.version,
        request: Some(request),
        answer: None,
        busy_server: false,
    }
}

enum Progress<T> {
    Pending,
    Failed(String),
    Ready(T),
}

fn started(error: CommandError) -> Result<(), CommandError> {
    if error.code.as_deref() == Some("starting") { Ok(()) } else { Err(error) }
}

fn is_java(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "java")
}

fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

fn java_files(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![folder.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let entry = entry.path();
            if entry.is_dir() {
                folders.push(entry);
            } else if is_java(&entry) {
                files.push(entry);
            }
        }
    }
    files.sort();
    files
}

fn served(session: &mut Session, path: &Path) -> bool {
    with_languages(session, |languages, _| languages.spec_for(path).is_some())
}

fn text_of(languages: &Languages, workbench: &Workbench, path: &Path) -> Option<String> {
    languages.synced_text(path).map(str::to_owned).or_else(|| crate::language::file_text(workbench, path).ok())
}

fn plan_of(planned: &Planned, workbench: &Workbench) -> RefactoringPlan {
    let project = workbench.project();
    RefactoringPlan {
        pending: false,
        files: edits::text_files(&planned.steps).map(|file| preview_file(project, file)).collect(),
        operations: edits::file_operations(&planned.steps)
            .map(|operation| preview_operation(project, operation))
            .collect(),
        usage_count: edits::text_files(&planned.steps).map(|file| file.changes.len()).sum(),
        conflicts: planned.conflicts.clone(),
        conflicts_message: (!planned.conflicts.is_empty())
            .then(|| t!("workbench-refactoring-conflicts", count = planned.conflicts.len())),
        error: None,
    }
}

fn failed_plan(error: String) -> RefactoringPlan {
    RefactoringPlan { error: Some(error), ..RefactoringPlan::default() }
}

fn pending_plan() -> RefactoringPlan {
    RefactoringPlan { pending: true, ..RefactoringPlan::default() }
}

fn failed_form(error: String) -> RefactoringForm {
    RefactoringForm { error: Some(error), ..RefactoringForm::default() }
}

fn conflict_labels(form: RefactoringForm) -> RefactoringForm {
    RefactoringForm {
        conflicts_title: t!("workbench-refactoring-problems"),
        anyway_label: t!("workbench-refactoring-continue"),
        view_label: t!("workbench-refactoring-view-conflicts"),
        ..form
    }
}

fn field(id: &str, label: String, kind: FieldKind, value: Value) -> RefactoringField {
    RefactoringField {
        id: id.to_owned(),
        label,
        kind,
        value,
        options: Vec::new(),
        columns: Vec::new(),
        new_row: Value::Null,
        fixed: false,
    }
}

fn apply_planned(session: &mut Session, planned: Planned, action: &str) -> Result<RefactoringApplied, CommandError> {
    session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
    let steps = planned.steps;
    let (written, entry) = with_languages(session, |languages, workbench| {
        let applied = edits::apply(&steps, languages, workbench).map_err(|error| CommandError::new(error.message()))?;
        let written: crate::local_history::Written = applied.iter().flat_map(Applied::written).collect();
        Ok::<_, CommandError>((written, edits::journal(applied, workbench)))
    })?;
    session.state_mut::<Refactoring>().record(entry);
    crate::local_history::changed_by(session, action, written);
    if let Some((from, to)) = planned.renamed {
        let touched = [Touched { path: from, new_path: Some(to), folder: true }];
        file_operations::did(session, FileOperation::Rename, &touched);
    }
    Ok(RefactoringApplied {
        files: edits::changed_paths(&steps),
        edits: edits::text_files(&steps).map(|file| file.changes.len()).sum(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MoveTarget {
    Classes(Vec<PathBuf>),
    Package(PathBuf),
}

impl MoveTarget {
    fn find(session: &Session, context: &Value) -> Result<Self, String> {
        let workbench = session.state::<Workbench>().ok_or_else(|| t!("workbench-error-no-document"))?;
        let paths: Vec<PathBuf> = context
            .get("paths")
            .and_then(Value::as_array)
            .map(|paths| {
                paths.iter().filter_map(Value::as_str).map(|path| workbench.resolve(Path::new(path))).collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            let path = context
                .get("path")
                .and_then(Value::as_str)
                .map(|path| workbench.resolve(Path::new(path)))
                .or_else(|| workbench.active_document().map(|document| document.path().to_owned()))
                .ok_or_else(|| t!("workbench-error-no-document"))?;
            return if is_java(&path) {
                Ok(Self::Classes(vec![path]))
            } else {
                Err(t!("workbench-java-move-not-here"))
            };
        }
        if let [folder] = paths.as_slice()
            && folder.is_dir()
        {
            return if java_files(folder).is_empty() {
                Err(t!("workbench-java-move-no-classes"))
            } else {
                Ok(Self::Package(folder.clone()))
            };
        }
        if paths.iter().all(|path| is_java(path) && path.is_file()) {
            let mut paths = paths;
            paths.sort();
            paths.dedup();
            return Ok(Self::Classes(paths));
        }
        Err(t!("workbench-java-move-not-here"))
    }

    fn probe(&self) -> Option<PathBuf> {
        match self {
            Self::Classes(paths) => paths.first().cloned(),
            Self::Package(folder) => java_files(folder).into_iter().next(),
        }
    }

    fn source_uris(&self) -> Vec<String> {
        match self {
            Self::Classes(paths) => paths.iter().map(|path| zen_lsp::position::file_uri(path)).collect(),
            Self::Package(folder) => vec![zen_lsp::position::file_uri(folder)],
        }
    }

    fn folder(&self) -> PathBuf {
        match self {
            Self::Classes(paths) => {
                paths.first().and_then(|path| path.parent()).map(Path::to_owned).unwrap_or_default()
            }
            Self::Package(folder) => folder.parent().map(Path::to_owned).unwrap_or_default(),
        }
    }

    fn key(&self) -> String {
        format!("{self:?}")
    }

    fn offers(&self, destination: &Value) -> bool {
        match self {
            Self::Classes(_) => true,
            Self::Package(folder) => {
                destination_folder(destination).is_none_or(|place| !real(&place).starts_with(real(folder)))
            }
        }
    }
}

fn destination_folder(destination: &Value) -> Option<PathBuf> {
    destination.get("uri").and_then(Value::as_str).and_then(zen_lsp::position::uri_path)
}

fn package_name(target: &MoveTarget, destination: &Value) -> String {
    let parent = destination.get("displayName").and_then(Value::as_str).unwrap_or_default();
    let default = destination.get("isDefaultPackage").and_then(Value::as_bool).unwrap_or(false);
    match target {
        MoveTarget::Classes(_) => parent.to_owned(),
        MoveTarget::Package(folder) => {
            let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            if default || parent.is_empty() { name } else { format!("{parent}.{name}") }
        }
    }
}

fn find_destinations(
    state: &mut JavaRefactorings,
    languages: &mut Languages,
    workbench: &mut Workbench,
    target: &MoveTarget,
    wait: Duration,
) -> Result<Progress<Vec<Value>>, CommandError> {
    languages.sync(workbench);
    let key = target.key();
    let reuse = state.destinations.as_ref().is_some_and(|job| job.key == key && !matches!(job.done, Some(Err(_))));
    if !reuse {
        drop_job(languages, state.destinations.take());
        let Some(probe) = target.probe() else { return Ok(Progress::Failed(t!("workbench-java-move-no-classes"))) };
        let uris = target.source_uris();
        let method = "java/getMoveDestinations";
        let request = request_in(
            languages,
            workbench,
            &probe,
            method,
            |_, _| json!({ "moveKind": "moveResource", "sourceUris": uris, "params": null }),
        );
        match request {
            Ok(request) => state.destinations = Some(Job::new(key, vec![lookup(request, method, &probe, 0)])),
            Err(message) => {
                started(languages.lookup_error(&probe, message))?;
                return Ok(Progress::Pending);
            }
        }
    }
    let Some(job) = state.destinations.as_mut() else { return Ok(Progress::Pending) };
    if job.done.is_none() {
        if !job.poll(Instant::now() + wait) {
            return Ok(Progress::Pending);
        }
        job.done = Some(job.answer(0).and_then(|answer| {
            if let Some(error) = answer.get("errorMessage").and_then(Value::as_str) {
                return Err(error.to_owned());
            }
            let list = answer.get("destinations").and_then(Value::as_array).cloned().unwrap_or_default();
            if list.is_empty() { Err(t!("workbench-java-move-no-destinations")) } else { Ok(list) }
        }));
        job.lookups.clear();
    }
    Ok(match job.done.clone() {
        Some(Ok(list)) => Progress::Ready(list.into_iter().filter(|destination| target.offers(destination)).collect()),
        Some(Err(error)) => Progress::Failed(error),
        None => Progress::Pending,
    })
}

fn default_destination(target: &MoveTarget, list: &[Value]) -> String {
    let here = real(&target.folder());
    let uri = |destination: &Value| destination.get("uri").and_then(Value::as_str).unwrap_or_default().to_owned();
    list.iter()
        .find(|destination| destination_folder(destination).is_some_and(|folder| real(&folder) == here))
        .or_else(|| list.first())
        .map(uri)
        .unwrap_or_default()
}

struct Move;

impl Move {
    fn destinations(
        session: &mut Session,
        context: &Value,
        wait: Duration,
    ) -> Result<Progress<(MoveTarget, Vec<Value>)>, CommandError> {
        let target = match MoveTarget::find(session, context) {
            Ok(target) => target,
            Err(error) => return Ok(Progress::Failed(error)),
        };
        let found = with_state(session, |state, languages, workbench| {
            find_destinations(state, languages, workbench, &target, wait)
        })?;
        Ok(match found {
            Progress::Ready(list) => Progress::Ready((target, list)),
            Progress::Pending => Progress::Pending,
            Progress::Failed(error) => Progress::Failed(error),
        })
    }

    fn title_for(target: &MoveTarget) -> String {
        match target {
            MoveTarget::Classes(paths) if paths.len() == 1 => {
                let name = paths[0].file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                t!("workbench-java-move-class-title", name = name)
            }
            MoveTarget::Classes(paths) => t!("workbench-java-move-classes-title", count = paths.len()),
            MoveTarget::Package(folder) => {
                let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                t!("workbench-java-move-package-title", name = name)
            }
        }
    }
}

impl RefactoringProvider for Move {
    fn id(&self) -> &str {
        MOVE
    }

    fn title(&self) -> String {
        t!("workbench-java-move-title")
    }

    fn action(&self) -> Option<&str> {
        Some("move")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        let Ok(target) = MoveTarget::find(session, context) else { return false };
        target.probe().is_some_and(|probe| served(session, &probe))
    }

    fn form(&self, session: &mut Session, context: &Value, wait: Duration) -> Result<RefactoringForm, CommandError> {
        let (target, list) = match Self::destinations(session, context, wait)? {
            Progress::Ready(found) => found,
            Progress::Failed(error) => return Ok(failed_form(error)),
            Progress::Pending => return Ok(RefactoringForm { pending: true, ..RefactoringForm::default() }),
        };
        let projects: BTreeSet<&str> =
            list.iter().filter_map(|destination| destination.get("project").and_then(Value::as_str)).collect();
        let options = list
            .iter()
            .map(|destination| {
                let name = destination.get("displayName").and_then(Value::as_str).unwrap_or_default();
                let project = destination.get("project").and_then(Value::as_str).unwrap_or_default();
                FieldOption {
                    value: destination.get("uri").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    label: if projects.len() > 1 { format!("{name} ({project})") } else { name.to_owned() },
                }
            })
            .collect();
        let mut destination = field(
            "destination",
            t!("workbench-java-move-to-package"),
            FieldKind::Choice,
            json!(default_destination(&target, &list)),
        );
        destination.options = options;
        Ok(conflict_labels(RefactoringForm {
            title: Self::title_for(&target),
            fields: vec![destination],
            ..RefactoringForm::default()
        }))
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let (target, list) = match Self::destinations(session, context, wait)? {
            Progress::Ready(found) => found,
            Progress::Failed(error) => return Ok(failed_plan(error)),
            Progress::Pending => return Ok(pending_plan()),
        };
        let chosen = values
            .get("destination")
            .and_then(Value::as_str)
            .map_or_else(|| default_destination(&target, &list), str::to_owned);
        let Some(destination) = list.iter().find(|destination| destination.get("uri") == Some(&json!(chosen))) else {
            return Ok(failed_plan(t!("workbench-java-move-choose")));
        };
        let Some(folder) = destination_folder(destination) else {
            return Ok(failed_plan(t!("workbench-java-move-choose")));
        };
        let package = package_name(&target, destination);
        if real(&folder) == real(&target.folder()) {
            let here = destination.get("displayName").and_then(Value::as_str).unwrap_or_default();
            return Ok(failed_plan(t!("workbench-java-move-same-package", package = here)));
        }
        match &target {
            MoveTarget::Classes(paths) => {
                if let Some(name) =
                    paths.iter().filter_map(|path| path.file_name()).find(|name| folder.join(name).exists())
                {
                    let name = name.to_string_lossy().into_owned();
                    return Ok(failed_plan(t!("workbench-java-move-file-exists", name = name, package = package)));
                }
            }
            MoveTarget::Package(source) => {
                if source.file_name().is_some_and(|name| folder.join(name).exists()) {
                    return Ok(failed_plan(t!("workbench-java-move-package-exists", package = package)));
                }
            }
        }
        let key = format!("{}\n{chosen}", target.key());
        with_state(session, |state, languages, workbench| {
            plan_move(state, languages, workbench, &target, destination, &folder, key, wait)
        })
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let plan = self.plan(session, context, values, wait)?;
        if plan.pending {
            return Err(CommandError::new(LspError::Timeout.to_string()));
        }
        if let Some(error) = plan.error {
            return Err(CommandError::new(error));
        }
        let job = session.state_mut::<JavaRefactorings>().moving.take();
        let Some(Ok(planned)) = job.and_then(|job| job.done) else {
            return Err(CommandError::new(t!("workbench-java-move-not-here")));
        };
        session.state_mut::<JavaRefactorings>().destinations = None;
        apply_planned(session, planned, crate::local_history::MOVE)
    }

    fn cancel(&self, session: &mut Session) {
        with_state(session, |state, languages, _| {
            drop_job(languages, state.destinations.take());
            drop_job(languages, state.moving.take());
        });
    }
}

#[allow(clippy::too_many_arguments, reason = "the move's target, destination and where it waits")]
fn plan_move(
    state: &mut JavaRefactorings,
    languages: &mut Languages,
    workbench: &mut Workbench,
    target: &MoveTarget,
    destination: &Value,
    folder: &Path,
    key: String,
    wait: Duration,
) -> Result<RefactoringPlan, CommandError> {
    languages.sync(workbench);
    let Some(probe) = target.probe() else { return Ok(failed_plan(t!("workbench-java-move-no-classes"))) };
    let key = format!("{key}\n{:?}", languages.server_version(&probe));
    let reuse = state.moving.as_ref().is_some_and(|job| job.key == key);
    if !reuse {
        drop_job(languages, state.moving.take());
        match start_move(languages, workbench, target, destination, folder, &probe, key) {
            Ok(job) => state.moving = Some(job),
            Err(error) => {
                started(error)?;
                return Ok(pending_plan());
            }
        }
    }
    let Some(job) = state.moving.as_mut() else { return Ok(pending_plan()) };
    if job.done.is_none() {
        if !job.poll(Instant::now() + wait) {
            return Ok(pending_plan());
        }
        let done = finish_move(job, languages, workbench, target, folder);
        job.done = Some(done);
        job.lookups.clear();
    }
    Ok(match &job.done {
        Some(Ok(planned)) => plan_of(planned, workbench),
        Some(Err(error)) => failed_plan(error.clone()),
        None => pending_plan(),
    })
}

fn start_move(
    languages: &mut Languages,
    workbench: &Workbench,
    target: &MoveTarget,
    destination: &Value,
    folder: &Path,
    probe: &Path,
    key: String,
) -> Result<Job<Planned>, CommandError> {
    match target {
        MoveTarget::Classes(paths) => {
            let method = "java/move";
            let uris = target.source_uris();
            let destination = destination.clone();
            let request = request_in(languages, workbench, probe, method, |_, _| {
                json!({
                    "moveKind": "moveResource",
                    "sourceUris": uris,
                    "params": null,
                    "destination": destination,
                    "updateReferences": true
                })
            })
            .map_err(|message| languages.lookup_error(probe, message))?;
            let mut job = Job::new(key, vec![lookup(request, method, probe, 0)]);
            for path in paths {
                let Some(text) = text_of(languages, workbench, path) else { continue };
                for (name, offset) in hidden_types(&text) {
                    let method = "textDocument/references";
                    let extra = json!({ "context": { "includeDeclaration": false } });
                    if let Ok(request) = request_at(languages, workbench, path, offset, method, extra) {
                        job.lookups.push(lookup(request, method, path, offset));
                        job.notes.push(name);
                    }
                }
            }
            Ok(job)
        }
        MoveTarget::Package(source) => {
            let Some(name) = source.file_name() else {
                return Err(CommandError::new(t!("workbench-java-move-not-here")));
            };
            let moved = folder.join(name);
            let (old, new) = (zen_lsp::position::file_uri(source), zen_lsp::position::file_uri(&moved));
            let renamed = json!({ "documentChanges": [{ "kind": "rename", "oldUri": old, "newUri": new }] });
            languages.sync(workbench);
            if !languages.serves(probe) {
                languages.start_for(probe);
                return Err(languages.lookup_error(probe, t!("workbench-language-not-running")));
            }
            let version = languages.server_version(probe).unwrap_or_default();
            let client =
                languages.client_for(probe).ok_or_else(|| CommandError::new(t!("workbench-language-not-running")))?;
            let params = json!({ "files": [{ "oldUri": old, "newUri": new }] });
            let pending = client
                .request(FileOperation::Rename.will_method(), params)
                .map_err(|error| CommandError::new(error.to_string()))?;
            let request = DocumentRequest { pending, uri: old.clone(), version };
            let mut job = Job::new(key, vec![lookup(request, FileOperation::Rename.will_method(), probe, 0)]);
            job.notes.push(renamed.to_string());
            Ok(job)
        }
    }
}

fn finish_move(
    job: &Job<Planned>,
    languages: &mut Languages,
    workbench: &Workbench,
    target: &MoveTarget,
    folder: &Path,
) -> Result<Planned, String> {
    let answer = job.answer(0)?;
    match target {
        MoveTarget::Classes(paths) => {
            if let Some(error) = answer.get("errorMessage").and_then(Value::as_str) {
                return Err(error.to_owned());
            }
            let edit =
                answer.get("edit").filter(|edit| !edit.is_null()).ok_or_else(|| t!("workbench-java-move-refused"))?;
            let steps = edits::parse(edit, languages).map_err(|error| error.message())?;
            let moved: Vec<PathBuf> = paths.iter().map(|path| real(path)).collect();
            let there = real(folder);
            let mut cache = TextCache::new();
            let mut conflicts = Vec::new();
            for (index, name) in job.notes.iter().enumerate() {
                let Ok(found) = job.answer(index + 1) else { continue };
                for place in places(&found, languages, &mut cache) {
                    let file = real(&place.path);
                    if moved.contains(&file) || file.parent().is_some_and(|parent| parent == there) {
                        continue;
                    }
                    let shown = relative(workbench.project(), &place.path);
                    conflicts.push(RefactoringConflict {
                        message: t!("workbench-java-move-package-private", name = name.as_str(), file = shown.as_str()),
                        relative: Some(shown),
                        place: Some(place),
                    });
                }
            }
            Ok(Planned { steps, conflicts, renamed: None })
        }
        MoveTarget::Package(source) => {
            let mut steps = if answer.is_null() {
                Vec::new()
            } else {
                edits::parse(&answer, languages).map_err(|error| error.message())?
            };
            let renamed: Value = job.notes.first().and_then(|note| serde_json::from_str(note).ok()).unwrap_or_default();
            steps.extend(edits::parse(&renamed, languages).map_err(|error| error.message())?);
            let moved = source.file_name().map(|name| folder.join(name)).unwrap_or_default();
            Ok(Planned { steps, conflicts: Vec::new(), renamed: Some((source.clone(), moved)) })
        }
    }
}

fn token_offset(text: &str, token: &str) -> usize {
    token.as_ptr() as usize - text.as_ptr() as usize
}

pub(crate) fn hidden_types(text: &str) -> Vec<(String, usize)> {
    let tokens = tokens(text);
    let mut depth = 0usize;
    let mut found = Vec::new();
    let mut public = false;
    for (index, token) in tokens.iter().enumerate() {
        match token.text {
            "{" => depth += 1,
            "}" => {
                depth = depth.saturating_sub(1);
                public = false;
            }
            ";" if depth == 0 => public = false,
            "public" if depth == 0 => public = true,
            "class" | "interface" | "enum" | "record" if depth == 0 => {
                let after_dot = index > 0 && tokens[index - 1].text == ".";
                if let Some(name) = tokens.get(index + 1).filter(|_| !after_dot) {
                    if !public {
                        found.push((name.text.to_owned(), char_offset(text, token_offset(text, name.text))));
                    }
                    public = false;
                }
            }
            _ => {}
        }
    }
    found
}

pub(crate) fn body_uses(text: &str, from: usize, names: &[String]) -> Vec<(String, usize, usize)> {
    let tokens = tokens(text);
    let from = byte_offset(text, from);
    let mut index = tokens.iter().position(|token| token_offset(text, token.text) >= from && token.text == "(");
    let mut depth = 0usize;
    while let Some(at) = index {
        match tokens.get(at).map(|token| token.text) {
            Some("(") => depth += 1,
            Some(")") => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            None => return Vec::new(),
            _ => {}
        }
        index = Some(at + 1);
    }
    let Some(close) = index else { return Vec::new() };
    let Some(open) =
        tokens[close..].iter().position(|token| token.text == "{" || token.text == ";").map(|at| close + at)
    else {
        return Vec::new();
    };
    if tokens[open].text == ";" {
        return Vec::new();
    }
    let mut depth = 0usize;
    let mut used = Vec::new();
    for (at, token) in tokens.iter().enumerate().skip(open) {
        match token.text {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            word if names.iter().any(|name| name == word) && tokens[at - 1].text != "." => {
                let start = char_offset(text, token_offset(text, word));
                used.push((word.to_owned(), start, start + word.chars().count()));
            }
            _ => {}
        }
    }
    used
}

pub(crate) fn refine(edit: &Value, text_of: impl Fn(&Path) -> Option<String>) -> Value {
    let split = |uri: &str, edits: &Value| -> Value {
        let Some(list) = edits.as_array() else { return edits.clone() };
        let Some(text) = zen_lsp::position::uri_path(uri).and_then(|path| text_of(&path)) else { return edits.clone() };
        Value::Array(list.iter().flat_map(|edit| hunks(&text, edit)).collect())
    };
    let mut edit = edit.clone();
    if let Some(changes) = edit.get_mut("documentChanges").and_then(Value::as_array_mut) {
        for change in changes {
            let uri = change.pointer("/textDocument/uri").and_then(Value::as_str).map(str::to_owned);
            if let (Some(uri), Some(edits)) = (uri, change.get("edits").cloned()) {
                change["edits"] = split(&uri, &edits);
            }
        }
    }
    if let Some(changes) = edit.get_mut("changes").and_then(Value::as_object_mut) {
        for (uri, edits) in changes.iter_mut() {
            *edits = split(uri, edits);
        }
    }
    edit
}

fn hunks(text: &str, edit: &Value) -> Vec<Value> {
    let offset = |key: &str| -> Option<usize> {
        let position = edit.get("range")?.get(key)?;
        let line = position.get("line")?.as_u64()? as u32;
        let character = position.get("character")?.as_u64()? as u32;
        zen_lsp::position::offset(text, lsp_types::Position { line, character })
    };
    let (Some(start), Some(end), Some(new_text)) =
        (offset("start"), offset("end"), edit.get("newText").and_then(Value::as_str))
    else {
        return vec![edit.clone()];
    };
    if start > end {
        return vec![edit.clone()];
    }
    let head = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let tail = text[end..].find('\n').map_or(text.len(), |at| end + at + 1);
    let before = &text[head..tail];
    let after = format!("{}{new_text}{}", &text[head..start], &text[end..tail]);
    let old_lines = zen_vcs::diff::lines(before);
    let new_lines = zen_vcs::diff::lines(&after);
    let line_start =
        |lines: &[&str], index: usize| -> usize { lines[..index.min(lines.len())].iter().map(|line| line.len()).sum() };
    zen_vcs::diff::hunks(before, &after)
        .into_iter()
        .map(|hunk| {
            let from = head + line_start(&old_lines, hunk.before.start);
            let to = head + line_start(&old_lines, hunk.before.end);
            let replacement: String =
                new_lines[hunk.after.start.min(new_lines.len())..hunk.after.end.min(new_lines.len())].concat();
            json!({
                "range": {
                    "start": zen_lsp::position::position(text, from),
                    "end": zen_lsp::position::position(text, to)
                },
                "newText": replacement
            })
        })
        .collect()
}

struct ChangeSignature;

impl ChangeSignature {
    fn place(session: &Session, context: &Value) -> Result<(PathBuf, usize), String> {
        let params: LookupParams =
            crate::language::refactoring_dialog::context(context).map_err(|error| error.message)?;
        let workbench = session.state::<Workbench>().ok_or_else(|| t!("workbench-error-no-document"))?;
        let (path, offset) = params.place(workbench).map_err(|error| error.message)?;
        if is_java(&path) { Ok((path, offset)) } else { Err(t!("workbench-java-signature-not-here")) }
    }

    fn signature(session: &mut Session, context: &Value, wait: Duration) -> Result<Progress<Signature>, CommandError> {
        let (path, offset) = match Self::place(session, context) {
            Ok(place) => place,
            Err(error) => return Ok(Progress::Failed(error)),
        };
        with_state(session, |state, languages, workbench| {
            find_signature(state, languages, workbench, &path, offset, wait)
        })
    }
}

fn code_action_params(text: &str, uri: &str, offset: usize) -> Value {
    let position = zen_lsp::position::position(text, byte_offset(text, offset));
    json!({
        "textDocument": { "uri": uri },
        "range": { "start": position, "end": position },
        "context": { "diagnostics": [] }
    })
}

fn ask_info(
    languages: &mut Languages,
    workbench: &Workbench,
    path: &Path,
    offset: usize,
) -> Result<Lookup, CommandError> {
    let method = "java/getChangeSignatureInfo";
    let request = request_in(languages, workbench, path, method, |uri, text| code_action_params(text, uri, offset))
        .map_err(|message| languages.lookup_error(path, message))?;
    Ok(lookup(request, method, path, offset))
}

fn find_signature(
    state: &mut JavaRefactorings,
    languages: &mut Languages,
    workbench: &mut Workbench,
    path: &Path,
    offset: usize,
    wait: Duration,
) -> Result<Progress<Signature>, CommandError> {
    languages.sync(workbench);
    let key = format!("{path:?}\n{offset}\n{:?}", languages.server_version(path));
    let reuse = state.signature.as_ref().is_some_and(|job| job.key == key && !matches!(job.done, Some(Err(_))));
    if !reuse {
        drop_job(languages, state.signature.take());
        match ask_info(languages, workbench, path, offset) {
            Ok(lookup) => state.signature = Some(Job::new(key, vec![lookup])),
            Err(error) => {
                started(error)?;
                return Ok(Progress::Pending);
            }
        }
    }
    let Some(job) = state.signature.as_mut() else { return Ok(Progress::Pending) };
    let deadline = Instant::now() + wait;
    while job.done.is_none() {
        if !job.poll(deadline) {
            return Ok(Progress::Pending);
        }
        let answer = job.answer(0);
        let asked = job.lookups.first().map(|lookup| (lookup.path.clone(), lookup.offset));
        job.lookups.clear();
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => {
                job.done = Some(Err(error));
                break;
            }
        };
        match job.step {
            0 | 2 if answer.get("methodIdentifier").is_some_and(Value::is_string) => {
                let (path, offset) = asked.unwrap_or_default();
                job.done = Some(Ok(Signature { info: answer, path, offset }));
            }
            0 => {
                let method = "textDocument/definition";
                match request_at(languages, workbench, path, offset, method, json!({})) {
                    Ok(request) => {
                        job.lookups.push(lookup(request, method, path, offset));
                        job.step = 1;
                    }
                    Err(error) => job.done = Some(Err(error)),
                }
            }
            1 => {
                let target = definition(&answer, languages);
                match target {
                    Some((path, offset)) => match ask_info(languages, workbench, &path, offset) {
                        Ok(lookup) => {
                            job.lookups.push(lookup);
                            job.step = 2;
                        }
                        Err(error) => job.done = Some(Err(error.message)),
                    },
                    None => job.done = Some(Err(t!("workbench-java-signature-not-here"))),
                }
            }
            _ => job.done = Some(Err(t!("workbench-java-signature-not-here"))),
        }
    }
    Ok(match job.done.clone() {
        Some(Ok(signature)) => Progress::Ready(signature),
        Some(Err(error)) => Progress::Failed(error),
        None => Progress::Pending,
    })
}

fn definition(answer: &Value, languages: &Languages) -> Option<(PathBuf, usize)> {
    let first = if answer.is_array() { answer.get(0)? } else { answer };
    let uri = first.get("targetUri").or_else(|| first.get("uri"))?.as_str()?;
    let range = first.get("targetSelectionRange").or_else(|| first.get("range"))?;
    let path = zen_lsp::position::uri_path(uri)?;
    let place = TextCache::new().place(languages, &path, range)?;
    Some((path, place.start))
}

struct Parameter {
    kind: String,
    name: String,
    default: String,
    original: i64,
}

fn parameters(value: Option<&Value>) -> Vec<Parameter> {
    let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_owned();
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| Parameter {
                    kind: text(row, "type"),
                    name: text(row, "name"),
                    default: text(row, "defaultValue"),
                    original: row.get("originalIndex").and_then(Value::as_i64).unwrap_or(-1),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn check_signature(name: &str, return_type: &str, rows: &[Parameter]) -> Result<(), String> {
    if !is_identifier(Some("java"), name) {
        return Err(t!("workbench-rename-invalid-name", name = name));
    }
    if return_type.is_empty() {
        return Err(t!("workbench-java-signature-no-return-type"));
    }
    let mut seen = BTreeSet::new();
    for row in rows {
        if row.kind.is_empty() {
            return Err(t!("workbench-java-signature-no-type", name = row.name.as_str()));
        }
        if !is_identifier(Some("java"), &row.name) {
            return Err(t!("workbench-rename-invalid-name", name = row.name.as_str()));
        }
        if !seen.insert(row.name.as_str()) {
            return Err(t!("workbench-java-signature-duplicate", name = row.name.as_str()));
        }
        if row.original < 0 && row.default.is_empty() {
            return Err(t!("workbench-java-signature-default-needed", name = row.name.as_str()));
        }
    }
    Ok(())
}

impl RefactoringProvider for ChangeSignature {
    fn id(&self) -> &str {
        CHANGE_SIGNATURE
    }

    fn title(&self) -> String {
        t!("workbench-java-signature-title")
    }

    fn action(&self) -> Option<&str> {
        Some("changeSignature")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        match Self::place(session, context) {
            Ok((path, _)) => served(session, &path),
            Err(_) => false,
        }
    }

    fn form(&self, session: &mut Session, context: &Value, wait: Duration) -> Result<RefactoringForm, CommandError> {
        let signature = match Self::signature(session, context, wait)? {
            Progress::Ready(signature) => signature,
            Progress::Failed(error) => return Ok(failed_form(error)),
            Progress::Pending => return Ok(RefactoringForm { pending: true, ..RefactoringForm::default() }),
        };
        let info = &signature.info;
        let text = |key: &str| json!(info.get(key).and_then(Value::as_str).unwrap_or_default());
        let mut visibility =
            field("visibility", t!("workbench-java-signature-visibility"), FieldKind::Choice, text("modifier"));
        visibility.options = VISIBILITIES
            .iter()
            .map(|value| FieldOption {
                value: (*value).to_owned(),
                label: if value.is_empty() {
                    t!("workbench-java-signature-package-private")
                } else {
                    (*value).to_owned()
                },
            })
            .collect();
        let rows: Vec<Value> = info
            .get("parameters")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        json!({
                            "type": row.get("type").cloned().unwrap_or_else(|| json!("")),
                            "name": row.get("name").cloned().unwrap_or_else(|| json!("")),
                            "defaultValue": row.get("defaultValue").cloned().unwrap_or_else(|| json!("")),
                            "originalIndex": row.get("originalIndex").cloned().unwrap_or_else(|| json!(-1))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut table = field("parameters", t!("workbench-java-signature-parameters"), FieldKind::Table, json!(rows));
        table.columns = [
            ("type", t!("workbench-java-signature-type")),
            ("name", t!("workbench-java-signature-name")),
            ("defaultValue", t!("workbench-java-signature-default")),
        ]
        .into_iter()
        .map(|(id, label)| TableColumn::new(id, label, ColumnKind::Text))
        .collect();
        table.new_row = json!({ "type": "", "name": "", "defaultValue": "", "originalIndex": -1 });
        let name = info.get("methodName").and_then(Value::as_str).unwrap_or_default();
        Ok(conflict_labels(RefactoringForm {
            title: t!("workbench-java-signature-title-of", name = name),
            fields: vec![
                visibility,
                field("returnType", t!("workbench-java-signature-return-type"), FieldKind::Text, text("returnType")),
                field("name", t!("workbench-java-signature-name"), FieldKind::Text, text("methodName")),
                table,
                field("delegate", t!("workbench-java-signature-delegate"), FieldKind::Checkbox, json!(false)),
            ],
            ..RefactoringForm::default()
        }))
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let signature = match Self::signature(session, context, wait)? {
            Progress::Ready(signature) => signature,
            Progress::Failed(error) => return Ok(failed_plan(error)),
            Progress::Pending => return Ok(pending_plan()),
        };
        let info = &signature.info;
        let given = |key: &str, from: &str| {
            values
                .get(key)
                .and_then(Value::as_str)
                .or_else(|| info.get(from).and_then(Value::as_str))
                .unwrap_or_default()
                .trim()
                .to_owned()
        };
        let name = given("name", "methodName");
        let return_type = given("returnType", "returnType");
        let visibility = given("visibility", "modifier");
        let rows = parameters(values.get("parameters").or_else(|| info.get("parameters")));
        if let Err(error) = check_signature(&name, &return_type, &rows) {
            return Ok(failed_plan(error));
        }
        let delegate = values.get("delegate").and_then(Value::as_bool).unwrap_or(false);
        let removed: Vec<String> = parameters(info.get("parameters"))
            .into_iter()
            .filter(|original| !rows.iter().any(|row| row.original == original.original))
            .map(|original| original.name)
            .collect();
        let arguments = json!([
            info.get("methodIdentifier").cloned().unwrap_or_default(),
            delegate,
            name,
            visibility,
            return_type,
            rows.iter()
                .map(|row| json!({
                    "type": row.kind,
                    "name": row.name,
                    "defaultValue": row.default,
                    "originalIndex": row.original
                }))
                .collect::<Vec<_>>(),
            info.get("exceptions").cloned().unwrap_or_else(|| json!([])),
            false
        ]);
        with_state(session, |state, languages, workbench| {
            plan_signature(state, languages, workbench, &signature, arguments, &removed, wait)
        })
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let plan = self.plan(session, context, values, wait)?;
        if plan.pending {
            return Err(CommandError::new(LspError::Timeout.to_string()));
        }
        if let Some(error) = plan.error {
            return Err(CommandError::new(error));
        }
        let job = session.state_mut::<JavaRefactorings>().changing.take();
        let Some(Ok(planned)) = job.and_then(|job| job.done) else {
            return Err(CommandError::new(t!("workbench-java-signature-not-here")));
        };
        session.state_mut::<JavaRefactorings>().signature = None;
        apply_planned(session, planned, crate::local_history::CHANGE_SIGNATURE)
    }

    fn cancel(&self, session: &mut Session) {
        with_state(session, |state, languages, _| {
            drop_job(languages, state.signature.take());
            drop_job(languages, state.changing.take());
        });
    }
}

fn plan_signature(
    state: &mut JavaRefactorings,
    languages: &mut Languages,
    workbench: &mut Workbench,
    signature: &Signature,
    arguments: Value,
    removed: &[String],
    wait: Duration,
) -> Result<RefactoringPlan, CommandError> {
    languages.sync(workbench);
    let path = &signature.path;
    let key = format!("{path:?}\n{arguments}\n{:?}", languages.server_version(path));
    let reuse = state.changing.as_ref().is_some_and(|job| job.key == key);
    if !reuse {
        drop_job(languages, state.changing.take());
        let method = "java/getRefactorEdit";
        let offset = signature.offset;
        let request = request_in(languages, workbench, path, method, |uri, text| {
            json!({
                "command": "changeSignature",
                "context": code_action_params(text, uri, offset),
                "options": { "tabSize": 4, "insertSpaces": true },
                "commandArguments": arguments
            })
        });
        match request {
            Ok(request) => state.changing = Some(Job::new(key, vec![lookup(request, method, path, offset)])),
            Err(message) => {
                started(languages.lookup_error(path, message))?;
                return Ok(pending_plan());
            }
        }
    }
    let Some(job) = state.changing.as_mut() else { return Ok(pending_plan()) };
    if job.done.is_none() {
        if !job.poll(Instant::now() + wait) {
            return Ok(pending_plan());
        }
        let done = job.answer(0).and_then(|answer| {
            if let Some(error) = answer.get("errorMessage").and_then(Value::as_str) {
                return Err(error.to_owned());
            }
            let edit = answer
                .get("edit")
                .filter(|edit| !edit.is_null())
                .ok_or_else(|| t!("workbench-java-signature-refused"))?;
            let edit = refine(edit, |path| text_of(languages, workbench, path));
            let steps = edits::parse(&edit, languages).map_err(|error| error.message())?;
            let conflicts = text_of(languages, workbench, path)
                .map(|text| {
                    body_uses(&text, signature.offset, removed)
                        .into_iter()
                        .filter_map(|(name, start, end)| {
                            let place = TextCache::new().place(languages, path, &range_of(&text, start, end))?;
                            let shown = relative(workbench.project(), path);
                            Some(RefactoringConflict {
                                message: t!("workbench-java-signature-removed-used", name = name.as_str()),
                                place: Some(place),
                                relative: Some(shown),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(Planned { steps, conflicts, renamed: None })
        });
        job.done = Some(done);
        job.lookups.clear();
    }
    Ok(match &job.done {
        Some(Ok(planned)) => plan_of(planned, workbench),
        Some(Err(error)) => failed_plan(error.clone()),
        None => pending_plan(),
    })
}

fn range_of(text: &str, start: usize, end: usize) -> Value {
    json!({
        "start": zen_lsp::position::position(text, byte_offset(text, start)),
        "end": zen_lsp::position::position(text, byte_offset(text, end))
    })
}

mod fields;
mod hierarchy;

#[cfg(test)]
mod tests;

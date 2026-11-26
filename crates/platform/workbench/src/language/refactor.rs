use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::LspError;
use zen_lsp::lsp_types::OneOf;

use super::edits::{self, Applied, FileEdit, FileOperation, Step};
use super::places::{Place, place};
use super::{Languages, Lookup, LookupParams, MAX_WAIT, file_text, request_at, with_languages};
use crate::Workbench;

pub const JOURNAL_SIZE: usize = 20;

const GO_KEYWORDS: [&str; 25] = [
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

#[derive(Default)]
pub struct Refactoring {
    prepare: Option<Lookup>,
    rename: Option<Plan>,
    journal: Vec<JournalEntry>,
    recorded: u64,
}

impl Refactoring {
    fn waiting(&self) -> BTreeSet<PathBuf> {
        let plan = self.rename.as_ref().map(|plan| &plan.lookup);
        self.prepare
            .iter()
            .chain(plan)
            .filter(|lookup| lookup.request.is_some())
            .map(|lookup| lookup.path.clone())
            .collect()
    }
}

struct Plan {
    new_name: String,
    lookup: Lookup,
    files: Option<Result<Vec<Step>, String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub files: Vec<Applied>,
}

impl Refactoring {
    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    pub fn recorded(&self) -> u64 {
        self.recorded
    }

    pub fn record(&mut self, files: Vec<Applied>) {
        if files.is_empty() {
            return;
        }
        self.recorded += 1;
        self.journal.push(JournalEntry { files });
        if self.journal.len() > JOURNAL_SIZE {
            self.journal.remove(0);
        }
    }

    pub(crate) fn take_latest(&mut self, wanted: impl Fn(&JournalEntry) -> bool) -> Option<JournalEntry> {
        let index = self.journal.iter().rposition(wanted)?;
        Some(self.journal.remove(index))
    }

    fn cancel(&mut self, languages: &mut Languages) {
        drop_lookup(languages, self.prepare.take());
        drop_lookup(languages, self.rename.take().map(|plan| plan.lookup));
    }
}

pub fn is_identifier(language: Option<&str>, name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    let shaped = (first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_');
    shaped && !(language == Some("go") && GO_KEYWORDS.contains(&name))
}

/// What `refactor.rename.prepare` found at the place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameTarget {
    /// Whether the server has not answered yet; call again to wait more.
    pub pending: bool,
    /// The first character of the name to rename, a character offset in the file.
    pub start: usize,
    /// The character after the name.
    pub end: usize,
    /// The name as it is now, to start the new name from.
    pub placeholder: String,
    /// Why there is nothing to rename, in words for the user.
    pub error: Option<String>,
}

impl RenameTarget {
    fn failed(error: String) -> Self {
        Self { error: Some(error), ..Self::default() }
    }
}

/// The parameters of `refactor.rename.preview`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewParams {
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
    /// The new name.
    pub new_name: String,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// One change a rename makes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameChange {
    /// The range replaced, with its line.
    #[serde(flatten)]
    pub place: Place,
    /// The text it becomes.
    pub new_text: String,
}

/// The changes a rename makes in one file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameFile {
    /// The file.
    pub path: PathBuf,
    /// The file's path relative to the project.
    pub relative: String,
    /// Its changes, in order.
    pub changes: Vec<RenameChange>,
}

/// What a file operation of a rename does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RenameOperationKind {
    /// A new file.
    Create,
    /// A file or folder moved or renamed.
    Rename,
    /// A file or folder deleted.
    Delete,
}

/// A file a rename creates, renames or deletes, in the order the server sent it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameOperation {
    /// What happens to the file.
    pub kind: RenameOperationKind,
    /// The file or folder; for a rename, its old path.
    pub path: PathBuf,
    /// `path` relative to the project.
    pub relative: String,
    /// The new path of a renamed file or folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_path: Option<PathBuf>,
    /// `newPath` relative to the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_relative: Option<String>,
}

/// What `refactor.rename.preview` would change.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreview {
    /// Whether the server has not answered yet; call again to wait more.
    pub pending: bool,
    /// The files changed, in path order, or in the server's order when it also creates, renames or
    /// deletes files.
    pub files: Vec<RenameFile>,
    /// The files created, renamed or deleted, in order.
    pub operations: Vec<RenameOperation>,
    /// How many changes there are in all.
    pub usage_count: usize,
    /// Why the rename cannot be done, in words for the user.
    pub error: Option<String>,
}

/// The parameters of `refactor.rename.apply`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplyParams {
    /// The new name; the previewed one when absent.
    #[serde(default)]
    pub new_name: Option<String>,
    /// The file; the previewed one, else the active one, when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in the file; `line` and `column` instead, else the previewed one, else
    /// the primary caret, when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// The line, one-based as answers count them: with `column`, the place instead of `offset`.
    #[serde(default)]
    pub line: Option<u32>,
    /// The column on `line`, one-based, in characters; 1 when absent.
    #[serde(default)]
    pub column: Option<u32>,
    /// Whether to save the open files the rename changed; an agent's rename saves them unless `false`.
    #[serde(default)]
    pub save: Option<bool>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000, which is the default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `refactor.rename.apply` changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplied {
    /// The files changed, created, renamed (their new paths) or deleted, in path order.
    pub files: Vec<PathBuf>,
    /// How many changes were made in all.
    pub edits: usize,
}

pub struct PrepareRename;

impl Command for PrepareRename {
    type Params = LookupParams;
    type Output = RenameTarget;
    const ID: &'static str = "refactor.rename.prepare";
    const TITLE: &'static str = "Prepare Rename";
    const DESCRIPTION: &'static str = "Asks the language server whether the symbol at a place (the caret by default) can be \
         renamed: returns its range and current name, or why not. Call again with waitMs while pending.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<RenameTarget, CommandError> {
        prepare_at(session, &params)
    }
}

pub struct PreviewRename;

impl Command for PreviewRename {
    type Params = RenamePreviewParams;
    type Output = RenamePreview;
    const ID: &'static str = "refactor.rename.preview";
    const TITLE: &'static str = "Preview Rename";
    const DESCRIPTION: &'static str = "Asks the language server what renaming the symbol at a place (the caret by default) to \
         newName changes, file by file, and the files it creates, renames or deletes, without changing anything; \
         refactor.rename.apply makes those changes.";

    fn run(&self, session: &mut Session, params: RenamePreviewParams) -> Result<RenamePreview, CommandError> {
        let mut preview = preview_at(session, &params)?;
        for file in &mut preview.files {
            file.path = crate::told_path(session, &file.path);
            for change in &mut file.changes {
                change.place.path = crate::told_path(session, &change.place.path);
            }
        }
        for operation in &mut preview.operations {
            operation.path = crate::told_path(session, &operation.path);
            operation.new_path = operation.new_path.as_deref().map(|path| crate::told_path(session, path));
        }
        Ok(preview)
    }
}

pub struct ApplyRename;

impl Command for ApplyRename {
    type Params = RenameApplyParams;
    type Output = RenameApplied;
    const ID: &'static str = "refactor.rename.apply";
    const TITLE: &'static str = "Rename";
    const DESCRIPTION: &'static str = "Renames the symbol at a place across the project: the previewed rename, or a new one to \
         newName, with the files the server creates, renames or deletes. Every file changes or none does; open files \
         keep the change unsaved unless `save` (an agent's rename saves them unless `save: false`), and Undo reverts \
         every file and file operation.";

    fn run(&self, session: &mut Session, params: RenameApplyParams) -> Result<RenameApplied, CommandError> {
        let mut applied = apply_rename(session, &params)?;
        let saving = crate::text_replace::saving(session, params.save);
        if saving != crate::text_replace::Saving::No {
            for path in &applied.files {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                crate::text_replace::save_with(session, path, &name, saving)?;
            }
        }
        applied.files = applied.files.iter().map(|path| crate::told_path(session, path)).collect();
        Ok(applied)
    }
}

pub struct CancelRename;

impl Command for CancelRename {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "refactor.rename.cancel";
    const TITLE: &'static str = "Cancel Rename";
    const DESCRIPTION: &'static str =
        "Forgets the rename being prepared or previewed, cancelling the server's requests that still run.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        cancel_rename(session);
        Ok(())
    }
}

pub(crate) fn prepare_at(session: &mut Session, params: &LookupParams) -> Result<RenameTarget, CommandError> {
    with_refactoring(session, |refactoring, languages, workbench| prepare(refactoring, languages, workbench, params))
}

pub(crate) fn preview_at(session: &mut Session, params: &RenamePreviewParams) -> Result<RenamePreview, CommandError> {
    with_refactoring(session, |refactoring, languages, workbench| preview(refactoring, languages, workbench, params))
}

pub(crate) fn apply_rename(session: &mut Session, params: &RenameApplyParams) -> Result<RenameApplied, CommandError> {
    session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
    let (applied, written) = with_refactoring(session, |refactoring, languages, workbench| {
        apply(refactoring, languages, workbench, params)
    })?;
    crate::local_history::changed_by(session, crate::local_history::RENAME, written);
    Ok(applied)
}

pub(crate) fn cancel_rename(session: &mut Session) {
    with_refactoring(session, |refactoring, languages, _| refactoring.cancel(languages));
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(PrepareRename)?;
    registry.register(PreviewRename)?;
    registry.register(ApplyRename)?;
    registry.register(CancelRename)?;
    Ok(())
}

pub(crate) fn merge_undo(session: &mut Session, log: Vec<Applied>) -> JournalEntry {
    let files = edits::journal(log, session.state_mut::<Workbench>());
    session.state_mut::<Refactoring>().record(files.clone());
    JournalEntry { files }
}

pub(crate) fn undo_recorded(session: &mut Session, entry: &JournalEntry) -> bool {
    session.state_mut::<Refactoring>().take_latest(|recorded| recorded == entry);
    with_languages(session, |languages, workbench| undo_entry(entry, languages, workbench))
}

pub(crate) fn undo(session: &mut Session) -> Option<bool> {
    let document = session.state::<Workbench>()?.active_document()?;
    let (path, version) = (document.path().to_owned(), document.version());
    let index = session
        .state::<Refactoring>()?
        .journal
        .iter()
        .rposition(|entry| edits::matches_document(&entry.files, &path, version))?;
    let entry = session.state_mut::<Refactoring>().journal.remove(index);
    Some(with_languages(session, |languages, workbench| undo_entry(&entry, languages, workbench)))
}

pub(crate) fn undo_entry(entry: &JournalEntry, languages: &mut Languages, workbench: &mut Workbench) -> bool {
    let (changed, skipped) = edits::revert(&entry.files, languages, workbench, true);
    if !skipped.is_empty() {
        let files: Vec<String> = skipped.iter().map(|path| relative(workbench.project(), path)).collect();
        workbench.messages.push(t!("workbench-undo-skipped", count = skipped.len(), files = files.join(", ")));
    }
    changed
}

fn with_refactoring<T>(
    session: &mut Session,
    f: impl FnOnce(&mut Refactoring, &mut Languages, &mut Workbench) -> T,
) -> T {
    let mut refactoring = std::mem::take(session.state_mut::<Refactoring>());
    let result = with_languages(session, |languages, workbench| {
        let result = f(&mut refactoring, languages, workbench);
        languages.hold("rename", refactoring.waiting());
        result
    });
    session.set_state(refactoring);
    result
}

fn prepare(
    refactoring: &mut Refactoring,
    languages: &mut Languages,
    workbench: &mut Workbench,
    params: &LookupParams,
) -> Result<RenameTarget, CommandError> {
    let (path, offset) = params.place(workbench)?;
    languages.sync(workbench);
    let reuse = refactoring
        .prepare
        .as_ref()
        .is_some_and(|lookup| lookup.path == path && lookup.offset == offset && is_current(languages, lookup));
    if !reuse {
        drop_lookup(languages, refactoring.prepare.take());
        let Some(client) = languages.client_for(&path) else {
            languages.start_for(&path);
            return Err(languages.lookup_error(&path, t!("workbench-language-not-running")));
        };
        match client.capabilities().and_then(|capabilities| capabilities.rename_provider.clone()) {
            None | Some(OneOf::Left(false)) => return Ok(RenameTarget::failed(t!("workbench-rename-not-here"))),
            Some(OneOf::Right(options)) if options.prepare_provider == Some(true) => {
                let request = request_at(languages, workbench, &path, offset, "textDocument/prepareRename", json!({}))
                    .map_err(|message| languages.lookup_error(&path, message))?;
                refactoring.prepare = Some(lookup(request, "textDocument/prepareRename", path.clone(), offset));
            }
            Some(_) => return Ok(identifier_target(workbench, &path, offset)),
        }
    }
    let Some(lookup) = refactoring.prepare.as_mut() else {
        return Ok(RenameTarget::failed(t!("workbench-rename-not-here")));
    };
    if !lookup.poll(params.wait()) {
        return Ok(RenameTarget { pending: true, ..RenameTarget::default() });
    }
    let answer = match lookup.answer.clone() {
        Some(Ok(answer)) => answer,
        Some(Err(error)) => return Ok(RenameTarget::failed(error)),
        None => return Ok(RenameTarget { pending: true, ..RenameTarget::default() }),
    };
    if answer.is_null() {
        return Ok(RenameTarget::failed(t!("workbench-rename-not-here")));
    }
    if answer["defaultBehavior"] == true {
        return Ok(identifier_target(workbench, &path, offset));
    }
    let range = if answer.get("range").is_some() { &answer["range"] } else { &answer };
    let Some(text) = languages.synced_text(&path) else {
        return Ok(RenameTarget::failed(t!("workbench-rename-not-here")));
    };
    let Some(found) = place(path.clone(), text, range) else {
        return Ok(RenameTarget::failed(t!("workbench-rename-not-here")));
    };
    let placeholder = match answer["placeholder"].as_str() {
        Some(placeholder) => placeholder.to_owned(),
        None => text.chars().skip(found.start).take(found.end - found.start).collect(),
    };
    Ok(RenameTarget { pending: false, start: found.start, end: found.end, placeholder, error: None })
}

fn preview(
    refactoring: &mut Refactoring,
    languages: &mut Languages,
    workbench: &mut Workbench,
    params: &RenamePreviewParams,
) -> Result<RenamePreview, CommandError> {
    let at = LookupParams {
        path: params.path.clone(),
        offset: params.offset,
        line: params.line,
        column: params.column,
        wait_ms: params.wait_ms,
    };
    let (path, offset) = at.place(workbench)?;
    if let Err(error) = check_name(languages, &path, &params.new_name) {
        return Ok(RenamePreview { error: Some(error), ..RenamePreview::default() });
    }
    languages.sync(workbench);
    let reuse = refactoring.rename.as_ref().is_some_and(|plan| {
        plan.new_name == params.new_name
            && plan.lookup.path == path
            && plan.lookup.offset == offset
            && (plan.files.is_some() || is_current(languages, &plan.lookup))
    });
    if !reuse {
        start_plan(refactoring, languages, workbench, &path, offset, &params.new_name)?;
    }
    let Some(plan) = refactoring.rename.as_mut() else { return Ok(RenamePreview::default()) };
    if !poll_plan(plan, languages, at.wait()) {
        return Ok(RenamePreview { pending: true, ..RenamePreview::default() });
    }
    match &plan.files {
        Some(Ok(steps)) => Ok(RenamePreview {
            pending: false,
            files: edits::text_files(steps).map(|file| preview_file(workbench.project(), file)).collect(),
            operations: edits::file_operations(steps)
                .map(|operation| preview_operation(workbench.project(), operation))
                .collect(),
            usage_count: edits::text_files(steps).map(|file| file.changes.len()).sum(),
            error: None,
        }),
        Some(Err(error)) => Ok(RenamePreview { error: Some(error.clone()), ..RenamePreview::default() }),
        None => Ok(RenamePreview { pending: true, ..RenamePreview::default() }),
    }
}

fn apply(
    refactoring: &mut Refactoring,
    languages: &mut Languages,
    workbench: &mut Workbench,
    params: &RenameApplyParams,
) -> Result<(RenameApplied, crate::local_history::Written), CommandError> {
    languages.sync(workbench);
    let at = LookupParams {
        path: params.path.clone(),
        offset: params.offset,
        line: params.line,
        column: params.column,
        wait_ms: None,
    };
    let offset = match params.line.filter(|_| params.offset.is_none()) {
        Some(_) => Some(at.place(workbench)?.1),
        None => params.offset,
    };
    let stored = refactoring.rename.as_ref().is_some_and(|plan| {
        params.new_name.as_ref().is_none_or(|name| *name == plan.new_name)
            && params.path.as_ref().is_none_or(|path| workbench.resolve(path) == plan.lookup.path)
            && offset.is_none_or(|offset| offset == plan.lookup.offset)
    });
    if !stored {
        let name = params.new_name.clone().unwrap_or_default();
        let (path, offset) = at.place(workbench)?;
        check_name(languages, &path, &name).map_err(CommandError::new)?;
        start_plan(refactoring, languages, workbench, &path, offset, &name)?;
    }
    let Some(mut plan) = refactoring.rename.take() else {
        return Err(CommandError::new(t!("workbench-rename-not-here")));
    };
    let wait = params.wait_ms.map_or(MAX_WAIT, |wait_ms| Duration::from_millis(wait_ms).min(MAX_WAIT));
    if !poll_plan(&mut plan, languages, wait) {
        drop_lookup(languages, Some(plan.lookup));
        return Err(CommandError::new(LspError::Timeout.to_string()));
    }
    let steps = match plan.files {
        Some(Ok(steps)) => steps,
        Some(Err(error)) => return Err(CommandError::new(error)),
        None => return Err(CommandError::new(LspError::Timeout.to_string())),
    };
    let applied = edits::apply(&steps, languages, workbench).map_err(|error| CommandError::new(error.message()))?;
    let written = applied.iter().flat_map(Applied::written).collect();
    refactoring.record(edits::journal(applied, workbench));
    let renamed = RenameApplied {
        files: edits::changed_paths(&steps),
        edits: edits::text_files(&steps).map(|file| file.changes.len()).sum(),
    };
    Ok((renamed, written))
}

fn start_plan(
    refactoring: &mut Refactoring,
    languages: &mut Languages,
    workbench: &Workbench,
    path: &Path,
    offset: usize,
    new_name: &str,
) -> Result<(), CommandError> {
    drop_lookup(languages, refactoring.rename.take().map(|plan| plan.lookup));
    let method = "textDocument/rename";
    let request = request_at(languages, workbench, path, offset, method, json!({ "newName": new_name }))
        .map_err(|message| languages.lookup_error(path, message))?;
    refactoring.rename = Some(Plan {
        new_name: new_name.to_owned(),
        lookup: lookup(request, method, path.to_owned(), offset),
        files: None,
    });
    Ok(())
}

fn poll_plan(plan: &mut Plan, languages: &mut Languages, wait: Duration) -> bool {
    if plan.files.is_some() {
        return true;
    }
    if !plan.lookup.poll(wait) {
        return false;
    }
    let files = match plan.lookup.answer.clone() {
        Some(Ok(_)) if !is_current(languages, &plan.lookup) => Err(t!("workbench-rename-files-changed")),
        Some(Ok(answer)) => edits::parse(&answer, languages).map_err(|error| error.message()),
        Some(Err(error)) => Err(error),
        None => return false,
    };
    plan.files = Some(files);
    true
}

fn check_name(languages: &Languages, path: &Path, name: &str) -> Result<(), String> {
    let language = languages.spec_for(path).map(|spec| spec.language_for(path));
    if is_identifier(language.as_deref(), name) {
        Ok(())
    } else {
        Err(t!("workbench-rename-invalid-name", name = name))
    }
}

fn lookup(request: zen_lsp::DocumentRequest, method: &str, path: PathBuf, offset: usize) -> Lookup {
    Lookup {
        method: method.to_owned(),
        path,
        offset,
        uri: request.uri.clone(),
        version: request.version,
        request: Some(request),
        answer: None,
        busy_server: false,
    }
}

fn is_current(languages: &Languages, lookup: &Lookup) -> bool {
    languages.server_version(&lookup.path) == Some(lookup.version)
}

fn drop_lookup(languages: &mut Languages, lookup: Option<Lookup>) {
    if let Some(mut lookup) = lookup
        && let Some(request) = lookup.request.take()
        && let Some(client) = languages.client_for(&lookup.path)
    {
        client.cancel(&request.pending);
    }
}

fn identifier_target(workbench: &Workbench, path: &Path, offset: usize) -> RenameTarget {
    let Ok(text) = file_text(workbench, path) else {
        return RenameTarget::failed(t!("workbench-rename-not-here"));
    };
    let chars: Vec<char> = text.chars().collect();
    let word = |c: &char| c.is_alphanumeric() || *c == '_';
    let offset = offset.min(chars.len());
    let start = offset - chars[..offset].iter().rev().take_while(|c| word(c)).count();
    let end = offset + chars[offset..].iter().take_while(|c| word(c)).count();
    if start == end {
        return RenameTarget::failed(t!("workbench-rename-not-here"));
    }
    RenameTarget { pending: false, start, end, placeholder: chars[start..end].iter().collect(), error: None }
}

pub(crate) fn preview_file(project: Option<&Path>, file: &FileEdit) -> RenameFile {
    RenameFile {
        path: file.path.clone(),
        relative: relative(project, &file.path),
        changes: file
            .changes
            .iter()
            .map(|change| RenameChange { place: change.place.clone(), new_text: change.new_text.clone() })
            .collect(),
    }
}

pub(crate) fn preview_operation(project: Option<&Path>, operation: &FileOperation) -> RenameOperation {
    let (kind, new_path) = match operation {
        FileOperation::Create { .. } => (RenameOperationKind::Create, None),
        FileOperation::Rename { to, .. } => (RenameOperationKind::Rename, Some(to.clone())),
        FileOperation::Delete { .. } => (RenameOperationKind::Delete, None),
    };
    RenameOperation {
        kind,
        path: operation.path().to_owned(),
        relative: relative(project, operation.path()),
        new_relative: new_path.as_deref().map(|path| relative(project, path)),
        new_path,
    }
}

pub(crate) fn relative(project: Option<&Path>, path: &Path) -> String {
    project.and_then(|project| path.strip_prefix(project).ok()).unwrap_or(path).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;

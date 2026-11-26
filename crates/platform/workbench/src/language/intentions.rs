use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::LspError;
use zen_lsp::lsp_types::CodeActionProviderCapability;

use super::edits::{self, Applied, EditError};
use super::refactor::{JournalEntry, Refactoring, undo_entry};
use super::{Languages, Lookup, MAX_WAIT, byte_offset, request_in, with_languages};
use crate::Workbench;
use crate::suppression::{ApplySuppression, SuppressParams};

const OWNER: &str = "intentions";

pub const SCRATCH_FROM_SELECTION: &str = "scratch.fromSelection";

#[derive(Default)]
pub struct Intentions {
    generation: u64,
    listing: Option<Listing>,
}

pub(super) struct OfferedAction {
    pub(super) path: PathBuf,
    pub(super) server: Option<String>,
    pub(super) action: Value,
}

struct Listing {
    generation: u64,
    path: PathBuf,
    range: Range<usize>,
    document_version: Option<u64>,
    lookup: Option<Lookup>,
    extras: Vec<(String, Lookup)>,
    actions: Option<Result<Vec<Offered>, String>>,
}

type Offered = (Option<String>, Value);

impl Intentions {
    pub(super) fn waiting(&self) -> BTreeSet<PathBuf> {
        self.listing
            .iter()
            .filter(|listing| {
                listing.lookup.as_ref().is_some_and(|lookup| lookup.request.is_some())
                    || listing.extras.iter().any(|(_, lookup)| lookup.request.is_some())
            })
            .map(|listing| listing.path.clone())
            .collect()
    }

    pub(super) fn offered(&self, id: &str) -> Option<OfferedAction> {
        let (generation, index) = id.split_once('.')?;
        let listing = self.listing.as_ref().filter(|listing| generation.parse() == Ok(listing.generation))?;
        let Some(Ok(actions)) = &listing.actions else { return None };
        let (server, action) = actions.get(index.parse::<usize>().ok()?)?.clone();
        Some(OfferedAction { path: listing.path.clone(), server, action })
    }

    pub(super) fn drop_listing(&mut self, languages: &mut Languages) {
        let Some(mut listing) = self.listing.take() else { return };
        if let Some(request) = listing.lookup.as_mut().and_then(|lookup| lookup.request.take())
            && let Some(client) = languages.client_for(&listing.path)
        {
            client.cancel(&request.pending);
        }
        for (server, lookup) in &mut listing.extras {
            if let Some(request) = lookup.request.take()
                && let Some(client) = languages.client_of(server)
            {
                client.cancel(&request.pending);
            }
        }
    }
}

/// The parameters of `editor.intentions.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentionsListParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in the file; the primary caret, or its selection, when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// A context action that `editor.intentions.apply` can apply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextAction {
    /// What `editor.intentions.apply` takes to apply it, valid until the next listing or change of the file.
    pub id: String,
    /// What it does, in the server's words.
    pub title: String,
    /// Its kind in the protocol, e.g. `quickfix`, `refactor.extract`, `source.organizeImports`.
    pub kind: Option<String>,
    /// Whether it fixes a problem (a quick fix) rather than changes working code (an intention).
    pub quick_fix: bool,
    /// Whether the server suggests it before the others.
    pub preferred: bool,
    /// Why it cannot be applied now, in the server's words, when it cannot.
    pub disabled: Option<String>,
}

/// What `editor.intentions.list` found at the place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentionList {
    /// Whether the server has not answered yet; call again to wait more.
    pub pending: bool,
    /// The actions, quick fixes first.
    pub actions: Vec<ContextAction>,
    /// Why there are none, in words for the user.
    pub error: Option<String>,
}

/// The parameters of `editor.intentions.apply`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentionApplyParams {
    /// The id of an action of the last `editor.intentions.list`.
    pub id: String,
    /// How long to wait for the server in milliseconds, at most 10 000, which is the default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `editor.intentions.apply` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentionApplied {
    /// The action's title.
    pub title: String,
    /// The files changed, in path order.
    pub files: Vec<PathBuf>,
    /// The command the server ran for it, when it has one.
    pub command: Option<String>,
}

pub struct ListIntentions;

impl Command for ListIntentions {
    type Params = IntentionsListParams;
    type Output = IntentionList;
    const ID: &'static str = "editor.intentions.list";
    const TITLE: &'static str = "List Context Actions";
    const DESCRIPTION: &'static str = "Asks the language server for the context actions at a place (the caret or its selection \
         by default): quick fixes of the problems there, refactorings and source actions, quick fixes first. Each has an \
         id for editor.intentions.apply. Call again with waitMs while pending. With text selected, Create new scratch \
         file from selection (id `scratch.fromSelection`) comes last, also when no server runs. In a string \
         literal that holds SQL, Run SQL (id `db.injection.run`, as db.injection.run) and the choice of the file's SQL \
         connection (id `db.injection.choose`, refused outside the window with code `chooseConnection`: use \
         db.injection.bind) come after the server's.";

    fn run(&self, session: &mut Session, params: IntentionsListParams) -> Result<IntentionList, CommandError> {
        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned());
        let at_selection = params.offset.is_none() && (params.path.is_none() || params.path == active);
        let scratch = at_selection
            && crate::scratch::has_selection(session)
            && session.state::<crate::scratch::Scratches>().is_some_and(|scratches| !scratches.folders().is_empty());
        let listed = with_intentions(session, |intentions, languages, workbench| {
            list(intentions, languages, workbench, &params, None)
        });
        let pending = listed.as_ref().is_ok_and(|list| list.pending);
        let mut extra: Vec<(String, String)> = if pending { Vec::new() } else { sql_intentions(session, &params) };
        if !pending {
            extra.extend(suppress_intentions(session, &params));
        }
        if scratch {
            extra.push((SCRATCH_FROM_SELECTION.to_owned(), t!("workbench-scratch-from-selection")));
        }
        if extra.is_empty() {
            return listed;
        }
        let mut list =
            listed.unwrap_or_else(|error| IntentionList { error: Some(error.message), ..IntentionList::default() });
        list.actions.extend(extra.into_iter().map(|(id, title)| ContextAction {
            id,
            title,
            kind: None,
            quick_fix: false,
            preferred: false,
            disabled: None,
        }));
        Ok(list)
    }
}

pub struct ApplyIntention;

impl Command for ApplyIntention {
    type Params = IntentionApplyParams;
    type Output = IntentionApplied;
    const ID: &'static str = "editor.intentions.apply";
    const TITLE: &'static str = "Apply Context Action";
    const DESCRIPTION: &'static str = "Applies a context action listed by editor.intentions.list: resolves it with the server \
         when needed, makes its edit (text changes and the files it creates, renames or deletes, inside the project) and \
         runs its command, applying the edits the server sends meanwhile: all of it in every file or none (open files keep it unsaved, one Undo reverts it). Refused for a read-only file or a file \
         outside the project.";

    fn run(&self, session: &mut Session, params: IntentionApplyParams) -> Result<IntentionApplied, CommandError> {
        #[cfg(not(target_arch = "wasm32"))]
        if params.id == crate::database::injected::RUN_ACTION {
            let run = crate::database::injected::RunInjection.run(session, Default::default())?;
            return Ok(IntentionApplied {
                title: t!("database-injection-run"),
                files: vec![run.run.path],
                command: None,
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        if params.id == crate::database::injected::CHOOSE_ACTION {
            return Err(crate::database::injected::choose_refusal(session));
        }
        if params.id == SCRATCH_FROM_SELECTION {
            let path = crate::scratch::scratch_from_selection(session)?;
            return Ok(IntentionApplied {
                title: t!("workbench-scratch-from-selection"),
                files: vec![path],
                command: None,
            });
        }
        if let Some(option) = crate::suppression::offered(session, &params.id) {
            let (title, path) = (option.title, option.path);
            let suppress = SuppressParams {
                path: path.clone(),
                offset: option.offset,
                inspection: option.inspection,
                scope: option.scope,
            };
            ApplySuppression.run(session, suppress)?;
            return Ok(IntentionApplied { title, files: vec![path], command: None });
        }
        let allowed =
            session.state::<crate::scratch::Scratches>().map(crate::scratch::Scratches::folders).unwrap_or_default();
        session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
        let (result, applied) = with_intentions(session, |intentions, languages, workbench| {
            apply(intentions, languages, workbench, &params, &allowed)
        });
        session.state_mut::<Refactoring>().record(applied);
        result
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn sql_intentions(session: &Session, params: &IntentionsListParams) -> Vec<(String, String)> {
    crate::database::injected::intentions(session, params.path.as_deref(), params.offset)
        .into_iter()
        .map(|(id, title)| (id.to_owned(), title))
        .collect()
}

#[cfg(target_arch = "wasm32")]
fn sql_intentions(_: &Session, _: &IntentionsListParams) -> Vec<(String, String)> {
    Vec::new()
}

fn suppress_intentions(session: &mut Session, params: &IntentionsListParams) -> Vec<(String, String)> {
    let Some(workbench) = session.state::<Workbench>() else { return Vec::new() };
    let Ok((path, range)) = place(workbench, params) else { return Vec::new() };
    crate::suppression::intentions(session, &path, range.end)
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListIntentions)?;
    registry.register(ApplyIntention)?;
    Ok(())
}

fn with_intentions<T>(
    session: &mut Session,
    f: impl FnOnce(&mut Intentions, &mut Languages, &mut Workbench) -> T,
) -> T {
    let mut intentions = std::mem::take(session.state_mut::<Intentions>());
    let result = with_languages(session, |languages, workbench| {
        let result = f(&mut intentions, languages, workbench);
        languages.hold(OWNER, intentions.waiting());
        result
    });
    session.set_state(intentions);
    result
}

fn place(workbench: &Workbench, params: &IntentionsListParams) -> Result<(PathBuf, Range<usize>), CommandError> {
    let path = params
        .path
        .clone()
        .or_else(|| workbench.active_document().map(|document| document.path().to_owned()))
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    if let Some(offset) = params.offset {
        return Ok((path, offset..offset));
    }
    let document = workbench
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    Ok((path, document.selections().primary().range()))
}

fn document_version(workbench: &Workbench, path: &Path) -> Option<u64> {
    workbench.documents().iter().find(|document| document.path() == path).map(crate::Document::version)
}

fn is_current(languages: &Languages, workbench: &Workbench, listing: &Listing) -> bool {
    listing.lookup.as_ref().is_none_or(|lookup| languages.server_version(&listing.path) == Some(lookup.version))
        && document_version(workbench, &listing.path) == listing.document_version
}

fn lookup_of(path: &Path, offset: usize, request: zen_lsp::DocumentRequest) -> Lookup {
    Lookup {
        method: "textDocument/codeAction".to_owned(),
        path: path.to_owned(),
        offset,
        uri: request.uri.clone(),
        version: request.version,
        request: Some(request),
        answer: None,
        busy_server: false,
    }
}

fn alongside_with_actions(languages: &mut Languages, path: &Path) -> Vec<String> {
    languages
        .alongside_running(path)
        .into_iter()
        .filter(|id| languages.client_of(id).is_some_and(|client| gives_actions(client)))
        .collect()
}

fn gives_actions(client: &zen_lsp::Client) -> bool {
    !matches!(
        client.capabilities().and_then(|capabilities| capabilities.code_action_provider.as_ref()),
        None | Some(CodeActionProviderCapability::Simple(false))
    )
}

pub(super) fn list(
    intentions: &mut Intentions,
    languages: &mut Languages,
    workbench: &mut Workbench,
    params: &IntentionsListParams,
    only: Option<&[&str]>,
) -> Result<IntentionList, CommandError> {
    let (path, requested) = place(workbench, params)?;
    languages.sync(workbench);
    let reuse = intentions.listing.as_ref().is_some_and(|listing| {
        listing.path == path && listing.range == requested && is_current(languages, workbench, listing)
    });
    if !reuse {
        intentions.drop_listing(languages);
        let alongside = alongside_with_actions(languages, &path);
        let primary = match provides_code_actions(languages, workbench, &path) {
            Ok(gives) => gives,
            Err(_) if !alongside.is_empty() => false,
            Err(error) => return Err(error),
        };
        if !primary && alongside.is_empty() {
            return Ok(IntentionList::default());
        }
        let (range, diagnostics) = context(languages, workbench, &path, requested.clone());
        let params = |uri: &str, text: &str| {
            let position = |offset: usize| zen_lsp::position::position(text, byte_offset(text, offset));
            let mut params = json!({
                "textDocument": { "uri": uri },
                "range": { "start": position(range.start), "end": position(range.end) },
                "context": { "diagnostics": diagnostics, "triggerKind": 1 }
            });
            if let Some(only) = only {
                params["context"]["only"] = json!(only);
            }
            params
        };
        let lookup = if primary {
            let request = request_in(languages, workbench, &path, "textDocument/codeAction", params)
                .map_err(CommandError::new)?;
            Some(lookup_of(&path, range.start, request))
        } else {
            None
        };
        let extras = alongside
            .into_iter()
            .filter_map(|server| {
                let request = languages.request_to(&server, &path, "textDocument/codeAction", params).ok()?;
                Some((server, lookup_of(&path, range.start, request)))
            })
            .collect();
        intentions.generation += 1;
        intentions.listing = Some(Listing {
            generation: intentions.generation,
            document_version: document_version(workbench, &path),
            path,
            range: requested,
            lookup,
            extras,
            actions: None,
        });
    }
    let Some(listing) = intentions.listing.as_mut() else { return Ok(IntentionList::default()) };
    if listing.actions.is_none() {
        let deadline = Instant::now() + super::wait(params.wait_ms);
        let left = || deadline.saturating_duration_since(Instant::now());
        let mut done = listing.lookup.as_mut().is_none_or(|lookup| lookup.poll(left()));
        for (_, lookup) in &mut listing.extras {
            done &= lookup.poll(left());
        }
        if !done {
            return Ok(IntentionList { pending: true, ..IntentionList::default() });
        }
        let primary = listing.lookup.as_mut().and_then(|lookup| lookup.answer.take());
        let mut merged: Vec<Offered> = Vec::new();
        let mut failed = None;
        match primary {
            Some(Ok(answer)) => merged.extend(sorted(&answer).into_iter().map(|action| (None, action))),
            Some(Err(error)) => failed = Some(error),
            None => {}
        }
        for (server, lookup) in &mut listing.extras {
            if let Some(Ok(answer)) = lookup.answer.take() {
                merged.extend(sorted(&answer).into_iter().map(|action| (Some(server.clone()), action)));
            }
        }
        merged.sort_by_key(|(_, action)| (!is_quick_fix(action), action["isPreferred"] != true));
        listing.actions = Some(match failed {
            Some(error) if merged.is_empty() => Err(error),
            _ => Ok(merged),
        });
    }
    match &listing.actions {
        Some(Ok(actions)) => Ok(IntentionList {
            pending: false,
            actions: actions
                .iter()
                .enumerate()
                .map(|(index, (_, action))| shown(listing.generation, index, action))
                .collect(),
            error: None,
        }),
        Some(Err(error)) => Ok(IntentionList { error: Some(error.clone()), ..IntentionList::default() }),
        None => Ok(IntentionList { pending: true, ..IntentionList::default() }),
    }
}

fn provides_code_actions(languages: &mut Languages, workbench: &Workbench, path: &Path) -> Result<bool, CommandError> {
    if !languages.serves(path) {
        return Err(CommandError::new(t!("workbench-language-not-running")));
    }
    let Some(client) = languages.client_for(path) else {
        if workbench.documents().iter().all(|document| document.path() != path) {
            languages.start_for(path);
        }
        return Err(CommandError::new(t!("workbench-language-not-running")));
    };
    Ok(match client.capabilities().and_then(|capabilities| capabilities.code_action_provider.as_ref()) {
        None | Some(CodeActionProviderCapability::Simple(false)) => false,
        Some(_) => true,
    })
}

fn context(
    languages: &Languages,
    workbench: &Workbench,
    path: &Path,
    range: Range<usize>,
) -> (Range<usize>, Vec<Value>) {
    let diagnostics = languages.diagnostics(path);
    let overlapping: Vec<&super::Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.start <= range.end && range.start <= diagnostic.end)
        .collect();
    if !overlapping.is_empty() || !range.is_empty() {
        return (range, overlapping.into_iter().map(|diagnostic| diagnostic.raw.clone()).collect());
    }
    let Ok(text) = super::file_text(workbench, path) else { return (range, Vec::new()) };
    let chars: Vec<char> = text.chars().collect();
    let caret = range.start.min(chars.len());
    let line_start = caret - chars[..caret].iter().rev().take_while(|c| **c != '\n').count();
    let line_end = caret + chars[caret..].iter().take_while(|c| **c != '\n').count();
    let on_line: Vec<&super::Diagnostic> =
        diagnostics.iter().filter(|diagnostic| diagnostic.start <= line_end && line_start <= diagnostic.end).collect();
    let Some(start) = on_line.iter().map(|diagnostic| diagnostic.start).min() else { return (range, Vec::new()) };
    let end = on_line.iter().map(|diagnostic| diagnostic.end).max().unwrap_or(start);
    (start..end, on_line.into_iter().map(|diagnostic| diagnostic.raw.clone()).collect())
}

fn is_quick_fix(action: &Value) -> bool {
    action["kind"].as_str().is_some_and(|kind| kind == "quickfix" || kind.starts_with("quickfix."))
        || action["diagnostics"].as_array().is_some_and(|diagnostics| !diagnostics.is_empty())
}

fn sorted(answer: &Value) -> Vec<Value> {
    let mut actions: Vec<Value> = answer.as_array().cloned().unwrap_or_default();
    actions.retain(|action| action["title"].is_string());
    actions.sort_by_key(|action| (!is_quick_fix(action), action["isPreferred"] != true));
    actions
}

fn shown(generation: u64, index: usize, action: &Value) -> ContextAction {
    let bare_command = action["command"].is_string();
    ContextAction {
        id: format!("{generation}.{index}"),
        title: action["title"].as_str().unwrap_or_default().to_owned(),
        kind: if bare_command { None } else { action["kind"].as_str().map(str::to_owned) },
        quick_fix: !bare_command && is_quick_fix(action),
        preferred: action["isPreferred"] == true,
        disabled: action["disabled"]["reason"].as_str().map(str::to_owned),
    }
}

pub(super) fn edit_message(error: &EditError) -> String {
    match error {
        EditError::Unsupported => t!("workbench-intentions-unsupported-change"),
        EditError::FilesChanged => t!("workbench-intentions-files-changed"),
        other => other.message(),
    }
}

pub(super) fn apply(
    intentions: &mut Intentions,
    languages: &mut Languages,
    workbench: &mut Workbench,
    params: &IntentionApplyParams,
    allowed: &[PathBuf],
) -> (Result<IntentionApplied, CommandError>, Vec<Applied>) {
    let deadline =
        Instant::now() + params.wait_ms.map_or(MAX_WAIT, |wait_ms| Duration::from_millis(wait_ms).min(MAX_WAIT));
    let unknown = || CommandError::new(t!("workbench-intentions-unknown", id = params.id.as_str()));
    let Some((generation, index)) = params
        .id
        .split_once('.')
        .and_then(|(generation, index)| Some((generation.parse::<u64>().ok()?, index.parse::<usize>().ok()?)))
    else {
        return (Err(unknown()), Vec::new());
    };
    languages.sync(workbench);
    let Some(listing) = intentions.listing.as_ref().filter(|listing| listing.generation == generation) else {
        return (Err(unknown()), Vec::new());
    };
    let Some(Ok(actions)) = &listing.actions else { return (Err(unknown()), Vec::new()) };
    let Some((server, mut action)) = actions.get(index).cloned() else { return (Err(unknown()), Vec::new()) };
    if !is_current(languages, workbench, listing) {
        return (Err(CommandError::new(t!("workbench-intentions-stale"))), Vec::new());
    }
    let path = listing.path.clone();
    if let Some(reason) =
        workbench.documents().iter().find(|document| document.path() == path).and_then(crate::Document::read_only)
    {
        return (Err(CommandError::new(reason.to_owned())), Vec::new());
    }
    if let Some(reason) = action["disabled"]["reason"].as_str() {
        return (Err(CommandError::new(reason.to_owned())), Vec::new());
    }
    intentions.drop_listing(languages);
    let title = action["title"].as_str().unwrap_or_default().to_owned();
    let command = if action["command"].is_string() {
        Some(action.clone())
    } else {
        if !action["edit"].is_object()
            && !action["command"].is_object()
            && resolves(languages, server.as_deref(), &path)
        {
            match resolve(languages, server.as_deref(), &path, &action, deadline) {
                Ok(resolved) => action = resolved,
                Err(error) => return (Err(CommandError::new(error)), Vec::new()),
            }
        }
        action["command"].is_object().then(|| action["command"].clone())
    };
    let mut applied = Vec::new();
    if action["edit"].is_object() {
        match apply_edit(&action["edit"], languages, workbench, allowed) {
            Ok(files) => applied = files,
            Err(error) => return (Err(CommandError::new(edit_message(&error))), Vec::new()),
        }
    } else if command.is_none() {
        return (Err(CommandError::new(t!("workbench-intentions-nothing"))), Vec::new());
    }
    let mut result = Ok(());
    if let Some(command) = &command {
        let (sent, more) = execute(languages, workbench, server.as_deref(), &path, command, deadline, allowed);
        applied.extend(more);
        result = sent;
    }
    let applied = edits::journal(applied, workbench);
    if let Err(error) = result {
        undo_entry(&JournalEntry { files: applied }, languages, workbench);
        return (Err(CommandError::new(error)), Vec::new());
    }
    let mut files: Vec<PathBuf> = applied.iter().map(|applied| applied.path().to_owned()).collect();
    files.sort();
    files.dedup();
    let command = command.as_ref().and_then(|command| command["command"].as_str()).map(str::to_owned);
    (Ok(IntentionApplied { title, files, command }), applied)
}

pub(super) fn apply_edit(
    edit: &Value,
    languages: &mut Languages,
    workbench: &mut Workbench,
    allowed: &[PathBuf],
) -> Result<Vec<Applied>, EditError> {
    let files = edits::parse(edit, languages)?;
    edits::confine(&files, workbench, allowed)?;
    edits::apply(&files, languages, workbench)
}

pub(super) fn client_of<'a>(
    languages: &'a mut Languages,
    server: Option<&str>,
    path: &Path,
) -> Option<&'a mut zen_lsp::Client> {
    match server {
        Some(id) => languages.client_of(id),
        None => languages.client_for(path),
    }
}

pub(super) fn resolves(languages: &mut Languages, server: Option<&str>, path: &Path) -> bool {
    match server {
        Some(id) => languages.client_of(id).is_some_and(|client| {
            matches!(
                client.capabilities().and_then(|capabilities| capabilities.code_action_provider.clone()),
                Some(CodeActionProviderCapability::Options(options)) if options.resolve_provider == Some(true)
            )
        }),
        None => super::format::can_resolve(languages, path),
    }
}

pub(super) fn resolve(
    languages: &mut Languages,
    server: Option<&str>,
    path: &Path,
    action: &Value,
    deadline: Instant,
) -> Result<Value, String> {
    let client = client_of(languages, server, path).ok_or_else(|| t!("workbench-language-not-running"))?;
    let pending = client.request("codeAction/resolve", action.clone()).map_err(|error| error.to_string())?;
    match pending.wait(deadline.saturating_duration_since(Instant::now())) {
        Ok(resolved) if resolved.is_object() => Ok(resolved),
        Ok(_) => Ok(action.clone()),
        Err(error) => {
            if error == LspError::Timeout {
                client.cancel(&pending);
            }
            Err(error.to_string())
        }
    }
}

pub(crate) fn execute(
    languages: &mut Languages,
    workbench: &mut Workbench,
    from: Option<&str>,
    path: &Path,
    command: &Value,
    deadline: Instant,
    allowed: &[PathBuf],
) -> (Result<(), String>, Vec<Applied>) {
    let Some(server) = from.map(str::to_owned).or_else(|| languages.server_id(path)) else {
        return (Err(t!("workbench-language-not-running")), Vec::new());
    };
    languages.refuse_server_requests(&server);
    let Some(client) = languages.client_of(&server) else {
        return (Err(t!("workbench-language-not-running")), Vec::new());
    };
    let arguments = command.get("arguments").cloned().unwrap_or_else(|| json!([]));
    let request = json!({ "command": command["command"], "arguments": arguments });
    let pending = match client.request("workspace/executeCommand", request) {
        Ok(pending) => pending,
        Err(error) => return (Err(error.to_string()), Vec::new()),
    };
    languages.set_executing(Some(&server));
    let mut applied = Vec::new();
    let mut refused: Option<String> = None;
    let answer = loop {
        languages.sync(workbench);
        for request in languages.take_server_requests(&server) {
            let made = match &refused {
                Some(reason) => Err(reason.clone()),
                None => apply_edit(&request.params["edit"], languages, workbench, allowed)
                    .map_err(|error| edit_message(&error)),
            };
            let reply = match made {
                Ok(files) => {
                    applied.extend(files);
                    json!({ "applied": true })
                }
                Err(reason) => {
                    refused.get_or_insert_with(|| reason.clone());
                    json!({ "applied": false, "failureReason": reason })
                }
            };
            languages.respond(&server, &request.id, reply);
        }
        match pending.wait(Duration::from_millis(5)) {
            Err(LspError::Timeout) if Instant::now() < deadline => {}
            Err(LspError::Timeout) => {
                if let Some(client) = languages.client_of(&server) {
                    client.cancel(&pending);
                }
                break Err(LspError::Timeout.to_string());
            }
            answer => break answer.map(|_| ()).map_err(|error| error.to_string()),
        }
    };
    languages.set_executing(None);
    languages.refuse_server_requests(&server);
    let result = match (answer, refused) {
        (_, Some(reason)) | (Err(reason), None) => Err(reason),
        (Ok(()), None) => Ok(()),
    };
    (result, applied)
}

#[cfg(test)]
mod tests;

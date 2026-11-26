use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::LspError;
use zen_lsp::lsp_types::OneOf;
use zen_syntax::SymbolKind;

use super::edits::{self, Applied};
use super::places::{Place, TextCache, place, places};
use super::refactor::{Refactoring, RenameChange, RenameFile, RenameOperation, RenameOperationKind, relative};
use super::structure::{self, Symbol};
use super::usages::{UsagesFile, group};
use super::{Languages, Lookup, LookupParams, MAX_WAIT, byte_offset, request_at, request_in, with_languages};
use crate::Workbench;

const OWNER: &str = "safeDelete";

/// What Safe Delete deletes: the declaration at a place, or files.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SafeDeleteParams {
    /// The file of the declaration; the active one when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The character offset of the declaration's name in the file; `line` and `column` instead, else the primary caret, when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    /// The line, one-based as answers count them: with `column`, the place instead of `offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column on `line`, one-based, in characters; 1 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Files of the project to delete instead of a declaration: the usages of their declarations
    /// from other files are searched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<PathBuf>>,
    /// How long to wait for the language servers in milliseconds, at most 10 000; none by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

/// The parameters of `refactor.safeDelete.apply`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SafeDeleteApplyParams {
    /// What to delete.
    #[serde(flatten)]
    pub target: SafeDeleteParams,
    /// Whether to delete even though usages were found; refused when false and there are usages.
    #[serde(default)]
    pub force: bool,
}

/// What `refactor.safeDelete.find` found.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SafeDeleteSearch {
    /// Whether the language servers have not answered yet; call again to wait more.
    pub pending: bool,
    /// The declaration's name, or the files' names.
    pub name: String,
    /// The usages that keep it from being deleted safely, by file.
    pub usages: Vec<UsagesFile>,
    /// How many usages there are in all.
    pub total: usize,
    /// The text the deletion removes.
    pub deletes: Vec<RenameFile>,
    /// The files the deletion moves to the Trash.
    pub operations: Vec<RenameOperation>,
    /// Why nothing can be deleted, in words for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `refactor.safeDelete.apply` deleted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SafeDeleteApplied {
    /// The files changed or moved to the Trash.
    pub files: Vec<PathBuf>,
    /// The name of what was deleted.
    pub name: String,
}

#[derive(Default)]
pub struct SafeDeletion {
    search: Option<Search>,
}

impl SafeDeletion {
    fn waiting(&self) -> BTreeSet<PathBuf> {
        let Some(search) = &self.search else { return BTreeSet::new() };
        let lookups: Vec<&Lookup> = match &search.stage {
            Stage::Symbols(lookups) => lookups.iter().collect(),
            Stage::Usages { lookups, .. } => lookups.iter().map(|(_, lookup)| lookup).collect(),
            Stage::Done(_) => Vec::new(),
        };
        lookups.into_iter().filter(|lookup| lookup.request.is_some()).map(|lookup| lookup.path.clone()).collect()
    }

    fn cancel(&mut self, languages: &mut Languages) {
        let Some(search) = self.search.take() else { return };
        let lookups: Vec<Lookup> = match search.stage {
            Stage::Symbols(lookups) => lookups,
            Stage::Usages { lookups, .. } => lookups.into_iter().map(|(_, lookup)| lookup).collect(),
            Stage::Done(_) => Vec::new(),
        };
        for mut lookup in lookups {
            if let Some(request) = lookup.request.take()
                && let Some(client) = languages.client_for(&lookup.path)
            {
                client.cancel(&request.pending);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Symbol { path: PathBuf, offset: usize },
    Files(Vec<PathBuf>),
}

struct Search {
    target: Target,
    stage: Stage,
}

enum Stage {
    Symbols(Vec<Lookup>),
    Usages { declarations: Vec<Declaration>, lookups: Vec<(usize, Lookup)> },
    Done(Result<Found, String>),
}

#[derive(Clone, Debug)]
struct Declaration {
    path: PathBuf,
    name: String,
    version: i32,
    selection_start: usize,
    delete: (usize, usize),
}

#[derive(Clone, Debug)]
struct Found {
    declarations: Vec<Declaration>,
    usages: Vec<Place>,
}

pub struct FindSafeDelete;

impl Command for FindSafeDelete {
    type Params = SafeDeleteParams;
    type Output = SafeDeleteSearch;
    const ID: &'static str = "refactor.safeDelete.find";
    const TITLE: &'static str = "Find Usages Before Safe Delete";
    const DESCRIPTION: &'static str = "Finds what keeps a declaration (the one named at a place, the caret by default) or files \
         (`paths`) from being deleted safely: the usages the language servers know of outside it, with the text and \
         files the deletion would remove, changing nothing. Call again with waitMs while pending; \
         refactor.safeDelete.apply deletes.";

    fn run(&self, session: &mut Session, params: SafeDeleteParams) -> Result<SafeDeleteSearch, CommandError> {
        find(session, &params, wait(params.wait_ms, Duration::ZERO))
    }
}

pub struct ApplySafeDelete;

impl Command for ApplySafeDelete {
    type Params = SafeDeleteApplyParams;
    type Output = SafeDeleteApplied;
    const ID: &'static str = "refactor.safeDelete.apply";
    const TITLE: &'static str = "Safe Delete";
    const DESCRIPTION: &'static str = "Deletes a declaration (the one named at a place, the caret by default: its whole text with \
         its attributes, annotations and doc comment) or files (`paths`, moved to the Trash as file.fs.delete does) when \
         nothing outside uses it; with usages it refuses unless `force`. Waits up to waitMs (10 000 by default) for the \
         search; one Undo reverts a declaration's deletion.";

    fn run(&self, session: &mut Session, params: SafeDeleteApplyParams) -> Result<SafeDeleteApplied, CommandError> {
        apply(session, &params.target, params.force)
    }
}

pub struct CancelSafeDelete;

impl Command for CancelSafeDelete {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "refactor.safeDelete.cancel";
    const TITLE: &'static str = "Cancel Safe Delete";
    const DESCRIPTION: &'static str =
        "Forgets the Safe Delete search, cancelling the language servers' requests that still run.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        cancel(session);
        Ok(())
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(FindSafeDelete)?;
    registry.register(ApplySafeDelete)?;
    registry.register(CancelSafeDelete)?;
    Ok(())
}

pub(crate) fn cancel(session: &mut Session) {
    with_deletion(session, |deletion, languages, _| deletion.cancel(languages));
}

pub(crate) fn find(
    session: &mut Session,
    params: &SafeDeleteParams,
    wait: Duration,
) -> Result<SafeDeleteSearch, CommandError> {
    let target = target(session, params)?;
    let paths: Vec<&Path> = match &target {
        Target::Symbol { path, .. } => vec![path],
        Target::Files(paths) => paths.iter().map(PathBuf::as_path).collect(),
    };
    crate::hidden_files::guard_read(session, &paths)?;
    with_deletion(session, |deletion, languages, workbench| search(deletion, languages, workbench, target, wait))
}

pub(crate) fn apply(
    session: &mut Session,
    params: &SafeDeleteParams,
    force: bool,
) -> Result<SafeDeleteApplied, CommandError> {
    cancel(session);
    let found = find(session, params, wait(params.wait_ms, MAX_WAIT))?;
    if found.pending {
        return Err(CommandError::new(LspError::Timeout.to_string()));
    }
    if let Some(error) = found.error {
        return Err(CommandError::new(error));
    }
    if found.total > 0 && !force {
        return Err(CommandError::new(t!(
            "workbench-safe-delete-has-usages",
            name = found.name.as_str(),
            count = found.total
        )));
    }
    let target = target(session, params)?;
    let mut deletion = std::mem::take(session.state_mut::<SafeDeletion>());
    let search = deletion.search.take();
    session.set_state(deletion);
    let Some(Search { stage: Stage::Done(Ok(found_now)), .. }) = search else {
        return Err(CommandError::new(t!("workbench-safe-delete-not-here")));
    };
    match target {
        Target::Files(paths) => {
            let workbench = session.state::<Workbench>();
            let unsaved = workbench
                .into_iter()
                .flat_map(Workbench::documents)
                .find(|document| document.is_dirty() && paths.iter().any(|path| document.path().starts_with(path)));
            if let Some(document) = unsaved {
                return Err(CommandError::new(t!(
                    "workbench-safe-delete-unsaved",
                    file = relative(session.state::<Workbench>().and_then(Workbench::project), document.path())
                )));
            }
            let files = crate::delete_entries(session, &paths)?;
            Ok(SafeDeleteApplied { files, name: found.name })
        }
        Target::Symbol { .. } => {
            session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
            let Some(declaration) = found_now.declarations.first().cloned() else {
                return Err(CommandError::new(t!("workbench-safe-delete-not-here")));
            };
            let (written, entry) = with_languages(session, |languages, workbench| {
                let applied = delete_text(&declaration, languages, workbench)?;
                let written: crate::local_history::Written = applied.iter().flat_map(Applied::written).collect();
                Ok::<_, CommandError>((written, edits::journal(applied, workbench)))
            })?;
            session.state_mut::<Refactoring>().record(entry);
            crate::local_history::changed_by(session, crate::local_history::SAFE_DELETE, written);
            Ok(SafeDeleteApplied { files: vec![declaration.path], name: declaration.name })
        }
    }
}

fn delete_text(
    declaration: &Declaration,
    languages: &mut Languages,
    workbench: &mut Workbench,
) -> Result<Vec<Applied>, CommandError> {
    languages.sync(workbench);
    if languages.server_version(&declaration.path) != Some(declaration.version) {
        return Err(CommandError::new(t!("workbench-rename-files-changed")));
    }
    let text = languages
        .synced_text(&declaration.path)
        .map(str::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-rename-files-changed")))?;
    let position = |chars: usize| zen_lsp::position::position(&text, byte_offset(&text, chars));
    let (from, to) = declaration.delete;
    let uri = zen_lsp::position::file_uri(&declaration.path);
    let edit =
        json!({ "changes": { uri: [{ "range": { "start": position(from), "end": position(to) }, "newText": "" }] } });
    let steps = edits::parse(&edit, languages).map_err(|error| CommandError::new(error.message()))?;
    edits::apply(&steps, languages, workbench).map_err(|error| CommandError::new(error.message()))
}

fn wait(wait_ms: Option<u64>, default: Duration) -> Duration {
    wait_ms.map_or(default, |wait_ms| Duration::from_millis(wait_ms).min(MAX_WAIT))
}

fn target(session: &Session, params: &SafeDeleteParams) -> Result<Target, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    if let Some(paths) = params.paths.as_ref().filter(|paths| !paths.is_empty()) {
        let mut paths: Vec<PathBuf> = paths.iter().map(|path| workbench.resolve(path)).collect();
        paths.sort();
        paths.dedup();
        return Ok(Target::Files(paths));
    }
    let at = LookupParams {
        path: params.path.clone(),
        offset: params.offset,
        line: params.line,
        column: params.column,
        wait_ms: None,
    };
    let (path, offset) = at.place(workbench)?;
    Ok(Target::Symbol { path, offset })
}

fn with_deletion<T>(
    session: &mut Session,
    f: impl FnOnce(&mut SafeDeletion, &mut Languages, &mut Workbench) -> T,
) -> T {
    let mut deletion = std::mem::take(session.state_mut::<SafeDeletion>());
    let result = with_languages(session, |languages, workbench| {
        let result = f(&mut deletion, languages, workbench);
        languages.hold(OWNER, deletion.waiting());
        result
    });
    session.set_state(deletion);
    result
}

fn search(
    deletion: &mut SafeDeletion,
    languages: &mut Languages,
    workbench: &mut Workbench,
    target: Target,
    wait: Duration,
) -> Result<SafeDeleteSearch, CommandError> {
    languages.sync(workbench);
    let reuse = deletion.search.as_ref().is_some_and(|search| search.target == target && is_current(search, languages));
    if !reuse {
        deletion.cancel(languages);
        match start(languages, workbench, &target) {
            Ok(search) => deletion.search = Some(search),
            Err(error) if error.code.as_deref() == Some("starting") => {
                return Ok(SafeDeleteSearch { pending: true, ..SafeDeleteSearch::default() });
            }
            Err(error) => return Err(error),
        }
    }
    let Some(search) = deletion.search.as_mut() else { return Ok(SafeDeleteSearch::default()) };
    let deadline = Instant::now() + wait;
    advance(search, languages, workbench, deadline);
    Ok(report(search, workbench))
}

fn is_current(search: &Search, languages: &Languages) -> bool {
    match (&search.stage, &search.target) {
        (Stage::Done(Ok(found)), Target::Symbol { .. }) => found
            .declarations
            .iter()
            .all(|declaration| languages.server_version(&declaration.path) == Some(declaration.version)),
        (Stage::Done(Err(_)), _) => false,
        _ => true,
    }
}

fn start(languages: &mut Languages, workbench: &Workbench, target: &Target) -> Result<Search, CommandError> {
    let files = match target {
        Target::Symbol { path, .. } => vec![path.clone()],
        Target::Files(paths) => paths.iter().flat_map(|path| files_under(path)).collect(),
    };
    let symbol = matches!(target, Target::Symbol { .. });
    let mut lookups = Vec::new();
    for path in files {
        if !languages.serves(&path) {
            if symbol {
                languages.start_for(&path);
                return Err(languages.lookup_error(&path, t!("workbench-language-not-running")));
            }
            continue;
        }
        if let Some(client) = languages.client_for(&path)
            && !lists_symbols(client)
        {
            if symbol {
                return Err(CommandError::new(t!("workbench-safe-delete-unsupported")));
            }
            continue;
        }
        let method = "textDocument/documentSymbol";
        let request =
            request_in(languages, workbench, &path, method, |uri, _| json!({ "textDocument": { "uri": uri } }))
                .map_err(|message| languages.lookup_error(&path, message))?;
        lookups.push(lookup(request, method, path, 0));
    }
    let stage = if lookups.is_empty() {
        Stage::Done(Ok(Found { declarations: Vec::new(), usages: Vec::new() }))
    } else {
        Stage::Symbols(lookups)
    };
    Ok(Search { target: target.clone(), stage })
}

fn files_under(path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() {
        return vec![path.to_owned()];
    }
    let mut files = Vec::new();
    let mut folders = vec![path.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let entry = entry.path();
            if entry.is_dir() {
                folders.push(entry);
            } else {
                files.push(entry);
            }
        }
    }
    files.sort();
    files
}

fn lists_symbols(client: &zen_lsp::Client) -> bool {
    client
        .capabilities()
        .and_then(|capabilities| capabilities.document_symbol_provider.as_ref())
        .is_some_and(|provider| !matches!(provider, OneOf::Left(false)))
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

fn poll_all<'a>(lookups: impl Iterator<Item = &'a mut Lookup>, deadline: Instant) -> bool {
    let mut done = true;
    for lookup in lookups {
        done &= lookup.poll(deadline.saturating_duration_since(Instant::now()));
    }
    done
}

fn advance(search: &mut Search, languages: &mut Languages, workbench: &Workbench, deadline: Instant) {
    if let Stage::Symbols(lookups) = &mut search.stage {
        if !poll_all(lookups.iter_mut(), deadline) {
            return;
        }
        let lookups = std::mem::take(lookups);
        search.stage = match declarations(&search.target, &lookups, languages) {
            Ok(declarations) => ask_usages(declarations, languages, workbench),
            Err(error) => Stage::Done(Err(error)),
        };
    }
    if let Stage::Usages { declarations, lookups } = &mut search.stage {
        if !poll_all(lookups.iter_mut().map(|(_, lookup)| lookup), deadline) {
            return;
        }
        let declarations = std::mem::take(declarations);
        let lookups = std::mem::take(lookups);
        search.stage = Stage::Done(usages(&search.target, declarations, &lookups, languages));
    }
}

fn declarations(target: &Target, lookups: &[Lookup], languages: &Languages) -> Result<Vec<Declaration>, String> {
    let mut found = Vec::new();
    for lookup in lookups {
        let answer = match &lookup.answer {
            Some(Ok(answer)) => answer,
            Some(Err(error)) => return Err(error.clone()),
            None => continue,
        };
        let Some(text) = languages.synced_text(&lookup.path) else { continue };
        let symbols = structure::parse(text, answer);
        let made = |symbol: &Symbol| declaration(text, &lookup.path, lookup.version, symbol);
        match target {
            Target::Symbol { offset, .. } => {
                let Some(symbol) = named_at(&symbols, text, *offset) else {
                    return Err(t!("workbench-safe-delete-not-here"));
                };
                found.push(made(symbol));
            }
            Target::Files(_) => found.extend(file_declarations(&symbols).into_iter().map(made)),
        }
    }
    Ok(found)
}

fn file_declarations(symbols: &[Symbol]) -> Vec<&Symbol> {
    let mut found = Vec::new();
    for symbol in symbols {
        if matches!(symbol.kind, SymbolKind::Package | SymbolKind::Module) {
            continue;
        }
        found.push(symbol);
        found.extend(symbol.children.iter().filter(|child| child.start >= symbol.end || child.end <= symbol.start));
    }
    found
}

fn named_at<'a>(symbols: &'a [Symbol], text: &str, offset: usize) -> Option<&'a Symbol> {
    for symbol in symbols {
        if let Some(inner) = named_at(&symbol.children, text, offset) {
            return Some(inner);
        }
        let end = symbol.selection_start + name_length(text, symbol.selection_start);
        if symbol.selection_start <= offset && offset <= end {
            return Some(symbol);
        }
    }
    None
}

fn name_length(text: &str, start: usize) -> usize {
    text[byte_offset(text, start)..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').count()
}

fn declaration(text: &str, path: &Path, version: i32, symbol: &Symbol) -> Declaration {
    let start = symbol.selection_start;
    let name: String = text[byte_offset(text, start)..].chars().take(name_length(text, start)).collect();
    let (from, to) = deletion(text, byte_offset(text, symbol.start), byte_offset(text, symbol.end));
    Declaration {
        path: path.to_owned(),
        name: if name.is_empty() { symbol.name.clone() } else { name },
        version,
        selection_start: start,
        delete: (super::char_offset(text, from), super::char_offset(text, to)),
    }
}

pub(crate) fn deletion(text: &str, start: usize, end: usize) -> (usize, usize) {
    let line_start = |at: usize| text[..at].rfind('\n').map_or(0, |index| index + 1);
    let line_end = |at: usize| text[at..].find('\n').map_or(text.len(), |index| at + index);
    let head = line_start(start);
    let tail = line_end(end);
    let after = text[end..tail].trim();
    if !text[head..start].trim().is_empty() || !(after.is_empty() || after == ";") {
        return (start, end);
    }
    let mut from = head;
    while from > 0 {
        let above = line_start(from - 1);
        let line = text[above..from - 1].trim();
        if line.starts_with('@') || line.starts_with("#[") || line.starts_with("//") {
            from = above;
            continue;
        }
        if line.ends_with("*/")
            && let Some(open) = text[..from].rfind("/*")
            && text[line_start(open)..open].trim().is_empty()
        {
            from = line_start(open);
            continue;
        }
        break;
    }
    let mut to = (tail + 1).min(text.len());
    let blank_above = from == 0 || text[line_start(from - 1)..from - 1].trim().is_empty();
    if blank_above && to < text.len() {
        let next = line_end(to);
        if text[to..next].trim().is_empty() {
            to = (next + 1).min(text.len());
        }
    } else if blank_above && from > 0 {
        from = line_start(from - 1);
    }
    (from, to)
}

fn ask_usages(declarations: Vec<Declaration>, languages: &mut Languages, workbench: &Workbench) -> Stage {
    let method = "textDocument/references";
    let mut lookups = Vec::new();
    for (index, declaration) in declarations.iter().enumerate() {
        let extra = json!({ "context": { "includeDeclaration": false } });
        match request_at(languages, workbench, &declaration.path, declaration.selection_start, method, extra) {
            Ok(request) => {
                lookups.push((index, lookup(request, method, declaration.path.clone(), declaration.selection_start)));
            }
            Err(error) => return Stage::Done(Err(error)),
        }
    }
    if lookups.is_empty() {
        return Stage::Done(Ok(Found { declarations, usages: Vec::new() }));
    }
    Stage::Usages { declarations, lookups }
}

fn usages(
    target: &Target,
    declarations: Vec<Declaration>,
    lookups: &[(usize, Lookup)],
    languages: &Languages,
) -> Result<Found, String> {
    let mut cache = TextCache::new();
    let mut found = Vec::new();
    for (_, lookup) in lookups {
        match &lookup.answer {
            Some(Ok(answer)) => found.extend(places(answer, languages, &mut cache)),
            Some(Err(error)) => return Err(error.clone()),
            None => {}
        }
    }
    let deleted = |place: &Place| match target {
        Target::Files(paths) => paths.iter().any(|path| place.path.starts_with(path)),
        Target::Symbol { .. } => declarations.iter().any(|declaration| {
            declaration.path == place.path && declaration.delete.0 <= place.start && place.end <= declaration.delete.1
        }),
    };
    found.retain(|place| !deleted(place));
    found.sort_by(|a, b| (&a.path, a.start).cmp(&(&b.path, b.start)));
    found.dedup_by(|a, b| a.path == b.path && a.start == b.start);
    Ok(Found { declarations, usages: found })
}

fn report(search: &Search, workbench: &Workbench) -> SafeDeleteSearch {
    let project = workbench.project();
    let name = match &search.target {
        Target::Files(paths) => paths
            .iter()
            .map(|path| {
                path.file_name().map_or_else(|| relative(project, path), |name| name.to_string_lossy().into_owned())
            })
            .collect::<Vec<_>>()
            .join(", "),
        Target::Symbol { .. } => match &search.stage {
            Stage::Done(Ok(found)) => {
                found.declarations.first().map(|declaration| declaration.name.clone()).unwrap_or_default()
            }
            Stage::Usages { declarations, .. } => {
                declarations.first().map(|declaration| declaration.name.clone()).unwrap_or_default()
            }
            _ => String::new(),
        },
    };
    let found = match &search.stage {
        Stage::Done(Ok(found)) => found,
        Stage::Done(Err(error)) => {
            return SafeDeleteSearch { name, error: Some(error.clone()), ..SafeDeleteSearch::default() };
        }
        _ => return SafeDeleteSearch { pending: true, name, ..SafeDeleteSearch::default() },
    };
    let (deletes, operations) = match &search.target {
        Target::Files(paths) => {
            let operations = paths
                .iter()
                .map(|path| RenameOperation {
                    kind: RenameOperationKind::Delete,
                    path: path.clone(),
                    relative: relative(project, path),
                    new_path: None,
                    new_relative: None,
                })
                .collect();
            (Vec::new(), operations)
        }
        Target::Symbol { .. } => (
            found.declarations.iter().filter_map(|declaration| deleted_text(workbench, declaration)).collect(),
            Vec::new(),
        ),
    };
    SafeDeleteSearch {
        pending: false,
        name,
        total: found.usages.len(),
        usages: group(found.usages.clone(), project),
        deletes,
        operations,
        error: None,
    }
}

fn deleted_text(workbench: &Workbench, declaration: &Declaration) -> Option<RenameFile> {
    let text = super::file_text(workbench, &declaration.path).ok()?;
    let (from, to) = (byte_offset(&text, declaration.delete.0), byte_offset(&text, declaration.delete.1));
    let start = zen_lsp::position::position(&text, from);
    let end = zen_lsp::position::position(&text, to);
    let range = json!({ "start": start, "end": end });
    let found = place(declaration.path.clone(), &text, &range)?;
    Some(RenameFile {
        path: declaration.path.clone(),
        relative: relative(workbench.project(), &declaration.path),
        changes: vec![RenameChange { place: found, new_text: String::new() }],
    })
}

#[cfg(test)]
pub(crate) mod tests;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::SymbolKind;

use super::places::{Place, TextCache, places};
use super::structure::{self, Symbol};
use super::usages::word_at;
use super::{LookupKind, LookupParams, WaitParams, file_text, with_languages};
use crate::Workbench;
use crate::history::{NavigateTo, NavigateToParams};

/// What a lookup looks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    /// Where the symbol is declared.
    #[default]
    Declaration,
    /// The implementations of the symbol.
    Implementation,
    /// The usages of the symbol, looked up on its declaration.
    Usages,
    /// Where the type of the symbol is declared.
    TypeDeclaration,
    /// The interface methods the method around the place implements.
    SuperMethod,
    /// The tests of the code at the place.
    Test,
    /// The code the test at the place tests.
    TestSubject,
}

impl TargetKind {
    fn method(self) -> &'static str {
        match self {
            Self::Declaration => "textDocument/definition",
            Self::Implementation => "textDocument/implementation",
            Self::Usages => "textDocument/references",
            Self::TypeDeclaration => "textDocument/typeDefinition",
            Self::SuperMethod => "textDocument/documentSymbol",
            Self::Test | Self::TestSubject => "",
        }
    }

    fn none(self) -> String {
        match self {
            Self::Declaration => t!("workbench-nav-no-declaration"),
            Self::Implementation => t!("workbench-nav-no-implementation"),
            Self::Usages => t!("workbench-nav-no-usages"),
            Self::TypeDeclaration => t!("workbench-nav-no-type-declaration"),
            Self::SuperMethod => t!("workbench-nav-no-super-method"),
            Self::Test => t!("workbench-nav-no-test"),
            Self::TestSubject => t!("workbench-nav-no-test-subject"),
        }
    }
}

/// What a go-to lookup has found so far.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LookupState {
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// The places found, in the server's order.
    pub targets: Vec<Place>,
    /// Whether the editor went to the only target.
    pub navigated: bool,
    /// Why nothing was found, in words for the user.
    pub error: Option<String>,
    /// What the lookup looks for: `declaration`, `implementation`, `typeDeclaration`, `superMethod`, or `usages` when it
    /// started on a declaration.
    pub kind: TargetKind,
    /// The name of the symbol the lookup is about, when there is a word at its place.
    pub symbol: Option<String>,
    /// Whether Go to Test found no test and offers Create New Test… (`navigate.test.options`, `navigate.test.create`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub create_test: bool,
}

#[derive(Debug)]
struct Origin {
    kind: TargetKind,
    path: PathBuf,
    offset: usize,
    caret: Option<(PathBuf, usize)>,
    symbol: Option<String>,
    method: Option<usize>,
    declaration_asked: bool,
    contents: Option<Contents>,
}

#[derive(Debug)]
struct Contents {
    answer: serde_json::Value,
    extension: String,
    asked: Vec<(String, zen_lsp::Pending)>,
    answered: Vec<(String, serde_json::Value)>,
}

impl Contents {
    fn poll(&mut self, wait: std::time::Duration) -> bool {
        let deadline = Instant::now() + wait;
        let mut left = Vec::new();
        for (uri, pending) in std::mem::take(&mut self.asked) {
            let wait = deadline.saturating_duration_since(Instant::now());
            let answer = if wait.is_zero() {
                pending.try_answer()
            } else {
                match pending.wait(wait) {
                    Err(zen_lsp::LspError::Timeout) => None,
                    other => Some(other),
                }
            };
            match answer {
                Some(Ok(text)) => self.answered.push((uri, text)),
                Some(Err(_)) => {}
                None => left.push((uri, pending)),
            }
        }
        self.asked = left;
        self.asked.is_empty()
    }

    fn store(&self) {
        let folder = crate::virtual_documents::folder();
        for (uri, text) in &self.answered {
            if let Some(text) = text.as_str() {
                let _ = crate::virtual_documents::from_server(&folder, uri, text, &self.extension);
            }
        }
    }
}

fn target_uris(answer: &serde_json::Value) -> Vec<&str> {
    let items: Vec<&serde_json::Value> = match answer {
        serde_json::Value::Array(list) => list.iter().collect(),
        serde_json::Value::Object(_) => vec![answer],
        _ => Vec::new(),
    };
    items.into_iter().filter_map(|item| item["targetUri"].as_str().or_else(|| item["uri"].as_str())).collect()
}

fn ask_contents(session: &mut Session, path: &Path, answer: &serde_json::Value) -> Option<Contents> {
    let mut wanted: Vec<String> = target_uris(answer)
        .into_iter()
        .filter(|uri| zen_lsp::position::uri_path(uri).is_none())
        .map(str::to_owned)
        .collect();
    wanted.dedup();
    if wanted.is_empty() {
        return None;
    }
    with_languages(session, |languages, _| {
        let declared = languages.virtual_documents(path)?;
        let asked: Vec<(String, zen_lsp::Pending)> = wanted
            .into_iter()
            .filter(|uri| declared.serves(uri))
            .filter_map(|uri| languages.request_contents(path, &uri).ok().map(|pending| (uri, pending)))
            .collect();
        (!asked.is_empty()).then(|| Contents {
            answer: answer.clone(),
            extension: declared.extension,
            asked,
            answered: Vec::new(),
        })
    })
}

#[derive(Debug, Default)]
struct Navigation {
    origin: Option<Origin>,
    outcome: Option<LookupState>,
}

fn active_caret(workbench: &Workbench) -> Option<(PathBuf, usize)> {
    workbench.active_document().map(|document| (document.path().to_owned(), document.selections().primary().head))
}

pub(crate) fn offer(session: &mut Session, state: LookupState) {
    *session.state_mut::<Navigation>() = Navigation { origin: None, outcome: Some(state) };
}

pub(crate) fn none_found(kind: TargetKind) -> String {
    kind.none()
}

pub fn lookup_in_progress(session: &Session) -> bool {
    session.state::<Navigation>().is_some_and(|navigation| navigation.origin.is_some() || navigation.outcome.is_some())
}

fn assisted_go_to(session: &mut Session, path: &Path, offset: usize) -> Option<Result<(), String>> {
    let assists = session.state::<zen_modules::assist::LanguageAssists>()?.clone();
    let (text, _) = crate::run::file_text(session, path).ok()?;
    let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
    let target = assists.declaration(&zen_modules::assist::AssistContext { path, text: &text, offset: byte })?;
    Some(target.and_then(|path| {
        let params = NavigateToParams { path, offset: 0, end: None };
        NavigateTo.run(session, params).map(|_| ()).map_err(|error| error.to_string())
    }))
}

fn told(session: &Session, mut state: LookupState) -> LookupState {
    for target in &mut state.targets {
        target.path = crate::told_path(session, &target.path);
    }
    state
}

fn start(session: &mut Session, kind: TargetKind, params: &LookupParams) -> Result<LookupState, CommandError> {
    let deadline = Instant::now() + params.wait();
    session.state_mut::<Navigation>().origin = None;
    let workbench = session.state_mut::<Workbench>();
    let (path, offset) = params.place(workbench)?;
    let caret = active_caret(workbench);
    let symbol = file_text(workbench, &path).ok().and_then(|text| word_at(&text, offset));
    #[cfg(not(target_arch = "wasm32"))]
    if kind == TargetKind::Declaration
        && let Some(opened) = crate::database::assist::go_to(session, &path, offset)
    {
        let state = match opened {
            Ok(()) => LookupState { navigated: true, kind, symbol, ..LookupState::default() },
            Err(error) => LookupState { error: Some(error), kind, symbol, ..LookupState::default() },
        };
        session.state_mut::<Navigation>().outcome = Some(state.clone());
        return Ok(state);
    }
    if kind == TargetKind::Declaration
        && let Some(opened) = crate::project_index::go_to(session, &path, offset)
    {
        let state = match opened {
            Ok(()) => LookupState { navigated: true, kind, symbol, ..LookupState::default() },
            Err(error) => LookupState { error: Some(error), kind, symbol, ..LookupState::default() },
        };
        session.state_mut::<Navigation>().outcome = Some(state.clone());
        return Ok(state);
    }
    if kind == TargetKind::Declaration
        && let Some(opened) = crate::resource_bundles::go_to(session, &path, offset)
    {
        let state = match opened {
            Ok(()) => LookupState { navigated: true, kind, symbol, ..LookupState::default() },
            Err(error) => LookupState { error: Some(error), kind, symbol, ..LookupState::default() },
        };
        session.state_mut::<Navigation>().outcome = Some(state.clone());
        return Ok(state);
    }
    if kind == TargetKind::Declaration
        && let Some(opened) = crate::ant::go_to(session, &path, offset)
    {
        let state = match opened {
            Ok(()) => LookupState { navigated: true, kind, symbol, ..LookupState::default() },
            Err(error) => LookupState { error: Some(error), kind, symbol, ..LookupState::default() },
        };
        session.state_mut::<Navigation>().outcome = Some(state.clone());
        return Ok(state);
    }
    if kind == TargetKind::Declaration
        && let Some(opened) = assisted_go_to(session, &path, offset)
    {
        let state = match opened {
            Ok(()) => LookupState { navigated: true, kind, symbol, ..LookupState::default() },
            Err(error) => LookupState { error: Some(error), kind, symbol, ..LookupState::default() },
        };
        session.state_mut::<Navigation>().outcome = Some(state.clone());
        return Ok(state);
    }
    if kind == TargetKind::Declaration
        && let Some(files) = referenced_files(session, &path, offset)
    {
        let origin =
            Origin { kind, path, offset, caret, symbol, method: None, declaration_asked: false, contents: None };
        return Ok(found_targets(session, origin, Ok(file_locations(&files)), deadline, true));
    }
    let started = with_languages(session, |languages, workbench| {
        let started = languages
            .start_lookup(workbench, LookupKind::Navigation, &path, offset, kind.method(), json!({}))
            .map(|_| ());
        started.map_err(|message| languages.lookup_error(&path, message))
    });
    if let Err(error) = started {
        session.state_mut::<Navigation>().outcome =
            Some(LookupState { error: Some(error.message.clone()), kind, symbol, ..LookupState::default() });
        return Err(error);
    }
    session.state_mut::<Navigation>().origin =
        Some(Origin { kind, path, offset, caret, symbol, method: None, declaration_asked: false, contents: None });
    Ok(resolve(session, deadline, true))
}

fn referenced_files(session: &Session, path: &Path, offset: usize) -> Option<Vec<PathBuf>> {
    let references = session.state::<zen_modules::file_references::FileReferenceSet>()?;
    let text = file_text(session.state::<Workbench>()?, path).ok()?;
    references.files_at(path, &text, offset)
}

fn file_locations(files: &[PathBuf]) -> serde_json::Value {
    let start = json!({ "line": 0, "character": 0 });
    let locations: Vec<serde_json::Value> = files
        .iter()
        .map(|file| json!({ "uri": zen_lsp::position::file_uri(file), "range": { "start": start, "end": start } }))
        .collect();
    json!(locations)
}

fn resolve(session: &mut Session, deadline: Instant, keep: bool) -> LookupState {
    let navigation = session.state_mut::<Navigation>();
    let Some(mut origin) = navigation.origin.take() else {
        return navigation.outcome.take().unwrap_or_default();
    };
    let wait = deadline.saturating_duration_since(Instant::now());
    if let Some(mut contents) = origin.contents.take() {
        if !contents.poll(wait) {
            let state = LookupState {
                pending: true,
                kind: origin.kind,
                symbol: origin.symbol.clone(),
                ..LookupState::default()
            };
            origin.contents = Some(contents);
            session.state_mut::<Navigation>().origin = Some(origin);
            return state;
        }
        contents.store();
        return found_targets(session, origin, Ok(contents.answer), deadline, keep);
    }
    let answered = with_languages(session, |languages, _| {
        languages.lookup_mut(LookupKind::Navigation).map(|lookup| lookup.poll(wait))
    });
    match answered {
        None => return LookupState::default(),
        Some(false) => {
            let state = LookupState {
                pending: true,
                kind: origin.kind,
                symbol: origin.symbol.clone(),
                ..LookupState::default()
            };
            session.state_mut::<Navigation>().origin = Some(origin);
            return state;
        }
        Some(true) => {}
    }
    let answer = with_languages(session, |languages, workbench| {
        let stale = languages.is_outdated(LookupKind::Navigation) || active_caret(workbench) != origin.caret;
        let answer = languages.end_lookup(LookupKind::Navigation).and_then(|lookup| lookup.answer);
        (!stale).then(|| answer.unwrap_or(Ok(serde_json::Value::Null)))
    });
    let Some(answer) = answer else { return LookupState::default() };
    if origin.kind == TargetKind::SuperMethod && origin.method.is_none() {
        return ask_super_methods(session, origin, answer, deadline, keep);
    }
    if let Ok(value) = &answer
        && let Some(contents) = ask_contents(session, &origin.path, value)
    {
        origin.contents = Some(contents);
        session.state_mut::<Navigation>().origin = Some(origin);
        return resolve(session, deadline, keep);
    }
    found_targets(session, origin, answer, deadline, keep)
}

fn found_targets(
    session: &mut Session,
    origin: Origin,
    answer: Result<serde_json::Value, String>,
    deadline: Instant,
    keep: bool,
) -> LookupState {
    let found = answer.map(|answer| {
        with_languages(session, |languages, _| {
            let mut seen = HashSet::new();
            let mut found = places(&answer, languages, &mut TextCache::new());
            found.retain(|place| seen.insert((place.path.clone(), place.start)));
            if let Some(method) = origin.method {
                found.retain(|place| place.start != method || !same_file(&place.path, &origin.path));
            }
            found
        })
    });
    if let (TargetKind::SuperMethod, Some(method), false) = (origin.kind, origin.method, origin.declaration_asked)
        && found.as_ref().is_ok_and(Vec::is_empty)
    {
        let started = with_languages(session, |languages, workbench| {
            languages
                .start_lookup(
                    workbench,
                    LookupKind::Navigation,
                    &origin.path,
                    method,
                    "textDocument/declaration",
                    json!({}),
                )
                .map(|_| ())
        });
        if started.is_ok() {
            session.state_mut::<Navigation>().origin = Some(Origin { declaration_asked: true, ..origin });
            return resolve(session, deadline, keep);
        }
    }
    let mut state = LookupState { kind: origin.kind, symbol: origin.symbol.clone(), ..LookupState::default() };
    match found {
        Err(_) if matches!(origin.kind, TargetKind::TypeDeclaration | TargetKind::SuperMethod) => {
            state.error = Some(origin.kind.none());
        }
        Err(error) => state.error = Some(error),
        Ok(targets) if targets.is_empty() => state.error = Some(origin.kind.none()),
        Ok(targets)
            if origin.kind == TargetKind::Declaration
                && targets.len() == 1
                && targets[0].path == origin.path
                && (targets[0].start..=targets[0].end).contains(&origin.offset) =>
        {
            let started = with_languages(session, |languages, workbench| {
                languages
                    .start_lookup(
                        workbench,
                        LookupKind::Navigation,
                        &origin.path,
                        origin.offset,
                        TargetKind::Usages.method(),
                        json!({ "context": { "includeDeclaration": false } }),
                    )
                    .map(|_| ())
            });
            match started {
                Ok(()) => {
                    session.state_mut::<Navigation>().origin = Some(Origin { kind: TargetKind::Usages, ..origin });
                    return resolve(session, deadline, keep);
                }
                Err(error) => {
                    state.kind = TargetKind::Usages;
                    state.error = Some(error);
                }
            }
        }
        Ok(targets) if targets.len() == 1 => {
            let target = &targets[0];
            let params = NavigateToParams { path: target.path.clone(), offset: target.start, end: None };
            let left =
                (state.kind == TargetKind::Declaration).then(|| crate::tabs::tab_to_replace(session, &target.path));
            match NavigateTo.run(session, params) {
                Ok(_) => {
                    state.navigated = true;
                    if let Some(Some(left)) = left {
                        crate::tabs::replace_left_tab(session, &left, &target.path);
                    }
                }
                Err(error) => state.error = Some(error.message),
            }
            state.targets = targets;
        }
        Ok(targets) => state.targets = targets,
    }
    if keep {
        session.state_mut::<Navigation>().outcome = Some(state.clone());
    }
    state
}

fn ask_super_methods(
    session: &mut Session,
    mut origin: Origin,
    symbols: Result<serde_json::Value, String>,
    deadline: Instant,
    keep: bool,
) -> LookupState {
    let text = file_text(session.state_mut::<Workbench>(), &origin.path);
    let method = match (symbols, text) {
        (Ok(answer), Ok(text)) => concrete_method_around(&structure::parse(&text, &answer), origin.offset)
            .map(|method| (method, word_at(&text, method))),
        _ => None,
    };
    let started = method.ok_or_else(|| origin.kind.none()).and_then(|method| {
        with_languages(session, |languages, workbench| {
            languages
                .start_lookup(
                    workbench,
                    LookupKind::Navigation,
                    &origin.path,
                    method.0,
                    TargetKind::Implementation.method(),
                    json!({}),
                )
                .map(|_| method)
        })
    });
    match started {
        Ok((method, name)) => {
            origin.method = Some(method);
            origin.symbol = name;
            session.state_mut::<Navigation>().origin = Some(origin);
            resolve(session, deadline, keep)
        }
        Err(error) => {
            let state =
                LookupState { error: Some(error), kind: origin.kind, symbol: origin.symbol, ..LookupState::default() };
            if keep {
                session.state_mut::<Navigation>().outcome = Some(state.clone());
            }
            state
        }
    }
}

fn same_file(one: &Path, other: &Path) -> bool {
    one == other
        || std::fs::canonicalize(one).is_ok_and(|one| std::fs::canonicalize(other).is_ok_and(|other| one == other))
}

fn concrete_method_around(symbols: &[Symbol], offset: usize) -> Option<usize> {
    let mut chain = Vec::new();
    let mut level = symbols;
    for index in structure::current(symbols, offset) {
        let symbol = level.get(index)?;
        chain.push(symbol);
        level = &symbol.children;
    }
    let at = chain.iter().rposition(|symbol| matches!(symbol.kind, SymbolKind::Method | SymbolKind::Function))?;
    let abstract_method = at > 0 && chain[at - 1].kind == SymbolKind::Interface;
    (!abstract_method).then_some(chain[at].selection_start)
}

pub struct GoToDeclaration;

impl Command for GoToDeclaration {
    type Params = LookupParams;
    type Output = LookupState;
    const ID: &'static str = "navigate.declaration.goTo";
    const TITLE: &'static str = "Go to Declaration or Usages";
    const DESCRIPTION: &'static str = "Asks the language server where the symbol at a place (the caret by default) is declared \
         and goes there when there is one target; on a declaration it finds its usages. navigate.lookup.poll reads the answer.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<LookupState, CommandError> {
        start(session, TargetKind::Declaration, &params).map(|state| told(session, state))
    }
}

pub struct GoToImplementation;

impl Command for GoToImplementation {
    type Params = LookupParams;
    type Output = LookupState;
    const ID: &'static str = "navigate.implementation.goTo";
    const TITLE: &'static str = "Go to Implementation(s)";
    const DESCRIPTION: &'static str = "Asks the language server for the implementations of the symbol at a place (the caret by \
         default) and goes there when there is one; navigate.lookup.poll reads the answer.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<LookupState, CommandError> {
        start(session, TargetKind::Implementation, &params).map(|state| told(session, state))
    }
}

pub struct GoToTypeDeclaration;

impl Command for GoToTypeDeclaration {
    type Params = LookupParams;
    type Output = LookupState;
    const ID: &'static str = "navigate.typeDeclaration.goTo";
    const TITLE: &'static str = "Go to Type Declaration";
    const DESCRIPTION: &'static str = "Asks the language server where the type of the symbol at a place (the caret by \
         default) is declared (textDocument/typeDefinition) and goes there when there is one; navigate.lookup.poll reads the \
         answer.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<LookupState, CommandError> {
        start(session, TargetKind::TypeDeclaration, &params).map(|state| told(session, state))
    }
}

pub struct GoToSuperMethod;

impl Command for GoToSuperMethod {
    type Params = LookupParams;
    type Output = LookupState;
    const ID: &'static str = "navigate.superMethod.goTo";
    const TITLE: &'static str = "Go to Super Method";
    const DESCRIPTION: &'static str = "Finds the method around a place (the caret by default) in the file's symbols \
         (textDocument/documentSymbol) and, when it is a method of a concrete type, asks the language server for the \
         interface methods it implements (textDocument/implementation, or textDocument/declaration when that finds none, \
         which leads from a trait implementation's method to the trait's) and goes there when there is one; a method of an \
         interface or a function has none. navigate.lookup.poll reads the answer.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<LookupState, CommandError> {
        start(session, TargetKind::SuperMethod, &params).map(|state| told(session, state))
    }
}

pub struct PollLookup;

impl Command for PollLookup {
    type Params = WaitParams;
    type Output = LookupState;
    const ID: &'static str = "navigate.lookup.poll";
    const TITLE: &'static str = "Navigation Lookup State";
    const DESCRIPTION: &'static str = "The state of the go-to lookup in progress (declaration, implementation, type \
         declaration, super method), waiting up to waitMs for the server's answer. Once the answer is in it goes to a single \
         target, or lists the targets to choose from, or says why there is none; that final state is returned once, then \
         the state is empty. An answer that comes after the file changed or the caret moved is dropped.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<LookupState, CommandError> {
        let state = resolve(session, Instant::now() + params.wait(), false);
        Ok(told(session, state))
    }
}

pub struct CancelLookup;

impl Command for CancelLookup {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "navigate.lookup.cancel";
    const TITLE: &'static str = "Cancel Navigation Lookup";
    const DESCRIPTION: &'static str = "Cancels the go-to lookup in progress.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        *session.state_mut::<Navigation>() = Navigation::default();
        with_languages(session, |languages, _| languages.end_lookup(LookupKind::Navigation));
        Ok(())
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GoToDeclaration)?;
    registry.register(GoToImplementation)?;
    registry.register(GoToTypeDeclaration)?;
    registry.register(GoToSuperMethod)?;
    registry.register(PollLookup)?;
    registry.register(CancelLookup)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use zen_lsp::fake::Reply;

    use super::*;
    use crate::history::History;
    use crate::language::testing::{Fixture, fixture};

    const MAIN: &str = "package main\n\n// привет 😀\nfunc main() { ё := \"😀\"; Println(ё); Println(ё) }\n";
    const LIB: &str = "package main\n\n/* 😀 ё */ func Println(a ...any) {}\n";

    fn chars(text: &str, needle: &str) -> usize {
        text[..text.find(needle).unwrap()].chars().count()
    }

    fn utf16(text: &str, needle: &str) -> Value {
        let before = &text[..text.find(needle).unwrap()];
        let line = before.matches('\n').count();
        let column = before[before.rfind('\n').map_or(0, |at| at + 1)..].encode_utf16().count();
        json!({ "line": line, "character": column })
    }

    fn location(fixture: &Fixture, name: &str, text: &str, needle: &str) -> Value {
        let start = utf16(text, needle);
        let end = json!({ "line": start["line"], "character": start["character"].as_u64().unwrap() + needle.encode_utf16().count() as u64 });
        json!({ "uri": zen_lsp::position::file_uri(&fixture.path(name)), "range": { "start": start, "end": end } })
    }

    fn state(value: Value) -> (bool, usize, bool, Option<String>, String) {
        (
            value["pending"].as_bool().unwrap(),
            value["targets"].as_array().unwrap().len(),
            value["navigated"].as_bool().unwrap(),
            value["error"].as_str().map(str::to_owned),
            value["kind"].as_str().unwrap().to_owned(),
        )
    }

    fn caret(fixture: &Fixture) -> (PathBuf, usize) {
        let workbench = fixture.session.state::<Workbench>().unwrap();
        active_caret(workbench).unwrap()
    }

    #[test]
    fn one_declaration_is_gone_to_and_recorded_in_the_history() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let target = location(&fixture, "lib.go", LIB, "Println");
        fixture.server.on("textDocument/definition", target);
        fixture.open("main.go");
        let main = fixture.path("main.go");
        let at = chars(MAIN, "Println") + 2;
        let went = fixture
            .dispatch("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 5000 }))
            .unwrap();
        assert_eq!(state(went.clone()), (false, 1, true, None, "declaration".to_owned()));
        assert_eq!(went["symbol"], "Println");
        let sent = &fixture.server.requests("textDocument/definition")[0];
        assert_eq!(sent["position"], utf16(MAIN, "intln"));
        assert_eq!(caret(&fixture), (fixture.path("lib.go"), chars(LIB, "Println")));
        let back = fixture.session.state::<History>().unwrap().back().to_vec();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].path, main);
        assert!(lookup_in_progress(&fixture.session), "the outcome waits for the window");
        let read = fixture.dispatch("navigate.lookup.poll", json!({})).unwrap();
        assert!(read["navigated"].as_bool().unwrap(), "the window reads the outcome once");
        assert!(!lookup_in_progress(&fixture.session));
        let read = fixture.dispatch("navigate.lookup.poll", json!({})).unwrap();
        assert_eq!(state(read), (false, 0, false, None, "declaration".to_owned()));
    }

    #[test]
    fn a_declaration_in_a_library_class_opens_the_servers_contents_read_only_under_its_uri() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        let uri = "lib://contents/lib.jar/lib/Printer.class?=demo";
        let contents = "package lib\n\nfunc Println() {}\n";
        let start = json!({ "line": 2, "character": 5 });
        fixture
            .server
            .on("textDocument/definition", json!([{ "uri": uri, "range": { "start": start, "end": start } }]));
        fixture.server.on("lib/contents", json!(contents));
        fixture.open("main.go");
        let declared = zen_lsp::VirtualDocuments {
            schemes: vec!["lib".to_owned()],
            request: "lib/contents".to_owned(),
            extension: "go".to_owned(),
        };
        with_languages(&mut fixture.session, |languages, _| languages.declare_virtual_documents_for_test(&declared));
        let at = chars(MAIN, "Println") + 2;
        let went = fixture
            .dispatch(
                "navigate.declaration.goTo",
                json!({ "path": fixture.path("main.go"), "offset": at, "waitMs": 5000 }),
            )
            .unwrap();
        assert_eq!(state(went.clone()), (false, 1, true, None, "declaration".to_owned()), "{went}");
        let asked = fixture.server.requests("lib/contents");
        assert_eq!(asked[0]["uri"], uri);
        let (path, offset) = caret(&fixture);
        assert!(path.starts_with(crate::virtual_documents::folder()), "{}", path.display());
        assert!(path.ends_with("Printer.go"), "{}", path.display());
        assert_eq!(offset, chars(contents, "Println"));
        assert_eq!(zen_lsp::position::file_uri(&path), uri, "the server hears of it by its own URI");
        let workbench = fixture.session.state::<Workbench>().unwrap();
        let document = workbench.active_document().unwrap();
        assert_eq!(document.text(), contents);
        assert!(document.read_only().is_some_and(|reason| reason.contains("Printer.go")), "read-only");
        assert!(!document.is_locked(), "it cannot be made writable");
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let opened = fixture.server.wait_for("textDocument/didOpen", 2, Duration::from_secs(5));
        assert!(opened.iter().any(|open| open["textDocument"]["uri"] == uri), "{opened:?}");
    }

    #[test]
    fn the_declaration_in_a_closed_file_is_gone_to_without_opening_it() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        fixture.server.on("textDocument/definition", location(&fixture, "lib.go", LIB, "Println"));
        let main = fixture.path("main.go");
        let at = chars(MAIN, "Println") + 2;
        let went = fixture
            .dispatch_once_running("navigate.declaration.goTo", json!({ "path": main, "offset": at, "waitMs": 5000 }))
            .unwrap();
        assert_eq!(state(went.clone()), (false, 1, true, None, "declaration".to_owned()));
        assert_eq!(went["symbol"], "Println");
        assert_eq!(fixture.server.requests("textDocument/definition")[0]["position"], utf16(MAIN, "intln"));
        assert_eq!(fixture.open_files(), [fixture.path("lib.go")], "only the declaration's file is opened");
    }

    struct Embedded(Vec<PathBuf>);

    impl zen_modules::file_references::FileReferences for Embedded {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn files_at(&self, _: &Path, _: &str, offset: usize) -> Option<Vec<PathBuf>> {
            (offset == 0).then(|| self.0.clone())
        }
    }

    #[test]
    fn a_modules_file_references_answer_go_to_declaration_before_the_server() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let files = vec![fixture.path("lib.go"), fixture.path("main.go")];
        let references = zen_modules::file_references::FileReferenceSet::default();
        fixture.session.set_state(references);
        let embedded = std::sync::Arc::new(Embedded(files.clone()));
        fixture.session.state_mut::<zen_modules::file_references::FileReferenceSet>().add(embedded);
        fixture.open("main.go");
        let listed = fixture.dispatch("navigate.declaration.goTo", json!({ "offset": 0, "waitMs": 2000 })).unwrap();
        assert_eq!(listed["targets"][0]["path"], json!(files[0]));
        assert_eq!(listed["targets"][1]["path"], json!(files[1]));
        assert_eq!(listed["navigated"], false);
        fixture.server.on("textDocument/definition", json!([]));
        let elsewhere = fixture.dispatch("navigate.declaration.goTo", json!({ "offset": 3, "waitMs": 2000 })).unwrap();
        assert!(elsewhere["targets"].as_array().unwrap().is_empty(), "other places go to the server");
    }

    #[test]
    fn several_declarations_are_listed_and_the_caret_stays() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let first = location(&fixture, "lib.go", LIB, "Println");
        let second = location(&fixture, "main.go", MAIN, "main()");
        fixture
            .server
            .reply("textDocument/definition", Reply::Delayed(json!([first, second]), Duration::from_millis(50)));
        fixture.open("main.go");
        let main = fixture.path("main.go");
        let at = chars(MAIN, "Println");
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": at }] })).unwrap();
        let started = fixture.dispatch("navigate.declaration.goTo", json!({})).unwrap();
        assert_eq!(state(started), (true, 0, false, None, "declaration".to_owned()));
        assert!(lookup_in_progress(&fixture.session));
        let read = fixture.dispatch("navigate.lookup.poll", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(state(read.clone()), (false, 2, false, None, "declaration".to_owned()));
        assert_eq!(read["targets"][1]["path"], json!(main));
        assert_eq!(read["targets"][1]["preview"], "func main() { ё := \"😀\"; Println(ё); Println(ё) }");
        assert_eq!(caret(&fixture), (main, at));
        assert!(fixture.dispatch("navigate.lookup.poll", json!({})).unwrap()["targets"].as_array().unwrap().is_empty());
    }

    #[test]
    fn no_declaration_is_an_error_for_the_user() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.on("textDocument/definition", json!(null));
        fixture.open("main.go");
        let went = fixture.dispatch("navigate.declaration.goTo", json!({ "offset": 1, "waitMs": 5000 })).unwrap();
        assert_eq!(state(went), (false, 0, false, Some(t!("workbench-nav-no-declaration")), "declaration".to_owned()));
    }

    #[test]
    fn on_the_declaration_itself_its_usages_are_looked_up() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let declaration = location(&fixture, "lib.go", LIB, "Println");
        let first = location(&fixture, "main.go", MAIN, "Println(ё); Println");
        let second = location(&fixture, "main.go", MAIN, "Println(ё) }");
        fixture.server.on("textDocument/definition", declaration);
        fixture.server.on("textDocument/references", json!([first, second, first]));
        fixture.open("lib.go");
        let lib = fixture.path("lib.go");
        let at = chars(LIB, "Println") + 7;
        let went = fixture
            .dispatch("navigate.declaration.goTo", json!({ "path": lib, "offset": at, "waitMs": 5000 }))
            .unwrap();
        assert_eq!(state(went.clone()), (false, 2, false, None, "usages".to_owned()));
        assert_eq!(went["targets"][0]["line"], 4);
        let references = fixture.server.requests("textDocument/references");
        assert_eq!(references[0]["context"], json!({ "includeDeclaration": false }));
        assert_eq!(references[0]["position"], fixture.server.requests("textDocument/definition")[0]["position"]);
    }

    #[test]
    fn implementations_one_is_gone_to_several_listed_none_an_error() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let one = location(&fixture, "lib.go", LIB, "Println");
        let two = location(&fixture, "main.go", MAIN, "main");
        let answers =
            std::sync::Arc::new(std::sync::Mutex::new(vec![json!([]), json!([one.clone(), two]), json!([one])]));
        fixture.server.on_with("textDocument/implementation", move |_| answers.lock().unwrap().pop().unwrap());
        fixture.open("main.go");
        let params = json!({ "offset": chars(MAIN, "Println"), "waitMs": 5000 });
        let went = fixture.dispatch("navigate.implementation.goTo", params.clone()).unwrap();
        assert_eq!(state(went), (false, 1, true, None, "implementation".to_owned()));
        fixture.dispatch("file.document.open", json!({ "path": fixture.path("main.go") })).unwrap();
        let went = fixture.dispatch("navigate.implementation.goTo", params.clone()).unwrap();
        assert_eq!(state(went), (false, 2, false, None, "implementation".to_owned()));
        let went = fixture.dispatch("navigate.implementation.goTo", params).unwrap();
        assert_eq!(
            state(went),
            (false, 0, false, Some(t!("workbench-nav-no-implementation")), "implementation".to_owned())
        );
    }

    #[test]
    fn type_declaration_one_is_gone_to_and_back_returns_several_listed_none_an_error() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let one = location(&fixture, "lib.go", LIB, "Println");
        let two = location(&fixture, "main.go", MAIN, "main");
        let answers =
            std::sync::Arc::new(std::sync::Mutex::new(vec![json!(null), json!([one.clone(), two]), json!([one])]));
        fixture.server.on_with("textDocument/typeDefinition", move |_| answers.lock().unwrap().pop().unwrap());
        fixture.open("main.go");
        let main = fixture.path("main.go");
        let at = chars(MAIN, "ё := ");
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": at }] })).unwrap();
        let params = json!({ "waitMs": 5000 });
        let went = fixture.dispatch("navigate.typeDeclaration.goTo", params.clone()).unwrap();
        assert_eq!(state(went.clone()), (false, 1, true, None, "typeDeclaration".to_owned()));
        assert_eq!(went["symbol"], "ё");
        assert_eq!(fixture.server.requests("textDocument/typeDefinition")[0]["position"], utf16(MAIN, "ё := "));
        assert_eq!(caret(&fixture), (fixture.path("lib.go"), chars(LIB, "Println")));
        fixture.dispatch("navigate.history.back", json!(null)).unwrap();
        assert_eq!(caret(&fixture), (main.clone(), at));
        fixture.dispatch("navigate.history.forward", json!(null)).unwrap();
        assert_eq!(caret(&fixture), (fixture.path("lib.go"), chars(LIB, "Println")));
        fixture.dispatch("file.document.open", json!({ "path": main })).unwrap();
        let went = fixture.dispatch("navigate.typeDeclaration.goTo", params.clone()).unwrap();
        assert_eq!(state(went), (false, 2, false, None, "typeDeclaration".to_owned()));
        let went = fixture.dispatch("navigate.typeDeclaration.goTo", params).unwrap();
        assert_eq!(
            state(went),
            (false, 0, false, Some(t!("workbench-nav-no-type-declaration")), "typeDeclaration".to_owned())
        );
    }

    fn range(text: &str, from: &str, to: &str) -> Value {
        let end = text.find(to).unwrap() + to.len();
        let line = text[..end].matches('\n').count();
        let column = text[text[..end].rfind('\n').map_or(0, |at| at + 1)..end].encode_utf16().count();
        json!({ "start": utf16(text, from), "end": { "line": line, "character": column } })
    }

    fn symbol(text: &str, name: &str, kind: u32, (from, to): (&str, &str), at: &str, children: Value) -> Value {
        let selection = range(text, at, at);
        json!({ "name": name, "kind": kind, "range": range(text, from, to), "selectionRange": selection, "children": children })
    }

    const IMPL: &str = "package main\n\ntype File struct{}\n\nfunc (f *File) Read(p []byte) (int, error) { return 0, nil }\n\nfunc main() {}\n";
    const API: &str = "package main\n\ntype Reader interface {\n\tRead(p []byte) (int, error)\n}\n\ntype Source interface {\n\tRead(q []byte) (int, error)\n}\n";

    fn symbols() -> Value {
        json!([
            symbol(IMPL, "File", 23, ("type File", "struct{}"), "File", json!([])),
            symbol(IMPL, "(*File).Read", 6, ("func (f", "nil }"), "Read", json!([])),
            symbol(IMPL, "main", 12, ("func main", "main() {}"), "main()", json!([])),
        ])
    }

    #[test]
    fn super_method_asks_for_the_interface_methods_of_the_method_around_the_caret() {
        let mut fixture = fixture(&[("file.go", IMPL), ("api.go", API)]);
        let reader = location(&fixture, "api.go", API, "Read");
        let source = location(&fixture, "api.go", API, "Read(q");
        let itself = location(&fixture, "file.go", IMPL, "Read");
        let answers = std::sync::Arc::new(std::sync::Mutex::new(vec![
            json!([itself.clone()]),
            json!([reader.clone(), source, itself]),
            json!([reader]),
        ]));
        fixture.server.on("textDocument/documentSymbol", symbols());
        fixture.server.on_with("textDocument/implementation", move |_| answers.lock().unwrap().pop().unwrap());
        fixture.open("file.go");
        let file = fixture.path("file.go");
        let at = chars(IMPL, "return");
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": at }] })).unwrap();
        let params = json!({ "waitMs": 5000 });
        let went = fixture.dispatch("navigate.superMethod.goTo", params.clone()).unwrap();
        assert_eq!(state(went.clone()), (false, 1, true, None, "superMethod".to_owned()));
        assert_eq!(went["symbol"], "Read");
        assert_eq!(fixture.server.requests("textDocument/implementation")[0]["position"], utf16(IMPL, "Read"));
        assert_eq!(caret(&fixture), (fixture.path("api.go"), chars(API, "Read")));
        let back = fixture.session.state::<History>().unwrap().back().to_vec();
        assert_eq!((back.len(), &back[0].path), (1, &file));
        fixture.dispatch("navigate.history.back", json!(null)).unwrap();
        assert_eq!(caret(&fixture), (file.clone(), at));
        let went = fixture.dispatch("navigate.superMethod.goTo", params.clone()).unwrap();
        assert_eq!(state(went.clone()), (false, 2, false, None, "superMethod".to_owned()));
        assert_eq!(went["targets"][1]["line"], 8);
        assert_eq!(caret(&fixture), (file.clone(), at));
        let went = fixture.dispatch("navigate.superMethod.goTo", params).unwrap();
        assert_eq!(state(went), (false, 0, false, Some(t!("workbench-nav-no-super-method")), "superMethod".to_owned()));
        assert_eq!(
            fixture.server.requests("textDocument/declaration").len(),
            1,
            "the method itself leads to its declaration"
        );
        assert_eq!(caret(&fixture), (file, at));
    }

    #[test]
    fn super_method_falls_back_on_the_declaration_of_a_trait_implementation_method() {
        const RS: &str = "trait Shape {\n    fn area(&self);\n}\n\nstruct Square;\n\nimpl Shape for Square {\n    fn area(&self) {}\n}\n";
        let mut fixture = fixture(&[("trait.go", RS)]);
        let method = "fn area(&self) {}";
        let impl_at = "area(&self) {}";
        let found = json!([
            symbol(
                RS,
                "Shape",
                11,
                ("trait", "area(&self);\n}"),
                "Shape",
                json!([symbol(RS, "area", 6, ("fn area(&self);", "fn area(&self);"), "area", json!([])),])
            ),
            symbol(
                RS,
                "impl Shape for Square",
                19,
                ("impl", "{}\n}"),
                "Shape for",
                json!([symbol(RS, "area", 6, (method, method), impl_at, json!([])),])
            ),
        ]);
        fixture.server.on("textDocument/documentSymbol", found);
        let itself = location(&fixture, "trait.go", RS, impl_at);
        fixture.server.on("textDocument/implementation", json!([itself]));
        fixture.server.on("textDocument/declaration", location(&fixture, "trait.go", RS, "area"));
        fixture.open("trait.go");
        let path = fixture.path("trait.go");
        let at = chars(RS, method) + 3;
        let went = fixture
            .dispatch("navigate.superMethod.goTo", json!({ "path": path, "offset": at, "waitMs": 5000 }))
            .unwrap();
        assert_eq!(state(went), (false, 1, true, None, "superMethod".to_owned()));
        assert_eq!(fixture.server.requests("textDocument/declaration")[0]["position"], utf16(RS, impl_at));
        assert_eq!(caret(&fixture), (path.clone(), chars(RS, "area")));
        let went = fixture.dispatch("navigate.superMethod.goTo", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(state(went), (false, 0, false, Some(t!("workbench-nav-no-super-method")), "superMethod".to_owned()));
        assert_eq!(fixture.server.requests("textDocument/implementation").len(), 1, "a trait's method is not asked");
    }

    #[test]
    fn a_function_has_no_super_method_and_a_server_error_is_the_hint() {
        let mut fixture = fixture(&[("file.go", IMPL)]);
        fixture.server.on("textDocument/documentSymbol", symbols());
        fixture.server.reply(
            "textDocument/typeDefinition",
            Reply::Error { code: 0, message: "no enclosing expression has a type".to_owned() },
        );
        fixture.open("file.go");
        let path = fixture.path("file.go");
        let at = chars(IMPL, "main()");
        let params = json!({ "path": path, "offset": at, "waitMs": 5000 });
        let went = fixture.dispatch("navigate.superMethod.goTo", params.clone()).unwrap();
        assert_eq!(state(went), (false, 0, false, Some(t!("workbench-nav-no-super-method")), "superMethod".to_owned()));
        let went = fixture.dispatch("navigate.typeDeclaration.goTo", params).unwrap();
        assert_eq!(
            state(went),
            (false, 0, false, Some(t!("workbench-nav-no-type-declaration")), "typeDeclaration".to_owned())
        );
    }

    #[test]
    fn an_answer_after_the_caret_moved_or_the_text_changed_is_dropped() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let target = location(&fixture, "lib.go", LIB, "Println");
        fixture.server.reply("textDocument/definition", Reply::Delayed(target, Duration::from_millis(100)));
        fixture.open("main.go");
        let main = fixture.path("main.go");
        assert!(fixture.dispatch("navigate.declaration.goTo", json!({ "offset": 3 })).unwrap()["pending"] == true);
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": 5 }] })).unwrap();
        let read = fixture.dispatch("navigate.lookup.poll", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(state(read), (false, 0, false, None, "declaration".to_owned()));
        assert_eq!(caret(&fixture), (main.clone(), 5));

        assert!(fixture.dispatch("navigate.declaration.goTo", json!({ "offset": 3 })).unwrap()["pending"] == true);
        fixture.dispatch("editor.edit.insert", json!({ "text": "x" })).unwrap();
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": 5 }] })).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let read = fixture.dispatch("navigate.lookup.poll", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(state(read), (false, 0, false, None, "declaration".to_owned()));
        assert_eq!(caret(&fixture).0, main);

        fixture.server.reply("textDocument/definition", Reply::Hold);
        let asked = fixture.server.requests("textDocument/definition").len();
        fixture.dispatch("navigate.declaration.goTo", json!({ "offset": 3 })).unwrap();
        fixture.server.wait_for("textDocument/definition", asked + 1, Duration::from_secs(5));
        fixture.dispatch("navigate.lookup.cancel", json!({})).unwrap();
        assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, Duration::from_secs(5)).len(), 1);
        assert!(fixture.dispatch("navigate.lookup.poll", json!({})).unwrap()["pending"] == false);
    }

    #[test]
    fn a_location_link_goes_to_its_selection_range_in_utf16() {
        let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
        let uri = zen_lsp::position::file_uri(&fixture.path("lib.go"));
        let name = utf16(LIB, "Println");
        let link = json!([{
            "originSelectionRange": { "start": utf16(MAIN, "Println"), "end": utf16(MAIN, "(ё);") },
            "targetUri": uri,
            "targetRange": { "start": utf16(LIB, "/*"), "end": utf16(LIB, "{}") },
            "targetSelectionRange": { "start": name, "end": utf16(LIB, "(a ...") }
        }]);
        fixture.server.on("textDocument/definition", link);
        fixture.open("main.go");
        let at = chars(MAIN, "ё); Println");
        let went = fixture.dispatch("navigate.declaration.goTo", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(fixture.server.requests("textDocument/definition")[0]["position"], utf16(MAIN, "ё); Println"));
        assert!(went["navigated"].as_bool().unwrap());
        assert_eq!(went["symbol"], "ё");
        assert_eq!(
            went["targets"][0]["column"],
            LIB.lines().nth(2).unwrap().split("Println").next().unwrap().chars().count() + 1
        );
        assert_eq!(caret(&fixture), (fixture.path("lib.go"), chars(LIB, "Println")));
    }

    #[test]
    fn a_lookup_that_cannot_start_fails_and_tells_the_window() {
        let mut fixture = fixture(&[("main.go", MAIN), ("notes.txt", "text\n")]);
        fixture.dispatch("file.document.open", json!({ "path": fixture.path("notes.txt") })).unwrap();
        assert!(fixture.dispatch("navigate.declaration.goTo", json!({})).is_err());
        let read = fixture.dispatch("navigate.lookup.poll", json!({})).unwrap();
        assert_eq!(read["error"], t!("workbench-language-not-running"));
    }

    #[test]
    fn word_at_takes_the_identifier_around_an_offset() {
        assert_eq!(word_at("a := Бета(1)", 6).as_deref(), Some("Бета"));
        assert_eq!(word_at("a := Бета(1)", 9).as_deref(), Some("Бета"));
        assert_eq!(word_at("a := (1)", 5), None);
    }
}

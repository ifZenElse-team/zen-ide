use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::DocumentRequest;

use super::{Languages, byte_offset, char_offset, request_at, with_languages};
use crate::brackets::Highlighters;
use crate::templates::{self, Template};
use crate::todo_patterns::TodoPatterns;
use crate::{Document, Workbench};

const LIMIT: usize = 200;

const HIDDEN_COPY: &str = "zen_expression_";

pub(super) fn is_hidden_copy(uri: &str) -> bool {
    uri.rsplit('/').next().is_some_and(|name| name.starts_with(HIDDEN_COPY))
}

pub(super) struct ExpressionCompletion {
    path: PathBuf,
    uri: String,
    text: String,
    context: String,
    pending: Option<zen_lsp::Pending>,
    items: Vec<Item>,
    error: Option<String>,
}

pub(super) struct Completion {
    path: PathBuf,
    start: usize,
    request: Option<DocumentRequest>,
    items: Vec<Item>,
    error: Option<String>,
    plugin: bool,
    listed: Option<CompletionList>,
    incomplete: bool,
    asked: String,
    database: Option<u64>,
    keys: bool,
    smart: bool,
    dependencies: Option<u64>,
    coordinates: bool,
}

impl Completion {
    fn word(&self) -> fn(char) -> bool {
        word_of(self.keys, self.plugin, self.coordinates)
    }
}

#[derive(Clone, Debug)]
struct Item {
    label: String,
    kind: String,
    detail: Option<String>,
    documentation: Option<String>,
    filter: String,
    sort: String,
    text: String,
    snippet: bool,
    caret: Option<usize>,
    edit_start: Option<usize>,
    additional: Vec<(Range<usize>, String)>,
    template: Option<Template>,
    replace_from: Option<usize>,
    todo: bool,
    server: bool,
    preselect: bool,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    database: bool,
    dependency: bool,
}

/// An item as listed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListedItem {
    /// Its index, for `editor.completion.accept`.
    pub index: usize,
    /// What is shown, e.g. `Println`.
    pub label: String,
    /// Its kind: `function`, `method`, `variable`, `field`, `constant`, `struct`, `interface`,
    /// `module`, `keyword`, `snippet` and others of the protocol.
    pub kind: String,
    /// Its type or signature, e.g. `func(a ...any) (n int, err error)`.
    pub detail: Option<String>,
    /// Its documentation, Markdown or plain text.
    pub documentation: Option<String>,
    /// The characters of the label that match what was typed (character indices).
    pub matches: Vec<usize>,
}

/// What `editor.completion.list` reports.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompletionList {
    /// Whether a completion is in progress.
    pub open: bool,
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// What has been typed of the word so far.
    pub prefix: String,
    /// The matching items, best first.
    pub items: Vec<ListedItem>,
    /// Why there is nothing, if the server failed.
    pub error: Option<String>,
    /// A number that changes whenever the listed items do, for `since`.
    pub revision: u64,
    /// Whether the items are those of the `since` revision: then they are left out.
    pub unchanged: bool,
}

/// The parameters of `editor.completion.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The revision the caller already holds: when the items are still those, they are left out.
    pub since: Option<u64>,
}

fn next_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_plugin_word(c: char) -> bool {
    is_word(c) || c == '-' || c == '$'
}

fn is_key_word(c: char) -> bool {
    !c.is_whitespace() && c != '"' && c != '\\'
}

fn is_coordinate_word(c: char) -> bool {
    is_plugin_word(c) || c == '.'
}

fn word_of(keys: bool, plugin: bool, coordinates: bool) -> fn(char) -> bool {
    match (keys, coordinates, plugin) {
        (true, _, _) => is_key_word,
        (false, true, _) => is_coordinate_word,
        (false, false, true) => is_plugin_word,
        (false, false, false) => is_word,
    }
}

fn plugin_items(suggestions: zen_syntax::Suggestions) -> Vec<Item> {
    suggestions
        .items
        .into_iter()
        .enumerate()
        .map(|(index, suggestion)| Item {
            filter: suggestion.label.clone(),
            sort: format!("~{index:05}"),
            kind: suggestion.kind,
            detail: suggestion.detail,
            documentation: None,
            text: suggestion.insert,
            snippet: false,
            caret: suggestion.caret,
            edit_start: Some(suggestions.from),
            additional: Vec::new(),
            template: None,
            replace_from: None,
            todo: false,
            label: suggestion.label,
            server: false,
            preselect: false,
            database: false,
            dependency: false,
        })
        .collect()
}

fn documented_items((suggestions, docs): (zen_syntax::Suggestions, Vec<Option<String>>)) -> Vec<Item> {
    let mut items = plugin_items(suggestions);
    for (item, documentation) in items.iter_mut().zip(docs) {
        item.documentation = documentation;
    }
    items
}

#[cfg(not(target_arch = "wasm32"))]
fn database_items(session: &mut Session, path: &Path, caret: usize, read: bool) -> Option<(Vec<Item>, Option<u64>)> {
    let offered = crate::database::assist::suggest(session, path, caret, read)?;
    let revision = offered.loading.then(|| crate::database::assist::revision(session));
    let Some(suggestions) = offered.suggestions else { return Some((Vec::new(), revision)) };
    let from = suggestions.from;
    let items = suggestions
        .items
        .into_iter()
        .enumerate()
        .map(|(index, suggestion)| Item {
            kind: database_kind(suggestion.kind).to_owned(),
            detail: suggestion.detail,
            documentation: None,
            filter: suggestion.filter,
            sort: format!("{}{index:06}", suggestion.rank),
            text: suggestion.insert,
            snippet: false,
            caret: suggestion.caret,
            edit_start: Some(from),
            additional: Vec::new(),
            template: None,
            replace_from: None,
            todo: false,
            label: suggestion.label,
            server: false,
            preselect: false,
            database: true,
            dependency: false,
        })
        .collect();
    Some((items, revision))
}

#[cfg(target_arch = "wasm32")]
fn database_items(_: &mut Session, _: &Path, _: usize, _: bool) -> Option<(Vec<Item>, Option<u64>)> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn database_kind(kind: zen_db::assist::ItemKind) -> &'static str {
    use zen_db::assist::ItemKind;
    match kind {
        ItemKind::Keyword => "keyword",
        ItemKind::Schema => "schema",
        ItemKind::Table => "table",
        ItemKind::View => "view",
        ItemKind::Column => "column",
        ItemKind::Function => "function",
        ItemKind::Alias => "alias",
        ItemKind::Join => "join",
        ItemKind::Command => "command",
        ItemKind::Key => "key",
    }
}

#[cfg(target_arch = "wasm32")]
fn refresh_database(_: &mut Session) {}

#[cfg(not(target_arch = "wasm32"))]
fn refresh_database(session: &mut Session) {
    let waiting = session
        .state::<Languages>()
        .and_then(|languages| languages.completion.as_ref())
        .and_then(|completion| completion.database.map(|revision| (completion.path.clone(), revision)));
    let Some((path, revision)) = waiting else { return };
    crate::database::assist::poll(session, &path);
    if crate::database::assist::revision(session) == revision {
        return;
    }
    let caret = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .filter(|document| document.path() == path)
        .map(|document| byte_offset(document.text(), document.selections().primary().head));
    let Some(caret) = caret else { return };
    let offered = database_items(session, &path, caret, false);
    let languages = session.state_mut::<Languages>();
    let Some(completion) = languages.completion.as_mut().filter(|completion| completion.path == path) else { return };
    completion.database = None;
    if let Some((items, _)) = offered {
        completion.items.retain(|item| !item.database);
        completion.items.extend(items);
        completion.listed = None;
    }
}

fn template_items(templates: Vec<Template>) -> Vec<Item> {
    templates
        .into_iter()
        .map(|template| Item {
            label: template.abbreviation.clone(),
            kind: "template".to_owned(),
            detail: Some(template.description_text()).filter(|text| !text.is_empty()),
            documentation: Some(format!("```\n{}\n```", templates::expand(&template, "", "    ").text)),
            filter: template.abbreviation.clone(),
            sort: format!("~~{}", template.abbreviation),
            text: String::new(),
            snippet: false,
            caret: None,
            edit_start: None,
            additional: Vec::new(),
            template: Some(template),
            replace_from: None,
            todo: false,
            server: false,
            preselect: false,
            database: false,
            dependency: false,
        })
        .collect()
}

fn postfix_items(offered: Vec<templates::postfix::Offered>) -> Vec<Item> {
    let mut items = template_items(offered.iter().map(|offered| offered.template.clone()).collect());
    for (item, offered) in items.iter_mut().zip(&offered) {
        item.replace_from = Some(offered.expression_start);
        item.kind = "postfix".to_owned();
    }
    items
}

fn applicable_here(session: &Session, path: &Path, text: &str, head: usize) -> Vec<Template> {
    let Some(document) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
    else {
        return Vec::new();
    };
    let caret = byte_offset(text, head);
    let word = text[..caret].chars().rev().take_while(|c| is_word(*c)).count();
    let place = templates::context_at(session, document, head - word, head);
    templates::applicable(session, path).into_iter().filter(|template| template.expands_in(place)).collect()
}

fn todo_items(session: &Session) -> Vec<Item> {
    let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return Vec::new() };
    let text = document.text();
    let caret = byte_offset(text, document.selections().primary().head);
    if !in_comment(session, document, caret) {
        return Vec::new();
    }
    TodoPatterns::of(session)
        .words()
        .into_iter()
        .enumerate()
        .map(|(index, word)| Item {
            label: word.clone(),
            kind: "text".to_owned(),
            detail: None,
            documentation: None,
            filter: word.clone(),
            sort: format!("~~~{index:05}"),
            text: word,
            snippet: false,
            caret: None,
            edit_start: None,
            additional: Vec::new(),
            template: None,
            replace_from: None,
            todo: true,
            server: false,
            preselect: false,
            database: false,
            dependency: false,
        })
        .collect()
}

fn in_comment(session: &Session, document: &Document, caret: usize) -> bool {
    let text = document.text();
    let before = &text[..caret];
    let word = before.char_indices().rev().take_while(|(_, c)| is_word(*c)).last();
    let Some((at, _)) = word.or_else(|| before.char_indices().next_back()) else { return false };
    let highlighting = session.state::<Highlighters>().and_then(|highlighters| highlighters.highlighting_of(document));
    highlighting.is_some_and(|highlighting| {
        let highlighting = highlighting.lock().unwrap_or_else(PoisonError::into_inner);
        highlighting.kind_at(text, at).is_some_and(zen_syntax::HighlightKind::is_comment)
    })
}

fn todo_matches(prefix: &str, word: &str) -> bool {
    prefix != word && word.to_lowercase().starts_with(&prefix.to_lowercase())
}

fn put_word_at_carets(document: &mut Document, word: &str, replace: bool, is_word: fn(char) -> bool) {
    let text = document.text().to_owned();
    let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
    for selection in document.selections().iter() {
        let head = byte_offset(&text, selection.head);
        let start =
            text[..head].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(head, |(at, _)| at);
        if !word.to_lowercase().starts_with(&text[start..head].to_lowercase()) {
            continue;
        }
        let end = if replace {
            head + text[head..]
                .char_indices()
                .take_while(|(_, c)| is_word(*c))
                .last()
                .map_or(0, |(at, c)| at + c.len_utf8())
        } else {
            head
        };
        let range = char_offset(&text, start)..char_offset(&text, end);
        if replacements.last().is_none_or(|(last, _)| last.end <= range.start) {
            replacements.push((range, word.to_owned()));
        }
    }
    document.replace_ranges(replacements);
}

fn kind_name(kind: u64) -> &'static str {
    match kind {
        2 => "method",
        3 => "function",
        4 => "constructor",
        5 => "field",
        6 => "variable",
        7 => "class",
        8 => "interface",
        9 => "module",
        10 => "property",
        12 => "value",
        13 => "enum",
        14 => "keyword",
        15 => "snippet",
        20 => "enumMember",
        21 => "constant",
        22 => "struct",
        25 => "typeParameter",
        _ => "text",
    }
}

fn item(text: &str, lines: &zen_lsp::position::LineStarts, value: &Value) -> Option<Item> {
    let label = value["label"].as_str()?.to_owned();
    let range_start = |range: &Value| -> Option<usize> {
        let position = lsp_types::Position {
            line: range["start"]["line"].as_u64()? as u32,
            character: range["start"]["character"].as_u64()? as u32,
        };
        lines.offset(text, position)
    };
    let range = |range: &Value| -> Option<Range<usize>> {
        let start = range_start(range)?;
        let end = lsp_types::Position {
            line: range["end"]["line"].as_u64()? as u32,
            character: range["end"]["character"].as_u64()? as u32,
        };
        Some(start..lines.offset(text, end)?.max(start))
    };
    let edit = &value["textEdit"];
    let (text_to_insert, edit_start) = if edit.is_object() {
        let span = if edit["range"].is_object() { &edit["range"] } else { &edit["insert"] };
        (edit["newText"].as_str().unwrap_or(&label).to_owned(), range_start(span))
    } else {
        (value["insertText"].as_str().unwrap_or(&label).to_owned(), None)
    };
    let documentation = match &value["documentation"] {
        Value::String(text) => Some(text.clone()),
        Value::Object(markup) => markup.get("value").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
    .filter(|text| !text.trim().is_empty());
    let additional = value["additionalTextEdits"]
        .as_array()
        .map(|edits| {
            edits
                .iter()
                .filter_map(|edit| Some((range(&edit["range"])?, edit["newText"].as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    Some(Item {
        filter: value["filterText"].as_str().unwrap_or(&label).to_owned(),
        sort: value["sortText"].as_str().unwrap_or(&label).to_owned(),
        kind: kind_name(value["kind"].as_u64().unwrap_or(1)).to_owned(),
        detail: value["detail"].as_str().map(str::to_owned).filter(|detail| !detail.is_empty()),
        documentation,
        text: text_to_insert,
        snippet: value["insertTextFormat"].as_u64() == Some(2),
        caret: None,
        edit_start,
        additional,
        template: None,
        replace_from: None,
        todo: false,
        label,
        server: false,
        preselect: value["preselect"] == true,
        database: false,
        dependency: false,
    })
}

pub(super) fn expand_snippet(snippet: &str) -> (String, usize) {
    let mut out = String::new();
    let mut stops: Vec<(u32, usize)> = Vec::new();
    let mut chars = snippet.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '$' => {
                let braced = chars.peek() == Some(&'{');
                if braced {
                    chars.next();
                }
                let mut number = String::new();
                while let Some(digit) = chars.peek().filter(|c| c.is_ascii_digit()) {
                    number.push(*digit);
                    chars.next();
                }
                let Ok(tab) = number.parse::<u32>() else {
                    out.push('$');
                    if braced {
                        out.push('{');
                    }
                    continue;
                };
                stops.push((tab, out.len()));
                if braced {
                    if chars.peek() == Some(&':') {
                        chars.next();
                        let mut depth = 0;
                        for c in chars.by_ref() {
                            match c {
                                '{' => depth += 1,
                                '}' if depth == 0 => break,
                                '}' => depth -= 1,
                                _ => {}
                            }
                            out.push(c);
                        }
                    } else {
                        for c in chars.by_ref() {
                            if c == '}' {
                                break;
                            }
                        }
                    }
                }
            }
            c => out.push(c),
        }
    }
    let caret = stops
        .iter()
        .filter(|(tab, _)| *tab > 0)
        .min_by_key(|(tab, _)| *tab)
        .or_else(|| stops.iter().find(|(tab, _)| *tab == 0))
        .map_or(out.len(), |(_, at)| *at);
    (out, caret)
}

impl Languages {
    fn poll_completion(&mut self) {
        let Some(completion) = &mut self.completion else { return };
        let Some(request) = &completion.request else { return };
        let Some(answer) = request.pending.try_answer() else { return };
        let path = completion.path.clone();
        let text = match self.server_for(&path) {
            Some((_, synced)) => synced.text.clone(),
            None => String::new(),
        };
        let Some(completion) = &mut self.completion else { return };
        completion.request = None;
        completion.listed = None;
        match answer {
            Ok(answer) => {
                let list = if answer.is_array() { &answer } else { &answer["items"] };
                let templates: Vec<String> = completion
                    .items
                    .iter()
                    .filter(|item| item.template.is_some())
                    .map(|item| item.label.clone())
                    .collect();
                let lines = zen_lsp::position::LineStarts::new(&text);
                let mut items: Vec<Item> = list
                    .as_array()
                    .map(|items| items.iter().filter_map(|value| item(&text, &lines, value)).collect())
                    .unwrap_or_default();
                items.retain(|item| item.kind != "snippet" || !templates.contains(&item.label));
                items.iter_mut().for_each(|item| item.server = true);
                completion.items.retain(|item| !item.server);
                items.append(&mut completion.items);
                completion.items = items;
                completion.incomplete = answer["isIncomplete"] == true;
            }
            Err(error) => completion.error = Some(error.to_string()),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_database_console(session: &Session, path: &Path) -> bool {
    crate::database::assist::is_bound(session, path)
}

#[cfg(target_arch = "wasm32")]
fn is_database_console(_: &Session, _: &Path) -> bool {
    false
}

pub struct StartCompletion;

impl Command for StartCompletion {
    type Params = NoParams;
    type Output = CompletionList;
    const ID: &'static str = "editor.completion.start";
    const TITLE: &'static str = "Basic Completion";
    const DESCRIPTION: &'static str = "Asks the language server, and the file's plug-in when it completes, what can be \
         typed at the caret of the active file; editor.completion.list reads the answer.";

    fn is_available(&self, session: &Session) -> bool {
        let path = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned());
        let Some(path) = path else { return false };
        session.state::<Languages>().is_some_and(|languages| languages.spec_for(&path).is_some())
            || crate::plugins::plugin_for(session, &path, zen_syntax::Feature::Completion).is_some()
            || crate::dependencies::completion::completes(session, &path)
            || templates::any_applicable(session, &path)
            || is_database_console(session, &path)
            || crate::resource_bundles::completes_in(session, &path)
            || crate::spring_boot::completes_in(session, &path)
            || crate::project_index::completes_in(session, &path)
            || session.state::<zen_modules::assist::LanguageAssists>().is_some_and(|assists| assists.serves(&path))
            || session.state::<Highlighters>().is_some_and(|highlighters| {
                path.file_name().is_some_and(|name| highlighters.for_name(&name.to_string_lossy()).is_some())
            })
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CompletionList, CommandError> {
        start(session, false)
    }
}

pub struct SmartCompletion;

impl Command for SmartCompletion {
    type Params = NoParams;
    type Output = CompletionList;
    const ID: &'static str = "editor.completion.smart";
    const TITLE: &'static str = "Type-Matching Completion";
    const DESCRIPTION: &'static str = "Completion at the caret of the active file with only what fits the expected type \
         (⌃⇧Space): the items the language server marks as preselected for the place, or, when it marks none, its \
         values, functions and types without keywords, snippets and templates; editor.completion.list reads them.";

    fn is_available(&self, session: &Session) -> bool {
        let path = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned());
        let Some(path) = path else { return false };
        session.state::<Languages>().is_some_and(|languages| languages.spec_for(&path).is_some())
            || crate::plugins::plugin_for(session, &path, zen_syntax::Feature::Completion).is_some()
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CompletionList, CommandError> {
        start(session, true)
    }
}

fn smart_keeps(item: &Item, any_preselected: bool) -> bool {
    if any_preselected {
        return item.preselect;
    }
    item.template.is_none() && !item.todo && !matches!(item.kind.as_str(), "keyword" | "snippet" | "text")
}

fn assisted(session: &Session, path: &Path, text: &str, offset: usize) -> Option<zen_syntax::Suggestions> {
    let assists = session.state::<zen_modules::assist::LanguageAssists>()?;
    assists.complete(&zen_modules::assist::AssistContext { path, text, offset })
}

fn start(session: &mut Session, smart: bool) -> Result<CompletionList, CommandError> {
    let active = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .map(|document| (document.path().to_owned(), document.text().to_owned(), document.selections().primary().head));
    let offered = active.as_ref().filter(|_| !smart).map(|(path, text, head)| {
        let mut offered = template_items(applicable_here(session, path, text, *head));
        offered.extend(postfix_items(templates::postfix::offered(session, path, text, byte_offset(text, *head))));
        offered.extend(todo_items(session));
        offered
    });
    let keys = active.as_ref().and_then(|(path, text, head)| {
        let caret = byte_offset(text, *head);
        crate::spring_boot::complete(session, path, text, caret)
            .or_else(|| crate::resource_bundles::complete(session, path, text, caret).map(|keys| (keys, Vec::new())))
            .or_else(|| crate::project_index::complete(session, path, text, caret).map(|keys| (keys, Vec::new())))
            .or_else(|| assisted(session, path, text, caret).map(|items| (items, Vec::new())))
    });
    let database = active
        .as_ref()
        .filter(|_| keys.is_none())
        .and_then(|(path, text, head)| database_items(session, path, byte_offset(text, *head), true));
    let dependencies =
        active.as_ref().filter(|_| !smart && database.is_none() && keys.is_none()).and_then(|(path, text, head)| {
            crate::dependencies::completion::offered(session, path, text, byte_offset(text, *head))
        });
    let waiting_dependencies = dependencies.as_ref().and_then(|offered| offered.revision);
    let coordinates = dependencies.is_some();
    let from_dependencies = dependencies.map(dependency_items);
    let suggested = active.filter(|_| database.is_none() && keys.is_none()).and_then(|(path, text, head)| {
        let plugin = crate::plugins::plugin_for(session, &path, zen_syntax::Feature::Completion);
        let items =
            plugin.map(|plugin| plugin.complete(&text, byte_offset(&text, head)).map(plugin_items).unwrap_or_default());
        match (items, from_dependencies) {
            (Some(mut items), Some(found)) => {
                items.extend(found);
                Some(items)
            }
            (items, found) => items.or(found),
        }
    });
    let offered = offered.unwrap_or_default();
    let started = with_languages(session, |languages, workbench| -> Result<(), String> {
        let document = workbench.active_document().ok_or_else(|| t!("workbench-error-no-document"))?;
        let path = document.path().to_owned();
        let text = document.text();
        let head = document.selections().primary().head;
        let caret = byte_offset(text, head);
        let plugin = suggested.is_some() && database.is_none();
        let word = word_of(false, plugin, coordinates);
        let start = keys.as_ref().map_or_else(
            || text[..caret].char_indices().rev().take_while(|(_, c)| word(*c)).last().map_or(caret, |(at, _)| at),
            |(keys, _)| keys.from,
        );
        let asked = (keys.is_none() && ((!plugin && database.is_none()) || languages.serves(&path))).then(|| {
            request_at(
                languages,
                workbench,
                &path,
                head,
                "textDocument/completion",
                json!({ "context": { "triggerKind": 1 } }),
            )
        });
        let request = match asked {
            Some(Ok(request)) => Some(request),
            Some(Err(error)) if !plugin && database.is_none() && offered.is_empty() => return Err(error),
            _ => None,
        };
        if let Some(old) = languages.completion.take().and_then(|completion| completion.request)
            && let Some((client, _)) = languages.server_for(&path)
        {
            client.cancel(&old.pending);
        }
        let from_keys = keys.is_some();
        let mut items = keys.map(documented_items).or(suggested).unwrap_or_default();
        let (from_database, waiting) = database.unwrap_or_default();
        items.extend(from_database);
        if !from_keys {
            items.extend(offered);
        }
        let asked = text[start..caret].to_owned();
        languages.completion = Some(Completion {
            path,
            start,
            request,
            items,
            error: None,
            plugin,
            listed: None,
            incomplete: false,
            asked,
            database: waiting,
            keys: from_keys,
            smart,
            dependencies: waiting_dependencies,
            coordinates,
        });
        Ok(())
    });
    started.map_err(CommandError::new)?;
    list(session, None)
}

fn refresh_dependencies(session: &mut Session) {
    let waiting = session
        .state::<Languages>()
        .and_then(|languages| languages.completion.as_ref())
        .and_then(|completion| completion.dependencies.map(|revision| (completion.path.clone(), revision)));
    let Some((path, revision)) = waiting else { return };
    if session.state_mut::<crate::dependencies::Dependencies>().generation() == revision {
        return;
    }
    let active = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .filter(|document| document.path() == path)
        .map(|document| {
            (document.text().to_owned(), byte_offset(document.text(), document.selections().primary().head))
        });
    let Some((text, caret)) = active else { return };
    let offered = crate::dependencies::completion::offered(session, &path, &text, caret);
    let languages = session.state_mut::<Languages>();
    let Some(completion) = languages.completion.as_mut().filter(|completion| completion.path == path) else { return };
    completion.dependencies = offered.as_ref().and_then(|offered| offered.revision);
    if let Some(offered) = offered {
        completion.items.retain(|item| !item.dependency);
        completion.items.extend(dependency_items(offered));
        completion.listed = None;
    }
}

fn dependency_items(offered: crate::dependencies::completion::Offered) -> Vec<Item> {
    let from = offered.from;
    offered
        .items
        .into_iter()
        .enumerate()
        .map(|(index, item)| Item {
            filter: item.label.clone(),
            sort: format!("{index:05}"),
            kind: "module".to_owned(),
            detail: item.detail,
            documentation: None,
            text: item.label.clone(),
            snippet: false,
            caret: None,
            edit_start: Some(from),
            additional: Vec::new(),
            template: None,
            todo: false,
            label: item.label,
            server: false,
            database: false,
            dependency: true,
            preselect: false,
            replace_from: None,
        })
        .collect()
}

fn list(session: &mut Session, since: Option<u64>) -> Result<CompletionList, CommandError> {
    refresh_database(session);
    refresh_dependencies(session);
    with_languages(session, |languages, workbench| {
        languages.poll_completion();
        let Some(completion) = &mut languages.completion else { return Ok(CompletionList::default()) };
        let document = workbench.active_document().filter(|document| document.path() == completion.path);
        let Some(document) = document else {
            languages.completion = None;
            return Ok(CompletionList::default());
        };
        let text = document.text();
        let caret = byte_offset(text, document.selections().primary().head);
        let prefix = text.get(completion.start..caret).map(str::to_owned);
        let word = completion.word();
        let Some(prefix) = prefix.filter(|prefix| prefix.chars().all(word)) else {
            languages.completion = None;
            return Ok(CompletionList::default());
        };
        if completion.incomplete && completion.request.is_none() && prefix != completion.asked {
            let (path, head) = (completion.path.clone(), document.selections().primary().head);
            let asked = request_at(
                languages,
                workbench,
                &path,
                head,
                "textDocument/completion",
                json!({ "context": { "triggerKind": 3 } }),
            );
            let Some(completion) = &mut languages.completion else { return Ok(CompletionList::default()) };
            completion.incomplete = false;
            completion.asked = prefix.clone();
            if let Ok(request) = asked {
                completion.request = Some(request);
                completion.listed = None;
            }
        }
        let Some(completion) = &mut languages.completion else { return Ok(CompletionList::default()) };
        let pending =
            completion.request.is_some() || completion.database.is_some() || completion.dependencies.is_some();
        if let Some(listed) = completion
            .listed
            .as_ref()
            .filter(|listed| listed.prefix == prefix && listed.pending == pending && listed.error == completion.error)
        {
            return Ok(if since == Some(listed.revision) {
                CompletionList { unchanged: true, ..listed.clone_without_items() }
            } else {
                listed.clone()
            });
        }
        let any_preselected = completion.items.iter().any(|item| item.preselect);
        let smart = completion.smart;
        let mut scored: Vec<(i64, &str, usize)> = completion
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if item.todo && !todo_matches(&prefix, &item.label) {
                    return None;
                }
                if smart && !smart_keeps(item, any_preselected) {
                    return None;
                }
                let score = if prefix.is_empty() { 0 } else { zen_index::score(&prefix, &item.filter, &item.filter)? };
                Some((score, item.sort.as_str(), index))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let items = scored
            .into_iter()
            .take(LIMIT)
            .map(|(_, _, index)| {
                let item = &completion.items[index];
                ListedItem {
                    index,
                    label: item.label.clone(),
                    kind: item.kind.clone(),
                    detail: item.detail.clone(),
                    documentation: item.documentation.clone(),
                    matches: matches(&prefix, &item.label),
                }
            })
            .collect();
        let listed = CompletionList {
            open: true,
            pending,
            prefix,
            items,
            error: completion.error.clone(),
            revision: next_revision(),
            unchanged: false,
        };
        completion.listed = Some(listed.clone());
        Ok(listed)
    })
}

impl CompletionList {
    fn clone_without_items(&self) -> Self {
        Self {
            open: self.open,
            pending: self.pending,
            prefix: self.prefix.clone(),
            items: Vec::new(),
            error: self.error.clone(),
            revision: self.revision,
            unchanged: false,
        }
    }
}

fn matches(query: &str, label: &str) -> Vec<usize> {
    let mut wanted = query.chars().flat_map(char::to_lowercase).peekable();
    let mut found = Vec::new();
    for (index, c) in label.chars().enumerate() {
        if wanted.peek().is_some_and(|want| c.to_lowercase().next() == Some(*want)) {
            found.push(index);
            wanted.next();
        }
    }
    found
}

pub struct ListCompletion;

impl Command for ListCompletion {
    type Params = ListParams;
    type Output = CompletionList;
    const ID: &'static str = "editor.completion.list";
    const TITLE: &'static str = "Completion Items";
    const DESCRIPTION: &'static str = "The items of the completion in progress that match what has been typed, best first; closes it when the caret left the word. With since set to the revision listed last, an unchanged list comes back with unchanged true and no items.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<CompletionList, CommandError> {
        list(session, params.since)
    }
}

/// The parameters of `editor.completion.accept`.
#[derive(Deserialize, JsonSchema)]
pub struct AcceptParams {
    /// The item, as `editor.completion.list` numbers it.
    pub index: usize,
    /// Replace the rest of the word after the caret too (Tab), instead of inserting (Enter).
    #[serde(default)]
    pub replace: bool,
}

pub struct AcceptCompletion;

impl Command for AcceptCompletion {
    type Params = AcceptParams;
    type Output = ();
    const ID: &'static str = "editor.completion.accept";
    const TITLE: &'static str = "Choose Lookup Item";
    const DESCRIPTION: &'static str =
        "Puts a completion item into the text, with its extra edits such as an import, and ends the completion.";

    fn run(&self, session: &mut Session, params: AcceptParams) -> Result<(), CommandError> {
        if let Some((path, range, template)) = template_accepted(session, &params) {
            templates::insert(session, Some(&path), range, &template)?;
            return Ok(());
        }
        with_languages(session, |languages, workbench| {
            let completion =
                languages.completion.take().ok_or_else(|| CommandError::new(t!("workbench-completion-none")))?;
            let item = completion
                .items
                .get(params.index)
                .cloned()
                .ok_or_else(|| CommandError::new(t!("workbench-completion-none")))?;
            let document = workbench
                .active_document_mut()
                .filter(|document| document.path() == completion.path)
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
            let word = completion.word();
            if item.todo {
                put_word_at_carets(document, &item.text, params.replace, word);
                return Ok(());
            }
            let text = document.text().to_owned();
            let caret = byte_offset(&text, document.selections().primary().head);
            let start = item.edit_start.filter(|start| *start <= caret).unwrap_or(completion.start);
            let end = if params.replace {
                caret
                    + text[caret..]
                        .char_indices()
                        .take_while(|(_, c)| word(*c))
                        .last()
                        .map_or(0, |(at, c)| at + c.len_utf8())
            } else {
                caret
            };
            let (insert, caret_in) = if item.snippet {
                expand_snippet(&item.text)
            } else {
                (item.text.clone(), item.caret.filter(|at| item.text.is_char_boundary(*at)).unwrap_or(item.text.len()))
            };
            let (insert, caret_in) = keep_existing_arguments(&text[end..], insert, caret_in);
            let caret_in = document.with_line_separator(insert[..caret_in].to_owned()).len();
            let insert = document.with_line_separator(insert);
            let mut replacements: Vec<(Range<usize>, String)> = item
                .additional
                .iter()
                .filter(|(range, _)| range.end <= start || range.start >= end)
                .map(|(range, new)| (range.clone(), document.with_line_separator(new.clone())))
                .collect();
            replacements.push((start..end, insert.clone()));
            replacements.sort_by_key(|(range, _)| range.start);
            let start_chars = char_offset(&text, start);
            let in_chars: Vec<(Range<usize>, String)> = replacements
                .into_iter()
                .map(|(range, new)| (char_offset(&text, range.start)..char_offset(&text, range.end), new))
                .collect();
            let shift: isize = in_chars
                .iter()
                .filter(|(range, _)| range.end <= start_chars && range.start < start_chars)
                .map(|(range, new)| new.chars().count() as isize - range.len() as isize)
                .sum();
            if document.replace_ranges(in_chars) {
                let at = (start_chars as isize + shift) as usize + char_offset(&insert, caret_in);
                document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(at)));
            }
            Ok(())
        })
    }
}

fn template_accepted(session: &mut Session, params: &AcceptParams) -> Option<(PathBuf, Range<usize>, Template)> {
    let completion = session.state::<Languages>()?.completion.as_ref()?;
    let item = completion.items.get(params.index)?;
    let template = item.template.clone()?;
    let document =
        session.state::<Workbench>()?.active_document().filter(|document| document.path() == completion.path)?;
    let text = document.text();
    let caret = byte_offset(text, document.selections().primary().head);
    let start = item.replace_from.unwrap_or(completion.start).min(caret);
    let end = if params.replace {
        caret
            + text[caret..]
                .char_indices()
                .take_while(|(_, c)| is_word(*c))
                .last()
                .map_or(0, |(at, c)| at + c.len_utf8())
    } else {
        caret
    };
    let range = char_offset(text, start)..char_offset(text, end);
    let path = completion.path.clone();
    session.state_mut::<Languages>().completion = None;
    Some((path, range, template))
}

fn keep_existing_arguments(after: &str, insert: String, caret_in: usize) -> (String, usize) {
    match insert.find(['(', '[']) {
        Some(end) if end > 0 && after.starts_with('(') => (insert[..end].to_owned(), end),
        _ => (insert, caret_in),
    }
}

pub struct CancelCompletion;

impl Command for CancelCompletion {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "editor.completion.cancel";
    const TITLE: &'static str = "Close Completion";
    const DESCRIPTION: &'static str = "Ends the completion in progress without changing the text.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        with_languages(session, |languages, _| {
            if let Some(completion) = languages.completion.take()
                && let Some(request) = completion.request
                && let Some((client, _)) = languages.server_for(&completion.path)
            {
                client.cancel(&request.pending);
            }
        });
        Ok(())
    }
}

pub struct InsertTemplate;

impl Command for InsertTemplate {
    type Params = NoParams;
    type Output = CompletionList;
    const ID: &'static str = "editor.templates.insert";
    const TITLE: &'static str = "Insert Live Template...";
    const DESCRIPTION: &'static str = "Lists the live templates of the active file's language at the caret as a completion \
         (⌘J); editor.completion.accept puts the chosen one in and starts filling it in.";

    fn is_available(&self, session: &Session) -> bool {
        session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .is_some_and(|document| document.image().is_none() && templates::any_applicable(session, document.path()))
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CompletionList, CommandError> {
        let path = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let items = template_items(templates::applicable(session, &path));
        with_languages(session, |languages, workbench| {
            let Some(document) = workbench.active_document() else { return };
            let text = document.text();
            let caret = byte_offset(text, document.selections().primary().head);
            let start =
                text[..caret].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(caret, |(at, _)| at);
            if let Some(old) = languages.completion.take().and_then(|completion| completion.request)
                && let Some((client, _)) = languages.server_for(&path)
            {
                client.cancel(&old.pending);
            }
            languages.completion = Some(Completion {
                path,
                start,
                request: None,
                items,
                error: None,
                plugin: false,
                listed: None,
                incomplete: false,
                asked: String::new(),
                database: None,
                keys: false,
                smart: false,
                dependencies: None,
                coordinates: false,
            });
        });
        list(session, None)
    }
}

/// An expression typed in a field of the IDE, completed as if it were typed at a line of a file.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionParams {
    pub path: PathBuf,
    /// The line, from 0, the expression is evaluated at, such as a breakpoint's.
    pub line: u32,
    /// The field's text.
    pub text: String,
    /// The caret in the field, in characters; the end of the text when absent.
    #[serde(default)]
    pub caret: Option<usize>,
}

/// A completion of an expression field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionItem {
    /// What is shown, e.g. `Println`.
    pub label: String,
    /// What replaces the word at the caret.
    pub insert: String,
    /// Its kind, as for `editor.completion.list`.
    pub kind: String,
    /// Its type or signature.
    pub detail: Option<String>,
}

/// What `debug.expression.complete` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionCompletions {
    /// Whether the language server has not answered yet; ask again.
    pub pending: bool,
    /// Where the word at the caret starts, in characters: the items replace the text from there to the caret.
    pub start: usize,
    /// The items matching the word at the caret, best first.
    pub items: Vec<ExpressionItem>,
    /// Why there is nothing, if the server failed.
    pub error: Option<String>,
}

fn hidden_copy(path: &Path) -> PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{HIDDEN_COPY}{name}"))
}

fn copy_with(text: &str, line: u32, typed: &str) -> (String, usize) {
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(end) => start += end + 1,
            None => break,
        }
    }
    let indent = text[start..].chars().take_while(|c| *c == ' ' || *c == '\t').map(char::len_utf8).sum::<usize>();
    let at = start + indent;
    let indentation = &text[start..at];
    let copy = format!("{}{typed}\n{indentation}{}", &text[..at], &text[at..]);
    (copy, at + typed.len())
}

impl Languages {
    fn close_expression(&mut self) {
        if let Some(old) = self.expression.take()
            && let Some((client, _)) = self.server_for(&old.path)
        {
            let _ = client.did_close(&old.uri);
        }
    }

    pub(super) fn poll_expression(&mut self, wait: std::time::Duration) {
        let Some(expression) = &mut self.expression else { return };
        let Some(pending) = &expression.pending else { return };
        let answer = match pending.wait(wait) {
            Err(zen_lsp::LspError::Timeout) => return,
            answer => answer,
        };
        expression.pending = None;
        match answer {
            Ok(answer) => {
                let list = if answer.is_array() { &answer } else { &answer["items"] };
                let lines = zen_lsp::position::LineStarts::new(&expression.text);
                expression.items = list
                    .as_array()
                    .map(|items| items.iter().filter_map(|value| item(&expression.text, &lines, value)).collect())
                    .unwrap_or_default();
            }
            Err(error) => expression.error = Some(error.to_string()),
        }
        let (path, uri) = (expression.path.clone(), expression.uri.clone());
        if let Some((client, _)) = self.server_for(&path) {
            let _ = client.did_close(&uri);
        }
    }
}

pub struct CompleteExpression;

impl Command for CompleteExpression {
    type Params = ExpressionParams;
    type Output = ExpressionCompletions;
    const ID: &'static str = "debug.expression.complete";
    const TITLE: &'static str = "Complete Expression";
    const DESCRIPTION: &'static str = "Completes an expression typed in a field of the IDE (a breakpoint's condition, \
         an expression to log) as if it were typed at a line of a file: the language server gets a hidden copy of the \
         file with the text at that line, the file itself is not changed. Call it again with the same text to read \
         the answer while `pending`.";

    fn run(&self, session: &mut Session, params: ExpressionParams) -> Result<ExpressionCompletions, CommandError> {
        let chars: Vec<char> = params.text.chars().collect();
        let caret = params.caret.unwrap_or(chars.len()).min(chars.len());
        let start = chars[..caret].iter().rposition(|c| !is_word(*c)).map_or(0, |at| at + 1);
        let prefix: String = chars[start..caret].iter().collect();
        let context: String = chars[..start].iter().collect();
        let typed: String = chars[..caret].iter().collect();
        let path = params.path;
        with_languages(session, |languages, workbench| -> Result<(), CommandError> {
            let same = languages
                .expression
                .as_ref()
                .is_some_and(|expression| expression.path == path && expression.context == context);
            if same {
                languages.poll_expression(std::time::Duration::ZERO);
                return Ok(());
            }
            languages.close_expression();
            let open = workbench.documents().iter().any(|document| document.path() == path);
            languages.sync(workbench);
            if !languages.serves(&path) {
                return Err(CommandError::new(t!("workbench-language-not-running")));
            }
            if !open {
                languages.lend(&path).map_err(CommandError::new)?;
            }
            let language = languages.spec_for(&path).map(|spec| spec.language_for(&path)).unwrap_or_default();
            let (client, synced) =
                languages.server_for(&path).ok_or_else(|| CommandError::new(t!("workbench-language-not-running")))?;
            let (copy, at) = copy_with(&synced.text, params.line, &typed);
            let uri = zen_lsp::position::file_uri(&hidden_copy(&path));
            let position = zen_lsp::position::position(&copy, at);
            client.did_open(&uri, &language, &copy).map_err(|error| CommandError::new(error.to_string()))?;
            let request =
                json!({ "textDocument": { "uri": uri }, "position": position, "context": { "triggerKind": 1 } });
            let pending = client
                .request("textDocument/completion", request)
                .map_err(|error| CommandError::new(error.to_string()))?;
            languages.expression = Some(ExpressionCompletion {
                path: path.clone(),
                uri,
                text: copy,
                context,
                pending: Some(pending),
                items: Vec::new(),
                error: None,
            });
            Ok(())
        })?;
        Ok(with_languages(session, |languages, _| {
            let Some(expression) = &languages.expression else { return ExpressionCompletions::default() };
            let mut scored: Vec<(i64, &Item)> = expression
                .items
                .iter()
                .filter_map(|item| {
                    let score =
                        if prefix.is_empty() { 0 } else { zen_index::score(&prefix, &item.filter, &item.filter)? };
                    Some((score, item))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.sort.cmp(&b.1.sort)));
            let items = scored
                .into_iter()
                .take(LIMIT)
                .map(|(_, item)| ExpressionItem {
                    label: item.label.clone(),
                    insert: if item.snippet { expand_snippet(&item.text).0 } else { item.text.clone() },
                    kind: item.kind.clone(),
                    detail: item.detail.clone(),
                })
                .collect();
            ExpressionCompletions {
                pending: expression.pending.is_some(),
                start,
                items,
                error: expression.error.clone(),
            }
        }))
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CompleteExpression)?;
    registry.register(StartCompletion)?;
    registry.register(SmartCompletion)?;
    registry.register(InsertTemplate)?;
    registry.register(ListCompletion)?;
    registry.register(AcceptCompletion)?;
    registry.register(CancelCompletion)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: std::time::Duration = std::time::Duration::from_secs(60);

    #[test]
    fn an_expression_is_completed_in_a_hidden_copy_at_its_line_and_the_file_is_left_alone() {
        let text = "package main\n\nfunc main() {\n\tcount := 1\n\tprintln(count)\n}\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        fixture.server.on(
            "textDocument/completion",
            json!({ "isIncomplete": false, "items": [
                { "label": "count", "kind": 6, "detail": "int" },
                { "label": "copy", "kind": 3, "insertTextFormat": 2, "insertText": "copy(${1:dst}, ${2:src})" },
                { "label": "main", "kind": 3 },
            ] }),
        );
        fixture.open("main.go");
        let path = fixture.path("main.go");
        let ask = |fixture: &mut crate::language::testing::Fixture, text: &str| {
            fixture.dispatch("debug.expression.complete", json!({ "path": path, "line": 4, "text": text })).unwrap()
        };
        ask(&mut fixture, "x > co");
        fixture.with(|languages, _| languages.poll_expression(MINUTE));
        let answer = ask(&mut fixture, "x > co");
        assert_eq!(answer["pending"], false, "{answer}");
        assert_eq!(answer["start"], 4);
        let labels: Vec<&str> =
            answer["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
        assert_eq!(labels, ["copy", "count"], "matched by the word at the caret: {answer}");
        assert_eq!(answer["items"][0]["insert"], "copy(dst, src)");
        let opened = fixture.server.requests("textDocument/didOpen");
        let hidden =
            opened.iter().find(|open| open["textDocument"]["uri"].as_str().is_some_and(is_hidden_copy)).unwrap();
        assert_eq!(
            hidden["textDocument"]["text"],
            "package main\n\nfunc main() {\n\tcount := 1\n\tx > co\n\tprintln(count)\n}\n"
        );
        let asked = fixture.server.requests("textDocument/completion");
        assert_eq!(asked.last().unwrap()["position"], json!({ "line": 4, "character": 7 }));
        assert_eq!(asked.last().unwrap()["textDocument"]["uri"], hidden["textDocument"]["uri"]);
        let hidden_uri = hidden["textDocument"]["uri"].as_str().unwrap().to_owned();
        let still_open = fixture.with(|languages, _| languages.server_for(&path).unwrap().0.version(&hidden_uri));
        assert_eq!(still_open, None, "the copy is closed once answered");
        let closed = fixture.server.wait_for("textDocument/didClose", 1, MINUTE);
        assert!(
            closed.iter().any(|close| close["textDocument"]["uri"] == hidden_uri.as_str()),
            "the server is told the copy is closed: {closed:?}"
        );
        let narrowed = ask(&mut fixture, "x > cou");
        assert_eq!(narrowed["items"].as_array().unwrap().len(), 1, "the same context filters without asking again");
        assert_eq!(fixture.server.requests("textDocument/completion").len(), asked.len());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(fixture.server.requests("textDocument/didChange").is_empty(), "the real document is not changed");
    }

    #[test]
    fn a_hidden_copy_is_closed_once_answered_even_if_nobody_asks_again() {
        let text = "package main\n\nfunc main() {\n\tcount := 1\n}\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        fixture.server.on("textDocument/completion", json!([{ "label": "count", "kind": 6 }]));
        let (wake, woken) = std::sync::mpsc::channel();
        fixture.with(|languages, _| {
            languages.set_waker(std::sync::Arc::new(move || {
                let _ = wake.send(());
            }))
        });
        fixture.open("main.go");
        let path = fixture.path("main.go");
        let params = json!({ "path": path, "line": 3, "text": "co" });
        let first = fixture.dispatch("debug.expression.complete", params.clone()).unwrap();
        assert_eq!(first["pending"], true);
        let uri = fixture.with(|languages, _| languages.expression.as_ref().unwrap().uri.clone());
        let open = |fixture: &mut crate::language::testing::Fixture| {
            fixture.with(|languages, _| languages.server_for(&path).unwrap().0.version(&uri)).is_some()
        };
        while open(&mut fixture) {
            woken.recv_timeout(std::time::Duration::from_secs(10)).expect("the server answers");
            fixture.dispatch("language.servers.sync", json!({})).unwrap();
        }
        let closed = fixture.server.wait_for("textDocument/didClose", 1, std::time::Duration::from_secs(10));
        assert!(closed.iter().any(|close| close["textDocument"]["uri"] == uri.as_str()), "{closed:?}");
        let asked = fixture.server.requests("textDocument/completion").len();
        let later = fixture.dispatch("debug.expression.complete", params).unwrap();
        assert_eq!(later["pending"], false, "the answer is kept for the field: {later}");
        assert_eq!(later["items"][0]["label"], "count");
        assert_eq!(fixture.server.requests("textDocument/completion").len(), asked);
    }

    #[test]
    fn snippets_become_text_with_a_caret() {
        assert_eq!(expand_snippet("Println(${1:a ...any})"), ("Println(a ...any)".to_owned(), 8));
        assert_eq!(
            expand_snippet("for ${1:i} := range ${2:x} {\n\t$0\n}"),
            ("for i := range x {\n\t\n}".to_owned(), 4)
        );
        assert_eq!(expand_snippet("Len()$0"), ("Len()".to_owned(), 5));
        assert_eq!(expand_snippet("plain"), ("plain".to_owned(), 5));
        assert_eq!(expand_snippet(r"cost \$5"), ("cost $5".to_owned(), 7));
    }

    #[test]
    fn an_existing_argument_list_is_kept() {
        assert_eq!(keep_existing_arguments("(2)\n", "Total()".to_owned(), 6), ("Total".to_owned(), 5));
        assert_eq!(keep_existing_arguments("(2)", "Add(a, b)".to_owned(), 4), ("Add".to_owned(), 3));
        assert_eq!(keep_existing_arguments(" + 1", "Total()".to_owned(), 6), ("Total()".to_owned(), 6));
        assert_eq!(keep_existing_arguments("(x)", "Name".to_owned(), 4), ("Name".to_owned(), 4));
        assert_eq!(keep_existing_arguments("(xs)", "Map[T any](xs []T)".to_owned(), 4), ("Map".to_owned(), 3));
        assert_eq!(
            keep_existing_arguments("", "Map[T any](xs []T)".to_owned(), 4),
            ("Map[T any](xs []T)".to_owned(), 4)
        );
        assert_eq!(keep_existing_arguments("(x)", "fmt.Sprint(a)".to_owned(), 11), ("fmt.Sprint".to_owned(), 10));
    }

    #[test]
    fn tab_and_enter_on_a_call_keep_its_arguments() {
        for (replace, text, caret) in [
            (true, "package main\n\nfunc f() { item.Total(2) }\n", "item.".len()),
            (false, "package main\n\nfunc f() { item.To(2) }\n", "item.To".len()),
        ] {
            let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
            fixture.server.on(
                "textDocument/completion",
                json!([{ "label": "Total", "kind": 2, "insertTextFormat": 2, "insertText": "Total(${1:n int})" }]),
            );
            fixture.open("main.go");
            let caret = text.find("item.").unwrap() + caret;
            let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
            fixture.dispatch("editor.completion.start", json!({})).unwrap();
            for _ in 0..1000 {
                let list = fixture.dispatch("editor.completion.list", json!({})).unwrap();
                if list["pending"] == false && !list["items"].as_array().is_none_or(Vec::is_empty) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            fixture.dispatch("editor.completion.accept", json!({ "index": 0, "replace": replace })).unwrap();
            let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
            assert!(document.text().contains("{ item.Total(2) }"), "replace {replace}: {}", document.text());
            let at = document.selections().primary().head;
            assert_eq!(at, text.find("item.").unwrap() + "item.Total".len(), "the caret is after the name");
        }
    }

    #[test]
    fn an_incomplete_list_is_asked_for_again_when_the_word_changes() {
        let text = "package main\n\nfunc f() { items. }\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = asked.clone();
        fixture.server.on_with("textDocument/completion", move |params| {
            let mut seen = seen.lock().unwrap();
            seen.push(params["context"]["triggerKind"].clone());
            if seen.len() == 1 {
                json!({ "isIncomplete": true, "items": [{ "label": "size", "kind": 2 }, { "label": "contains", "kind": 2 }] })
            } else {
                json!({ "isIncomplete": false, "items": [{ "label": "map", "kind": 2 }, { "label": "mapNotNull", "kind": 2 }] })
            }
        });
        fixture.open("main.go");
        let caret = text.find("items.").unwrap() + "items.".len();
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
        fixture.dispatch("editor.completion.start", json!({})).unwrap();
        let first = settled(&mut fixture);
        assert!(first["items"].to_string().contains("size"), "{first}");
        fixture.dispatch("editor.edit.type", json!({ "text": "ma" })).unwrap();
        let mut list = json!(null);
        for _ in 0..1000 {
            list = fixture.dispatch("editor.completion.list", json!({})).unwrap();
            if list["pending"] == false && list["items"].to_string().contains("\"map\"") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let labels: Vec<&str> =
            list["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
        assert_eq!(labels, ["map", "mapNotNull"], "the new answer in place of the old one");
        assert_eq!(*asked.lock().unwrap(), [json!(1), json!(3)], "asked again for an incomplete list");
        fixture.dispatch("editor.edit.type", json!({ "text": "p" })).unwrap();
        settled(&mut fixture);
        assert_eq!(asked.lock().unwrap().len(), 2, "a complete list is filtered, not asked for again");
    }

    #[test]
    fn items_are_read_with_their_edits() {
        let text = "package main\n\nfunc main() {\n\tfmt.Pri\n}\n";
        let value = serde_json::json!({
            "label": "Println",
            "kind": 3,
            "detail": "func(a ...any) (n int, err error)",
            "documentation": { "kind": "markdown", "value": "Println formats..." },
            "sortText": "00000",
            "filterText": "Println",
            "insertTextFormat": 2,
            "textEdit": { "range": { "start": { "line": 3, "character": 5 }, "end": { "line": 3, "character": 8 } }, "newText": "Println(${1:})" },
            "additionalTextEdits": [{ "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 0 } }, "newText": "import \"fmt\"\n" }]
        });
        let item = item(text, &zen_lsp::position::LineStarts::new(text), &value).unwrap();
        assert_eq!(item.kind, "function");
        assert_eq!(&text[item.edit_start.unwrap()..], "Pri\n}\n");
        assert_eq!(item.additional, vec![(13..13, "import \"fmt\"\n".to_owned())], "the start of line 2");
        assert_eq!(item.documentation.as_deref(), Some("Println formats..."));
        assert_eq!(matches("pln", "Println"), vec![0, 5, 6]);
    }

    fn fori() -> templates::LiveTemplates {
        let templates = templates::LiveTemplates::default();
        templates.declare([Template {
            language: "go".to_owned(),
            abbreviation: "fori".to_owned(),
            description: "A loop".to_owned(),
            text: "for $I$ := 0; $I$ < $N$; $I$++ {\n\t$END$\n}".to_owned(),
            variables: Vec::new(),
            imports: Vec::new(),
            contexts: Vec::new(),
            enabled: true,
        }]);
        templates
    }

    fn settled(fixture: &mut crate::language::testing::Fixture) -> Value {
        let mut list = json!(null);
        for _ in 0..1000 {
            list = fixture.dispatch("editor.completion.list", json!({})).unwrap();
            if list["pending"] == false {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        list
    }

    #[test]
    fn live_templates_join_the_completion_once_and_are_filled_in() {
        let text = "package main\n\nfunc f() {\n\tfo\n}\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        fixture.session.set_state(fori());
        fixture.server.on(
            "textDocument/completion",
            json!([{ "label": "fori", "kind": 15, "insertTextFormat": 2, "insertText": "for ${1:i} {}" }, { "label": "format", "kind": 3 }]),
        );
        fixture.open("main.go");
        let caret = text.find("fo\n").unwrap() + 2;
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
        fixture.dispatch("editor.completion.start", json!({})).unwrap();
        let list = settled(&mut fixture);
        let items = list["items"].as_array().unwrap();
        let labels: Vec<(&str, &str)> =
            items.iter().map(|item| (item["label"].as_str().unwrap(), item["kind"].as_str().unwrap())).collect();
        assert_eq!(labels.iter().filter(|(label, _)| *label == "fori").count(), 1, "{labels:?}");
        assert!(labels.contains(&("fori", "template")) && labels.contains(&("format", "function")), "{labels:?}");
        let index = items.iter().find(|item| item["label"] == "fori").unwrap()["index"].clone();
        fixture.dispatch("editor.completion.accept", json!({ "index": index })).unwrap();
        let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "package main\n\nfunc f() {\n\tfor  := 0;  < ; ++ {\n\t\t\n\t}\n}\n");
        assert_eq!(document.selections().len(), 3, "the variable's occurrences are selected");
        assert!(document.template_session().is_some());
    }

    #[test]
    fn type_matching_keeps_what_the_server_preselects_or_else_drops_keywords_and_templates() {
        let text = "package main\n\nfunc f() {\n\tvar n int = fo\n}\n";
        let labels = |answer: Value| -> Vec<String> {
            let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
            fixture.session.set_state(fori());
            fixture.server.on("textDocument/completion", answer);
            fixture.open("main.go");
            let caret = text.find("fo\n").unwrap() + 2;
            let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
            fixture.dispatch("editor.completion.smart", json!({})).unwrap();
            let list = settled(&mut fixture);
            list["items"].as_array().unwrap().iter().map(|item| item["label"].as_str().unwrap().to_owned()).collect()
        };
        let marked = json!([
            { "label": "format", "kind": 3, "detail": "func() string" },
            { "label": "fourCount", "kind": 6, "detail": "int", "preselect": true },
            { "label": "for", "kind": 14 }
        ]);
        assert_eq!(labels(marked), ["fourCount"]);
        let unmarked =
            json!([{ "label": "format", "kind": 3 }, { "label": "for", "kind": 14 }, { "label": "fori", "kind": 15 }]);
        assert_eq!(labels(unmarked), ["format"], "no keywords, snippets or live templates");
    }

    #[test]
    fn insert_live_template_lists_the_templates_alone() {
        let text = "package main\n\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text), ("notes.txt", "")]);
        fixture.session.set_state(fori());
        fixture.open("main.go");
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(14)));
        let list = fixture.dispatch("editor.templates.insert", json!({})).unwrap();
        assert_eq!((list["open"].as_bool(), list["pending"].as_bool()), (Some(true), Some(false)));
        assert_eq!(list["items"].as_array().map(Vec::len), Some(1));
        assert_eq!(list["items"][0]["detail"], "A loop");
        fixture.dispatch("editor.completion.accept", json!({ "index": 0 })).unwrap();
        let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert!(document.text().starts_with("package main\n\nfor  := 0;"), "{}", document.text());
        let notes = fixture.path("notes.txt");
        fixture.dispatch("file.document.open", json!({ "path": notes })).unwrap();
        assert!(fixture.dispatch("editor.templates.insert", json!({})).is_err(), "no templates for plain text");
    }

    struct LineComments;

    impl zen_syntax::Highlighter for LineComments {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<zen_syntax::Span> {
            let mut spans = Vec::new();
            let mut line_start = 0;
            for line in text.split_inclusive('\n') {
                if let Some(at) = line.find("//") {
                    let end = line_start + line.trim_end_matches('\n').len();
                    spans.push(zen_syntax::Span {
                        range: line_start + at..end,
                        kind: zen_syntax::HighlightKind::Comment,
                    });
                }
                line_start += line.len();
            }
            spans
        }
    }

    fn commented(text: &str) -> crate::language::testing::Fixture {
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        let comments: std::sync::Arc<dyn zen_syntax::Highlighter> = std::sync::Arc::new(LineComments);
        fixture.session.state_mut::<Highlighters>().set_source(std::sync::Arc::new(move |name: &str| {
            name.ends_with(".go").then(|| std::sync::Arc::clone(&comments))
        }));
        fixture.server.on("textDocument/completion", json!([]));
        fixture.open("main.go");
        fixture
    }

    fn labels_at(fixture: &mut crate::language::testing::Fixture, carets: &[usize]) -> Vec<String> {
        let selections: Vec<Value> = carets.iter().map(|caret| json!({ "head": caret })).collect();
        fixture.dispatch("editor.selection.set", json!({ "selections": selections })).unwrap();
        fixture.dispatch("editor.completion.start", json!({})).unwrap();
        let list = settled(fixture);
        list["items"]
            .as_array()
            .map(|items| items.iter().filter_map(|item| item["label"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    fn accept_first(fixture: &mut crate::language::testing::Fixture, replace: bool) {
        let index = fixture.dispatch("editor.completion.list", json!({})).unwrap()["items"][0]["index"].clone();
        fixture.dispatch("editor.completion.accept", json!({ "index": index, "replace": replace })).unwrap();
    }

    #[test]
    fn todo_words_are_offered_in_comments_only() {
        let text = "package main\n\n// to\n// fI\n// TODO\n// todo\nfunc f() { to }\nvar s = \"to\"\n";
        let mut fixture = commented(text);
        let after = |needle: &str| text.find(needle).unwrap() + needle.len();
        assert_eq!(labels_at(&mut fixture, &[after("// to")]), vec!["TODO"]);
        assert_eq!(labels_at(&mut fixture, &[after("// fI")]), vec!["FIXME"], "case-insensitive");
        assert!(labels_at(&mut fixture, &[after("// TODO")]).is_empty(), "nothing to add to a whole word");
        assert_eq!(labels_at(&mut fixture, &[after("// todo")]), vec!["TODO"]);
        assert!(labels_at(&mut fixture, &[after("{ to")]).is_empty(), "not in code");
        assert!(labels_at(&mut fixture, &[after("\"to")]).is_empty(), "not in a string");
        assert_eq!(labels_at(&mut fixture, &[after("// ")]), vec!["TODO", "FIXME"], "everything for nothing typed");
        fixture.dispatch("editor.edit.type", json!({ "text": "o" })).unwrap();
        let typed = settled(&mut fixture);
        assert!(typed["items"].as_array().is_some_and(Vec::is_empty), "o starts no word: {typed}");
    }

    #[test]
    fn the_todo_words_follow_the_window_s_patterns() {
        let text = "package main\n\n// ha\n// to\n";
        let mut fixture = commented(text);
        let hack = crate::todo_patterns::TodoPattern { pattern: r"\bhack\b.*".to_owned(), ..Default::default() };
        let settings = crate::todo_patterns::TodoSettings { patterns: vec![hack], ..Default::default() };
        fixture.session.settings_mut().set(crate::todo_patterns::SETTINGS, &settings).unwrap();
        assert_eq!(labels_at(&mut fixture, &[text.find("// ha").unwrap() + 5]), vec!["HACK"]);
        assert!(labels_at(&mut fixture, &[text.find("// to").unwrap() + 5]).is_empty(), "TODO is not a pattern now");
    }

    #[test]
    fn accepting_a_todo_word_puts_it_in_at_every_caret() {
        let text = "package main\n\n// to\n// to\nfunc f() {}\n";
        let mut fixture = commented(text);
        let first = text.find("// to").unwrap() + 5;
        let second = text.rfind("// to").unwrap() + 5;
        assert_eq!(labels_at(&mut fixture, &[first, second]), vec!["TODO"]);
        accept_first(&mut fixture, false);
        let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "package main\n\n// TODO\n// TODO\nfunc f() {}\n");
        let carets: Vec<usize> = document.selections().iter().map(|selection| selection.head).collect();
        assert_eq!(carets, vec![first + 2, second + 4], "each caret after its word");
        let text = "package main\n\n// fixm here\n";
        let mut fixture = commented(text);
        let caret = text.find("fixm").unwrap() + 3;
        assert_eq!(labels_at(&mut fixture, &[caret]), vec!["FIXME"]);
        accept_first(&mut fixture, true);
        let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "package main\n\n// FIXME here\n", "Tab replaces the rest of the word");
        let text = "package main\n\n// \n// ab\n// t\n";
        let mut fixture = commented(text);
        let carets = [text.find("// \n").unwrap() + 3, text.find("// ab").unwrap() + 5, text.find("// t").unwrap() + 4];
        assert_eq!(labels_at(&mut fixture, &carets), vec!["TODO"]);
        accept_first(&mut fixture, false);
        let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "package main\n\n// TODO\n// ab\n// TODO\n", "a caret after another word keeps it");
    }

    #[test]
    fn completion_is_asked_at_the_caret_in_utf16_and_listed() {
        let text = "package main\n\nfunc main() { ё := \"😀\"; fmt.Pri }\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        fixture.server.on("textDocument/completion", json!([{ "label": "Println", "kind": 3 }]));
        fixture.open("main.go");
        let caret = text[..text.find(" }").unwrap()].chars().count();
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
        fixture.dispatch("editor.completion.start", json!({})).unwrap();
        let sent = &fixture.server.wait_for("textDocument/completion", 1, std::time::Duration::from_secs(5))[0];
        let utf16 = "func main() { ё := \"😀\"; fmt.Pri".encode_utf16().count();
        assert_eq!(sent["position"], json!({ "line": 2, "character": utf16 }));
        assert_eq!(sent["context"], json!({ "triggerKind": 1 }));
        let mut list = json!(null);
        for _ in 0..1000 {
            list = fixture.dispatch("editor.completion.list", json!({})).unwrap();
            if list["pending"] == false {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(list["prefix"], "Pri");
        assert_eq!(list["items"][0]["label"], "Println");
    }

    #[test]
    fn an_unchanged_list_is_neither_matched_again_nor_sent_again() {
        let text = "package main\n\nfunc main() { fmt.P }\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        fixture.server.on(
            "textDocument/completion",
            json!([{ "label": "Println", "kind": 3 }, { "label": "Printf", "kind": 3 }]),
        );
        fixture.open("main.go");
        let caret = text.find(" }").unwrap();
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
        fixture.dispatch("editor.completion.start", json!({})).unwrap();
        let answered = settled(&mut fixture);
        assert_eq!(answered["items"].as_array().map(Vec::len), Some(2));
        let revision = answered["revision"].as_u64().unwrap();
        let again = fixture.dispatch("editor.completion.list", json!({})).unwrap();
        assert_eq!(again["revision"].as_u64(), Some(revision), "the same items: the same revision");
        assert_eq!(again["items"], answered["items"], "listed in full without since");
        let unchanged = fixture.dispatch("editor.completion.list", json!({ "since": revision })).unwrap();
        assert_eq!(unchanged["unchanged"], true);
        assert_eq!(unchanged["items"], json!([]), "the caller holds them already");
        assert_eq!((unchanged["open"].as_bool(), unchanged["prefix"].as_str()), (Some(true), Some("P")));
        fixture.dispatch("editor.edit.type", json!({ "text": "rintl" })).unwrap();
        let typed = fixture.dispatch("editor.completion.list", json!({ "since": revision })).unwrap();
        assert_eq!(typed["unchanged"], false, "typing changes what matches");
        assert_ne!(typed["revision"].as_u64(), Some(revision));
        assert_eq!(typed["items"].as_array().map(Vec::len), Some(1));
        assert_eq!(typed["items"][0]["label"], "Println");
    }
}

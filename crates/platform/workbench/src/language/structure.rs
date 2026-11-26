use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lsp_types::OneOf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::{Client, LspError, Pending};
use zen_syntax::{OutlineItem, Outliner, SymbolKind};

use super::{Languages, Reason, Server, ServerAbsence, ServerInstall, Synced, with_languages};
use crate::Workbench;

/// How the symbols of a structure are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StructureSort {
    /// In the order of the text.
    #[default]
    Position,
    /// Alphabetically, by name.
    Name,
}

/// Whether a structure is ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StructureStatus {
    /// The symbols are current.
    Ready,
    /// The answer has not come yet; the symbols are the last known ones.
    Pending,
    /// There is no structure; `reason` says why.
    Unavailable,
}

/// Where a structure comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StructureSource {
    /// The language server's document symbols.
    Server,
    /// The language module's outliner.
    Syntax,
}

/// A symbol of a file, with the symbols inside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    /// The symbol's name.
    pub name: String,
    /// What is shown after the name, e.g. a signature.
    pub detail: Option<String>,
    /// What the symbol is.
    pub kind: SymbolKind,
    /// The first character of the whole symbol (a character offset of the file's text).
    pub start: usize,
    /// The character after the symbol's last.
    pub end: usize,
    /// The first character of the symbol's name, where the caret goes.
    pub selection_start: usize,
    /// The line of the name, one-based.
    pub line: u32,
    /// The column of the name, one-based, in characters.
    pub column: u32,
    /// The symbols inside it, in the requested order.
    pub children: Vec<Symbol>,
}

/// The parameters of `editor.structure.get`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructureParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// How long to wait for the language server, in milliseconds; not at all when absent.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// How to order the symbols; by position when absent.
    #[serde(default)]
    pub sort: Option<StructureSort>,
    /// Whether to include fields; yes when absent.
    #[serde(default)]
    pub fields: Option<bool>,
    /// Keeps the symbols whose names have the query's letters in order, ignoring case (camel humps
    /// count), and the declarations that hold them, as the File Structure popup (⌘F12) narrows
    /// down on typing; every symbol when absent or empty.
    #[serde(default)]
    pub query: Option<String>,
    /// Whether to answer `rows`, one line of text per symbol (`line:column kind name detail`,
    /// indented two spaces per level), instead of the tree of `symbols` with offsets; false by default.
    #[serde(default)]
    pub compact: Option<bool>,
}

/// What `editor.structure.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructureState {
    /// The file.
    pub path: PathBuf,
    /// Whether the symbols are ready.
    pub status: StructureStatus,
    /// Why there is no structure, when there is none, in words.
    pub message: Option<String>,
    /// Why there are no symbols when the file's language server is not there (`serverMissing`,
    /// `serverNotRunning`, `starting`), with `server` and, when it is missing, `install`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// The server that is not there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// How to install the server, when it is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<ServerInstall>,
    /// Where the symbols come from.
    pub source: StructureSource,
    /// Raised whenever the symbols change.
    pub generation: u64,
    /// The file's top-level symbols.
    pub symbols: Vec<Symbol>,
    /// With `compact`: the symbols as rows, `line:column kind name detail`, two spaces of indent per
    /// level, and `symbols` empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<String>,
    /// The indices, level by level, of the innermost symbol at the caret; empty when none.
    pub current: Vec<usize>,
}

#[derive(Default)]
pub(crate) struct FileSymbols {
    symbols: Vec<Symbol>,
    version: Option<u64>,
    pending: Option<(u64, Pending)>,
    asked: Option<u64>,
}

#[derive(Default)]
pub struct Outliners {
    outliners: Vec<Arc<dyn Outliner>>,
    outlines: HashMap<PathBuf, (u64, usize, Vec<Symbol>)>,
    seen: HashMap<PathBuf, (StructureSource, Vec<Symbol>, u64)>,
    generation: u64,
}

impl Outliners {
    pub fn set(&mut self, outliners: Vec<Arc<dyn Outliner>>) {
        self.outliners = outliners;
        self.outlines.clear();
    }

    pub fn all(&self) -> Vec<Arc<dyn Outliner>> {
        self.outliners.clone()
    }

    pub fn for_file(&self, path: &Path) -> Option<Arc<dyn Outliner>> {
        let name = zen_vfs::file_types::known_name_at(path, None)?;
        self.outliners.iter().find(|outliner| outliner.handles(&name)).cloned()
    }

    fn generation(&mut self, path: &Path, source: StructureSource, symbols: &[Symbol]) -> u64 {
        if let Some((seen_source, seen, generation)) = self.seen.get(path)
            && *seen_source == source
            && seen == symbols
        {
            return *generation;
        }
        self.generation += 1;
        self.seen.insert(path.to_owned(), (source, symbols.to_vec(), self.generation));
        self.generation
    }
}

fn provides(client: &Client) -> bool {
    client
        .capabilities()
        .and_then(|capabilities| capabilities.document_symbol_provider.as_ref())
        .is_some_and(|provider| !matches!(provider, OneOf::Left(false)))
}

pub(super) fn step(client: &mut Client, synced: &mut Synced) {
    if !provides(client) {
        return;
    }
    settle(synced, Duration::ZERO);
    let symbols = &mut synced.symbols;
    if symbols.pending.is_none() && symbols.asked != Some(synced.version) {
        let request = client.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": synced.uri } }));
        if let Ok(pending) = request {
            symbols.pending = Some((synced.version, pending));
            symbols.asked = Some(synced.version);
        }
    }
}

fn settle(synced: &mut Synced, wait: Duration) {
    let Some((version, pending)) = &synced.symbols.pending else { return };
    let answer = if wait.is_zero() {
        pending.try_answer()
    } else {
        match pending.wait(wait) {
            Err(LspError::Timeout) => None,
            other => Some(other),
        }
    };
    let Some(answer) = answer else { return };
    let version = *version;
    synced.symbols.pending = None;
    if version == synced.version
        && let Ok(answer) = answer
    {
        synced.symbols.symbols = parse(&synced.text, &answer);
        synced.symbols.version = Some(version);
    }
}

struct Lines<'a> {
    text: &'a str,
    bytes: Vec<usize>,
    chars: Vec<usize>,
    count: usize,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let (mut bytes, mut chars, mut count) = (vec![0], vec![0], 0);
        for (byte, c) in text.char_indices() {
            count += 1;
            if c == '\n' {
                bytes.push(byte + 1);
                chars.push(count);
            }
        }
        Self { text, bytes, chars, count }
    }

    fn offset(&self, position: &Value) -> Option<usize> {
        let line = position["line"].as_u64()? as usize;
        let character = position["character"].as_u64()? as usize;
        let Some(&byte) = self.bytes.get(line) else { return Some(self.count) };
        let mut units = 0;
        let mut index = 0;
        for c in self.text[byte..].chars() {
            if units >= character || c == '\n' || c == '\r' {
                break;
            }
            units += c.len_utf16();
            index += 1;
        }
        Some(self.chars[line] + index)
    }

    fn place(&self, offset: usize) -> (u32, u32) {
        let line = self.chars.partition_point(|&start| start <= offset).saturating_sub(1);
        (line as u32 + 1, (offset - self.chars[line]) as u32 + 1)
    }

    fn range(&self, range: &Value) -> Option<(usize, usize)> {
        let start = self.offset(&range["start"])?;
        let end = self.offset(&range["end"])?.max(start);
        Some((start, end))
    }
}

pub fn symbol_kind(kind: u64) -> SymbolKind {
    match kind {
        2 => SymbolKind::Module,
        3 | 4 => SymbolKind::Package,
        5 | 10 | 26 => SymbolKind::Type,
        6 => SymbolKind::Method,
        7 | 8 | 20 => SymbolKind::Field,
        9 | 12 | 25 => SymbolKind::Function,
        11 => SymbolKind::Interface,
        13 => SymbolKind::Variable,
        14 | 22 => SymbolKind::Constant,
        23 => SymbolKind::Struct,
        _ => SymbolKind::Other,
    }
}

fn symbol(lines: &Lines, value: &Value, range: &Value, selection: Option<&Value>) -> Option<Symbol> {
    let name = value["name"].as_str()?.to_owned();
    let (start, end) = lines.range(range)?;
    let selection_start = selection.and_then(|selection| lines.range(selection)).map_or(start, |(at, _)| at);
    let (line, column) = lines.place(selection_start);
    Some(Symbol {
        name,
        detail: value["detail"].as_str().filter(|detail| !detail.is_empty()).map(str::to_owned),
        kind: symbol_kind(value["kind"].as_u64().unwrap_or(0)),
        start,
        end,
        selection_start,
        line,
        column,
        children: Vec::new(),
    })
}

fn hierarchical(lines: &Lines, value: &Value) -> Option<Symbol> {
    let mut symbol = symbol(lines, value, &value["range"], value.get("selectionRange"))?;
    if let Some(children) = value["children"].as_array() {
        symbol.children = children.iter().filter_map(|child| hierarchical(lines, child)).collect();
    }
    Some(symbol)
}

fn flat(lines: &Lines, list: &[Value]) -> Vec<Symbol> {
    let entries: Vec<(Symbol, Option<&str>)> = list
        .iter()
        .filter_map(|value| {
            let symbol = symbol(lines, value, &value["location"]["range"], None)?;
            Some((symbol, value["containerName"].as_str().filter(|name| !name.is_empty())))
        })
        .collect();
    let length = |symbol: &Symbol| symbol.end - symbol.start;
    let parents: Vec<Option<usize>> = entries
        .iter()
        .enumerate()
        .map(|(index, (child, container))| {
            let container = (*container)?;
            entries
                .iter()
                .enumerate()
                .filter(|(other, (parent, _))| {
                    *other != index
                        && parent.name == container
                        && parent.start <= child.start
                        && child.end <= parent.end
                        && (length(parent) > length(child) || *other < index)
                })
                .min_by_key(|(_, (parent, _))| length(parent))
                .map(|(other, _)| other)
        })
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    let mut roots = Vec::new();
    for (index, parent) in parents.iter().enumerate() {
        match parent {
            Some(parent) => children[*parent].push(index),
            None => roots.push(index),
        }
    }
    fn build(index: usize, entries: &[(Symbol, Option<&str>)], children: &[Vec<usize>]) -> Symbol {
        let mut symbol = entries[index].0.clone();
        symbol.children = children[index].iter().map(|&child| build(child, entries, children)).collect();
        symbol
    }
    roots.into_iter().map(|index| build(index, &entries, &children)).collect()
}

fn receiver(name: &str) -> Option<(String, String, String)> {
    let (inside, method) = name.strip_prefix('(')?.split_once(").")?;
    let named = inside.strip_prefix('*').unwrap_or(inside);
    let owner = named.split('[').next().unwrap_or_default();
    let word = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || c == '_');
    (word(owner) && word(method)).then(|| (inside.to_owned(), owner.to_owned(), method.to_owned()))
}

fn group_receivers(symbols: Vec<Symbol>) -> Vec<Symbol> {
    let mut roots = Vec::with_capacity(symbols.len());
    let mut methods = Vec::new();
    for symbol in symbols {
        match receiver(&symbol.name) {
            Some(parts) => methods.push((symbol, parts)),
            None => roots.push(symbol),
        }
    }
    for (mut symbol, (detail, owner, method)) in methods {
        let found = roots.iter_mut().find(|root| {
            root.name == owner && matches!(root.kind, SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Type)
        });
        match found {
            Some(root) => {
                symbol.name = method;
                symbol.kind = SymbolKind::Method;
                symbol.detail = Some(detail);
                root.children.push(symbol);
            }
            None => roots.push(symbol),
        }
    }
    roots
}

fn sort_by_position(symbols: &mut [Symbol]) {
    symbols.sort_by_key(|symbol| symbol.start);
    for symbol in symbols {
        sort_by_position(&mut symbol.children);
    }
}

fn sort_by_name(symbols: &mut [Symbol]) {
    symbols.sort_by_cached_key(|symbol| symbol.name.to_lowercase());
    for symbol in symbols {
        sort_by_name(&mut symbol.children);
    }
}

pub fn matching(symbols: Vec<Symbol>, query: &str) -> Vec<Symbol> {
    symbols
        .into_iter()
        .filter_map(|mut symbol| {
            symbol.children = matching(std::mem::take(&mut symbol.children), query);
            let matches = zen_index::match_positions(query, &symbol.name).is_some();
            (matches || !symbol.children.is_empty()).then_some(symbol)
        })
        .collect()
}

fn without_fields(symbols: &mut Vec<Symbol>) {
    symbols.retain(|symbol| symbol.kind != SymbolKind::Field);
    for symbol in symbols {
        without_fields(&mut symbol.children);
    }
}

pub fn parse(text: &str, answer: &Value) -> Vec<Symbol> {
    let Some(list) = answer.as_array() else { return Vec::new() };
    let lines = Lines::new(text);
    let symbols = if list.iter().any(|value| value.get("location").is_some()) {
        flat(&lines, list)
    } else {
        list.iter().filter_map(|value| hierarchical(&lines, value)).collect()
    };
    let mut symbols = group_receivers(symbols);
    sort_by_position(&mut symbols);
    symbols
}

pub fn from_outline(text: &str, items: &[OutlineItem]) -> Vec<Symbol> {
    fn convert(lines: &Lines, item: &OutlineItem) -> Symbol {
        let (line, column) = lines.place(item.selection_start);
        Symbol {
            name: item.name.clone(),
            detail: item.detail.clone(),
            kind: item.kind,
            start: item.start,
            end: item.end,
            selection_start: item.selection_start,
            line,
            column,
            children: item.children.iter().map(|child| convert(lines, child)).collect(),
        }
    }
    let lines = Lines::new(text);
    let mut symbols: Vec<Symbol> = items.iter().map(|item| convert(&lines, item)).collect();
    sort_by_position(&mut symbols);
    symbols
}

pub fn current(symbols: &[Symbol], caret: usize) -> Vec<usize> {
    fn visit(symbols: &[Symbol], caret: usize, path: &mut Vec<usize>, best: &mut Option<(usize, Vec<usize>)>) {
        for (index, symbol) in symbols.iter().enumerate() {
            path.push(index);
            if symbol.start <= caret && caret <= symbol.end {
                let length = symbol.end - symbol.start;
                let better = best.as_ref().is_none_or(|(shortest, deepest)| {
                    length < *shortest || (length == *shortest && path.len() > deepest.len())
                });
                if better {
                    *best = Some((length, path.clone()));
                }
            }
            visit(&symbol.children, caret, path, best);
            path.pop();
        }
    }
    let mut best = None;
    visit(symbols, caret, &mut Vec::new(), &mut best);
    best.map(|(_, path)| path).unwrap_or_default()
}

enum Found {
    Ready(Vec<Symbol>),
    Pending(Option<(u64, Vec<Symbol>)>),
    Missing(String),
}

fn server_symbols(languages: &mut Languages, path: &Path, open: bool, wait: Duration) -> Found {
    let Some(language) = languages.spec_for(path).map(|spec| spec.id().to_owned()) else {
        return Found::Missing(t!("workbench-structure-no-outline"));
    };
    if !open {
        return Found::Missing(t!("workbench-structure-no-outline"));
    }
    match languages.servers.get_mut(&language) {
        Some(Server::Starting(_)) => Found::Pending(None),
        Some(Server::Running { client, files }) => {
            if !provides(client) {
                return Found::Missing(t!("workbench-structure-no-outline"));
            }
            let Some(synced) = files.get_mut(path) else { return Found::Pending(None) };
            let deadline = Instant::now() + wait;
            loop {
                step(client, synced);
                if synced.symbols.version == Some(synced.version) {
                    return Found::Ready(synced.symbols.symbols.clone());
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if synced.symbols.pending.is_none() || left.is_zero() {
                    let answered = synced.symbols.version.map(|version| (version, synced.symbols.symbols.clone()));
                    return Found::Pending(answered);
                }
                settle(synced, left);
            }
        }
        _ => Found::Missing(t!("workbench-language-not-running")),
    }
}

pub struct GetStructure;

impl Command for GetStructure {
    type Params = StructureParams;
    type Output = StructureState;
    const ID: &'static str = "editor.structure.get";
    const TITLE: &'static str = "File Structure";
    const DESCRIPTION: &'static str = "The symbols of a file as a tree, from the language server or the language's outliner, \
         and the symbol at the caret; the active file by default; `compact` answers one row of text per symbol instead. Without symbols because the file's server is not \
         there, `reason` (serverMissing, serverNotRunning, starting), `server` and `install` say why.";

    fn run(&self, session: &mut Session, params: StructureParams) -> Result<StructureState, CommandError> {
        let path = target(session, params.path)?;
        let Resolved { status, source, mut symbols, reason, absence, caret, .. } =
            resolve(session, &path, super::wait(params.wait_ms));
        let generation = session.state_mut::<Outliners>().generation(&path, source, &symbols);
        if params.fields == Some(false) {
            without_fields(&mut symbols);
        }
        if params.sort == Some(StructureSort::Name) {
            sort_by_name(&mut symbols);
        }
        if let Some(query) = params.query.as_deref().map(str::trim).filter(|query| !query.is_empty()) {
            symbols = matching(symbols, query);
        }
        let current = caret.map(|caret| current(&symbols, caret)).unwrap_or_default();
        let (message, reason, server, install) = match absence {
            Some(absence) => (Some(absence.message), Some(absence.reason), Some(absence.server), absence.install),
            None => (reason, None, None, None),
        };
        let rows = if params.compact == Some(true) {
            let mut rows = Vec::new();
            compact_rows(&symbols, 0, &mut rows);
            symbols = Vec::new();
            rows
        } else {
            Vec::new()
        };
        Ok(StructureState {
            path,
            status,
            message,
            reason,
            server,
            install,
            source,
            generation,
            symbols,
            rows,
            current,
        })
    }
}

fn compact_rows(symbols: &[Symbol], depth: usize, rows: &mut Vec<String>) {
    for symbol in symbols {
        let mut what = format!("{} {}", super::compact::name_of(&symbol.kind), symbol.name);
        if let Some(detail) = symbol.detail.as_deref().filter(|detail| !detail.is_empty()) {
            what.push(' ');
            what.push_str(detail);
        }
        rows.push(format!("{}{}:{} {what}", "  ".repeat(depth), symbol.line, symbol.column));
        compact_rows(&symbol.children, depth + 1, rows);
    }
}

pub(crate) fn target(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    let workbench = session.state::<Workbench>();
    path.map(|path| workbench.map_or_else(|| path.clone(), |workbench| workbench.resolve(&path)))
        .or_else(|| {
            session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path().to_owned())
        })
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

pub(crate) struct Resolved {
    pub(crate) status: StructureStatus,
    pub(crate) source: StructureSource,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) reason: Option<String>,
    pub(crate) absence: Option<ServerAbsence>,
    pub(crate) caret: Option<usize>,
    pub(crate) read_for: Option<u64>,
}

pub(crate) fn resolve(session: &mut Session, path: &Path, wait: Duration) -> Resolved {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| (document.version(), document.selections().primary().head));
    let (found, absence) = with_languages(session, |languages, workbench| {
        languages.sync(workbench);
        let found = server_symbols(languages, path, open.is_some(), wait);
        let absence = match found {
            Found::Ready(_) | Found::Pending(Some(_)) => None,
            Found::Pending(None) | Found::Missing(_) => languages.absence_for(path),
        };
        (found, absence)
    });
    let version = open.map(|(version, _)| version);
    let read_for = match &found {
        Found::Pending(Some((read, _))) if Some(*read) != version => Some(*read),
        _ => None,
    };
    let (status, source, symbols, reason) = match found {
        Found::Ready(symbols) => (StructureStatus::Ready, StructureSource::Server, symbols, None),
        Found::Pending(Some((_, last))) => (StructureStatus::Pending, StructureSource::Server, last, None),
        Found::Pending(None) => match outline(session, path, version) {
            Some(symbols) => (StructureStatus::Pending, StructureSource::Syntax, symbols, None),
            None => (StructureStatus::Pending, StructureSource::Server, Vec::new(), None),
        },
        Found::Missing(reason) => match outline(session, path, version) {
            Some(symbols) => (StructureStatus::Ready, StructureSource::Syntax, symbols, None),
            None => (StructureStatus::Unavailable, StructureSource::Server, Vec::new(), Some(reason)),
        },
    };
    let absence = absence.filter(|_| symbols.is_empty());
    Resolved { status, source, symbols, reason, absence, caret: open.map(|(_, caret)| caret), read_for }
}

fn outline(session: &mut Session, path: &Path, version: Option<u64>) -> Option<Vec<Symbol>> {
    let outliner = session.state::<Outliners>().and_then(|outliners| outliners.for_file(path));
    let plugin = match &outliner {
        Some(_) => None,
        None => Some(crate::plugins::plugin_for(session, path, zen_syntax::Feature::Outline)?),
    };
    let maker = match (&outliner, &plugin) {
        (Some(outliner), _) => Arc::as_ptr(outliner) as *const () as usize,
        (_, Some(plugin)) => Arc::as_ptr(plugin) as *const () as usize,
        _ => return None,
    };
    let open: Vec<PathBuf> = session
        .state::<Workbench>()
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default();
    let outlines = &mut session.state_mut::<Outliners>().outlines;
    outlines.retain(|known, _| open.contains(known));
    if let Some(version) = version
        && let Some((seen, made_by, symbols)) = outlines.get(path)
        && (*seen, *made_by) == (version, maker)
    {
        return Some(symbols.clone());
    }
    let text = match version {
        Some(_) => {
            session.state::<Workbench>()?.documents().iter().find(|document| document.path() == path)?.text().to_owned()
        }
        None => std::fs::read_to_string(path).ok()?,
    };
    let items = match outliner {
        Some(outliner) => outliner.outline(&text),
        None => crate::plugins::outline(session, path, &text)?,
    };
    let symbols = from_outline(&text, &items);
    if let Some(version) = version {
        session.state_mut::<Outliners>().outlines.insert(path.to_owned(), (version, maker, symbols.clone()));
    }
    Some(symbols)
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetStructure)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn without_a_server_the_structure_is_unavailable() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        assert!(registry.dispatch(&mut session, "editor.structure.get", json!({})).is_err(), "no file");
        let params = json!({ "path": "/p/main.go", "waitMs": 0, "sort": "name", "fields": false });
        let state = registry.dispatch(&mut session, "editor.structure.get", params).unwrap();
        assert_eq!(state["status"], "unavailable");
        assert_eq!(state["source"], "server");
        assert_eq!(state["symbols"], json!([]));
        assert_eq!(state["current"], json!([]));
        assert!(state["message"].is_string());
    }

    #[test]
    fn symbols_survive_json() {
        let symbol = Symbol {
            name: "main".to_owned(),
            detail: Some("func()".to_owned()),
            kind: SymbolKind::Function,
            start: 14,
            end: 30,
            selection_start: 19,
            line: 3,
            column: 6,
            children: Vec::new(),
        };
        let json = serde_json::to_value(&symbol).unwrap();
        assert_eq!(json["kind"], "function");
        assert_eq!(json["selectionStart"], 19);
        assert_eq!(serde_json::from_value::<Symbol>(json).unwrap(), symbol);
    }

    #[test]
    fn matching_keeps_the_symbols_found_and_what_holds_them() {
        let symbol = |name: &str, kind, children| Symbol {
            name: name.to_owned(),
            detail: None,
            kind,
            start: 0,
            end: 0,
            selection_start: 0,
            line: 1,
            column: 1,
            children,
        };
        let symbols = vec![
            symbol(
                "Server",
                SymbolKind::Struct,
                vec![symbol("ListenAndServe", SymbolKind::Method, vec![]), symbol("Close", SymbolKind::Method, vec![])],
            ),
            symbol("main", SymbolKind::Function, vec![]),
        ];
        let names = |symbols: &[Symbol]| -> Vec<String> {
            let mut names = Vec::new();
            for symbol in symbols {
                names.push(symbol.name.clone());
                names.extend(symbol.children.iter().map(|child| format!("  {}", child.name)));
            }
            names
        };
        assert_eq!(names(&matching(symbols.clone(), "las")), ["Server", "  ListenAndServe"], "camel humps");
        assert_eq!(names(&matching(symbols.clone(), "MAIN")), ["main"]);
        assert_eq!(names(&matching(symbols.clone(), "serv")), ["Server", "  ListenAndServe"]);
        assert!(matching(symbols, "xyz").is_empty());
    }

    const MAIN: &str = "package main\n\n// Привет, мир\ntype Сервер struct {\n\tимя string\n}\n\nfunc (s *Сервер) Старт() {\n}\n\nfunc main() {\n}\n";

    fn range(start: (u32, u32), end: (u32, u32)) -> Value {
        json!({
            "start": { "line": start.0, "character": start.1 },
            "end": { "line": end.0, "character": end.1 }
        })
    }

    fn hierarchical_answer() -> Value {
        json!([
            {
                "name": "Сервер", "detail": "struct{...}", "kind": 23,
                "range": range((3, 0), (5, 1)), "selectionRange": range((3, 5), (3, 11)),
                "children": [{
                    "name": "имя", "detail": "string", "kind": 8,
                    "range": range((4, 1), (4, 11)), "selectionRange": range((4, 1), (4, 4))
                }]
            },
            {
                "name": "main", "detail": "func()", "kind": 12,
                "range": range((10, 0), (11, 1)), "selectionRange": range((10, 5), (10, 9))
            },
            {
                "name": "(*Сервер).Старт", "detail": "func()", "kind": 6,
                "range": range((7, 0), (8, 1)), "selectionRange": range((7, 17), (7, 22))
            }
        ])
    }

    fn names(symbols: &Value) -> Vec<String> {
        symbols.as_array().unwrap().iter().map(|symbol| symbol["name"].as_str().unwrap().to_owned()).collect()
    }

    fn char_index(text: &str, needle: &str) -> usize {
        text[..text.find(needle).unwrap()].chars().count()
    }

    fn served(reply: zen_lsp::fake::Reply) -> crate::language::testing::Fixture {
        let mut fixture = crate::language::testing::fixture(&[("main.go", MAIN)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "documentSymbolProvider": true
        }));
        fixture.server.reply("textDocument/documentSymbol", reply);
        fixture.open("main.go");
        fixture
    }

    fn set_caret(fixture: &mut crate::language::testing::Fixture, caret: usize) {
        fixture.with(|_, workbench| {
            let document = workbench.active_document_mut().unwrap();
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
        });
    }

    struct MainOutliner;

    impl Outliner for MainOutliner {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn outline(&self, text: &str) -> Vec<OutlineItem> {
            let start = char_index(text, "func main");
            vec![OutlineItem {
                name: "main".to_owned(),
                detail: None,
                kind: SymbolKind::Function,
                start,
                end: start + 14,
                selection_start: start + 5,
                children: Vec::new(),
            }]
        }
    }

    #[test]
    fn server_symbols_are_nested_sorted_and_point_at_the_caret() {
        let mut fixture = served(zen_lsp::fake::Reply::Result(hierarchical_answer()));
        set_caret(&mut fixture, char_index(MAIN, "Старт") + 2);
        let state = fixture.dispatch("editor.structure.get", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(state["status"], "ready");
        assert_eq!(state["source"], "server");
        assert!(state["message"].is_null());
        assert_eq!(names(&state["symbols"]), ["Сервер", "main"]);
        let server = &state["symbols"][0];
        assert_eq!(server["kind"], "struct");
        assert_eq!(names(&server["children"]), ["имя", "Старт"]);
        let method = &server["children"][1];
        assert_eq!(method["kind"], "method");
        assert_eq!(method["detail"], "*Сервер");
        assert_eq!((method["line"].as_u64(), method["column"].as_u64()), (Some(8), Some(18)));
        let chars: Vec<char> = MAIN.chars().collect();
        let at = method["selectionStart"].as_u64().unwrap() as usize;
        assert_eq!(chars[at..at + 5].iter().collect::<String>(), "Старт");
        let start = method["start"].as_u64().unwrap() as usize;
        let end = method["end"].as_u64().unwrap() as usize;
        assert_eq!(chars[start..end].iter().collect::<String>(), "func (s *Сервер) Старт() {\n}");
        assert_eq!(state["current"], json!([0, 1]));
        let generation = state["generation"].clone();

        let by_name = fixture.dispatch("editor.structure.get", json!({ "sort": "name" })).unwrap();
        assert_eq!(names(&by_name["symbols"]), ["main", "Сервер"]);
        assert_eq!(names(&by_name["symbols"][1]["children"]), ["имя", "Старт"]);
        assert_eq!(by_name["current"], json!([1, 1]));
        assert_eq!(by_name["generation"], generation);

        let no_fields = fixture.dispatch("editor.structure.get", json!({ "fields": false })).unwrap();
        assert_eq!(names(&no_fields["symbols"][0]["children"]), ["Старт"]);
        assert_eq!(no_fields["current"], json!([0, 0]));
        assert_eq!(fixture.server.requests("textDocument/documentSymbol").len(), 1);
    }

    #[test]
    fn flat_symbols_nest_by_container() {
        let uri = "file:///p/main.go";
        let flat = json!([
            { "name": "Сервер", "kind": 23, "location": { "uri": uri, "range": range((3, 0), (5, 1)) } },
            {
                "name": "имя", "kind": 8, "containerName": "Сервер",
                "location": { "uri": uri, "range": range((4, 1), (4, 11)) }
            },
            { "name": "(Сервер).Старт", "kind": 6, "location": { "uri": uri, "range": range((7, 0), (8, 1)) } },
            { "name": "main", "kind": 12, "location": { "uri": uri, "range": range((10, 0), (11, 1)) } },
            { "name": "(*Нет).Метод", "kind": 6, "location": { "uri": uri, "range": range((7, 0), (8, 1)) } }
        ]);
        let symbols = parse(MAIN, &flat);
        let top: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(top, ["Сервер", "(*Нет).Метод", "main"]);
        let children: Vec<&str> = symbols[0].children.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(children, ["имя", "Старт"]);
        assert_eq!(symbols[0].children[0].kind, SymbolKind::Field);
        assert_eq!(symbols[0].children[1].detail.as_deref(), Some("Сервер"));
        assert_eq!((symbols[0].line, symbols[0].column), (4, 1));
        assert!(parse(MAIN, &Value::Null).is_empty());
    }

    #[test]
    fn protocol_kinds_map_to_platform_kinds() {
        let kinds = [
            (2, SymbolKind::Module),
            (4, SymbolKind::Package),
            (5, SymbolKind::Type),
            (6, SymbolKind::Method),
            (8, SymbolKind::Field),
            (11, SymbolKind::Interface),
            (12, SymbolKind::Function),
            (13, SymbolKind::Variable),
            (14, SymbolKind::Constant),
            (23, SymbolKind::Struct),
            (26, SymbolKind::Type),
            (99, SymbolKind::Other),
        ];
        for (number, kind) in kinds {
            assert_eq!(symbol_kind(number), kind, "kind {number}");
        }
    }

    #[test]
    fn an_answer_about_an_older_text_is_dropped() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let handler: zen_lsp::fake::Handler = Arc::new(move |_| {
            if counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                std::thread::sleep(Duration::from_millis(300));
                json!([{ "name": "Old", "kind": 12, "range": range((10, 0), (11, 1)), "selectionRange": range((10, 5), (10, 9)) }])
            } else {
                json!([{ "name": "New", "kind": 12, "range": range((10, 0), (11, 1)), "selectionRange": range((10, 5), (10, 9)) }])
            }
        });
        let mut fixture = served(zen_lsp::fake::Reply::With(handler));
        let end = MAIN.chars().count();
        fixture.with(|_, workbench| {
            workbench.active_document_mut().unwrap().replace_ranges(vec![(end..end, "\n".to_owned())]);
        });
        let state = fixture.dispatch("editor.structure.get", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(state["status"], "ready");
        assert_eq!(names(&state["symbols"]), ["New"]);
        assert_eq!(fixture.server.requests("textDocument/documentSymbol").len(), 2);
    }

    #[test]
    fn while_the_server_thinks_the_outliner_answers() {
        let mut fixture = served(zen_lsp::fake::Reply::Hold);
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(MainOutliner)]);
        let state = fixture.dispatch("editor.structure.get", json!({})).unwrap();
        assert_eq!(state["status"], "pending");
        assert_eq!(state["source"], "syntax");
        assert_eq!(names(&state["symbols"]), ["main"]);
        assert_eq!((state["symbols"][0]["line"].as_u64(), state["symbols"][0]["column"].as_u64()), (Some(11), Some(6)));
        let generation = state["generation"].as_u64().unwrap();
        fixture.session.state_mut::<Outliners>().set(Vec::new());
        let state = fixture.dispatch("editor.structure.get", json!({})).unwrap();
        assert_eq!(state["status"], "pending");
        assert_eq!(state["source"], "server");
        assert_eq!(state["symbols"], json!([]));
        assert!(state["generation"].as_u64().unwrap() > generation);
    }

    #[test]
    fn after_an_edit_the_last_server_symbols_stay_until_the_new_answer() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let handler: zen_lsp::fake::Handler = Arc::new(move |_| {
            if counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
                std::thread::sleep(Duration::from_millis(300));
            }
            hierarchical_answer()
        });
        let mut fixture = served(zen_lsp::fake::Reply::With(handler));
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(MainOutliner)]);
        let ready = fixture.dispatch("editor.structure.get", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!((ready["status"].as_str(), ready["source"].as_str()), (Some("ready"), Some("server")));
        let end = MAIN.chars().count();
        fixture.with(|_, workbench| {
            workbench.active_document_mut().unwrap().replace_ranges(vec![(end..end, "\n".to_owned())]);
        });
        let stale = fixture.dispatch("editor.structure.get", json!({ "waitMs": 0 })).unwrap();
        assert_eq!((stale["status"].as_str(), stale["source"].as_str()), (Some("pending"), Some("server")));
        assert_eq!(names(&stale["symbols"]), ["Сервер", "main"]);
        assert_eq!(stale["generation"], ready["generation"]);
        let fresh = fixture.dispatch("editor.structure.get", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!((fresh["status"].as_str(), fresh["source"].as_str()), (Some("ready"), Some("server")));
        assert_eq!(fixture.server.requests("textDocument/documentSymbol").len(), 2);
    }

    #[test]
    fn wait_ms_waits_for_the_server() {
        let answer = hierarchical_answer();
        let mut fixture = served(zen_lsp::fake::Reply::Delayed(answer, Duration::from_millis(300)));
        let state = fixture.dispatch("editor.structure.get", json!({ "waitMs": 0 })).unwrap();
        assert_eq!(state["status"], "pending");
        let state = fixture.dispatch("editor.structure.get", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(state["status"], "ready");
        assert_eq!(state["source"], "server");
        assert_eq!(names(&state["symbols"]), ["Сервер", "main"]);
    }

    #[test]
    fn without_document_symbols_or_a_server_the_outliner_is_used() {
        let mut fixture = crate::language::testing::fixture(&[("main.go", MAIN)]);
        fixture.open("main.go");
        let state = fixture.dispatch("editor.structure.get", json!({ "waitMs": 0 })).unwrap();
        assert_eq!(state["status"], "unavailable");
        assert_eq!(state["message"], t!("workbench-structure-no-outline"));
        assert!(fixture.server.requests("textDocument/documentSymbol").is_empty());
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(MainOutliner)]);
        let state = fixture.dispatch("editor.structure.get", json!({})).unwrap();
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("syntax")));

        let mut fixture = crate::language::testing::fixture(&[("main.go", MAIN)]);
        fixture.session.state_mut::<Languages>().set_specs(vec![crate::language::ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(|_| Err("not installed".to_owned())),
            initialization_options: json!({}),
        }]);
        let path = fixture.path("main.go");
        fixture.dispatch("file.document.open", json!({ "path": path })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while fixture.dispatch("language.servers.sync", json!({})).unwrap()["servers"]["go"]["state"] != "failed" {
            assert!(Instant::now() < deadline, "the server did not fail");
            std::thread::sleep(Duration::from_millis(5));
        }
        let state = fixture.dispatch("editor.structure.get", json!({})).unwrap();
        assert_eq!(state["status"], "unavailable");
        assert_eq!(state["message"], t!("workbench-language-server-failed", server = "go", reason = "not installed"));
        assert_eq!((&state["reason"], &state["server"]), (&json!("serverNotRunning"), &json!("go")));
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(MainOutliner)]);
        let state = fixture.dispatch("editor.structure.get", json!({})).unwrap();
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("syntax")));
        assert_eq!(state["current"], json!([]));
    }

    #[test]
    fn the_outlines_of_open_files_are_kept_one_per_file_and_dropped_once_closed() {
        let mut fixture = crate::language::testing::fixture(&[("a.go", MAIN), ("b.go", MAIN)]);
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(MainOutliner)]);
        fixture.open("a.go");
        fixture.open("b.go");
        let (a, b) = (fixture.path("a.go"), fixture.path("b.go"));
        let kept = |fixture: &crate::language::testing::Fixture| {
            let mut paths: Vec<PathBuf> =
                fixture.session.state::<Outliners>().unwrap().outlines.keys().cloned().collect();
            paths.sort();
            paths
        };
        fixture.dispatch("editor.structure.get", json!({ "path": a })).unwrap();
        fixture.dispatch("editor.structure.get", json!({ "path": b })).unwrap();
        assert_eq!(kept(&fixture), [a.clone(), b.clone()]);
        fixture.with(|_, workbench| {
            workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "\n".to_owned())]);
        });
        fixture.dispatch("editor.structure.get", json!({ "path": b })).unwrap();
        assert_eq!(kept(&fixture), [a.clone(), b.clone()], "a new version replaces the old one");
        fixture.dispatch("file.document.close", json!({ "index": 0 })).unwrap();
        fixture.dispatch("editor.structure.get", json!({ "path": b })).unwrap();
        assert_eq!(kept(&fixture), [b], "a closed file's outline is dropped");
    }
}

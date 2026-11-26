use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session, Settings};
use zen_i18n::t;
use zen_lsp::{Client, DocumentRequest, Pending};
use zen_syntax::SymbolKind;

use super::structure::{GetStructure, StructureParams, StructureStatus, Symbol};
use super::{Languages, Server, with_languages};
use crate::Workbench;
use crate::vcs::log::{BlameParams, GetBlame, LineAnnotation};

pub const ENABLED: &str = "editor.codeVision.enabled";
pub const USAGES: &str = "editor.codeVision.usages";
pub const AUTHORS: &str = "editor.codeVision.authors";
pub const AUTHORS_BY_DEFAULT: bool = false;
pub const MAX_IN_FLIGHT: usize = 4;
pub const MAX_LINES: usize = 500;
const BLAME_CHECK: Duration = Duration::from_secs(2);
const LENS_WAIT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switches {
    pub usages: bool,
    pub authors: bool,
}

impl Switches {
    pub fn from_settings(settings: &Settings) -> Self {
        let on = |key: &str, default: bool| settings.get::<bool>(key).unwrap_or(default);
        let enabled = on(ENABLED, true);
        Self { usages: enabled && on(USAGES, true), authors: enabled && on(AUTHORS, AUTHORS_BY_DEFAULT) }
    }

    pub fn any(self) -> bool {
        self.usages || self.authors
    }
}

/// The parameters of `editor.codeVision.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeVisionParams {
    /// The file, open in the editor; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The first line (from 0) to compute lenses for; the file's first when absent.
    #[serde(default)]
    pub from_line: Option<usize>,
    /// The last line (from 0) to compute lenses for; at most 500 lines after the first.
    #[serde(default)]
    pub to_line: Option<usize>,
    /// How long to wait for the lenses of the lines in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// The authors of a declaration, from Git blame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Authors {
    /// The author of the latest change among its lines.
    pub name: String,
    /// How many other authors changed its lines.
    pub others: usize,
    /// The latest commit among its lines.
    pub commit: String,
    /// When that commit was authored, in seconds since 1970.
    pub time: i64,
}

/// The lens above one declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Lens {
    /// The line of the declaration's name, from 0; the lens is drawn above it.
    pub line: usize,
    /// The declaration's name.
    pub name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// The character offset of its name, where Find Usages looks.
    pub offset: usize,
    /// How many usages it has, once counted (while recounted, the last count).
    pub usages: Option<usize>,
    /// Who changed it, once known.
    pub authors: Option<Authors>,
}

/// What `editor.codeVision.list` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeVisionList {
    /// The file.
    pub path: PathBuf,
    /// The version of the text the lenses are for.
    pub version: u64,
    /// Whether something asked for is still being computed.
    pub pending: bool,
    /// Every line of the file with a lens above it, from 0, in order; empty while not known.
    pub lines: Vec<usize>,
    /// The lenses of the lines asked for, in order.
    pub lenses: Vec<Lens>,
}

#[derive(Clone, Debug)]
struct Declaration {
    key: String,
    name: String,
    kind: SymbolKind,
    offset: usize,
    line: usize,
    first_line: usize,
    last_line: usize,
}

struct ServerLens {
    line: usize,
    lens: Value,
    count: Option<usize>,
    resolving: Option<Pending>,
    done: bool,
}

#[derive(Default)]
enum Lenses {
    #[default]
    Unknown,
    Asked(Pending, Instant),
    Answered(Vec<ServerLens>),
    Missing,
}

#[derive(Default)]
struct FileVision {
    version: u64,
    settled: u64,
    declarations: Option<Vec<Declaration>>,
    usages: HashMap<usize, Option<usize>>,
    in_flight: Vec<(usize, DocumentRequest)>,
    lenses: Lenses,
    last_usages: HashMap<String, usize>,
    blame: Option<Vec<Option<LineAnnotation>>>,
    blame_stamp: Option<u64>,
    blame_checked: Option<Instant>,
    last_authors: HashMap<String, Authors>,
}

impl FileVision {
    fn advance(&mut self, version: u64) -> Vec<Pending> {
        let mut dropped: Vec<Pending> = self.in_flight.drain(..).map(|(_, request)| request.pending).collect();
        if let Lenses::Asked(pending, _) = std::mem::take(&mut self.lenses) {
            dropped.push(pending);
        }
        if let Some(declarations) = &self.declarations {
            for declaration in declarations {
                if let Some(Some(count)) = self.usages.get(&declaration.offset) {
                    self.last_usages.insert(declaration.key.clone(), *count);
                }
                if let Some(authors) = self.blame.as_deref().and_then(|lines| authors(lines, declaration)) {
                    self.last_authors.insert(declaration.key.clone(), authors);
                }
            }
        }
        *self = FileVision {
            version,
            last_usages: std::mem::take(&mut self.last_usages),
            last_authors: std::mem::take(&mut self.last_authors),
            blame_stamp: self.blame_stamp,
            ..FileVision::default()
        };
        dropped
    }
}

#[derive(Default)]
pub struct CodeVision {
    files: HashMap<PathBuf, FileVision>,
}

fn top_level(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Struct
            | SymbolKind::Interface
            | SymbolKind::Type
            | SymbolKind::Class
            | SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constant
            | SymbolKind::Variable
    )
}

fn member(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Method | SymbolKind::Function)
}

fn declarations(symbols: &[Symbol], snapshot: &zen_text::Text) -> Vec<Declaration> {
    let line_of = |offset: usize| snapshot.offset_to_point(offset).map_or(0, |point| point.line);
    let declaration = |symbol: &Symbol, key: String| Declaration {
        key,
        name: symbol.name.clone(),
        kind: symbol.kind,
        offset: symbol.selection_start,
        line: symbol.line.saturating_sub(1) as usize,
        first_line: line_of(symbol.start),
        last_line: line_of(symbol.end),
    };
    let mut found = Vec::new();
    for symbol in symbols.iter().filter(|symbol| top_level(symbol.kind)) {
        found.push(declaration(symbol, symbol.name.clone()));
        for child in symbol.children.iter().filter(|child| member(child.kind)) {
            found.push(declaration(child, format!("{}.{}", symbol.name, child.name)));
        }
    }
    found.sort_by_key(|declaration| declaration.offset);
    let mut lines = HashSet::new();
    found.retain(|declaration| lines.insert(declaration.line));
    found
}

pub fn usage_count(title: &str) -> Option<usize> {
    let lower = title.to_lowercase();
    if !(lower.contains("usage") || lower.contains("reference")) {
        return None;
    }
    let first = lower.split_whitespace().next()?;
    if first == "no" { Some(0) } else { first.parse().ok() }
}

fn reference_count(answer: &Value) -> usize {
    let Some(list) = answer.as_array() else { return 0 };
    let places: HashSet<(String, u64, u64)> = list
        .iter()
        .filter_map(|location| {
            let start = &location["range"]["start"];
            Some((location["uri"].as_str()?.to_owned(), start["line"].as_u64()?, start["character"].as_u64()?))
        })
        .collect();
    places.len()
}

fn authors(lines: &[Option<LineAnnotation>], declaration: &Declaration) -> Option<Authors> {
    let last = declaration.last_line.min(lines.len().saturating_sub(1));
    let annotations = lines.get(declaration.first_line..=last).unwrap_or_default().iter().flatten();
    let latest = annotations.clone().max_by_key(|annotation| annotation.time)?;
    let others: HashSet<&str> =
        annotations.map(|annotation| annotation.author.as_str()).filter(|author| *author != latest.author).collect();
    Some(Authors {
        name: latest.author.clone(),
        others: others.len(),
        commit: latest.commit.clone(),
        time: latest.time,
    })
}

fn server_lenses(answer: &Value) -> Vec<ServerLens> {
    answer
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|lens| {
                    let line = lens["range"]["start"]["line"].as_u64()? as usize;
                    let title = lens["command"]["title"].as_str();
                    Some(ServerLens {
                        line,
                        lens: lens.clone(),
                        count: title.and_then(usage_count),
                        resolving: None,
                        done: title.is_some(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lens_provider(client: &Client) -> Option<bool> {
    let options = client.capabilities()?.code_lens_provider.as_ref()?;
    Some(options.resolve_provider.unwrap_or(false))
}

enum Reach {
    Ready,
    Later,
    Never,
}

fn reach(languages: &Languages, path: &Path, version: u64) -> Reach {
    let Some(spec) = languages.spec_for(path) else { return Reach::Never };
    match languages.servers.get(spec.id()) {
        Some(Server::Running { client, files }) if client.is_running() => match files.get(path) {
            Some(synced) if synced.version == version => Reach::Ready,
            _ => Reach::Later,
        },
        Some(Server::Running { .. } | Server::Failed { .. } | Server::Stopped) => Reach::Never,
        Some(Server::Starting(_)) | None => Reach::Later,
    }
}

fn step_usages(
    languages: &mut Languages,
    workbench: &Workbench,
    path: &Path,
    file: &mut FileVision,
    wanted: &[Declaration],
    snapshot: &zen_text::Text,
) -> bool {
    languages.sync(workbench);
    match reach(languages, path, file.version) {
        Reach::Never => return false,
        Reach::Later => return true,
        Reach::Ready => {}
    }
    let Some((client, synced)) = languages.server_for(path) else { return true };
    let uri = synced.uri.clone();
    let shown: HashSet<usize> = wanted.iter().map(|declaration| declaration.offset).collect();
    let lines: HashSet<usize> = wanted.iter().map(|declaration| declaration.line).collect();
    let mut answered = Vec::new();
    file.in_flight.retain(|(offset, request)| match request.pending.try_answer() {
        Some(answer) => {
            answered.push((*offset, answer.ok().map(|answer| reference_count(&answer))));
            false
        }
        None if shown.contains(offset) => true,
        None => {
            client.cancel(&request.pending);
            false
        }
    });
    file.usages.extend(answered);
    let provider = lens_provider(client);
    if provider.is_none() {
        file.lenses = Lenses::Missing;
    }
    if matches!(file.lenses, Lenses::Unknown) {
        file.lenses = match client.request("textDocument/codeLens", json!({ "textDocument": { "uri": uri } })) {
            Ok(pending) => Lenses::Asked(pending, Instant::now()),
            Err(_) => Lenses::Missing,
        };
    }
    if let Lenses::Asked(pending, since) = &file.lenses {
        if let Some(answer) = pending.try_answer() {
            file.lenses = answer.map_or(Lenses::Missing, |answer| Lenses::Answered(server_lenses(&answer)));
        } else if since.elapsed() > LENS_WAIT {
            client.cancel(pending);
            file.lenses = Lenses::Missing;
        }
    }
    let resolves = provider == Some(true);
    if let Lenses::Answered(lenses) = &mut file.lenses {
        for lens in lenses.iter_mut() {
            let Some(pending) = &lens.resolving else { continue };
            if let Some(answer) = pending.try_answer() {
                lens.resolving = None;
                lens.done = true;
                lens.count = answer.ok().and_then(|answer| answer["command"]["title"].as_str().and_then(usage_count));
            } else if !lines.contains(&lens.line) {
                client.cancel(pending);
                lens.resolving = None;
            }
        }
    }
    let mut pending = !file.in_flight.is_empty();
    let mut busy_requests = file.in_flight.len()
        + match &file.lenses {
            Lenses::Answered(lenses) => lenses.iter().filter(|lens| lens.resolving.is_some()).count(),
            _ => 0,
        };
    for declaration in wanted {
        if file.usages.contains_key(&declaration.offset) {
            continue;
        }
        let from_lenses = match &mut file.lenses {
            Lenses::Unknown | Lenses::Asked(..) => {
                pending = true;
                continue;
            }
            Lenses::Missing => None,
            Lenses::Answered(lenses) => {
                let mut verdict = None;
                let mut busy = false;
                for lens in lenses.iter_mut().filter(|lens| lens.line == declaration.line) {
                    if let Some(count) = lens.count {
                        verdict = Some(count);
                        break;
                    }
                    if !lens.done && resolves {
                        busy = true;
                        if lens.resolving.is_none()
                            && busy_requests < MAX_IN_FLIGHT
                            && let Ok(asked) = client.request("codeLens/resolve", lens.lens.clone())
                        {
                            lens.resolving = Some(asked);
                            busy_requests += 1;
                        }
                    }
                }
                if busy && verdict.is_none() {
                    pending = true;
                    continue;
                }
                verdict
            }
        };
        if let Some(count) = from_lenses {
            file.usages.insert(declaration.offset, Some(count));
            continue;
        }
        if file.in_flight.iter().any(|(offset, _)| *offset == declaration.offset) {
            pending = true;
            continue;
        }
        if busy_requests >= MAX_IN_FLIGHT {
            pending = true;
            continue;
        }
        let Ok((line, character)) = snapshot.offset_to_utf16(declaration.offset) else {
            file.usages.insert(declaration.offset, None);
            continue;
        };
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": false },
        });
        match client.document_request(&uri, "textDocument/references", params) {
            Ok(request) => {
                file.in_flight.push((declaration.offset, request));
                busy_requests += 1;
                pending = true;
            }
            Err(_) => {
                file.usages.insert(declaration.offset, None);
            }
        }
    }
    pending
}

fn step_blame(session: &mut Session, path: &Path, file: &mut FileVision, wait: bool) -> bool {
    let checked = file.blame_checked.is_some_and(|at| at.elapsed() < BLAME_CHECK);
    if file.blame.is_some() && checked {
        return false;
    }
    let Ok(repository) = crate::vcs::repository(session) else {
        file.blame = Some(Vec::new());
        file.blame_checked = Some(Instant::now());
        return false;
    };
    let stamp = repository.stamp();
    file.blame_checked = Some(Instant::now());
    if file.blame.is_some() && file.blame_stamp == Some(stamp) {
        return false;
    }
    let params = BlameParams { path: Some(path.to_owned()), force: true, wait };
    match GetBlame.run(session, params) {
        Ok(info) if info.pending => {
            file.blame_checked = None;
            true
        }
        Ok(info) => {
            file.blame = Some(info.lines);
            file.blame_stamp = Some(stamp);
            false
        }
        Err(_) => {
            file.blame = Some(Vec::new());
            file.blame_stamp = Some(stamp);
            false
        }
    }
}

fn cancel(session: &mut Session, path: &Path, dropped: Vec<Pending>) {
    if dropped.is_empty() {
        return;
    }
    if let Some(client) = session.state_mut::<Languages>().client_for(path) {
        for pending in &dropped {
            client.cancel(pending);
        }
    }
}

fn list(
    session: &mut Session,
    vision: &mut CodeVision,
    path: &Path,
    lines: std::ops::RangeInclusive<usize>,
    wait: bool,
) -> Result<CodeVisionList, CommandError> {
    let switches = Switches::from_settings(session.settings());
    let open: HashSet<PathBuf> = session
        .state::<Workbench>()
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default();
    let closed: Vec<PathBuf> = vision.files.keys().filter(|file| !open.contains(*file)).cloned().collect();
    for closed in closed {
        if let Some(mut file) = vision.files.remove(&closed) {
            let dropped = file.advance(0);
            cancel(session, &closed, dropped);
        }
    }
    let (version, snapshot) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .filter(|document| document.image().is_none())
        .map(|document| (document.version(), document.snapshot().clone()))
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let mut answer = CodeVisionList { path: path.to_owned(), version, ..CodeVisionList::default() };
    if !switches.any() {
        if let Some(mut file) = vision.files.remove(path) {
            let dropped = file.advance(0);
            cancel(session, path, dropped);
        }
        return Ok(answer);
    }
    let settled = session.state::<Languages>().map_or(0, Languages::settled);
    let file = vision.files.entry(path.to_owned()).or_default();
    if file.version != version || file.settled != settled {
        let dropped = file.advance(version);
        file.settled = settled;
        cancel(session, path, dropped);
    }
    if file.declarations.is_none() {
        let params = StructureParams { path: Some(path.to_owned()), ..StructureParams::default() };
        match GetStructure.run(session, params) {
            Ok(structure) if structure.status == StructureStatus::Ready => {
                file.declarations = Some(declarations(&structure.symbols, &snapshot));
            }
            Ok(structure) if structure.status == StructureStatus::Unavailable => file.declarations = Some(Vec::new()),
            Ok(_) => {}
            Err(error) => return Err(error),
        }
    }
    let Some(all) = file.declarations.as_deref() else {
        answer.pending = true;
        return Ok(answer);
    };
    answer.lines = all.iter().map(|declaration| declaration.line).collect();
    let from = all.partition_point(|declaration| declaration.line < *lines.start());
    let to = all.partition_point(|declaration| declaration.line <= *lines.end());
    let wanted: Vec<Declaration> = all[from..to].to_vec();
    if wanted.is_empty() {
        return Ok(answer);
    }
    if switches.usages {
        let usages_pending = with_languages(session, |languages, workbench| {
            step_usages(languages, workbench, path, file, &wanted, &snapshot)
        });
        answer.pending |= usages_pending;
    }
    if switches.authors {
        answer.pending |= step_blame(session, path, file, wait);
    }
    answer.lenses = wanted
        .iter()
        .map(|declaration| Lens {
            line: declaration.line,
            name: declaration.name.clone(),
            kind: declaration.kind,
            offset: declaration.offset,
            usages: switches
                .usages
                .then(|| match file.usages.get(&declaration.offset) {
                    Some(count) => *count,
                    None => file.last_usages.get(&declaration.key).copied(),
                })
                .flatten(),
            authors: switches
                .authors
                .then(|| match &file.blame {
                    Some(lines) => authors(lines, declaration),
                    None => file.last_authors.get(&declaration.key).cloned(),
                })
                .flatten(),
        })
        .collect();
    Ok(answer)
}

pub struct ListCodeVision;

impl Command for ListCodeVision {
    type Params = CodeVisionParams;
    type Output = CodeVisionList;
    const ID: &'static str = "editor.codeVision.list";
    const TITLE: &'static str = "Code Vision";
    const DESCRIPTION: &'static str = "The lenses above the declarations of an open file (the active one by default): how many \
         usages each has, from the language server, and who changed it last, from Git blame; computed for the lines asked for \
         only, at most 500, and kept until the text changes.";

    fn run(&self, session: &mut Session, params: CodeVisionParams) -> Result<CodeVisionList, CommandError> {
        let path = params
            .path
            .clone()
            .or_else(|| {
                session
                    .state::<Workbench>()
                    .and_then(Workbench::active_document)
                    .map(|document| document.path().to_owned())
            })
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let from = params.from_line.unwrap_or(0);
        let to = params.to_line.unwrap_or(usize::MAX).max(from).min(from + MAX_LINES - 1);
        let deadline = Instant::now() + super::wait(params.wait_ms);
        let waits = params.wait_ms.is_some_and(|wait| wait > 0);
        let mut vision = std::mem::take(session.state_mut::<CodeVision>());
        let answer = loop {
            let answer = list(session, &mut vision, &path, from..=to, waits);
            match answer {
                Ok(answer) if answer.pending && Instant::now() < deadline => std::thread::sleep(POLL),
                other => break other,
            }
        };
        session.set_state(vision);
        answer
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListCodeVision)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use zen_lsp::fake::Reply;

    use super::*;
    use crate::language::testing::{Fixture, fixture};

    const MAIN: &str = "package main\n\n// Greet says hello.\nfunc Greet() {\n\tprintln(\"hi\")\n}\n\ntype Box struct{}\n\nfunc (b Box) Open() {}\n\nfunc main() {\n\tGreet()\n\tGreet()\n}\n";

    fn range(from: (u64, u64), to: (u64, u64)) -> Value {
        json!({ "start": { "line": from.0, "character": from.1 }, "end": { "line": to.0, "character": to.1 } })
    }

    fn symbol(name: &str, kind: u64, lines: (u64, u64), name_at: (u64, u64)) -> Value {
        json!({
            "name": name,
            "kind": kind,
            "range": range((lines.0, 0), (lines.1, 1)),
            "selectionRange": range(name_at, (name_at.0, name_at.1 + name.len() as u64)),
        })
    }

    fn symbols() -> Value {
        json!([
            symbol("Greet", 12, (3, 5), (3, 5)),
            symbol("Box", 23, (7, 7), (7, 5)),
            symbol("(Box).Open", 6, (9, 9), (9, 13)),
            symbol("main", 12, (11, 14), (11, 5)),
        ])
    }

    fn served(capabilities: Value) -> Fixture {
        let fixture = fixture(&[("main.go", MAIN)]);
        let mut all = json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "referencesProvider": true,
            "documentSymbolProvider": true,
        });
        if let (Some(all), Value::Object(more)) = (all.as_object_mut(), capabilities) {
            all.extend(more);
        }
        fixture.server.set_capabilities(all);
        fixture.server.on("textDocument/documentSymbol", symbols());
        fixture
    }

    fn references_by_line(fixture: &Fixture) {
        let uri = zen_lsp::position::file_uri(&fixture.path("main.go"));
        fixture.server.on_with("textDocument/references", move |params| {
            let at = |line: u64| json!({ "uri": uri, "range": range((line, 1), (line, 6)) });
            match params["position"]["line"].as_u64() {
                Some(3) => json!([at(12), at(13), at(13)]),
                Some(7) => json!([at(9)]),
                _ => json!([]),
            }
        });
    }

    fn list(fixture: &mut Fixture, params: Value) -> CodeVisionList {
        serde_json::from_value(fixture.dispatch("editor.codeVision.list", params).unwrap()).unwrap()
    }

    fn usages(list: &CodeVisionList) -> Vec<(usize, &str, Option<usize>)> {
        list.lenses.iter().map(|lens| (lens.line, lens.name.as_str(), lens.usages)).collect()
    }

    #[test]
    fn usages_are_counted_for_the_lines_asked_for_and_kept_until_the_text_changes() {
        let mut fixture = served(json!({}));
        references_by_line(&fixture);
        fixture.open("main.go");
        let some = list(&mut fixture, json!({ "fromLine": 0, "toLine": 4, "waitMs": 5000 }));
        assert_eq!(some.lines, [3, 7, 9, 11], "every declaration's line, for the layout");
        assert_eq!(usages(&some), [(3, "Greet", Some(2))], "the duplicate place counts once");
        assert_eq!(fixture.server.requests("textDocument/references").len(), 1, "only the lines asked for");
        let sent = &fixture.server.requests("textDocument/references")[0];
        assert_eq!(sent["context"]["includeDeclaration"], false);
        assert_eq!(sent["position"], json!({ "line": 3, "character": 5 }));

        let all = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert!(!all.pending);
        assert_eq!(
            usages(&all),
            [(3, "Greet", Some(2)), (7, "Box", Some(1)), (9, "Open", Some(0)), (11, "main", Some(0))]
        );
        assert_eq!(all.lenses[2].kind, SymbolKind::Method);
        assert_eq!(fixture.server.requests("textDocument/references").len(), 4);
        let again = list(&mut fixture, json!({}));
        assert_eq!(again.lenses, all.lenses);
        assert_eq!(fixture.server.requests("textDocument/references").len(), 4, "answered from the cache");

        let end = MAIN.chars().count();
        let workbench = fixture.session.state_mut::<Workbench>();
        workbench.active_document_mut().unwrap().replace_ranges(vec![(end..end, "\n".to_owned())]);
        fixture.server.reply("textDocument/references", Reply::Hold);
        let typed = list(&mut fixture, json!({}));
        assert!(typed.pending);
        assert_ne!(typed.version, all.version);
        references_by_line(&fixture);
        let counted = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert_eq!(usages(&counted), usages(&all));
        assert!(fixture.server.requests("textDocument/references").len() > 4, "counted again for the new text");
    }

    #[test]
    fn the_last_counts_stay_while_the_new_text_is_counted() {
        let mut fixture = served(json!({}));
        references_by_line(&fixture);
        fixture.open("main.go");
        let first = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert_eq!(first.lenses[0].usages, Some(2));
        fixture.server.reply("textDocument/references", Reply::Hold);
        let end = MAIN.chars().count();
        let workbench = fixture.session.state_mut::<Workbench>();
        workbench.active_document_mut().unwrap().replace_ranges(vec![(end..end, "\n".to_owned())]);
        let deadline = Instant::now() + Duration::from_secs(60);
        let typed = loop {
            let typed = list(&mut fixture, json!({}));
            if !typed.lenses.is_empty() || Instant::now() > deadline {
                break typed;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(typed.pending);
        assert_eq!(usages(&typed), usages(&first), "the counts of the text before");
    }

    #[test]
    fn at_most_a_few_requests_are_on_their_way_at_once() {
        let text: String = (0..8).map(|index| format!("func F{index}() {{}}\n")).collect();
        let fixture = fixture(&[("many.go", &text)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "referencesProvider": true,
            "documentSymbolProvider": true,
        }));
        let found: Vec<Value> =
            (0..8u64).map(|line| symbol(&format!("F{line}"), 12, (line, line), (line, 5))).collect();
        fixture.server.on("textDocument/documentSymbol", json!(found));
        fixture.server.reply("textDocument/references", Reply::Hold);
        let mut fixture = fixture;
        fixture.open("many.go");
        let path = fixture.path("many.go");
        let listed = list(&mut fixture, json!({ "path": path, "waitMs": 300 }));
        assert!(listed.pending);
        assert_eq!(listed.lines.len(), 8);
        assert!(listed.lenses.iter().all(|lens| lens.usages.is_none()));
        fixture.server.wait_for("textDocument/references", MAX_IN_FLIGHT, Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(fixture.server.requests("textDocument/references").len(), MAX_IN_FLIGHT);
    }

    #[test]
    fn requests_for_declarations_scrolled_away_are_cancelled() {
        let text: String = (0..8).map(|index| format!("func F{index}() {{}}\n")).collect();
        let fixture = fixture(&[("many.go", &text)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "referencesProvider": true,
            "documentSymbolProvider": true,
        }));
        let found: Vec<Value> =
            (0..8u64).map(|line| symbol(&format!("F{line}"), 12, (line, line), (line, 5))).collect();
        fixture.server.on("textDocument/documentSymbol", json!(found));
        fixture.server.reply("textDocument/references", Reply::Hold);
        let mut fixture = fixture;
        fixture.open("many.go");
        let path = fixture.path("many.go");
        list(&mut fixture, json!({ "path": path, "fromLine": 0, "toLine": 1, "waitMs": 300 }));
        fixture.server.wait_for("textDocument/references", 2, Duration::from_secs(5));
        list(&mut fixture, json!({ "path": path, "fromLine": 6, "toLine": 7, "waitMs": 300 }));
        let cancelled = fixture.server.wait_for("$/cancelRequest", 2, Duration::from_secs(5));
        assert_eq!(cancelled.len(), 2, "the requests for the lines left are cancelled");
        let lines: Vec<u64> = fixture
            .server
            .wait_for("textDocument/references", 4, Duration::from_secs(5))
            .iter()
            .filter_map(|request| request["position"]["line"].as_u64())
            .collect();
        assert_eq!(lines, [0, 1, 6, 7]);
    }

    #[test]
    fn a_server_with_usage_lenses_gives_the_counts_and_references_fill_the_rest() {
        let mut fixture = served(json!({ "codeLensProvider": { "resolveProvider": true } }));
        references_by_line(&fixture);
        fixture.server.on(
            "textDocument/codeLens",
            json!([
                { "range": range((3, 5), (3, 10)), "command": { "title": "5 references", "command": "x" } },
                { "range": range((7, 5), (7, 8)), "data": { "id": 7 } },
                { "range": range((11, 5), (11, 9)), "command": { "title": "▶ Run", "command": "run" } },
            ]),
        );
        fixture.server.on_with("codeLens/resolve", |lens| {
            let mut lens = lens.clone();
            lens["command"] = json!({ "title": "no references", "command": "x" });
            lens
        });
        fixture.open("main.go");
        let all = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert_eq!(
            usages(&all),
            [(3, "Greet", Some(5)), (7, "Box", Some(0)), (9, "Open", Some(0)), (11, "main", Some(0))]
        );
        assert_eq!(fixture.server.requests("codeLens/resolve")[0]["data"]["id"], 7);
        let lines: Vec<u64> = fixture
            .server
            .requests("textDocument/references")
            .iter()
            .filter_map(|request| request["position"]["line"].as_u64())
            .collect();
        assert_eq!(lines, [9, 11], "only where no lens counts the usages");
    }

    #[test]
    fn switched_off_kinds_are_not_computed() {
        let mut fixture = served(json!({}));
        references_by_line(&fixture);
        fixture.open("main.go");
        fixture.session.settings_mut().set(AUTHORS, true).unwrap();
        fixture.session.settings_mut().set(USAGES, false).unwrap();
        let without = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert_eq!(without.lines.len(), 4);
        assert!(without.lenses.iter().all(|lens| lens.usages.is_none()));
        assert!(fixture.server.requests("textDocument/references").is_empty());
        fixture.session.settings_mut().set(ENABLED, false).unwrap();
        let off = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert!(off.lines.is_empty() && off.lenses.is_empty() && !off.pending);
        assert_eq!(
            Switches::from_settings(&Settings::default()),
            Switches { usages: true, authors: false },
            "the usages are on by default, the authors off"
        );
    }

    #[test]
    fn authors_come_from_blame_with_the_last_author_first() {
        use crate::vcs::testing::{git, isolate};
        isolate();
        let mut fixture = served(json!({}));
        fixture.server.on("textDocument/references", json!([]));
        let root = fixture.path("main.go").parent().unwrap().to_owned();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["add", "main.go"]);
        git(&root, &["commit", "-q", "-m", "First"]);
        let changed =
            MAIN.replace("type Box struct{}", "type Box struct{ }").replace("\tprintln(\"hi\")", "\tprintln(\"hey\")");
        std::fs::write(root.join("main.go"), changed).unwrap();
        git(
            &root,
            &[
                "commit",
                "-q",
                "-am",
                "Second",
                "--author",
                "Grace Hopper <grace@example.com>",
                "--date",
                "@4102444800 +0000",
            ],
        );
        fixture.open("main.go");
        let off = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert!(off.lenses.iter().all(|lens| lens.authors.is_none()), "the authors are off by default");
        fixture.session.settings_mut().set(AUTHORS, true).unwrap();
        let all = list(&mut fixture, json!({ "waitMs": 5000 }));
        let authors: Vec<Option<(String, usize)>> = all
            .lenses
            .iter()
            .map(|lens| lens.authors.as_ref().map(|authors| (authors.name.clone(), authors.others)))
            .collect();
        assert_eq!(
            authors,
            [
                Some(("Grace Hopper".to_owned(), 1)),
                Some(("Grace Hopper".to_owned(), 0)),
                Some(("Ada Lovelace".to_owned(), 0)),
                Some(("Ada Lovelace".to_owned(), 0)),
            ]
        );
        let commit = git(&root, &["rev-parse", "HEAD"]);
        assert_eq!(all.lenses[0].authors.as_ref().unwrap().commit, commit.trim(), "the latest change of Greet");
        assert_eq!(all.lenses[1].authors.as_ref().unwrap().commit, commit.trim());
    }

    #[test]
    fn titles_of_usage_lenses_give_their_counts() {
        assert_eq!(usage_count("3 references"), Some(3));
        assert_eq!(usage_count("1 usage"), Some(1));
        assert_eq!(usage_count("no references"), Some(0));
        assert_eq!(usage_count("▶ Run test"), None);
        assert_eq!(usage_count("2 implementations"), None);
    }
}

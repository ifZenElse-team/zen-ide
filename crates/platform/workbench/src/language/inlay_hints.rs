use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::{Client, Edit, Pending};

use super::{Languages, Server, Synced, with_languages};
use crate::Workbench;

pub const SETTLE: Duration = Duration::from_millis(300);

pub const MARGIN: usize = 100;

pub const ENABLED: &str = "editor.inlayHints.enabled";

/// A kind of inlay hint, switched on and off in Settings → Editor → Inlay Hints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HintKind {
    /// The names of the parameters before the arguments of a call (`in:`).
    ParameterNames,
    /// The names of the fields before the values of a composite literal (`key:`).
    FieldNames,
    /// The types of variables declared without one.
    VariableTypes,
    /// The types of composite literals written without one.
    LiteralTypes,
    /// The type arguments inferred for a call of a generic function.
    TypeArguments,
    /// The values of constants.
    ConstantValues,
}

/// What a hint is in the protocol (`InlayHintKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HintCategory {
    /// A type.
    Type,
    /// A parameter name.
    Parameter,
}

impl HintKind {
    pub const ALL: [HintKind; 6] = [
        HintKind::ParameterNames,
        HintKind::FieldNames,
        HintKind::VariableTypes,
        HintKind::LiteralTypes,
        HintKind::TypeArguments,
        HintKind::ConstantValues,
    ];

    pub fn id(self) -> &'static str {
        match self {
            HintKind::ParameterNames => "parameterNames",
            HintKind::FieldNames => "fieldNames",
            HintKind::VariableTypes => "variableTypes",
            HintKind::LiteralTypes => "literalTypes",
            HintKind::TypeArguments => "typeArguments",
            HintKind::ConstantValues => "constantValues",
        }
    }

    pub fn setting(self) -> String {
        format!("editor.inlayHints.{}", self.id())
    }

    pub fn on_by_default(self) -> bool {
        self == HintKind::ParameterNames
    }

    pub fn category(self) -> Option<HintCategory> {
        match self {
            HintKind::ParameterNames | HintKind::FieldNames => Some(HintCategory::Parameter),
            HintKind::VariableTypes | HintKind::LiteralTypes | HintKind::TypeArguments => Some(HintCategory::Type),
            HintKind::ConstantValues => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HintSwitches {
    pub enabled: bool,
    pub kinds: BTreeSet<HintKind>,
}

impl Default for HintSwitches {
    fn default() -> Self {
        Self { enabled: true, kinds: HintKind::ALL.into_iter().filter(|kind| kind.on_by_default()).collect() }
    }
}

impl HintSwitches {
    pub fn from_settings(settings: &zen_core::Settings) -> Self {
        Self {
            enabled: settings.get_or(ENABLED, true),
            kinds: HintKind::ALL
                .into_iter()
                .filter(|kind| settings.get_or(&kind.setting(), kind.on_by_default()))
                .collect(),
        }
    }

    pub fn keeps(&self, category: Option<HintCategory>) -> bool {
        self.enabled && self.kinds.iter().any(|kind| kind.category() == category)
    }

    pub fn is_on(&self, id: &str) -> bool {
        self.enabled && self.kinds.iter().any(|kind| kind.id() == id)
    }
}

/// An inlay hint: a label the editor draws before a character.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InlayHint {
    /// The hint's id in its file, for `editor.inlayHints.resolve`; a new answer of the server gives new ids.
    pub id: u64,
    /// The character offset of the file's text it is drawn before.
    pub offset: usize,
    /// The line it is on, one-based.
    pub line: u32,
    /// The column it is drawn before, one-based, in characters.
    pub column: u32,
    /// What it says, e.g. `in:` or `int`.
    pub label: String,
    /// What it is in the protocol, when the server says.
    pub kind: Option<HintCategory>,
    /// Whether the server asks for a space before it.
    pub padding_left: bool,
    /// Whether the server asks for a space after it.
    pub padding_right: bool,
    /// What the server says about it, shown when the pointer rests on it.
    pub tooltip: Option<String>,
    /// Whether `editor.inlayHints.resolve` can still ask the server for more about it.
    pub resolvable: bool,
}

struct Hint {
    shown: InlayHint,
    raw: Value,
}

struct Asked {
    version: u64,
    lines: Range<usize>,
    switches: HintSwitches,
}

#[derive(Default)]
pub(super) struct FileHints {
    hints: Vec<Hint>,
    generation: u64,
    pending: Option<(Asked, Pending)>,
    asked: Option<Asked>,
    wanted: Option<Range<usize>>,
    changed: Option<Instant>,
    next_id: u64,
    resolving: Option<(u64, Pending)>,
}

impl FileHints {
    fn covers(asked: &Asked, version: u64, wanted: &Range<usize>, switches: &HintSwitches) -> bool {
        asked.version == version
            && asked.switches == *switches
            && asked.lines.start <= wanted.start
            && wanted.end <= asked.lines.end
    }

    fn clear(&mut self) {
        if !self.hints.is_empty() {
            self.hints.clear();
            self.generation += 1;
        }
    }

    fn is_waiting(&self, version: u64, switches: &HintSwitches) -> bool {
        let Some(wanted) = &self.wanted else { return false };
        switches.enabled
            && (self.pending.is_some()
                || self.resolving.is_some()
                || !self.asked.as_ref().is_some_and(|asked| Self::covers(asked, version, wanted, switches)))
    }
}

pub(super) fn section(
    declared: Option<&zen_lsp::servers::InlayHintSettings>,
    base: &Value,
    switches: &HintSwitches,
) -> Option<(String, Value)> {
    let declared = declared?;
    Some((declared.section.clone(), declared.settings(base, |kind| switches.is_on(kind))))
}

pub(super) fn with_switches(
    declared: Option<&zen_lsp::servers::InlayHintSettings>,
    base: &Value,
    switches: &HintSwitches,
) -> Value {
    match declared {
        Some(declared) => declared.settings(base, |kind| switches.is_on(kind)),
        None => base.clone(),
    }
}

pub(super) fn step(client: &mut Client, synced: &mut Synced, switches: &HintSwitches, settle: bool) {
    let Some(resolves) = client.inlay_hints() else { return };
    let hints = &mut synced.hints;
    if let Some((_, pending)) = &hints.pending
        && let Some(answer) = pending.try_answer()
        && let Some((asked, _)) = hints.pending.take()
        && asked.version == synced.version
    {
        if let Ok(answer) = answer {
            replace(hints, &synced.text, &answer, switches, resolves);
        }
        hints.asked = Some(asked);
    }
    if let Some((_, pending)) = &hints.resolving
        && let Some(answer) = pending.try_answer()
        && let Some((id, _)) = hints.resolving.take()
        && let Ok(answer) = answer
        && let Some(hint) = hints.hints.iter_mut().find(|hint| hint.shown.id == id)
    {
        if let Some(tooltip) = tooltip(&answer) {
            hint.shown.tooltip = Some(tooltip);
        }
        if let Some(label) = label(&answer["label"]).filter(|label| !label.is_empty()) {
            hint.shown.label = label;
        }
        hint.shown.resolvable = false;
        hint.raw = answer;
        hints.generation += 1;
    }
    if !switches.enabled {
        hints.clear();
        if let Some((_, pending)) = hints.pending.take() {
            client.cancel(&pending);
        }
        hints.asked = None;
        return;
    }
    let Some(wanted) = hints.wanted.clone() else { return };
    if hints.asked.as_ref().is_some_and(|asked| FileHints::covers(asked, synced.version, &wanted, switches)) {
        return;
    }
    let settled = !settle || hints.asked.is_none() || hints.changed.is_none_or(|at| at.elapsed() >= SETTLE);
    if let Some((asked, pending)) = &hints.pending {
        if FileHints::covers(asked, synced.version, &wanted, switches) || !settled {
            return;
        }
        client.cancel(pending);
        hints.pending = None;
    }
    if !settled {
        return;
    }
    let lines = wanted.start.saturating_sub(MARGIN)..wanted.end.saturating_add(MARGIN);
    let range = protocol_range(&synced.text, &lines);
    let params = json!({ "textDocument": { "uri": synced.uri }, "range": range });
    if let Ok(pending) = client.request("textDocument/inlayHint", params) {
        hints.pending = Some((Asked { version: synced.version, lines, switches: switches.clone() }, pending));
    }
}

pub(super) fn resolve(client: &mut Client, synced: &mut Synced, id: u64) {
    let hints = &mut synced.hints;
    if hints.resolving.is_some() || client.inlay_hints() != Some(true) {
        return;
    }
    let Some(hint) = hints.hints.iter().find(|hint| hint.shown.id == id && hint.shown.resolvable) else { return };
    if let Ok(pending) = client.request("inlayHint/resolve", hint.raw.clone()) {
        hints.resolving = Some((id, pending));
    }
}

pub(super) fn refresh(synced: &mut Synced) {
    synced.hints.asked = None;
}

pub(super) fn touch(synced: &mut Synced) {
    synced.hints.changed = Some(Instant::now());
}

pub(super) fn follow_edit(hints: &mut FileHints, old: &str, edit: &Edit) {
    hints.changed = Some(Instant::now());
    if hints.hints.is_empty() {
        return;
    }
    let start = super::char_offset(old, edit.range.start);
    let end = start + old.get(edit.range.clone()).map_or(0, |replaced| replaced.chars().count());
    let delta = edit.text.chars().count() as isize - (end - start) as isize;
    hints.hints.retain_mut(|hint| {
        let offset = hint.shown.offset;
        if offset < start || (offset == start && end > start) {
            true
        } else if offset >= end {
            hint.shown.offset = (offset as isize + delta) as usize;
            true
        } else {
            false
        }
    });
    hints.generation += 1;
}

fn protocol_range(text: &str, lines: &Range<usize>) -> Value {
    let mut line = 0;
    let mut end = None;
    for (index, c) in text.char_indices() {
        if c == '\n' || (c == '\r' && !text[index + 1..].starts_with('\n')) {
            line += 1;
            if line == lines.end {
                end = Some(json!({ "line": line, "character": 0 }));
                break;
            }
        }
    }
    let end = end.unwrap_or_else(|| json!(zen_lsp::position::position(text, text.len())));
    json!({ "start": { "line": lines.start.min(line), "character": 0 }, "end": end })
}

fn replace(hints: &mut FileHints, text: &str, answer: &Value, switches: &HintSwitches, resolves: bool) {
    let mut found: Vec<(u32, u32, &Value)> = answer
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|hint| {
                    let position = &hint["position"];
                    Some((position["line"].as_u64()? as u32, position["character"].as_u64()? as u32, hint))
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort_by_key(|(line, character, _)| (*line, *character));
    let offsets = char_offsets(text, found.iter().map(|(line, character, _)| (*line, *character)));
    hints.hints.clear();
    for ((_, _, raw), offset) in found.into_iter().zip(offsets) {
        let kind = match raw["kind"].as_u64() {
            Some(1) => Some(HintCategory::Type),
            Some(2) => Some(HintCategory::Parameter),
            _ => None,
        };
        let Some(label) = label(&raw["label"]).filter(|label| !label.trim().is_empty()) else { continue };
        if !switches.keeps(kind) {
            continue;
        }
        hints.next_id += 1;
        let shown = InlayHint {
            id: hints.next_id,
            offset,
            line: 0,
            column: 0,
            label,
            kind,
            padding_left: raw["paddingLeft"].as_bool().unwrap_or(false),
            padding_right: raw["paddingRight"].as_bool().unwrap_or(false),
            tooltip: tooltip(raw),
            resolvable: resolves && raw.get("data").is_some(),
        };
        hints.hints.push(Hint { shown, raw: raw.clone() });
    }
    hints.generation += 1;
}

fn label(label: &Value) -> Option<String> {
    match label {
        Value::String(label) => Some(label.clone()),
        Value::Array(parts) => Some(parts.iter().filter_map(|part| part["value"].as_str()).collect()),
        _ => None,
    }
}

fn tooltip(hint: &Value) -> Option<String> {
    let text = |tooltip: &Value| match tooltip {
        Value::String(text) => Some(text.clone()),
        Value::Object(markup) => markup.get("value").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    };
    text(&hint["tooltip"])
        .or_else(|| hint["label"].as_array().and_then(|parts| parts.iter().find_map(|part| text(&part["tooltip"]))))
        .filter(|tooltip| !tooltip.trim().is_empty())
}

fn char_offsets(text: &str, positions: impl Iterator<Item = (u32, u32)>) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut chars = text.char_indices().peekable();
    let (mut line, mut chars_before, mut line_start_chars) = (0u32, 0usize, 0usize);
    let mut column_units = 0usize;
    for (wanted_line, wanted_units) in positions {
        while line < wanted_line {
            let Some((index, c)) = chars.next() else { break };
            chars_before += 1;
            let ends = c == '\n' || (c == '\r' && !text[index + 1..].starts_with('\n'));
            if ends {
                line += 1;
                line_start_chars = chars_before;
                column_units = 0;
            }
        }
        if line < wanted_line {
            offsets.push(chars_before);
            continue;
        }
        if chars_before < line_start_chars {
            chars_before = line_start_chars;
        }
        while column_units < wanted_units as usize {
            match chars.peek() {
                Some(&(_, c)) if c != '\n' && c != '\r' => {
                    column_units += c.len_utf16();
                    chars_before += 1;
                    chars.next();
                }
                _ => break,
            }
        }
        offsets.push(chars_before);
    }
    offsets
}

impl Languages {
    pub fn set_hint_switches(&mut self, switches: HintSwitches) {
        if self.hint_switches == switches {
            return;
        }
        self.hint_switches = switches;
        for server in self.servers.values_mut() {
            if let Server::Running { files, .. } = server {
                files.values_mut().for_each(touch);
            }
        }
    }

    fn hints_of(&mut self, path: &Path) -> Option<(&mut Client, &mut Synced)> {
        let id = self.spec_for(path)?.id().to_owned();
        match self.servers.get_mut(&id)? {
            Server::Running { client, files } => Some((client, files.get_mut(path)?)),
            _ => None,
        }
    }
}

/// The parameters of `editor.inlayHints.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The first line wanted, one-based; the first of the file when absent.
    #[serde(default)]
    pub from_line: Option<usize>,
    /// The last line wanted, one-based; the last of the file when absent.
    #[serde(default)]
    pub to_line: Option<usize>,
    /// The generation the caller has; when it is still current, the hints are left out.
    #[serde(default)]
    pub since: Option<u64>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000, without waiting for
    /// the text to settle; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `editor.inlayHints.list` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HintsAnswer {
    /// Whether inlay hints are on (Settings → Editor → Inlay Hints).
    pub enabled: bool,
    /// Bumped whenever the file's hints change.
    pub generation: u64,
    /// Whether newer hints are on their way: asked for, or waiting for the text to settle.
    pub pending: bool,
    /// The hints on the lines wanted, in order, unless `since` was current.
    pub hints: Option<Vec<InlayHint>>,
}

pub struct ListHints;

const PLUGIN_GENERATION: u64 = 1 << 62;

fn active_path(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    path.or_else(|| {
        session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path().to_owned())
    })
    .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

fn shown_hints(languages: &Languages, workbench: &Workbench, path: &Path, lines: &Range<usize>) -> Vec<InlayHint> {
    let Some(synced) = languages.servers.values().find_map(|server| match server {
        Server::Running { files, .. } => files.get(path),
        _ => None,
    }) else {
        return Vec::new();
    };
    let document = workbench.documents().iter().find(|document| document.path() == path);
    let snapshot = document.filter(|document| document.version() == synced.version).map(crate::Document::snapshot);
    let owned;
    let text = match snapshot {
        Some(text) => text,
        None => {
            owned = zen_text::Text::from(synced.text.as_str());
            &owned
        }
    };
    synced
        .hints
        .hints
        .iter()
        .filter_map(|hint| {
            let point = text.offset_to_point(hint.shown.offset).ok()?;
            lines.contains(&point.line).then(|| InlayHint {
                line: point.line as u32 + 1,
                column: point.column as u32 + 1,
                ..hint.shown.clone()
            })
        })
        .collect()
}

impl Command for ListHints {
    type Params = ListParams;
    type Output = HintsAnswer;
    const ID: &'static str = "editor.inlayHints.list";
    const TITLE: &'static str = "Inlay Hints";
    const DESCRIPTION: &'static str = "The inlay hints of a file from its language server (parameter names, types, constant \
         values), with where the editor draws each, for some lines (fromLine and toLine, one-based) or the whole file; \
         the active file by default. Asks for them when they are missing; waitMs waits for the answer.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<HintsAnswer, CommandError> {
        let path = active_path(session, params.path)?;
        let switches = HintSwitches::from_settings(session.settings());
        let wait = super::wait(params.wait_ms);
        let deadline = Instant::now() + wait;
        let from = params.from_line.unwrap_or(1).saturating_sub(1);
        let to = params.to_line.unwrap_or(usize::MAX - MARGIN);
        let lines = from..to.max(from + 1);
        let mut plugin_hints = if switches.enabled { crate::plugins::inlays(session, &path) } else { None };
        Ok(with_languages(session, |languages, workbench| {
            languages.set_hint_switches(switches.clone());
            loop {
                if !wait.is_zero() {
                    languages.sync(workbench);
                }
                let waiting = match languages.hints_of(&path) {
                    Some((client, synced)) => {
                        synced.hints.wanted = Some(lines.clone());
                        step(client, synced, &switches, wait.is_zero());
                        client.inlay_hints().is_some() && synced.hints.is_waiting(synced.version, &switches)
                    }
                    None => !wait.is_zero() && languages.serves(&path),
                };
                if !waiting || Instant::now() >= deadline {
                    if languages.hints_of(&path).is_none()
                        && let Some((version, found)) = plugin_hints.take()
                    {
                        let generation = PLUGIN_GENERATION | version;
                        let hints = (params.since != Some(generation)).then(|| {
                            found.into_iter().filter(|hint| lines.contains(&(hint.line as usize - 1))).collect()
                        });
                        return HintsAnswer { enabled: switches.enabled, generation, pending: false, hints };
                    }
                    let generation = languages.hints_of(&path).map_or(0, |(_, synced)| synced.hints.generation);
                    let hints = (params.since != Some(generation) || generation == 0)
                        .then(|| shown_hints(languages, workbench, &path, &lines));
                    return HintsAnswer { enabled: switches.enabled, generation, pending: waiting, hints };
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }))
    }
}

/// The parameters of `editor.inlayHints.resolve`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The hint's id, as `editor.inlayHints.list` gives it.
    pub id: u64,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct ResolveHint;

impl Command for ResolveHint {
    type Params = ResolveParams;
    type Output = Option<InlayHint>;
    const ID: &'static str = "editor.inlayHints.resolve";
    const TITLE: &'static str = "Resolve Inlay Hint";
    const DESCRIPTION: &'static str = "Asks the language server for what it left out of an inlay hint (its tooltip), when it \
         resolves hints; returns the hint as it is now, or null when it is gone.";

    fn run(&self, session: &mut Session, params: ResolveParams) -> Result<Option<InlayHint>, CommandError> {
        let path = active_path(session, params.path)?;
        let switches = HintSwitches::from_settings(session.settings());
        let deadline = Instant::now() + super::wait(params.wait_ms);
        Ok(with_languages(session, |languages, workbench| {
            loop {
                let resolving = match languages.hints_of(&path) {
                    Some((client, synced)) => {
                        resolve(client, synced, params.id);
                        step(client, synced, &switches, true);
                        synced.hints.resolving.as_ref().is_some_and(|(id, _)| *id == params.id)
                    }
                    None => false,
                };
                if !resolving || Instant::now() >= deadline {
                    return shown_hints(languages, workbench, &path, &(0..usize::MAX))
                        .into_iter()
                        .find(|hint| hint.id == params.id);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }))
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListHints)?;
    registry.register(ResolveHint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::testing::{self, Fixture};

    const MAIN: &str =
        "package main\n\nfunc greet(name string, times int) {}\n\nfunc main() {\n\tgreet(\"hi\", 3)\n\tx := 2\n}\n";

    fn hint(line: u32, character: u32, label: &str, kind: u32) -> Value {
        json!({ "position": { "line": line, "character": character }, "label": label, "kind": kind, "paddingRight": kind == 2 })
    }

    fn served(text: &str, capabilities: Value) -> Fixture {
        let mut fixture = testing::fixture(&[("main.go", text)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "inlayHintProvider": capabilities
        }));
        fixture.server.on_with("textDocument/inlayHint", |_| {
            json!([hint(5, 7, "name:", 2), hint(5, 13, "times:", 2), hint(6, 2, " int", 1)])
        });
        fixture.open("main.go");
        fixture
    }

    fn list(fixture: &mut Fixture, params: Value) -> Value {
        fixture.dispatch("editor.inlayHints.list", params).unwrap()
    }

    fn labels(answer: &Value) -> Vec<String> {
        answer["hints"].as_array().unwrap().iter().map(|hint| hint["label"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn hints_come_for_the_lines_asked_with_where_they_are_drawn() {
        let mut fixture = served(MAIN, json!(true));
        let answer = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert_eq!(answer["pending"], false);
        assert_eq!(labels(&answer), ["name:", "times:"], "type hints are off by default");
        let first = &answer["hints"][0];
        assert_eq!((first["line"].as_u64(), first["column"].as_u64()), (Some(6), Some(8)));
        let chars: Vec<char> = MAIN.chars().collect();
        assert_eq!(chars[first["offset"].as_u64().unwrap() as usize], '"');
        assert_eq!(first["kind"], "parameter");
        assert_eq!(first["paddingRight"], true);
        assert_eq!(labels(&list(&mut fixture, json!({ "fromLine": 1, "toLine": 5 }))), Vec::<String>::new());
        let generation = answer["generation"].as_u64();
        let again = list(&mut fixture, json!({ "since": generation }));
        assert!(again["hints"].is_null());
        assert_eq!(fixture.server.requests("textDocument/inlayHint").len(), 1);
    }

    #[test]
    fn only_the_shown_lines_and_a_margin_are_asked_for() {
        let text: String = (0..400).map(|line| format!("// {line}\n")).collect();
        let mut fixture = served(&text, json!(true));
        list(&mut fixture, json!({ "fromLine": 201, "toLine": 210, "waitMs": 5000 }));
        let asked = fixture.server.requests("textDocument/inlayHint");
        assert_eq!(
            asked[0]["range"],
            json!({ "start": { "line": 100, "character": 0 }, "end": { "line": 310, "character": 0 } })
        );
        list(&mut fixture, json!({ "fromLine": 250, "toLine": 260 }));
        assert_eq!(fixture.server.requests("textDocument/inlayHint").len(), 1, "still inside the lines asked for");
        list(&mut fixture, json!({ "fromLine": 380, "toLine": 400, "waitMs": 5000 }));
        let asked = fixture.server.requests("textDocument/inlayHint");
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[1]["range"]["start"]["line"], 279);
        assert_eq!(asked[1]["range"]["end"], json!({ "line": 400, "character": 0 }));
    }

    #[test]
    fn hints_move_with_edits_and_are_asked_for_once_the_text_settles() {
        let mut fixture = served(MAIN, json!(true));
        let before = list(&mut fixture, json!({ "waitMs": 5000 }));
        let offset = before["hints"][0]["offset"].as_u64().unwrap();
        let line_start = MAIN.find("\tgreet").unwrap();
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": line_start }] })).unwrap();
        fixture.dispatch("editor.edit.insert", json!({ "text": "ab" })).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let typed = list(&mut fixture, json!({}));
        assert_eq!(typed["hints"][0]["offset"].as_u64(), Some(offset + 2));
        assert_eq!(typed["hints"][0]["column"].as_u64(), Some(10));
        assert_eq!(typed["pending"], true);
        assert_eq!(fixture.server.requests("textDocument/inlayHint").len(), 1, "nothing is asked while typing goes on");
        std::thread::sleep(SETTLE + Duration::from_millis(20));
        list(&mut fixture, json!({}));
        assert_eq!(fixture.server.wait_for("textDocument/inlayHint", 2, Duration::from_secs(5)).len(), 2);
        let deadline = Instant::now() + Duration::from_secs(60);
        while list(&mut fixture, json!({}))["pending"] == true {
            assert!(Instant::now() < deadline, "the second answer did not come");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut hints = FileHints::default();
        hints.hints.push(Hint { shown: InlayHint { offset: 5, ..before_hint() }, raw: Value::Null });
        hints.hints.push(Hint { shown: InlayHint { offset: 9, ..before_hint() }, raw: Value::Null });
        follow_edit(&mut hints, "0123456789", &Edit { range: 4..7, text: String::new() });
        let offsets: Vec<usize> = hints.hints.iter().map(|hint| hint.shown.offset).collect();
        assert_eq!(offsets, [6], "a hint inside what was deleted goes, one after it moves back");
    }

    fn before_hint() -> InlayHint {
        InlayHint {
            id: 0,
            offset: 0,
            line: 0,
            column: 0,
            label: "a:".to_owned(),
            kind: None,
            padding_left: false,
            padding_right: false,
            tooltip: None,
            resolvable: false,
        }
    }

    #[test]
    fn each_kind_is_switched_in_the_settings() {
        let mut fixture = served(MAIN, json!(true));
        fixture.session.settings_mut().set(&HintKind::VariableTypes.setting(), true).unwrap();
        fixture.session.settings_mut().set(&HintKind::ParameterNames.setting(), false).unwrap();
        assert_eq!(labels(&list(&mut fixture, json!({ "waitMs": 5000 }))), [" int"]);
        fixture.session.settings_mut().set(&HintKind::FieldNames.setting(), true).unwrap();
        std::thread::sleep(SETTLE);
        assert_eq!(labels(&list(&mut fixture, json!({ "waitMs": 5000 }))), ["name:", "times:", " int"]);
        fixture.session.settings_mut().set(ENABLED, false).unwrap();
        let off = list(&mut fixture, json!({ "waitMs": 5000 }));
        assert_eq!((off["enabled"].as_bool(), off["pending"].as_bool()), (Some(false), Some(false)));
        assert_eq!(labels(&off), Vec::<String>::new());
        assert_eq!(fixture.server.requests("textDocument/inlayHint").len(), 2);
    }

    #[test]
    fn a_server_with_settings_for_the_kinds_is_told_which_are_on() {
        let mut fixture = testing::fixture(&[("main.go", MAIN)]);
        fixture.server.set_capabilities(
            json!({ "textDocumentSync": { "openClose": true, "change": 2 }, "inlayHintProvider": true }),
        );
        fixture.with(|languages, _| {
            languages.entries[0].declaration.inlay_hints = Some(zen_lsp::servers::InlayHintSettings {
                section: "gopls".to_owned(),
                kinds: [
                    ("parameterNames".to_owned(), vec!["hints.parameterNames".to_owned()]),
                    (
                        "variableTypes".to_owned(),
                        vec!["hints.assignVariableTypes".to_owned(), "hints.rangeVariableTypes".to_owned()],
                    ),
                ]
                .into(),
            });
        });
        fixture.open("main.go");
        let initialize = &fixture.server.requests("initialize")[0];
        assert_eq!(
            initialize["initializationOptions"]["hints"],
            json!({ "parameterNames": true, "assignVariableTypes": false, "rangeVariableTypes": false })
        );
        fixture.session.settings_mut().set(&HintKind::VariableTypes.setting(), true).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let changes = fixture.server.wait_for("workspace/didChangeConfiguration", 2, Duration::from_secs(5));
        let last = &changes.last().unwrap()["settings"]["gopls"]["hints"];
        assert_eq!(*last, json!({ "parameterNames": true, "assignVariableTypes": true, "rangeVariableTypes": true }));
        let count = changes.len();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        assert_eq!(fixture.server.requests("workspace/didChangeConfiguration").len(), count, "told once");
    }

    #[test]
    fn a_hint_is_resolved_when_asked() {
        let mut fixture = testing::fixture(&[("main.go", MAIN)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "inlayHintProvider": { "resolveProvider": true }
        }));
        fixture.server.on("textDocument/inlayHint", json!([{ "position": { "line": 5, "character": 7 }, "label": [{ "value": "na" }, { "value": "me:" }], "kind": 2, "data": 7 }]));
        fixture.server.on_with("inlayHint/resolve", |hint| {
            let mut hint = hint.clone();
            hint["tooltip"] = json!({ "kind": "markdown", "value": "the name to greet" });
            hint
        });
        fixture.open("main.go");
        let answer = list(&mut fixture, json!({ "waitMs": 5000 }));
        let shown = &answer["hints"][0];
        assert_eq!((shown["label"].as_str(), shown["resolvable"].as_bool()), (Some("name:"), Some(true)));
        let resolved =
            fixture.dispatch("editor.inlayHints.resolve", json!({ "id": shown["id"], "waitMs": 5000 })).unwrap();
        assert_eq!(resolved["tooltip"], "the name to greet");
        assert_eq!(resolved["resolvable"], false);
        assert_eq!(fixture.server.requests("inlayHint/resolve")[0]["data"], 7);
    }

    #[test]
    fn protocol_positions_become_character_offsets() {
        let text = "// привет\r\nx😀y\rz\n";
        let offsets = char_offsets(text, [(0, 3), (1, 1), (1, 3), (2, 0), (2, 9), (7, 0)].into_iter());
        assert_eq!(offsets, [3, 12, 13, 15, 16, 17]);
    }
}

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::lsp_types::{CodeActionProviderCapability, Position};

use super::edits::{self, Applied};
use super::intentions::{apply_edit, edit_message, execute, resolve, resolves};
use super::refactor::{JournalEntry, Refactoring, is_identifier, undo_entry};
use super::{Languages, Lookup, MAX_WAIT, byte_offset, file_text, request_in, with_languages};
use crate::Workbench;

const OWNER: &str = "extract";

const EXPRESSIONS: usize = 8;

const MATCHED_TOKENS: usize = 4_000_000;

const RENAME_COMMANDS: [&str; 2] = ["_typescript.rename", "editor.action.rename"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extract {
    Variable,
    Constant,
    Field,
    Parameter,
    Method,
    Inline,
}

impl Extract {
    fn only(self) -> Value {
        match self {
            Self::Inline => json!(["refactor.inline"]),
            _ => json!(["refactor.extract", "refactor.introduce"]),
        }
    }

    fn name(self) -> String {
        match self {
            Self::Variable => t!("workbench-extract-variable"),
            Self::Constant => t!("workbench-extract-constant"),
            Self::Field => t!("workbench-extract-field"),
            Self::Parameter => t!("workbench-extract-parameter"),
            Self::Method => t!("workbench-extract-method"),
            Self::Inline => t!("workbench-extract-inline"),
        }
    }
}

pub fn refactoring_of(kind: &str, title: &str) -> Option<Extract> {
    let title = title.to_lowercase();
    let has = |word: &str| title.contains(word);
    if kind.starts_with("refactor.inline") {
        return Some(Extract::Inline);
    }
    match kind {
        "refactor.extract.variable" | "refactor.extract.variable-all" => Some(Extract::Variable),
        "refactor.extract.constant" | "refactor.extract.constant-all" => Some(if has("field") {
            Extract::Field
        } else if has("enclosing scope") || has(" in function") || has(" in method") || has(" in arrow function") {
            Extract::Variable
        } else {
            Extract::Constant
        }),
        "refactor.extract.field" => Some(Extract::Field),
        "refactor.introduce.parameter" | "refactor.extract.parameter" => Some(Extract::Parameter),
        "refactor.extract.function" | "refactor.extract.method" => Some(Extract::Method),
        _ if kind == "refactor.extract" || kind == "refactor.introduce" || kind.starts_with("refactor.extract.") => {
            if has("variable") {
                Some(Extract::Variable)
            } else if has("constant") {
                Some(Extract::Constant)
            } else if has("field") {
                Some(Extract::Field)
            } else if has("parameter") {
                Some(Extract::Parameter)
            } else if has("function") || has("method") {
                Some(Extract::Method)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[derive(Default)]
pub struct Extracting {
    asked: Option<Asked>,
}

type Performed = (Result<(String, Option<Value>), CommandError>, Vec<Applied>);

struct Asked {
    refactoring: Extract,
    path: PathBuf,
    range: Range<usize>,
    document_version: Option<u64>,
    lookup: Lookup,
}

impl Extracting {
    fn waiting(&self) -> BTreeSet<PathBuf> {
        self.asked.iter().filter(|asked| asked.lookup.request.is_some()).map(|asked| asked.path.clone()).collect()
    }

    fn drop_asked(&mut self, languages: &mut Languages) {
        if let Some(mut asked) = self.asked.take()
            && let Some(request) = asked.lookup.request.take()
            && let Some(client) = languages.client_for(&asked.path)
        {
            client.cancel(&request.pending);
        }
    }
}

/// The parameters of the extract and inline refactorings.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtractParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The first character of the code to extract, a character offset; the primary selection when absent.
    #[serde(default)]
    pub start: Option<usize>,
    /// The character after it; `start` when absent.
    #[serde(default)]
    pub end: Option<usize>,
    /// Which of the answer's `variants` to apply, by index; needed when there are several.
    #[serde(default)]
    pub variant: Option<usize>,
    /// The name to give what is extracted instead of the server's.
    #[serde(default)]
    pub name: Option<String>,
    /// How long to wait for the server in milliseconds, at most 10 000, which is the default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// A range of a file, in characters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRange {
    /// The first character, a character offset in the file.
    pub start: usize,
    /// The character after the range.
    pub end: usize,
    /// The text of the range.
    pub text: String,
}

/// What an extract or inline refactoring did, or what it needs to go on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    /// Whether the server has not answered yet; call again with the same `start` and `end`.
    pub pending: bool,
    /// The code the refactoring works on, once known.
    pub start: usize,
    /// The character after it.
    pub end: usize,
    /// The expressions at the caret that qualify, smallest first, when there are several: call again with the
    /// `start` and `end` of one.
    pub expressions: Vec<ExtractRange>,
    /// The ways the server offers, in its words, when there are several: call again with `variant`.
    pub variants: Vec<String>,
    /// The title of the applied action, once applied.
    pub title: Option<String>,
    /// The files changed, in path order.
    pub files: Vec<PathBuf>,
    /// The file the new name is in.
    pub path: Option<PathBuf>,
    /// The name given to what was extracted.
    pub name: Option<String>,
    /// Every place of that name in `path`, the declaration and its uses.
    pub occurrences: Vec<ExtractRange>,
    /// Other names that fit, the given one first.
    pub suggestions: Vec<String>,
}

macro_rules! refactoring {
    ($command:ident, $kind:expr, $id:literal, $title:literal, $description:literal) => {
        pub struct $command;

        impl Command for $command {
            type Params = ExtractParams;
            type Output = Extraction;
            const ID: &'static str = $id;
            const TITLE: &'static str = $title;
            const DESCRIPTION: &'static str = concat!(
                $description,
                " The language server does the change: its code actions of this kind at the selection, or at the \
                 caret (when several expressions qualify there, the answer lists them in `expressions` and nothing \
                 changes: call again with the `start` and `end` of one; when the server offers several ways, \
                 `variants` lists them: call again with `variant`). Call again with the returned `start` and `end` \
                 while pending. The change is one undo step; the answer names the new name and every place of it, \
                 and `name` gives it another name at once."
            );

            fn run(&self, session: &mut Session, params: ExtractParams) -> Result<Extraction, CommandError> {
                run(session, $kind, params)
            }
        }
    };
}

refactoring!(
    ExtractVariable,
    Extract::Variable,
    "refactor.extract.variable",
    "Extract Variable",
    "Puts an expression into a new local variable and uses the variable in its place."
);
refactoring!(
    ExtractConstant,
    Extract::Constant,
    "refactor.extract.constant",
    "Extract Constant",
    "Puts an expression into a new constant and uses the constant in its place."
);
refactoring!(
    ExtractField,
    Extract::Field,
    "refactor.extract.field",
    "Extract Field",
    "Puts an expression into a new field of the class and uses the field in its place."
);
refactoring!(
    ExtractParameter,
    Extract::Parameter,
    "refactor.extract.parameter",
    "Extract Parameter",
    "Makes an expression a new parameter of its function, passing it at every call."
);
refactoring!(
    ExtractMethod,
    Extract::Method,
    "refactor.extract.method",
    "Extract Method",
    "Moves the selected code into a new function or method and calls it in its place."
);
refactoring!(
    InlineSymbol,
    Extract::Inline,
    "refactor.inline.apply",
    "Inline",
    "Puts the value of the variable, constant or the body of the function at the caret in place of its uses."
);

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ExtractVariable)?;
    registry.register(ExtractConstant)?;
    registry.register(ExtractField)?;
    registry.register(ExtractParameter)?;
    registry.register(ExtractMethod)?;
    registry.register(InlineSymbol)?;
    Ok(())
}

fn run(session: &mut Session, refactoring: Extract, params: ExtractParams) -> Result<Extraction, CommandError> {
    let (path, mut range) = place(session, &params)?;
    crate::hidden_files::guard_read(session, &[&path])?;
    if range.is_empty() && params.start.is_none() && refactoring != Extract::Inline {
        let found = expressions(session, &path, range.start);
        match found.as_slice() {
            [] => {}
            [only] => range = only.start..only.end,
            _ => {
                return Ok(Extraction {
                    start: range.start,
                    end: range.end,
                    expressions: found,
                    ..Extraction::default()
                });
            }
        }
    }
    let allowed =
        session.state::<crate::scratch::Scratches>().map(crate::scratch::Scratches::folders).unwrap_or_default();
    session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
    let mut extracting = std::mem::take(session.state_mut::<Extracting>());
    let (result, applied) = with_languages(session, |languages, workbench| {
        let result = step(&mut extracting, languages, workbench, refactoring, &path, range, &params, &allowed);
        languages.hold(OWNER, extracting.waiting());
        result
    });
    session.set_state(extracting);
    let written: crate::local_history::Written = applied.iter().flat_map(Applied::written).collect();
    session.state_mut::<Refactoring>().record(applied);
    crate::local_history::changed_by(session, crate::local_history::EXTRACT, written);
    result
}

fn place(session: &Session, params: &ExtractParams) -> Result<(PathBuf, Range<usize>), CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let path = match &params.path {
        Some(path) => workbench.resolve(path),
        None => workbench
            .active_document()
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
    };
    if let Some(start) = params.start {
        let end = params.end.unwrap_or(start);
        return Ok((path, start.min(end)..start.max(end)));
    }
    let document = workbench
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    Ok((path, document.selections().primary().range()))
}

fn expressions(session: &mut Session, path: &Path, caret: usize) -> Vec<ExtractRange> {
    let extra = crate::extend_selection::server_ranges(session, path, &[caret], crate::extend_selection::DEFAULT_WAIT)
        .pop()
        .unwrap_or_default();
    let Some(document) =
        session.state::<Workbench>().and_then(|workbench| workbench.documents().iter().find(|d| d.path() == path))
    else {
        return Vec::new();
    };
    let units = crate::extend_selection::structure(session, document);
    let chars: Vec<char> = document.text().chars().collect();
    let mut found: Vec<ExtractRange> = Vec::new();
    let mut range = caret..caret;
    while let Some(next) = units.extend(range.clone(), &extra) {
        if next == range || next.end > chars.len() {
            break;
        }
        range = next;
        let text: String = chars[range.clone()].iter().collect();
        if text.contains('\n') || found.len() == EXPRESSIONS {
            break;
        }
        if qualifies(&chars, &range, &text) && found.last().is_none_or(|last| last.text != text.trim()) {
            let trimmed = text.trim();
            let start = range.start + text.chars().take_while(|c| c.is_whitespace()).count();
            found.push(ExtractRange { start, end: start + trimmed.chars().count(), text: trimmed.to_owned() });
        }
    }
    found
}

fn qualifies(chars: &[char], range: &Range<usize>, text: &str) -> bool {
    let text = text.trim();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    if text.is_empty() || text.ends_with([';', '{', '}', ',']) || text.starts_with(['{', '}', ',']) {
        return false;
    }
    let before = chars[..range.start].iter().rev().find(|c| !c.is_whitespace()).copied();
    let after = chars[range.end..].iter().find(|c| !c.is_whitespace()).copied();
    if text.starts_with(['(', '[']) && before.is_some_and(word) {
        return false;
    }
    if text.chars().all(word) && after == Some('(') {
        return false;
    }
    let line_start = chars[..range.start].iter().rposition(|c| *c == '\n').map_or(0, |at| at + 1);
    let line_end = chars[range.end..].iter().position(|c| *c == '\n').map_or(chars.len(), |at| range.end + at);
    let line: String = chars[line_start..line_end].iter().collect();
    if line.trim() == text || line.trim().trim_end_matches(';') == text {
        return false;
    }
    let mut depth = 0i32;
    let mut previous = ' ';
    let all: Vec<char> = text.chars().collect();
    for (index, &c) in all.iter().enumerate() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' | ';' if depth == 0 => return false,
            '=' if depth == 0 => {
                let next = all.get(index + 1).copied().unwrap_or(' ');
                if !matches!(previous, '=' | '!' | '<' | '>') && !matches!(next, '=' | '>') {
                    return false;
                }
            }
            _ => {}
        }
        if depth < 0 {
            return false;
        }
        previous = c;
    }
    depth == 0
}

#[allow(clippy::too_many_arguments)]
fn step(
    extracting: &mut Extracting,
    languages: &mut Languages,
    workbench: &mut Workbench,
    refactoring: Extract,
    path: &Path,
    range: Range<usize>,
    params: &ExtractParams,
    allowed: &[PathBuf],
) -> (Result<Extraction, CommandError>, Vec<Applied>) {
    let wait = params.wait_ms.map_or(MAX_WAIT, |wait_ms| Duration::from_millis(wait_ms).min(MAX_WAIT));
    let deadline = Instant::now() + wait;
    let document_version = document_version(workbench, path);
    if let Some(reason) =
        workbench.documents().iter().find(|document| document.path() == path).and_then(crate::Document::read_only)
    {
        return (Err(CommandError::new(reason.to_owned())), Vec::new());
    }
    if let Some(name) = &params.name {
        let language = languages.spec_for(path).map(|spec| spec.language_for(path));
        if !is_identifier(language.as_deref(), name) {
            return (Err(CommandError::new(t!("workbench-rename-invalid-name", name = name.as_str()))), Vec::new());
        }
    }
    languages.sync(workbench);
    let reuse = extracting.asked.as_ref().is_some_and(|asked| {
        asked.refactoring == refactoring
            && asked.path == path
            && asked.range == range
            && asked.document_version == document_version
            && languages.server_version(path) == Some(asked.lookup.version)
    });
    if !reuse {
        extracting.drop_asked(languages);
        if let Err(error) = gives_actions(languages, workbench, path) {
            return (Err(error), Vec::new());
        }
        let only = refactoring.only();
        let request = request_in(languages, workbench, path, "textDocument/codeAction", |uri, text| {
            let position = |offset: usize| zen_lsp::position::position(text, byte_offset(text, offset));
            json!({
                "textDocument": { "uri": uri },
                "range": { "start": position(range.start), "end": position(range.end) },
                "context": { "diagnostics": [], "only": only, "triggerKind": 1 }
            })
        });
        let request = match request {
            Ok(request) => request,
            Err(error) => return (Err(CommandError::new(error)), Vec::new()),
        };
        extracting.asked = Some(Asked {
            refactoring,
            path: path.to_owned(),
            range: range.clone(),
            document_version,
            lookup: Lookup {
                method: "textDocument/codeAction".to_owned(),
                path: path.to_owned(),
                offset: range.start,
                uri: request.uri.clone(),
                version: request.version,
                request: Some(request),
                answer: None,
                busy_server: false,
            },
        });
    }
    let pending = Extraction { pending: true, start: range.start, end: range.end, ..Extraction::default() };
    let Some(asked) = extracting.asked.as_mut() else { return (Ok(pending), Vec::new()) };
    if !asked.lookup.poll(wait) {
        return (Ok(pending), Vec::new());
    }
    let answer = match asked.lookup.answer.clone() {
        Some(Ok(answer)) => answer,
        Some(Err(error)) => {
            extracting.asked = None;
            return (Err(CommandError::new(error)), Vec::new());
        }
        None => return (Ok(pending), Vec::new()),
    };
    let offered: Vec<&Value> = answer
        .as_array()
        .into_iter()
        .flatten()
        .filter(|action| {
            let title = action["title"].as_str().unwrap_or_default();
            action["kind"].as_str().and_then(|kind| refactoring_of(kind, title)) == Some(refactoring)
        })
        .collect();
    let usable: Vec<Value> =
        offered.iter().filter(|action| action["disabled"].is_null()).map(|a| (*a).clone()).collect();
    if usable.is_empty() {
        extracting.asked = None;
        let reason = offered.iter().find_map(|action| action["disabled"]["reason"].as_str()).map(str::to_owned);
        let message = reason.unwrap_or_else(|| t!("workbench-extract-none", refactoring = refactoring.name()));
        return (Err(CommandError::new(message)), Vec::new());
    }
    let action = match params.variant {
        Some(index) => match usable.get(index) {
            Some(action) => action.clone(),
            None => return (Err(CommandError::new(t!("workbench-extract-no-variant", index = index))), Vec::new()),
        },
        None if usable.len() > 1 => {
            let variants =
                usable.iter().map(|action| action["title"].as_str().unwrap_or_default().to_owned()).collect();
            return (
                Ok(Extraction { start: range.start, end: range.end, variants, ..Extraction::default() }),
                Vec::new(),
            );
        }
        None => usable[0].clone(),
    };
    extracting.asked = None;
    let before = file_text(workbench, path).unwrap_or_default();
    let (result, applied) = perform(languages, workbench, path, action.clone(), deadline, allowed);
    let (title, rename_at) = match result {
        Ok(done) => done,
        Err(error) => return (Err(error), Vec::new()),
    };
    let mut applied = applied;
    let mut files: Vec<PathBuf> = applied.iter().map(|applied| applied.path().to_owned()).collect();
    files.sort();
    files.dedup();
    let mut done =
        Extraction { start: range.start, end: range.end, title: Some(title), files, ..Extraction::default() };
    if refactoring != Extract::Inline {
        let after = file_text(workbench, path).unwrap_or_default();
        let found = snippet_name(&action["edit"])
            .or_else(|| rename_at.as_ref().and_then(|arguments| renamed_at(&after, arguments)))
            .or_else(|| new_name(&before, &after, range.start));
        if let Some(found) = found {
            let mut occurrences = occurrences(&before, &after, &found);
            let mut name = found;
            if let Some(wanted) = params.name.as_ref().filter(|wanted| **wanted != name) {
                match rename(workbench, path, &occurrences, wanted) {
                    Ok(more) => {
                        applied.push(more);
                        let renamed = file_text(workbench, path).unwrap_or_default();
                        occurrences = whole_words(&renamed, wanted);
                        name = wanted.clone();
                    }
                    Err(error) => {
                        let applied = edits::journal(applied, workbench);
                        undo_entry(&JournalEntry { files: applied }, languages, workbench);
                        return (Err(CommandError::new(error.message())), Vec::new());
                    }
                }
                languages.sync(workbench);
            }
            let chars: Vec<char> = before.chars().collect();
            let expression: String = chars.get(range.clone()).map(|chars| chars.iter().collect()).unwrap_or_default();
            let language = languages.spec_for(path).map(|spec| spec.language_for(path));
            done.suggestions = suggestions(&name, &expression, refactoring, language.as_deref());
            done.path = Some(path.to_owned());
            done.occurrences = occurrences;
            done.name = Some(name);
        }
    }
    (Ok(done), edits::journal(applied, workbench))
}

fn document_version(workbench: &Workbench, path: &Path) -> Option<u64> {
    workbench.documents().iter().find(|document| document.path() == path).map(crate::Document::version)
}

fn gives_actions(languages: &mut Languages, workbench: &Workbench, path: &Path) -> Result<(), CommandError> {
    if !languages.serves(path) {
        return Err(CommandError::new(t!("workbench-language-not-running")));
    }
    let Some(client) = languages.client_for(path) else {
        if workbench.documents().iter().all(|document| document.path() != path) {
            languages.start_for(path);
        }
        return Err(CommandError::new(t!("workbench-language-not-running")));
    };
    match client.capabilities().and_then(|capabilities| capabilities.code_action_provider.as_ref()) {
        None | Some(CodeActionProviderCapability::Simple(false)) => {
            Err(CommandError::new(t!("workbench-extract-unsupported")))
        }
        Some(_) => Ok(()),
    }
}

fn perform(
    languages: &mut Languages,
    workbench: &mut Workbench,
    path: &Path,
    mut action: Value,
    deadline: Instant,
    allowed: &[PathBuf],
) -> Performed {
    let title = action["title"].as_str().unwrap_or_default().to_owned();
    let command = if action["command"].is_string() {
        Some(action.clone())
    } else {
        if !action["edit"].is_object() && !action["command"].is_object() && resolves(languages, None, path) {
            match resolve(languages, None, path, &action, deadline) {
                Ok(resolved) => action = resolved,
                Err(error) => return (Err(CommandError::new(error)), Vec::new()),
            }
        }
        action["command"].is_object().then(|| action["command"].clone())
    };
    let (command, rename) = match command {
        Some(command) if RENAME_COMMANDS.contains(&command["command"].as_str().unwrap_or_default()) => {
            (None, Some(command["arguments"].clone()))
        }
        other => (other, None),
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
    languages.take_rename_at();
    if let Some(command) = &command {
        let (sent, more) = execute(languages, workbench, None, path, command, deadline, allowed);
        applied.extend(more);
        if let Err(error) = sent {
            let journal = edits::journal(applied, workbench);
            undo_entry(&JournalEntry { files: journal }, languages, workbench);
            return (Err(CommandError::new(error)), Vec::new());
        }
    }
    if applied.is_empty() {
        return (Err(CommandError::new(t!("workbench-intentions-nothing"))), Vec::new());
    }
    languages.sync(workbench);
    let rename = rename.or_else(|| languages.take_rename_at().map(|params| json!([params])));
    (Ok((title, rename)), applied)
}

fn renamed_at(after: &str, arguments: &Value) -> Option<String> {
    let place = arguments.as_array()?.iter().find_map(|argument| {
        let position = if argument["position"].is_object() { &argument["position"] } else { argument };
        let position: Position = serde_json::from_value(position.clone()).ok()?;
        Some(position)
    })?;
    let lines = zen_lsp::position::LineStarts::new(after);
    let offset = super::char_offset(after, lines.offset(after, place)?);
    identifiers(after)
        .into_iter()
        .find(|(start, word)| *start <= offset && offset <= start + word.chars().count())
        .map(|(_, word)| word)
}

fn rename(
    workbench: &mut Workbench,
    path: &Path,
    occurrences: &[ExtractRange],
    name: &str,
) -> Result<Applied, edits::EditError> {
    let replacements: Vec<(Range<usize>, String)> =
        occurrences.iter().map(|occurrence| (occurrence.start..occurrence.end, name.to_owned())).collect();
    if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) {
        if !document.replace_ranges(replacements) {
            return Err(edits::EditError::FilesChanged);
        }
        return Ok(Applied::Document { path: path.to_owned(), version: document.version() });
    }
    let before = zen_vfs::read_text(path)
        .map_err(|error| edits::EditError::Read { path: path.to_owned(), error: error.to_string() })?;
    let mut after = before.clone();
    for (range, text) in replacements.iter().rev() {
        after.replace_range(byte_offset(&before, range.start)..byte_offset(&before, range.end), text);
    }
    zen_vfs::replace_unchanged(path, Some(before.as_bytes()), after.as_bytes())
        .map_err(|error| edits::EditError::Write { path: path.to_owned(), error: error.to_string() })?;
    Ok(Applied::Disk { path: path.to_owned(), before, after })
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn identifiers(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    for (count, c) in text.chars().chain(std::iter::once(' ')).enumerate() {
        if is_word(c) {
            if current.is_empty() {
                start = count;
            }
            current.push(c);
        } else if !current.is_empty() {
            if current.chars().next().is_some_and(|first| !first.is_ascii_digit()) {
                found.push((start, std::mem::take(&mut current)));
            }
            current.clear();
        }
    }
    found
}

fn whole_words(text: &str, name: &str) -> Vec<ExtractRange> {
    identifiers(text)
        .into_iter()
        .filter(|(_, word)| word == name)
        .map(|(start, word)| ExtractRange { start, end: start + word.chars().count(), text: word })
        .collect()
}

pub(crate) fn snippet_name(edit: &Value) -> Option<String> {
    let mut lists: Vec<&Value> = Vec::new();
    if let Some(changes) = edit["documentChanges"].as_array() {
        lists.extend(changes.iter().map(|change| &change["edits"]));
    }
    if let Some(changes) = edit["changes"].as_object() {
        lists.extend(changes.values());
    }
    lists
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|edit| match (edit["snippet"]["value"].as_str(), edit["newText"].as_str()) {
            (Some(snippet), _) => Some(snippet),
            (None, Some(text)) if edit["insertTextFormat"] == 2 => Some(text),
            _ => None,
        })
        .find_map(placeholder_name)
}

fn placeholder_name(snippet: &str) -> Option<String> {
    let chars: Vec<char> = snippet.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            '$' => {
                let rest = &chars[index + 1..];
                if rest.first() == Some(&'{') {
                    let digits = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();
                    if digits > 0 && rest.get(1 + digits) == Some(&':') {
                        let name: String = rest[2 + digits..].iter().take_while(|c| **c != '}').collect();
                        if !name.is_empty() && name.chars().all(is_word) {
                            return Some(name);
                        }
                    }
                }
                let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
                let skip = if digits > 0 {
                    digits
                } else if rest.first() == Some(&'{') {
                    rest.iter().position(|c| *c == '}').map_or(0, |at| at + 1)
                } else {
                    0
                };
                let name: String = rest[skip..].iter().take_while(|c| is_word(**c)).collect();
                if skip > 0 && !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) {
                    return Some(name);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn new_name(before: &str, after: &str, near: usize) -> Option<String> {
    let mut counts: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();
    for (_, word) in identifiers(before) {
        counts.entry(word).or_default().0 += 1;
    }
    for (_, word) in identifiers(after) {
        counts.entry(word).or_default().1 += 1;
    }
    identifiers(after)
        .into_iter()
        .filter_map(|(start, word)| {
            let (old, new) = counts[&word];
            (new > old).then_some((new - old < 2, old > 0, start.abs_diff(near), word))
        })
        .min()
        .map(|(_, _, _, word)| word)
}

fn occurrences(before: &str, after: &str, name: &str) -> Vec<ExtractRange> {
    let old = identifiers(before);
    let new = identifiers(after);
    if !old.iter().any(|(_, word)| word == name) {
        return whole_words(after, name);
    }
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a.1 == b.1).count();
    let suffix = old[prefix..].iter().rev().zip(new[prefix..].iter().rev()).take_while(|(a, b)| a.1 == b.1).count();
    let (old, new) = (&old[prefix..old.len() - suffix], &new[prefix..new.len() - suffix]);
    let mut added = vec![true; new.len()];
    if old.len() * new.len() <= MATCHED_TOKENS {
        let mut table = vec![vec![0u32; new.len() + 1]; old.len() + 1];
        for i in (0..old.len()).rev() {
            for j in (0..new.len()).rev() {
                table[i][j] =
                    if old[i].1 == new[j].1 { table[i + 1][j + 1] + 1 } else { table[i + 1][j].max(table[i][j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < old.len() && j < new.len() {
            if old[i].1 == new[j].1 {
                added[j] = false;
                i += 1;
                j += 1;
            } else if table[i + 1][j] >= table[i][j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    new.iter()
        .zip(added)
        .filter(|((_, word), added)| *added && word == name)
        .map(|((start, word), _)| ExtractRange { start: *start, end: start + word.chars().count(), text: word.clone() })
        .collect()
}

fn words_of(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (index, &c) in chars.iter().enumerate() {
        if c == '_' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let next_lower = chars.get(index + 1).is_some_and(|next| next.is_lowercase());
        let previous_lower = index > 0 && chars[index - 1].is_lowercase();
        if c.is_uppercase() && !current.is_empty() && (previous_lower || next_lower) {
            words.push(std::mem::take(&mut current));
        }
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn trailing_name(expression: &str) -> Option<String> {
    let mut text: Vec<char> = expression.trim().chars().collect();
    while let Some(&last) = text.last() {
        let open = match last {
            ')' => '(',
            ']' => '[',
            '>' => '<',
            _ => break,
        };
        let mut depth = 0;
        let mut at = None;
        for index in (0..text.len()).rev() {
            if text[index] == last {
                depth += 1;
            } else if text[index] == open {
                depth -= 1;
                if depth == 0 {
                    at = Some(index);
                    break;
                }
            }
        }
        text.truncate(at?);
    }
    let name: String = text.iter().rev().take_while(|c| is_word(**c)).collect::<Vec<_>>().into_iter().rev().collect();
    let before = text.len() - name.chars().count();
    let joined = before == 0 || matches!(text[before - 1], '.' | ':' | ' ' | '&' | '*' | '!');
    (joined && !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit())).then_some(name)
}

fn suggestions(name: &str, expression: &str, refactoring: Extract, language: Option<&str>) -> Vec<String> {
    let mut found = vec![name.to_owned()];
    let Some(source) = trailing_name(expression) else { return found };
    let mut words = words_of(&source);
    if words.len() > 1
        && ["get", "is", "has", "to", "as", "new", "create", "make", "read", "load"].contains(&words[0].as_str())
    {
        words.remove(0);
    }
    let screaming = name.chars().any(char::is_alphabetic) && !name.chars().any(char::is_lowercase);
    let snake = name.contains('_') || matches!(language, Some("rust" | "python"));
    let pascal = name.starts_with(char::is_uppercase) && !screaming;
    for from in 0..words.len() {
        let part = &words[from..];
        let candidate = if screaming || (refactoring == Extract::Constant && matches!(language, Some("rust" | "java")))
        {
            part.iter().map(|word| word.to_uppercase()).collect::<Vec<_>>().join("_")
        } else if snake {
            part.join("_")
        } else {
            part.iter()
                .enumerate()
                .map(|(index, word)| {
                    if index == 0 && !pascal {
                        word.clone()
                    } else {
                        let mut chars = word.chars();
                        chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
                    }
                })
                .collect()
        };
        if is_identifier(language, &candidate) && !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    found
}

#[cfg(test)]
mod tests;

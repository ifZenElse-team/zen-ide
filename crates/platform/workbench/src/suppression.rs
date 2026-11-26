use std::collections::HashMap;
use std::ops::{Range, RangeInclusive};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::servers::{SuppressionDeclaration, SuppressionForm};
use zen_project::inspect::{FixKind, InspectionFix};
use zen_syntax::{HighlightKind, OutlineItem, Span, SuppressedLines, SymbolKind};

use crate::Workbench;
use crate::inspect_code::{inspection_id, is_inspection};
use crate::language::edits::{self, Step};
use crate::language::refactor::Refactoring;
use crate::language::structure::Outliners;
use crate::language::{Languages, Problem, Severity, with_languages};

pub const INTENTION_PREFIX: &str = "inspect.suppress:";

const RECHECK: Duration = Duration::from_secs(2);

/// How far a suppression reaches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SuppressScope {
    /// The statement of the problem's line.
    #[default]
    Statement,
    /// The function around the problem.
    Function,
    /// The whole file.
    File,
}

const SCOPES: [SuppressScope; 3] = [SuppressScope::Statement, SuppressScope::Function, SuppressScope::File];

impl SuppressScope {
    fn title(self, inspection: &str) -> String {
        match self {
            Self::Statement => t!("workbench-suppress-statement", inspection = inspection),
            Self::Function => t!("workbench-suppress-function", inspection = inspection),
            Self::File => t!("workbench-suppress-file", inspection = inspection),
        }
    }

    fn form(self, declaration: &SuppressionDeclaration) -> Option<&SuppressionForm> {
        match self {
            Self::Statement => declaration.statement.as_ref(),
            Self::Function => declaration.function.as_ref(),
            Self::File => declaration.file.as_ref(),
        }
    }
}

type Stamp = Option<(u64, SystemTime)>;

struct ReadFile {
    version: Option<u64>,
    stamp: Stamp,
    checked: Instant,
    found: Vec<SuppressedLines>,
}

#[derive(Default)]
pub struct Suppressions {
    files: HashMap<PathBuf, ReadFile>,
    offered: Vec<SuppressOption>,
}

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

pub(crate) fn refresh(session: &mut Session, path: &Path) {
    let Some(outliner) = session
        .state::<Outliners>()
        .and_then(|outliners| outliners.for_file(path))
        .filter(|outliner| outliner.reads_suppressions())
    else {
        return;
    };
    let version = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(crate::Document::version);
    let known = session.state::<Suppressions>().and_then(|state| state.files.get(path));
    let now = Instant::now();
    if known.is_some_and(|known| {
        known.version == version && (version.is_some() || now.duration_since(known.checked) < RECHECK)
    }) {
        return;
    }
    let stamp = if version.is_none() { stamp(path) } else { None };
    if version.is_none() && known.is_some_and(|known| known.version.is_none() && known.stamp == stamp) {
        if let Some(known) = session.state_mut::<Suppressions>().files.get_mut(path) {
            known.checked = now;
        }
        return;
    }
    let Some(workbench) = session.state::<Workbench>() else { return };
    let found =
        crate::language::file_text(workbench, path).map(|text| outliner.suppressions(&text)).unwrap_or_default();
    session.state_mut::<Suppressions>().files.insert(path.to_owned(), ReadFile { version, stamp, checked: now, found });
}

fn hides(found: &[SuppressedLines], problem: &Problem) -> bool {
    !matches!(problem.severity, Severity::Error | Severity::Typo)
        && found.iter().any(|one| {
            one.lines.contains(&problem.line)
                && (one.ids.is_empty() || one.ids.iter().any(|id| is_inspection(problem, id)))
        })
}

pub(crate) fn apply(session: &Session, path: &Path, problems: &mut Vec<Problem>) {
    let Some(read) = session.state::<Suppressions>().and_then(|state| state.files.get(path)) else { return };
    if !read.found.is_empty() {
        problems.retain(|problem| !hides(&read.found, problem));
    }
}

fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
}

fn written_id(problem: &Problem) -> Option<&str> {
    match (problem.source.as_deref(), problem.code.as_deref()) {
        (_, Some(code)) if plain_id(code) && code != "default" => Some(code),
        (Some(source), _) if plain_id(source) => Some(source),
        _ => None,
    }
}

fn declaration_for(session: &Session, path: &Path, problem: &Problem) -> Option<SuppressionDeclaration> {
    let source = problem.source.as_deref().unwrap_or_default();
    session.state::<Languages>()?.suppression_for(path, source)
}

fn line_of(text: &str, offset: usize) -> u32 {
    text.chars().take(offset).filter(|c| *c == '\n').count() as u32 + 1
}

fn enclosing(items: &[OutlineItem], offset: usize) -> Option<(usize, usize)> {
    items.iter().filter(|item| item.start <= offset && offset < item.end.max(item.start + 1)).find_map(|item| {
        enclosing(&item.children, offset).or_else(|| {
            matches!(item.kind, SymbolKind::Function | SymbolKind::Method).then_some((item.start, item.end))
        })
    })
}

fn function_at(session: &mut Session, path: &Path, text: &str, offset: usize) -> Option<(usize, usize)> {
    let outline = match session.state::<Outliners>().and_then(|outliners| outliners.for_file(path)) {
        Some(outliner) => outliner.outline(text),
        None => crate::plugins::outline(session, path, text)?,
    };
    enclosing(&outline, offset)
}

/// A way to suppress a problem, as `inspect.suppress.list` offers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuppressOption {
    /// What a person reads.
    pub title: String,
    /// The file.
    pub path: PathBuf,
    /// The character offset of the problem.
    pub offset: usize,
    /// The inspection, as `inspect.results.list` names it.
    pub inspection: String,
    /// How far it reaches.
    pub scope: SuppressScope,
}

/// The parameters of `inspect.suppress.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuppressListParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

fn text_of(session: &Session, path: &Path) -> Result<String, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    crate::language::file_text(workbench, path).map_err(CommandError::new)
}

fn scopes_of(session: &mut Session, path: &Path, text: &str, problem: &Problem) -> Vec<SuppressScope> {
    if matches!(problem.severity, Severity::Error | Severity::Typo) || written_id(problem).is_none() {
        return Vec::new();
    }
    let Some(declaration) = declaration_for(session, path, problem) else { return Vec::new() };
    let function = declaration.function.is_some() && function_at(session, path, text, problem.start).is_some();
    SCOPES
        .into_iter()
        .filter(|scope| scope.form(&declaration).is_some())
        .filter(|scope| *scope != SuppressScope::Function || function)
        .collect()
}

pub(crate) fn options_of(session: &mut Session, path: &Path, text: &str, problem: &Problem) -> Vec<SuppressOption> {
    let inspection = inspection_id(problem);
    let title = written_id(problem).unwrap_or_default().to_owned();
    scopes_of(session, path, text, problem)
        .into_iter()
        .map(|scope| SuppressOption {
            title: scope.title(&title),
            path: path.to_owned(),
            offset: problem.start,
            inspection: inspection.clone(),
            scope,
        })
        .collect()
}

fn covers(problem: &Problem, offset: usize) -> bool {
    problem.start <= offset && offset <= problem.end.max(problem.start + 1)
}

fn listed(session: &mut Session, path: &Path, offset: usize) -> Vec<SuppressOption> {
    let Ok(text) = text_of(session, path) else { return Vec::new() };
    let problems = crate::inspections::all_problems(session, path);
    let line = line_of(&text, offset);
    let mut found: Vec<&Problem> = problems.iter().filter(|problem| covers(problem, offset)).collect();
    if found.is_empty() {
        found = problems.iter().filter(|problem| problem.line == line).collect();
    }
    let mut options: Vec<SuppressOption> = Vec::new();
    for problem in found {
        for option in options_of(session, path, &text, problem) {
            if !options.iter().any(|kept| kept.inspection == option.inspection && kept.scope == option.scope) {
                options.push(option);
            }
        }
    }
    options
}

pub(crate) fn intentions(session: &mut Session, path: &Path, offset: usize) -> Vec<(String, String)> {
    let options = listed(session, path, offset);
    let titled =
        options.iter().enumerate().map(|(at, option)| (format!("{INTENTION_PREFIX}{at}"), option.title.clone()));
    let intentions = titled.collect();
    session.state_mut::<Suppressions>().offered = options;
    intentions
}

pub(crate) fn offered(session: &Session, id: &str) -> Option<SuppressOption> {
    let at: usize = id.strip_prefix(INTENTION_PREFIX)?.parse().ok()?;
    session.state::<Suppressions>()?.offered.get(at).cloned()
}

pub(crate) fn with_fixes(session: &mut Session, path: &Path, problems: &mut [Problem]) {
    let Ok(text) = text_of(session, path) else { return };
    for problem in problems {
        for option in options_of(session, path, &text, problem) {
            let params = serde_json::to_value(&option).unwrap_or_default();
            let scope = serde_json::to_value(option.scope).unwrap_or_default();
            problem.fixes.push(InspectionFix {
                id: format!("{INTENTION_PREFIX}{}:{}", option.inspection, scope.as_str().unwrap_or_default()),
                title: option.title,
                kind: FixKind::Action,
                batch: None,
                label: None,
                inline: false,
                args: json!({ "action": ApplySuppression::ID, "params": params }),
            });
        }
    }
}

struct Inserted {
    at: usize,
    text: String,
}

fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (at, _) in text.match_indices('\n') {
        lines.push(start..at);
        start = at + 1;
    }
    lines.push(start..text.len());
    lines
}

fn added_to(
    text: &str,
    line: &Range<usize>,
    form: &SuppressionForm,
    separator: &str,
    id: &str,
) -> Option<Option<Inserted>> {
    let (prefix, suffix) = form.text.split_once("{id}")?;
    let shown = text[line.clone()].trim_end_matches('\r');
    let start = shown.find(prefix)? + prefix.len();
    let rest = &shown[start..];
    let end = if suffix.is_empty() { rest.find(char::is_whitespace).unwrap_or(rest.len()) } else { rest.find(suffix)? };
    let known = rest[..end].split(separator.trim()).any(|one| one.trim() == id);
    Some((!known).then(|| Inserted { at: line.start + start + end, text: format!("{separator}{id}") }))
}

fn commented(spans: &[Span], line: &Range<usize>) -> bool {
    spans.iter().any(|span| {
        matches!(span.kind, HighlightKind::Comment | HighlightKind::DocComment)
            && span.range.start < line.end
            && line.start < span.range.end
    })
}

fn ends_in_a_literal(spans: &[Span], line: &Range<usize>) -> bool {
    line.end > line.start && zen_syntax::in_string_or_comment(spans, line.end)
}

fn inserted(
    text: &str,
    spans: &[Span],
    form: &SuppressionForm,
    separator: &str,
    line: usize,
    id: &str,
) -> Option<Option<Inserted>> {
    let lines = line_ranges(text);
    let target = match &form.before {
        Some(start) => lines.iter().position(|line| text[line.clone()].starts_with(start.as_str())).unwrap_or(0),
        None => line.min(lines.len() - 1),
    };
    let above = target.checked_sub(1).map(|at| &lines[at]);
    if let Some(found) =
        std::iter::once(&lines[target]).chain(above).find_map(|line| added_to(text, line, form, separator, id))
    {
        return Some(found);
    }
    let directive = form.text.replace("{id}", id);
    let place = &lines[target];
    let shown = text[place.clone()].trim_end_matches('\r');
    if form.line_end && !commented(spans, place) && !ends_in_a_literal(spans, place) {
        return Some(Some(Inserted { at: place.start + shown.len(), text: format!(" {directive}") }));
    }
    let indent = &shown[..shown.len() - shown.trim_start().len()];
    let newline = if text[place.clone()].ends_with('\r') { "\r\n" } else { "\n" };
    Some(Some(Inserted { at: place.start, text: format!("{indent}{directive}{newline}") }))
}

pub(crate) struct Written {
    pub(crate) covered: RangeInclusive<u32>,
    pub(crate) at_char: usize,
    pub(crate) chars: usize,
    pub(crate) lines: u32,
}

pub(crate) fn write(
    session: &mut Session,
    path: &Path,
    problem: &Problem,
    scope: SuppressScope,
) -> Result<Option<Written>, CommandError> {
    let refused = || CommandError::new(t!("workbench-suppress-unavailable", inspection = inspection_id(problem)));
    let before = text_of(session, path)?;
    if !scopes_of(session, path, &before, problem).contains(&scope) {
        return Err(refused());
    }
    let declaration = declaration_for(session, path, problem).ok_or_else(refused)?;
    let form = scope.form(&declaration).ok_or_else(refused)?;
    let id = written_id(problem).ok_or_else(refused)?;
    let (line, covered) = match scope {
        SuppressScope::Statement => (problem.line as usize - 1, problem.line..=problem.line),
        SuppressScope::Function => {
            let (start, end) = function_at(session, path, &before, problem.start).ok_or_else(refused)?;
            let first = line_of(&before, start);
            (first as usize - 1, first..=line_of(&before, end))
        }
        SuppressScope::File => (0, 1..=u32::MAX),
    };
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let spans = crate::search::highlighters(session)
        .and_then(|source| source(&name))
        .map(|found| found.highlight(&before))
        .unwrap_or_default();
    let Some(insertion) = inserted(&before, &spans, form, &declaration.separator, line, id).ok_or_else(refused)? else {
        return Ok(None);
    };
    let mut after = before.clone();
    after.insert_str(insertion.at, &insertion.text);
    let Some(edit) = crate::resource_bundles::change(path, &before, &after) else { return Ok(None) };
    let steps = [Step::Text(edit)];
    session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
    let applied = with_languages(session, |languages, workbench| edits::apply(&steps, languages, workbench))
        .map_err(|error| CommandError::new(error.message()))?;
    session.state_mut::<Refactoring>().record(applied);
    Ok(Some(Written {
        covered,
        at_char: before[..insertion.at].chars().count(),
        chars: insertion.text.chars().count(),
        lines: insertion.text.matches('\n').count() as u32,
    }))
}

/// The parameters of `inspect.suppress.apply`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuppressParams {
    /// The file.
    pub path: PathBuf,
    /// The character offset of the problem.
    pub offset: usize,
    /// The inspection.
    pub inspection: String,
    /// How far it reaches.
    pub scope: SuppressScope,
}

/// What `inspect.suppress.apply` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Suppressed {
    /// Whether the file changed; false when it already says so.
    pub changed: bool,
}

pub struct ListSuppressions;

impl Command for ListSuppressions {
    type Params = SuppressListParams;
    type Output = Vec<SuppressOption>;
    const ID: &'static str = "inspect.suppress.list";
    const TITLE: &'static str = "List Suppressions";
    const DESCRIPTION: &'static str = "How the problems at a place (the caret of the active file by default, else anywhere on \
         its line) can be suppressed with the language's own mechanism: for the statement, the function or the file. \
         An error from the compiler cannot be. Apply one with inspect.suppress.apply.";

    fn run(&self, session: &mut Session, params: SuppressListParams) -> Result<Vec<SuppressOption>, CommandError> {
        let workbench =
            session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let path = params
            .path
            .or_else(|| workbench.active_document().map(|document| document.path().to_owned()))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let offset = match params.offset {
            Some(offset) => offset,
            None => workbench
                .documents()
                .iter()
                .find(|document| document.path() == path)
                .map(|document| document.selections().primary().head)
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        };
        crate::hidden_files::guard_read(session, std::slice::from_ref(&path))?;
        Ok(listed(session, &path, offset))
    }
}

pub struct ApplySuppression;

impl Command for ApplySuppression {
    type Params = SuppressParams;
    type Output = Suppressed;
    const ID: &'static str = "inspect.suppress.apply";
    const TITLE: &'static str = "Suppress Inspection";
    const DESCRIPTION: &'static str = "Suppresses a problem listed by inspect.suppress.list by writing the language's own \
         mechanism into the file, //nolint:<id> in Go and #[allow(<id>)] in Rust; an open file keeps it unsaved and one \
         Undo reverts it.";

    fn run(&self, session: &mut Session, params: SuppressParams) -> Result<Suppressed, CommandError> {
        crate::hidden_files::guard_read(session, std::slice::from_ref(&params.path))?;
        let problem = crate::inspections::all_problems(session, &params.path)
            .into_iter()
            .find(|problem| covers(problem, params.offset) && inspection_id(problem) == params.inspection)
            .ok_or_else(|| CommandError::new(t!("workbench-inspect-fix-gone")))?;
        let written = write(session, &params.path, &problem, params.scope)?;
        Ok(Suppressed { changed: written.is_some() })
    }
}

pub(crate) fn item_options(session: &mut Session, item: &crate::inspect_code::ResultItem) -> Vec<SuppressOption> {
    let Ok(text) = text_of(session, &item.path) else { return Vec::new() };
    options_of(session, &item.path, &text, &item.problem)
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListSuppressions)?;
    registry.register(ApplySuppression)?;
    Ok(())
}

#[cfg(test)]
mod tests;

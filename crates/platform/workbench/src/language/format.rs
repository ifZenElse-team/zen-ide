use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::lsp_types::CodeActionProviderCapability;
use zen_project::settings::CodeStyle;

use super::{Languages, Lookup, MAX_WAIT, char_offset, with_languages};
use crate::Workbench;
use crate::actions_on_save::{ActionsOnSave, ReformatScope};

pub const REFORMAT_ON_SAVE: &str = "editor.onSave.reformat";

pub const OPTIMIZE_IMPORTS_ON_SAVE: &str = "editor.onSave.optimizeImports";

pub const SAVE_WAIT: Duration = Duration::from_secs(1);

const DEFAULT_WAIT: Duration = Duration::from_secs(1);

const ORGANIZE_IMPORTS: &str = "source.organizeImports";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Reformat,
    OptimizeImports,
    Fix,
}

impl Step {
    fn method(self) -> &'static str {
        match self {
            Self::Reformat => "textDocument/formatting",
            Self::OptimizeImports | Self::Fix => "textDocument/codeAction",
        }
    }

    fn by_kind(self) -> bool {
        matches!(self, Self::OptimizeImports | Self::Fix)
    }
}

struct Job {
    step: Step,
    document_version: u64,
    lookup: Lookup,
    resolved: bool,
    kind: String,
    rest: Vec<String>,
    changed: bool,
    server: Option<String>,
    keep_marked: bool,
    only_lines: Option<Vec<Range<usize>>>,
}

#[derive(Default)]
struct Formatting {
    reformat: Option<Job>,
    optimize: Option<Job>,
    fix: Option<Job>,
    external: Option<crate::code_quality::FormatterRun>,
}

impl Formatting {
    fn slot(&mut self, step: Step) -> &mut Option<Job> {
        match step {
            Step::Reformat => &mut self.reformat,
            Step::OptimizeImports => &mut self.optimize,
            Step::Fix => &mut self.fix,
        }
    }
}

/// The parameters of `editor.code.reformat` and `editor.imports.optimize`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormatParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; 1 000 by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// Reformat the whole SQL file even when the active one has a selection, whose statements alone are reformatted otherwise.
    #[serde(default)]
    pub whole_file: bool,
    /// Rearrange Code first (editor.code.rearrange); the setting editor.code.reformat.rearrange when absent. Reformat Code only.
    #[serde(default)]
    pub rearrange: Option<bool>,
}

/// What reformatting or optimizing imports did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormatResult {
    /// Whether the file's text changed.
    pub changed: bool,
    /// Whether the server has not answered yet; running the command again waits on for the same answer.
    pub pending: bool,
    /// Why nothing was applied, in words for the user.
    pub error: Option<String>,
    /// Whether Code Style → Formatter → Do not format leaves this file alone.
    pub excluded: bool,
}

#[derive(Clone, Debug, Default)]
struct FormatStyle {
    indent: CodeStyle,
    declared: serde_json::Map<String, Value>,
}

fn configured_style(session: &Session, path: &Path) -> FormatStyle {
    FormatStyle {
        indent: crate::file_indent::configured_at(session, path).style,
        declared: crate::code_style_options::formatting_options(session, path),
    }
}

fn send(
    languages: &mut Languages,
    workbench: &Workbench,
    style: &FormatStyle,
    step: Step,
    path: &Path,
    keep_marked: bool,
) -> Result<Job, String> {
    let mut kinds = match step {
        Step::Reformat | Step::Fix => Vec::new(),
        Step::OptimizeImports => languages
            .spec_for(path)
            .map(|entry| entry.declaration.optimize_imports_kinds())
            .unwrap_or_else(|| vec![ORGANIZE_IMPORTS.to_owned()]),
    };
    let kind = if kinds.is_empty() { String::new() } else { kinds.remove(0) };
    let server = if step == Step::Reformat { formatting_server(languages, path) } else { None };
    send_kind(languages, workbench, style, step, path, kind, kinds, server)
        .map(|job| Job { keep_marked: keep_marked && step == Step::Reformat, ..job })
}

fn formatting_server(languages: &Languages, path: &Path) -> Option<String> {
    let running = languages.alongside_running(path);
    languages
        .alongside_for(path)
        .into_iter()
        .find(|entry| entry.declaration.formats && running.iter().any(|id| id == entry.id()))
        .map(|entry| entry.id().to_owned())
}

pub(crate) fn fixes_for(
    languages: &Languages,
    settings: &zen_core::Settings,
    path: &Path,
    all: bool,
) -> Vec<(String, String)> {
    let running = languages.alongside_running(path);
    languages
        .alongside_for(path)
        .into_iter()
        .filter(|entry| running.iter().any(|id| id == entry.id()))
        .filter_map(|entry| {
            let fix = entry.declaration.fix_on_save.as_ref()?;
            (all || settings.get_or(&fix.setting, false)).then(|| (entry.id().to_owned(), fix.kind.clone()))
        })
        .collect()
}

#[allow(clippy::too_many_arguments, reason = "one request of a step, with what the step carries on to")]
fn send_kind(
    languages: &mut Languages,
    workbench: &Workbench,
    style: &FormatStyle,
    step: Step,
    path: &Path,
    kind: String,
    rest: Vec<String>,
    server: Option<String>,
) -> Result<Job, String> {
    languages.sync(workbench);
    let document = workbench
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .ok_or_else(|| t!("workbench-error-no-document"))?;
    let params = |uri: &str, text: &str| match step {
        Step::Reformat => {
            json!({ "textDocument": { "uri": uri }, "options": declared_options(style) })
        }
        Step::OptimizeImports | Step::Fix => {
            let end = zen_lsp::position::position(text, text.len());
            json!({
                "textDocument": { "uri": uri },
                "range": { "start": { "line": 0, "character": 0 }, "end": end },
                "context": { "diagnostics": [], "only": [kind.as_str()], "triggerKind": 1 }
            })
        }
    };
    let request = match &server {
        Some(id) => languages.request_to(id, path, step.method(), params)?,
        None => {
            let (client, synced) = languages.server_for(path).ok_or_else(|| t!("workbench-language-not-running"))?;
            let uri = synced.uri.clone();
            let params = params(&uri, &synced.text);
            client.document_request(&uri, step.method(), params).map_err(|error| error.to_string())?
        }
    };
    let lookup = Lookup {
        method: step.method().to_owned(),
        path: path.to_owned(),
        offset: 0,
        uri: request.uri.clone(),
        version: request.version,
        request: Some(request),
        answer: None,
        busy_server: false,
    };
    Ok(Job {
        step,
        document_version: document.version(),
        lookup,
        resolved: false,
        kind,
        rest,
        changed: false,
        server,
        keep_marked: false,
        only_lines: None,
    })
}

fn finish(
    languages: &mut Languages,
    workbench: &mut Workbench,
    job: &mut Job,
    wait: Duration,
) -> Option<Result<bool, String>> {
    let deadline = Instant::now() + wait;
    loop {
        if !job.lookup.poll(deadline.saturating_duration_since(Instant::now())) {
            return None;
        }
        let answer = match job.lookup.answer.take().unwrap_or(Ok(Value::Null)) {
            Ok(answer) => answer,
            Err(error) => return Some(Err(error)),
        };
        if job.step.by_kind()
            && !job.resolved
            && let Some(action) = organize_imports_action(&answer, &job.kind).filter(|action| !has_edit(action))
            && resolves(languages, job)
        {
            if let Err(error) = resolve(languages, job, action.clone()) {
                return Some(Err(error));
            }
            continue;
        }
        let applied = match apply(workbench, job, &answer) {
            Ok(changed) => changed || job.changed,
            Err(error) => return Some(Err(error)),
        };
        if job.rest.is_empty() {
            return Some(Ok(applied));
        }
        let kind = job.rest.remove(0);
        let rest = std::mem::take(&mut job.rest);
        let path = job.lookup.path.clone();
        let server = job.server.clone();
        match send_kind(languages, workbench, &FormatStyle::default(), job.step, &path, kind, rest, server) {
            Ok(next) => *job = Job { changed: applied, ..next },
            Err(error) => return Some(Err(error)),
        }
    }
}

pub(crate) fn can_resolve(languages: &mut Languages, path: &Path) -> bool {
    let provider = languages.client_for(path).and_then(|client| client.capabilities()?.code_action_provider.clone());
    matches!(provider, Some(CodeActionProviderCapability::Options(options)) if options.resolve_provider == Some(true))
}

fn client_of<'a>(languages: &'a mut Languages, job: &Job) -> Option<&'a mut zen_lsp::Client> {
    match &job.server {
        Some(id) => languages.client_of(id),
        None => languages.client_for(&job.lookup.path),
    }
}

fn resolves(languages: &mut Languages, job: &Job) -> bool {
    let provider = client_of(languages, job).and_then(|client| client.capabilities()?.code_action_provider.clone());
    matches!(provider, Some(CodeActionProviderCapability::Options(options)) if options.resolve_provider == Some(true))
}

fn resolve(languages: &mut Languages, job: &mut Job, action: Value) -> Result<(), String> {
    let client = client_of(languages, job).ok_or_else(|| t!("workbench-language-not-running"))?;
    let pending = client.request("codeAction/resolve", action).map_err(|error| error.to_string())?;
    job.lookup.request =
        Some(zen_lsp::DocumentRequest { pending, uri: job.lookup.uri.clone(), version: job.lookup.version });
    job.lookup.method = "codeAction/resolve".to_owned();
    job.resolved = true;
    Ok(())
}

fn apply(workbench: &mut Workbench, job: &Job, answer: &Value) -> Result<bool, String> {
    let document = workbench
        .documents
        .iter_mut()
        .find(|document| document.path() == job.lookup.path)
        .ok_or_else(|| t!("workbench-error-no-document"))?;
    if document.version() != job.document_version {
        return Err(t!("workbench-format-outdated"));
    }
    let edits = match job.step {
        Step::Reformat => answer.as_array().cloned().unwrap_or_default(),
        Step::OptimizeImports | Step::Fix => organize_imports_edits(answer, &job.kind, &job.lookup.path),
    };
    let edits = match &job.only_lines {
        Some(changed) => edits.into_iter().filter(|edit| touches_changed(edit, changed)).collect(),
        None => edits,
    };
    let replacements = replacements(document.text(), &edits).ok_or_else(|| t!("workbench-format-bad-edits"))?;
    let replacements = if job.keep_marked && crate::formatter_markers::has_markers(document.text()) {
        keeping_marked(document.text(), replacements)
    } else {
        replacements
    };
    let changed = !replacements.is_empty() && document.replace_ranges(replacements);
    if changed && job.step == Step::Reformat {
        document.detect_indent();
    }
    Ok(changed)
}

fn keeping_marked(text: &str, replacements: Vec<(Range<usize>, String)>) -> Vec<(Range<usize>, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut formatted = String::with_capacity(text.len());
    let mut at = 0;
    for (range, replacement) in &replacements {
        formatted.extend(&chars[at..range.start]);
        formatted.push_str(replacement);
        at = range.end;
    }
    formatted.extend(&chars[at..]);
    let new: Vec<char> = crate::formatter_markers::restore(text, &formatted).chars().collect();
    let prefix = chars.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = chars[prefix..].iter().rev().zip(new[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    if prefix == chars.len() && prefix == new.len() {
        return Vec::new();
    }
    vec![(prefix..chars.len() - suffix, new[prefix..new.len() - suffix].iter().collect())]
}

fn touches_changed(edit: &Value, changed: &[Range<usize>]) -> bool {
    let line = |end: &str| edit["range"][end]["line"].as_u64().map_or(0, |line| line as usize);
    crate::actions_on_save::touches(changed, line("start"), line("end"))
}

fn cancel(languages: &mut Languages, job: &Job) {
    if let Some(request) = &job.lookup.request
        && let Some(client) = client_of(languages, job)
    {
        client.cancel(&request.pending);
    }
}

fn has_edit(action: &Value) -> bool {
    action.get("edit").is_some_and(Value::is_object)
}

fn organize_imports_action<'a>(answer: &'a Value, kind: &str) -> Option<&'a Value> {
    let actions = match answer {
        Value::Array(actions) => actions.as_slice(),
        Value::Object(_) => std::slice::from_ref(answer),
        _ => return None,
    };
    actions.iter().find(|action| match action["kind"].as_str() {
        Some(offered) => offered == kind || offered.strip_prefix(kind).is_some_and(|rest| rest.starts_with('.')),
        None => has_edit(action),
    })
}

fn organize_imports_edits(answer: &Value, kind: &str, path: &Path) -> Vec<Value> {
    organize_imports_action(answer, kind)
        .filter(|action| has_edit(action))
        .map(|action| file_edits(&action["edit"], path))
        .unwrap_or_default()
}

fn file_edits(edit: &Value, path: &Path) -> Vec<Value> {
    let same_file = |uri: &str| zen_lsp::position::uri_path(uri).is_some_and(|other| other == path);
    let edits_of = |edits: &Value| edits.as_array().cloned().unwrap_or_default();
    if let Some(changes) = edit["documentChanges"].as_array() {
        return changes
            .iter()
            .filter(|change| change["textDocument"]["uri"].as_str().is_some_and(same_file))
            .flat_map(|change| edits_of(&change["edits"]))
            .collect();
    }
    edit["changes"]
        .as_object()
        .map(|changes| {
            changes.iter().filter(|(uri, _)| same_file(uri)).flat_map(|(_, edits)| edits_of(edits)).collect()
        })
        .unwrap_or_default()
}

fn byte_at(text: &str, position: &Value) -> Option<usize> {
    let position: lsp_types::Position = serde_json::from_value(position.clone()).ok()?;
    Some(zen_lsp::position::offset(text, position).unwrap_or(text.len()))
}

pub(crate) fn replacements(text: &str, edits: &[Value]) -> Option<Vec<(Range<usize>, String)>> {
    let mut list = Vec::with_capacity(edits.len());
    for edit in edits {
        let start = byte_at(text, &edit["range"]["start"])?;
        let end = byte_at(text, &edit["range"]["end"])?;
        let new_text = edit["newText"].as_str()?;
        if end < start {
            return None;
        }
        if text[start..end] != *new_text {
            list.push((start..end, new_text.to_owned()));
        }
    }
    list.sort_by_key(|(range, _)| (range.start, range.end));
    if list.windows(2).any(|pair| pair[0].0.end > pair[1].0.start) {
        return None;
    }
    Some(
        list.into_iter()
            .map(|(range, new_text)| (char_offset(text, range.start)..char_offset(text, range.end), new_text))
            .collect(),
    )
}

pub(crate) fn reformat(session: &mut Session, path: &Path, wait_ms: u64) -> Result<FormatResult, CommandError> {
    run_step(
        session,
        Step::Reformat,
        FormatParams { path: Some(path.to_owned()), wait_ms: Some(wait_ms), whole_file: true, rearrange: None },
    )
}

fn run_step(session: &mut Session, step: Step, params: FormatParams) -> Result<FormatResult, CommandError> {
    let wait = params.wait_ms.map_or(DEFAULT_WAIT, Duration::from_millis).min(MAX_WAIT);
    let workbench = session.state::<Workbench>();
    let path = params
        .path
        .or_else(|| workbench.and_then(Workbench::active_document).map(|document| document.path().to_owned()))
        .filter(|path| workbench.is_some_and(|workbench| workbench.documents().iter().any(|d| d.path() == path)))
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    if crate::project_settings::formatting_excluded(session, &path) {
        return Ok(FormatResult { excluded: true, ..FormatResult::default() });
    }
    let rearrange = params
        .rearrange
        .unwrap_or_else(|| session.settings().get_or(crate::code_style_options::REARRANGE_ON_REFORMAT, false));
    if step == Step::Reformat && rearrange {
        let _ = crate::code_style_options::rearrange(session, &path);
    }
    if step == Step::Reformat {
        crate::code_style_options::before_reformat(session, &path);
    }
    let style = configured_style(session, &path);
    let keep_marked = crate::project_settings::formatter_markers(session);
    if step == Step::Reformat
        && let Some(formatter) = crate::code_quality::formatter_for(session, &path, false)
    {
        return Ok(run_formatter(session, &formatter, &path, wait));
    }
    if step == Step::Reformat && crate::sql_format::is_sql(&path) {
        return Ok(crate::sql_format::reformat(session, &path, params.whole_file, wait));
    }
    let mut formatting = std::mem::take(session.state_mut::<Formatting>());
    let result = with_languages(session, |languages, workbench| {
        let slot = formatting.slot(step);
        let job = match slot.take() {
            Some(job) if job.lookup.path == path && is_current(workbench, &job) => Ok(job),
            other => {
                if let Some(old) = other {
                    cancel(languages, &old);
                }
                send(languages, workbench, &style, step, &path, keep_marked)
            }
        };
        let mut job = match job {
            Ok(job) => job,
            Err(error) => return FormatResult { error: Some(error), ..FormatResult::default() },
        };
        match finish(languages, workbench, &mut job, wait) {
            None => {
                *slot = Some(job);
                FormatResult { pending: true, ..FormatResult::default() }
            }
            Some(Ok(changed)) => FormatResult { changed, ..FormatResult::default() },
            Some(Err(error)) => FormatResult { error: Some(error), ..FormatResult::default() },
        }
    });
    session.set_state(formatting);
    Ok(result)
}

fn run_formatter(
    session: &mut Session,
    formatter: &crate::code_quality::ProjectFormatter,
    path: &Path,
    wait: Duration,
) -> FormatResult {
    let Some((text, version)) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| (document.text().to_owned(), document.version()))
    else {
        return FormatResult { error: Some(t!("workbench-error-no-document")), ..FormatResult::default() };
    };
    let mut formatting = std::mem::take(session.state_mut::<Formatting>());
    let run = match formatting.external.take() {
        Some(run) if run.path == path && run.document_version == version => run,
        _ => crate::code_quality::FormatterRun::start(formatter, path, text, version),
    };
    let result = match run.wait(wait) {
        None => {
            formatting.external = Some(run);
            FormatResult { pending: true, ..FormatResult::default() }
        }
        Some(answer) => formatted(session, &run, answer),
    };
    session.set_state(formatting);
    result
}

fn formatted(
    session: &mut Session,
    run: &crate::code_quality::FormatterRun,
    answer: Result<String, String>,
) -> FormatResult {
    let failed = |error: String| t!("workbench-format-formatter-failed", tool = run.name.as_str(), error = error);
    match answer {
        Err(error) => FormatResult { error: Some(failed(error)), ..FormatResult::default() },
        Ok(text) => {
            let text = if crate::project_settings::formatter_markers(session) {
                let old = session
                    .state::<Workbench>()
                    .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == run.path))
                    .map(|document| document.text().to_owned());
                old.map_or_else(|| text.clone(), |old| crate::formatter_markers::restore(&old, &text))
            } else {
                text
            };
            let workbench = session.state_mut::<Workbench>();
            match crate::code_quality::apply_formatted(workbench, &run.path, run.document_version, &text) {
                Ok(changed) => FormatResult { changed, ..FormatResult::default() },
                Err(error) => FormatResult { error: Some(error), ..FormatResult::default() },
            }
        }
    }
}

fn format_before_save(
    session: &mut Session,
    formatter: &crate::code_quality::ProjectFormatter,
    path: &Path,
    base: Option<&str>,
) -> Option<String> {
    let (text, version) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| (document.text().to_owned(), document.version()))?;
    let run = crate::code_quality::FormatterRun::start(formatter, path, text, version);
    match run.wait(crate::code_quality::FORMATTER_TIMEOUT) {
        None => Some(t!(
            "workbench-format-formatter-failed",
            tool = run.name.as_str(),
            error = t!("workbench-format-formatter-timeout")
        )),
        Some(answer) => {
            let answer = answer.map(|new| match base {
                Some(base) => crate::actions_on_save::keeping_changed(&run.text, &new, base),
                None => new,
            });
            formatted(session, &run, answer).error
        }
    }
}

fn apply_fixes(session: &mut Session, path: &Path, all: bool, wait: Duration) -> (FormatResult, bool) {
    let style = configured_style(session, path);
    let settings = session.settings().clone();
    with_languages(session, |languages, workbench| {
        languages.sync(workbench);
        let fixes = fixes_for(languages, &settings, path, all);
        let mut result = FormatResult::default();
        let mut answered = true;
        for (server, kind) in fixes {
            match send_kind(languages, workbench, &style, Step::Fix, path, kind, Vec::new(), Some(server)) {
                Err(error) => result.error = Some(error),
                Ok(mut job) => match finish(languages, workbench, &mut job, wait) {
                    None => {
                        cancel(languages, &job);
                        answered = false;
                    }
                    Some(Err(error)) => result.error = Some(error),
                    Some(Ok(changed)) => result.changed |= changed,
                },
            }
        }
        (result, answered)
    })
}

/// The parameters of `editor.code.fixAll`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FixAllParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// How long to wait for each linter in milliseconds, at most 10 000; 3 000 by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct FixAll;

impl Command for FixAll {
    type Params = FixAllParams;
    type Output = FormatResult;
    const ID: &'static str = "editor.code.fixAll";
    const TITLE: &'static str = "Fix Problems with Linters";
    const DESCRIPTION: &'static str = "Applies the fixes of the linters that serve an open file (the active one by default) alongside its language's server, as Fix ESLint Problems does: ESLint's source.fixAll.eslint, Biome's safe fixes; one undo step per linter. changed says whether the text changed.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| !workbench.documents().is_empty())
    }

    fn run(&self, session: &mut Session, params: FixAllParams) -> Result<FormatResult, CommandError> {
        let wait = params.wait_ms.map_or(Duration::from_secs(3), Duration::from_millis).min(MAX_WAIT);
        let workbench = session.state::<Workbench>();
        let path = params
            .path
            .or_else(|| workbench.and_then(Workbench::active_document).map(|document| document.path().to_owned()))
            .filter(|path| workbench.is_some_and(|workbench| workbench.documents().iter().any(|d| d.path() == path)))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let (result, answered) = apply_fixes(session, &path, true, wait);
        Ok(FormatResult { pending: !answered && result.error.is_none(), ..result })
    }
}

fn options(style: CodeStyle) -> Value {
    let size = if style.use_tabs { style.tab_size } else { style.indent };
    json!({ "tabSize": size, "insertSpaces": !style.use_tabs })
}

fn declared_options(style: &FormatStyle) -> Value {
    let mut options = options(style.indent);
    if let Some(object) = options.as_object_mut() {
        for (key, value) in &style.declared {
            object.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    options
}

fn is_current(workbench: &Workbench, job: &Job) -> bool {
    workbench
        .documents()
        .iter()
        .any(|document| document.path() == job.lookup.path && document.version() == job.document_version)
}

pub fn before_save(session: &mut Session, path: &Path) -> bool {
    let (answered, message) = before_save_noting(session, path);
    if let Some(message) = message {
        session.state_mut::<Workbench>().messages.push(message);
    }
    crate::on_save::before_save(session, path);
    answered
}

pub(crate) fn before_save_noting(session: &mut Session, path: &Path) -> (bool, Option<String>) {
    let dirty = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .is_some_and(|document| document.is_dirty() && document.image().is_none());
    if !dirty {
        return (true, None);
    }
    let (fixed, fixes_answered) = apply_fixes(session, path, false, SAVE_WAIT);
    if crate::project_settings::formatting_excluded(session, path) {
        return (fixes_answered, fixed.error);
    }
    let actions = crate::actions_on_save::of(session);
    let reformat = actions.reformats(path);
    let formatter = crate::code_quality::formatter_for(session, path, true)
        .or_else(|| reformat.then(|| crate::code_quality::formatter_for(session, path, false)).flatten());
    let replaces_reformat = formatter.as_ref().is_some_and(|formatter| !formatter.declared.fixer);
    let (answered, message) = server_steps_before_save(session, path, &actions, replaces_reformat);
    let message = message.or(fixed.error);
    if reformat && formatter.is_none() && crate::sql_format::is_sql(path) {
        let result = crate::sql_format::reformat(session, path, true, SAVE_WAIT);
        if result.pending {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            return (false, Some(t!("workbench-format-sql-timeout", file = name.as_str())));
        }
        return (answered && fixes_answered, message.or(result.error));
    }
    if let Some(formatter) = formatter
        && let Some(error) = {
            let base = if formatter.declared.fixer { None } else { changed_lines_base(session, path, &actions) };
            format_before_save(session, &formatter, path, base.as_deref())
        }
    {
        return (answered && fixes_answered, Some(error));
    }
    (answered && fixes_answered, message)
}

fn server_steps_before_save(
    session: &mut Session,
    path: &Path,
    actions: &ActionsOnSave,
    formatter: bool,
) -> (bool, Option<String>) {
    let steps: Vec<Step> =
        [(actions.optimizes(path), Step::OptimizeImports), (actions.reformats(path), Step::Reformat)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, step)| step)
            .filter(|step| !(formatter && *step == Step::Reformat))
            .collect();
    if steps.is_empty() {
        return (true, None);
    }
    let base = changed_lines_base(session, path, actions);
    if steps.contains(&Step::Reformat) {
        crate::code_style_options::before_reformat(session, path);
    }
    let style = configured_style(session, path);
    let keep_marked = crate::project_settings::formatter_markers(session);
    with_languages(session, |languages, workbench| {
        let Some(document) = workbench.documents().iter().find(|document| document.path() == path) else {
            return (true, None);
        };
        if !document.is_dirty() || document.image().is_some() || !languages.serves(path) {
            return (true, None);
        }
        let name = document.name();
        for step in steps {
            let failed = |error: String| t!("workbench-format-failed", file = name.as_str(), error = error);
            let (message, answered) = match send(languages, workbench, &style, step, path, keep_marked) {
                Err(error) => (Some(failed(error)), true),
                Ok(mut job) => {
                    if step == Step::Reformat
                        && let Some(base) = &base
                        && let Some(document) = workbench.documents().iter().find(|d| d.path() == path)
                    {
                        job.only_lines = Some(crate::actions_on_save::changed_lines(base, document.text()));
                    }
                    match finish(languages, workbench, &mut job, SAVE_WAIT) {
                        None => {
                            cancel(languages, &job);
                            (Some(t!("workbench-format-timeout", file = name.as_str())), false)
                        }
                        Some(Err(error)) => (Some(failed(error)), true),
                        Some(Ok(_)) => (None, true),
                    }
                }
            };
            if message.is_some() {
                return (answered, message);
            }
        }
        (true, None)
    })
}

fn changed_lines_base(session: &mut Session, path: &Path, actions: &ActionsOnSave) -> Option<String> {
    (actions.reformat_scope == ReformatScope::ChangedLines)
        .then(|| crate::vcs::commit::head_text(session, path))
        .flatten()
}

pub struct FormatCode;

impl Command for FormatCode {
    type Params = FormatParams;
    type Output = FormatResult;
    const ID: &'static str = "editor.code.reformat";
    const TITLE: &'static str = "Reformat Code";
    const DESCRIPTION: &'static str = "Reformats a whole open file (the active one by default) as its language server does \
         (gofmt for Go), as one undo step, waiting up to waitMs (1 000 by default) for the server; when it is still \
         pending, running the command again waits on. An answer for text that changed meanwhile is dropped. SQL files \
         and consoles are formatted by the IDE itself with Code Style → SQL in the dialect of their connection; with a \
         selection in the active one only the statements it touches, unless wholeFile.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| !workbench.documents().is_empty())
    }

    fn run(&self, session: &mut Session, params: FormatParams) -> Result<FormatResult, CommandError> {
        run_step(session, Step::Reformat, params)
    }
}

pub struct OptimizeImports;

impl Command for OptimizeImports {
    type Params = FormatParams;
    type Output = FormatResult;
    const ID: &'static str = "editor.imports.optimize";
    const TITLE: &'static str = "Optimize Imports";
    const DESCRIPTION: &'static str = "Organizes the imports of an open file (the active one by default) with the language \
         server's source.organizeImports action (goimports for Go), as one undo step, waiting up to waitMs (1 000 by \
         default); when it is still pending, running the command again waits on. An answer for text that changed \
         meanwhile is dropped.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| !workbench.documents().is_empty())
    }

    fn run(&self, session: &mut Session, params: FormatParams) -> Result<FormatResult, CommandError> {
        run_step(session, Step::OptimizeImports, params)
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(FormatCode)?;
    registry.register(OptimizeImports)?;
    registry.register(FixAll)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use zen_lsp::fake::Reply;
    use zen_project::settings::CodeStyleOverride;

    use super::*;
    use crate::language::ServerSpec;
    use crate::language::testing::{Fixture, fixture};
    use crate::project_settings;

    fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> Value {
        json!({
            "range": {
                "start": { "line": start.0, "character": start.1 },
                "end": { "line": end.0, "character": end.1 }
            },
            "newText": text
        })
    }

    fn text(fixture: &Fixture) -> String {
        fixture.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn change(fixture: &mut Fixture, at: usize, insert: &str) {
        fixture.with(|_, workbench| {
            workbench.active_document_mut().unwrap().replace_ranges(vec![(at..at, insert.to_owned())]);
        });
    }

    fn result(value: Value) -> FormatResult {
        FormatResult {
            changed: value["changed"].as_bool().unwrap(),
            pending: value["pending"].as_bool().unwrap(),
            error: value["error"].as_str().map(str::to_owned),
            excluded: value["excluded"].as_bool().unwrap(),
        }
    }

    const UNFORMATTED: &str = "package main\n\n// привет 😀\nfunc  main()  {\n}\n";

    #[test]
    fn reformat_applies_the_edits_as_one_undo_step_after_non_ascii_text() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.on(
            "textDocument/formatting",
            json!([edit((3, 4), (3, 6), " "), edit((2, 12), (2, 12), "!"), edit((3, 12), (3, 14), " ")]),
        );
        let done = fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(result(done), FormatResult { changed: true, ..FormatResult::default() });
        assert_eq!(text(&fixture), "package main\n\n// привет 😀!\nfunc main() {\n}\n");
        let sent = &fixture.server.requests("textDocument/formatting")[0];
        assert_eq!(sent["options"], json!({ "tabSize": 4, "insertSpaces": false }));
        assert!(sent["textDocument"]["uri"].as_str().unwrap().ends_with("/main.go"));
        assert_eq!(fixture.dispatch("editor.edit.undo", json!({})).unwrap(), json!(true));
        assert_eq!(text(&fixture), UNFORMATTED);
    }

    #[test]
    fn do_not_format_leaves_a_matching_file_alone_on_reformat_imports_and_save() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        script_organize_imports(&fixture);
        fixture.server.on("textDocument/formatting", json!([edit((7, 4), (7, 6), " ")]));
        fixture.session.settings_mut().set(crate::code_style::DO_NOT_FORMAT, ["*.go"]).unwrap();
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        fixture.session.settings_mut().set(OPTIMIZE_IMPORTS_ON_SAVE, true).unwrap();
        for command in ["editor.code.reformat", "editor.imports.optimize"] {
            let done = fixture.dispatch(command, json!({ "waitMs": 5000 })).unwrap();
            assert_eq!(result(done), FormatResult { excluded: true, ..FormatResult::default() }, "{command}");
        }
        change(&mut fixture, IMPORTS.chars().count(), "// end\n");
        let (on_disk, messages) = save(&mut fixture);
        assert_eq!(on_disk, format!("{IMPORTS}// end\n"));
        assert!(messages.is_empty(), "{messages:?}");
        assert!(fixture.server.requests("textDocument/formatting").is_empty());
        assert!(fixture.server.requests("textDocument/codeAction").is_empty());
    }

    #[test]
    fn do_not_format_takes_a_star_for_every_file_and_a_path_inside_the_project() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        let root = fixture.session.state::<Workbench>().unwrap().project().unwrap().to_owned();
        let excluded = |fixture: &mut Fixture, patterns: &[&str], file: &str| {
            fixture.session.settings_mut().set(crate::code_style::DO_NOT_FORMAT, patterns).unwrap();
            project_settings::formatting_excluded(&fixture.session, &root.join(file))
        };
        assert!(excluded(&mut fixture, &["*"], "main.go"));
        assert!(excluded(&mut fixture, &["gen/*.go"], "gen/a.go"));
        assert!(!excluded(&mut fixture, &["gen/*.go"], "main.go"));
        assert!(!excluded(&mut fixture, &["*.rs", " "], "main.go"));
    }

    #[test]
    fn the_formatter_markers_keep_the_text_between_them_when_they_are_on() {
        let source = "a  b\n// @formatter:off\nc  d\n// @formatter:on\ne  f\n";
        let mut fixture = fixture(&[("main.go", source)]);
        fixture.open("main.go");
        let spaces = |line: u32| edit((line, 1), (line, 3), " ");
        fixture.server.on("textDocument/formatting", json!([spaces(0), spaces(2), spaces(4)]));
        fixture.session.settings_mut().set(crate::code_style::FORMATTER_MARKERS, true).unwrap();
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(text(&fixture), "a b\n// @formatter:off\nc  d\n// @formatter:on\ne f\n");
        fixture.dispatch("editor.edit.undo", json!({})).unwrap();
        assert_eq!(text(&fixture), source, "one undo step");
        fixture.session.settings_mut().set(crate::code_style::FORMATTER_MARKERS, false).unwrap();
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(text(&fixture), "a b\n// @formatter:off\nc d\n// @formatter:on\ne f\n");
    }

    struct GoStyle;

    impl zen_project::settings::SettingsProvider for GoStyle {
        fn id(&self) -> &str {
            "go-style"
        }

        fn watches(&self, _: &Path) -> bool {
            false
        }

        fn load(
            &self,
            _: &Path,
            _: &dyn zen_project::settings::ReadFiles,
        ) -> Option<zen_project::settings::ProjectSettings> {
            let go = CodeStyleOverride { use_tabs: Some(false), indent: Some(2), ..Default::default() };
            let mut settings = zen_project::settings::ProjectSettings::default();
            settings.code_style.insert("go".to_owned(), go);
            Some(settings)
        }
    }

    #[test]
    fn the_project_code_style_gives_the_reformat_options() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        let root = fixture.path("");
        let state = fixture.session.state_mut::<project_settings::ProjectSettingsState>();
        state.set_providers(vec![Arc::new(GoStyle)]);
        state.refresh(Some(&root));
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([]));
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        let sent = &fixture.server.requests("textDocument/formatting")[0];
        assert_eq!(sent["options"], json!({ "tabSize": 2, "insertSpaces": true }));
    }

    #[test]
    fn editorconfig_goes_over_the_project_code_style_in_the_reformat_options() {
        let config = "[*.go]\nindent_style = tab\ntab_width = 8\n";
        let mut fixture = fixture(&[("main.go", UNFORMATTED), (".editorconfig", config)]);
        let root = fixture.path("");
        let state = fixture.session.state_mut::<project_settings::ProjectSettingsState>();
        state.set_providers(vec![Arc::new(GoStyle)]);
        state.refresh(Some(&root));
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([]));
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        let sent = &fixture.server.requests("textDocument/formatting")[0];
        assert_eq!(sent["options"], json!({ "tabSize": 8, "insertSpaces": false }));
        fixture.session.settings_mut().set(crate::editor_config::ENABLED, false).unwrap();
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        let sent = &fixture.server.requests("textDocument/formatting")[1];
        assert_eq!(sent["options"], json!({ "tabSize": 2, "insertSpaces": true }), "off: the project's style");
    }

    #[test]
    fn reformat_keeps_the_configured_style_of_a_file_indented_otherwise() {
        let mut fixture = fixture(&[("main.go", "package main\n\nfunc main() {\n    a()\n    b()\n    c()\n}\n")]);
        fixture.open("main.go");
        let workbench = fixture.session.state::<Workbench>().unwrap();
        let document = workbench.active_document().unwrap();
        assert_eq!(project_settings::indent_unit_of(&fixture.session, document), "    ", "Tab follows the file");
        fixture.server.on("textDocument/formatting", json!([]));
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        let sent = &fixture.server.requests("textDocument/formatting")[0];
        assert_eq!(sent["options"], json!({ "tabSize": 4, "insertSpaces": false }));
    }

    #[test]
    fn after_gofmt_turns_spaces_into_tabs_tab_types_a_tab() {
        let mut fixture = fixture(&[("main.go", "package main\n\nfunc main() {\n    a()\n    b()\n    c()\n}\n")]);
        fixture.open("main.go");
        let gofmt = json!([edit((3, 0), (3, 4), "\t"), edit((4, 0), (4, 4), "\t"), edit((5, 0), (5, 4), "\t")]);
        fixture.server.on("textDocument/formatting", gofmt);
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        let workbench = fixture.session.state::<Workbench>().unwrap();
        let document = workbench.active_document().unwrap();
        assert_eq!(document.text(), "package main\n\nfunc main() {\n\ta()\n\tb()\n\tc()\n}\n");
        assert_eq!(document.detected_indent(), Some(crate::indent_detection::DetectedIndent::Tabs));
        assert_eq!(project_settings::indent_unit_of(&fixture.session, document), "\t");
    }

    #[test]
    fn reformat_sends_the_configured_code_style() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([]));
        let mut styles = std::collections::BTreeMap::new();
        styles.insert("go".to_owned(), CodeStyle { use_tabs: false, tab_size: 8, indent: 2 });
        crate::code_style::apply(fixture.session.settings_mut(), &styles);
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        styles.insert("go".to_owned(), CodeStyle { use_tabs: true, tab_size: 8, indent: 2 });
        crate::code_style::apply(fixture.session.settings_mut(), &styles);
        fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        let sent = fixture.server.requests("textDocument/formatting");
        assert_eq!(sent[0]["options"], json!({ "tabSize": 2, "insertSpaces": true }));
        assert_eq!(sent[1]["options"], json!({ "tabSize": 8, "insertSpaces": false }));
    }

    #[test]
    fn options_follow_the_code_style() {
        let style = |use_tabs, tab_size, indent| CodeStyle { use_tabs, tab_size, indent };
        assert_eq!(options(style(true, 4, 4)), json!({ "tabSize": 4, "insertSpaces": false }));
        assert_eq!(options(style(false, 4, 2)), json!({ "tabSize": 2, "insertSpaces": true }));
    }

    #[test]
    fn an_empty_or_null_answer_changes_nothing() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        for answer in [json!([]), Value::Null] {
            fixture.server.on("textDocument/formatting", answer);
            let done = fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
            assert_eq!(result(done), FormatResult::default());
            assert_eq!(text(&fixture), UNFORMATTED);
            assert!(!fixture.session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());
        }
    }

    #[test]
    fn a_pending_reformat_is_finished_by_running_it_again() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        let edits = json!([edit((3, 4), (3, 6), " ")]);
        fixture.server.reply("textDocument/formatting", Reply::Delayed(edits, Duration::from_millis(300)));
        let first = fixture.dispatch("editor.code.reformat", json!({ "waitMs": 0 })).unwrap();
        assert_eq!(result(first), FormatResult { pending: true, ..FormatResult::default() });
        let second = fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(result(second), FormatResult { changed: true, ..FormatResult::default() });
        assert_eq!(fixture.server.requests("textDocument/formatting").len(), 1);
        assert!(text(&fixture).contains("func main()"));
    }

    #[test]
    fn an_answer_for_text_that_changed_meanwhile_is_dropped() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        let edits = json!([edit((3, 4), (3, 6), " ")]);
        fixture.server.reply("textDocument/formatting", Reply::Delayed(edits, Duration::from_millis(200)));
        let first = fixture.dispatch("editor.code.reformat", json!({ "waitMs": 0 })).unwrap();
        assert!(result(first).pending);
        change(&mut fixture, 0, "// x\n");
        std::thread::sleep(Duration::from_millis(400));
        fixture.server.on("textDocument/formatting", json!([]));
        let second = fixture.dispatch("editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(result(second), FormatResult::default());
        assert_eq!(text(&fixture), format!("// x\n{UNFORMATTED}"));
        assert_eq!(fixture.server.requests("textDocument/formatting").len(), 2);
    }

    #[test]
    fn the_job_of_a_changed_file_reports_it_as_outdated() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([edit((3, 4), (3, 6), " ")]));
        let path = fixture.path("main.go");
        let outcome = fixture.with(|languages, workbench| {
            let mut job = send(
                languages,
                workbench,
                &FormatStyle { indent: CodeStyle::default_for("go"), ..FormatStyle::default() },
                Step::Reformat,
                &path,
                false,
            )
            .unwrap();
            workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "// x\n".to_owned())]);
            finish(languages, workbench, &mut job, Duration::from_secs(5))
        });
        assert_eq!(outcome, Some(Err(t!("workbench-format-outdated"))));
        assert_eq!(text(&fixture), format!("// x\n{UNFORMATTED}"));
    }

    const IMPORTS: &str =
        "package main\n\nimport (\n\t\"os\"\n\t\"fmt\"\n)\n\nfunc  main() {\n\tfmt.Println(os.Args)\n}\n";
    const ORGANIZED: &str =
        "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n\nfunc  main() {\n\tfmt.Println(os.Args)\n}\n";

    fn script_organize_imports(fixture: &Fixture) {
        fixture.server.on_with("textDocument/codeAction", |params| {
            let uri = params["textDocument"]["uri"].clone();
            let edits = json!([edit((3, 0), (5, 0), "\t\"fmt\"\n\t\"os\"\n")]);
            json!([
                { "title": "Run a command", "command": "gopls.tidy", "arguments": [] },
                {
                    "title": "Organize Imports",
                    "kind": "source.organizeImports",
                    "edit": { "documentChanges": [
                        { "textDocument": { "uri": "file:///elsewhere/other.go", "version": 1 }, "edits": [edit((0, 0), (0, 0), "x")] },
                        { "textDocument": { "uri": uri, "version": 1 }, "edits": edits }
                    ] },
                    "command": { "title": "ignored", "command": "gopls.ignored" }
                }
            ])
        });
    }

    #[test]
    fn optimize_imports_asks_for_organize_imports_only_and_applies_its_edit() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        script_organize_imports(&fixture);
        let done = fixture.dispatch("editor.imports.optimize", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(result(done), FormatResult { changed: true, ..FormatResult::default() });
        assert_eq!(text(&fixture), ORGANIZED);
        let sent = &fixture.server.requests("textDocument/codeAction")[0];
        assert_eq!(sent["context"]["only"], json!(["source.organizeImports"]));
        assert_eq!(sent["range"]["start"], json!({ "line": 0, "character": 0 }));
        assert_eq!(sent["range"]["end"], json!({ "line": 10, "character": 0 }));
        assert!(fixture.server.requests("workspace/executeCommand").is_empty());
        fixture.dispatch("editor.edit.undo", json!({})).unwrap();
        assert_eq!(text(&fixture), IMPORTS);
    }

    #[test]
    fn optimize_imports_asks_for_each_kind_in_turn_on_the_text_the_one_before_left() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        fixture.server.on_with("textDocument/codeAction", |params| {
            let uri = params["textDocument"]["uri"].clone();
            let (kind, line) = match params["context"]["only"][0].as_str() {
                Some("source.removeUnusedImports") => ("source.removeUnusedImports.ts", "// unused gone\n"),
                _ => ("source.organizeImports.ts", "// sorted\n"),
            };
            json!([{ "title": kind, "kind": kind, "edit": { "changes": { uri.as_str().unwrap(): [edit((0, 0), (0, 0), line)] } } }])
        });
        let path = fixture.path("main.go");
        let outcome = fixture.with(|languages, workbench| {
            let kinds = vec![ORGANIZE_IMPORTS.to_owned()];
            let mut job = send_kind(
                languages,
                workbench,
                &FormatStyle::default(),
                Step::OptimizeImports,
                &path,
                "source.removeUnusedImports".to_owned(),
                kinds,
                None,
            )
            .unwrap();
            finish(languages, workbench, &mut job, Duration::from_secs(5))
        });
        assert_eq!(outcome, Some(Ok(true)));
        assert_eq!(text(&fixture), format!("// sorted\n// unused gone\n{IMPORTS}"));
        let asked: Vec<Value> = fixture
            .server
            .requests("textDocument/codeAction")
            .iter()
            .map(|sent| sent["context"]["only"].clone())
            .collect();
        assert_eq!(asked, [json!(["source.removeUnusedImports"]), json!(["source.organizeImports"])]);
    }

    #[test]
    fn an_organize_imports_action_without_its_edit_is_resolved_first() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
            "codeActionProvider": { "codeActionKinds": ["source.organizeImports"], "resolveProvider": true }
        }));
        fixture.open("main.go");
        let uri = zen_lsp::position::file_uri(&fixture.path("main.go"));
        fixture.server.on(
            "textDocument/codeAction",
            json!([{ "title": "Organize Imports", "kind": "source.organizeImports", "data": { "id": 7 } }]),
        );
        let edits = json!([edit((3, 0), (5, 0), "\t\"fmt\"\n\t\"os\"\n")]);
        fixture.server.on_with("codeAction/resolve", move |action| {
            let mut action = action.clone();
            action["edit"] = json!({ "changes": { uri.clone(): edits.clone() } });
            action
        });
        let done = fixture.dispatch("editor.imports.optimize", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(result(done), FormatResult { changed: true, ..FormatResult::default() });
        assert_eq!(text(&fixture), ORGANIZED);
        assert_eq!(fixture.server.requests("codeAction/resolve")[0]["data"], json!({ "id": 7 }));
    }

    #[test]
    fn an_action_without_an_edit_changes_nothing_when_the_server_cannot_resolve_it() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        fixture
            .server
            .on("textDocument/codeAction", json!([{ "title": "Organize Imports", "kind": "source.organizeImports" }]));
        let done = fixture.dispatch("editor.imports.optimize", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(result(done), FormatResult::default());
        assert_eq!(text(&fixture), IMPORTS);
        assert!(fixture.server.requests("codeAction/resolve").is_empty());
    }

    #[test]
    fn workspace_edits_are_kept_to_the_file() {
        let path = Path::new("/project/main.go");
        let edits = json!([edit((0, 0), (0, 1), "q")]);
        let changes = json!({ "changes": { "file:///project/main.go": edits, "file:///project/other.go": [edit((0, 0), (0, 0), "x")] } });
        assert_eq!(file_edits(&changes, path), edits.as_array().unwrap().clone());
        let answer = json!([{ "title": "Fix", "kind": "quickfix", "edit": changes }]);
        assert!(organize_imports_edits(&answer, ORGANIZE_IMPORTS, path).is_empty());
        let typescript = json!([{ "title": "Organize Imports", "kind": "source.organizeImports.ts", "edit": changes }]);
        assert_eq!(organize_imports_edits(&typescript, ORGANIZE_IMPORTS, path).len(), 1, "a sub-kind counts");
        assert!(organize_imports_edits(&typescript, "source.organize", path).is_empty(), "a prefix of a word does not");
    }

    #[test]
    fn edits_become_sorted_character_ranges() {
        let text = "é😀ab\ncd\n";
        let edits = [edit((1, 0), (1, 1), "X"), edit((0, 3), (0, 4), "Y"), edit((0, 4), (0, 5), "b")];
        assert_eq!(replacements(text, &edits), Some(vec![(2..3, "Y".to_owned()), (5..6, "X".to_owned())]));
        let overlapping = [edit((0, 0), (0, 4), "Z"), edit((0, 3), (0, 5), "W")];
        assert_eq!(replacements(text, &overlapping), None);
        assert_eq!(replacements(text, &[json!({ "newText": "x" })]), None);
    }

    fn save(fixture: &mut Fixture) -> (String, Vec<String>) {
        fixture.dispatch("file.document.save", json!({})).unwrap();
        let on_disk = std::fs::read_to_string(fixture.path("main.go")).unwrap();
        (on_disk, fixture.session.state_mut::<Workbench>().take_messages())
    }

    #[test]
    fn a_save_leaves_the_text_alone_while_the_settings_are_off() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        script_organize_imports(&fixture);
        fixture.server.on("textDocument/formatting", json!([edit((7, 4), (7, 6), " ")]));
        change(&mut fixture, IMPORTS.chars().count(), "// end\n");
        let (on_disk, messages) = save(&mut fixture);
        assert_eq!(on_disk, format!("{IMPORTS}// end\n"));
        assert!(messages.is_empty());
        assert!(fixture.server.requests("textDocument/formatting").is_empty());
        assert!(fixture.server.requests("textDocument/codeAction").is_empty());
    }

    #[test]
    fn a_save_reformats_only_the_files_of_the_chosen_languages() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([edit((7, 4), (7, 6), " ")]));
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        fixture.session.settings_mut().set(crate::actions_on_save::REFORMAT_FILES, ["rust"]).unwrap();
        change(&mut fixture, IMPORTS.chars().count(), "// end\n");
        let (on_disk, _) = save(&mut fixture);
        assert_eq!(on_disk, format!("{IMPORTS}// end\n"));
        assert!(fixture.server.requests("textDocument/formatting").is_empty());
    }

    #[test]
    fn a_save_takes_the_projects_own_actions_over_the_defaults() {
        let mut fixture = fixture(&[("main.go", IMPORTS), (crate::actions_on_save::FILE, "{ \"reformat\": false }")]);
        fixture.open("main.go");
        let root = fixture.path("");
        crate::trust::trust_session_in_tests(&mut fixture.session, &root);
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        change(&mut fixture, IMPORTS.chars().count(), "// end\n");
        save(&mut fixture);
        assert!(fixture.server.requests("textDocument/formatting").is_empty());
    }

    #[test]
    fn an_edit_of_a_line_that_did_not_change_is_dropped() {
        let changed = vec![2..3, 20..21];
        let on_line = |line: u32| edit((line, 0), (line, 1), " ");
        assert!(touches_changed(&on_line(2), &changed));
        assert!(!touches_changed(&on_line(0), &changed));
        assert!(touches_changed(&edit((0, 0), (9, 0), ""), &changed), "a whole-file edit stays");
    }

    #[test]
    fn a_save_optimizes_imports_then_reformats_when_the_settings_are_on() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        script_organize_imports(&fixture);
        fixture.server.on("textDocument/formatting", json!([edit((7, 4), (7, 6), " ")]));
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        fixture.session.settings_mut().set(OPTIMIZE_IMPORTS_ON_SAVE, true).unwrap();
        change(&mut fixture, IMPORTS.chars().count(), "// end\n");
        let (on_disk, messages) = save(&mut fixture);
        assert_eq!(on_disk, format!("{}// end\n", ORGANIZED.replace("func  main", "func main")));
        assert!(messages.is_empty(), "{messages:?}");
        assert!(!fixture.session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());
        let methods: Vec<String> = fixture
            .server
            .received()
            .iter()
            .filter_map(|message| message["method"].as_str().map(str::to_owned))
            .collect();
        let at = |method: &str| methods.iter().position(|other| other == method).unwrap();
        assert!(at("textDocument/codeAction") < at("textDocument/formatting"));
    }

    #[test]
    fn a_save_all_formats_every_modified_file() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([edit((4, 4), (4, 6), " ")]));
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        change(&mut fixture, 0, "// x\n");
        fixture.dispatch("file.document.saveAll", json!({})).unwrap();
        let on_disk = std::fs::read_to_string(fixture.path("main.go")).unwrap();
        assert_eq!(on_disk, format!("// x\n{}", UNFORMATTED.replace("func  main", "func main")));
    }

    #[test]
    fn closing_a_tab_formats_its_file_before_saving_as_save_all_does() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([edit((4, 4), (4, 6), " ")]));
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        change(&mut fixture, 0, "// x\n");
        fixture.dispatch("file.document.close", json!({})).unwrap();
        let on_disk = std::fs::read_to_string(fixture.path("main.go")).unwrap();
        assert_eq!(on_disk, format!("// x\n{}", UNFORMATTED.replace("func  main", "func main")));
    }

    #[test]
    fn a_save_goes_ahead_with_a_message_when_the_server_does_not_answer_in_time() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.reply("textDocument/formatting", Reply::Hold);
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        change(&mut fixture, 0, "// x\n");
        let started = Instant::now();
        let (on_disk, messages) = save(&mut fixture);
        assert!(started.elapsed() < SAVE_WAIT + Duration::from_secs(60));
        assert_eq!(on_disk, format!("// x\n{UNFORMATTED}"));
        assert_eq!(messages, vec![t!("workbench-format-timeout", file = "main.go")]);
        assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, Duration::from_secs(5)).len(), 1);
    }

    #[test]
    fn autosave_runs_the_actions_on_save_as_save_all_does() {
        let mut fixture = fixture(&[("main.go", IMPORTS)]);
        fixture.open("main.go");
        script_organize_imports(&fixture);
        fixture.server.on("textDocument/formatting", json!([edit((7, 4), (7, 6), " ")]));
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        fixture.session.settings_mut().set(OPTIMIZE_IMPORTS_ON_SAVE, true).unwrap();
        change(&mut fixture, IMPORTS.chars().count(), "// end\n");
        fixture.dispatch("file.document.autosave", json!({})).unwrap();
        let on_disk = std::fs::read_to_string(fixture.path("main.go")).unwrap();
        assert_eq!(on_disk, format!("{}// end\n", ORGANIZED.replace("func  main", "func main")));
    }

    #[test]
    fn a_file_changed_outside_is_neither_formatted_nor_written_on_save() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.on("textDocument/formatting", json!([edit((0, 0), (0, 0), "// formatted\n")]));
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        change(&mut fixture, 0, "// x\n");
        std::fs::write(fixture.path("main.go"), "// theirs\n").unwrap();
        for command in ["file.document.save", "file.document.saveAll"] {
            assert!(fixture.dispatch(command, json!({})).is_err(), "{command}");
        }
        assert_eq!(std::fs::read_to_string(fixture.path("main.go")).unwrap(), "// theirs\n");
        let workbench = fixture.session.state::<Workbench>().unwrap();
        assert_eq!(workbench.active_document().unwrap().text(), format!("// x\n{UNFORMATTED}"), "not formatted");
        assert!(fixture.server.requests("textDocument/formatting").is_empty());
    }

    #[test]
    fn a_failing_formatter_does_not_stop_autosave() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture
            .server
            .reply("textDocument/formatting", Reply::Error { code: -32603, message: "gofmt: syntax error".to_owned() });
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        change(&mut fixture, 0, "// x\n");
        let done = fixture.dispatch("file.document.autosave", json!({})).unwrap();
        assert_eq!(done["saved"].as_array().unwrap().len(), 1, "{done}");
        let on_disk = std::fs::read_to_string(fixture.path("main.go")).unwrap();
        assert_eq!(on_disk, format!("// x\n{UNFORMATTED}"));
        let messages = fixture.session.state_mut::<Workbench>().take_messages();
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("main.go") && messages[0].contains("gofmt: syntax error"), "{messages:?}");

        change(&mut fixture, 0, "// y\n");
        let again = fixture.dispatch("file.document.autosave", json!({})).unwrap();
        assert_eq!(again["saved"].as_array().unwrap().len(), 1, "{again}");
        let messages = fixture.session.state_mut::<Workbench>().take_messages();
        assert!(messages.is_empty(), "the next autosave does not notify the same failure again: {messages:?}");

        change(&mut fixture, 0, "// z\n");
        fixture.dispatch("file.document.saveAll", json!({})).unwrap();
        let messages = fixture.session.state_mut::<Workbench>().take_messages();
        assert_eq!(messages.len(), 1, "an explicit save always tells: {messages:?}");
    }

    #[test]
    fn autosave_goes_ahead_when_the_formatter_does_not_answer_in_time() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.server.reply("textDocument/formatting", Reply::Hold);
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        change(&mut fixture, 0, "// x\n");
        let started = Instant::now();
        fixture.dispatch("file.document.autosave", json!({})).unwrap();
        assert!(started.elapsed() < SAVE_WAIT + Duration::from_secs(60));
        let on_disk = std::fs::read_to_string(fixture.path("main.go")).unwrap();
        assert_eq!(on_disk, format!("// x\n{UNFORMATTED}"));
    }

    #[test]
    fn a_save_all_stops_formatting_once_the_server_does_not_answer() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED), ("lib.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.open("lib.go");
        fixture.server.reply("textDocument/formatting", Reply::Hold);
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        fixture.with(|_, workbench| {
            for document in &mut workbench.documents {
                document.replace_ranges(vec![(0..0, "// x  \n".to_owned())]);
            }
        });
        fixture.dispatch("file.document.saveAll", json!({})).unwrap();
        assert_eq!(fixture.server.requests("textDocument/formatting").len(), 1);
        for name in ["main.go", "lib.go"] {
            assert_eq!(std::fs::read_to_string(fixture.path(name)).unwrap(), format!("// x\n{UNFORMATTED}"));
        }
    }

    #[test]
    fn a_save_goes_ahead_with_a_message_without_a_server() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED)]);
        fixture.session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(|_| Err("gopls is not installed".to_owned())),
            initialization_options: json!({}),
        }]);
        let path = fixture.path("main.go");
        fixture.dispatch("file.document.open", json!({ "path": path })).unwrap();
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        fixture.session.settings_mut().set(OPTIMIZE_IMPORTS_ON_SAVE, true).unwrap();
        change(&mut fixture, 0, "// x\n");
        let (on_disk, messages) = save(&mut fixture);
        assert_eq!(on_disk, format!("// x\n{UNFORMATTED}"));
        let reason = t!("workbench-language-not-running");
        assert_eq!(messages, vec![t!("workbench-format-failed", file = "main.go", error = reason)]);
        let unserved = fixture.dispatch("editor.code.reformat", json!({ "waitMs": 0 })).unwrap();
        assert_eq!(result(unserved).error, Some(t!("workbench-language-not-running")));
    }
    #[test]
    fn a_save_all_stops_formatting_once_the_server_does_not_answer_but_cleans_every_file() {
        let mut fixture = fixture(&[("main.go", UNFORMATTED), ("lib.go", UNFORMATTED)]);
        fixture.open("main.go");
        fixture.open("lib.go");
        fixture.server.reply("textDocument/formatting", Reply::Hold);
        fixture.session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        fixture.with(|_, workbench| {
            for document in &mut workbench.documents {
                document.replace_ranges(vec![(0..0, "// x  \n".to_owned())]);
            }
        });
        fixture.dispatch("file.document.saveAll", json!({})).unwrap();
        assert_eq!(fixture.server.requests("textDocument/formatting").len(), 1);
        for name in ["main.go", "lib.go"] {
            assert_eq!(std::fs::read_to_string(fixture.path(name)).unwrap(), format!("// x\n{UNFORMATTED}"));
        }
    }
}

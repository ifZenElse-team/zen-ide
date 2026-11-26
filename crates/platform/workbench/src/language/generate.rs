use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::{LspError, MemberList, PromptGenerator};
use zen_modules::generate::{Chooser, ChooserOption, Chosen, CodeGenerator, CodeGenerators, GenerateContext, Member};

use super::edits::Applied;
use super::intentions::{self, IntentionApplyParams, Intentions, IntentionsListParams};
use super::refactor::Refactoring;
use super::{Languages, MAX_WAIT, with_languages};
use crate::Workbench;

const OWNER: &str = "generate";

const SOURCE: &str = "source";

#[derive(Default)]
pub struct Generating {
    actions: Intentions,
    generation: u64,
    listed: Option<Listed>,
    asking: Option<Asking>,
}

struct Listed {
    generation: u64,
    path: PathBuf,
    document_version: u64,
    offset: usize,
    entries: Vec<Entry>,
}

#[derive(Clone)]
enum Source {
    Provider { generator: Arc<dyn CodeGenerator>, id: String },
    Server { action: String },
    Prompt { server: Option<String>, prompt: Box<PromptGenerator>, context: Value },
}

#[derive(Clone)]
struct Entry {
    title: String,
    kind: Option<String>,
    action: Option<String>,
    chooser: bool,
    source: Source,
}

struct Ask<'a> {
    path: &'a Path,
    id: &'a str,
    server: Option<&'a str>,
    prompt: &'a PromptGenerator,
    context: &'a Value,
}

struct Asking {
    id: String,
    path: PathBuf,
    server: Option<String>,
    pending: Option<zen_lsp::Pending>,
    answer: Option<Result<Value, String>>,
}

impl Generating {
    fn waiting(&self) -> BTreeSet<PathBuf> {
        let mut paths = self.actions.waiting();
        if let Some(asking) = self.asking.as_ref().filter(|asking| asking.pending.is_some()) {
            paths.insert(asking.path.clone());
        }
        paths
    }

    fn forget(&mut self, languages: &mut Languages) {
        self.actions.drop_listing(languages);
        self.listed = None;
        if let Some(asking) = self.asking.take()
            && let Some(pending) = &asking.pending
            && let Some(client) = intentions::client_of(languages, asking.server.as_deref(), &asking.path)
        {
            client.cancel(pending);
        }
    }
}

/// The parameters of `editor.generate.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateListParams {
    /// The open file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in the file; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// How long to wait for the language server in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// Something Generate can write at the place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorEntry {
    /// What `editor.generate.members` and `editor.generate.apply` take, valid until the next listing or change of the file.
    pub id: String,
    /// What it generates, e.g. `Constructor` or `toString()`.
    pub title: String,
    /// The language server's code action kind when the server offers it, e.g. `source.generate.toString`.
    pub kind: Option<String>,
    /// Whether it asks which members to use first: call `editor.generate.members`, then pass the choice to `editor.generate.apply`.
    pub chooser: bool,
    /// The editor action that opens its chooser directly: `override` (Override Methods…, ⌃O) or `implement` (Implement Methods…, ⌃I).
    #[serde(default)]
    pub action: Option<String>,
}

/// What `editor.generate.list` found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorList {
    /// Whether the language server has not answered yet; call again to wait more.
    pub pending: bool,
    /// The generators, the language's own first, then the server's.
    pub generators: Vec<GeneratorEntry>,
    /// Why there are none, in words for the user.
    pub error: Option<String>,
}

/// The parameters of `editor.generate.members`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateMembersParams {
    /// The id of a generator of the last `editor.generate.list` that has `chooser`.
    pub id: String,
    /// How long to wait for the language server in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `editor.generate.members` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateMembers {
    /// Whether the language server has not answered yet; call again to wait more.
    pub pending: bool,
    /// The members and options to choose from.
    pub chooser: Option<Chooser>,
}

/// The parameters of `editor.generate.apply`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateApplyParams {
    /// The id of a generator of the last `editor.generate.list`.
    pub id: String,
    /// The ids of the members to use, from `editor.generate.members`; the ones it marks `selected` when absent.
    #[serde(default)]
    pub members: Option<Vec<String>>,
    /// The generator's options by id; the chooser's values for those absent.
    #[serde(default)]
    pub options: BTreeMap<String, bool>,
    /// How long to wait for the language server in milliseconds, at most 10 000, which is the default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `editor.generate.apply` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateApplied {
    /// The generator's title.
    pub title: String,
    /// The files changed, in path order.
    pub files: Vec<PathBuf>,
}

pub struct ListGenerators;

impl Command for ListGenerators {
    type Params = GenerateListParams;
    type Output = GeneratorList;
    const ID: &'static str = "editor.generate.list";
    const TITLE: &'static str = "List Generators";
    const DESCRIPTION: &'static str = "Lists what Code → Generate… can write at a place of an open file (the primary caret \
         of the active one by default): the generators of the file's language module or plug-in first, then the \
         language server's source actions (code action kinds `source.*`, without organizing imports and fixing all). \
         Each has an id; one with `chooser` asks for members first (editor.generate.members). Call again with waitMs \
         while pending.";

    fn run(&self, session: &mut Session, params: GenerateListParams) -> Result<GeneratorList, CommandError> {
        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned());
        guard_hidden(session, params.path.clone().or(active))?;
        let generators = session.state::<CodeGenerators>().cloned().unwrap_or_default();
        let indent = indent_at(session, params.path.as_deref());
        with_generating(session, |generating, languages, workbench| {
            list(generating, languages, workbench, &generators, &indent, &params)
        })
    }
}

pub struct GeneratorMembers;

impl Command for GeneratorMembers {
    type Params = GenerateMembersParams;
    type Output = GenerateMembers;
    const ID: &'static str = "editor.generate.members";
    const TITLE: &'static str = "List Generator Members";
    const DESCRIPTION: &'static str = "Gives the member chooser of a generator listed by editor.generate.list with \
         `chooser`: the fields or methods it can use (each with an id, the ones chosen at first `selected`), grouped \
         by the type they belong to, and the generator's options with their values. Call again with waitMs while \
         pending.";

    fn run(&self, session: &mut Session, params: GenerateMembersParams) -> Result<GenerateMembers, CommandError> {
        guard_hidden(session, listed_path(session))?;
        let indent = indent_at(session, None);
        with_generating(session, |generating, languages, workbench| {
            members(generating, languages, workbench, &indent, &params)
        })
    }
}

pub struct ApplyGenerator;

impl Command for ApplyGenerator {
    type Params = GenerateApplyParams;
    type Output = GenerateApplied;
    const ID: &'static str = "editor.generate.apply";
    const TITLE: &'static str = "Generate Code";
    const DESCRIPTION: &'static str = "Writes the code of a generator listed by editor.generate.list, with the members \
         (ids from editor.generate.members, the selected ones by default) and options given, as one undo step; the \
         language server's generators are applied with the server's own edit. Refused for a read-only file or when \
         nothing is chosen for a generator that needs a member.";

    fn run(&self, session: &mut Session, params: GenerateApplyParams) -> Result<GenerateApplied, CommandError> {
        guard_hidden(session, listed_path(session))?;
        let indent = indent_at(session, None);
        let allowed =
            session.state::<crate::scratch::Scratches>().map(crate::scratch::Scratches::folders).unwrap_or_default();
        session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
        let (result, applied) = with_generating(session, |generating, languages, workbench| {
            apply(generating, languages, workbench, &indent, &params, &allowed)
        });
        session.state_mut::<Refactoring>().record(applied);
        result
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListGenerators)?;
    registry.register(GeneratorMembers)?;
    registry.register(ApplyGenerator)?;
    Ok(())
}

fn listed_path(session: &Session) -> Option<PathBuf> {
    session.state::<Generating>().and_then(|generating| generating.listed.as_ref()).map(|listed| listed.path.clone())
}

fn guard_hidden(session: &Session, path: Option<PathBuf>) -> Result<(), CommandError> {
    let Some(path) = path else { return Ok(()) };
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
    crate::hidden_files::guard_change(session, &root, &[path])
}

fn with_generating<T>(
    session: &mut Session,
    f: impl FnOnce(&mut Generating, &mut Languages, &mut Workbench) -> T,
) -> T {
    let mut generating = std::mem::take(session.state_mut::<Generating>());
    let result = with_languages(session, |languages, workbench| {
        let result = f(&mut generating, languages, workbench);
        languages.hold(OWNER, generating.waiting());
        result
    });
    session.set_state(generating);
    result
}

fn indent_at(session: &Session, path: Option<&Path>) -> String {
    let path = path.map(Path::to_owned).or_else(|| {
        session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path().to_owned())
    });
    let Some(path) = path else { return "\t".to_owned() };
    let style = crate::file_indent::configured_at(session, &path).style;
    if style.use_tabs { "\t".to_owned() } else { " ".repeat(usize::from(style.indent.max(1))) }
}

fn document_at(workbench: &Workbench, path: Option<&Path>) -> Result<(PathBuf, String, u64, usize), CommandError> {
    let document = match path {
        Some(path) => workbench.documents().iter().find(|document| document.path() == path),
        None => workbench.active_document(),
    }
    .filter(|document| document.image().is_none())
    .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let caret = document.selections().primary().range().start;
    Ok((document.path().to_owned(), document.text().to_owned(), document.version(), caret))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn list(
    generating: &mut Generating,
    languages: &mut Languages,
    workbench: &mut Workbench,
    generators: &CodeGenerators,
    indent: &str,
    params: &GenerateListParams,
) -> Result<GeneratorList, CommandError> {
    let (path, text, version, caret) = document_at(workbench, params.path.as_deref())?;
    let offset = params.offset.unwrap_or(caret);
    let context = GenerateContext { path: &path, text: &text, offset, indent };
    let mut entries: Vec<Entry> = Vec::new();
    for generator in generators.for_file(&file_name(&path)) {
        for offer in generator.offers(&context) {
            entries.push(Entry {
                title: offer.title,
                kind: None,
                action: offer.action,
                chooser: offer.chooser,
                source: Source::Provider { generator: Arc::clone(&generator), id: offer.id },
            });
        }
    }
    let asked = IntentionsListParams { path: Some(path.clone()), offset: Some(offset), wait_ms: params.wait_ms };
    let error = match intentions::list(&mut generating.actions, languages, workbench, &asked, Some(&[SOURCE])) {
        Ok(found) if found.pending => return Ok(GeneratorList { pending: true, ..GeneratorList::default() }),
        Ok(found) => {
            let mut served: Vec<(usize, Entry)> = found
                .actions
                .iter()
                .flat_map(|action| {
                    let Some(offered) = generating.actions.offered(&action.id) else { return Vec::new() };
                    server_entries(languages, &offered.path, offered.server, &action.id, &offered.action)
                })
                .collect();
            served.sort_by_key(|(order, _)| *order);
            entries.extend(served.into_iter().map(|(_, entry)| entry));
            found.error
        }
        Err(failed) => Some(failed.message),
    };
    let signature = |entries: &[Entry]| entries.iter().map(|entry| entry.title.clone()).collect::<Vec<_>>();
    let same = generating.listed.as_ref().is_some_and(|listed| {
        listed.path == path
            && listed.document_version == version
            && listed.offset == offset
            && signature(&listed.entries) == signature(&entries)
    });
    if !same {
        generating.generation += 1;
        generating.asking = None;
    }
    let generation = generating.generation;
    let shown = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| GeneratorEntry {
            id: format!("{generation}.{index}"),
            title: entry.title.clone(),
            kind: entry.kind.clone(),
            chooser: entry.chooser,
            action: entry.action.clone(),
        })
        .collect::<Vec<_>>();
    generating.listed = Some(Listed { generation, path, document_version: version, offset, entries });
    let error = if shown.is_empty() { Some(error.unwrap_or_else(|| t!("workbench-generate-none"))) } else { None };
    Ok(GeneratorList { pending: false, generators: shown, error })
}

fn server_entries(
    languages: &Languages,
    path: &Path,
    server: Option<String>,
    id: &str,
    action: &Value,
) -> Vec<(usize, Entry)> {
    let Some(kind) = action["kind"].as_str() else { return Vec::new() };
    if action["disabled"].is_object() {
        return Vec::new();
    }
    let declaration = server
        .clone()
        .or_else(|| languages.server_id(path))
        .and_then(|server| languages.declaration_of(&server).cloned());
    let generate = declaration.as_ref().and_then(|declaration| declaration.generate.clone()).unwrap_or_default();
    let tidies = declaration.as_ref().map(|declaration| declaration.optimize_imports_kinds()).unwrap_or_default();
    let under = |prefix: &str| kind == prefix || kind.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('.'));
    if !generate.offers_kind(kind)
        || under("source.organizeImports")
        || under("source.fixAll")
        || tidies.iter().any(|tidy| under(tidy))
    {
        return Vec::new();
    }
    let Some(title) = action["title"].as_str() else { return Vec::new() };
    let command = action["command"]["command"].as_str().unwrap_or_default();
    let context = &action["command"]["arguments"][0];
    let prompts = generate.prompts_for(command, context);
    if prompts.is_empty() {
        let source = Source::Server { action: id.to_owned() };
        let entry =
            Entry { title: title.to_owned(), kind: Some(kind.to_owned()), action: None, chooser: false, source };
        return vec![(usize::MAX, entry)];
    }
    let language = zen_i18n::language();
    let order = |prompt: &PromptGenerator| generate.prompts.iter().position(|declared| declared == prompt);
    prompts
        .into_iter()
        .map(|prompt| {
            (
                order(prompt).unwrap_or(usize::MAX),
                Entry {
                    title: if prompt.name.is_empty() {
                        title.to_owned()
                    } else {
                        zen_lsp::servers::text_in(&prompt.name, &language)
                    },
                    kind: Some(kind.to_owned()),
                    action: Some(prompt.action.clone()).filter(|action| !action.is_empty()),
                    chooser: true,
                    source: Source::Prompt {
                        server: server.clone(),
                        prompt: Box::new(prompt.clone()),
                        context: context.clone(),
                    },
                },
            )
        })
        .collect()
}

fn entry(generating: &Generating, workbench: &Workbench, id: &str) -> Result<(PathBuf, Entry), CommandError> {
    let unknown = || CommandError::new(t!("workbench-generate-unknown", id = id));
    let (generation, index) = id.split_once('.').ok_or_else(unknown)?;
    let listed = generating.listed.as_ref().filter(|listed| generation.parse() == Ok(listed.generation));
    let listed = listed.ok_or_else(unknown)?;
    let entry = index.parse::<usize>().ok().and_then(|index| listed.entries.get(index)).ok_or_else(unknown)?;
    let current = workbench
        .documents()
        .iter()
        .find(|document| document.path() == listed.path)
        .is_some_and(|document| document.version() == listed.document_version);
    if !current {
        return Err(CommandError::new(t!("workbench-generate-stale")));
    }
    Ok((listed.path.clone(), entry.clone()))
}

fn wait_of(wait_ms: Option<u64>, default: Duration) -> Duration {
    wait_ms.map_or(default, Duration::from_millis).min(MAX_WAIT)
}

fn context_text(workbench: &Workbench, path: &Path) -> Result<String, CommandError> {
    workbench
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .map(|document| document.text().to_owned())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

fn listed_offset(generating: &Generating) -> usize {
    generating.listed.as_ref().map_or(0, |listed| listed.offset)
}

fn members(
    generating: &mut Generating,
    languages: &mut Languages,
    workbench: &mut Workbench,
    indent: &str,
    params: &GenerateMembersParams,
) -> Result<GenerateMembers, CommandError> {
    let (path, entry) = entry(generating, workbench, &params.id)?;
    match &entry.source {
        Source::Provider { generator, id } => {
            let text = context_text(workbench, &path)?;
            let context = GenerateContext { path: &path, text: &text, offset: listed_offset(generating), indent };
            let deadline = Instant::now() + wait_of(params.wait_ms, Duration::ZERO);
            while generator.pending(&context, id) {
                if Instant::now() >= deadline {
                    return Ok(GenerateMembers { pending: true, chooser: None });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let chooser = generator.chooser(&context, id).filter(|_| entry.chooser);
            let chooser = chooser.ok_or_else(|| CommandError::new(t!("workbench-generate-no-chooser")))?;
            Ok(GenerateMembers { pending: false, chooser: Some(chooser) })
        }
        Source::Server { .. } => Err(CommandError::new(t!("workbench-generate-no-chooser"))),
        Source::Prompt { server, prompt, context, .. } => {
            let wait = wait_of(params.wait_ms, Duration::ZERO);
            let ask = Ask { path: &path, id: &params.id, server: server.as_deref(), prompt, context };
            match ask_members(generating, languages, &ask, wait) {
                None => Ok(GenerateMembers { pending: true, chooser: None }),
                Some(Ok(answer)) => {
                    Ok(GenerateMembers { pending: false, chooser: Some(prompt_chooser(prompt, &answer, &entry.title)) })
                }
                Some(Err(error)) => Err(CommandError::new(error)),
            }
        }
    }
}

fn ask_members(
    generating: &mut Generating,
    languages: &mut Languages,
    ask: &Ask<'_>,
    wait: Duration,
) -> Option<Result<Value, String>> {
    let Ask { path, id, server, prompt, context } = *ask;
    if generating.asking.as_ref().is_none_or(|asking| asking.id != id) {
        let pending = intentions::client_of(languages, server, path)
            .ok_or_else(|| t!("workbench-language-not-running"))
            .and_then(|client| client.request(&prompt.members, context.clone()).map_err(|error| error.to_string()));
        let (pending, answer) = match pending {
            Ok(pending) => (Some(pending), None),
            Err(error) => (None, Some(Err(error))),
        };
        generating.asking = Some(Asking {
            id: id.to_owned(),
            path: path.to_owned(),
            server: server.map(str::to_owned),
            pending,
            answer,
        });
    }
    let asking = generating.asking.as_mut()?;
    if asking.answer.is_none()
        && let Some(pending) = &asking.pending
    {
        let answer = if wait.is_zero() {
            pending.try_answer()
        } else {
            match pending.wait(wait) {
                Err(LspError::Timeout) => None,
                other => Some(other),
            }
        };
        if let Some(answer) = answer {
            asking.answer = Some(answer.map_err(|error| error.to_string()));
            asking.pending = None;
        }
    }
    asking.answer.clone()
}

fn apply(
    generating: &mut Generating,
    languages: &mut Languages,
    workbench: &mut Workbench,
    indent: &str,
    params: &GenerateApplyParams,
    allowed: &[PathBuf],
) -> (Result<GenerateApplied, CommandError>, Vec<Applied>) {
    let (path, entry) = match entry(generating, workbench, &params.id) {
        Ok(found) => found,
        Err(error) => return (Err(error), Vec::new()),
    };
    if let Some(reason) =
        workbench.documents().iter().find(|document| document.path() == path).and_then(crate::Document::read_only)
    {
        return (Err(CommandError::new(reason.to_owned())), Vec::new());
    }
    let wait = wait_of(params.wait_ms, MAX_WAIT);
    let done = match &entry.source {
        Source::Provider { generator, id } => {
            let text = context_text(workbench, &path);
            text.and_then(|text| {
                let context = GenerateContext { path: &path, text: &text, offset: listed_offset(generating), indent };
                let chooser = generator.chooser(&context, id).filter(|_| entry.chooser);
                let choice = chooser.map_or_else(|| Ok(Chosen::default()), |chooser| chosen(&chooser, params))?;
                let insertion = generator.generate(&context, id, &choice).map_err(CommandError::new)?;
                insert(workbench, &path, insertion)
            })
        }
        Source::Server { action } => {
            let asked = IntentionApplyParams { id: action.clone(), wait_ms: Some(wait.as_millis() as u64) };
            let (result, applied) = intentions::apply(&mut generating.actions, languages, workbench, &asked, allowed);
            result.map(|done| (done.files, applied))
        }
        Source::Prompt { server, prompt, context, .. } => {
            let ask = Ask { path: &path, id: &params.id, server: server.as_deref(), prompt, context };
            let deadline = Instant::now() + wait;
            apply_prompt(generating, languages, workbench, &ask, &entry.title, params, deadline, allowed)
        }
    };
    generating.forget(languages);
    match done {
        Ok((files, applied)) => (Ok(GenerateApplied { title: entry.title, files }), applied),
        Err(error) => (Err(error), Vec::new()),
    }
}

fn chosen(chooser: &Chooser, params: &GenerateApplyParams) -> Result<Chosen, CommandError> {
    let members = match &params.members {
        Some(members) => {
            if let Some(unknown) = members.iter().find(|id| chooser.members.iter().all(|member| &member.id != *id)) {
                return Err(CommandError::new(t!("workbench-generate-unknown-member", id = unknown.as_str())));
            }
            members.clone()
        }
        None => chooser.members.iter().filter(|member| member.selected).map(|member| member.id.clone()).collect(),
    };
    if members.is_empty() && !chooser.allow_empty && !chooser.members.is_empty() {
        return Err(CommandError::new(t!("workbench-generate-nothing-chosen")));
    }
    let options = chooser
        .options
        .iter()
        .map(|option| (option.id.clone(), params.options.get(&option.id).copied().unwrap_or(option.value)))
        .collect();
    Ok(Chosen { members, options })
}

fn insert(
    workbench: &mut Workbench,
    path: &Path,
    insertion: zen_modules::generate::Insertion,
) -> Result<(Vec<PathBuf>, Vec<Applied>), CommandError> {
    let document = workbench
        .documents
        .iter_mut()
        .find(|document| document.path() == path)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let length = document.text().chars().count();
    let at = insertion.at.min(length);
    let end = insertion.end.unwrap_or(at).clamp(at, length);
    let lead = insertion.text.chars().take_while(|c| c.is_whitespace()).count();
    let also = insertion.also.into_iter().filter(|(range, _)| range.end <= at);
    let also: Vec<(std::ops::Range<usize>, String)> = also.collect();
    let grown: usize = also.iter().map(|(_, text)| text.chars().count()).sum();
    let removed: usize = also.iter().map(|(range, _)| range.len()).sum();
    let edits = also.into_iter().chain([(at..end, insertion.text)]).collect();
    if !document.replace_ranges(edits) {
        return Err(CommandError::new(t!("workbench-intentions-nothing")));
    }
    let caret = at + grown - removed + lead;
    document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
    let applied = vec![Applied::Document { path: path.to_owned(), version: document.version() }];
    Ok((vec![path.to_owned()], applied))
}

#[allow(clippy::too_many_arguments, reason = "the server's generator, its place and the choice")]
fn apply_prompt(
    generating: &mut Generating,
    languages: &mut Languages,
    workbench: &mut Workbench,
    ask: &Ask<'_>,
    title: &str,
    params: &GenerateApplyParams,
    deadline: Instant,
    allowed: &[PathBuf],
) -> Result<(Vec<PathBuf>, Vec<Applied>), CommandError> {
    let left = || deadline.saturating_duration_since(Instant::now());
    let Ask { path, server, prompt, context, .. } = *ask;
    let answer = match ask_members(generating, languages, ask, left()) {
        Some(answer) => answer.map_err(CommandError::new)?,
        None => return Err(CommandError::new(LspError::Timeout.to_string())),
    };
    let chooser = prompt_chooser(prompt, &answer, title);
    let choice = chosen(&chooser, params)?;
    let request = prompt_params(prompt, &answer, context, &choice);
    let client = intentions::client_of(languages, server, path)
        .ok_or_else(|| CommandError::new(t!("workbench-language-not-running")))?;
    let pending = client.request(&prompt.generate, request).map_err(|error| CommandError::new(error.to_string()))?;
    let edit = match pending.wait(left()) {
        Ok(edit) => edit,
        Err(error) => {
            if error == LspError::Timeout {
                client.cancel(&pending);
            }
            return Err(CommandError::new(error.to_string()));
        }
    };
    if !edit.is_object() {
        return Err(CommandError::new(t!("workbench-intentions-nothing")));
    }
    let applied = intentions::apply_edit(&edit, languages, workbench, allowed)
        .map_err(|error| CommandError::new(intentions::edit_message(&error)))?;
    let applied = super::edits::journal(applied, workbench);
    if applied.is_empty() {
        return Err(CommandError::new(t!("workbench-intentions-nothing")));
    }
    let mut files: Vec<PathBuf> = applied.iter().map(|applied| applied.path().to_owned()).collect();
    files.sort();
    files.dedup();
    Ok((files, applied))
}

struct Item {
    value: Value,
    parent: Option<Value>,
}

fn child(at: &Value, part: &str) -> Option<Value> {
    let key = part.replace("~1", "/").replace("~0", "~");
    match at {
        Value::Array(list) => key.parse::<usize>().ok().and_then(|index| list.get(index)).cloned(),
        Value::Object(map) => map.get(&key).cloned(),
        _ => None,
    }
}

fn items(answer: &Value, pointer: &str) -> Vec<Item> {
    let mut cursors: Vec<(Value, Option<Value>, Option<Value>)> = vec![(answer.clone(), Some(answer.clone()), None)];
    let mut ends_expanded = false;
    for part in pointer.split('/').skip(1) {
        ends_expanded = part == "*";
        cursors = cursors
            .into_iter()
            .flat_map(|(at, item, parent)| {
                if part == "*" {
                    let elements = at.as_array().cloned().unwrap_or_default();
                    elements.into_iter().map(|element| (element.clone(), Some(element), item.clone())).collect()
                } else {
                    child(&at, part).map(|next| vec![(next, item, parent)]).unwrap_or_default()
                }
            })
            .collect();
    }
    cursors
        .into_iter()
        .flat_map(|(at, item, parent)| match at {
            Value::Array(elements) if !ends_expanded => {
                elements.into_iter().map(|element| Item { value: element, parent: item.clone() }).collect()
            }
            value => vec![Item { value, parent: if ends_expanded { parent } else { item } }],
        })
        .collect()
}

fn rendered(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Array(list) => list.iter().map(rendered).collect::<Vec<_>>().join(", "),
        Value::Null | Value::Object(_) => String::new(),
    }
}

fn text_template(template: &str, item: &Item) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let reference = &after[..close];
        let value = match reference.strip_prefix("parent") {
            Some(pointer) => item.parent.as_ref().and_then(|parent| parent.pointer(pointer)),
            None => item.value.pointer(reference),
        };
        out.push_str(&value.map(rendered).unwrap_or_default());
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

fn starts_selected(list: &MemberList, item: &Item) -> bool {
    match &list.selected {
        Value::Bool(flag) => *flag,
        Value::String(condition) => zen_lsp::servers::holds(&item.value, condition),
        _ => false,
    }
}

fn list_items(answer: &Value, list: &MemberList) -> Vec<Item> {
    let mut found = items(answer, &list.items);
    if !list.when.is_empty() {
        found.retain(|item| zen_lsp::servers::holds(&item.value, &list.when));
    }
    found
}

fn prompt_chooser(prompt: &PromptGenerator, answer: &Value, title: &str) -> Chooser {
    let language = zen_i18n::language();
    let mut members = Vec::new();
    for list in &prompt.lists {
        for (index, item) in list_items(answer, list).iter().enumerate() {
            let text = |template: &str| Some(text_template(template, item)).filter(|text| !text.is_empty());
            members.push(Member {
                id: format!("{}.{index}", list.id),
                name: text_template(&list.label, item),
                detail: text(&list.detail),
                group: text(&list.group),
                selected: starts_selected(list, item),
            });
        }
    }
    let options = prompt
        .options
        .iter()
        .filter(|option| option.when.is_empty() || zen_lsp::servers::holds(answer, &option.when))
        .map(|option| ChooserOption {
            id: option.id.clone(),
            label: zen_lsp::servers::text_in(&option.label, &language),
            value: option.default,
        })
        .collect();
    let title =
        if prompt.title.is_empty() { title.to_owned() } else { zen_lsp::servers::text_in(&prompt.title, &language) };
    Chooser { title, members, options, allow_empty: prompt.allow_empty, search: false }
}

struct Scope<'a> {
    context: &'a Value,
    members: &'a BTreeMap<String, Vec<Value>>,
    options: &'a BTreeMap<String, bool>,
    item: Option<&'a Item>,
}

fn fill(template: &Value, scope: &Scope<'_>) -> Value {
    match template {
        Value::String(text) => {
            let Some(name) = text.strip_prefix("${").and_then(|text| text.strip_suffix('}')) else {
                return template.clone();
            };
            let (name, argument) = name.split_once(':').map_or((name, None), |(name, argument)| (name, Some(argument)));
            let pointed = |value: Option<&Value>| match (value, argument) {
                (Some(value), Some(pointer)) => value.pointer(pointer).cloned().unwrap_or(Value::Null),
                (Some(value), None) => value.clone(),
                (None, _) => Value::Null,
            };
            match name {
                "context" => scope.context.clone(),
                "members" => match argument {
                    Some(list) => Value::Array(scope.members.get(list).cloned().unwrap_or_default()),
                    None => Value::Array(scope.members.values().flatten().cloned().collect()),
                },
                "option" => Value::Bool(argument.is_some_and(|id| scope.options.get(id).copied().unwrap_or(false))),
                "item" => pointed(scope.item.map(|item| &item.value)),
                "parent" => pointed(scope.item.and_then(|item| item.parent.as_ref())),
                _ => template.clone(),
            }
        }
        Value::Array(list) => Value::Array(list.iter().map(|value| fill(value, scope)).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(key, value)| (key.clone(), fill(value, scope))).collect::<Map<_, _>>())
        }
        other => other.clone(),
    }
}

fn prompt_params(prompt: &PromptGenerator, answer: &Value, context: &Value, choice: &Chosen) -> Value {
    let no_members = BTreeMap::new();
    let mut members: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for list in &prompt.lists {
        let found = list_items(answer, list);
        let chosen = members.entry(list.id.clone()).or_default();
        for (index, item) in found.iter().enumerate() {
            if !choice.members.contains(&format!("{}.{index}", list.id)) {
                continue;
            }
            chosen.push(if list.value.is_null() {
                item.value.clone()
            } else {
                fill(&list.value, &Scope { context, members: &no_members, options: &choice.options, item: Some(item) })
            });
        }
    }
    let template = if prompt.params.is_null() { json!("${context}") } else { prompt.params.clone() };
    fill(&template, &Scope { context, members: &members, options: &choice.options, item: None })
}

#[cfg(test)]
mod tests;

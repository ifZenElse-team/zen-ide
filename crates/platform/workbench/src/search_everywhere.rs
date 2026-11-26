use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_modules::search::{
    CancelToken, SearchContributor, SearchContributors, SearchItem, SearchProgress, SearchQuery, SearchSink,
    SearchTarget, TextOptions, TypeNoun,
};

use crate::Workbench;
use crate::language::Languages;
use crate::language::Reason;
use crate::language::classes::{ClassParams, GoToClass};
use crate::language::symbols::{GoToSymbol, SymbolParams};
use crate::navigate::{GoToFile, GoToFileParams};
use crate::search_scopes::{ALL_PLACES, FilterParams, FilterPaths, PROJECT_FILES};

pub const WAIT_FOR_ALL: &str = "search.everywhere.waitForAll";

pub const TEXT_ON_ALL: &str = "search.everywhere.textOnAll";

pub const FEW: usize = ALL_LIMIT;

pub const HOLD: Duration = Duration::from_millis(2000);

pub const ENOUGH: usize = 8;

pub const TAB_LIMIT: usize = 50;

pub const ALL_LIMIT: usize = 6;

pub const ALL: &str = "all";

pub const QUERY_HISTORY: usize = 50;

pub const RECENT_ACTIONS: usize = 20;

const MAX_WAIT: Duration = Duration::from_secs(10);

pub const POLL_EVERY: Duration = Duration::from_millis(50);

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(QueryEverywhere)?;
    registry.register(ListContributors)?;
    registry.register(ListHistory)?;
    registry.register(ClearHistory)?;
    registry.register(GetOptions)?;
    registry.register(SetOptions)?;
    registry.register(ListFilters)?;
    registry.register(OpenInFind)?;
    Ok(())
}

pub fn contributors(session: &Session) -> Vec<Arc<dyn SearchContributor>> {
    let noun = session.state::<TypeNoun>().copied().unwrap_or_default();
    let mut all: Vec<Arc<dyn SearchContributor>> = vec![
        Arc::new(CalculatorContributor),
        Arc::new(RecentFilesContributor),
        Arc::new(ClassesContributor { noun }),
        Arc::new(FilesContributor),
        Arc::new(SymbolsContributor),
        Arc::new(TextContributor::default()),
    ];
    if let Some(added) = session.state::<SearchContributors>() {
        for contributor in added.list() {
            if !all.iter().any(|other| other.id() == contributor.id()) {
                all.push(Arc::clone(contributor));
            }
        }
    }
    all.sort_by_key(|contributor| contributor.weight());
    all
}

fn has_project(session: &Session) -> bool {
    session.state::<Workbench>().is_some_and(|workbench| workbench.project().is_some())
}

pub struct CalculatorContributor;

impl SearchContributor for CalculatorContributor {
    fn id(&self) -> &str {
        "calculator"
    }

    fn title(&self) -> &str {
        "ide-se-calculator"
    }

    fn has_tab(&self) -> bool {
        false
    }

    fn weight(&self) -> i32 {
        20
    }

    fn search(&self, _session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        let expression = query.text.trim();
        if !query.all {
            return SearchProgress::Done;
        }
        let comma = zen_i18n::t!("workbench-calculator-decimal-separator") == ",";
        if let Some(value) = crate::calculator::evaluate(expression, comma) {
            let start = expression.chars().count() + 3;
            sink.push(SearchItem {
                name: format!("{expression} = {value}"),
                detail: String::new(),
                hint: None,
                ranges: vec![[start, start + value.chars().count()]],
                detail_ranges: Vec::new(),
                in_project: true,
                available: true,
                target: SearchTarget::Value { value },
            });
        }
        SearchProgress::Done
    }
}

pub struct RecentFilesContributor;

impl SearchContributor for RecentFilesContributor {
    fn id(&self) -> &str {
        "recentFiles"
    }

    fn title(&self) -> &str {
        "ide-se-recent-files"
    }

    fn has_tab(&self) -> bool {
        false
    }

    fn weight(&self) -> i32 {
        50
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        if !query.all || !query.text.trim().is_empty() {
            return SearchProgress::Done;
        }
        let Some(workbench) = session.state::<Workbench>() else { return SearchProgress::Done };
        let project = workbench.project().map(Path::to_owned).unwrap_or_default();
        let current = workbench.active_document().map(|document| document.path().to_owned());
        for path in workbench.recent_files().iter().filter(|path| Some(*path) != current.as_ref()) {
            let relative = path.strip_prefix(&project).unwrap_or(path).to_string_lossy().replace('\\', "/");
            let (folder, name) = relative.rsplit_once('/').unwrap_or(("", relative.as_str()));
            let item = SearchItem {
                name: name.to_owned(),
                detail: folder.to_owned(),
                hint: None,
                ranges: Vec::new(),
                detail_ranges: Vec::new(),
                in_project: path.starts_with(&project),
                available: true,
                target: SearchTarget::File { path: path.clone(), line: None, column: None },
            };
            if !sink.push(item) {
                break;
            }
        }
        SearchProgress::Done
    }
}

pub struct ClassesContributor {
    pub noun: TypeNoun,
}

impl SearchContributor for ClassesContributor {
    fn id(&self) -> &str {
        "classes"
    }

    fn title(&self) -> &str {
        match self.noun {
            TypeNoun::Class => "ide-se-classes",
            TypeNoun::Type => "ide-se-types",
        }
    }

    fn weight(&self) -> i32 {
        100
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        if query.text.trim().is_empty() || !has_project(session) {
            return SearchProgress::Done;
        }
        let params = ClassParams { query: query.text.clone(), limit: Some(query.limit), wait_ms: None, compact: None };
        let Ok(found) = GoToClass.run(session, params) else { return SearchProgress::Done };
        for class in found.classes.into_iter().filter(|class| query.include_non_project || class.in_project) {
            let item = SearchItem {
                name: class.name,
                detail: class.container.unwrap_or_default(),
                hint: None,
                ranges: class.ranges,
                detail_ranges: Vec::new(),
                in_project: class.in_project,
                available: true,
                target: SearchTarget::Place {
                    path: class.path,
                    offset: class.offset,
                    line: class.line,
                    symbol: class.kind,
                },
            };
            if !sink.push(item) {
                break;
            }
        }
        server_progress(found.pending, found.reason, "ide-se-classes-indexing")
    }
}

pub struct FilesContributor;

impl SearchContributor for FilesContributor {
    fn id(&self) -> &str {
        "files"
    }

    fn title(&self) -> &str {
        "ide-se-files"
    }

    fn weight(&self) -> i32 {
        200
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        if query.text.trim().is_empty() {
            return SearchProgress::Done;
        }
        let params = GoToFileParams { query: query.text.clone(), limit: Some(query.limit) };
        let Ok(found) = GoToFile.run(session, params) else { return SearchProgress::Done };
        let mut folders = folders_matching(session, &query.text, query.limit).into_iter().peekable();
        let rank = |kind: &str| ["exact", "prefix", "humps", "middle"].iter().position(|known| *known == kind);
        for file in found.files {
            while let Some(folder) = folders.next_if(|(kind, _)| rank(kind) < rank(&file.kind)) {
                if !sink.push(folder.1) {
                    return SearchProgress::Done;
                }
            }
            let (folder, name) = file.relative.rsplit_once('/').unwrap_or(("", file.relative.as_str()));
            let at = if folder.is_empty() { 0 } else { folder.chars().count() + 1 };
            let (in_name, in_folder): (Vec<[usize; 2]>, Vec<[usize; 2]>) =
                file.ranges.iter().partition(|range| range[0] >= at);
            let item = SearchItem {
                name: name.to_owned(),
                detail: folder.to_owned(),
                hint: None,
                ranges: in_name.into_iter().map(|[start, end]| [start - at, end - at]).collect(),
                detail_ranges: in_folder
                    .into_iter()
                    .map(|[start, end]| [start, end.min(at.saturating_sub(1))])
                    .collect(),
                in_project: true,
                available: true,
                target: SearchTarget::File { path: file.path.clone(), line: found.line, column: found.column },
            };
            if !sink.push(item) {
                break;
            }
        }
        for (_, folder) in folders {
            if !sink.push(folder) {
                break;
            }
        }
        if found.indexing { SearchProgress::Pending("ide-goto-file-indexing") } else { SearchProgress::Done }
    }
}

fn folders_matching(session: &Session, query: &str, limit: usize) -> Vec<(String, SearchItem)> {
    let pattern = zen_index::NamePattern::new(query.trim());
    let Some(workbench) = session.state::<Workbench>() else { return Vec::new() };
    let (Some(index), Some(project)) = (workbench.index.as_ref(), workbench.project()) else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (_, relative) in index.file_list() {
        let mut folder = relative.as_str();
        while let Some((parent, _)) = folder.rsplit_once('/') {
            folder = parent;
            if !seen.insert(folder.to_owned()) {
                break;
            }
            let name = folder.rsplit_once('/').map_or(folder, |(_, name)| name);
            if let Some(matched) = pattern.matches(name) {
                found.push((matched, folder.to_owned()));
            }
        }
    }
    found.sort_by(|(a, a_folder), (b, b_folder)| {
        b.kind
            .cmp(&a.kind)
            .then(b.degree.cmp(&a.degree))
            .then(a_folder.len().cmp(&b_folder.len()))
            .then(a_folder.cmp(b_folder))
    });
    found
        .into_iter()
        .take(limit)
        .map(|(matched, folder)| {
            let (above, name) = folder.rsplit_once('/').unwrap_or(("", folder.as_str()));
            let item = SearchItem {
                name: name.to_owned(),
                detail: above.to_owned(),
                hint: None,
                ranges: matched.ranges.iter().map(|range| [range.start, range.end]).collect(),
                detail_ranges: Vec::new(),
                in_project: true,
                available: true,
                target: SearchTarget::Directory { path: project.join(&folder) },
            };
            (matched.kind.as_str().to_owned(), item)
        })
        .collect()
}

fn server_progress(pending: bool, reason: Option<Reason>, indexing: &'static str) -> SearchProgress {
    match (pending, reason) {
        (false, _) => SearchProgress::Done,
        (true, Some(Reason::Scanning | Reason::Indexing)) => SearchProgress::Pending(indexing),
        (true, _) => SearchProgress::Pending("ide-se-classes-searching"),
    }
}

pub struct SymbolsContributor;

impl SearchContributor for SymbolsContributor {
    fn id(&self) -> &str {
        "symbols"
    }

    fn title(&self) -> &str {
        "ide-se-symbols"
    }

    fn weight(&self) -> i32 {
        300
    }

    fn primary(&self) -> bool {
        false
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        if query.text.trim().is_empty() || !has_project(session) {
            return SearchProgress::Done;
        }
        let params = SymbolParams {
            query: query.text.clone(),
            limit: Some(query.limit),
            wait_ms: None,
            types: Some(!query.all),
            compact: None,
        };
        let Ok(found) = GoToSymbol.run(session, params) else { return SearchProgress::Done };
        for symbol in found.symbols.into_iter().filter(|symbol| query.include_non_project || symbol.in_project) {
            let file = symbol.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let item = SearchItem {
                name: symbol.qualified_name,
                detail: symbol.container.unwrap_or_default(),
                hint: Some(format!("{file}:{}", symbol.line)),
                ranges: symbol.ranges,
                detail_ranges: Vec::new(),
                in_project: symbol.in_project,
                available: true,
                target: SearchTarget::Place {
                    path: symbol.path,
                    offset: symbol.offset,
                    line: symbol.line,
                    symbol: symbol.kind,
                },
            };
            if !sink.push(item) {
                break;
            }
        }
        server_progress(found.pending, found.reason, "ide-se-symbols-indexing")
    }
}

#[derive(Default)]
pub struct TextContributor {
    running: Mutex<Option<RunningText>>,
}

struct RunningText {
    text: String,
    options: TextOptions,
    scope: Option<String>,
    limit: usize,
    search: crate::search::TextSearch,
}

fn text_limit(limit: usize) -> usize {
    limit.saturating_mul(4).min(2000)
}

impl SearchContributor for TextContributor {
    fn id(&self) -> &str {
        "text"
    }

    fn title(&self) -> &str {
        "ide-se-text"
    }

    fn weight(&self) -> i32 {
        500
    }

    fn primary(&self) -> bool {
        false
    }

    fn fallback(&self) -> bool {
        true
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        if query.text.trim().is_empty() || !has_project(session) {
            return SearchProgress::Done;
        }
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        let fresh = running.as_ref().is_none_or(|run| {
            run.text != query.text
                || run.options != query.text_options
                || run.scope != query.scope
                || run.limit != query.limit
        });
        if fresh {
            if let Some(old) = running.take() {
                old.search.cancel();
            }
            let options = &query.text_options;
            let params = crate::search::SearchParams {
                query: zen_text::search::Query {
                    text: query.text.clone(),
                    match_case: options.match_case,
                    whole_words: options.whole_words,
                    regex: options.regex,
                    ..zen_text::search::Query::default()
                },
                mask: options.mask.trim().to_owned(),
                scope: query.scope.clone(),
                limit: Some(text_limit(query.limit)),
                ..crate::search::SearchParams::default()
            };
            let Ok(search) = crate::search::spawn(session, &params, 2000) else { return SearchProgress::Done };
            *running = Some(RunningText {
                text: query.text.clone(),
                options: options.clone(),
                scope: query.scope.clone(),
                limit: query.limit,
                search,
            });
        }
        let Some(RunningText { search, .. }) = running.as_ref() else { return SearchProgress::Done };
        if query.cancel.is_cancelled() {
            search.cancel();
            return SearchProgress::Done;
        }
        let done = search.is_done();
        for item in text_rows(&search.matches()) {
            if !sink.push(item) {
                return SearchProgress::Done;
            }
        }
        if done { SearchProgress::Done } else { SearchProgress::Pending("ide-se-text-searching") }
    }
}

impl Drop for TextContributor {
    fn drop(&mut self) {
        if let Some(RunningText { search, .. }) = self.running.get_mut().unwrap_or_else(PoisonError::into_inner).take()
        {
            search.cancel();
        }
    }
}

const TEXT_ROW: usize = 160;

fn text_rows(matches: &[zen_index::TextMatch]) -> Vec<SearchItem> {
    let mut rows: Vec<SearchItem> = Vec::new();
    let mut last: Option<(&Path, usize)> = None;
    for found in matches {
        let cut = found.line_text.chars().take_while(|ch| ch.is_whitespace()).count();
        let start = found.column.saturating_sub(cut);
        let range = [start, (start + found.range.len()).min(TEXT_ROW)];
        if last == Some((found.path.as_path(), found.line))
            && let Some(row) = rows.last_mut()
        {
            if range[0] < range[1] {
                row.ranges.push(range);
            }
            continue;
        }
        last = Some((found.path.as_path(), found.line));
        let name: String = found.line_text.chars().skip(cut).take(TEXT_ROW).collect();
        let file = found.relative.rsplit('/').next().unwrap_or(&found.relative);
        rows.push(SearchItem {
            name,
            detail: String::new(),
            hint: Some(format!("{file}:{}", found.line + 1)),
            ranges: if range[0] < range[1] { vec![range] } else { Vec::new() },
            detail_ranges: Vec::new(),
            in_project: true,
            available: true,
            target: SearchTarget::File {
                path: found.path.clone(),
                line: u32::try_from(found.line + 1).ok(),
                column: u32::try_from(found.column + 1).ok(),
            },
        });
    }
    rows
}

struct FileType {
    id: &'static str,
    name: &'static str,
    names: &'static [&'static str],
    extensions: &'static [&'static str],
}

const FILE_TYPES: &[FileType] = &[
    FileType { id: "css", name: "workbench-file-type-css", names: &[], extensions: &["css", "scss", "sass", "less"] },
    FileType {
        id: "dockerfile",
        name: "workbench-file-type-dockerfile",
        names: &["dockerfile", "containerfile"],
        extensions: &[],
    },
    FileType { id: "go", name: "workbench-file-type-go", names: &[], extensions: &["go"] },
    FileType {
        id: "goModule",
        name: "workbench-file-type-go-module",
        names: &["go.mod", "go.work", "go.sum", "go.work.sum"],
        extensions: &[],
    },
    FileType { id: "html", name: "workbench-file-type-html", names: &[], extensions: &["html", "htm", "xhtml"] },
    FileType {
        id: "image",
        name: "workbench-file-type-image",
        names: &[],
        extensions: &["png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "bmp"],
    },
    FileType {
        id: "javascript",
        name: "workbench-file-type-javascript",
        names: &[],
        extensions: &["js", "mjs", "cjs", "jsx"],
    },
    FileType { id: "json", name: "workbench-file-type-json", names: &[], extensions: &["json", "jsonc", "json5"] },
    FileType {
        id: "markdown",
        name: "workbench-file-type-markdown",
        names: &[],
        extensions: &["md", "markdown", "mdx"],
    },
    FileType { id: "python", name: "workbench-file-type-python", names: &[], extensions: &["py", "pyi"] },
    FileType { id: "rust", name: "workbench-file-type-rust", names: &[], extensions: &["rs"] },
    FileType { id: "shell", name: "workbench-file-type-shell", names: &[], extensions: &["sh", "bash", "zsh", "fish"] },
    FileType { id: "sql", name: "workbench-file-type-sql", names: &[], extensions: &["sql"] },
    FileType { id: "text", name: "workbench-file-type-text", names: &[], extensions: &["txt", "text", "log"] },
    FileType { id: "toml", name: "workbench-file-type-toml", names: &[], extensions: &["toml"] },
    FileType {
        id: "typescript",
        name: "workbench-file-type-typescript",
        names: &[],
        extensions: &["ts", "mts", "cts", "tsx"],
    },
    FileType { id: "xml", name: "workbench-file-type-xml", names: &[], extensions: &["xml", "xsd", "plist"] },
    FileType { id: "yaml", name: "workbench-file-type-yaml", names: &[], extensions: &["yml", "yaml"] },
];

pub const DIRECTORY: &str = "directory";

fn file_type(item: &SearchItem) -> Option<&'static str> {
    let path = match &item.target {
        SearchTarget::Directory { .. } => return Some(DIRECTORY),
        SearchTarget::File { path, .. } => path,
        _ => return None,
    };
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    FILE_TYPES
        .iter()
        .find(|kind| kind.names.contains(&name.as_str()))
        .or_else(|| {
            FILE_TYPES.iter().find(|kind| extension.is_some_and(|extension| kind.extensions.contains(&extension)))
        })
        .map(|kind| kind.id)
}

fn language_name(language: &str) -> String {
    FILE_TYPES
        .iter()
        .find(|kind| kind.id.eq_ignore_ascii_case(language))
        .map_or_else(|| language.to_owned(), |kind| zen_i18n::t!(kind.name))
}

fn filtered(session: &mut Session, tab: &str, options: &EverywhereOptions, items: Vec<SearchItem>) -> Vec<SearchItem> {
    match tab {
        "files" if !options.excluded_file_types.is_empty() => items
            .into_iter()
            .filter(|item| {
                file_type(item).is_none_or(|kind| !options.excluded_file_types.iter().any(|out| out == kind))
            })
            .collect(),
        "classes" | "symbols" if !options.excluded_languages.is_empty() => items
            .into_iter()
            .filter(|item| {
                let SearchTarget::Place { path, .. } = &item.target else { return true };
                let language = session.state_mut::<Languages>().language_of(path);
                language.is_none_or(|language| !options.excluded_languages.contains(&language))
            })
            .collect(),
        _ => items,
    }
}

/// What to search for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchRequest {
    /// The tab: `all` or a contributor's id (`classes`, `files`, `symbols`, `actions`).
    pub tab: String,
    /// The text typed.
    pub query: String,
    /// Whether libraries and the SDK are searched too ("Include non-project items").
    pub include_non_project: bool,
    /// How many items each contributor gives; 6 on All (50 for a `/` command) and 50 on a tab of its own by default.
    pub limit: Option<usize>,
    /// The search scope (`search.scope.list`) files and declarations are kept to; any scope but
    /// Project Files searches the libraries and the SDK too, and All Places keeps everything.
    pub scope: Option<String>,
    /// How many times more… was chosen in each group, by contributor id: each time the group lists
    /// [`TAB_LIMIT`] more of its items.
    pub more: BTreeMap<String, usize>,
    /// How text is matched (Cc, W, .* and the file mask); the popup's options when none.
    pub text: Option<TextOptions>,
    /// Whether the query was typed rather than pasted: the
    /// contributors then also match it read in the other keyboard layout (`ьфшт` finds `main`,
    /// [`zen_index::layout`]), after what the text as typed finds. The Text tab and Text on All
    /// search the text as typed only.
    pub typed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchGroup {
    pub contributor: String,
    pub title: String,
    pub items: Vec<SearchItem>,
    pub more: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAnswer {
    pub tab: String,
    pub groups: Vec<SearchGroup>,
    pub pending: bool,
    pub held: bool,
    pub waiting: Option<String>,
}

impl SearchAnswer {
    pub fn items(&self) -> impl Iterator<Item = &SearchItem> {
        self.groups.iter().flat_map(|group| &group.items)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RunKey {
    tab: String,
    query: String,
    include_non_project: bool,
    limit: usize,
    more: BTreeMap<String, usize>,
    scope: Option<String>,
    settings: u64,
    filters: [Vec<String>; 3],
    include_disabled_actions: bool,
    text: TextOptions,
    text_on_all: bool,
    typed: bool,
}

struct Found {
    contributor: Arc<dyn SearchContributor>,
    items: Vec<SearchItem>,
    more: bool,
    progress: Option<SearchProgress>,
    forms: Vec<String>,
    form: usize,
    settled: Vec<SearchItem>,
    first_done: bool,
}

impl Found {
    fn new(contributor: Arc<dyn SearchContributor>) -> Self {
        Self {
            contributor,
            items: Vec::new(),
            more: false,
            progress: None,
            forms: Vec::new(),
            form: 0,
            settled: Vec::new(),
            first_done: false,
        }
    }

    fn restart(&mut self) {
        self.progress = None;
        self.form = 0;
        self.settled.clear();
        self.first_done = false;
    }
}

#[derive(Default)]
pub struct SearchRun {
    key: Option<RunKey>,
    started: Option<Instant>,
    polled: Option<Instant>,
    cancel: CancelToken,
    found: Vec<Found>,
    shown: Vec<SearchGroup>,
    released: bool,
    growing: Option<(Instant, Vec<usize>)>,
}

impl SearchRun {
    pub fn poll(&mut self, session: &mut Session, request: &SearchRequest) -> Result<SearchAnswer, CommandError> {
        self.poll_at(session, request, Instant::now())
    }

    pub fn poll_at(
        &mut self,
        session: &mut Session,
        request: &SearchRequest,
        now: Instant,
    ) -> Result<SearchAnswer, CommandError> {
        let tab = if request.tab.is_empty() { ALL } else { request.tab.as_str() };
        let all = tab == ALL;
        let typed = !request.query.trim().is_empty();
        let command = request.query.trim_start().starts_with('/');
        let limit = request.limit.unwrap_or(if all && typed && !command { ALL_LIMIT } else { TAB_LIMIT }).max(1);
        let scope = request.scope.as_deref().filter(|scope| !scope.is_empty());
        let include_non_project = request.include_non_project || scope.is_some_and(|scope| scope != PROJECT_FILES);
        let options = session.state::<SearchHistory>().map(SearchHistory::options).unwrap_or_default();
        let text = request.text.clone().unwrap_or_else(|| options.text_options());
        let text_on_all = session.settings().get_or(TEXT_ON_ALL, true);
        let key = RunKey {
            tab: tab.to_owned(),
            query: request.query.clone(),
            include_non_project,
            limit,
            more: request.more.clone(),
            scope: scope.map(str::to_owned),
            settings: session
                .state::<crate::project_settings::ProjectSettingsState>()
                .map_or(0, crate::project_settings::ProjectSettingsState::revision),
            filters: [
                options.excluded_contributors.clone(),
                options.excluded_file_types.clone(),
                options.excluded_languages.clone(),
            ],
            include_disabled_actions: options.include_disabled_actions,
            text: text.clone(),
            text_on_all,
            typed: request.typed,
        };
        let grown = self
            .key
            .as_ref()
            .filter(|old| **old != key && RunKey { more: key.more.clone(), ..(*old).clone() } == key)
            .filter(|_| self.released)
            .map(|old| old.more.clone());
        if let Some(old) = grown {
            let changed: Vec<usize> = self
                .found
                .iter()
                .enumerate()
                .filter(|(_, found)| old.get(found.contributor.id()) != key.more.get(found.contributor.id()))
                .map(|(index, _)| index)
                .collect();
            for &index in &changed {
                self.found[index].restart();
            }
            self.growing = Some((now, changed));
            self.key = Some(key);
            self.polled = None;
        } else if self.key.as_ref() != Some(&key) {
            let known = contributors(session);
            if !all && !known.iter().any(|contributor| contributor.has_tab() && contributor.id() == tab) {
                return Err(CommandError::with_code(
                    "unknownTab",
                    format!("no tab `{tab}`; search.everywhere.contributors lists them"),
                ));
            }
            let searched: Vec<_> = known
                .into_iter()
                .filter(|contributor| {
                    if all && command {
                        contributor.commands()
                    } else if all {
                        !contributor.commands()
                            && (text_on_all || !contributor.fallback())
                            && !options.excluded_contributors.iter().any(|out| out == contributor.id())
                    } else {
                        contributor.has_tab() && contributor.id() == tab
                    }
                })
                .collect();
            if searched.is_empty() && !all {
                return Err(CommandError::with_code(
                    "unknownTab",
                    format!("no tab `{tab}`; search.everywhere.contributors lists them"),
                ));
            }
            self.cancel.cancel();
            self.cancel = CancelToken::default();
            self.found = searched.into_iter().map(Found::new).collect();
            if self.key.as_ref().is_some_and(|old| old.tab != key.tab) {
                self.shown.clear();
            }
            self.key = Some(key);
            self.started = Some(now);
            self.polled = None;
            self.released = false;
            self.growing = None;
        }
        if self.polled.is_none_or(|polled| now.saturating_duration_since(polled) >= POLL_EVERY) {
            self.polled = Some(now);
            let order: Vec<usize> = (0..self.found.len())
                .filter(|&index| !self.found[index].contributor.fallback())
                .chain((0..self.found.len()).filter(|&index| self.found[index].contributor.fallback()))
                .collect();
            for index in order {
                if all && self.found[index].contributor.fallback() && self.found[index].progress.is_none() {
                    let others = self.found.iter().filter(|other| !other.contributor.fallback());
                    let (mut searching, mut listed) = (false, 0);
                    for other in others {
                        searching |= other.progress != Some(SearchProgress::Done);
                        listed += other.items.len();
                    }
                    if searching {
                        continue;
                    }
                    if listed >= FEW {
                        self.found[index].progress = Some(SearchProgress::Done);
                        continue;
                    }
                }
                let found = &mut self.found[index];
                if found.progress == Some(SearchProgress::Done) {
                    continue;
                }
                let times = request.more.get(found.contributor.id()).copied().unwrap_or(0);
                let listed = limit.saturating_add(times.saturating_mul(TAB_LIMIT));
                if found.forms.is_empty() {
                    found.forms.push(request.query.clone());
                    if request.typed && !command && !found.contributor.fallback() {
                        found.forms.extend(zen_index::layout::other_forms(&request.query));
                    }
                }
                let query = SearchQuery {
                    text: found.forms[found.form].clone(),
                    all,
                    include_non_project,
                    limit: listed + 1,
                    scope: scope.map(str::to_owned),
                    text_options: text.clone(),
                    cancel: self.cancel.clone(),
                };
                let mut sink = SearchSink::new(listed + 1, self.cancel.clone());
                let mut progress = found.contributor.search(session, &query, &mut sink);
                let mut items = found.settled.clone();
                let settled = items.len();
                for item in sink.into_items() {
                    if !items[..settled].iter().any(|known| known.target == item.target) {
                        items.push(item);
                    }
                }
                if progress == SearchProgress::Done {
                    found.first_done = true;
                    if found.form + 1 < found.forms.len() && items.len() <= listed {
                        found.form += 1;
                        found.settled = items.clone();
                        progress = SearchProgress::Pending("ide-se-classes-searching");
                    }
                }
                found.items = items;
                found.progress = Some(progress);
                if let Some(scope) = scope.filter(|scope| ![ALL_PLACES, PROJECT_FILES].contains(scope)) {
                    found.items = in_scope(session, scope, std::mem::take(&mut found.items));
                }
                if !all {
                    found.items = filtered(session, tab, &options, std::mem::take(&mut found.items));
                }
                found.more = found.items.len() > listed;
                found.items.truncate(listed);
            }
        }
        let done = |found: &Found| found.progress == Some(SearchProgress::Done);
        if let Some((since, changed)) = &self.growing {
            let full = |found: &Found| {
                let times = request.more.get(found.contributor.id()).copied().unwrap_or(0);
                found.items.len() >= limit.saturating_add(times.saturating_mul(TAB_LIMIT))
            };
            let grown = changed.iter().all(|&index| done(&self.found[index]) || full(&self.found[index]));
            if grown || now.duration_since(*since) >= HOLD {
                self.growing = None;
            }
        }
        let ready = self.growing.is_none()
            && (self.released
                || if session.settings().get_or(WAIT_FOR_ALL, false) {
                    self.found.iter().all(done)
                } else {
                    let enough = ENOUGH.min(limit);
                    self.started.is_some_and(|started| now.duration_since(started) >= HOLD)
                        || self
                            .found
                            .iter()
                            .filter(|found| !all || found.contributor.primary())
                            .all(|found| done(found) || found.first_done || found.items.len() >= enough)
                });
        if ready {
            self.released = true;
            let (main, fallback): (Vec<&Found>, Vec<&Found>) =
                self.found.iter().partition(|found| !found.contributor.fallback());
            self.shown = main
                .into_iter()
                .chain(fallback)
                .filter(|found| !found.items.is_empty())
                .map(|found| SearchGroup {
                    contributor: found.contributor.id().to_owned(),
                    title: found.contributor.title().to_owned(),
                    items: found.items.clone(),
                    more: found.more,
                })
                .collect();
        }
        let waiting = self.found.iter().find_map(|found| match found.progress {
            Some(SearchProgress::Pending(message)) => Some(message.to_owned()),
            _ => None,
        });
        Ok(SearchAnswer {
            tab: tab.to_owned(),
            groups: self.shown.clone(),
            pending: !self.found.iter().all(done),
            held: !ready,
            waiting,
        })
    }
}

fn in_scope(session: &mut Session, scope: &str, items: Vec<SearchItem>) -> Vec<SearchItem> {
    let path = |item: &SearchItem| match &item.target {
        SearchTarget::File { path, .. } | SearchTarget::Place { path, .. } => Some(path.clone()),
        _ => None,
    };
    let paths: Vec<PathBuf> = items.iter().filter_map(path).collect();
    if paths.is_empty() {
        return items;
    }
    let params = FilterParams { scope: Some(scope.to_owned()), paths };
    let Ok(kept) = FilterPaths.run(session, params) else { return items };
    let kept: HashSet<PathBuf> = kept.into_iter().collect();
    items.into_iter().filter(|item| path(item).is_none_or(|path| kept.contains(&path))).collect()
}

impl Drop for SearchRun {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchHistory {
    inner: Arc<Mutex<HistoryState>>,
}

#[derive(Debug, Default)]
struct HistoryState {
    file: Option<PathBuf>,
    data: HistoryData,
    options: EverywhereOptions,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    queries: Vec<String>,
    actions: Vec<String>,
    options: EverywhereOptions,
}

pub const PREVIEW_HEIGHT: f32 = 220.0;

/// The options of the Search Everywhere popup, remembered across restarts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EverywhereOptions {
    /// Whether Preview shows the selected item's file under the list.
    pub preview: bool,
    /// The height of the preview under the splitter, in points.
    pub preview_height: f32,
    /// The contributors the Filter of All leaves out, by id (`search.everywhere.contributors`).
    pub excluded_contributors: Vec<String>,
    /// The file types the Filter of Files leaves out (`directory`, `go`...; `search.everywhere.filters`).
    pub excluded_file_types: Vec<String>,
    /// The languages the Filter of Types and Symbols leaves out (`go`, `rust`...).
    pub excluded_languages: Vec<String>,
    /// Whether Actions lists the actions that cannot run now (Include disabled actions).
    pub include_disabled_actions: bool,
    /// Whether the Text tab matches case (Cc).
    pub text_match_case: bool,
    /// Whether the Text tab matches whole words only (W).
    pub text_whole_words: bool,
    /// Whether the Text tab's query is a regular expression (.*).
    pub text_regex: bool,
    /// The Text tab's file mask (`*.go, !*_test.go`); every file when empty.
    pub text_mask: String,
}

impl EverywhereOptions {
    pub fn text_options(&self) -> TextOptions {
        TextOptions {
            match_case: self.text_match_case,
            whole_words: self.text_whole_words,
            regex: self.text_regex,
            mask: self.text_mask.clone(),
        }
    }
}

impl Default for EverywhereOptions {
    fn default() -> Self {
        Self {
            preview: false,
            preview_height: PREVIEW_HEIGHT,
            excluded_contributors: Vec::new(),
            excluded_file_types: Vec::new(),
            excluded_languages: Vec::new(),
            include_disabled_actions: false,
            text_match_case: false,
            text_whole_words: false,
            text_regex: false,
            text_mask: String::new(),
        }
    }
}

/// What the history holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryData {
    /// The queries a result was chosen with, newest first.
    pub queries: Vec<String>,
    /// The actions run from the popup (keymap actions or command ids), newest first.
    pub actions: Vec<String>,
}

impl SearchHistory {
    pub fn load(file: &Path) -> Self {
        let stored: Stored =
            std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        let data = HistoryData { queries: stored.queries, actions: stored.actions };
        let state = HistoryState { file: Some(file.to_owned()), data, options: stored.options };
        Self { inner: Arc::new(Mutex::new(state)) }
    }

    fn lock(&self) -> MutexGuard<'_, HistoryState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn change(&self, edit: impl FnOnce(&mut HistoryData, &mut EverywhereOptions)) {
        let mut state = self.lock();
        let HistoryState { data, options, .. } = &mut *state;
        edit(data, options);
        let stored = Stored {
            queries: state.data.queries.clone(),
            actions: state.data.actions.clone(),
            options: state.options.clone(),
        };
        if let Some(file) = &state.file
            && let Ok(text) = serde_json::to_string_pretty(&stored)
        {
            if let Some(folder) = file.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = zen_vfs::write_atomic(file, text.as_bytes());
        }
    }

    pub fn data(&self) -> HistoryData {
        self.lock().data.clone()
    }

    pub fn remember_query(&self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.change(|data, _| remember(&mut data.queries, query, QUERY_HISTORY));
    }

    pub fn remember_action(&self, action: &str) {
        self.change(|data, _| remember(&mut data.actions, action, RECENT_ACTIONS));
    }

    pub fn clear(&self) {
        self.change(|data, _| *data = HistoryData::default());
    }

    pub fn options(&self) -> EverywhereOptions {
        self.lock().options.clone()
    }

    pub fn set_options(&self, edit: impl FnOnce(&mut EverywhereOptions)) {
        self.change(|_, options| edit(options));
    }
}

fn remember(list: &mut Vec<String>, value: &str, limit: usize) {
    list.retain(|other| other != value);
    list.insert(0, value.to_owned());
    list.truncate(limit);
}

/// The parameters of `search.everywhere.query`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryParams {
    /// The tab: `all` (by default) or a contributor's id (`classes`, `files`, `symbols`, `actions`).
    #[serde(default)]
    pub tab: Option<String>,
    /// The text typed.
    pub query: String,
    /// Whether libraries and the SDK are searched too; false by default, as in the popup.
    #[serde(default)]
    pub include_non_project: bool,
    /// How many items each contributor gives; 6 on All (50 for a `/` command) and 50 on a tab of its own by default.
    #[serde(default)]
    pub limit: Option<usize>,
    /// How long to wait for the contributors to finish, in milliseconds (at most 10 000).
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// A search scope from `search.scope.list { everywhere: true }` the files and declarations are
    /// kept to, as the popup's tabs other than All; none searches as All does.
    #[serde(default)]
    pub scope: Option<String>,
    /// How many times more… is chosen in each group, by contributor id (`{"files": 1}`): each time
    /// the group lists 50 more of its items.
    #[serde(default)]
    pub more: BTreeMap<String, usize>,
    /// Whether Text matches case (Cc); the popup's choice when absent.
    #[serde(default)]
    pub match_case: Option<bool>,
    /// Whether Text matches whole words only (W); the popup's choice when absent.
    #[serde(default)]
    pub whole_words: Option<bool>,
    /// Whether Text's query is a regular expression (.*); the popup's choice when absent.
    #[serde(default)]
    pub regex: Option<bool>,
    /// Text's file masks, comma-separated (`*.go, !*_test.go`); the popup's mask when absent, every
    /// file when empty.
    #[serde(default)]
    pub mask: Option<String>,
    /// Whether the query counts as typed on the keyboard, as in the popup: it then also matches
    /// read in the other keyboard layout (`ьфшт` finds `main`), after what it finds as given. False
    /// by default: a query an agent sends is taken as pasted.
    #[serde(default)]
    pub typed: bool,
}

impl QueryParams {
    fn text_options(&self, session: &Session) -> TextOptions {
        let options = session.state::<SearchHistory>().map(SearchHistory::options).unwrap_or_default();
        TextOptions {
            match_case: self.match_case.unwrap_or(options.text_match_case),
            whole_words: self.whole_words.unwrap_or(options.text_whole_words),
            regex: self.regex.unwrap_or(options.text_regex),
            mask: self.mask.clone().unwrap_or(options.text_mask),
        }
    }
}

#[derive(Default)]
struct AgentRun(SearchRun);

pub struct QueryEverywhere;

impl Command for QueryEverywhere {
    type Params = QueryParams;
    type Output = SearchAnswer;
    const ID: &'static str = "search.everywhere.query";
    const TITLE: &'static str = "Search Everywhere";
    const DESCRIPTION: &'static str = "Searches as the Search Everywhere popup does and answers the same list: the groups of \
         the tab's contributors (`all` lists a few of each), each item with its name, detail, hint and target (a file, a \
         place for navigate.place.goTo, or an action for run_action). It waits while the new results are held back \
         (2000 ms at most, unless searchWaitForAll); `pending` while a contributor still searches, and `waitMs` waits \
         for every contributor. On `all` a query that starts with `/` lists the commands: the groups of Settings \
         (target `settings`) and, for `/plugins`, the plug-ins (target `switch`: dispatch its command with its params); \
         an arithmetic expression gets a Calculator group with its value (target `value`). The `text` tab lists the \
         lines holding the query (target `file` with `line` and `column`) as Find in Files finds them, with \
         matchCase, wholeWords, regex and mask (the popup's choices when absent); `all` lists them last, and only \
         when the other groups found fewer than 6 items and searchTextOnAll is on. With `typed` the query also matches \
         read in the other keyboard layout (ЙЦУКЕН ↔ QWERTY: `ьфшт` finds `main`), after what it finds as given, as \
         the popup does for what is typed rather than pasted.";

    fn run(&self, session: &mut Session, params: QueryParams) -> Result<SearchAnswer, CommandError> {
        let text = params.text_options(session);
        let request = SearchRequest {
            tab: params.tab.unwrap_or_else(|| ALL.to_owned()),
            query: params.query,
            include_non_project: params.include_non_project,
            limit: params.limit,
            scope: params.scope,
            more: params.more,
            text: Some(text),
            typed: params.typed,
        };
        let start = Instant::now();
        let deadline = start + Duration::from_millis(params.wait_ms.unwrap_or(0)).min(MAX_WAIT);
        let mut run = std::mem::take(&mut session.state_mut::<AgentRun>().0);
        if request.query.trim_start().starts_with('/') {
            run = SearchRun::default();
        }
        let answer = loop {
            let answer = run.poll(session, &request);
            let now = Instant::now();
            match &answer {
                Ok(found) if (found.held || found.pending && now < deadline) && now < start + MAX_WAIT => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => break answer,
            }
        };
        session.state_mut::<AgentRun>().0 = run;
        answer
    }
}

/// A contributor as `search.everywhere.contributors` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContributorInfo {
    /// The id, also its tab's.
    pub id: String,
    /// Its title in the interface's language.
    pub title: String,
    /// Whether it has a tab of its own.
    pub has_tab: bool,
    /// Where its tab and group go, lighter first.
    pub weight: i32,
    /// Whether All holds results back for it.
    pub primary: bool,
    /// Whether All searches it only when the other groups found few items, listing it last.
    pub fallback: bool,
}

pub struct ListContributors;

impl Command for ListContributors {
    type Params = NoParams;
    type Output = Vec<ContributorInfo>;
    const ID: &'static str = "search.everywhere.contributors";
    const TITLE: &'static str = "Search Everywhere Contributors";
    const DESCRIPTION: &'static str = "Lists the contributors of Search Everywhere, lighter first: the id \
         search.everywhere.query takes as its tab, the title, whether it has a tab, its weight, whether All holds \
         results back for it and whether All searches it only when the others found few items (Text).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<ContributorInfo>, CommandError> {
        Ok(contributors(session)
            .iter()
            .map(|contributor| ContributorInfo {
                id: contributor.id().to_owned(),
                title: zen_i18n::t!(contributor.title()),
                has_tab: contributor.has_tab(),
                weight: contributor.weight(),
                primary: contributor.primary(),
                fallback: contributor.fallback(),
            })
            .collect())
    }
}

pub struct ListHistory;

impl Command for ListHistory {
    type Params = NoParams;
    type Output = HistoryData;
    const ID: &'static str = "search.history.list";
    const TITLE: &'static str = "Search Everywhere History";
    const DESCRIPTION: &'static str = "Lists the queries a result of Search Everywhere was chosen with and the actions run \
         from it, newest first, kept across restarts; ⌥↓ and ⌥↑ bring the queries back in the popup.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<HistoryData, CommandError> {
        Ok(session.state_mut::<SearchHistory>().data())
    }
}

pub struct ClearHistory;

impl Command for ClearHistory {
    type Params = NoParams;
    type Output = HistoryData;
    const ID: &'static str = "search.history.clear";
    const TITLE: &'static str = "Clear Search Everywhere History";
    const DESCRIPTION: &'static str =
        "Forgets the queries and recent actions of Search Everywhere, in every window and in the file that keeps them.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<HistoryData, CommandError> {
        let history = session.state_mut::<SearchHistory>().clone();
        history.clear();
        Ok(history.data())
    }
}

pub struct GetOptions;

impl Command for GetOptions {
    type Params = NoParams;
    type Output = EverywhereOptions;
    const ID: &'static str = "search.everywhere.options";
    const TITLE: &'static str = "Search Everywhere Options";
    const DESCRIPTION: &'static str = "Tells the options of the Search Everywhere popup, kept across restarts: Preview and \
         its height, what the Filters of All, Files and Types/Symbols leave out, Include disabled actions, and the Text tab's \
         Cc, W, .* and file mask.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EverywhereOptions, CommandError> {
        Ok(session.state_mut::<SearchHistory>().options())
    }
}

/// The parameters of `search.everywhere.setOptions`: the options to change; those left out stay.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OptionsParams {
    /// Whether Preview shows the selected item's file under the list.
    pub preview: Option<bool>,
    /// The height of the preview, in points (80 to 900).
    pub preview_height: Option<f32>,
    /// The contributors the Filter of All leaves out.
    pub excluded_contributors: Option<Vec<String>>,
    /// The file types the Filter of Files leaves out.
    pub excluded_file_types: Option<Vec<String>>,
    /// The languages the Filter of Types and Symbols leaves out.
    pub excluded_languages: Option<Vec<String>>,
    /// Whether Actions lists the actions that cannot run now.
    pub include_disabled_actions: Option<bool>,
    /// Whether the Text tab matches case (Cc).
    pub text_match_case: Option<bool>,
    /// Whether the Text tab matches whole words only (W).
    pub text_whole_words: Option<bool>,
    /// Whether the Text tab's query is a regular expression (.*).
    pub text_regex: Option<bool>,
    /// The Text tab's file mask; empty for every file.
    pub text_mask: Option<String>,
}

pub struct SetOptions;

impl Command for SetOptions {
    type Params = OptionsParams;
    type Output = EverywhereOptions;
    const ID: &'static str = "search.everywhere.setOptions";
    const TITLE: &'static str = "Set Search Everywhere Options";
    const DESCRIPTION: &'static str = "Changes the options of the Search Everywhere popup, as its toolbar does: preview, \
         previewHeight, excludedContributors (Filter on All), excludedFileTypes (Filter on Files), excludedLanguages \
         (Filter on Types and Symbols), includeDisabledActions, and the Text tab's textMatchCase (Cc), textWholeWords (W), \
         textRegex (.*) and textMask (the file mask); those left out stay. The ids come from \
         search.everywhere.filters. Answers with the options.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<EverywhereOptions, CommandError> {
        let history = session.state_mut::<SearchHistory>().clone();
        history.set_options(|options| {
            options.preview = params.preview.unwrap_or(options.preview);
            if let Some(height) = params.preview_height.filter(|height| height.is_finite()) {
                options.preview_height = height.clamp(80.0, 900.0);
            }
            for (list, new) in [
                (&mut options.excluded_contributors, params.excluded_contributors),
                (&mut options.excluded_file_types, params.excluded_file_types),
                (&mut options.excluded_languages, params.excluded_languages),
            ] {
                if let Some(mut new) = new {
                    new.sort();
                    new.dedup();
                    *list = new;
                }
            }
            options.include_disabled_actions =
                params.include_disabled_actions.unwrap_or(options.include_disabled_actions);
            options.text_match_case = params.text_match_case.unwrap_or(options.text_match_case);
            options.text_whole_words = params.text_whole_words.unwrap_or(options.text_whole_words);
            options.text_regex = params.text_regex.unwrap_or(options.text_regex);
            if let Some(mask) = params.text_mask {
                options.text_mask = mask.trim().to_owned();
            }
        });
        Ok(history.options())
    }
}

/// The parameters of `search.everywhere.filters`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FiltersParams {
    /// The tab: `all`, `files`, `classes` or `symbols`.
    pub tab: String,
}

/// An entry of the Filter of a tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilterEntry {
    /// The id the options keep: a contributor, a file type or a language.
    pub id: String,
    /// Its name in the interface's language.
    pub name: String,
    /// Whether its results are listed (ticked).
    pub on: bool,
}

pub fn filter_entries(session: &mut Session, tab: &str) -> Result<Vec<FilterEntry>, CommandError> {
    let options = session.state_mut::<SearchHistory>().options();
    let entry = |id: &str, name: String, out: &[String]| FilterEntry {
        id: id.to_owned(),
        name,
        on: !out.iter().any(|other| other == id),
    };
    match tab {
        ALL | "" => Ok(contributors(session)
            .iter()
            .filter(|contributor| !contributor.commands())
            .map(|contributor| {
                entry(contributor.id(), zen_i18n::t!(contributor.title()), &options.excluded_contributors)
            })
            .collect()),
        "files" => {
            let mut types: Vec<FilterEntry> = FILE_TYPES
                .iter()
                .map(|kind| entry(kind.id, zen_i18n::t!(kind.name), &options.excluded_file_types))
                .collect();
            types.sort_by_key(|kind| kind.name.to_lowercase());
            let directory =
                entry(DIRECTORY, zen_i18n::t!("workbench-file-type-directory"), &options.excluded_file_types);
            Ok(std::iter::once(directory).chain(types).collect())
        }
        "classes" | "symbols" => {
            let mut languages: Vec<FilterEntry> = session
                .state_mut::<Languages>()
                .server_languages()
                .iter()
                .map(|language| entry(language, language_name(language), &options.excluded_languages))
                .collect();
            languages.sort_by_key(|language| language.name.to_lowercase());
            Ok(languages)
        }
        _ => Err(CommandError::with_code("noFilter", format!("the tab `{tab}` has no Filter"))),
    }
}

pub struct ListFilters;

impl Command for ListFilters {
    type Params = FiltersParams;
    type Output = Vec<FilterEntry>;
    const ID: &'static str = "search.everywhere.filters";
    const TITLE: &'static str = "Search Everywhere Filters";
    const DESCRIPTION: &'static str = "Lists the Filter of a Search Everywhere tab with what is ticked: on `all` the \
         contributors, on `files` the file types (Directory first), on `classes` and `symbols` the languages. Change \
         them with search.everywhere.setOptions.";

    fn run(&self, session: &mut Session, params: FiltersParams) -> Result<Vec<FilterEntry>, CommandError> {
        filter_entries(session, &params.tab)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundEverywhere {
    pub search: u64,
    pub tab: String,
    pub title: String,
    pub query: String,
    pub groups: Vec<SearchGroup>,
}

pub struct OpenInFind;

impl Command for OpenInFind {
    type Params = QueryParams;
    type Output = FoundEverywhere;
    const ID: &'static str = "search.everywhere.openInFind";
    const TITLE: &'static str = "Open in Find Tool Window";
    const DESCRIPTION: &'static str = "Searches as search.everywhere.query does (waiting up to 5000 ms for every \
         contributor unless waitMs says otherwise) and puts the results on a new tab of the Find tool window, which \
         shows; not on the Actions tab, as in the popup.";

    fn run(&self, session: &mut Session, params: QueryParams) -> Result<FoundEverywhere, CommandError> {
        let tab = params.tab.clone().unwrap_or_else(|| ALL.to_owned());
        if tab == "actions" {
            return Err(CommandError::with_code("notAvailable", "Actions cannot be opened in the Find tool window"));
        }
        let query = params.query.clone();
        let params = QueryParams { wait_ms: Some(params.wait_ms.unwrap_or(5000)), ..params };
        let answer = QueryEverywhere.run(session, params)?;
        Ok(show_in_find(session, &tab, &query, answer.groups))
    }
}

pub fn show_in_find(session: &mut Session, tab: &str, query: &str, groups: Vec<SearchGroup>) -> FoundEverywhere {
    let title = if tab == ALL {
        "ide-se-all".to_owned()
    } else {
        contributors(session)
            .iter()
            .find(|contributor| contributor.id() == tab)
            .map_or_else(|| tab.to_owned(), |contributor| contributor.title().to_owned())
    };
    let state = session.state_mut::<FoundEverywhere>();
    *state = FoundEverywhere { search: state.search + 1, tab: tab.to_owned(), title, query: query.to_owned(), groups };
    state.clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_popup_that_needs_the_index_says_it_is_not_available_while_indexing() {
        let key = "ide-se-classes-indexing";
        assert!(matches!(server_progress(true, Some(Reason::Indexing), key), SearchProgress::Pending(k) if k == key));
        assert!(matches!(server_progress(true, Some(Reason::Scanning), key), SearchProgress::Pending(k) if k == key));
        assert!(matches!(
            server_progress(true, Some(Reason::Searching), key),
            SearchProgress::Pending("ide-se-classes-searching")
        ));
        assert!(matches!(server_progress(false, Some(Reason::Indexing), key), SearchProgress::Done));
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::{Value, json};

    use super::*;

    struct Slow {
        id: &'static str,
        weight: i32,
        primary: bool,
        ready_after: usize,
        polls: AtomicUsize,
    }

    impl Slow {
        fn new(id: &'static str, weight: i32, primary: bool, ready_after: usize) -> Arc<Self> {
            Arc::new(Self { id, weight, primary, ready_after, polls: AtomicUsize::new(0) })
        }
    }

    impl SearchContributor for Slow {
        fn id(&self) -> &str {
            self.id
        }

        fn title(&self) -> &str {
            self.id
        }

        fn weight(&self) -> i32 {
            self.weight
        }

        fn primary(&self) -> bool {
            self.primary
        }

        fn search(&self, _: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
            let polls = self.polls.fetch_add(1, Ordering::Relaxed) + 1;
            if polls < self.ready_after {
                return SearchProgress::Pending("ide-se-classes-searching");
            }
            for index in 0..3 {
                let item = SearchItem {
                    name: format!("{}-{}-{index}", self.id, query.text),
                    detail: String::new(),
                    hint: None,
                    ranges: Vec::new(),
                    detail_ranges: Vec::new(),
                    in_project: true,
                    available: true,
                    target: SearchTarget::File { path: PathBuf::from("x"), line: None, column: None },
                };
                if !sink.push(item) {
                    break;
                }
            }
            SearchProgress::Done
        }
    }

    struct Slash;

    impl SearchContributor for Slash {
        fn id(&self) -> &str {
            "commands"
        }

        fn title(&self) -> &str {
            "commands"
        }

        fn has_tab(&self) -> bool {
            false
        }

        fn weight(&self) -> i32 {
            10
        }

        fn commands(&self) -> bool {
            true
        }

        fn search(&self, _session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
            sink.push(SearchItem {
                name: format!("command {}", query.text),
                detail: String::new(),
                hint: None,
                ranges: Vec::new(),
                detail_ranges: Vec::new(),
                in_project: true,
                available: true,
                target: SearchTarget::Settings { page: "appearance".to_owned() },
            });
            SearchProgress::Done
        }
    }

    #[test]
    fn a_slash_asks_only_the_commands_and_an_expression_shows_its_value_first() {
        zen_i18n::register(crate::LOCALES);
        let plain = Slow::new("plain", 900, true, 1);
        let mut session = session_with(&[plain]);
        session.state_mut::<SearchContributors>().add(Arc::new(Slash));
        let mut run = SearchRun::default();
        let start = Instant::now();
        let contributors = |answer: &SearchAnswer| -> Vec<String> {
            answer.groups.iter().map(|group| group.contributor.clone()).collect()
        };
        let slash = run.poll_at(&mut session, &all("/ap"), start).unwrap();
        assert_eq!(contributors(&slash), ["commands"]);
        assert_eq!(names(&slash), ["command /ap"]);
        let typed = run.poll_at(&mut session, &all("a"), start).unwrap();
        assert_eq!(contributors(&typed), ["plain"], "the commands answer only a slash");
        let sum = run.poll_at(&mut session, &all("2 + 3*4"), start).unwrap();
        assert_eq!(contributors(&sum), ["calculator", "plain"]);
        let value = &sum.groups[0].items[0];
        assert_eq!(value.name, "2 + 3*4 = 14");
        assert_eq!(value.ranges, [[10, 12]]);
        assert_eq!(value.target, SearchTarget::Value { value: "14".to_owned() });
        assert_eq!(serde_json::to_value(&value.target).unwrap(), json!({ "kind": "value", "value": "14" }));
        let entries = filter_entries(&mut session, ALL).unwrap();
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        assert!(ids.contains(&"calculator") && !ids.contains(&"commands"), "{ids:?}");
        let options =
            OptionsParams { excluded_contributors: Some(vec!["calculator".to_owned()]), ..OptionsParams::default() };
        SetOptions.run(&mut session, options).unwrap();
        let left_out = run.poll_at(&mut session, &all("2 + 3*4"), start).unwrap();
        assert_eq!(contributors(&left_out), ["plain"], "the Filter leaves the calculator out");
    }

    struct Named(&'static [&'static str]);

    impl SearchContributor for Named {
        fn id(&self) -> &str {
            "names"
        }

        fn title(&self) -> &str {
            "names"
        }

        fn has_tab(&self) -> bool {
            true
        }

        fn weight(&self) -> i32 {
            900
        }

        fn search(&self, _: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
            let text = query.text.to_lowercase();
            for name in self.0.iter().filter(|name| name.to_lowercase().contains(&text)) {
                let item = SearchItem {
                    name: (*name).to_owned(),
                    detail: String::new(),
                    hint: None,
                    ranges: Vec::new(),
                    detail_ranges: Vec::new(),
                    in_project: true,
                    available: true,
                    target: SearchTarget::File { path: PathBuf::from(name), line: None, column: None },
                };
                if !sink.push(item) {
                    break;
                }
            }
            SearchProgress::Done
        }
    }

    #[test]
    fn a_typed_query_also_matches_in_the_other_layout_after_what_it_finds_as_typed() {
        let mut session = Session::new();
        let names = &["ьфшт notes.txt", "main.go", "domain main.rs", "заметки.md", "readme.md"];
        session.state_mut::<SearchContributors>().add(Arc::new(Named(names)));
        let found = |session: &mut Session, query: &str, typed: bool| {
            let request =
                SearchRequest { tab: "names".to_owned(), query: query.to_owned(), typed, ..SearchRequest::default() };
            let (mut run, mut now) = (SearchRun::default(), Instant::now());
            loop {
                let answer = run.poll_at(session, &request, now).unwrap();
                if !answer.pending {
                    return names_of(&answer);
                }
                now += POLL_EVERY;
            }
        };
        assert_eq!(
            found(&mut session, "ьфшт", true),
            ["ьфшт notes.txt", "main.go", "domain main.rs"],
            "Russian typed on the US keys: what the text finds as typed first, each once"
        );
        assert_eq!(found(&mut session, "ьфшт", false), ["ьфшт notes.txt"], "pasted, it is only itself");
        assert_eq!(found(&mut session, "pfvtnrb", true), ["заметки.md"], "the US layout read as Russian");
        assert!(found(&mut session, "pfvtnrb", false).is_empty());
        assert_eq!(found(&mut session, "readme", true), ["readme.md"], "a query that needs no reading stays as it is");
        let agent = QueryParams {
            tab: Some("names".to_owned()),
            query: "ьфшт".to_owned(),
            typed: true,
            wait_ms: Some(5000),
            ..QueryParams::default()
        };
        let answer = QueryEverywhere.run(&mut session, agent).unwrap();
        assert_eq!(names_of(&answer), ["ьфшт notes.txt", "main.go", "domain main.rs"], "agents ask with `typed`");
    }

    struct OneAtATime(Mutex<Option<String>>);

    impl SearchContributor for OneAtATime {
        fn id(&self) -> &str {
            "one"
        }

        fn title(&self) -> &str {
            "one"
        }

        fn weight(&self) -> i32 {
            950
        }

        fn primary(&self) -> bool {
            true
        }

        fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
            let mut running = self.0.lock().unwrap();
            if running.as_deref() != Some(query.text.as_str()) {
                *running = Some(query.text.clone());
                return SearchProgress::Pending("ide-se-classes-searching");
            }
            drop(running);
            Named(&["main.go", "ьфшт.txt"]).search(session, query, sink)
        }
    }

    #[test]
    fn the_other_layout_is_searched_after_the_typed_text_so_one_running_search_settles() {
        let mut session = Session::new();
        session.state_mut::<SearchContributors>().add(Arc::new(OneAtATime(Mutex::new(None))));
        let request =
            SearchRequest {
                tab: "one".to_owned(), query: "ьфшт".to_owned(), typed: true, ..SearchRequest::default()
            };
        let (mut run, mut now) = (SearchRun::default(), Instant::now());
        let mut shown = Vec::new();
        for _ in 0..10 {
            let answer = run.poll_at(&mut session, &request, now).unwrap();
            if !answer.held && shown.is_empty() {
                shown = names_of(&answer);
                assert_eq!(shown, ["ьфшт.txt"], "the typed text's items show without waiting for the rest");
            }
            if !answer.pending {
                assert_eq!(names_of(&answer), ["ьфшт.txt", "main.go"]);
                return;
            }
            now += POLL_EVERY;
        }
        panic!("the search never settled");
    }

    fn names_of(answer: &SearchAnswer) -> Vec<String> {
        answer.items().map(|item| item.name.clone()).collect()
    }

    fn session_with(added: &[Arc<Slow>]) -> Session {
        let mut session = Session::new();
        for contributor in added {
            session.state_mut::<SearchContributors>().add(Arc::clone(contributor) as Arc<dyn SearchContributor>);
        }
        session
    }

    fn names(answer: &SearchAnswer) -> Vec<String> {
        answer.items().map(|item| item.name.clone()).collect()
    }

    fn all(query: &str) -> SearchRequest {
        SearchRequest { tab: ALL.to_owned(), query: query.to_owned(), ..SearchRequest::default() }
    }

    #[test]
    fn more_searches_again_only_the_group_it_was_chosen_in() {
        let first = Slow::new("first", 900, true, 1);
        let second = Slow::new("second", 950, true, 1);
        let mut session = session_with(&[Arc::clone(&first), Arc::clone(&second)]);
        let mut run = SearchRun::default();
        let start = Instant::now();
        let request = SearchRequest { limit: Some(2), ..all("a") };
        let groups = |answer: &SearchAnswer| -> Vec<(usize, bool)> {
            answer.groups.iter().map(|group| (group.items.len(), group.more)).collect()
        };
        let shown = run.poll_at(&mut session, &request, start).unwrap();
        assert_eq!(groups(&shown), [(2, true), (2, true)]);
        let more = SearchRequest { more: BTreeMap::from([("second".to_owned(), 1)]), ..request };
        let grown = run.poll_at(&mut session, &more, start + Duration::from_millis(1)).unwrap();
        assert!(!grown.held && !grown.pending, "{grown:?}");
        assert_eq!(groups(&grown), [(2, true), (3, false)]);
        assert_eq!(first.polls.load(Ordering::Relaxed), 1, "the other group is not searched again");
        assert_eq!(second.polls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn results_are_held_until_the_primary_contributors_are_done() {
        let fast = Slow::new("fast", 900, true, 1);
        let slow = Slow::new("slow", 950, true, 3);
        let mut session = session_with(&[fast, slow]);
        let mut run = SearchRun::default();
        let start = Instant::now();
        let first = run.poll_at(&mut session, &all("a"), start).unwrap();
        assert!(first.held && first.pending);
        assert!(first.groups.is_empty(), "nothing shown while a primary contributor searches");
        run.poll_at(&mut session, &all("a"), start + POLL_EVERY).unwrap();
        let ready = run.poll_at(&mut session, &all("a"), start + POLL_EVERY * 2).unwrap();
        assert!(!ready.held && !ready.pending);
        assert_eq!(ready.groups.iter().map(|group| group.contributor.as_str()).collect::<Vec<_>>(), ["fast", "slow"]);
    }

    #[test]
    fn the_last_list_stays_until_the_new_one_is_ready_or_the_hold_ends() {
        let slow = Slow::new("slow", 900, true, 1);
        let mut session = session_with(&[Arc::clone(&slow)]);
        let mut run = SearchRun::default();
        let start = Instant::now();
        let shown = run.poll_at(&mut session, &all("a"), start).unwrap();
        assert_eq!(names(&shown), ["slow-a-0", "slow-a-1", "slow-a-2"]);
        let late = Slow::new("late", 950, true, usize::MAX);
        session.state_mut::<SearchContributors>().add(late as Arc<dyn SearchContributor>);
        let held = run.poll_at(&mut session, &all("ab"), start + Duration::from_millis(100)).unwrap();
        assert!(held.held);
        assert_eq!(names(&held), ["slow-a-0", "slow-a-1", "slow-a-2"], "the last query's list does not blink");
        let released = run.poll_at(&mut session, &all("ab"), start + Duration::from_millis(100) + HOLD).unwrap();
        assert!(!released.held && released.pending, "shown after 2000 ms, while the late one still searches");
        assert_eq!(names(&released), ["slow-ab-0", "slow-ab-1", "slow-ab-2"]);
    }

    #[test]
    fn a_secondary_contributor_does_not_hold_all_unless_waiting_for_all() {
        let fast = Slow::new("fast", 900, true, 1);
        let late = Slow::new("late", 950, false, 3);
        let mut session = session_with(&[Arc::clone(&fast), Arc::clone(&late)]);
        let mut run = SearchRun::default();
        let start = Instant::now();
        let answer = run.poll_at(&mut session, &all("q"), start).unwrap();
        assert!(!answer.held && answer.pending);
        assert_eq!(names(&answer), ["fast-q-0", "fast-q-1", "fast-q-2"]);

        let mut session = session_with(&[Slow::new("fast", 900, true, 1), Slow::new("late", 950, false, 3)]);
        session.settings_mut().set(WAIT_FOR_ALL, true).unwrap();
        let mut run = SearchRun::default();
        assert!(run.poll_at(&mut session, &all("q"), start).unwrap().held);
        assert!(run.poll_at(&mut session, &all("q"), start + HOLD * 2).unwrap().held, "no time limit then");
        let done = run.poll_at(&mut session, &all("q"), start + HOLD * 3).unwrap();
        assert!(!done.held && !done.pending);
        assert_eq!(done.groups.len(), 2);
    }

    #[test]
    fn a_new_query_cancels_the_last_and_done_contributors_are_not_asked_again() {
        let fast = Slow::new("fast", 900, true, 1);
        let mut session = session_with(&[Arc::clone(&fast)]);
        let mut run = SearchRun::default();
        let request = SearchRequest { tab: "fast".to_owned(), query: "x".to_owned(), ..SearchRequest::default() };
        run.poll(&mut session, &request).unwrap();
        let token = run.cancel.clone();
        run.poll(&mut session, &request).unwrap();
        assert_eq!(fast.polls.load(Ordering::Relaxed), 1);
        let request = SearchRequest { query: "y".to_owned(), ..request };
        run.poll(&mut session, &request).unwrap();
        assert!(token.is_cancelled());
        assert_eq!(fast.polls.load(Ordering::Relaxed), 2);
        let unknown = SearchRequest { tab: "nope".to_owned(), ..request };
        assert!(run.poll(&mut session, &unknown).is_err());
    }

    #[test]
    fn a_searching_contributor_is_asked_again_at_most_every_poll_interval() {
        let slow = Slow::new("slow", 900, true, usize::MAX);
        let mut session = session_with(&[Arc::clone(&slow)]);
        let mut run = SearchRun::default();
        let start = Instant::now();
        for frame in 0..10 {
            run.poll_at(&mut session, &all("a"), start + Duration::from_millis(frame)).unwrap();
        }
        assert_eq!(slow.polls.load(Ordering::Relaxed), 1, "frames in between do not search again");
        run.poll_at(&mut session, &all("a"), start + POLL_EVERY).unwrap();
        assert_eq!(slow.polls.load(Ordering::Relaxed), 2);
        run.poll_at(&mut session, &all("ab"), start + POLL_EVERY + Duration::from_millis(1)).unwrap();
        assert_eq!(slow.polls.load(Ordering::Relaxed), 3, "a new query searches at once");
    }

    #[test]
    fn the_command_waits_until_the_results_are_no_longer_held() {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = session_with(&[Slow::new("slow", 900, true, 3)]);
        let found = registry.dispatch(&mut session, "search.everywhere.query", json!({ "query": "q" })).unwrap();
        assert_eq!(found["held"], false, "{found}");
        assert_eq!(found["groups"][0]["items"][0]["name"], "slow-q-0");
    }

    #[test]
    fn the_history_is_kept_in_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("search-everywhere.json");
        let history = SearchHistory::load(&file);
        history.remember_query("cart");
        history.remember_query("store");
        history.remember_query("cart");
        history.remember_action("view.gotoLine.open");
        for index in 0..30 {
            history.remember_action(&format!("action{index}"));
        }
        let again = SearchHistory::load(&file).data();
        assert_eq!(again.queries, ["cart", "store"]);
        assert_eq!(again.actions.len(), RECENT_ACTIONS);
        assert_eq!(again.actions[0], "action29");
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(SearchHistory::load(&file));
        let listed = registry.dispatch(&mut session, "search.history.list", json!({})).unwrap();
        assert_eq!(listed["queries"], json!(["cart", "store"]));
        registry.dispatch(&mut session, "search.history.clear", json!({})).unwrap();
        assert_eq!(SearchHistory::load(&file).data(), HistoryData::default());
    }

    #[test]
    fn the_command_lists_files_and_the_contributors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cart.go"), "package cart\n").unwrap();
        std::fs::write(dir.path().join("readme.md"), "cart\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let found = registry
            .dispatch(
                &mut session,
                "search.everywhere.query",
                json!({ "tab": "files", "query": "cart", "waitMs": 5000 }),
            )
            .unwrap();
        assert_eq!(found["groups"][0]["contributor"], "files");
        assert_eq!(found["groups"][0]["items"][0]["name"], "cart.go");
        assert_eq!(found["groups"][0]["items"][0]["target"]["kind"], "file");
        assert_eq!(found["held"], false);
        let listed = registry.dispatch(&mut session, "search.everywhere.contributors", json!({})).unwrap();
        let ids: Vec<&str> = listed.as_array().unwrap().iter().filter_map(|info| info["id"].as_str()).collect();
        assert_eq!(ids, ["calculator", "recentFiles", "classes", "files", "symbols", "text"]);
    }

    #[test]
    fn a_group_with_more_items_ends_with_more_which_lists_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..10 {
            std::fs::write(dir.path().join(format!("item{index}.md")), "").unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let mut ask = |params: Value| -> (usize, Value) {
            let found = registry.dispatch(&mut session, "search.everywhere.query", params).unwrap();
            let files = found["groups"].as_array().unwrap().iter().find(|group| group["contributor"] == "files");
            let files = files.unwrap_or_else(|| panic!("no files: {found}"));
            (files["items"].as_array().unwrap().len(), files["more"].clone())
        };
        let query = |more: Value| json!({ "query": "item", "waitMs": 5000, "more": more });
        assert_eq!(ask(query(json!({}))), (ALL_LIMIT, json!(true)), "a few on All, then more…");
        assert_eq!(ask(query(json!({ "files": 1 }))), (10, json!(false)), "more… lists the rest");
        let tab = json!({ "tab": "files", "query": "item", "limit": 4, "waitMs": 5000 });
        assert_eq!(ask(tab), (4, json!(true)), "a tab of its own ends with more… too");
        let exact = json!({ "tab": "files", "query": "item", "limit": 10, "waitMs": 5000 });
        assert_eq!(ask(exact), (10, json!(false)), "no more… when everything is listed");
    }

    fn project() -> (Registry, Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("cart/model")).unwrap();
        std::fs::write(dir.path().join("cart/model/cart.go"), "package model\n\n    total := cartTotal(cart)\n")
            .unwrap();
        std::fs::write(dir.path().join("cart.md"), "the cart\n").unwrap();
        std::fs::write(dir.path().join("main.go"), "package main\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(SearchHistory::load(&dir.path().join("search-everywhere.json")));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        (registry, session, dir)
    }

    fn query(registry: &Registry, session: &mut Session, tab: &str, text: &str) -> Vec<String> {
        let found = registry
            .dispatch(session, "search.everywhere.query", json!({ "tab": tab, "query": text, "waitMs": 5000 }))
            .unwrap();
        found["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| {
                group["items"].as_array().unwrap().iter().map(|item| {
                    format!("{}:{}", group["contributor"].as_str().unwrap(), item["name"].as_str().unwrap())
                })
            })
            .collect()
    }

    #[test]
    fn files_lists_folders_text_finds_lines_and_an_empty_query_lists_only_recent_files() {
        let (registry, mut session, dir) = project();
        let found = query(&registry, &mut session, "files", "cart");
        assert_eq!(found, ["files:cart.md", "files:cart.go", "files:cart"], "the folder after the files as good");
        assert_eq!(query(&registry, &mut session, "files", "mod"), ["files:model"]);
        let lines = query(&registry, &mut session, "text", "cartTotal");
        assert_eq!(lines, ["text:total := cartTotal(cart)"], "the line without its indent");
        let answer = registry
            .dispatch(
                &mut session,
                "search.everywhere.query",
                json!({ "tab": "text", "query": "cartTotal", "waitMs": 5000 }),
            )
            .unwrap();
        let item = &answer["groups"][0]["items"][0];
        assert_eq!((item["hint"].clone(), item["ranges"].clone()), (json!("cart.go:3"), json!([[9, 18]])));
        assert_eq!((item["target"]["line"].clone(), item["target"]["column"].clone()), (json!(3), json!(14)));
        for tab in ["files", "classes", "symbols", "text"] {
            assert!(query(&registry, &mut session, tab, "").is_empty(), "{tab} lists nothing without a query");
        }
        for file in ["main.go", "cart.md"] {
            registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join(file) })).unwrap();
        }
        assert_eq!(query(&registry, &mut session, "all", ""), ["recentFiles:main.go"], "the current file left out");
    }

    fn wait_for_text(registry: &Registry, session: &mut Session, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let found = registry.dispatch(session, "search.text.find", json!({ "query": { "text": text } })).unwrap();
            if found["matches"].as_array().is_some_and(|matches| !matches.is_empty()) {
                return;
            }
            assert!(Instant::now() < deadline, "the text index timed out: {found}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn text_query(registry: &Registry, session: &mut Session, params: Value) -> Vec<String> {
        let mut params = params;
        params["tab"] = json!("text");
        params["waitMs"] = json!(5000);
        let found = registry.dispatch(session, "search.everywhere.query", params).unwrap();
        found["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["items"].as_array().unwrap().iter())
            .map(|item| format!("{} {}", item["hint"].as_str().unwrap(), item["name"].as_str().unwrap()))
            .collect()
    }

    #[test]
    fn text_matches_with_case_words_a_regex_and_a_file_mask_from_the_request_or_the_options() {
        let (registry, mut session, dir) = project();
        std::fs::write(dir.path().join("notes.md"), "Total cart\ncarts total\n").unwrap();
        let path = dir.path().join("notes.md");
        registry.dispatch(&mut session, "file.index.refresh", json!({ "paths": [path] })).unwrap();
        wait_for_text(&registry, &mut session, "carts total");
        wait_for_text(&registry, &mut session, "cartTotal");
        let plain = text_query(&registry, &mut session, json!({ "query": "total" }));
        assert_eq!(plain, ["cart.go:3 total := cartTotal(cart)", "notes.md:1 Total cart", "notes.md:2 carts total"]);
        let case = text_query(&registry, &mut session, json!({ "query": "Total", "matchCase": true }));
        assert_eq!(case, ["cart.go:3 total := cartTotal(cart)", "notes.md:1 Total cart"]);
        let words = text_query(&registry, &mut session, json!({ "query": "cart", "wholeWords": true }));
        assert_eq!(words, ["cart.md:1 the cart", "cart.go:3 total := cartTotal(cart)", "notes.md:1 Total cart"]);
        let regex = text_query(&registry, &mut session, json!({ "query": "carts? total$", "regex": true }));
        assert_eq!(regex, ["notes.md:2 carts total"]);
        let masked = text_query(&registry, &mut session, json!({ "query": "total", "mask": "*.md" }));
        assert_eq!(masked, ["notes.md:1 Total cart", "notes.md:2 carts total"]);
        let options = json!({ "textMatchCase": true, "textMask": " *.go " });
        let set = registry.dispatch(&mut session, "search.everywhere.setOptions", options).unwrap();
        assert_eq!((set["textMatchCase"].clone(), set["textMask"].clone()), (json!(true), json!("*.go")));
        assert_eq!(
            text_query(&registry, &mut session, json!({ "query": "Total" })),
            ["cart.go:3 total := cartTotal(cart)"]
        );
        let wider = text_query(&registry, &mut session, json!({ "query": "Total", "matchCase": false, "mask": "" }));
        assert_eq!(wider.len(), 3, "the request's choices win over the popup's: {wider:?}");
        let kept = SearchHistory::load(&dir.path().join("search-everywhere.json")).options();
        assert!(kept.text_match_case && !kept.text_whole_words && !kept.text_regex);
        assert_eq!(kept.text_mask, "*.go");
    }

    #[test]
    fn all_lists_text_last_only_when_the_other_groups_found_few_unless_turned_off() {
        let (registry, mut session, dir) = project();
        let contributors = |registry: &Registry, session: &mut Session, text: &str| -> Vec<String> {
            let params = json!({ "query": text, "waitMs": 5000 });
            let found = registry.dispatch(session, "search.everywhere.query", params).unwrap();
            assert_eq!(found["pending"], false, "{found}");
            found["groups"]
                .as_array()
                .unwrap()
                .iter()
                .map(|group| group["contributor"].as_str().unwrap().to_owned())
                .collect()
        };
        wait_for_text(&registry, &mut session, "cartTotal");
        let few = contributors(&registry, &mut session, "cartTotal");
        assert_eq!(few, ["text"], "nothing else found: the text");
        let cart = contributors(&registry, &mut session, "cart");
        assert_eq!(cart, ["files", "text"], "three files are few: text comes after them");
        let paths: Vec<PathBuf> = (0..ALL_LIMIT).map(|index| dir.path().join(format!("cart{index}.txt"))).collect();
        for path in &paths {
            std::fs::write(path, "cart\n").unwrap();
        }
        registry.dispatch(&mut session, "file.index.refresh", json!({ "paths": paths })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let params = json!({ "query": "cart", "limit": 50 });
            let found = registry.dispatch(&mut session, "navigate.file.goTo", params).unwrap();
            if found["files"].as_array().is_some_and(|files| files.len() > ALL_LIMIT) {
                break;
            }
            assert!(Instant::now() < deadline, "the file index timed out: {found}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let many = contributors(&registry, &mut session, "car");
        assert_eq!(many, ["files"], "the files found are many: no text");
        let entries = registry.dispatch(&mut session, "search.everywhere.filters", json!({ "tab": "all" })).unwrap();
        let text = entries.as_array().unwrap().iter().find(|entry| entry["id"] == "text");
        assert_eq!(text.map(|entry| entry["on"].clone()), Some(json!(true)), "Text is in the Filter of All: {entries}");
        registry
            .dispatch(&mut session, "search.everywhere.setOptions", json!({ "excludedContributors": ["text"] }))
            .unwrap();
        assert_eq!(
            contributors(&registry, &mut session, "cartTotal"),
            Vec::<String>::new(),
            "the Filter leaves it out"
        );
        registry.dispatch(&mut session, "search.everywhere.setOptions", json!({ "excludedContributors": [] })).unwrap();
        assert_eq!(contributors(&registry, &mut session, "cartTotal"), ["text"]);
        session.settings_mut().set(TEXT_ON_ALL, false).unwrap();
        assert_eq!(
            contributors(&registry, &mut session, "cartTotal"),
            Vec::<String>::new(),
            "the setting turns it off"
        );
        let listed = registry.dispatch(&mut session, "search.everywhere.contributors", json!({})).unwrap();
        let text = listed.as_array().unwrap().iter().find(|info| info["id"] == "text").unwrap();
        assert_eq!((text["fallback"].clone(), text["primary"].clone()), (json!(true), json!(false)));
    }

    #[test]
    fn a_new_query_cancels_the_text_search_of_the_last_and_the_search_stops_at_its_limit() {
        let (registry, mut session, dir) = project();
        let lines: String = (0..500).map(|index| format!("needle {index}\n")).collect();
        let path = dir.path().join("many.txt");
        std::fs::write(&path, lines).unwrap();
        registry.dispatch(&mut session, "file.index.refresh", json!({ "paths": [path] })).unwrap();
        wait_for_text(&registry, &mut session, "needle 499");
        let contributor = TextContributor::default();
        let query = |text: &str, limit: usize| SearchQuery {
            text: text.to_owned(),
            all: false,
            include_non_project: false,
            limit,
            scope: None,
            text_options: TextOptions::default(),
            cancel: CancelToken::default(),
        };
        let running = |contributor: &TextContributor| {
            contributor.running.lock().unwrap().as_ref().map(|run| run.search.clone()).unwrap()
        };
        let mut sink = SearchSink::new(51, CancelToken::default());
        contributor.search(&mut session, &query("needle", 51), &mut sink);
        let first = running(&contributor);
        let mut sink = SearchSink::new(51, CancelToken::default());
        contributor.search(&mut session, &query("needles", 51), &mut sink);
        assert!(first.is_cancelled(), "the next keystroke cancels the last search");
        let second = running(&contributor);
        assert!(!second.is_cancelled());
        drop(contributor);
        assert!(second.is_cancelled(), "a closed popup or a new query drops its contributor, which cancels");

        let contributor = TextContributor::default();
        let deadline = Instant::now() + Duration::from_secs(60);
        let items = loop {
            let mut sink = SearchSink::new(51, CancelToken::default());
            let progress = contributor.search(&mut session, &query("needle", 51), &mut sink);
            let items = sink.into_items();
            if progress == SearchProgress::Done && items.len() == 51 {
                break items;
            }
            assert!(Instant::now() < deadline, "the text search timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(items.len(), 51);
        assert!(running(&contributor).matches().len() <= text_limit(51), "the search stops at its limit");
    }

    #[test]
    fn the_filters_leave_out_what_is_unticked_and_are_kept_across_restarts() {
        let (registry, mut session, dir) = project();
        let filters = registry.dispatch(&mut session, "search.everywhere.filters", json!({ "tab": "files" })).unwrap();
        assert_eq!(filters[0], json!({ "id": "directory", "name": "Directory", "on": true }), "Directory first");
        let options =
            json!({ "excludedFileTypes": ["markdown", "directory"], "preview": true, "previewHeight": 300.0 });
        registry.dispatch(&mut session, "search.everywhere.setOptions", options).unwrap();
        assert_eq!(query(&registry, &mut session, "files", "cart"), ["files:cart.go"]);
        registry
            .dispatch(&mut session, "search.everywhere.setOptions", json!({ "excludedContributors": ["files"] }))
            .unwrap();
        assert!(!query(&registry, &mut session, "all", "cart").iter().any(|item| item.starts_with("files:")));
        let all = registry.dispatch(&mut session, "search.everywhere.filters", json!({ "tab": "all" })).unwrap();
        let files = all.as_array().unwrap().iter().find(|entry| entry["id"] == "files").unwrap();
        assert_eq!(files["on"], false);
        assert!(registry.dispatch(&mut session, "search.everywhere.filters", json!({ "tab": "actions" })).is_err());
        let kept = SearchHistory::load(&dir.path().join("search-everywhere.json")).options();
        assert!(kept.preview);
        assert_eq!(kept.preview_height, 300.0);
        assert_eq!(kept.excluded_file_types, ["directory", "markdown"]);
        assert_eq!(kept.excluded_contributors, ["files"]);
    }

    #[test]
    fn open_in_find_keeps_the_results_for_the_find_tool_window_except_on_actions() {
        let (registry, mut session, _dir) = project();
        let found = registry
            .dispatch(&mut session, "search.everywhere.openInFind", json!({ "tab": "files", "query": "main" }))
            .unwrap();
        assert_eq!((found["search"].clone(), found["title"].clone()), (json!(1), json!("ide-se-files")));
        assert_eq!(found["groups"][0]["items"][0]["name"], "main.go");
        assert_eq!(session.state::<FoundEverywhere>().unwrap().search, 1);
        let refused =
            registry.dispatch(&mut session, "search.everywhere.openInFind", json!({ "tab": "actions", "query": "x" }));
        assert!(refused.is_err());
    }

    #[test]
    fn the_type_tab_is_named_as_the_language_names_types() {
        let mut session = Session::new();
        let title = |session: &Session| {
            contributors(session).iter().find(|contributor| contributor.id() == "classes").unwrap().title().to_owned()
        };
        assert_eq!(title(&session), "ide-se-classes");
        session.set_state(TypeNoun::Type);
        assert_eq!(title(&session), "ide-se-types");
    }
}

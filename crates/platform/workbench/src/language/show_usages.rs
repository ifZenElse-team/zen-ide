use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::SymbolKind;

use super::highlights::{HighlightKind, HighlightRange, server_highlights};
use super::places::{Place, TextCache, place, places};
use super::semantic::{SemanticTokens, Token, TokensParams};
use super::structure::{self, Symbol};
use super::usages::{FindUsages, FindUsagesParams, UsagesState, group, word_at};
use super::{Languages, LookupKind, LookupParams, WaitParams, file_text, with_languages};
use crate::Workbench;

pub const PAGE: usize = 100;
pub(crate) const DECLARATION_FILES: usize = 16;
const STRUCTURE_WAIT: Duration = Duration::from_millis(300);
const GENERATED_LINES: usize = 40;
const IMPORT_LINES: usize = 400;
const MANIFESTS: [&str; 5] = ["go.mod", "Cargo.toml", "package.json", "pyproject.toml", "pom.xml"];

/// Where Show Usages looks, from the narrowest to the widest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UsagesScope {
    /// The file the search started in.
    CurrentFile,
    /// The files open in the editor.
    OpenFiles,
    /// The files of the project.
    #[default]
    ProjectFiles,
    /// Every place the language server found, libraries included.
    AllPlaces,
}

impl UsagesScope {
    pub fn wider(self) -> Self {
        match self {
            Self::CurrentFile => Self::OpenFiles,
            Self::OpenFiles => Self::ProjectFiles,
            Self::ProjectFiles | Self::AllPlaces => Self::AllPlaces,
        }
    }
}

/// What the symbol whose usages are shown is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DeclarationKind {
    /// A variable.
    Variable,
    /// A parameter of a function.
    Parameter,
    /// A constant.
    Constant,
    /// A function.
    Function,
    /// A method.
    Method,
    /// A named type.
    Type,
    /// A struct type.
    Struct,
    /// An interface or trait.
    Interface,
    /// A class.
    Class,
    /// A field of a struct.
    Field,
    /// A property.
    Property,
    /// A package or namespace.
    Package,
    /// A module.
    Module,
    /// Anything else, or not known.
    #[default]
    Symbol,
}

impl DeclarationKind {
    fn of_symbol(kind: SymbolKind) -> Self {
        match kind {
            SymbolKind::Struct => Self::Struct,
            SymbolKind::Interface => Self::Interface,
            SymbolKind::Type => Self::Type,
            SymbolKind::Function => Self::Function,
            SymbolKind::Method => Self::Method,
            SymbolKind::Field => Self::Field,
            SymbolKind::Constant => Self::Constant,
            SymbolKind::Variable => Self::Variable,
            SymbolKind::Package => Self::Package,
            SymbolKind::Module => Self::Module,
            SymbolKind::Class => Self::Class,
            SymbolKind::Property | SymbolKind::Key => Self::Property,
            _ => Self::Symbol,
        }
    }

    fn of_token(token: &Token) -> Self {
        let readonly = token.modifiers.iter().any(|modifier| modifier == "readonly");
        match token.kind.as_str() {
            "variable" if readonly => Self::Constant,
            "variable" => Self::Variable,
            "parameter" => Self::Parameter,
            "function" => Self::Function,
            "method" => Self::Method,
            "type" | "typeParameter" => Self::Type,
            "struct" => Self::Struct,
            "interface" => Self::Interface,
            "class" => Self::Class,
            "property" => Self::Property,
            "enumMember" => Self::Constant,
            "namespace" => Self::Package,
            _ => Self::Symbol,
        }
    }
}

/// How a usage uses the symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UsageAccess {
    /// The symbol is read there.
    #[default]
    Read,
    /// The symbol is written there: assigned or changed.
    Write,
}

/// The toggles of the Show Usages popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShowUsagesOptions {
    /// Merge Usages from the Same Line (⌃L): one row for the usages of a line.
    pub merge_same_line: bool,
    /// Show Read Access (⌃R): the usages that read the symbol are listed.
    pub read: bool,
    /// Show Write Access (⌃W): the usages that write the symbol are listed.
    pub write: bool,
    /// Show Results In Generated Code: the usages in generated files are listed.
    pub generated: bool,
    /// Show Import Statements (⌃I): the usages in imports are listed.
    pub imports: bool,
    /// Show Usages In Comments: the name's occurrences in comments are listed too.
    pub comments: bool,
    /// Preview Source: the lines around the selected usage are shown under the list.
    pub preview: bool,
}

impl Default for ShowUsagesOptions {
    fn default() -> Self {
        Self {
            merge_same_line: false,
            read: true,
            write: true,
            generated: true,
            imports: true,
            comments: false,
            preview: false,
        }
    }
}

/// A row of the popup: a usage, or the usages of one line when they are merged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    /// The file.
    pub path: PathBuf,
    /// The file's path relative to the project, or the whole path outside it.
    pub relative: String,
    /// The file's name.
    pub file_name: String,
    /// The module the file belongs to, shown grey after its name.
    pub module: Option<String>,
    /// The line, one-based.
    pub line: u32,
    /// The column of the first usage, one-based, in characters.
    pub column: u32,
    /// The first character of the first usage in the file.
    pub start: usize,
    /// The character after the first usage.
    pub end: usize,
    /// The line's text without its leading and trailing white space.
    pub preview: String,
    /// The usages within the preview, in characters, shown in bold.
    pub highlights: Vec<[usize; 2]>,
    /// How the row uses the symbol: a write when any of its usages writes it.
    pub access: UsageAccess,
    /// Whether the row is an occurrence of the name in a comment.
    pub comment: bool,
    /// How many usages the row holds.
    pub count: usize,
}

/// The rows of one file as agents get them (card ZneOu428): the file once, then its rows without it.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageRowsFile<'a> {
    /// The file's path relative to the project, with `/`, or the whole path outside it.
    pub path: &'a str,
    /// The module the file belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<&'a str>,
    /// Its rows, in order.
    pub rows: Vec<RowInFile<'a>>,
}

/// A [`UsageRow`] without its file, which its [`UsageRowsFile`] names.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowInFile<'a> {
    /// The line, one-based.
    pub line: u32,
    /// The column of the first usage, one-based, in characters.
    pub column: u32,
    /// The first character of the first usage in the file.
    pub start: usize,
    /// The character after the first usage.
    pub end: usize,
    /// The line's text without its leading and trailing white space.
    pub preview: &'a str,
    /// The usages within the preview, in characters.
    pub highlights: &'a [[usize; 2]],
    /// How the row uses the symbol.
    pub access: UsageAccess,
    /// Whether the row is an occurrence of the name in a comment; left out when it is not.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub comment: bool,
    /// How many usages the row holds; left out when it is one.
    #[serde(skip_serializing_if = "is_one")]
    pub count: usize,
}

fn is_one(count: &usize) -> bool {
    *count == 1
}

fn by_file<S: serde::Serializer>(rows: &[UsageRow], serializer: S) -> Result<S::Ok, S::Error> {
    let mut files: Vec<UsageRowsFile<'_>> = Vec::new();
    let mut index: HashMap<&Path, usize> = HashMap::new();
    for row in rows {
        let at = *index.entry(row.path.as_path()).or_insert_with(|| {
            files.push(UsageRowsFile { path: &row.relative, module: row.module.as_deref(), rows: Vec::new() });
            files.len() - 1
        });
        files[at].rows.push(RowInFile {
            line: row.line,
            column: row.column,
            start: row.start,
            end: row.end,
            preview: &row.preview,
            highlights: &row.highlights,
            access: row.access,
            comment: row.comment,
            count: row.count,
        });
    }
    serializer.collect_seq(files)
}

/// What the Show Usages popup shows. Agents get its rows grouped by file, as `files`; the popup
/// reads it with [`view`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShowUsagesState {
    /// The number of the search, raised by each new Show Usages; 0 before the first.
    pub search: u64,
    /// Raised by every change of what the popup shows.
    pub generation: u64,
    /// Whether the popup is open.
    pub open: bool,
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// The file the search started in.
    pub path: Option<PathBuf>,
    /// The character offset the search started at.
    pub offset: Option<usize>,
    /// The symbol, as written at its place.
    pub symbol: Option<String>,
    /// What the symbol is.
    pub kind: DeclarationKind,
    /// The file the symbol is declared in, else the file the search started in.
    pub declaration: Option<PathBuf>,
    /// That file's path relative to the project.
    pub declaration_relative: Option<String>,
    /// Where the usages are looked for.
    pub scope: UsagesScope,
    /// The popup's toggles.
    pub options: ShowUsagesOptions,
    /// Whether the Find Usages Settings panel of the popup is open.
    pub settings: bool,
    /// How many usages pass the scope and the toggles.
    pub total: usize,
    /// How many usages the server found, before the scope and the toggles.
    pub found: usize,
    /// The rows listed so far; agents get them grouped by file, as `files`.
    #[serde(rename = "files", serialize_with = "by_file")]
    #[schemars(with = "Vec<UsageRowsFile<'static>>")]
    pub rows: Vec<UsageRow>,
    /// How many more rows "more usages" can list.
    pub more: usize,
    /// Why nothing was found, in words for the user.
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
struct Usage {
    place: Place,
    relative: String,
    module: Option<String>,
    access: UsageAccess,
    generated: bool,
    import: bool,
    comment: bool,
}

#[derive(Debug)]
pub struct ShowUsages {
    generation: u64,
    search: u64,
    open: bool,
    pending: bool,
    origin: Option<(PathBuf, usize)>,
    symbol: Option<String>,
    kind: DeclarationKind,
    declaration: Option<(PathBuf, String)>,
    scope: UsagesScope,
    options: ShowUsagesOptions,
    settings: bool,
    limit: usize,
    found: usize,
    usages: Vec<Usage>,
    error: Option<String>,
}

impl Default for ShowUsages {
    fn default() -> Self {
        Self {
            generation: 0,
            search: 0,
            open: false,
            pending: false,
            origin: None,
            symbol: None,
            kind: DeclarationKind::Symbol,
            declaration: None,
            scope: UsagesScope::default(),
            options: ShowUsagesOptions::default(),
            settings: false,
            limit: PAGE,
            found: 0,
            usages: Vec::new(),
            error: None,
        }
    }
}

impl ShowUsages {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }

    fn touch(&mut self) {
        self.generation += 1;
    }
}

/// The parameters of `navigate.usages.show` and `navigate.usages.settings`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShowUsagesParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in the file; `line` and `column` instead, else the primary caret, when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// The line, one-based as answers count them: with `column`, the place instead of `offset`.
    #[serde(default)]
    pub line: Option<u32>,
    /// The column on `line`, one-based, in characters; 1 when absent.
    #[serde(default)]
    pub column: Option<u32>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// The scope; Project Files for a new search, the next wider one when asked again at the same place.
    #[serde(default)]
    pub scope: Option<UsagesScope>,
}

impl ShowUsagesParams {
    fn lookup(&self) -> LookupParams {
        LookupParams {
            path: self.path.clone(),
            offset: self.offset,
            line: self.line,
            column: self.column,
            wait_ms: self.wait_ms,
        }
    }
}

/// The parameters of `navigate.showUsages.setOptions`: the toggles to change, the others kept.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShowUsagesOptionsParams {
    /// Merge Usages from the Same Line.
    #[serde(default)]
    pub merge_same_line: Option<bool>,
    /// Show Read Access.
    #[serde(default)]
    pub read: Option<bool>,
    /// Show Write Access.
    #[serde(default)]
    pub write: Option<bool>,
    /// Show Results In Generated Code.
    #[serde(default)]
    pub generated: Option<bool>,
    /// Show Import Statements.
    #[serde(default)]
    pub imports: Option<bool>,
    /// Show Usages In Comments.
    #[serde(default)]
    pub comments: Option<bool>,
    /// Preview Source.
    #[serde(default)]
    pub preview: Option<bool>,
    /// The scope.
    #[serde(default)]
    pub scope: Option<UsagesScope>,
    /// Whether the Find Usages Settings panel is open.
    #[serde(default)]
    pub settings: Option<bool>,
}

fn not_image(session: &Session) -> bool {
    !session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .is_some_and(|document| document.image().is_some())
}

fn start(session: &mut Session, params: &ShowUsagesParams) -> Result<(), CommandError> {
    let (path, offset) = params.lookup().place(session.state_mut::<Workbench>())?;
    let state = session.state_mut::<ShowUsages>();
    if state.open && state.origin.as_ref().is_some_and(|origin| *origin == (path.clone(), offset)) {
        state.scope = params.scope.unwrap_or_else(|| state.scope.wider());
        state.limit = PAGE;
        state.touch();
        return Ok(());
    }
    let symbol = {
        let text = file_text(session.state_mut::<Workbench>(), &path).map_err(CommandError::new)?;
        word_at(&text, offset).ok_or_else(|| CommandError::new(t!("workbench-nav-no-symbol")))?
    };
    let context = json!({ "context": { "includeDeclaration": true } });
    with_languages(session, |languages, workbench| {
        let started = languages
            .start_lookup(workbench, LookupKind::ShowUsages, &path, offset, "textDocument/references", context)
            .map(|_| ());
        started.map_err(|message| languages.lookup_error(&path, message))
    })?;
    let state = session.state_mut::<ShowUsages>();
    let options = state.options;
    *state = ShowUsages {
        generation: state.generation + 1,
        search: state.search + 1,
        open: true,
        pending: true,
        origin: Some((path, offset)),
        symbol: Some(symbol),
        scope: params.scope.unwrap_or_default(),
        options,
        ..ShowUsages::default()
    };
    Ok(())
}

fn poll(session: &mut Session, wait: Duration) {
    if !session.state::<ShowUsages>().is_some_and(|state| state.pending) {
        return;
    }
    let languages = session.state_mut::<Languages>();
    let answer = if languages.lookup(LookupKind::ShowUsages).is_none() {
        Some(Err(t!("workbench-nav-no-usages")))
    } else if languages.is_outdated(LookupKind::ShowUsages) {
        languages.end_lookup(LookupKind::ShowUsages);
        Some(Err(t!("workbench-nav-usages-outdated")))
    } else if languages.lookup_mut(LookupKind::ShowUsages).is_some_and(|lookup| lookup.poll(wait)) {
        languages.end_lookup(LookupKind::ShowUsages).and_then(|lookup| lookup.answer)
    } else {
        None
    };
    let Some(answer) = answer else { return };
    let mut cache = TextCache::new();
    let collected = answer.map(|answer| {
        let found = with_languages(session, |languages, _| places(&answer, languages, &mut cache));
        collect(session, found, &mut cache)
    });
    let state = session.state_mut::<ShowUsages>();
    state.pending = false;
    state.touch();
    match collected {
        Ok(Some(collected)) => {
            state.kind = collected.kind;
            state.declaration = collected.declaration;
            state.found = collected.usages.iter().filter(|usage| !usage.comment).count();
            state.usages = collected.usages;
            if state.found == 0 {
                state.error = Some(t!("workbench-nav-no-usages"));
            }
        }
        Ok(None) => {}
        Err(error) => state.error = Some(error),
    }
}

struct Collected {
    kind: DeclarationKind,
    declaration: Option<(PathBuf, String)>,
    usages: Vec<Usage>,
}

fn collect(session: &mut Session, mut found: Vec<Place>, cache: &mut TextCache) -> Option<Collected> {
    let state = session.state::<ShowUsages>()?;
    let (origin, offset) = state.origin.clone()?;
    let symbol = state.symbol.clone()?;
    let mut seen = HashSet::new();
    found.retain(|place| seen.insert((place.path.clone(), place.start, place.end)));
    let (declared, kind) = declaration(session, &origin, offset, &symbol, &found);
    let declaration = declared.map(|index| found.remove(index));
    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let mut comment_files: Vec<PathBuf> = found.iter().map(|place| place.path.clone()).collect();
    comment_files.extend(declaration.as_ref().map(|place| place.path.clone()));
    comment_files.push(origin.clone());
    let mut known: HashSet<(PathBuf, usize)> = found.iter().map(|place| (place.path.clone(), place.start)).collect();
    known.extend(declaration.as_ref().map(|place| (place.path.clone(), place.start)));
    let mut in_comments = Vec::new();
    let mut listed = HashSet::new();
    for path in comment_files {
        if !listed.insert(path.clone()) {
            continue;
        }
        let Some(text) = with_languages(session, |languages, _| cache.text(languages, &path).map(str::to_owned)) else {
            continue;
        };
        for range in word_in_comments(&text, &symbol) {
            let range = json!({
                "start": zen_lsp::position::position(&text, range.start),
                "end": zen_lsp::position::position(&text, range.end),
            });
            if let Some(found) = place(path.clone(), &text, &range)
                && !known.contains(&(found.path.clone(), found.start))
            {
                in_comments.push(found);
            }
        }
    }
    let comments: HashSet<(PathBuf, usize)> =
        in_comments.iter().map(|place| (place.path.clone(), place.start)).collect();
    found.extend(in_comments);
    let mut modules = HashMap::new();
    let mut usages = Vec::new();
    for file in group(found, project.as_deref()) {
        let highlights = server_highlights(session, &file.path);
        let text = with_languages(session, |languages, _| cache.text(languages, &file.path).map(str::to_owned));
        let generated = text.as_deref().is_some_and(generated);
        let commented = text.as_deref().map(comment_ranges).unwrap_or_default();
        let module = module_of(&file.path, project.as_deref(), &mut modules);
        let starts: Vec<usize> = file.usages.iter().map(|place| place.start).collect();
        let bytes = text.as_deref().map(|text| byte_offsets(text, &starts));
        for (index, place) in file.usages.into_iter().enumerate() {
            let byte = bytes.as_ref().map(|bytes| bytes[index]);
            let comment = comments.contains(&(place.path.clone(), place.start))
                || byte.is_some_and(|byte| {
                    let after = commented.partition_point(|range| range.end <= byte);
                    commented.get(after).is_some_and(|range| range.contains(&byte))
                });
            let import = text.as_deref().zip(byte).is_some_and(|(text, byte)| in_import(text, byte));
            usages.push(Usage {
                access: access(&place, &highlights),
                relative: file.relative.clone(),
                module: module.clone(),
                generated,
                import,
                comment,
                place,
            });
        }
    }
    let declaration = declaration.map(|place| {
        let relative = relative(&place.path, project.as_deref());
        (place.path, relative)
    });
    Some(Collected { kind, declaration, usages })
}

fn byte_offsets(text: &str, chars: &[usize]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..chars.len()).collect();
    order.sort_by_key(|&index| chars[index]);
    let mut bytes = vec![text.len(); chars.len()];
    let mut next = 0;
    for (count, (byte, _)) in text.char_indices().enumerate() {
        while next < order.len() && chars[order[next]] <= count {
            bytes[order[next]] = byte;
            next += 1;
        }
        if next == order.len() {
            break;
        }
    }
    bytes
}

fn relative(path: &Path, project: Option<&Path>) -> String {
    match project.and_then(|root| path.strip_prefix(root).ok()) {
        Some(inside) => {
            inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
        }
        None => path.display().to_string(),
    }
}

fn flatten<'a>(symbols: &'a [Symbol], into: &mut Vec<&'a Symbol>) {
    for symbol in symbols {
        into.push(symbol);
        flatten(&symbol.children, into);
    }
}

fn declaration(
    session: &mut Session,
    origin: &Path,
    offset: usize,
    name: &str,
    found: &[Place],
) -> (Option<usize>, DeclarationKind) {
    let mut files: Vec<PathBuf> = vec![origin.to_owned()];
    for place in found {
        if files.len() < DECLARATION_FILES && !files.contains(&place.path) {
            files.push(place.path.clone());
        }
    }
    let tokens_of = |session: &mut Session, path: &Path| -> Vec<Token> {
        SemanticTokens
            .run(session, TokensParams { path: Some(path.to_owned()), since: None })
            .ok()
            .and_then(|answer| answer.tokens)
            .unwrap_or_default()
    };
    let mut at_origin = None;
    for path in &files {
        let tokens = tokens_of(session, path);
        if path == origin {
            at_origin = tokens.iter().find(|token| (token.start..=token.end).contains(&offset)).cloned();
        }
        let here = || found.iter().enumerate().filter(move |(_, place)| &place.path == path);
        for (index, place) in here() {
            let declares = tokens.iter().find(|token| {
                token.start == place.start
                    && token.modifiers.iter().any(|modifier| modifier == "definition" || modifier == "declaration")
            });
            if let Some(token) = declares {
                return (Some(index), DeclarationKind::of_token(token));
            }
        }
        let wait = if path == origin { STRUCTURE_WAIT } else { Duration::ZERO };
        let symbols = structure::resolve(session, path, wait).symbols;
        let mut all = Vec::new();
        flatten(&symbols, &mut all);
        for (index, place) in here() {
            if let Some(symbol) = all.iter().find(|symbol| symbol.name == name && symbol.selection_start == place.start)
            {
                return (Some(index), DeclarationKind::of_symbol(symbol.kind));
            }
        }
    }
    (None, at_origin.as_ref().map_or(DeclarationKind::Symbol, DeclarationKind::of_token))
}

pub(crate) fn kind_at(
    session: &mut Session,
    path: &Path,
    offset: usize,
    name: &str,
    files: &[PathBuf],
) -> DeclarationKind {
    let (_, kind) = declaration(session, path, offset, name, &[]);
    if kind != DeclarationKind::Symbol {
        return kind;
    }
    let mut seen = HashSet::new();
    for file in std::iter::once(path).chain(files.iter().map(PathBuf::as_path)).take(DECLARATION_FILES) {
        if !seen.insert(file.to_owned()) {
            continue;
        }
        let wait = if file == path { STRUCTURE_WAIT } else { Duration::ZERO };
        let symbols = structure::resolve(session, file, wait).symbols;
        let mut all = Vec::new();
        flatten(&symbols, &mut all);
        if let Some(symbol) = all.iter().find(|symbol| symbol.name == name) {
            return DeclarationKind::of_symbol(symbol.kind);
        }
    }
    DeclarationKind::Symbol
}

fn access(place: &Place, server: &[HighlightRange]) -> UsageAccess {
    match server.iter().find(|range| range.start == place.start).map(|range| range.kind) {
        Some(HighlightKind::Write) => return UsageAccess::Write,
        Some(HighlightKind::Read) => return UsageAccess::Read,
        _ => {}
    }
    let (Some(preview), Some([from, to])) = (&place.preview, place.highlight) else { return UsageAccess::Read };
    let before: String = preview.chars().take(from).collect();
    let after: String = preview.chars().skip(to).collect();
    if written(&before, &after) { UsageAccess::Write } else { UsageAccess::Read }
}

pub fn written(before: &str, after: &str) -> bool {
    let after = after.trim_start();
    let before = before.trim_end();
    const CHANGES: [&str; 14] = [":=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", "&^=", "++", "--"];
    let assigned = after.starts_with('=') && !after.starts_with("==") && !after.starts_with("=>");
    assigned
        || CHANGES.iter().any(|change| after.starts_with(change))
        || before.ends_with("++")
        || before.ends_with("--")
}

pub fn generated(text: &str) -> bool {
    text.lines().take(GENERATED_LINES).any(|line| {
        let line = line.trim_start();
        let comment = ["//", "#", "/*", "*", "--", "<!--"].iter().any(|start| line.starts_with(start));
        comment
            && ((line.contains("Code generated") && line.contains("DO NOT EDIT"))
                || line.contains("@generated")
                || line.contains("This file should not be edited"))
    })
}

pub fn in_import(text: &str, at: usize) -> bool {
    let line_start = text[..at].rfind('\n').map_or(0, |index| index + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |index| at + index);
    let line = text[line_start..line_end].trim_start();
    let statement = ["import", "use", "from"].iter().any(|keyword| {
        line.strip_prefix(keyword).is_some_and(|rest| {
            let operand = rest.trim_start();
            (rest.starts_with([' ', '\t']) || (*keyword == "import" && rest.starts_with('(')))
                && !operand.is_empty()
                && !operand
                    .starts_with(['=', ':', ',', ';', ')', '+', '-', '<', '>', '!', '&', '|', '[', '%', '/', '^'])
        })
    });
    if statement {
        return true;
    }
    for before in text[..line_start].lines().rev().take(IMPORT_LINES) {
        let before = before.trim();
        if before.strip_prefix("import").is_some_and(|rest| rest.trim() == "(") {
            return true;
        }
        let entry = before.is_empty() || before.starts_with("//") || (before.contains('"') && !before.contains('('));
        if !entry {
            return false;
        }
    }
    false
}

pub fn comment_ranges(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'"' | b'\'' | b'`') => {
                at += 1;
                while at < bytes.len() && bytes[at] != quote {
                    if quote != b'`' && bytes[at] == b'\n' {
                        break;
                    }
                    if quote != b'`' && bytes[at] == b'\\' {
                        at += 1;
                    }
                    at += 1;
                }
                at += 1;
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                let end = text[at..].find('\n').map_or(text.len(), |index| at + index);
                ranges.push(at..end);
                at = end;
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                let end = text[at + 2..].find("*/").map_or(text.len(), |index| at + 2 + index + 2);
                ranges.push(at..end);
                at = end;
            }
            _ => at += 1,
        }
    }
    ranges
}

pub fn word_in_comments(text: &str, name: &str) -> Vec<Range<usize>> {
    if name.is_empty() {
        return Vec::new();
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    for comment in comment_ranges(text) {
        let body = &text[comment.clone()];
        for (index, _) in body.match_indices(name) {
            let start = comment.start + index;
            let end = start + name.len();
            let open = text[..start].chars().next_back().is_none_or(|c| !is_word(c));
            let close = text[end..].chars().next().is_none_or(|c| !is_word(c));
            if open && close {
                found.push(start..end);
            }
        }
    }
    found
}

fn module_of(path: &Path, project: Option<&Path>, known: &mut HashMap<PathBuf, Option<String>>) -> Option<String> {
    let folder = path.parent()?;
    if let Some(module) = known.get(folder) {
        return module.clone();
    }
    let module = folder
        .ancestors()
        .find(|dir| MANIFESTS.iter().any(|manifest| dir.join(manifest).is_file()))
        .or_else(|| project.filter(|root| path.starts_with(root)))
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    known.insert(folder.to_owned(), module.clone());
    module
}

pub(crate) fn in_scope(
    scope: UsagesScope,
    path: &Path,
    origin: Option<&Path>,
    open: &[PathBuf],
    project: Option<&Path>,
) -> bool {
    match scope {
        UsagesScope::CurrentFile => origin == Some(path),
        UsagesScope::OpenFiles => open.iter().any(|file| file == path),
        UsagesScope::ProjectFiles => project.is_none_or(|root| path.starts_with(root)),
        UsagesScope::AllPlaces => true,
    }
}

fn passes(options: &ShowUsagesOptions, usage: &Usage) -> bool {
    if usage.comment {
        return options.comments;
    }
    let access = match usage.access {
        UsageAccess::Read => options.read,
        UsageAccess::Write => options.write,
    };
    access && (options.generated || !usage.generated) && (options.imports || !usage.import)
}

fn row(usage: &Usage) -> UsageRow {
    let place = &usage.place;
    UsageRow {
        path: place.path.clone(),
        relative: usage.relative.clone(),
        file_name: place.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        module: usage.module.clone(),
        line: place.line,
        column: place.column,
        start: place.start,
        end: place.end,
        preview: place.preview.clone().unwrap_or_default(),
        highlights: place.highlight.into_iter().collect(),
        access: usage.access,
        comment: usage.comment,
        count: 1,
    }
}

pub fn view(session: &Session) -> ShowUsagesState {
    let Some(state) = session.state::<ShowUsages>() else { return ShowUsagesState::default() };
    let workbench = session.state::<Workbench>();
    let project = workbench.and_then(Workbench::project);
    let open: Vec<PathBuf> = workbench
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default();
    let origin = state.origin.as_ref().map(|(path, _)| path.as_path());
    let shown: Vec<&Usage> = state
        .usages
        .iter()
        .filter(|usage| in_scope(state.scope, &usage.place.path, origin, &open, project))
        .filter(|usage| passes(&state.options, usage))
        .collect();
    let mut rows: Vec<UsageRow> = Vec::new();
    for usage in &shown {
        let merged = state.options.merge_same_line
            && rows.last().is_some_and(|last| last.path == usage.place.path && last.line == usage.place.line);
        if merged {
            let last = rows.last_mut().expect("checked");
            last.highlights.extend(usage.place.highlight);
            last.count += 1;
            if usage.access == UsageAccess::Write {
                last.access = UsageAccess::Write;
            }
        } else {
            rows.push(row(usage));
        }
    }
    let all = rows.len();
    rows.truncate(state.limit);
    let (declaration, declaration_relative) = match (&state.declaration, &state.origin) {
        (Some((path, relative)), _) => (Some(path.clone()), Some(relative.clone())),
        (None, Some((path, _))) => (Some(path.clone()), Some(relative(path, project))),
        (None, None) => (None, None),
    };
    ShowUsagesState {
        search: state.search,
        generation: state.generation,
        open: state.open,
        pending: state.pending,
        path: state.origin.as_ref().map(|(path, _)| path.clone()),
        offset: state.origin.as_ref().map(|(_, offset)| *offset),
        symbol: state.symbol.clone(),
        kind: state.kind,
        declaration,
        declaration_relative,
        scope: state.scope,
        options: state.options,
        settings: state.settings,
        total: shown.len(),
        found: state.found,
        more: all - rows.len(),
        rows,
        error: state.error.clone(),
    }
}

pub struct ShowUsagesAt;

impl Command for ShowUsagesAt {
    type Params = ShowUsagesParams;
    type Output = ShowUsagesState;
    const ID: &'static str = "navigate.usages.show";
    const TITLE: &'static str = "Show Usages";
    const DESCRIPTION: &'static str = "Shows the usages of the symbol at a place (the caret by default) in the Show Usages \
         popup, a hundred at a time; asked again at the same place while the popup is open, it widens the scope \
         (Current File, Open Files, Project Files, All Places). navigate.showUsages.get reads the popup.";

    fn is_available(&self, session: &Session) -> bool {
        not_image(session)
    }

    fn run(&self, session: &mut Session, params: ShowUsagesParams) -> Result<ShowUsagesState, CommandError> {
        start(session, &params)?;
        poll(session, params.lookup().wait());
        Ok(view(session))
    }
}

pub struct GetShowUsages;

impl Command for GetShowUsages {
    type Params = WaitParams;
    type Output = ShowUsagesState;
    const ID: &'static str = "navigate.showUsages.get";
    const TITLE: &'static str = "Show Usages State";
    const DESCRIPTION: &'static str = "What the Show Usages popup shows: the symbol, its kind and file, the scope, the \
         toggles and the rows listed so far, waiting up to waitMs for the server's answer.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<ShowUsagesState, CommandError> {
        poll(session, params.wait());
        Ok(view(session))
    }
}

pub struct MoreUsages;

impl Command for MoreUsages {
    type Params = NoParams;
    type Output = ShowUsagesState;
    const ID: &'static str = "navigate.showUsages.more";
    const TITLE: &'static str = "More Usages";
    const DESCRIPTION: &'static str = "Lists the next hundred rows of the Show Usages popup.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ShowUsagesState, CommandError> {
        let state = session.state_mut::<ShowUsages>();
        state.limit += PAGE;
        state.touch();
        Ok(view(session))
    }
}

pub struct SetShowUsagesOptions;

impl Command for SetShowUsagesOptions {
    type Params = ShowUsagesOptionsParams;
    type Output = ShowUsagesState;
    const ID: &'static str = "navigate.showUsages.setOptions";
    const TITLE: &'static str = "Show Usages Options";
    const DESCRIPTION: &'static str = "Changes the toggles of the Show Usages popup (mergeSameLine, read, write, \
         generated, imports, comments, preview), its scope and whether its settings panel is open; the toggles \
         left out stay.";

    fn run(&self, session: &mut Session, params: ShowUsagesOptionsParams) -> Result<ShowUsagesState, CommandError> {
        let state = session.state_mut::<ShowUsages>();
        let options = &mut state.options;
        let set = |value: &mut bool, new: Option<bool>| *value = new.unwrap_or(*value);
        set(&mut options.merge_same_line, params.merge_same_line);
        set(&mut options.read, params.read);
        set(&mut options.write, params.write);
        set(&mut options.generated, params.generated);
        set(&mut options.imports, params.imports);
        set(&mut options.comments, params.comments);
        set(&mut options.preview, params.preview);
        set(&mut state.settings, params.settings);
        if let Some(scope) = params.scope {
            state.scope = scope;
            state.limit = PAGE;
        }
        state.touch();
        Ok(view(session))
    }
}

pub struct OpenInFindWindow;

impl Command for OpenInFindWindow {
    type Params = NoParams;
    type Output = UsagesState;
    const ID: &'static str = "navigate.showUsages.openInFindWindow";
    const TITLE: &'static str = "Open in Find Tool Window";
    const DESCRIPTION: &'static str = "Closes the Show Usages popup and finds the same usages, in the popup's scope, on a \
         tab \"Usages of …\" of the Find tool window, as Find Usages (⌥F7) does.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<UsagesState, CommandError> {
        let (path, offset) = session
            .state::<ShowUsages>()
            .and_then(|state| state.origin.clone())
            .ok_or_else(|| CommandError::new(t!("workbench-show-usages-none")))?;
        let scope = session.state::<ShowUsages>().map(|state| state.scope);
        close(session);
        let params = FindUsagesParams { path: Some(path), offset: Some(offset), scope, ..FindUsagesParams::default() };
        FindUsages.run(session, params)
    }
}

pub(crate) fn open_popup(session: &Session) -> Option<(PathBuf, usize, UsagesScope)> {
    let state = session.state::<ShowUsages>().filter(|state| state.open)?;
    let (path, offset) = state.origin.clone()?;
    Some((path, offset, state.scope))
}

pub(crate) fn close(session: &mut Session) {
    session.state_mut::<Languages>().end_lookup(LookupKind::ShowUsages);
    let state = session.state_mut::<ShowUsages>();
    state.open = false;
    state.pending = false;
    state.settings = false;
    state.touch();
}

pub struct CloseShowUsages;

impl Command for CloseShowUsages {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "navigate.showUsages.close";
    const TITLE: &'static str = "Close Show Usages";
    const DESCRIPTION: &'static str = "Closes the Show Usages popup, cancelling its search if it still runs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        close(session);
        Ok(())
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ShowUsagesAt)?;
    registry.register(GetShowUsages)?;
    registry.register(MoreUsages)?;
    registry.register(SetShowUsagesOptions)?;
    registry.register(OpenInFindWindow)?;
    registry.register(CloseShowUsages)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::language::testing::{Fixture, fixture};

    const MAIN: &str = "package main\n\nvar Version = \"1\"\n\nfunc main() {\n\tVersion = \"2\"\n\tprintln(Version, Version)\n\t// Version is printed\n\tfmt.Sscan(&Version)\n}\n";
    const UTIL: &str =
        "// Code generated by hand. DO NOT EDIT.\n\npackage util\n\nimport (\n\t\"fmt\"\n)\n\nvar _ = main.Version\n";

    fn range(text: &str, needle: &str, nth: usize) -> Value {
        let at = text.match_indices(needle).nth(nth).unwrap().0;
        json!({
            "start": zen_lsp::position::position(text, at),
            "end": zen_lsp::position::position(text, at + needle.len()),
        })
    }

    fn location(fixture: &Fixture, name: &str, text: &str, nth: usize) -> Value {
        json!({ "uri": zen_lsp::position::file_uri(&fixture.path(name)), "range": range(text, "Version", nth) })
    }

    fn chars(text: &str, nth: usize) -> usize {
        text[..text.match_indices("Version").nth(nth).unwrap().0].chars().count()
    }

    fn served() -> Fixture {
        let fixture = fixture(&[("go.mod", "module app\n"), ("main.go", MAIN), ("util/util.go", UTIL)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "referencesProvider": true,
            "documentSymbolProvider": true,
            "documentHighlightProvider": true,
        }));
        fixture.server.on(
            "textDocument/documentSymbol",
            json!([{
                "name": "Version",
                "kind": 13,
                "range": range(MAIN, "var Version = \"1\"", 0),
                "selectionRange": range(MAIN, "Version", 0),
            }]),
        );
        let answer = json!([
            location(&fixture, "util/util.go", UTIL, 0),
            location(&fixture, "main.go", MAIN, 5),
            location(&fixture, "main.go", MAIN, 0),
            location(&fixture, "main.go", MAIN, 2),
            location(&fixture, "main.go", MAIN, 1),
            location(&fixture, "main.go", MAIN, 3),
        ]);
        fixture.server.on("textDocument/references", answer);
        let highlights: Vec<Value> = [(0, 3), (1, 3), (2, 2), (3, 2), (5, 3)]
            .into_iter()
            .map(|(nth, kind)| json!({ "range": range(MAIN, "Version", nth), "kind": kind }))
            .collect();
        fixture.server.on("textDocument/documentHighlight", json!(highlights));
        fixture
    }

    fn flat(state: &Value) -> Vec<Value> {
        let mut rows = Vec::new();
        for file in state["files"].as_array().unwrap() {
            for row in file["rows"].as_array().unwrap() {
                let mut row = row.clone();
                row["path"] = file["path"].clone();
                row["module"] = file.get("module").cloned().unwrap_or_default();
                rows.push(row);
            }
        }
        rows
    }

    fn lines(state: &Value) -> Vec<(String, u64)> {
        flat(state)
            .iter()
            .map(|row| (row["path"].as_str().unwrap().to_owned(), row["line"].as_u64().unwrap()))
            .collect()
    }

    fn row(name: &str, line: u64) -> (String, u64) {
        (name.to_owned(), line)
    }

    #[test]
    fn the_popup_lists_the_usages_with_kind_declaration_module_and_access() {
        let mut fixture = served();
        fixture.open("main.go");
        let at = chars(MAIN, 2) + 1;
        fixture.dispatch("editor.highlights.get", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        let state = fixture.dispatch("navigate.usages.show", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(fixture.server.requests("textDocument/references")[0]["context"]["includeDeclaration"], true);
        assert_eq!(
            (state["search"].clone(), state["open"].clone(), state["pending"].clone()),
            (json!(1), json!(true), json!(false))
        );
        assert_eq!(state["symbol"], "Version");
        assert_eq!(state["kind"], "variable", "{state}");
        assert_eq!(state["declarationRelative"], "main.go");
        assert_eq!(state["scope"], "projectFiles");
        assert_eq!(
            (state["found"].clone(), state["total"].clone(), state["more"].clone()),
            (json!(5), json!(5), json!(0))
        );
        assert_eq!(
            lines(&state),
            [row("main.go", 6), row("main.go", 7), row("main.go", 7), row("main.go", 9), row("util/util.go", 9)]
        );
        let files = state["files"].as_array().unwrap();
        assert_eq!(files.len(), 2, "each file once: {state}");
        assert!(files[0]["rows"][0].get("path").is_none(), "a row does not repeat its file");
        let rows = flat(&state);
        let folder = fixture.path("").file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(rows[0]["module"], json!(folder));
        assert_eq!(rows[4]["module"], json!(folder));
        assert_eq!(rows[0]["access"], "write");
        assert_eq!(rows[1]["access"], "read");
        assert_eq!(rows[3]["access"], "write", "the server's documentHighlight says Sscan writes it");
        assert_eq!(rows[4]["access"], "read");
        assert_eq!(rows[1]["preview"], "println(Version, Version)");
        assert_eq!(rows[1]["highlights"], json!([[8, 15]]));
        assert_eq!(rows[2]["highlights"], json!([[17, 24]]));

        let state = fixture.dispatch("navigate.showUsages.setOptions", json!({ "write": false })).unwrap();
        assert_eq!(lines(&state), [row("main.go", 7), row("main.go", 7), row("util/util.go", 9)]);
        let state = fixture
            .dispatch("navigate.showUsages.setOptions", json!({ "write": true, "mergeSameLine": true }))
            .unwrap();
        assert_eq!(lines(&state), [row("main.go", 6), row("main.go", 7), row("main.go", 9), row("util/util.go", 9)]);
        assert_eq!(state["total"], 5, "merging keeps the count of usages");
        assert_eq!(flat(&state)[1]["highlights"], json!([[8, 15], [17, 24]]));
        assert_eq!(flat(&state)[1]["count"], 2);
        let state = fixture
            .dispatch("navigate.showUsages.setOptions", json!({ "mergeSameLine": false, "generated": false }))
            .unwrap();
        assert_eq!(state["total"], 4, "util.go is generated");
        let state =
            fixture.dispatch("navigate.showUsages.setOptions", json!({ "generated": true, "comments": true })).unwrap();
        assert_eq!(state["total"], 6);
        let comment = flat(&state).into_iter().find(|row| row["comment"] == true).unwrap();
        assert_eq!((comment["line"].clone(), comment["preview"].clone()), (json!(8), json!("// Version is printed")));

        let state = fixture
            .dispatch("navigate.showUsages.setOptions", json!({ "comments": false, "scope": "currentFile" }))
            .unwrap();
        assert_eq!(state["total"], 4);
        let widen = |fixture: &mut Fixture| {
            fixture.dispatch("navigate.usages.show", json!({ "offset": at, "waitMs": 5000 })).unwrap()
        };
        let state = widen(&mut fixture);
        assert_eq!(
            (state["scope"].clone(), state["total"].clone(), state["search"].clone()),
            (json!("openFiles"), json!(4), json!(1))
        );
        let state = widen(&mut fixture);
        assert_eq!((state["scope"].clone(), state["total"].clone()), (json!("projectFiles"), json!(5)));
        let state = widen(&mut fixture);
        assert_eq!(state["scope"], "allPlaces");
        assert_eq!(widen(&mut fixture)["scope"], "allPlaces");
        assert_eq!(fixture.server.requests("textDocument/references").len(), 1, "widening asks nothing again");

        let state =
            fixture.dispatch("navigate.usages.show", json!({ "offset": chars(MAIN, 1), "waitMs": 5000 })).unwrap();
        assert_eq!((state["search"].clone(), state["scope"].clone()), (json!(2), json!("projectFiles")));
        assert_eq!(state["options"]["comments"], false, "the toggles stay for the next search");
    }

    #[test]
    fn rows_come_a_hundred_at_a_time() {
        let body: String = (0..250).map(|_| "\tx := Count\n").collect();
        let text = format!("package main\n\nfunc f() {{\n{body}}}\n");
        let mut fixture = fixture(&[("main.go", &text)]);
        let uri = zen_lsp::position::file_uri(&fixture.path("main.go"));
        let found: Vec<Value> = (0..250)
            .map(|line| {
                let range = json!({ "start": { "line": line + 3, "character": 6 }, "end": { "line": line + 3, "character": 11 } });
                json!({ "uri": uri, "range": range })
            })
            .collect();
        fixture.server.on("textDocument/references", json!(found));
        fixture.open("main.go");
        let at = text.find("Count").unwrap();
        let state = fixture.dispatch("navigate.usages.show", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(state["kind"], "symbol", "nothing says what it is");
        assert_eq!(state["declarationRelative"], "main.go");
        assert_eq!((state["total"].clone(), flat(&state).len(), state["more"].clone()), (json!(250), PAGE, json!(150)));
        let state = fixture.dispatch("navigate.showUsages.more", json!({})).unwrap();
        assert_eq!((flat(&state).len(), state["more"].clone()), (200, json!(50)));
        let state = fixture.dispatch("navigate.showUsages.more", json!({})).unwrap();
        assert_eq!((flat(&state).len(), state["more"].clone()), (250, json!(0)));
        assert_eq!(flat(&state)[249]["line"], 253);
        let state = fixture.dispatch("navigate.showUsages.setOptions", json!({ "scope": "allPlaces" })).unwrap();
        assert_eq!(state["more"], 150, "a new scope starts at the first hundred again");
    }

    #[test]
    fn settings_find_window_and_close() {
        let mut fixture = served();
        assert!(fixture.dispatch("navigate.showUsages.openInFindWindow", json!({})).is_err(), "nothing shown yet");
        fixture.open("main.go");
        let at = chars(MAIN, 1);
        fixture.dispatch("navigate.usages.show", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        let dialog = fixture.dispatch("navigate.usages.settings", json!({})).unwrap();
        assert_eq!((dialog["open"].clone(), dialog["showUsages"].clone()), (json!(true), json!(true)), "{dialog}");
        assert_eq!(dialog["targetName"], "Variable Version");
        assert_eq!(fixture.dispatch("navigate.showUsages.get", json!({})).unwrap()["open"], false);
        fixture.dispatch("navigate.usagesSettings.set", json!({ "scope": "currentFile" })).unwrap();
        fixture.dispatch("navigate.usagesSettings.find", json!({ "waitMs": 5000 })).unwrap();
        let state = fixture.dispatch("navigate.showUsages.get", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!((state["open"].clone(), state["scope"].clone()), (json!(true), json!("currentFile")));
        assert_eq!(state["total"], 4, "the usages of main.go: {state}");

        let found = fixture.dispatch("navigate.showUsages.openInFindWindow", json!({})).unwrap();
        assert_eq!(found["symbol"], "Version");
        assert_eq!(found["search"], 1, "Find Usages ran for the same place");
        let state = fixture.dispatch("navigate.showUsages.get", json!({})).unwrap();
        assert_eq!(state["open"], false);
        let listed = fixture.dispatch("navigate.usages.list", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(listed["total"], 6, "{listed}");
        let tree = fixture.dispatch("search.results.tree", json!({ "tab": listed["tab"], "depth": 0 })).unwrap();
        assert_eq!(tree["nodes"][1]["label"], "Usages in Current File", "the popup's scope: {tree}");
        assert_eq!(tree["total"], 5, "the fake server answers main.go's declaration too: {tree}");
        let asked = fixture.server.requests("textDocument/references");
        assert_eq!(asked.last().unwrap()["context"]["includeDeclaration"], false, "as Find Usages asks");

        fixture.dispatch("navigate.usages.show", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        let generation = fixture.dispatch("navigate.showUsages.get", json!({})).unwrap()["generation"].clone();
        fixture.dispatch("navigate.showUsages.close", json!({})).unwrap();
        let state = fixture.dispatch("navigate.showUsages.get", json!({})).unwrap();
        assert_eq!(state["open"], false);
        assert!(state["generation"].as_u64() > generation.as_u64());
    }

    #[test]
    fn nothing_found_is_an_error() {
        let mut fixture = served();
        fixture.server.on("textDocument/references", json!(null));
        fixture.open("main.go");
        let state =
            fixture.dispatch("navigate.usages.show", json!({ "offset": chars(MAIN, 1), "waitMs": 5000 })).unwrap();
        assert_eq!((state["open"].clone(), state["total"].clone()), (json!(true), json!(0)));
        assert_eq!(state["error"], zen_i18n::t!("workbench-nav-no-usages"));
        assert!(fixture.dispatch("navigate.usages.show", json!({ "offset": 0 })).is_ok(), "the word package");
        let blank = MAIN.find("\n\n").unwrap() + 1;
        assert!(
            fixture.dispatch("navigate.usages.show", json!({ "offset": blank })).is_err(),
            "no symbol on an empty line"
        );
    }

    #[test]
    fn writes_imports_generated_files_and_comments_are_told_from_the_text() {
        assert!(written("", " = 2"));
        assert!(written("", " := f()"));
        assert!(written("", "++"));
        assert!(written("x, ", " += 1"));
        assert!(written("++", ""));
        assert!(!written("", " == 2"));
        assert!(!written("", " => 2"));
        assert!(!written("println(", ")"));

        let go = "package main\n\nimport (\n\t\"fmt\"\n\n\t// the app\n\tapp \"example.com/app\"\n)\n\nfunc main() { app.Run() }\n";
        assert!(in_import(go, go.find("example").unwrap()));
        assert!(!in_import(go, go.find("app.Run").unwrap()));
        assert!(in_import("use crate::model::Store;\n", 12));
        assert!(in_import("import x from \"y\";\n", 7));
        assert!(in_import("import . \"fmt\"\n", 9));
        assert!(in_import("from . import name\n", 14));
        let code = "func f() {\n\tfrom := Version\n\tuse = Version\n\timporter.Import(\n\t\tVersion,\n\t)\n}\n";
        for nth in 0..3 {
            let at = code.match_indices("Version").nth(nth).unwrap().0;
            assert!(!in_import(code, at), "usage {nth} is code");
        }

        assert!(generated("// Code generated by protoc-gen-go. DO NOT EDIT.\npackage pb\n"));
        assert!(generated("// @generated\n"));
        let next_env = "/// <reference types=\"next\" />\n/// <reference types=\"next/image-types/global\" />\n\n\
                        // NOTE: This file should not be edited\n// see https://nextjs.org/docs/basic-features/typescript for more information.\n";
        assert!(generated(next_env), "next-env.d.ts is generated by Next.js");
        assert!(!generated("const note = \"This file should not be edited\";\n"));
        assert!(!generated("package main\n// Code generated is a phrase\n"));
        assert!(!generated("package gen\n\nconst header = \"// Code generated by gen. DO NOT EDIT.\"\n"));
        assert!(!generated("package gen\n\nvar tag = \"@generated\"\n"));

        let text = "é := Version // Version\n";
        assert_eq!(byte_offsets(text, &[16, 5, 99]), [17, 6, text.len()]);

        let text = "a := \"// Name\" // Name and Names\n/* Name */ b := `/* Name`\n";
        let comments = comment_ranges(text);
        assert_eq!(comments.len(), 2, "{comments:?}");
        let found: Vec<usize> = word_in_comments(text, "Name").into_iter().map(|range| range.start).collect();
        assert_eq!(found, [text.find("// Name and").unwrap() + 3, text.find("/* Name */").unwrap() + 3]);

        assert_eq!(UsagesScope::CurrentFile.wider(), UsagesScope::OpenFiles);
        assert_eq!(UsagesScope::OpenFiles.wider(), UsagesScope::ProjectFiles);
        assert_eq!(UsagesScope::ProjectFiles.wider(), UsagesScope::AllPlaces);
        assert_eq!(UsagesScope::AllPlaces.wider(), UsagesScope::AllPlaces);
    }
}

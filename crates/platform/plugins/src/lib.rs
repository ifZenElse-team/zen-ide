use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
pub use zen_lsp::{ServerDeclaration, ServerPlugin};
pub use zen_project::code_style::CodeStyleDeclaration;
pub use zen_project::generators::GeneratorDeclaration;
pub use zen_project::support::ProjectSupport;
pub use zen_syntax::PersonOnly;
use zen_syntax::{
    ColorSpan, Conversion, Expansion, Feature, HighlightKind, Highlighter, JvmClass, JvmFile, JvmMain, JvmTestClass,
    KeyReferences, LineFold, OutlineItem, RunOffer, Span, Suggestion, Suggestions, SymbolKind, TextFeatures, TextInlay,
    TextProblem,
};
pub use zen_syntax::{IndexDeclaration, IndexEdge, IndexNode, IndexProblem, IndexReference, ProjectIndex};

pub mod generator;
#[cfg(feature = "native")]
pub mod native;

pub use zen_syntax::SchemaDeclaration;
pub use zen_syntax::{ToolAction, ToolActionKind, ToolCommand, ToolRow, ToolSection, ToolWindowDeclaration};

pub const ABI_VERSIONS: &[u32] = &[0, 1];

pub const CATALOG_SCHEMA: u32 = 1;

const FUEL_PER_CALL: u64 = 2_000_000_000;

const FUEL_PER_BYTE: u64 = 10_000;

const MAX_FUEL_PER_CALL: u64 = 20_000_000_000;

fn fuel_for(len: usize) -> u64 {
    FUEL_PER_CALL.saturating_add(FUEL_PER_BYTE.saturating_mul(len as u64)).min(MAX_FUEL_PER_CALL)
}

const MAX_RESULT: usize = 64 << 20;

const MAX_MEMORY: usize = 1 << 30;

pub const MAX_JSON: usize = 32 << 20;

pub const MAX_FOLDS: usize = 100_000;

pub const MAX_OUTLINE: usize = 20_000;

pub const MAX_SUGGESTIONS: usize = 2_000;

pub const MAX_NAME: usize = 256;

pub const MAX_COLORS: usize = 100_000;

pub const MAX_TRIGGERS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginError(pub String);

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PluginError {}

fn error(message: impl Into<String>) -> PluginError {
    PluginError(message.into())
}

pub fn kind_from_abi(kind: u32) -> Option<HighlightKind> {
    Some(match kind {
        0 => HighlightKind::Keyword,
        1 => HighlightKind::String,
        2 => HighlightKind::Number,
        3 => HighlightKind::Comment,
        4 => HighlightKind::DocComment,
        5 => HighlightKind::Macro,
        6 => HighlightKind::Type,
        7 => HighlightKind::Function,
        8 => HighlightKind::Lifetime,
        9 => HighlightKind::Attribute,
        10 => HighlightKind::Constant,
        11 => HighlightKind::Punctuation,
        12 => HighlightKind::Heading,
        13 => HighlightKind::Strong,
        14 => HighlightKind::Emphasis,
        15 => HighlightKind::Link,
        16 => HighlightKind::Code,
        17 => HighlightKind::Property,
        18 => HighlightKind::GlobalVariable,
        _ => return None,
    })
}

struct Runtime {
    store: wasmi::Store<wasmi::StoreLimits>,
    memory: wasmi::Memory,
    alloc: wasmi::TypedFunc<i32, i32>,
    highlight: wasmi::TypedFunc<(i32, i32), i64>,
    fold: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    outline: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    complete: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    expand: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    colors: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    on_type: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    triggers: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    problems: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    dialect: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    jvm: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    project: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    inlays: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    key_references: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    resource: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    run_targets: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    tool_rows: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    index: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    convert: Option<wasmi::TypedFunc<(i32, i32), i64>>,
    convert_from: Option<wasmi::TypedFunc<(i32, i32), i64>>,
}

impl Runtime {
    fn export(&self, export: Export) -> Option<wasmi::TypedFunc<(i32, i32), i64>> {
        match export {
            Export::Highlight => Some(self.highlight),
            Export::Fold => self.fold,
            Export::Outline => self.outline,
            Export::Complete => self.complete,
            Export::Expand => self.expand,
            Export::Colors => self.colors,
            Export::OnType => self.on_type,
            Export::Triggers => self.triggers,
            Export::Problems => self.problems,
            Export::Dialect => self.dialect,
            Export::Jvm => self.jvm,
            Export::Project => self.project,
            Export::Inlays => self.inlays,
            Export::KeyReferences => self.key_references,
            Export::Resource => self.resource,
            Export::RunTargets => self.run_targets,
            Export::ToolRows => self.tool_rows,
            Export::Index => self.index,
            Export::Convert => self.convert,
            Export::ConvertFrom => self.convert_from,
        }
    }

    fn call(&mut self, export: Export, input: &[u8], limit: usize) -> Result<Vec<u8>, String> {
        let function = self.export(export).ok_or_else(|| format!("{} is not exported", export.name()))?;
        let length = i32::try_from(input.len()).map_err(|_| "the file is too large".to_owned())?;
        self.store.set_fuel(fuel_for(input.len())).map_err(|e| e.to_string())?;
        let pointer = self.alloc.call(&mut self.store, length).map_err(|e| e.to_string())?;
        let start = usize::try_from(pointer).map_err(|_| "zen_alloc returned a negative pointer".to_owned())?;
        self.memory.write(&mut self.store, start, input).map_err(|e| e.to_string())?;
        let packed = function.call(&mut self.store, (pointer, length)).map_err(|e| e.to_string())?;
        let (result, size) = ((packed as u64 >> 32) as usize, (packed as u64 & 0xFFFF_FFFF) as usize);
        if size > limit {
            return Err(format!("{} returned {size} bytes, more than {limit}", export.name()));
        }
        let mut bytes = vec![0u8; size];
        self.memory.read(&self.store, result, &mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Export {
    Highlight,
    Fold,
    Outline,
    Complete,
    Expand,
    Colors,
    OnType,
    Triggers,
    Problems,
    Dialect,
    Jvm,
    Project,
    Inlays,
    KeyReferences,
    Resource,
    RunTargets,
    ToolRows,
    Index,
    Convert,
    ConvertFrom,
}

impl Export {
    fn name(self) -> &'static str {
        match self {
            Export::Highlight => "zen_highlight",
            Export::Fold => "zen_fold",
            Export::Outline => "zen_outline",
            Export::Complete => "zen_complete",
            Export::Expand => "zen_expand",
            Export::Colors => "zen_colors",
            Export::OnType => "zen_on_type",
            Export::Triggers => "zen_on_type_triggers",
            Export::Problems => "zen_problems",
            Export::Dialect => "zen_dialect",
            Export::Jvm => "zen_jvm",
            Export::Project => "zen_project",
            Export::Inlays => "zen_inlays",
            Export::KeyReferences => "zen_key_references",
            Export::Resource => "zen_resource",
            Export::RunTargets => "zen_run_targets",
            Export::ToolRows => "zen_tool_rows",
            Export::Index => "zen_index",
            Export::Convert => "zen_convert",
            Export::ConvertFrom => "zen_convert_from",
        }
    }
}

pub struct WasmHighlighter {
    id: String,
    abi: u32,
    extensions: Vec<String>,
    names: Vec<String>,
    features: Vec<Feature>,
    triggers: Vec<char>,
    key_references: Vec<KeyReferences>,
    converts_from: Vec<String>,
    dialect: String,
    schemas: Vec<SchemaDeclaration>,
    runs: Vec<String>,
    person_only: Vec<PersonOnly>,
    target_files: Vec<String>,
    tool_windows: Vec<ToolWindowDeclaration>,
    index_declaration: Option<IndexDeclaration>,
    runtime: Arc<Mutex<Option<Runtime>>>,
}

impl std::fmt::Debug for WasmHighlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmHighlighter")
            .field("id", &self.id)
            .field("abi", &self.abi)
            .field("extensions", &self.extensions)
            .field("features", &self.features)
            .field("triggers", &self.triggers)
            .field("dialect", &self.dialect)
            .finish()
    }
}

impl WasmHighlighter {
    pub fn load(id: &str, wasm: &[u8], extensions: &[String]) -> Result<Self, PluginError> {
        let mut config = wasmi::Config::default();
        config.consume_fuel(true);
        let engine = wasmi::Engine::new(&config);
        let module =
            wasmi::Module::new(&engine, wasm).map_err(|e| error(format!("{id}: not a valid plug-in module: {e}")))?;
        if let Some(import) = module.imports().next() {
            return Err(error(format!(
                "{id}: plug-ins may not import anything, but this one imports {}::{}",
                import.module(),
                import.name()
            )));
        }
        let limits = wasmi::StoreLimitsBuilder::new().memory_size(MAX_MEMORY).build();
        let mut store = wasmi::Store::new(&engine, limits);
        store.limiter(|limits| limits);
        store.set_fuel(FUEL_PER_CALL).map_err(|e| error(e.to_string()))?;
        let linker = wasmi::Linker::<wasmi::StoreLimits>::new(&engine);
        let instance =
            linker.instantiate_and_start(&mut store, &module).map_err(|e| error(format!("{id}: cannot start: {e}")))?;
        let missing =
            |what: &str| error(format!("{id}: the module does not export `{what}` (ABI v0, docs/plugins.md)"));
        let memory = instance.get_memory(&store, "memory").ok_or_else(|| missing("memory"))?;
        let abi = instance.get_typed_func::<(), i32>(&store, "zen_abi").map_err(|_| missing("zen_abi"))?;
        let alloc = instance.get_typed_func::<i32, i32>(&store, "zen_alloc").map_err(|_| missing("zen_alloc"))?;
        let highlight = instance
            .get_typed_func::<(i32, i32), i64>(&store, "zen_highlight")
            .map_err(|_| missing("zen_highlight"))?;
        let version = abi.call(&mut store, ()).map_err(|e| error(format!("{id}: zen_abi failed: {e}")))?;
        let Some(version) = u32::try_from(version).ok().filter(|version| ABI_VERSIONS.contains(version)) else {
            return Err(error(format!("{id}: plug-in ABI version {version} is not supported by this IDE")));
        };
        let optional = |export: Export| {
            if version < 1 {
                return Ok(None);
            }
            match instance.get_export(&store, export.name()) {
                None => Ok(None),
                Some(_) => instance.get_typed_func::<(i32, i32), i64>(&store, export.name()).map(Some).map_err(|_| {
                    error(format!("{id}: `{}` must take (i32, i32) and return i64 (ABI v1)", export.name()))
                }),
            }
        };
        let fold = optional(Export::Fold)?;
        let outline = optional(Export::Outline)?;
        let complete = optional(Export::Complete)?;
        let expand = optional(Export::Expand)?;
        let colors = optional(Export::Colors)?;
        let on_type = optional(Export::OnType)?;
        let triggers = optional(Export::Triggers)?;
        let problems = optional(Export::Problems)?;
        let dialect = optional(Export::Dialect)?;
        let jvm = optional(Export::Jvm)?;
        let project = optional(Export::Project)?;
        let inlays = optional(Export::Inlays)?;
        let key_references = optional(Export::KeyReferences)?;
        let resource = optional(Export::Resource)?;
        let run_targets = optional(Export::RunTargets)?;
        let tool_rows = optional(Export::ToolRows)?;
        let index = optional(Export::Index)?;
        let convert = optional(Export::Convert)?;
        let convert_from = optional(Export::ConvertFrom)?;
        let mut runtime = Runtime {
            store,
            memory,
            alloc,
            highlight,
            fold,
            outline,
            complete,
            expand,
            colors,
            on_type,
            triggers,
            problems,
            dialect,
            jvm,
            project,
            inlays,
            key_references,
            resource,
            run_targets,
            tool_rows,
            index,
            convert,
            convert_from,
        };
        let triggers = match (on_type, triggers) {
            (Some(_), Some(_)) => runtime
                .call(Export::Triggers, &[], MAX_JSON)
                .map_err(|e| error(format!("{id}: zen_on_type_triggers failed: {e}")))
                .and_then(|bytes| decode_triggers(&bytes).map_err(|e| error(format!("{id}: {e}"))))?,
            _ => Vec::new(),
        };
        let key_references = match key_references {
            Some(_) => runtime
                .call(Export::KeyReferences, &[], MAX_JSON)
                .map_err(|e| error(format!("{id}: zen_key_references failed: {e}")))
                .and_then(|bytes| decode_key_references(&bytes).map_err(|e| error(format!("{id}: {e}"))))?,
            None => Vec::new(),
        };
        let converts_from = match (convert, convert_from) {
            (Some(_), Some(_)) => runtime
                .call(Export::ConvertFrom, &[], MAX_JSON)
                .map_err(|e| error(format!("{id}: zen_convert_from failed: {e}")))
                .map(|bytes| decode_converts_from(&bytes))?,
            _ => Vec::new(),
        };
        let features = [
            (fold.is_some(), Feature::Folding),
            (outline.is_some(), Feature::Outline),
            (complete.is_some(), Feature::Completion),
            (expand.is_some(), Feature::Expansion),
            (colors.is_some(), Feature::Colors),
            (!triggers.is_empty(), Feature::Typing),
            (problems.is_some(), Feature::Problems),
            (jvm.is_some(), Feature::Jvm),
            (inlays.is_some(), Feature::Inlays),
            (resource.is_some(), Feature::Resources),
            (run_targets.is_some(), Feature::RunTargets),
            (!converts_from.is_empty(), Feature::Conversion),
        ]
        .into_iter()
        .filter_map(|(present, feature)| present.then_some(feature))
        .collect();
        Ok(Self {
            id: id.to_owned(),
            abi: version,
            extensions: extensions.iter().map(|extension| extension.to_ascii_lowercase()).collect(),
            names: Vec::new(),
            features,
            triggers,
            key_references,
            converts_from,
            dialect: String::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index_declaration: None,
            runtime: Arc::new(Mutex::new(Some(runtime))),
        })
    }

    pub fn with_names(mut self, names: &[String]) -> Self {
        self.names = names.to_vec();
        self
    }

    pub fn with_schemas(mut self, schemas: &[SchemaDeclaration]) -> Self {
        self.schemas = schemas.to_vec();
        self
    }

    pub fn with_runs(mut self, runs: &[String]) -> Self {
        self.runs = runs.to_vec();
        self
    }

    pub fn with_person_only(mut self, person_only: &[PersonOnly]) -> Self {
        self.person_only = person_only.to_vec();
        self
    }

    pub fn with_target_files(mut self, patterns: &[String]) -> Self {
        self.target_files = patterns.to_vec();
        self
    }

    pub fn with_index(mut self, index: Option<&IndexDeclaration>) -> Self {
        self.index_declaration = index.cloned();
        self
    }

    pub fn with_tool_windows(mut self, windows: &[ToolWindowDeclaration]) -> Self {
        self.tool_windows = windows.to_vec();
        self
    }

    pub fn has_project(&self) -> bool {
        self.runtime.lock().ok().and_then(|runtime| runtime.as_ref().map(|runtime| runtime.project.is_some()))
            == Some(true)
    }

    pub fn project(&self, input: &str) -> Option<Vec<u8>> {
        self.run(Export::Project, input.as_bytes(), MAX_JSON, |bytes| Ok(bytes.to_vec()))
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn abi(&self) -> u32 {
        self.abi
    }

    pub fn is_disabled(&self) -> bool {
        self.runtime.lock().map_or(true, |runtime| runtime.is_none())
    }

    fn run<T>(
        &self,
        export: Export,
        input: &[u8],
        limit: usize,
        decode: impl FnOnce(&[u8]) -> Result<T, String>,
    ) -> Option<T> {
        let mut guard = self.runtime.lock().ok()?;
        let runtime = guard.as_mut()?;
        runtime.export(export)?;
        let told = match runtime.dialect {
            Some(_) => runtime.call(Export::Dialect, self.dialect.as_bytes(), MAX_JSON).map(drop),
            None => Ok(()),
        };
        match told.and_then(|()| runtime.call(export, input, limit)).and_then(|bytes| decode(&bytes)) {
            Ok(value) => Some(value),
            Err(_) => {
                *guard = None;
                None
            }
        }
    }
}

pub(crate) fn with_typed(text: &str, caret: usize, typed: char) -> Option<Vec<u8>> {
    let caret = u32::try_from(caret).ok()?;
    let mut input = Vec::with_capacity(text.len() + 8);
    input.extend_from_slice(&caret.to_le_bytes());
    input.extend_from_slice(&u32::from(typed).to_le_bytes());
    input.extend_from_slice(text.as_bytes());
    Some(input)
}

pub(crate) fn with_caret(text: &str, caret: usize) -> Option<Vec<u8>> {
    let caret = u32::try_from(caret).ok()?;
    let mut input = Vec::with_capacity(text.len() + 4);
    input.extend_from_slice(&caret.to_le_bytes());
    input.extend_from_slice(text.as_bytes());
    Some(input)
}

fn decode(bytes: &[u8], text: &str) -> Vec<Span> {
    let mut spans = Vec::with_capacity(bytes.len() / 12);
    let mut previous_end = 0;
    for record in bytes.as_chunks::<12>().0 {
        let field = |index: usize| {
            let mut value = [0u8; 4];
            value.copy_from_slice(&record[index * 4..index * 4 + 4]);
            u32::from_le_bytes(value)
        };
        let (start, end) = (field(0) as usize, field(1) as usize);
        let Some(kind) = kind_from_abi(field(2)) else { continue };
        if start < previous_end
            || end <= start
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            continue;
        }
        previous_end = end;
        spans.push(Span { range: start..end, kind });
    }
    spans
}

pub fn decode_folds(bytes: &[u8], lines: usize) -> Result<Vec<LineFold>, String> {
    if !bytes.len().is_multiple_of(8) {
        return Err(format!("zen_fold returned {} bytes, not a list of 8-byte ranges", bytes.len()));
    }
    let mut folds: Vec<LineFold> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .take(MAX_FOLDS)
        .map(|record| {
            let field = |index: usize| {
                let mut value = [0u8; 4];
                value.copy_from_slice(&record[index * 4..index * 4 + 4]);
                u32::from_le_bytes(value) as usize
            };
            LineFold { start_line: field(0), end_line: field(1) }
        })
        .filter(|fold| fold.start_line < fold.end_line && fold.end_line < lines)
        .collect();
    folds.sort_by_key(|fold| (fold.start_line, std::cmp::Reverse(fold.end_line)));
    folds.dedup();
    Ok(folds)
}

pub fn decode_colors(bytes: &[u8], text: &str) -> Result<Vec<ColorSpan>, String> {
    if !bytes.len().is_multiple_of(12) {
        return Err(format!("zen_colors returned {} bytes, not a list of 12-byte records", bytes.len()));
    }
    let mut colors = Vec::new();
    let mut previous_end = 0;
    for record in bytes.as_chunks::<12>().0 {
        if colors.len() == MAX_COLORS {
            break;
        }
        let field = |index: usize| {
            let mut value = [0u8; 4];
            value.copy_from_slice(&record[index * 4..index * 4 + 4]);
            u32::from_le_bytes(value)
        };
        let (start, end) = (field(0) as usize, field(1) as usize);
        if start < previous_end
            || end <= start
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            continue;
        }
        previous_end = end;
        colors.push(ColorSpan { range: start..end, rgba: field(2).to_be_bytes() });
    }
    Ok(colors)
}

pub fn decode_triggers(bytes: &[u8]) -> Result<Vec<char>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("zen_on_type_triggers: {e}"))?;
    let mut triggers: Vec<char> = Vec::new();
    for typed in text.chars().filter(|typed| !typed.is_control()) {
        if !triggers.contains(&typed) && triggers.len() < MAX_TRIGGERS {
            triggers.push(typed);
        }
    }
    Ok(triggers)
}

#[derive(Deserialize)]
struct RawOutline {
    name: String,
    #[serde(default)]
    kind: String,
    start: u64,
    end: u64,
    #[serde(default)]
    children: Vec<RawOutline>,
}

pub fn symbol_kind_from_abi(kind: &str) -> SymbolKind {
    match kind {
        "element" => SymbolKind::Element,
        "rule" => SymbolKind::Rule,
        "property" => SymbolKind::Property,
        "function" => SymbolKind::Function,
        "class" => SymbolKind::Class,
        "method" => SymbolKind::Method,
        "variable" => SymbolKind::Variable,
        "constant" => SymbolKind::Constant,
        "interface" => SymbolKind::Interface,
        "type" => SymbolKind::Type,
        "module" => SymbolKind::Module,
        "field" => SymbolKind::Field,
        _ => SymbolKind::Key,
    }
}

const COMPLETION_KINDS: &[&str] = &[
    "element",
    "rule",
    "property",
    "function",
    "class",
    "method",
    "variable",
    "constant",
    "interface",
    "type",
    "module",
    "field",
    "key",
    "keyword",
    "tag",
    "attribute",
    "value",
    "snippet",
];

const CHAR_STEP: usize = 64;

struct CharOffsets<'a> {
    text: &'a str,
    marks: Vec<usize>,
}

impl<'a> CharOffsets<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, marks: text.char_indices().step_by(CHAR_STEP).map(|(at, _)| at).collect() }
    }

    fn of(&self, byte: usize) -> usize {
        let mark = self.marks.partition_point(|start| *start <= byte).saturating_sub(1);
        let from = self.marks.get(mark).copied().unwrap_or(0);
        mark * CHAR_STEP + self.text.get(from..byte).map_or(0, |part| part.chars().count())
    }
}

pub fn decode_outline(bytes: &[u8], text: &str) -> Result<Vec<OutlineItem>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawOutline> = serde_json::from_slice(bytes).map_err(|e| format!("zen_outline: {e}"))?;
    let offsets = CharOffsets::new(text);
    let mut budget = MAX_OUTLINE;
    Ok(outline_items(raw, text, &offsets, 0..text.len(), &mut budget))
}

fn outline_items(
    raw: Vec<RawOutline>,
    text: &str,
    offsets: &CharOffsets<'_>,
    within: std::ops::Range<usize>,
    budget: &mut usize,
) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for entry in raw {
        if *budget == 0 {
            break;
        }
        let (Ok(start), Ok(end)) = (usize::try_from(entry.start), usize::try_from(entry.end)) else { continue };
        if start > end
            || start < within.start
            || end > within.end
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
            || entry.name.trim().is_empty()
        {
            continue;
        }
        *budget -= 1;
        let children = outline_items(entry.children, text, offsets, start..end, budget);
        items.push(OutlineItem {
            name: entry.name.chars().take(MAX_NAME).collect(),
            detail: None,
            kind: symbol_kind_from_abi(&entry.kind),
            start: offsets.of(start),
            end: offsets.of(end),
            selection_start: offsets.of(start),
            children,
        });
    }
    items
}

fn caret_mark(text: &str) -> (String, Option<usize>) {
    match text.find("$0") {
        Some(at) => (format!("{}{}", &text[..at], &text[at + 2..]), Some(at)),
        None => (text.to_owned(), None),
    }
}

#[derive(Deserialize)]
struct RawSuggestion {
    label: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    insert: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

#[derive(Deserialize)]
struct RawSuggestions {
    from: u64,
    #[serde(default)]
    items: Vec<RawSuggestion>,
}

pub fn decode_suggestions(bytes: &[u8], text: &str, caret: usize) -> Result<Option<Suggestions>, String> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let raw: RawSuggestions = serde_json::from_slice(bytes).map_err(|e| format!("zen_complete: {e}"))?;
    let Some(from) = usize::try_from(raw.from).ok().filter(|from| *from <= caret && text.is_char_boundary(*from))
    else {
        return Ok(None);
    };
    let items: Vec<Suggestion> = raw
        .items
        .into_iter()
        .filter(|item| !item.label.trim().is_empty())
        .take(MAX_SUGGESTIONS)
        .map(|item| {
            let (insert, caret) = caret_mark(item.insert.as_deref().unwrap_or(&item.label));
            let kind = if COMPLETION_KINDS.contains(&item.kind.as_str()) { item.kind } else { "key".to_owned() };
            Suggestion {
                label: item.label.chars().take(MAX_NAME).collect(),
                kind,
                insert,
                caret,
                detail: item
                    .detail
                    .filter(|detail| !detail.is_empty())
                    .map(|detail| detail.chars().take(MAX_NAME).collect()),
            }
        })
        .collect();
    Ok((!items.is_empty()).then_some(Suggestions { from, items }))
}

#[derive(Deserialize)]
struct RawExpansion {
    from: u64,
    to: u64,
    text: String,
}

pub fn decode_expansion(bytes: &[u8], text: &str, caret: usize) -> Result<Option<Expansion>, String> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let raw: RawExpansion = serde_json::from_slice(bytes).map_err(|e| format!("zen_expand: {e}"))?;
    let (Ok(from), Ok(to)) = (usize::try_from(raw.from), usize::try_from(raw.to)) else { return Ok(None) };
    if from > to || to > text.len() || to != caret || !text.is_char_boundary(from) || !text.is_char_boundary(to) {
        return Ok(None);
    }
    let (expanded, caret) = caret_mark(&raw.text);
    Ok(Some(Expansion { range: from..to, text: expanded, caret }))
}

pub const MAX_PROBLEMS: usize = 1000;

pub const MAX_PROBLEM_MESSAGE: usize = 512;

pub const MAX_INLAYS: usize = 10_000;

pub const MAX_INLAY_TEXT: usize = 120;

pub const MAX_KEY_REFERENCES: usize = 32;

#[derive(Deserialize)]
struct RawProblem {
    start: u32,
    end: u32,
    #[serde(default)]
    severity: String,
    message: String,
}

pub const MAX_CONVERTED: usize = 32 << 20;

pub fn decode_converts_from(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric()))
        .take(16)
        .collect()
}

pub fn convert_input(from: &str, text: &str) -> Option<Vec<u8>> {
    let length = u32::try_from(from.len()).ok()?;
    let mut input = length.to_le_bytes().to_vec();
    input.extend_from_slice(from.as_bytes());
    input.extend_from_slice(text.as_bytes());
    Some(input)
}

#[derive(Deserialize)]
struct RawConversion {
    text: String,
    #[serde(default)]
    problems: Vec<RawProblem>,
}

pub fn decode_conversion(bytes: &[u8], source: &str) -> Result<Option<Conversion>, String> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let raw: RawConversion =
        serde_json::from_slice(bytes).map_err(|e| format!("zen_convert returned invalid JSON: {e}"))?;
    let problems = raw
        .problems
        .into_iter()
        .filter_map(|problem| {
            let (start, end) = (problem.start as usize, problem.end as usize);
            let fits =
                start <= end && end <= source.len() && source.is_char_boundary(start) && source.is_char_boundary(end);
            let message: String =
                problem.message.lines().next().unwrap_or_default().chars().take(MAX_PROBLEM_MESSAGE).collect();
            (fits && !message.trim().is_empty()).then_some(TextProblem { range: start..end, error: false, message })
        })
        .take(MAX_PROBLEMS)
        .collect();
    Ok(Some(Conversion { text: raw.text, problems }))
}

pub fn decode_problems(bytes: &[u8], text: &str) -> Result<Vec<TextProblem>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawProblem> =
        serde_json::from_slice(bytes).map_err(|e| format!("zen_problems returned invalid JSON: {e}"))?;
    let mut problems: Vec<TextProblem> = raw
        .into_iter()
        .filter_map(|problem| {
            let (start, end) = (problem.start as usize, problem.end as usize);
            let fits = start <= end && end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end);
            let message: String =
                problem.message.lines().next().unwrap_or_default().chars().take(MAX_PROBLEM_MESSAGE).collect();
            (fits && !message.trim().is_empty()).then(|| TextProblem {
                range: start..end,
                error: problem.severity != "warning",
                message,
            })
        })
        .take(MAX_PROBLEMS)
        .collect();
    problems.sort_by_key(|problem| problem.range.start);
    Ok(problems)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJvm {
    #[serde(default)]
    package: String,
    #[serde(default)]
    file_class: String,
    #[serde(default)]
    classes: Vec<RawJvmClass>,
    #[serde(default)]
    mains: Vec<RawJvmMain>,
    #[serde(default)]
    tests: Vec<RawJvmTest>,
}

#[derive(Deserialize)]
struct RawJvmClass {
    name: String,
    start: u64,
    end: u64,
}

#[derive(Deserialize)]
struct RawJvmMain {
    class: String,
    #[serde(default)]
    owner: Option<u64>,
    start: u64,
    end: u64,
}

#[derive(Deserialize)]
struct RawJvmTest {
    name: String,
    start: u64,
    #[serde(default)]
    testng: bool,
    #[serde(default)]
    methods: Vec<RawJvmMethod>,
}

#[derive(Deserialize)]
struct RawJvmMethod {
    name: String,
    start: u64,
}

pub fn decode_jvm(bytes: &[u8], text: &str) -> Result<Option<JvmFile>, String> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let raw: RawJvm = serde_json::from_slice(bytes).map_err(|e| format!("zen_jvm: {e}"))?;
    let starts: Vec<usize> = text.match_indices('\n').map(|(at, _)| at).collect();
    let line = |offset: u64| {
        let offset = usize::try_from(offset).unwrap_or(usize::MAX).min(text.len());
        u32::try_from(starts.partition_point(|newline| *newline < offset)).unwrap_or(u32::MAX)
    };
    let name = |name: String| -> Option<String> {
        let name: String = name.chars().take(MAX_NAME).collect();
        (!name.trim().is_empty()).then_some(name)
    };
    Ok(Some(JvmFile {
        package: raw.package.chars().take(MAX_NAME).collect(),
        file_class: raw.file_class.chars().take(MAX_NAME).collect(),
        classes: raw
            .classes
            .into_iter()
            .take(MAX_OUTLINE)
            .filter_map(|class| {
                Some(JvmClass { name: name(class.name)?, first_line: line(class.start), last_line: line(class.end) })
            })
            .collect(),
        mains: raw
            .mains
            .into_iter()
            .take(MAX_OUTLINE)
            .map(|main| {
                let at = line(main.start);
                JvmMain {
                    class: main.class.chars().take(MAX_NAME).collect(),
                    class_line: main.owner.map_or(at, line),
                    line: at,
                    end_line: line(main.end),
                }
            })
            .collect(),
        tests: raw
            .tests
            .into_iter()
            .take(MAX_OUTLINE)
            .filter_map(|test| {
                Some(JvmTestClass {
                    name: name(test.name)?,
                    line: line(test.start),
                    testng: test.testng,
                    methods: test
                        .methods
                        .into_iter()
                        .take(MAX_OUTLINE)
                        .filter_map(|method| Some((name(method.name)?, line(method.start))))
                        .collect(),
                })
            })
            .collect(),
    }))
}

#[derive(Deserialize)]
struct RawInlay {
    offset: u64,
    text: String,
}

pub fn decode_key_references(bytes: &[u8]) -> Result<Vec<KeyReferences>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let declared: Vec<KeyReferences> =
        serde_json::from_slice(bytes).map_err(|e| format!("zen_key_references returned invalid JSON: {e}"))?;
    Ok(declared
        .into_iter()
        .filter(|declared| !declared.files.is_empty() && !(declared.keys.is_empty() && declared.bundles.is_empty()))
        .take(MAX_KEY_REFERENCES)
        .collect())
}

pub fn decode_inlays(bytes: &[u8], text: &str) -> Result<Vec<TextInlay>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawInlay> =
        serde_json::from_slice(bytes).map_err(|e| format!("zen_inlays returned invalid JSON: {e}"))?;
    let mut inlays: Vec<TextInlay> = raw
        .into_iter()
        .filter_map(|inlay| {
            let offset = usize::try_from(inlay.offset).ok()?;
            let label: String = inlay.text.lines().next().unwrap_or_default().chars().take(MAX_INLAY_TEXT).collect();
            (offset <= text.len() && text.is_char_boundary(offset) && !label.trim().is_empty())
                .then_some(TextInlay { offset, text: label })
        })
        .take(MAX_INLAYS)
        .collect();
    inlays.sort_by_key(|inlay| inlay.offset);
    Ok(inlays)
}

pub fn resource_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        && !path.contains(['\\', '\0'])
}

pub const MAX_RUN_TARGETS: usize = 200;

pub const MAX_RUN_ARGS: usize = 32 * 1024;

#[derive(Deserialize)]
struct RawRunTarget {
    line: u32,
    name: String,
    #[serde(default)]
    id: String,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    output: String,
    #[serde(default, rename = "documentType")]
    document_type: String,
}

pub const MAX_DOCUMENT_TYPE: usize = 16;

fn document_type(asked: &str) -> String {
    if asked.is_empty() || asked.len() > MAX_DOCUMENT_TYPE || !asked.chars().all(|c| c.is_ascii_alphanumeric()) {
        return "txt".to_owned();
    }
    asked.to_ascii_lowercase()
}

pub const MAX_TOOL_ROWS: usize = 5000;

pub const MAX_TOOL_COLUMNS: usize = 32;

pub const MAX_TOOL_TEXT: usize = 1000;

pub fn decode_index(bytes: &[u8]) -> Result<ProjectIndex, String> {
    if bytes.is_empty() {
        return Ok(ProjectIndex::default());
    }
    serde_json::from_slice(bytes).map_err(|e| format!("zen_index returned invalid JSON: {e}"))
}

pub fn decode_tool_rows(bytes: &[u8]) -> Result<Vec<ToolRow>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<ToolRow> =
        serde_json::from_slice(bytes).map_err(|e| format!("zen_tool_rows returned invalid JSON: {e}"))?;
    let cut =
        |text: String| -> String { text.lines().next().unwrap_or_default().chars().take(MAX_TOOL_TEXT).collect() };
    Ok(rows
        .into_iter()
        .filter(|row| !row.id.trim().is_empty())
        .take(MAX_TOOL_ROWS)
        .map(|row| ToolRow {
            id: cut(row.id),
            columns: row.columns.into_iter().take(MAX_TOOL_COLUMNS).map(cut).collect(),
            state: row.state.map(cut),
        })
        .collect())
}

pub fn with_path(path: &str, text: &str) -> Option<Vec<u8>> {
    if path.contains('\0') {
        return None;
    }
    let mut input = Vec::with_capacity(path.len() + 1 + text.len());
    input.extend_from_slice(path.as_bytes());
    input.push(0);
    input.extend_from_slice(text.as_bytes());
    Some(input)
}

pub fn decode_run_targets(bytes: &[u8], text: &str, runs: &[String]) -> Result<Vec<RunOffer>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawRunTarget> =
        serde_json::from_slice(bytes).map_err(|e| format!("zen_run_targets returned invalid JSON: {e}"))?;
    let lines = u32::try_from(text.split('\n').count()).unwrap_or(u32::MAX);
    Ok(raw
        .into_iter()
        .filter(|target| target.line < lines && !target.name.trim().is_empty())
        .filter(|target| !target.program.contains(['/', '\\']) && runs.contains(&target.program))
        .filter(|target| target.args.iter().map(String::len).sum::<usize>() <= MAX_RUN_ARGS)
        .take(MAX_RUN_TARGETS)
        .map(|target| RunOffer {
            line: target.line,
            id: if target.id.is_empty() { target.name.clone() } else { target.id },
            name: target.name.lines().next().unwrap_or_default().chars().take(MAX_INLAY_TEXT).collect(),
            program: target.program,
            args: target.args,
            cwd: target.cwd,
            env: target.env.into_iter().filter(|(name, _)| zen_syntax::kept_variable(name)).collect(),
            test: target.kind == "test",
            diff: target.output == "diff",
            document: (target.output == "document").then(|| document_type(&target.document_type)),
        })
        .collect())
}

pub fn supports(extensions: &[String], names: &[String], file_name: &str) -> bool {
    let by_extension = file_name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extensions.iter().any(|known| known.eq_ignore_ascii_case(extension)));
    by_extension || names.iter().any(|pattern| glob(&pattern.to_ascii_lowercase(), &file_name.to_ascii_lowercase()))
}

pub fn matches_name(names: &[String], file_name: &str) -> bool {
    names.iter().any(|pattern| glob(&pattern.to_ascii_lowercase(), &file_name.to_ascii_lowercase()))
}

fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, rest)) => {
            let Some(name) = name.strip_prefix(head) else { return false };
            if rest.is_empty() {
                return true;
            }
            name.char_indices().map(|(at, _)| at).chain(std::iter::once(name.len())).any(|at| glob(rest, &name[at..]))
        }
    }
}

impl Highlighter for WasmHighlighter {
    fn handles(&self, file_name: &str) -> bool {
        supports(&self.extensions, &self.names, file_name)
    }

    fn specificity(&self, file_name: &str) -> u8 {
        if matches_name(&self.names, file_name) { 2 } else { 1 }
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        self.run(Export::Highlight, text.as_bytes(), MAX_RESULT, |bytes| {
            if !bytes.len().is_multiple_of(12) {
                return Err(format!("zen_highlight returned {} bytes, not a list of 12-byte spans", bytes.len()));
            }
            Ok(decode(bytes, text))
        })
        .unwrap_or_default()
    }
}

impl zen_syntax::Outliner for WasmHighlighter {
    fn handles(&self, file_name: &str) -> bool {
        supports(&self.extensions, &self.names, file_name)
    }

    fn outline(&self, text: &str) -> Vec<OutlineItem> {
        self.run(Export::Outline, text.as_bytes(), MAX_JSON, |bytes| decode_outline(bytes, text)).unwrap_or_default()
    }
}

impl TextFeatures for WasmHighlighter {
    fn specificity(&self, file_name: &str) -> u8 {
        if matches_name(&self.names, file_name) { 2 } else { 1 }
    }

    fn supports(&self, feature: Feature) -> bool {
        self.features.contains(&feature)
    }

    fn folds(&self, text: &str) -> Vec<LineFold> {
        let lines = text.split('\n').count();
        self.run(Export::Fold, text.as_bytes(), MAX_RESULT, |bytes| decode_folds(bytes, lines)).unwrap_or_default()
    }

    fn complete(&self, text: &str, caret: usize) -> Option<Suggestions> {
        if caret > text.len() || !text.is_char_boundary(caret) {
            return None;
        }
        let input = with_caret(text, caret)?;
        self.run(Export::Complete, &input, MAX_JSON, |bytes| decode_suggestions(bytes, text, caret)).flatten()
    }

    fn expand(&self, text: &str, caret: usize) -> Option<Expansion> {
        if caret > text.len() || !text.is_char_boundary(caret) {
            return None;
        }
        let input = with_caret(text, caret)?;
        self.run(Export::Expand, &input, MAX_JSON, |bytes| decode_expansion(bytes, text, caret)).flatten()
    }

    fn problems(&self, text: &str) -> Vec<TextProblem> {
        self.run(Export::Problems, text.as_bytes(), MAX_JSON, |bytes| decode_problems(bytes, text)).unwrap_or_default()
    }

    fn colors(&self, text: &str) -> Vec<ColorSpan> {
        self.run(Export::Colors, text.as_bytes(), MAX_RESULT, |bytes| decode_colors(bytes, text)).unwrap_or_default()
    }

    fn reacts_to(&self, typed: char) -> bool {
        self.triggers.contains(&typed)
    }

    fn on_type(&self, text: &str, caret: usize, typed: char) -> Option<Expansion> {
        if !self.reacts_to(typed) || caret > text.len() || !text.is_char_boundary(caret) {
            return None;
        }
        let input = with_typed(text, caret, typed)?;
        self.run(Export::OnType, &input, MAX_JSON, |bytes| decode_expansion(bytes, text, caret)).flatten()
    }

    fn key_references(&self) -> Vec<KeyReferences> {
        self.key_references.clone()
    }

    fn inlays(&self, text: &str) -> Vec<TextInlay> {
        self.run(Export::Inlays, text.as_bytes(), MAX_JSON, |bytes| decode_inlays(bytes, text)).unwrap_or_default()
    }

    fn jvm(&self, text: &str) -> Option<JvmFile> {
        self.run(Export::Jvm, text.as_bytes(), MAX_JSON, |bytes| decode_jvm(bytes, text)).flatten()
    }

    fn run_targets(&self, path: &str, text: &str) -> Vec<RunOffer> {
        let Some(input) = with_path(path, text) else { return Vec::new() };
        self.run(Export::RunTargets, &input, MAX_JSON, |bytes| decode_run_targets(bytes, text, &self.runs))
            .unwrap_or_default()
    }

    fn plugin_id(&self) -> Option<&str> {
        Some(&self.id)
    }

    fn schemas(&self) -> Vec<SchemaDeclaration> {
        self.schemas.clone()
    }

    fn resource(&self, path: &str) -> Option<Vec<u8>> {
        if !resource_path(path) {
            return None;
        }
        self.run(Export::Resource, path.as_bytes(), MAX_JSON, |bytes| Ok(bytes.to_vec()))
            .filter(|bytes| !bytes.is_empty())
    }

    fn runs(&self) -> Vec<String> {
        self.runs.clone()
    }

    fn person_only(&self) -> Vec<PersonOnly> {
        self.person_only.clone()
    }

    fn target_files(&self) -> Vec<String> {
        self.target_files.clone()
    }

    fn tool_windows(&self) -> Vec<ToolWindowDeclaration> {
        self.tool_windows.clone()
    }

    fn index_declaration(&self) -> Option<IndexDeclaration> {
        self.index_declaration.clone()
    }

    fn index(&self, input: &[u8]) -> Option<ProjectIndex> {
        self.run(Export::Index, input, MAX_JSON, decode_index)
    }

    fn tool_rows(&self, section: &str, output: &str) -> Option<Vec<ToolRow>> {
        let input = with_path(section, output)?;
        self.run(Export::ToolRows, &input, MAX_JSON, decode_tool_rows)
    }

    fn converts_from(&self) -> Vec<String> {
        self.converts_from.clone()
    }

    fn convert(&self, from: &str, text: &str) -> Option<Conversion> {
        if !self.converts_from.iter().any(|known| known == from) {
            return None;
        }
        let input = convert_input(from, text)?;
        self.run(Export::Convert, &input, MAX_CONVERTED, |bytes| decode_conversion(bytes, text)).flatten()
    }

    fn in_dialect(&self, dialect: &str) -> Option<Arc<dyn TextFeatures>> {
        self.runtime.lock().ok()?.as_ref()?.dialect?;
        Some(Arc::new(Self {
            id: self.id.clone(),
            abi: self.abi,
            extensions: self.extensions.clone(),
            names: self.names.clone(),
            features: self.features.clone(),
            triggers: self.triggers.clone(),
            key_references: self.key_references.clone(),
            converts_from: self.converts_from.clone(),
            dialect: dialect.to_owned(),
            schemas: self.schemas.clone(),
            runs: self.runs.clone(),
            person_only: self.person_only.clone(),
            target_files: self.target_files.clone(),
            tool_windows: self.tool_windows.clone(),
            index_declaration: self.index_declaration.clone(),
            runtime: Arc::clone(&self.runtime),
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema: u32,
    pub plugins: Vec<CatalogPlugin>,
}

fn known_code_styles<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<CodeStyleDeclaration>, D::Error> {
    let declared = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(declared.into_iter().filter_map(|style| serde_json::from_value(style).ok()).collect())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPlugin {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashbangs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<ContentDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub language_servers: Vec<ServerDeclaration>,
    #[serde(default, skip_serializing_if = "FileTemplateDeclarations::is_empty")]
    pub file_templates: FileTemplateDeclarations,
    #[serde(default, skip_serializing_if = "LiveTemplateDeclarations::is_empty")]
    pub live_templates: LiveTemplateDeclarations,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server_plugins: Vec<ServerPlugin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "ProjectSupport::is_empty")]
    pub project: ProjectSupport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub database_drivers: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_generators: Vec<GeneratorDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_agents: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemas: Vec<SchemaDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub person_only: Vec<PersonOnly>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_windows: Vec<ToolWindowDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<IndexDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "known_code_styles")]
    pub code_styles: Vec<CodeStyleDeclaration>,
    pub versions: Vec<CatalogVersion>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTemplateDeclarations {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<serde_json::Value>,
}

impl FileTemplateDeclarations {
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty() && self.variables.is_empty()
    }

    pub fn parse(json: &str) -> Result<Self, PluginError> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).map_err(|e| error(format!("the file templates are invalid: {e}")))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTemplateDeclarations {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub postfix: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doc_comments: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statements: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surrounders: Vec<serde_json::Value>,
}

impl LiveTemplateDeclarations {
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
            && self.postfix.is_empty()
            && self.doc_comments.is_empty()
            && self.statements.is_empty()
            && self.surrounders.is_empty()
    }

    pub fn parse(json: &str) -> Result<Self, PluginError> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).map_err(|e| error(format!("the live templates are invalid: {e}")))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContentDeclaration {
    pub starts: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unless: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_tag: Option<String>,
}

impl ContentDeclaration {
    pub fn marker(&self) -> zen_vfs::file_types::ContentMarker {
        zen_vfs::file_types::ContentMarker {
            starts: self.starts.clone(),
            unless: self.unless.clone(),
            first_tag: self.first_tag.clone(),
        }
    }

    pub fn parse_list(json: &str) -> Result<Vec<Self>, PluginError> {
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        let list: Vec<Self> =
            serde_json::from_str(json).map_err(|e| error(format!("the content markers are invalid: {e}")))?;
        if list.iter().any(|marker| marker.starts.is_empty()) {
            return Err(error("a content marker needs what the text starts with".to_owned()));
        }
        Ok(list)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVersion {
    pub version: String,
    pub date: String,
    pub abi: u32,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, PluginError> {
        let catalog: Catalog =
            serde_json::from_slice(bytes).map_err(|e| error(format!("the plug-in catalog is invalid: {e}")))?;
        if catalog.schema != CATALOG_SCHEMA {
            return Err(error(format!(
                "the plug-in catalog has schema {}, this IDE reads {CATALOG_SCHEMA}",
                catalog.schema
            )));
        }
        Ok(catalog)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }

    pub fn verify(bytes: &[u8], signature_hex: &str, public_key_hex: &str) -> Result<Self, PluginError> {
        zen_update::verify_signature(bytes, signature_hex, public_key_hex).map_err(|e| error(e.to_string()))?;
        Self::parse(bytes)
    }

    pub fn for_file(&self, file_name: &str) -> Option<&CatalogPlugin> {
        self.plugins
            .iter()
            .find(|plugin| matches_name(&plugin.names, file_name))
            .or_else(|| self.plugins.iter().find(|plugin| supports(&plugin.extensions, &plugin.names, file_name)))
    }

    pub fn for_project(&self, root: &Path) -> Vec<&CatalogPlugin> {
        self.plugins.iter().filter(|plugin| plugin.project.supports(root)).collect()
    }

    pub fn with_requirements(&self, id: &str) -> Vec<&CatalogPlugin> {
        fn visit<'a>(catalog: &'a Catalog, id: &str, seen: &mut Vec<String>, order: &mut Vec<&'a CatalogPlugin>) {
            if seen.iter().any(|known| known == id) {
                return;
            }
            seen.push(id.to_owned());
            let Some(plugin) = catalog.plugins.iter().find(|plugin| plugin.id == id) else { return };
            for required in &plugin.requires {
                visit(catalog, required, seen, order);
            }
            order.push(plugin);
        }
        let mut order = Vec::new();
        visit(self, id, &mut Vec::new(), &mut order);
        order
    }

    pub fn add(&mut self, plugin: CatalogPlugin, version: CatalogVersion) {
        let entry = match self.plugins.iter().position(|existing| existing.id == plugin.id) {
            Some(index) => &mut self.plugins[index],
            None => {
                self.plugins.push(CatalogPlugin { versions: Vec::new(), ..plugin.clone() });
                self.plugins.last_mut().unwrap_or_else(|| unreachable!("just pushed"))
            }
        };
        entry.name = plugin.name;
        entry.description = plugin.description;
        entry.extensions = plugin.extensions;
        entry.names = plugin.names;
        entry.hashbangs = plugin.hashbangs;
        entry.contents = plugin.contents;
        entry.language_servers = plugin.language_servers;
        entry.file_templates = plugin.file_templates;
        entry.live_templates = plugin.live_templates;
        entry.server_plugins = plugin.server_plugins;
        entry.requires = plugin.requires;
        entry.project = plugin.project;
        entry.database_drivers = plugin.database_drivers;
        entry.project_generators = plugin.project_generators;
        entry.tests = plugin.tests;
        entry.terminal_agents = plugin.terminal_agents;
        entry.schemas = plugin.schemas;
        entry.runs = plugin.runs;
        entry.person_only = plugin.person_only;
        entry.target_files = plugin.target_files;
        entry.tool_windows = plugin.tool_windows;
        entry.index = plugin.index;
        entry.versions.retain(|existing| existing.version != version.version);
        entry.versions.push(version);
    }
}

impl CatalogPlugin {
    pub fn latest(&self) -> Option<&CatalogVersion> {
        self.versions
            .iter()
            .filter(|version| ABI_VERSIONS.contains(&version.abi))
            .max_by(|a, b| compare_versions(&a.version, &b.version))
    }

    pub fn version(&self, version: &str) -> Option<&CatalogVersion> {
        self.versions.iter().find(|candidate| candidate.version == version)
    }
}

pub fn is_newer(candidate: &str, installed: &str) -> bool {
    compare_versions(candidate, installed) == std::cmp::Ordering::Greater
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    match (semantic(a), semantic(b)) {
        (Some(a), Some(b)) => a.cmp_precedence(&b),
        _ => {
            let parts = |version: &str| {
                version.split(['.', '-']).map(|part| part.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>()
            };
            parts(a).cmp(&parts(b))
        }
    }
}

fn semantic(version: &str) -> Option<semver::Version> {
    let core = version.find(['-', '+']).unwrap_or(version.len());
    let padding = ".0".repeat(2usize.saturating_sub(version[..core].matches('.').count()));
    semver::Version::parse(&format!("{}{padding}{}", &version[..core], &version[core..])).ok()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub version: String,
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashbangs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<ContentDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub language_servers: Vec<ServerDeclaration>,
    #[serde(default, skip_serializing_if = "FileTemplateDeclarations::is_empty")]
    pub file_templates: FileTemplateDeclarations,
    #[serde(default, skip_serializing_if = "LiveTemplateDeclarations::is_empty")]
    pub live_templates: LiveTemplateDeclarations,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server_plugins: Vec<ServerPlugin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "ProjectSupport::is_empty")]
    pub project: ProjectSupport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub database_drivers: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_generators: Vec<GeneratorDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_agents: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemas: Vec<SchemaDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub person_only: Vec<PersonOnly>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_windows: Vec<ToolWindowDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<IndexDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "known_code_styles")]
    pub code_styles: Vec<CodeStyleDeclaration>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Clone, Debug)]
pub struct Installed {
    folder: PathBuf,
}

impl Installed {
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        Self { folder: folder.into() }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn list(&self) -> Vec<InstalledPlugin> {
        let Ok(entries) = std::fs::read_dir(&self.folder) else { return Vec::new() };
        let mut plugins: Vec<InstalledPlugin> = entries
            .flatten()
            .filter_map(|entry| std::fs::read(entry.path().join("plugin.json")).ok())
            .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
            .collect();
        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        plugins
    }

    pub fn load_all(&self) -> (Vec<WasmHighlighter>, Vec<PluginError>) {
        let mut loaded = Vec::new();
        let mut failed = Vec::new();
        for plugin in self.list() {
            match self.load(&plugin) {
                Ok(highlighter) => loaded.push(highlighter),
                Err(problem) => failed.push(problem),
            }
        }
        (loaded, failed)
    }

    pub fn load(&self, plugin: &InstalledPlugin) -> Result<WasmHighlighter, PluginError> {
        let wasm = std::fs::read(self.folder.join(&plugin.id).join("plugin.wasm"))
            .map_err(|e| error(format!("{}: cannot read the module: {e}", plugin.id)))?;
        WasmHighlighter::load(&plugin.id, &wasm, &plugin.extensions).map(|highlighter| {
            highlighter
                .with_names(&plugin.names)
                .with_schemas(&plugin.schemas)
                .with_runs(&plugin.runs)
                .with_person_only(&plugin.person_only)
                .with_target_files(&plugin.target_files)
                .with_tool_windows(&plugin.tool_windows)
                .with_index(plugin.index.as_ref())
        })
    }

    pub fn uninstall(&self, id: &str) -> Result<(), PluginError> {
        let taken_out = self.take_out(id)?;
        std::fs::remove_dir_all(&taken_out).map_err(|e| error(format!("cannot remove {}: {e}", taken_out.display())))
    }

    pub fn take_out(&self, id: &str) -> Result<PathBuf, PluginError> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(error(format!("`{id}` is not a valid plug-in id")));
        }
        let folder = self.folder.join(id);
        if !folder.join("plugin.json").is_file() {
            return Err(error(format!("the plug-in `{id}` is not installed")));
        }
        let trash = self.folder.join(".trash");
        std::fs::create_dir_all(&trash).map_err(|e| error(format!("cannot create {}: {e}", trash.display())))?;
        let taken_out = (0..)
            .map(|copy| trash.join(format!("{id}-{}-{copy}", std::process::id())))
            .find(|name| !name.exists())
            .unwrap_or_else(|| trash.join(id));
        std::fs::rename(&folder, &taken_out).map_err(|e| error(format!("cannot remove {}: {e}", folder.display())))?;
        Ok(taken_out)
    }

    pub fn driver_program(&self, plugin: &str, driver: &str, file: &str) -> PathBuf {
        self.folder.join(plugin).join("drivers").join(driver).join(file)
    }

    pub fn install_driver_program(
        &self,
        plugin: &str,
        driver: &str,
        file: &str,
        sha256: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, PluginError> {
        let plain = |text: &str| {
            !text.is_empty()
                && text != "."
                && text != ".."
                && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        if !plain(plugin) || !plain(driver) || !plain(file) {
            return Err(error(format!("`{plugin}/{driver}/{file}` is not a valid driver program")));
        }
        let found = zen_update::sha256_hex(bytes);
        if !found.eq_ignore_ascii_case(sha256.trim()) {
            return Err(error(format!("the SHA-256 of the driver program {file} is {found}, not {sha256}")));
        }
        let path = self.driver_program(plugin, driver, file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| error(format!("cannot create {}: {e}", parent.display())))?;
        }
        zen_vfs::write_atomic(&path, bytes).map_err(|e| error(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| error(format!("cannot make {} executable: {e}", path.display())))?;
        }
        Ok(path)
    }

    pub fn install(
        &self,
        plugin: &CatalogPlugin,
        version: &CatalogVersion,
        wasm: &[u8],
    ) -> Result<WasmHighlighter, PluginError> {
        if !plugin.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || plugin.id.is_empty() {
            return Err(error(format!("`{}` is not a valid plug-in id", plugin.id)));
        }
        let highlighter = WasmHighlighter::load(&plugin.id, wasm, &plugin.extensions)?
            .with_names(&plugin.names)
            .with_schemas(&plugin.schemas)
            .with_runs(&plugin.runs)
            .with_person_only(&plugin.person_only)
            .with_target_files(&plugin.target_files)
            .with_tool_windows(&plugin.tool_windows)
            .with_index(plugin.index.as_ref());
        let folder = self.folder.join(&plugin.id);
        std::fs::create_dir_all(&folder).map_err(|e| error(format!("cannot create {}: {e}", folder.display())))?;
        let manifest = InstalledPlugin {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            description: plugin.description.clone(),
            version: version.version.clone(),
            extensions: plugin.extensions.clone(),
            names: plugin.names.clone(),
            hashbangs: plugin.hashbangs.clone(),
            contents: plugin.contents.clone(),
            language_servers: plugin.language_servers.clone(),
            file_templates: plugin.file_templates.clone(),
            live_templates: plugin.live_templates.clone(),
            server_plugins: plugin.server_plugins.clone(),
            requires: plugin.requires.clone(),
            project: plugin.project.clone(),
            database_drivers: plugin.database_drivers.clone(),
            project_generators: plugin.project_generators.clone(),
            tests: plugin.tests.clone(),
            terminal_agents: plugin.terminal_agents.clone(),
            schemas: plugin.schemas.clone(),
            runs: plugin.runs.clone(),
            person_only: plugin.person_only.clone(),
            target_files: plugin.target_files.clone(),
            tool_windows: plugin.tool_windows.clone(),
            index: plugin.index.clone(),
            code_styles: plugin.code_styles.clone(),
            notes: version.notes.clone(),
        };
        zen_vfs::write_atomic(&folder.join("plugin.wasm"), wasm).map_err(|e| error(e.to_string()))?;
        let json = serde_json::to_vec_pretty(&manifest).unwrap_or_default();
        zen_vfs::write_atomic(&folder.join("plugin.json"), &json).map_err(|e| error(e.to_string()))?;
        Ok(highlighter)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod fetch {
    use super::{Catalog, CatalogPlugin, CatalogVersion, PluginError, error};

    pub fn catalog(catalog_url: &str, public_key: &str) -> Result<Catalog, PluginError> {
        let bytes = zen_update::fetch::signed(catalog_url, public_key).map_err(|e| error(e.to_string()))?;
        Catalog::parse(&bytes)
    }

    pub fn file(catalog_url: &str, url: &str, limit: u64) -> Result<Vec<u8>, PluginError> {
        let url = zen_update::fetch::resolve(catalog_url, url);
        zen_update::fetch::download(&url, limit, |_, _| {}).map_err(|e| error(e.to_string()))
    }

    pub fn module(
        catalog_url: &str,
        _plugin: &CatalogPlugin,
        version: &CatalogVersion,
    ) -> Result<Vec<u8>, PluginError> {
        let url = zen_update::fetch::resolve(catalog_url, &version.url);
        zen_update::fetch::checked(&url, version.size, &version.sha256).map_err(|e| error(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABI_V1_EXPORTS: [Export; 18] = [
        Export::Fold,
        Export::Outline,
        Export::Complete,
        Export::Expand,
        Export::Colors,
        Export::OnType,
        Export::Triggers,
        Export::Problems,
        Export::Dialect,
        Export::Jvm,
        Export::Project,
        Export::Inlays,
        Export::KeyReferences,
        Export::Resource,
        Export::RunTargets,
        Export::ToolRows,
        Export::Convert,
        Export::ConvertFrom,
    ];

    fn answered_abi(source: &str) -> Option<i32> {
        let after = source.split("fn zen_abi() -> i32 {").nth(1)?;
        after.split('}').next()?.trim().parse().ok()
    }

    #[test]
    fn a_plug_in_with_an_abi_v1_export_answers_abi_1_or_the_host_never_calls_it() {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let mut wrong = Vec::new();
        for entry in std::fs::read_dir(&plugins).unwrap().flatten() {
            let Ok(files) = std::fs::read_dir(entry.path().join("src")) else { continue };
            let source: String = files
                .flatten()
                .filter(|file| file.path().extension().is_some_and(|extension| extension == "rs"))
                .map(|file| std::fs::read_to_string(file.path()).unwrap_or_default())
                .collect();
            let exported: Vec<&str> = ABI_V1_EXPORTS
                .iter()
                .map(|export| export.name())
                .filter(|name| source.contains(&format!("pub extern \"C\" fn {name}(")))
                .collect();
            if !exported.is_empty() && answered_abi(&source) == Some(0) {
                wrong.push(format!("{}: {}", entry.file_name().to_string_lossy(), exported.join(", ")));
            }
        }
        assert!(wrong.is_empty(), "these answer zen_abi 0, so the host ignores their exports: {wrong:?}");
    }

    const TWO_BYTES: &str = r#"
        (module
          (memory (export "memory") 1)
          (func (export "zen_abi") (result i32) i32.const 0)
          (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
          (func (export "zen_highlight") (param i32 i32) (result i64)
            (i32.store (i32.const 0) (i32.const 0))
            (i32.store (i32.const 4) (i32.const 2))
            (i32.store (i32.const 8) (i32.const 0))
            i64.const 12))
    "#;

    fn module(wat: &str) -> Vec<u8> {
        wat::parse_str(wat).unwrap()
    }

    fn extensions() -> Vec<String> {
        vec!["md".to_owned()]
    }

    #[test]
    fn a_catalog_without_code_styles_or_with_a_newer_kind_of_them_still_reads() {
        let plugin = |styles: &str| {
            format!(
                r#"{{ "schema": {CATALOG_SCHEMA}, "plugins": [{{ "id": "p", "name": "P", "description": "", "extensions": [], "versions": [] {styles} }}] }}"#
            )
        };
        let old = Catalog::parse(plugin("").as_bytes()).unwrap();
        assert!(old.plugins[0].code_styles.is_empty(), "a catalog from before code styles");
        let known = r#"{ "language": "x", "title": { "en": "X" }, "options": [] }"#;
        let newer = r#"{ "language": "y", "title": { "en": "Y" }, "options": [{ "id": "a", "label": { "en": "A" },
            "tab": "aTabOfALaterVersion", "kind": { "type": "boolean" }, "default": true }] }"#;
        let both = plugin(&format!(r#", "codeStyles": [{known}, {newer}]"#));
        let read = Catalog::parse(both.as_bytes()).unwrap();
        let languages: Vec<&str> = read.plugins[0].code_styles.iter().map(|style| style.language.as_str()).collect();
        assert_eq!(languages, ["x"], "a declaration this IDE cannot read is left out, not the whole catalog");
    }

    #[test]
    fn runs_a_plugin() {
        let highlighter = WasmHighlighter::load("two", &module(TWO_BYTES), &extensions()).unwrap();
        assert!(highlighter.handles("README.md") && highlighter.handles("x.MD") && !highlighter.handles("main.go"));
        assert_eq!(highlighter.highlight("# hi"), [Span { range: 0..2, kind: HighlightKind::Keyword }]);
        assert!(highlighter.highlight("#").is_empty());
        assert!(!highlighter.is_disabled());
    }

    #[test]
    fn refuses_imports_and_unknown_abis() {
        let importing = TWO_BYTES.replace("(module", r#"(module (import "env" "clock" (func)) "#);
        assert!(
            WasmHighlighter::load("x", &module(&importing), &extensions()).unwrap_err().0.contains("may not import")
        );
        let future = TWO_BYTES.replace("(result i32) i32.const 0)", "(result i32) i32.const 7)");
        assert!(WasmHighlighter::load("x", &module(&future), &extensions()).unwrap_err().0.contains("ABI version 7"));
        assert!(WasmHighlighter::load("x", b"not wasm", &extensions()).is_err());
    }

    #[test]
    fn tells_a_plugin_the_dialect_before_every_call() {
        let told = r#"(module
          (memory (export "memory") 1)
          (global $told (mut i32) (i32.const 0))
          (func (export "zen_abi") (result i32) i32.const 1)
          (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
          (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0)
          (func (export "zen_dialect") (param i32 i32) (result i64) (global.set $told (local.get 1)) i64.const 0)
          (func (export "zen_fold") (param i32 i32) (result i64)
            (i32.store (i32.const 0) (i32.const 0))
            (i32.store (i32.const 4) (global.get $told))
            i64.const 8))"#;
        let text = "a\nb\nc\nd\ne\nf\ng";
        let plugin = WasmHighlighter::load("sql", &module(told), &extensions()).unwrap();
        assert!(plugin.folds(text).is_empty(), "no dialect: an empty name");
        let mysql = plugin.in_dialect("mysql").unwrap();
        assert_eq!(mysql.folds(text), [LineFold { start_line: 0, end_line: 5 }]);
        assert!(plugin.folds(text).is_empty(), "told again before each call");
        assert_eq!(mysql.folds(text), [LineFold { start_line: 0, end_line: 5 }]);
        let without = WasmHighlighter::load("two", &module(TWO_BYTES), &extensions()).unwrap();
        assert!(without.in_dialect("mysql").is_none(), "a module without zen_dialect");
    }

    #[test]
    fn switches_off_a_plugin_that_loops() {
        let looping = TWO_BYTES.replace("(i32.store (i32.const 0) (i32.const 0))", "(loop $forever (br $forever))");
        let highlighter = WasmHighlighter::load("loop", &module(&looping), &extensions()).unwrap();
        assert!(highlighter.highlight("text").is_empty());
        assert!(highlighter.is_disabled());
    }

    #[test]
    fn decodes_defensively() {
        let record =
            |start: u32, end: u32, kind: u32| [start.to_le_bytes(), end.to_le_bytes(), kind.to_le_bytes()].concat();
        let bytes =
            [record(0, 1, 0), record(0, 1, 0), record(1, 3, 99), record(1, 3, 16), record(3, 2, 0), record(5, 9, 0)]
                .concat();
        assert_eq!(
            decode(&bytes, "abcdé"),
            [Span { range: 0..1, kind: HighlightKind::Keyword }, Span { range: 1..3, kind: HighlightKind::Code },]
        );
    }

    fn data(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("\\{byte:02x}")).collect()
    }

    fn packed(pointer: u64, length: usize) -> u64 {
        (pointer << 32) | length as u64
    }

    const FOLDS: &[(u32, u32)] = &[(0, 2), (0, 2), (3, 1), (1, 99), (1, 2)];
    const OUTLINE: &str = r#"[{"name":"html","kind":"element","start":0,"end":14,"children":[{"name":"body","kind":"weird","start":6,"end":14},{"name":"bad","kind":"rule","start":2,"end":99}]},{"name":"half","kind":"rule","start":1,"end":2}]"#;
    const COMPLETE: &str = r#"{"from":1,"items":[{"label":"div","kind":"tag","insert":"<div>$0</div>","detail":"block"},{"label":" ","kind":"tag"},{"label":"span","kind":"zzz"}]}"#;
    const EXPAND: &str = r#"{"from":0,"to":3,"text":"<ul>$0</ul>"}"#;

    fn v1(fold_body: Option<&str>) -> Vec<u8> {
        let folds: Vec<u8> =
            FOLDS.iter().flat_map(|(start, end)| [start.to_le_bytes(), end.to_le_bytes()].concat()).collect();
        let fold = fold_body.map_or_else(|| format!("i64.const {}", packed(0, folds.len())), str::to_owned);
        module(&format!(
            r#"(module
              (memory (export "memory") 1)
              (data (i32.const 0) "{folds}")
              (data (i32.const 1024) "{outline}")
              (data (i32.const 2048) "{complete}")
              (data (i32.const 3072) "{expand}")
              (func (export "zen_abi") (result i32) i32.const 1)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 8192)
              (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0)
              (func (export "zen_fold") (param i32 i32) (result i64) {fold})
              (func (export "zen_outline") (param i32 i32) (result i64) i64.const {outline_at})
              (func (export "zen_complete") (param i32 i32) (result i64) i64.const {complete_at})
              (func (export "zen_expand") (param i32 i32) (result i64)
                (if (result i64) (i32.eqz (i32.load (local.get 0)))
                  (then i64.const 0)
                  (else i64.const {expand_at}))))"#,
            folds = data(&folds),
            outline = data(OUTLINE.as_bytes()),
            complete = data(COMPLETE.as_bytes()),
            expand = data(EXPAND.as_bytes()),
            outline_at = packed(1024, OUTLINE.len()),
            complete_at = packed(2048, COMPLETE.len()),
            expand_at = packed(3072, EXPAND.len()),
        ))
    }

    #[test]
    fn runs_the_features_of_abi_v1() {
        let plugin = WasmHighlighter::load("web", &v1(None), &["html".to_owned()]).unwrap();
        assert_eq!(plugin.abi(), 1);
        for feature in [Feature::Folding, Feature::Outline, Feature::Completion, Feature::Expansion] {
            assert!(TextFeatures::supports(&plugin, feature));
        }
        assert_eq!(
            plugin.folds("a\nb\nc\nd"),
            [LineFold { start_line: 0, end_line: 2 }, LineFold { start_line: 1, end_line: 2 }]
        );
        let text = "<html><body>é</body></html>";
        let outline = zen_syntax::Outliner::outline(&plugin, text);
        assert_eq!(outline.len(), 2, "{outline:?}");
        assert_eq!((outline[0].name.as_str(), outline[0].kind), ("html", SymbolKind::Element));
        assert_eq!((outline[0].start, outline[0].end), (0, 13), "character offsets");
        assert_eq!(outline[0].children.len(), 1, "the child out of its parent is dropped");
        assert_eq!(outline[0].children[0].kind, SymbolKind::Key, "unknown kinds are keys");
        assert_eq!((outline[0].children[0].start, outline[0].children[0].end), (6, 13));
        let suggestions = plugin.complete("<d", 2).unwrap();
        assert_eq!(suggestions.from, 1);
        assert_eq!(suggestions.items.len(), 2, "the empty label is dropped");
        assert_eq!(suggestions.items[0].insert, "<div></div>");
        assert_eq!(suggestions.items[0].caret, Some(5));
        assert_eq!(suggestions.items[0].detail.as_deref(), Some("block"));
        assert_eq!((suggestions.items[1].insert.as_str(), suggestions.items[1].kind.as_str()), ("span", "key"));
        assert!(plugin.complete("<d", 9).is_none(), "a caret past the text is not sent");
        assert!(plugin.expand("ul>", 0).is_none(), "the module answers nothing at 0");
        let expansion = plugin.expand("ul>", 3).unwrap();
        assert_eq!((expansion.range, expansion.text.as_str(), expansion.caret), (0..3, "<ul></ul>", Some(4)));
        assert!(plugin.expand("ul>li", 5).is_none(), "an expansion that does not end at the caret");
        assert!(!plugin.is_disabled());
    }

    const ON_TYPE: &str = r#"{"from":3,"to":3,"text":"$0</a>"}"#;

    fn colors_and_typing(triggers: &str) -> Vec<u8> {
        let record =
            |start: u32, end: u32, rgba: u32| [start.to_le_bytes(), end.to_le_bytes(), rgba.to_le_bytes()].concat();
        let colors = [
            record(0, 4, 0xff00_00ff),
            record(2, 6, 0x0000_00ff),
            record(5, 9, 0x0102_0380),
            record(9, 99, 0),
            record(10, 12, 0),
        ]
        .concat();
        module(&format!(
            r#"(module
              (memory (export "memory") 1)
              (data (i32.const 0) "{colors}")
              (data (i32.const 1024) "{on_type}")
              (data (i32.const 2048) "{triggers}")
              (func (export "zen_abi") (result i32) i32.const 1)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 8192)
              (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0)
              (func (export "zen_colors") (param i32 i32) (result i64) i64.const {colors_at})
              (func (export "zen_on_type_triggers") (param i32 i32) (result i64) i64.const {triggers_at})
              (func (export "zen_on_type") (param i32 i32) (result i64)
                (if (result i64) (i32.ne (i32.load (i32.add (local.get 0) (i32.const 4))) (i32.const 62))
                  (then i64.const 0)
                  (else i64.const {on_type_at}))))"#,
            colors = data(&colors),
            on_type = data(ON_TYPE.as_bytes()),
            triggers = data(triggers.as_bytes()),
            colors_at = packed(0, colors.len()),
            on_type_at = packed(1024, ON_TYPE.len()),
            triggers_at = packed(2048, triggers.len()),
        ))
    }

    #[test]
    fn runs_colours_and_typing() {
        let plugin = WasmHighlighter::load("web", &colors_and_typing(">/>"), &["html".to_owned()]).unwrap();
        assert!(TextFeatures::supports(&plugin, Feature::Colors) && TextFeatures::supports(&plugin, Feature::Typing));
        assert_eq!(
            plugin.colors("#f00 #000 é"),
            [
                ColorSpan { range: 0..4, rgba: [0xff, 0, 0, 0xff] },
                ColorSpan { range: 5..9, rgba: [1, 2, 3, 0x80] },
                ColorSpan { range: 10..12, rgba: [0, 0, 0, 0] },
            ],
            "overlapping and out-of-bounds records are dropped"
        );
        assert!(plugin.reacts_to('>') && plugin.reacts_to('/') && !plugin.reacts_to('a'));
        let added = plugin.on_type("<a>", 3, '>').unwrap();
        assert_eq!((added.range, added.text.as_str(), added.caret), (3..3, "</a>", Some(0)));
        assert!(plugin.on_type("<a/", 3, '/').is_none(), "the module answers nothing");
        assert!(plugin.on_type("<a>", 3, 'x').is_none(), "not a trigger");
        assert!(plugin.on_type("<a>b", 4, '>').is_none(), "an answer that does not end at the caret");
        assert!(!plugin.is_disabled());
        let silent = WasmHighlighter::load("web", &colors_and_typing(""), &["html".to_owned()]).unwrap();
        assert!(!TextFeatures::supports(&silent, Feature::Typing), "no triggers, no typing");
        assert!(silent.on_type("<a>", 3, '>').is_none());
        let plain = WasmHighlighter::load("two", &module(TWO_BYTES), &extensions()).unwrap();
        assert!(plain.colors("#fff").is_empty() && !plain.reacts_to('>'));
    }

    fn typing_with(body: &str) -> Vec<u8> {
        module(&format!(
            r#"(module
              (memory (export "memory") 1)
              (data (i32.const 2048) ">")
              (func (export "zen_abi") (result i32) i32.const 1)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 8192)
              (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0)
              (func (export "zen_on_type_triggers") (param i32 i32) (result i64) i64.const {triggers})
              (func (export "zen_on_type") (param i32 i32) (result i64) {body}))"#,
            triggers = packed(2048, 1),
        ))
    }

    #[test]
    fn a_module_converts_from_the_languages_it_names_and_its_problems_stay_in_the_source() {
        let json = r#"{"text":"class A\n","problems":[{"start":0,"end":5,"message":"check this"},{"start":0,"end":999,"message":"out of the text"}]}"#;
        let converting = module(&format!(
            r#"(module
              (memory (export "memory") 1)
              (data (i32.const 2048) "java,,K o t")
              (data (i32.const 3072) "{data}")
              (func (export "zen_abi") (result i32) i32.const 1)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 8192)
              (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0)
              (func (export "zen_convert_from") (param i32 i32) (result i64) i64.const {from})
              (func (export "zen_convert") (param i32 i32) (result i64) i64.const {answer}))"#,
            data = json.replace('\\', "\\\\").replace('"', "\\\""),
            from = packed(2048, 11),
            answer = packed(3072, json.len()),
        ));
        let plugin = WasmHighlighter::load("kotlin", &converting, &["kt".to_owned()]).unwrap();
        assert!(TextFeatures::supports(&plugin, Feature::Conversion));
        assert_eq!(plugin.converts_from(), ["java"], "names that are not plain words are left out");
        let converted = plugin.convert("java", "class A {}").unwrap();
        assert_eq!(converted.text, "class A\n");
        assert_eq!(converted.problems.len(), 1, "a problem outside the source is dropped");
        assert_eq!((converted.problems[0].range.clone(), converted.problems[0].message.as_str()), (0..5, "check this"));
        assert!(plugin.convert("scala", "class A {}").is_none(), "a language it does not name");
        let plain = WasmHighlighter::load("two", &module(TWO_BYTES), &extensions()).unwrap();
        assert!(!TextFeatures::supports(&plain, Feature::Conversion) && plain.convert("java", "x").is_none());
    }

    #[test]
    fn a_pre_release_is_older_than_its_release() {
        assert!(!is_newer("1.0.0-rc1", "1.0.0"));
        assert!(is_newer("1.0.0", "1.0.0-rc1"));
        assert!(is_newer("1.0.0-rc2", "1.0.0-rc1"));
        assert!(is_newer("1.0.1-rc1", "1.0.0"));
        assert!(is_newer("1.0.0-rc.11", "1.0.0-rc.2"));
    }

    #[test]
    fn short_and_unusual_versions_still_compare() {
        assert!(is_newer("1.2", "1.1.9"));
        assert!(!is_newer("1.2", "1.2.0"));
        assert!(!is_newer("1.0-beta", "1.0"));
        assert!(!is_newer("1.0.0+build2", "1.0.0+build1"));
        assert!(is_newer("1.0.0.1", "1.0.0"), "four numbers are compared one by one");
    }

    #[test]
    fn a_plugin_cannot_grow_its_memory_past_the_limit() {
        let growing = module(&format!(
            r#"(module
              (memory (export "memory") 1)
              (func (export "zen_abi") (result i32) i32.const 0)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
              (func (export "zen_highlight") (param i32 i32) (result i64)
                (if (i32.ne (memory.grow (i32.const {pages})) (i32.const -1)) (then unreachable))
                i64.const 0))"#,
            pages = MAX_MEMORY / 65536,
        ));
        let plugin = WasmHighlighter::load("big", &growing, &["txt".to_owned()]).unwrap();
        assert!(plugin.highlight("x").is_empty());
        assert!(!plugin.is_disabled(), "the growth is refused, and the module goes on");
    }

    #[test]
    fn a_plugin_that_asks_for_more_memory_at_start_is_not_loaded() {
        let big = module(&format!(
            r#"(module
              (memory (export "memory") {pages})
              (func (export "zen_abi") (result i32) i32.const 0)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
              (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0))"#,
            pages = MAX_MEMORY / 65536 + 1,
        ));
        let refused = WasmHighlighter::load("big", &big, &["txt".to_owned()]).err().unwrap();
        assert!(refused.0.contains("cannot start"), "{refused}");
    }

    #[test]
    fn typing_that_traps_or_runs_forever_switches_the_plugin_off() {
        for body in ["unreachable", "(loop $forever (br $forever)) i64.const 0"] {
            let plugin = WasmHighlighter::load("web", &typing_with(body), &["html".to_owned()]).unwrap();
            assert!(plugin.on_type("x", 1, 'x').is_none() && !plugin.is_disabled(), "not a trigger: not called");
            assert!(plugin.on_type("<a>", 3, '>').is_none());
            assert!(plugin.is_disabled(), "{body}");
            assert!(!TextFeatures::supports(&plugin, Feature::Folding));
        }
    }

    #[test]
    fn exports_this_host_does_not_know_are_ignored() {
        let future = module(
            r#"(module
              (memory (export "memory") 1)
              (func (export "zen_abi") (result i32) i32.const 1)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 8192)
              (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0)
              (func (export "zen_something_later") (param i64) (result f32) f32.const 0))"#,
        );
        let plugin = WasmHighlighter::load("later", &future, &extensions()).unwrap();
        assert!(plugin.colors("#fff").is_empty() && !plugin.reacts_to('>') && !plugin.is_disabled());
    }

    #[test]
    fn inlays_keep_the_first_line_on_character_boundaries_in_order() {
        let text = "a=\\u00e9\nb=é\n";
        let json = r#"[{"offset":13,"text":"= é"},{"offset":8,"text":"= é\nsecond"},{"offset":12,"text":"x"},{"offset":99,"text":"y"},{"offset":0,"text":" "}]"#;
        let inlays = decode_inlays(json.as_bytes(), text).unwrap();
        assert_eq!(
            inlays,
            [TextInlay { offset: 8, text: "= é".into() }, TextInlay { offset: 13, text: "= é".into() }],
            "past the end, inside a character and blank are dropped"
        );
        assert_eq!(decode_inlays(b"", text), Ok(Vec::new()));
        assert!(decode_inlays(b"{", text).is_err());
    }

    #[test]
    fn decodes_colours_and_triggers_defensively() {
        assert!(decode_colors(&[0; 11], "x").is_err());
        assert_eq!(decode_colors(&[], "x"), Ok(Vec::new()));
        let split = [1u32.to_le_bytes(), 2u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
        assert_eq!(decode_colors(&split, "é"), Ok(Vec::new()), "not on a character boundary");
        let one = [0u32.to_le_bytes(), 1u32.to_le_bytes(), 0x1122_3344u32.to_le_bytes()].concat();
        let many = one.repeat(3);
        assert_eq!(decode_colors(&many, "x").unwrap(), [ColorSpan { range: 0..1, rgba: [0x11, 0x22, 0x33, 0x44] }]);
        assert_eq!(decode_triggers(">/\n>".as_bytes()), Ok(vec!['>', '/']));
        assert_eq!(decode_triggers("abcdefghijklmnopqrstuvwxyz0123456789".as_bytes()).unwrap().len(), MAX_TRIGGERS);
        assert!(decode_triggers(&[0xff]).is_err());
    }

    #[test]
    fn jvm_answers_turn_offsets_into_lines_and_name_the_file_s_class() {
        let text = "package a\n\nfun main() {\n}\nclass B {\n}\n";
        let json = r#"{"package":"a","fileClass":"","classes":[{"name":"a.B","start":26,"end":36},{"name":" ","start":0,"end":1}],
            "mains":[{"class":"","start":16,"end":24}],
            "tests":[{"name":"a.B","start":32,"testng":true,"methods":[{"name":"t","start":999}]}]}"#;
        let file = decode_jvm(json.as_bytes(), text).unwrap().unwrap().named_after("app-main");
        assert_eq!(file.file_class, "a.App_mainKt");
        assert_eq!(file.classes, [JvmClass { name: "a.B".into(), first_line: 4, last_line: 5 }]);
        assert_eq!(file.mains, [JvmMain { class: "a.App_mainKt".into(), class_line: 2, line: 2, end_line: 3 }]);
        assert_eq!(file.tests[0].methods, [("t".to_owned(), 6)], "past the end is the last line");
        assert!(file.tests[0].testng);
        assert_eq!((file.class_at(4), file.class_at(2)), ("a.B", "a.App_mainKt"));
        assert_eq!(decode_jvm(b"", text), Ok(None));
        assert!(decode_jvm(b"[", text).is_err());
        let named = decode_jvm(br#"{"package":"a","fileClass":"a.Run"}"#, text).unwrap().unwrap().named_after("x");
        assert_eq!(named.file_class, "a.Run");
    }

    #[test]
    fn a_v0_module_has_no_features() {
        let plugin = WasmHighlighter::load("two", &module(TWO_BYTES), &extensions()).unwrap();
        assert_eq!(plugin.abi(), 0);
        assert!(!TextFeatures::supports(&plugin, Feature::Folding));
        assert!(plugin.folds("a\nb\nc").is_empty());
        assert!(zen_syntax::Outliner::outline(&plugin, "# a").is_empty());
        assert!(plugin.complete("a", 1).is_none() && plugin.expand("a", 1).is_none());
        assert!(!plugin.is_disabled());
    }

    #[test]
    fn the_fuel_grows_with_the_input_up_to_a_limit() {
        assert_eq!(fuel_for(0), FUEL_PER_CALL);
        assert_eq!(fuel_for(1_000_000), FUEL_PER_CALL + 1_000_000 * FUEL_PER_BYTE);
        assert_eq!(fuel_for(usize::MAX), MAX_FUEL_PER_CALL);
    }

    #[test]
    fn a_v1_plugin_that_traps_or_overflows_is_switched_off() {
        let trapping = WasmHighlighter::load("trap", &v1(Some("unreachable")), &extensions()).unwrap();
        assert!(trapping.folds("a\nb\nc").is_empty());
        assert!(trapping.is_disabled());
        assert!(trapping.complete("<d", 2).is_none(), "nothing runs once it is off");
        let huge = format!("i64.const {}", packed(0, MAX_RESULT + 8));
        let overflowing = WasmHighlighter::load("huge", &v1(Some(&huge)), &extensions()).unwrap();
        assert!(overflowing.folds("a\nb").is_empty());
        assert!(overflowing.is_disabled());
        let looping =
            WasmHighlighter::load("loop", &v1(Some("(loop $forever (br $forever)) i64.const 0")), &extensions())
                .unwrap();
        assert!(looping.folds("a\nb").is_empty());
        assert!(looping.is_disabled());
    }

    #[test]
    fn refuses_a_v1_export_of_the_wrong_type() {
        let wrong = TWO_BYTES.replace("(result i32) i32.const 0)", "(result i32) i32.const 1)").replace(
            "(func (export \"zen_alloc\")",
            "(func (export \"zen_fold\") (param i32) (result i32) i32.const 0)\n(func (export \"zen_alloc\")",
        );
        let problem = WasmHighlighter::load("x", &module(&wrong), &extensions()).unwrap_err();
        assert!(problem.0.contains("zen_fold"), "{problem}");
    }

    #[test]
    fn decodes_problems_defensively() {
        let text = "<a>é</b>";
        let answer = r#"[
            {"start": 5, "end": 9, "severity": "error", "message": "end tag </b> does not match <a>\nmore"},
            {"start": 0, "end": 3, "severity": "warning", "message": "first"},
            {"start": 4, "end": 5, "message": "inside é"},
            {"start": 5, "end": 2, "message": "reversed"},
            {"start": 0, "end": 99, "message": "past the end"},
            {"start": 0, "end": 1, "message": "  "}
        ]"#;
        let problems = decode_problems(answer.as_bytes(), text).unwrap();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(
            (problems[0].range.clone(), problems[0].error, problems[0].message.as_str()),
            (0..3, false, "first")
        );
        assert_eq!(problems[1].message, "end tag </b> does not match <a>", "one line");
        assert!(problems[1].error, "error unless it says warning");
        assert_eq!(decode_problems(b"", text), Ok(Vec::new()));
        assert!(decode_problems(b"{", text).is_err());
        let many = format!("[{}]", vec![r#"{"start":0,"end":1,"message":"m"}"#; MAX_PROBLEMS + 3].join(","));
        assert_eq!(decode_problems(many.as_bytes(), text).unwrap().len(), MAX_PROBLEMS);
    }

    #[test]
    fn decodes_v1_answers_defensively() {
        assert!(decode_folds(&[0; 7], 10).is_err());
        assert_eq!(decode_folds(&[], 10), Ok(Vec::new()));
        assert!(decode_outline(b"{not json", "x").is_err());
        assert_eq!(decode_outline(b"", "x"), Ok(Vec::new()));
        let split = r#"[{"name":"é","kind":"rule","start":1,"end":2}]"#;
        assert_eq!(decode_outline(split.as_bytes(), "é"), Ok(Vec::new()), "not on a character boundary");
        let deep = format!("{}{}", r#"[{"name":"a","start":0,"end":1,"children":"#.repeat(200), "[]}]".repeat(200));
        assert!(decode_outline(deep.as_bytes(), "a").is_err(), "nesting is limited");
        let long = format!(r#"[{{"name":"{}","start":0,"end":0}}]"#, "n".repeat(1000));
        assert_eq!(decode_outline(long.as_bytes(), "").unwrap().first().map(|item| item.name.len()), Some(MAX_NAME));
        assert_eq!(
            decode_suggestions(br#"{"from":3,"items":[{"label":"a"}]}"#, "abc", 2),
            Ok(None),
            "from after the caret"
        );
        assert_eq!(decode_suggestions(br#"{"from":0,"items":[]}"#, "abc", 2), Ok(None));
        assert!(decode_suggestions(br#"{"items":[]}"#, "abc", 2).is_err());
        let many = format!(r#"{{"from":0,"items":[{}]}}"#, vec![r#"{"label":"a"}"#; MAX_SUGGESTIONS + 5].join(","));
        assert_eq!(decode_suggestions(many.as_bytes(), "", 0).unwrap().unwrap().items.len(), MAX_SUGGESTIONS);
        assert_eq!(decode_expansion(br#"{"from":2,"to":1,"text":"x"}"#, "abc", 1), Ok(None));
        assert_eq!(decode_expansion(br#"{"from":0,"to":9,"text":"x"}"#, "abc", 9), Ok(None));
        assert_eq!(
            decode_expansion(br#"{"from":0,"to":1,"text":"a$0b$0"}"#, "abc", 1),
            Ok(Some(Expansion { range: 0..1, text: "ab$0".to_owned(), caret: Some(1) })),
            "only the first caret mark is taken"
        );
    }

    #[test]
    fn a_catalog_without_notes_still_loads_and_writes_none() {
        let old = br#"{ "schema": 1, "plugins": [{ "id": "sql", "name": "SQL", "description": "", "extensions": ["sql"],
            "versions": [{ "version": "0.1.0", "date": "2026-10-01", "abi": 0, "url": "sql.wasm", "sha256": "00", "size": 1 }] }] }"#;
        let catalog = Catalog::parse(old).unwrap();
        assert_eq!(catalog.plugins[0].versions[0].notes, "");
        assert!(!String::from_utf8(catalog.to_bytes()).unwrap().contains("notes"), "no empty field is written");
    }

    #[test]
    fn the_signature_covers_the_notes() {
        let (secret, public) = zen_update::key_pair_from_seed([7; 32]);
        let mut catalog = Catalog { schema: CATALOG_SCHEMA, plugins: Vec::new() };
        let plugin = CatalogPlugin {
            id: "sql".into(),
            name: "SQL".into(),
            description: String::new(),
            extensions: vec!["sql".into()],
            names: Vec::new(),
            hashbangs: Vec::new(),
            contents: Vec::new(),
            language_servers: Vec::new(),
            file_templates: FileTemplateDeclarations::default(),
            live_templates: LiveTemplateDeclarations::default(),
            server_plugins: Vec::new(),
            requires: Vec::new(),
            project: ProjectSupport::default(),
            database_drivers: Vec::new(),
            project_generators: Vec::new(),
            tests: Vec::new(),
            terminal_agents: Vec::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
            code_styles: Vec::new(),
            versions: Vec::new(),
        };
        let version = CatalogVersion {
            version: "0.1.0".into(),
            date: "2026-10-19".into(),
            abi: 0,
            url: "sql.wasm".into(),
            sha256: "00".into(),
            size: 1,
            notes: "- Signed notes.".into(),
        };
        catalog.add(plugin, version);
        let bytes = catalog.to_bytes();
        let signature = zen_update::sign_manifest(&bytes, &secret).unwrap();
        assert_eq!(
            Catalog::verify(&bytes, &signature, &public).unwrap().plugins[0].versions[0].notes,
            "- Signed notes."
        );
        let forged = String::from_utf8(bytes).unwrap().replace("- Signed notes.", "- [Click](https://example.invalid)");
        assert!(Catalog::verify(forged.as_bytes(), &signature, &public).is_err(), "changed notes break the signature");
    }

    #[test]
    fn catalog_round_trip_and_lookup() {
        let mut catalog = Catalog { schema: CATALOG_SCHEMA, plugins: Vec::new() };
        let plugin = CatalogPlugin {
            id: "markdown".into(),
            name: "Markdown".into(),
            description: String::new(),
            extensions: vec!["md".into(), "markdown".into()],
            names: Vec::new(),
            hashbangs: Vec::new(),
            contents: Vec::new(),
            language_servers: Vec::new(),
            file_templates: FileTemplateDeclarations::default(),
            live_templates: LiveTemplateDeclarations::default(),
            server_plugins: Vec::new(),
            requires: Vec::new(),
            project: ProjectSupport::default(),
            database_drivers: Vec::new(),
            project_generators: Vec::new(),
            tests: Vec::new(),
            terminal_agents: Vec::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
            code_styles: Vec::new(),
            versions: Vec::new(),
        };
        let version = |v: &str| CatalogVersion {
            version: v.into(),
            date: "2026-10-16".into(),
            abi: 0,
            url: format!("artifacts/plugin-markdown/{v}/markdown.wasm"),
            sha256: "00".into(),
            size: 1,
            notes: format!("### Added\n\n- Version {v}."),
        };
        catalog.add(plugin.clone(), version("0.1.0"));
        catalog.add(plugin.clone(), version("0.10.0"));
        catalog.add(plugin, version("0.2.0"));
        assert_eq!(Catalog::parse(&catalog.to_bytes()).unwrap(), catalog);
        let found = catalog.for_file("README.MD").unwrap();
        assert_eq!(found.latest().unwrap().version, "0.10.0");
        assert_eq!(found.version("0.2.0").unwrap().notes, "### Added\n\n- Version 0.2.0.");
        assert!(found.version("9.9.9").is_none());
        assert!(catalog.for_file("main.go").is_none() && catalog.for_file("Makefile").is_none());
    }

    #[test]
    fn matches_extensions_and_names() {
        let extensions = vec!["env".to_owned()];
        let names =
            vec![".env".to_owned(), ".env.*".to_owned(), "Dockerfile".to_owned(), "docker-compose*.yml".to_owned()];
        for name in [".env", ".env.local", ".ENV.production", "prod.env", "dockerfile", "docker-compose.override.yml"] {
            assert!(supports(&extensions, &names, name), "{name}");
        }
        for name in ["env", "my.envy", "Dockerfiles", "compose.yml", "main.go"] {
            assert!(!supports(&extensions, &names, name), "{name}");
        }
    }

    #[test]
    fn installs_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let installed = Installed::new(dir.path().join("plugins"));
        let plugin = CatalogPlugin {
            id: "two".into(),
            name: "Two".into(),
            description: String::new(),
            extensions: extensions(),
            names: Vec::new(),
            hashbangs: Vec::new(),
            contents: Vec::new(),
            language_servers: Vec::new(),
            file_templates: FileTemplateDeclarations::default(),
            live_templates: LiveTemplateDeclarations::default(),
            server_plugins: Vec::new(),
            requires: Vec::new(),
            project: ProjectSupport::default(),
            database_drivers: Vec::new(),
            project_generators: Vec::new(),
            tests: Vec::new(),
            terminal_agents: Vec::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
            code_styles: Vec::new(),
            versions: Vec::new(),
        };
        let version = CatalogVersion {
            version: "1.0.0".into(),
            date: String::new(),
            abi: 0,
            url: String::new(),
            sha256: String::new(),
            size: 0,
            notes: "- First.".into(),
        };
        installed.install(&plugin, &version, &module(TWO_BYTES)).unwrap();
        assert_eq!(installed.list().len(), 1);
        assert_eq!(installed.list()[0].notes, "- First.", "the installed version keeps its notes");
        let (loaded, failed) = installed.load_all();
        assert_eq!((loaded.len(), failed.len()), (1, 0));
        assert!(installed.install(&plugin, &version, b"broken").is_err());
        let bad = CatalogPlugin { id: "../escape".into(), ..plugin };
        assert!(installed.install(&bad, &version, &module(TWO_BYTES)).is_err());
        assert!(installed.uninstall("../escape").is_err());
        installed.uninstall("two").unwrap();
        assert!(installed.list().is_empty());
        assert!(installed.uninstall("two").is_err(), "not installed any more");
    }

    #[test]
    fn a_driver_program_is_installed_beside_its_plugin_only_when_its_checksum_matches() {
        let dir = tempfile::tempdir().unwrap();
        let installed = Installed::new(dir.path());
        let bytes = b"#!/bin/sh\n";
        let sum = zen_update::sha256_hex(bytes);
        assert!(installed.install_driver_program("duck", "duckdb", "zen-duckdb", &"0".repeat(64), bytes).is_err());
        assert!(!installed.driver_program("duck", "duckdb", "zen-duckdb").exists());
        assert!(installed.install_driver_program("duck", "duckdb", "../x", &sum, bytes).is_err());
        let path = installed.install_driver_program("duck", "duckdb", "zen-duckdb", &sum, bytes).unwrap();
        assert_eq!(path, dir.path().join("duck/drivers/duckdb/zen-duckdb"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
        }
    }

    #[test]
    fn manifests_carry_language_servers() {
        let manifest: InstalledPlugin = serde_json::from_str(
            r#"{ "id": "yaml", "name": "YAML", "version": "0.1.0", "extensions": ["yaml", "yml"],
                 "languageServers": [{ "id": "yaml-language-server", "name": "YAML Language Server",
                   "languages": ["yaml"], "extensions": ["yaml", "yml"], "command": "yaml-language-server",
                   "args": ["--stdio"], "install": { "kind": "npm", "package": "yaml-language-server" } }] }"#,
        )
        .unwrap();
        assert_eq!(manifest.language_servers[0].args, ["--stdio"]);
        let dir = tempfile::tempdir().unwrap();
        let installed = Installed::new(dir.path());
        let plugin = CatalogPlugin {
            id: "two".into(),
            name: "Two".into(),
            description: String::new(),
            extensions: extensions(),
            names: Vec::new(),
            hashbangs: Vec::new(),
            contents: Vec::new(),
            language_servers: manifest.language_servers.clone(),
            file_templates: manifest.file_templates.clone(),
            live_templates: manifest.live_templates.clone(),
            server_plugins: manifest.server_plugins.clone(),
            requires: vec!["vue".into()],
            project: ProjectSupport::parse(r#"{ "markers": { "packages": ["nuxt"] } }"#).unwrap(),
            database_drivers: Vec::new(),
            project_generators: Vec::new(),
            tests: Vec::new(),
            terminal_agents: Vec::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
            code_styles: Vec::new(),
            versions: Vec::new(),
        };
        let version = CatalogVersion {
            version: "1.0.0".into(),
            date: String::new(),
            abi: 0,
            url: String::new(),
            sha256: String::new(),
            size: 0,
            notes: String::new(),
        };
        installed.install(&plugin, &version, &module(TWO_BYTES)).unwrap();
        assert_eq!(installed.list()[0].language_servers, manifest.language_servers);
        assert_eq!(
            (installed.list()[0].requires.clone(), installed.list()[0].project.clone()),
            (plugin.requires.clone(), plugin.project.clone())
        );
        let mut catalog = Catalog { schema: CATALOG_SCHEMA, plugins: Vec::new() };
        catalog.add(plugin, version);
        assert_eq!(Catalog::parse(&catalog.to_bytes()).unwrap(), catalog);
        let plain: InstalledPlugin =
            serde_json::from_str(r#"{ "id": "a", "name": "A", "version": "1", "extensions": [] }"#).unwrap();
        assert!(plain.language_servers.is_empty());
        assert!(!serde_json::to_string(&plain).unwrap().contains("languageServers"));
    }

    #[test]
    fn the_declared_language_servers_are_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut declared: Vec<(String, ServerDeclaration)> = Vec::new();
        for entry in std::fs::read_dir(root.join("plugins")).unwrap().flatten() {
            let Ok(bytes) = std::fs::read(entry.path().join("plugin.json")) else { continue };
            let manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let servers: Vec<ServerDeclaration> = match manifest.get("languageServers") {
                Some(list) => serde_json::from_value(list.clone())
                    .unwrap_or_else(|error| panic!("{}: {error}", entry.path().display())),
                None => Vec::new(),
            };
            let plugin = entry.file_name().to_string_lossy().into_owned();
            declared.extend(servers.into_iter().map(|server| (plugin.clone(), server)));
        }
        let go = std::fs::read_to_string(root.join("crates/lang/go/language-servers.json")).unwrap();
        declared
            .extend(ServerDeclaration::parse_list(&go).unwrap().into_iter().map(|server| ("go".to_owned(), server)));
        for (plugin, server) in &declared {
            for (other, same) in declared.iter().filter(|(other, same)| other != plugin && same.id == server.id) {
                assert_eq!(server, same, "{plugin} and {other} declare {} differently", server.id);
            }
        }
        let ids: Vec<&str> = declared.iter().map(|(_, server)| server.id.as_str()).collect();
        for id in ["gopls", "yaml-language-server", "vue-language-server", "typescript-language-server"] {
            assert!(ids.contains(&id), "{id} in {ids:?}");
        }
    }

    #[test]
    fn tool_rows_are_cut_to_one_line_and_rows_without_an_id_are_dropped() {
        let long = "x".repeat(MAX_TOOL_TEXT + 10);
        let answer = serde_json::json!([
            { "id": "c1", "columns": ["web\nrm -rf /", long], "state": "running" },
            { "id": " ", "columns": ["nobody"] },
            { "id": "c2" }
        ]);
        let rows = decode_tool_rows(answer.to_string().as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].columns[0], "web");
        assert_eq!(rows[0].columns[1].chars().count(), MAX_TOOL_TEXT);
        assert_eq!(rows[1].state, None);
        assert!(decode_tool_rows(b"").unwrap().is_empty());
        assert!(decode_tool_rows(b"{").is_err());
    }

    #[test]
    fn a_run_target_may_say_its_output_is_a_diff() {
        let answer = serde_json::json!([
            { "line": 0, "name": "Diff", "program": "kubectl", "args": ["diff"], "output": "diff" },
            { "line": 0, "name": "Apply", "program": "kubectl", "args": ["apply"] }
        ]);
        let offers = decode_run_targets(answer.to_string().as_bytes(), "x", &["kubectl".to_owned()]).unwrap();
        assert_eq!(offers.iter().map(|offer| offer.diff).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn a_run_target_may_say_its_output_is_a_document_of_a_plain_type() {
        let answer = serde_json::json!([
            { "line": 0, "name": "Preview", "program": "helm", "output": "document", "documentType": "YAML" },
            { "line": 0, "name": "Odd", "program": "helm", "output": "document", "documentType": "../x" },
            { "line": 0, "name": "Plain", "program": "helm" }
        ]);
        let offers = decode_run_targets(answer.to_string().as_bytes(), "x", &["helm".to_owned()]).unwrap();
        let documents: Vec<Option<&str>> = offers.iter().map(|offer| offer.document.as_deref()).collect();
        assert_eq!(documents, [Some("yaml"), Some("txt"), None]);
    }

    #[test]
    fn a_run_target_may_only_name_a_program_its_plug_in_declared() {
        let answer = serde_json::json!([
            { "line": 0, "name": "web", "program": "docker", "args": ["compose", "up"], "env": { "PATH": "/x", "COMPOSE_MENU": "false", "LD_PRELOAD": "e" } },
            { "line": 1, "name": "shell", "program": "sh", "args": ["-c", "rm -rf /"] },
            { "line": 1, "name": "path", "program": "/bin/docker" },
            { "line": 9, "name": "past the end", "program": "docker" },
            { "line": 1, "name": " ", "program": "docker" },
            { "line": 1, "name": "test", "program": "docker", "kind": "test", "id": "t" }
        ]);
        let runs = vec!["docker".to_owned()];
        let found = decode_run_targets(answer.to_string().as_bytes(), "a\nb\n", &runs).unwrap();
        let names: Vec<&str> = found.iter().map(|target| target.name.as_str()).collect();
        assert_eq!(names, ["web", "test"]);
        assert_eq!(found[0].env, [("COMPOSE_MENU".to_owned(), "false".to_owned())]);
        assert_eq!(found[0].id, "web");
        assert!(found[1].test);
        assert!(
            decode_run_targets(answer.to_string().as_bytes(), "a\nb\n", &[]).unwrap().is_empty(),
            "nothing declared"
        );
    }

    #[test]
    fn a_resource_path_stays_inside_the_module() {
        for good in ["kubernetes/all.json", "compose.json"] {
            assert!(resource_path(good), "{good}");
        }
        for bad in ["", "/etc/passwd", "a/../b", "./a", "a//b", "a\\b"] {
            assert!(!resource_path(bad), "{bad}");
        }
    }

    fn declared(plugin: &str, id: &str) -> ServerDeclaration {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("plugins").join(plugin).join("plugin.json")).unwrap())
                .unwrap();
        let servers: Vec<ServerDeclaration> = serde_json::from_value(manifest["languageServers"].clone()).unwrap();
        servers.into_iter().find(|server| server.id == id).unwrap()
    }

    #[test]
    fn the_json_server_hands_web_schemas_to_the_ide_and_taplo_reads_none() {
        let json = declared("json", "vscode-json-language-server");
        assert_eq!(
            json.initialization_options["handledSchemaProtocols"],
            serde_json::json!(["file"]),
            "http(s) come to the IDE"
        );
        assert!(
            json.schema_support.as_ref().is_some_and(|support| support.setting.is_empty()),
            "answered, nothing mapped"
        );
        let taplo = declared("toml", "taplo");
        assert_eq!(taplo.settings["evenBetterToml"]["schema"]["enabled"], false, "no catalog, no schema download");
    }

    fn holds(source: &serde_json::Value, kept: &serde_json::Value) -> bool {
        match (source, kept) {
            (serde_json::Value::Object(source), serde_json::Value::Object(kept)) => {
                source.iter().all(|(key, value)| kept.get(key).is_some_and(|other| holds(value, other)))
            }
            (serde_json::Value::Array(source), serde_json::Value::Array(kept)) => {
                source.len() == kept.len() && source.iter().zip(kept).all(|(value, other)| holds(value, other))
            }
            _ => source == kept,
        }
    }

    #[test]
    fn the_catalog_keeps_what_a_plug_in_declares_about_running_programs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        for entry in std::fs::read_dir(root.join("plugins")).unwrap().flatten() {
            let Ok(bytes) = std::fs::read(entry.path().join("plugin.json")) else { continue };
            let mut source: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let id = entry.file_name().to_string_lossy().into_owned();
            source["id"] = serde_json::json!(id);
            source["versions"] = serde_json::json!([]);
            let plugin: CatalogPlugin = serde_json::from_value(source.clone()).unwrap();
            let kept = serde_json::to_value(&plugin).unwrap();
            for key in ["runs", "personOnly", "schemas", "toolWindows"] {
                if let Some(declared) = source.get(key) {
                    assert!(holds(declared, &kept[key]), "plugins/{id}/plugin.json: `{key}` changes in the catalog");
                }
            }
        }
    }

    #[test]
    fn the_yaml_server_downloads_no_schemas() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("plugins/yaml/plugin.json")).unwrap()).unwrap();
        let servers: Vec<ServerDeclaration> = serde_json::from_value(manifest["languageServers"].clone()).unwrap();
        let yaml = servers.iter().find(|server| server.id == "yaml-language-server").unwrap();
        assert_eq!(yaml.settings["yaml"]["schemaStore"]["enable"], false, "no schema catalog from the network");
        assert_eq!(yaml.settings["yaml"]["validate"], true, "YAML syntax is still checked");
        let support = yaml.schema_support.as_ref().expect("the platform serves its schemas");
        assert_eq!(support.setting, "yaml.schemas");
        assert_eq!(support.register.as_deref(), Some("yaml/registerContentRequest"), "downloads come to the IDE");
    }

    #[test]
    fn the_typescript_server_installs_no_types_by_itself() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        for plugin in ["typescript", "javascript"] {
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("plugins").join(plugin).join("plugin.json")).unwrap())
                    .unwrap();
            let servers: Vec<ServerDeclaration> = serde_json::from_value(manifest["languageServers"].clone()).unwrap();
            let server = servers.iter().find(|server| server.id == "typescript-language-server").unwrap();
            assert_eq!(server.initialization_options["disableAutomaticTypingAcquisition"], true, "{plugin}");
        }
    }

    #[test]
    fn the_catalog_finds_plugins_for_a_project_and_their_requirements() {
        let plugin = |id: &str, requires: &[&str], project: &str| CatalogPlugin {
            id: id.into(),
            name: id.to_uppercase(),
            description: String::new(),
            extensions: Vec::new(),
            names: Vec::new(),
            hashbangs: Vec::new(),
            contents: Vec::new(),
            language_servers: Vec::new(),
            file_templates: FileTemplateDeclarations::default(),
            live_templates: LiveTemplateDeclarations::default(),
            server_plugins: Vec::new(),
            requires: requires.iter().map(|id| (*id).to_owned()).collect(),
            project: ProjectSupport::parse(project).unwrap(),
            database_drivers: Vec::new(),
            project_generators: Vec::new(),
            tests: Vec::new(),
            terminal_agents: Vec::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
            code_styles: Vec::new(),
            versions: Vec::new(),
        };
        let catalog = Catalog {
            schema: CATALOG_SCHEMA,
            plugins: vec![
                plugin(
                    "nuxt",
                    &["vue", "missing"],
                    r#"{ "markers": { "files": ["nuxt.config.ts"], "packages": ["nuxt"] } }"#,
                ),
                plugin("vue", &["typescript"], ""),
                plugin("typescript", &["vue"], ""),
                plugin("svelte", &[], r#"{ "markers": { "packages": ["svelte"] } }"#),
            ],
        };
        let ids = |plugins: Vec<&CatalogPlugin>| plugins.iter().map(|plugin| plugin.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(catalog.with_requirements("nuxt")), ["typescript", "vue", "nuxt"], "a cycle stops");
        assert_eq!(ids(catalog.with_requirements("svelte")), ["svelte"]);
        assert!(catalog.with_requirements("unknown").is_empty());
        let dir = tempfile::tempdir().unwrap();
        assert!(catalog.for_project(dir.path()).is_empty());
        std::fs::write(dir.path().join("nuxt.config.ts"), "").unwrap();
        assert_eq!(ids(catalog.for_project(dir.path())), ["nuxt"]);
        std::fs::write(dir.path().join("package.json"), r#"{ "dependencies": { "svelte": "5" } }"#).unwrap();
        assert_eq!(ids(catalog.for_project(dir.path())), ["nuxt", "svelte"]);
        let mut published = Catalog { schema: CATALOG_SCHEMA, plugins: Vec::new() };
        published.add(
            catalog.plugins[0].clone(),
            CatalogVersion {
                version: "0.1.0".into(),
                date: String::new(),
                abi: 0,
                url: String::new(),
                sha256: String::new(),
                size: 0,
                notes: String::new(),
            },
        );
        let read = Catalog::parse(&published.to_bytes()).unwrap();
        assert_eq!(
            (read.plugins[0].requires.clone(), read.plugins[0].project.clone()),
            (catalog.plugins[0].requires.clone(), catalog.plugins[0].project.clone())
        );
    }
}

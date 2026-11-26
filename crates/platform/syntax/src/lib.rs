use std::collections::BTreeMap;
use std::ops::Range;

pub mod incremental;
pub mod inject;

pub use incremental::{FoldRegion, FoldRegionKind, Highlighting, Incremental, LineLexer, Lines, TextChange};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_text::search::SearchContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HighlightKind {
    Keyword,
    String,
    Number,
    Comment,
    DocComment,
    Macro,
    Type,
    Function,
    Lifetime,
    Constant,
    Attribute,
    Punctuation,
    Heading,
    Strong,
    Emphasis,
    Link,
    Code,
    Property,
    GlobalVariable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: HighlightKind,
}

pub trait Highlighter: Send + Sync {
    fn handles(&self, file_name: &str) -> bool;

    fn specificity(&self, _file_name: &str) -> u8 {
        1
    }

    fn highlight(&self, text: &str) -> Vec<Span>;

    fn incremental(&self, _text: &str) -> Option<Box<dyn Incremental>> {
        None
    }

    fn angle_brackets(&self) -> bool {
        false
    }
}

pub fn in_string_or_comment(spans: &[Span], offset: usize) -> bool {
    let at = spans.partition_point(|span| span.range.end <= offset);
    spans.get(at).is_some_and(|span| {
        span.range.start <= offset
            && matches!(span.kind, HighlightKind::String | HighlightKind::Comment | HighlightKind::DocComment)
    })
}

impl HighlightKind {
    pub fn is_comment(self) -> bool {
        matches!(self, HighlightKind::Comment | HighlightKind::DocComment)
    }

    pub fn is_string(self) -> bool {
        self == HighlightKind::String
    }
}

pub fn kind_at(spans: &[Span], offset: usize) -> Option<HighlightKind> {
    let at = spans.partition_point(|span| span.range.end <= offset);
    spans.get(at).filter(|span| span.range.start <= offset).map(|span| span.kind)
}

pub fn keep_in_context(
    text: &str,
    ranges: Vec<Range<usize>>,
    context: SearchContext,
    kind_at: impl Fn(usize) -> Option<HighlightKind>,
) -> Vec<Range<usize>> {
    if context == SearchContext::Anywhere {
        return ranges;
    }
    let mut chars = text.char_indices().map(|(byte, _)| byte).enumerate().peekable();
    ranges
        .into_iter()
        .filter(|range| {
            let mut byte = text.len();
            while let Some(&(index, at)) = chars.peek() {
                if index >= range.start {
                    if index == range.start {
                        byte = at;
                    }
                    break;
                }
                chars.next();
            }
            let kind = kind_at(byte);
            context.allows(kind.is_some_and(HighlightKind::is_comment), kind.is_some_and(HighlightKind::is_string))
        })
        .collect()
}

/// What a named element of source code is, in an outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    /// A struct type.
    Struct,
    /// An interface or trait.
    Interface,
    /// Any other named type.
    Type,
    /// A function.
    Function,
    /// A method.
    Method,
    /// A field of a struct.
    Field,
    /// A constant.
    Constant,
    /// A variable.
    Variable,
    /// A package or namespace.
    Package,
    /// A module, e.g. the module of a `go.mod` file.
    Module,
    /// A class.
    Class,
    /// A markup element, e.g. an HTML tag.
    Element,
    /// A style sheet rule, e.g. a CSS selector with its block.
    Rule,
    /// A property, e.g. a CSS declaration or an object's property.
    Property,
    /// A key of a data file, e.g. a JSON or YAML key.
    Key,
    /// Anything else.
    Other,
}

/// An element of a file's outline, with the elements inside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutlineItem {
    /// The element's name.
    pub name: String,
    /// What is shown after the name, e.g. a signature or a version.
    pub detail: Option<String>,
    /// What the element is.
    pub kind: SymbolKind,
    /// The first character of the whole element (a character offset).
    pub start: usize,
    /// The character after the element's last.
    pub end: usize,
    /// The first character of the element's name, where the caret goes.
    pub selection_start: usize,
    /// The elements inside it, in order.
    pub children: Vec<OutlineItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuppressedLines {
    pub lines: std::ops::RangeInclusive<u32>,
    pub ids: Vec<String>,
}

pub trait Outliner: Send + Sync {
    fn handles(&self, file_name: &str) -> bool;

    fn outline(&self, text: &str) -> Vec<OutlineItem>;

    fn reads_suppressions(&self) -> bool {
        false
    }

    fn suppressions(&self, _text: &str) -> Vec<SuppressedLines> {
        Vec::new()
    }
}

pub trait Arranger: Send + Sync {
    fn language(&self) -> &str;

    fn handles(&self, file_name: &str) -> bool;

    fn arrange(&self, text: &str, order: &[String]) -> Option<String>;

    fn before_reformat(&self, _text: &str, _option: &dyn Fn(&str) -> bool) -> Option<String> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineFold {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub kind: String,
    pub insert: String,
    pub caret: Option<usize>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Suggestions {
    pub from: usize,
    pub items: Vec<Suggestion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    pub range: Range<usize>,
    pub text: String,
    pub caret: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    Folding,
    Outline,
    Completion,
    Expansion,
    Colors,
    Typing,
    Problems,
    Jvm,
    Inlays,
    Resources,
    RunTargets,
    Conversion,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolWindowDeclaration {
    pub id: String,
    pub title: BTreeMap<String, String>,
    pub icon: String,
    pub side: String,
    pub choices: Vec<ToolChoice>,
    pub sections: Vec<ToolSection>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub window_values: Vec<WindowValue>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowValue {
    pub plugin: String,
    pub window: String,
    pub choice: String,
}

impl WindowValue {
    pub fn name(&self) -> String {
        format!("{}.{}", self.window, self.choice)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexStateDeclaration {
    pub columns: Vec<String>,
    pub column_titles: BTreeMap<String, BTreeMap<String, String>>,
    pub lists: Vec<ToolCommand>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolChoice {
    pub id: String,
    pub title: BTreeMap<String, String>,
    pub columns: Vec<String>,
    pub list: ToolCommand,
    pub initial_from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ToolSource>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolSource {
    pub variable: String,
    pub default: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolSection {
    pub id: String,
    pub title: BTreeMap<String, String>,
    pub columns: Vec<String>,
    pub column_titles: BTreeMap<String, BTreeMap<String, String>>,
    pub list: ToolCommand,
    pub actions: Vec<ToolAction>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rows: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<IndexStateDeclaration>,
}

pub const INDEX_ROWS: &str = "index";

pub const STATE_SECTION_PREFIX: &str = "state:";

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolActionKind {
    Stream,
    Terminal,
    #[default]
    Change,
    Destroy,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolAction {
    pub id: String,
    pub title: BTreeMap<String, String>,
    pub program: String,
    pub args: Vec<String>,
    pub kind: ToolActionKind,
    pub rows: bool,
    pub confirm: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<ToolInput>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolInputKind {
    #[default]
    Count,
    Port,
    Forward,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolInput {
    pub id: String,
    pub title: BTreeMap<String, String>,
    pub kind: ToolInputKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<u32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default_from: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolRow {
    pub id: String,
    pub columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

pub fn localized(texts: &BTreeMap<String, String>, language: &str) -> String {
    texts.get(language).or_else(|| texts.get("en")).or_else(|| texts.values().next()).cloned().unwrap_or_default()
}

pub fn kept_variable(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    !name.is_empty()
        && upper != "PATH"
        && !upper.starts_with("ZEN_")
        && !upper.starts_with("DYLD_")
        && !upper.starts_with("LD_")
        && !name.contains(['=', '\0'])
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexDeclaration {
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectIndex {
    pub kinds: Vec<String>,
    pub nodes: Vec<IndexNode>,
    pub edges: Vec<IndexEdge>,
    pub references: Vec<IndexReference>,
    pub problems: Vec<IndexProblem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub completions: Vec<IndexCompletion>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexNode {
    pub id: String,
    pub kind: String,
    pub group: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub namespace: String,
    pub path: String,
    pub start: usize,
    pub end: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(skip_serializing_if = "is_true")]
    pub in_graph: bool,
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Default for IndexNode {
    fn default() -> Self {
        IndexNode {
            id: String::new(),
            kind: String::new(),
            group: String::new(),
            name: String::new(),
            namespace: String::new(),
            path: String::new(),
            start: 0,
            end: 0,
            detail: String::new(),
            in_graph: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexReference {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub target: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexProblem {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub severity: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexCompletion {
    pub files: Vec<String>,
    pub after: String,
    pub items: Vec<IndexCompletionItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexCompletionItem {
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

pub const INDEX_COMPLETIONS: usize = 1_000;
pub const INDEX_COMPLETION_ITEMS: usize = 2_000;
pub const INDEX_COMPLETION_TEXT: usize = 200;
pub const INDEX_COMPLETION_AFTER: usize = 64;

pub const INDEX_NODES: usize = 20_000;
pub const INDEX_EDGES: usize = 50_000;
pub const INDEX_REFERENCES: usize = 50_000;
pub const INDEX_PROBLEMS: usize = 5_000;

impl ProjectIndex {
    pub fn checked(mut self, files: &[(&str, &str)]) -> ProjectIndex {
        let within = |path: &str, start: usize, end: usize| {
            files.iter().any(|(file, text)| {
                *file == path
                    && start <= end
                    && end <= text.len()
                    && text.is_char_boundary(start)
                    && text.is_char_boundary(end)
            })
        };
        self.nodes.retain(|node| {
            !node.id.is_empty()
                && (within(&node.path, node.start, node.end)
                    || (node.start == 0 && node.end == 0 && !node.path.is_empty()))
        });
        self.nodes.truncate(INDEX_NODES);
        let ids: std::collections::BTreeSet<String> = self.nodes.iter().map(|node| node.id.clone()).collect();
        self.edges.retain(|edge| ids.contains(&edge.from) && !edge.to.is_empty());
        self.edges.truncate(INDEX_EDGES);
        self.references.retain(|reference| {
            within(&reference.path, reference.start, reference.end) && !reference.target.is_empty()
        });
        self.references.truncate(INDEX_REFERENCES);
        self.problems.retain(|problem| {
            within(&problem.path, problem.start, problem.end)
                && matches!(problem.severity.as_str(), "error" | "warning")
                && !problem.message.is_empty()
        });
        self.problems.truncate(INDEX_PROBLEMS);
        let line = |text: &str, most: usize| -> String {
            text.lines().next().unwrap_or_default().chars().take(most).collect()
        };
        self.completions.retain_mut(|completion| {
            completion.files.retain(|path| files.iter().any(|(file, _)| file == path));
            completion.after = line(&completion.after, INDEX_COMPLETION_AFTER);
            completion.items.retain(|item| !item.label.trim().is_empty() && !item.label.contains('\n'));
            completion.items.truncate(INDEX_COMPLETION_ITEMS);
            for item in &mut completion.items {
                item.label = line(&item.label, INDEX_COMPLETION_TEXT);
                item.detail = line(&item.detail, INDEX_COMPLETION_TEXT);
            }
            !completion.files.is_empty() && !completion.after.is_empty() && !completion.items.is_empty()
        });
        self.completions.truncate(INDEX_COMPLETIONS);
        self
    }
}

pub fn index_matches(declaration: &IndexDeclaration, file_name: &str) -> bool {
    declaration.files.iter().any(|pattern| match pattern.strip_prefix('*') {
        Some(suffix) => file_name.ends_with(suffix),
        None => file_name == pattern,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PersonOnly {
    pub program: String,
    #[serde(default)]
    pub subcommands: Vec<String>,
}

pub fn is_person_only(declared: &[PersonOnly], program: &str, args: &[String]) -> bool {
    declared.iter().any(|rule| {
        rule.program == program
            && args
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .any(|arg| rule.subcommands.iter().any(|subcommand| subcommand == arg))
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOffer {
    pub line: u32,
    pub name: String,
    pub id: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub test: bool,
    pub diff: bool,
    pub document: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SchemaDeclaration {
    pub id: String,
    pub name: String,
    pub server: String,
    pub entry: String,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JvmFile {
    pub package: String,
    pub file_class: String,
    pub classes: Vec<JvmClass>,
    pub mains: Vec<JvmMain>,
    pub tests: Vec<JvmTestClass>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmClass {
    pub name: String,
    pub first_line: u32,
    pub last_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmMain {
    pub class: String,
    pub class_line: u32,
    pub line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmTestClass {
    pub name: String,
    pub line: u32,
    pub testng: bool,
    pub methods: Vec<(String, u32)>,
}

impl JvmFile {
    pub fn named_after(mut self, file_stem: &str) -> Self {
        if self.file_class.is_empty() {
            let mut name: String = file_stem
                .chars()
                .enumerate()
                .map(|(at, c)| match c {
                    c if at == 0 && c.is_alphabetic() => c.to_uppercase().next().unwrap_or(c),
                    c if c.is_alphanumeric() || c == '_' => c,
                    _ => '_',
                })
                .collect();
            name.push_str("Kt");
            self.file_class = if self.package.is_empty() { name } else { format!("{}.{name}", self.package) };
        }
        for main in self.mains.iter_mut().filter(|main| main.class.is_empty()) {
            main.class.clone_from(&self.file_class);
        }
        self
    }

    pub fn class_at(&self, line: u32) -> &str {
        self.classes
            .iter()
            .find(|class| class.first_line <= line && line <= class.last_line)
            .map_or(self.file_class.as_str(), |class| class.name.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextProblem {
    pub range: Range<usize>,
    pub error: bool,
    pub message: String,
}

/// Where a plug-in's files are named from string literals of other files: a key given to a call such as
/// `bundle.getString("key")`, or a bundle given to `ResourceBundle.getBundle("messages")`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyReferences {
    /// The name patterns of the files whose string literals name keys, e.g. `*.java`.
    pub files: Vec<String>,
    /// The methods whose first argument, a string literal, is a key, e.g. `getString`.
    pub keys: Vec<String>,
    /// The methods whose first argument, a string literal, names a bundle, e.g. `getBundle`.
    pub bundles: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversion {
    pub text: String,
    pub problems: Vec<TextProblem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextInlay {
    pub offset: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorSpan {
    pub range: Range<usize>,
    pub rgba: [u8; 4],
}

pub trait TextFeatures: Outliner {
    fn specificity(&self, _file_name: &str) -> u8 {
        1
    }

    fn supports(&self, feature: Feature) -> bool;

    fn folds(&self, text: &str) -> Vec<LineFold>;

    fn complete(&self, text: &str, caret: usize) -> Option<Suggestions>;

    fn expand(&self, text: &str, caret: usize) -> Option<Expansion>;

    fn colors(&self, _text: &str) -> Vec<ColorSpan> {
        Vec::new()
    }

    fn reacts_to(&self, _typed: char) -> bool {
        false
    }

    fn on_type(&self, _text: &str, _caret: usize, _typed: char) -> Option<Expansion> {
        None
    }

    fn problems(&self, _text: &str) -> Vec<TextProblem> {
        Vec::new()
    }

    fn in_dialect(&self, _dialect: &str) -> Option<std::sync::Arc<dyn TextFeatures>> {
        None
    }

    fn jvm(&self, _text: &str) -> Option<JvmFile> {
        None
    }

    fn inlays(&self, _text: &str) -> Vec<TextInlay> {
        Vec::new()
    }

    fn run_targets(&self, _path: &str, _text: &str) -> Vec<RunOffer> {
        Vec::new()
    }

    fn plugin_id(&self) -> Option<&str> {
        None
    }

    fn schemas(&self) -> Vec<SchemaDeclaration> {
        Vec::new()
    }

    fn resource(&self, _path: &str) -> Option<Vec<u8>> {
        None
    }

    fn runs(&self) -> Vec<String> {
        Vec::new()
    }

    fn person_only(&self) -> Vec<PersonOnly> {
        Vec::new()
    }

    fn target_files(&self) -> Vec<String> {
        Vec::new()
    }

    fn index_declaration(&self) -> Option<IndexDeclaration> {
        None
    }

    fn index(&self, _input: &[u8]) -> Option<ProjectIndex> {
        None
    }

    fn tool_windows(&self) -> Vec<ToolWindowDeclaration> {
        Vec::new()
    }

    fn tool_rows(&self, _section: &str, _output: &str) -> Option<Vec<ToolRow>> {
        None
    }

    fn key_references(&self) -> Vec<KeyReferences> {
        Vec::new()
    }

    fn converts_from(&self) -> Vec<String> {
        Vec::new()
    }

    fn convert(&self, _from: &str, _text: &str) -> Option<Conversion> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentSyntax {
    pub line: Option<&'static str>,
    pub block: Option<(&'static str, &'static str)>,
}

pub fn comment_syntax(file_name: &str) -> Option<CommentSyntax> {
    let lower = file_name.to_ascii_lowercase();
    let extension = lower.rsplit_once('.').map_or("", |(_, extension)| extension);
    let c_like = CommentSyntax { line: Some("//"), block: Some(("/*", "*/")) };
    let hash = CommentSyntax { line: Some("#"), block: None };
    Some(match extension {
        "go" | "rs" | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "java" | "kt" | "kts" | "scala" | "swift" | "js"
        | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "php" | "dart" | "proto" | "zig" | "jsonc" | "json5" => c_like,
        "css" | "scss" | "less" => {
            CommentSyntax { line: if extension == "css" { None } else { Some("//") }, block: Some(("/*", "*/")) }
        }
        "py" | "sh" | "bash" | "zsh" | "ksh" | "rb" | "pl" | "yml" | "yaml" | "toml" | "env" | "r" | "tf" | "cmake"
        | "dockerfile" | "containerfile" | "mk" => hash,
        "sql" | "lua" | "hs" => CommentSyntax { line: Some("--"), block: None },
        "html" | "htm" | "xml" | "svg" | "md" | "markdown" | "vue" => {
            CommentSyntax { line: None, block: Some(("<!--", "-->")) }
        }
        _ if lower == "dockerfile"
            || lower == "containerfile"
            || lower == "makefile"
            || lower.starts_with(".env")
            || (lower.starts_with('.') && lower.ends_with("ignore"))
            || lower.ends_with("rc") && lower.starts_with('.') =>
        {
            hash
        }
        _ if lower == "go.mod" || lower == "go.work" => c_like,
        _ => return None,
    })
}

#[cfg(test)]
mod outline_tests {
    use super::*;

    #[test]
    fn outline_items_survive_json() {
        let item = OutlineItem {
            name: "Server".to_owned(),
            detail: Some("struct{...}".to_owned()),
            kind: SymbolKind::Struct,
            start: 10,
            end: 40,
            selection_start: 15,
            children: vec![OutlineItem {
                name: "addr".to_owned(),
                detail: None,
                kind: SymbolKind::Field,
                start: 20,
                end: 31,
                selection_start: 20,
                children: Vec::new(),
            }],
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["kind"], "struct");
        assert_eq!(json["selectionStart"], 15);
        assert_eq!(json["children"][0]["kind"], "field");
        assert_eq!(serde_json::from_value::<OutlineItem>(json).unwrap(), item);
    }

    #[test]
    fn symbol_kinds_are_lowercase_words() {
        let kinds = [
            (SymbolKind::Interface, "interface"),
            (SymbolKind::Type, "type"),
            (SymbolKind::Function, "function"),
            (SymbolKind::Method, "method"),
            (SymbolKind::Constant, "constant"),
            (SymbolKind::Variable, "variable"),
            (SymbolKind::Package, "package"),
            (SymbolKind::Module, "module"),
            (SymbolKind::Class, "class"),
            (SymbolKind::Element, "element"),
            (SymbolKind::Rule, "rule"),
            (SymbolKind::Property, "property"),
            (SymbolKind::Key, "key"),
            (SymbolKind::Other, "other"),
        ];
        for (kind, word) in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap(), word);
        }
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    #[test]
    fn an_index_node_is_in_the_graph_unless_it_says_otherwise() {
        let shown: IndexNode = serde_json::from_str(r#"{"id": "a"}"#).unwrap();
        assert!(shown.in_graph);
        assert!(serde_json::to_value(&shown).unwrap().get("inGraph").is_none());
        let hidden: IndexNode = serde_json::from_str(r#"{"id": "b", "inGraph": false}"#).unwrap();
        assert!(!hidden.in_graph);
        assert_eq!(serde_json::to_value(&hidden).unwrap()["inGraph"], false);
    }

    #[test]
    fn index_completions_keep_known_files_and_single_line_labels_within_limits() {
        let item = |label: &str| IndexCompletionItem { label: label.to_owned(), detail: "x\ny".to_owned() };
        let mut many: Vec<IndexCompletionItem> =
            (0..INDEX_COMPLETION_ITEMS + 5).map(|n| item(&format!("k{n}"))).collect();
        many.push(item("two\nlines"));
        let index = ProjectIndex {
            completions: vec![
                IndexCompletion {
                    files: vec!["templates/a.yaml".to_owned(), "elsewhere.yaml".to_owned()],
                    after: ".Values.".to_owned(),
                    items: many,
                },
                IndexCompletion {
                    files: vec!["elsewhere.yaml".to_owned()],
                    after: ".V".to_owned(),
                    items: vec![item("a")],
                },
                IndexCompletion {
                    files: vec!["templates/a.yaml".to_owned()],
                    after: String::new(),
                    items: vec![item("a")],
                },
            ],
            ..ProjectIndex::default()
        }
        .checked(&[("templates/a.yaml", "{{ .Values.k1 }}")]);
        assert_eq!(index.completions.len(), 1);
        let kept = &index.completions[0];
        assert_eq!(kept.files, ["templates/a.yaml"]);
        assert_eq!(kept.items.len(), INDEX_COMPLETION_ITEMS);
        assert_eq!(kept.items[0].detail, "x");
        assert!(kept.items.iter().all(|item| !item.label.contains('\n')));
    }

    #[test]
    fn an_index_keeps_only_what_points_into_the_files_it_was_given() {
        let text = "kind: Kustomization\nmetadata:\n  name: apps\n";
        let node = |id: &str, path: &str, start: usize, end: usize| IndexNode {
            id: id.to_owned(),
            path: path.to_owned(),
            start,
            end,
            ..IndexNode::default()
        };
        let index = ProjectIndex {
            nodes: vec![
                node("a", "apps.yaml", 0, text.len()),
                node("b", "apps.yaml", 0, text.len() + 1),
                node("c", "other.yaml", 0, 4),
                node("path:apps/prod", "apps/prod", 0, 0),
                node("", "apps.yaml", 0, 1),
            ],
            edges: vec![
                IndexEdge { from: "a".to_owned(), to: "path:apps/prod".to_owned(), kind: "path".to_owned() },
                IndexEdge { from: "b".to_owned(), to: "a".to_owned(), kind: "source".to_owned() },
            ],
            references: vec![
                IndexReference { path: "apps.yaml".to_owned(), start: 6, end: 19, target: "a".to_owned() },
                IndexReference { path: "apps.yaml".to_owned(), start: 6, end: 999, target: "a".to_owned() },
            ],
            problems: vec![
                IndexProblem {
                    path: "apps.yaml".to_owned(),
                    start: 0,
                    end: 4,
                    severity: "error".to_owned(),
                    message: "x".to_owned(),
                },
                IndexProblem {
                    path: "apps.yaml".to_owned(),
                    start: 0,
                    end: 4,
                    severity: "fatal".to_owned(),
                    message: "x".to_owned(),
                },
            ],
            ..ProjectIndex::default()
        }
        .checked(&[("apps.yaml", text)]);
        let ids: Vec<&str> = index.nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(ids, ["a", "path:apps/prod"], "a folder node has no span; out-of-range and nameless ones go");
        assert_eq!(index.edges.len(), 1, "an edge from a dropped node goes");
        assert_eq!(index.references.len(), 1);
        assert_eq!(index.problems.len(), 1, "only errors and warnings");
        let declaration = IndexDeclaration { files: vec!["*.yaml".to_owned(), "Chart.lock".to_owned()] };
        assert!(index_matches(&declaration, "apps.yaml") && index_matches(&declaration, "Chart.lock"));
        assert!(!index_matches(&declaration, "apps.yml") && !index_matches(&declaration, "yaml"));
    }
}

#[cfg(test)]
mod person_only_tests {
    use super::*;

    #[test]
    fn a_declared_subcommand_anywhere_among_the_words_is_the_persons() {
        let rules =
            [PersonOnly { program: "terraform".to_owned(), subcommands: vec!["apply".to_owned(), "state".to_owned()] }];
        let words = |line: &str| line.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert!(is_person_only(&rules, "terraform", &words("apply")));
        assert!(is_person_only(&rules, "terraform", &words("-chdir=infra apply -auto-approve")));
        assert!(is_person_only(&rules, "terraform", &words("plan -out=p -- apply")), "however it is hidden");
        assert!(is_person_only(&rules, "terraform", &words("state rm aws_instance.web")));
        assert!(!is_person_only(&rules, "terraform", &words("plan -out=apply")), "a flag's value is no word");
        assert!(!is_person_only(&rules, "terraform", &words("validate")));
        assert!(!is_person_only(&rules, "docker", &words("apply")), "another program's words are its own");
        let json: PersonOnly = serde_json::from_str(r#"{"program":"terraform","subcommands":["apply"]}"#).unwrap();
        assert_eq!(json.subcommands, ["apply"]);
    }
}

#[cfg(test)]
mod bracket_tests {
    use super::*;

    #[test]
    fn strings_and_comments_hide_brackets() {
        let span = |range: Range<usize>, kind| Span { range, kind };
        let spans = [
            span(0..2, HighlightKind::Keyword),
            span(3..6, HighlightKind::String),
            span(8..12, HighlightKind::Comment),
            span(12..14, HighlightKind::DocComment),
        ];
        let hidden: Vec<usize> = (0..16).filter(|&offset| in_string_or_comment(&spans, offset)).collect();
        assert_eq!(hidden, [3, 4, 5, 8, 9, 10, 11, 12, 13]);
        assert!(!in_string_or_comment(&[], 0));
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn a_match_counts_by_the_token_its_start_lies_in() {
        let text = "é := \"total\" // total\ntotal";
        let spans =
            [Span { range: 6..13, kind: HighlightKind::String }, Span { range: 14..22, kind: HighlightKind::Comment }];
        let ranges = vec![6..11, 16..21, 22..27];
        let keep = |context| keep_in_context(text, ranges.clone(), context, |byte| kind_at(&spans, byte));
        let starts = |context| keep(context).into_iter().map(|range| range.start).collect::<Vec<_>>();
        assert_eq!(keep(SearchContext::Anywhere), ranges);
        assert_eq!(starts(SearchContext::InComments), [16]);
        assert_eq!(starts(SearchContext::InStringLiterals), [6]);
        assert_eq!(starts(SearchContext::ExceptComments), [6, 22]);
        assert_eq!(starts(SearchContext::ExceptStringLiterals), [16, 22]);
        assert_eq!(starts(SearchContext::ExceptCommentsAndStringLiterals), [22]);
        let before = std::iter::once(4..9).collect();
        let across = keep_in_context(text, before, SearchContext::InStringLiterals, |byte| kind_at(&spans, byte));
        assert!(across.is_empty(), "a match that starts before the string does not count");
    }

    #[test]
    fn doc_comments_are_comments() {
        assert!(HighlightKind::DocComment.is_comment());
        assert!(!HighlightKind::String.is_comment());
        assert!(HighlightKind::String.is_string());
        assert_eq!(kind_at(&[], 0), None);
    }
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    #[test]
    fn comment_syntax_by_file_name() {
        assert_eq!(comment_syntax("main.go").and_then(|c| c.line), Some("//"));
        assert_eq!(comment_syntax("main.go").and_then(|c| c.block), Some(("/*", "*/")));
        assert_eq!(comment_syntax("ci.YML").and_then(|c| c.line), Some("#"));
        assert_eq!(comment_syntax("Dockerfile").and_then(|c| c.line), Some("#"));
        assert_eq!(comment_syntax(".gitignore").and_then(|c| c.line), Some("#"));
        assert_eq!(comment_syntax("query.sql").and_then(|c| c.line), Some("--"));
        assert_eq!(comment_syntax("go.mod").and_then(|c| c.line), Some("//"));
        assert_eq!(comment_syntax("README.md").and_then(|c| c.line), None);
        assert_eq!(comment_syntax("data.json"), None);
    }
}

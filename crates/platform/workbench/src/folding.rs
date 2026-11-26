use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::folding::{self, FoldOption, FoldingGroups, FoldingOptions};
use zen_syntax::Feature;

use crate::{Document, Workbench};

pub const FOLD_IMPORTS: &str = folding::IMPORTS;

pub const SHOW_ARROWS: &str = "editor.folding.showArrows";

pub const ARROWS: &str = "editor.folding.arrows";

pub const BOTTOM_ARROWS: &str = "editor.folding.bottomArrows";

pub const GENERAL: FoldingOptions = FoldingOptions {
    title: "settings-folding-general",
    options: &[
        FoldOption { key: folding::FILE_HEADER, label: "settings-folding-file-header", default: false },
        FoldOption { key: folding::IMPORTS, label: "settings-folding-imports", default: true },
        FoldOption { key: folding::DOC_COMMENTS, label: "settings-folding-doc-comments", default: false },
        FoldOption { key: folding::METHOD_BODIES, label: "settings-folding-method-bodies", default: false },
        FoldOption { key: folding::CUSTOM_REGIONS, label: "settings-folding-custom-regions", default: false },
    ],
};

/// A fold as listed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoldInfo {
    /// The first character it hides.
    pub start: usize,
    /// The character after the last it hides.
    pub end: usize,
    /// The line it starts on, zero-based.
    pub start_line: usize,
    /// The line it ends on, zero-based.
    pub end_line: usize,
    /// `block`, `imports`, `comment`, `region` (a custom region) or `manual` (made with Fold Selection).
    pub kind: String,
    /// Whether it is folded.
    pub folded: bool,
    /// What shows in its place when folded, for a custom region with a name: the name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// The line that sticks over the editor while the view is inside it, for a block that sticks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sticky_line: Option<usize>,
    /// The setting of the "Fold by default" option that folds it when a file opens, e.g. `editor.folding.imports`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<&'static str>,
}

fn kind_name(kind: zen_editor::FoldKind) -> &'static str {
    match kind {
        zen_editor::FoldKind::Block => "block",
        zen_editor::FoldKind::Imports => "imports",
        zen_editor::FoldKind::Comment => "comment",
        zen_editor::FoldKind::Region => "region",
        zen_editor::FoldKind::Manual => "manual",
    }
}

/// A folded region as remembered: its first and last line, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoldLines {
    /// The line it starts on.
    pub start_line: usize,
    /// The line it ends on.
    pub end_line: usize,
}

/// A manual region (Fold Selection) as remembered: where it starts and ends, from 0, columns in characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManualRegion {
    /// The line it starts on.
    pub start_line: usize,
    /// The column it starts at.
    pub start_column: usize,
    /// The line it ends on.
    pub end_line: usize,
    /// The column it ends at.
    pub end_column: usize,
    /// Whether it is folded.
    pub folded: bool,
    /// The [`ManualRegion::hash`] of the text it holds; absent in what was remembered before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_hash: Option<u32>,
}

impl ManualRegion {
    pub fn range_in(&self, text: &zen_text::Text) -> Option<std::ops::Range<usize>> {
        let offset = |line: usize, column: usize| {
            let length = text.line(line).ok()?.trim_end_matches(['\n', '\r']).chars().count();
            (column <= length).then(|| text.point_to_offset(zen_text::Point::new(line, column)).ok()).flatten()
        };
        let range = offset(self.start_line, self.start_column)?..offset(self.end_line, self.end_column)?;
        let same = |hash: u32| text.slice(range.clone()).is_ok_and(|held| Self::hash(&held) == hash);
        (range.start < range.end && self.text_hash.is_none_or(same)).then_some(range)
    }

    pub fn hash(text: &str) -> u32 {
        text.bytes().fold(0x811c_9dc5, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
    }
}

/// The folded regions of one file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileFolds {
    /// The file.
    pub path: PathBuf,
    /// Its folded regions but the manual ones; none when everything is unfolded (its imports too).
    pub folds: Vec<FoldLines>,
    /// Its manual regions (Fold Selection), folded or not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual: Vec<ManualRegion>,
}

fn region_comments(document: &Document) -> zen_editor::RegionComments<'static> {
    let syntax = zen_syntax::comment_syntax(&document.name());
    zen_editor::RegionComments {
        line: syntax.and_then(|syntax| syntax.line),
        block: syntax.and_then(|syntax| syntax.block),
    }
}

pub fn custom_regions(session: &Session, document: &Document) -> Vec<zen_editor::CustomRegion> {
    let (text, comments) = (document.text(), region_comments(document));
    let highlighting = session
        .state::<crate::brackets::Highlighters>()
        .and_then(|highlighters| highlighters.highlighting_of(document));
    let Some(highlighting) = highlighting else {
        return zen_editor::custom_regions(text, comments);
    };
    let highlighting = highlighting.lock().unwrap_or_else(PoisonError::into_inner);
    zen_editor::custom_regions_where(text, comments, |line, at| {
        let carried = line > 0 && highlighting.in_string_or_comment(text, line - 1);
        let inside = highlighting.spans(text, at..at + 1).into_iter().any(|span| {
            span.range.contains(&at) && (span.kind.is_string() || (span.kind.is_comment() && span.range.start < at))
        });
        !carried && !inside
    })
}

fn read_folds(session: &Session, document: &Document) -> Vec<zen_editor::Fold> {
    zen_editor::with_custom_regions(read_code_folds(session, document), &custom_regions(session, document))
}

fn read_code_folds(session: &Session, document: &Document) -> Vec<zen_editor::Fold> {
    if let Some(plugin) = crate::plugins::plugin_for(session, document.path(), Feature::Folding) {
        return crate::plugins::line_folds(plugin.as_ref(), document);
    }
    let lexed = session
        .state::<crate::brackets::Highlighters>()
        .and_then(|highlighters| highlighters.highlighting_of(document))
        .and_then(|highlighting| highlighting.lock().unwrap_or_else(PoisonError::into_inner).folds(document.text()));
    match lexed {
        Some(regions) => in_characters(document.text(), regions),
        None => zen_editor::folds(document.text()),
    }
}

fn in_characters(text: &str, regions: Vec<zen_syntax::FoldRegion>) -> Vec<zen_editor::Fold> {
    let kind = |kind| match kind {
        zen_syntax::FoldRegionKind::Block => zen_editor::FoldKind::Block,
        zen_syntax::FoldRegionKind::Imports => zen_editor::FoldKind::Imports,
        zen_syntax::FoldRegionKind::Comment => zen_editor::FoldKind::Comment,
    };
    let fold = |region: zen_syntax::FoldRegion, range| zen_editor::Fold {
        range,
        kind: kind(region.kind),
        placeholder: region.placeholder,
        option: region.option,
    };
    if text.is_ascii() {
        return regions.into_iter().map(|region| fold(region.clone(), region.range)).collect();
    }
    let mut offsets: Vec<usize> = regions.iter().flat_map(|region| [region.range.start, region.range.end]).collect();
    offsets.sort_unstable();
    offsets.dedup();
    let mut chars = Vec::with_capacity(offsets.len());
    let mut counted = (0, 0);
    for offset in &offsets {
        counted.1 += text[counted.0..*offset].chars().count();
        counted.0 = *offset;
        chars.push(counted.1);
    }
    let char_of = |byte: usize| offsets.binary_search(&byte).map_or(0, |at| chars[at]);
    regions
        .into_iter()
        .map(|region| {
            let range = char_of(region.range.start)..char_of(region.range.end);
            fold(region, range)
        })
        .collect()
}

pub(crate) fn fold_on_open(session: &Session, document: &mut Document) {
    if document.image().is_some() {
        return;
    }
    let foldable = read_folds(session, document);
    let remembered =
        session.state::<Workbench>().and_then(|workbench| workbench.remembered_folds(document.path())).cloned();
    match remembered {
        Some(remembered) => {
            let text = document.snapshot().clone();
            let line = |offset: usize| text.offset_to_point(offset).map(|point| point.line).unwrap_or_default();
            let folded: std::collections::HashSet<&FoldLines> = remembered.folds.iter().collect();
            document.fold_all(foldable.iter().map(|fold| fold.range.clone()).filter(|range| {
                folded.contains(&FoldLines { start_line: line(range.start), end_line: line(range.end) })
            }));
            for manual in &remembered.manual {
                if let Some(range) = manual.range_in(&text) {
                    document.add_manual_region(range.clone());
                    if manual.folded {
                        document.set_folded(range, true);
                    }
                }
            }
        }
        None => {
            let text = document.snapshot().clone();
            let name = document.name();
            let mut on = std::collections::HashMap::new();
            let by_default: Vec<std::ops::Range<usize>> = foldable
                .iter()
                .filter(|fold| {
                    default_option(&name, &text, fold, &foldable)
                        .is_some_and(|option| *on.entry(option).or_insert_with(|| is_on(session, option)))
                })
                .map(|fold| fold.range.clone())
                .collect();
            let collapsed = custom_regions(session, document).into_iter().filter(|region| region.collapsed);
            document.fold_all(by_default.into_iter().chain(collapsed.map(|region| region.range)));
        }
    }
    document.foldable_listed_by(true, |_| foldable);
}

pub fn is_on(session: &Session, key: &str) -> bool {
    session.settings().get::<bool>(key).unwrap_or_else(|| option_of(session, key).is_some_and(|option| option.default))
}

fn option_of(session: &Session, key: &str) -> Option<FoldOption> {
    GENERAL
        .options
        .iter()
        .find(|option| option.key == key)
        .copied()
        .or_else(|| session.state::<FoldingGroups>().and_then(|groups| groups.option(key)))
}

pub fn default_option(
    name: &str,
    text: &zen_text::Text,
    fold: &zen_editor::Fold,
    foldable: &[zen_editor::Fold],
) -> Option<&'static str> {
    if let Some(option) = fold.option {
        return Some(option);
    }
    match fold.kind {
        zen_editor::FoldKind::Region => Some(folding::CUSTOM_REGIONS),
        _ if !has_c_like_comments(name) => None,
        zen_editor::FoldKind::Imports => Some(folding::IMPORTS),
        zen_editor::FoldKind::Comment if is_file_header(text, fold) => Some(folding::FILE_HEADER),
        zen_editor::FoldKind::Comment if is_doc_comment(text, fold) => Some(folding::DOC_COMMENTS),
        zen_editor::FoldKind::Block if is_function_body(text, fold, foldable) => Some(folding::METHOD_BODIES),
        _ => None,
    }
}

fn has_c_like_comments(name: &str) -> bool {
    zen_syntax::comment_syntax(name)
        .is_some_and(|syntax| syntax.line == Some("//") || syntax.block.is_some_and(|(open, _)| open == "/*"))
}

fn is_file_header(text: &zen_text::Text, fold: &zen_editor::Fold) -> bool {
    let first = line_of(text)(fold.range.start);
    !is_doc_comment(text, fold)
        && (0..first).all(|line| text.line(line).is_ok_and(|body| body.trim().is_empty()))
        && text.line(first).is_ok_and(|body| {
            let at = text.point_to_offset(zen_text::Point::new(first, 0)).unwrap_or_default();
            let before: String = body.chars().take(fold.range.start.saturating_sub(at)).collect();
            before.trim().is_empty() || before.trim_start().starts_with("//")
        })
}

fn is_function_body(text: &zen_text::Text, fold: &zen_editor::Fold, foldable: &[zen_editor::Fold]) -> bool {
    let Some(open) = fold.range.start.checked_sub(1) else { return false };
    if text.slice(open..fold.range.start).ok().as_deref() != Some("{") {
        return false;
    }
    let line = line_of(text);
    let mut statement = line(open);
    let first = text.line(statement).unwrap_or_default();
    if first.trim_start().starts_with(')') {
        let indent = first.chars().take_while(|c| c.is_whitespace()).count();
        let close = text.point_to_offset(zen_text::Point::new(statement, 0)).unwrap_or_default() + indent;
        let parameters = foldable.iter().find(|other| {
            other.range.end == close
                && other.kind == zen_editor::FoldKind::Block
                && other.range.start > 0
                && text.slice(other.range.start - 1..other.range.start).ok().as_deref() == Some("(")
        });
        if let Some(parameters) = parameters {
            statement = line(parameters.range.start - 1);
        }
    }
    let body = text.line(statement).unwrap_or_default();
    let mut words = body.trim_start();
    const MODIFIERS: &[&str] = &[
        "pub(crate)",
        "pub(super)",
        "pub",
        "async",
        "const",
        "unsafe",
        "extern \"C\"",
        "export",
        "default",
        "static",
        "override",
        "suspend",
        "inline",
        "private",
        "public",
        "protected",
    ];
    while let Some(rest) =
        MODIFIERS.iter().find_map(|modifier| words.strip_prefix(modifier).filter(|rest| rest.starts_with(' ')))
    {
        words = rest.trim_start();
    }
    ["fn ", "func ", "function ", "fun ", "def "].iter().any(|keyword| words.starts_with(keyword))
}

pub(crate) fn document(session: &mut Session) -> Result<&mut Document, CommandError> {
    session
        .state_mut::<Workbench>()
        .active_document_mut()
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

pub(crate) fn has_text(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

pub(crate) fn foldable(session: &mut Session, current: bool) -> Result<Arc<Vec<zen_editor::Fold>>, CommandError> {
    let read = {
        let document = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|document| document.image().is_none())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        document.foldable_wanted(current).then(|| read_folds(session, document))
    };
    let document = document(session)?;
    Ok(document.foldable_listed_by(current, |document| read.unwrap_or_else(|| zen_editor::folds(document.text()))).0)
}

fn list(document: &Document, foldable: &[zen_editor::Fold]) -> Vec<FoldInfo> {
    list_some(document, foldable, &ListParams::default())
}

fn list_some(document: &Document, foldable: &[zen_editor::Fold], params: &ListParams) -> Vec<FoldInfo> {
    let text = document.snapshot();
    let name = document.name();
    let line = |offset: usize| text.offset_to_point(offset).map(|point| point.line).unwrap_or_default();
    let offset = |line: usize| text.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or(text.len_chars());
    let folded = document.folds();
    let is_folded = |range: &std::ops::Range<usize>| {
        let at = folded.partition_point(|other| {
            (other.start, std::cmp::Reverse(other.end)) < (range.start, std::cmp::Reverse(range.end))
        });
        folded.get(at) == Some(range)
    };
    let (from, to) = match (params.from_line, params.to_line) {
        (None, None) => (0, foldable.len()),
        (from, to) => {
            let start = from.map_or(0, offset);
            let end = to.map_or(text.len_chars() + 1, |to| offset(to.saturating_add(1)));
            (
                foldable.partition_point(|fold| fold.range.start < start),
                foldable.partition_point(|fold| fold.range.start < end),
            )
        }
    };
    let comments = region_comments(document);
    let placeholder = |fold: &zen_editor::Fold| {
        if fold.placeholder.is_some() {
            return fold.placeholder.clone();
        }
        if fold.kind != zen_editor::FoldKind::Region {
            return None;
        }
        let first = text.line(line(fold.range.start)).ok()?;
        match zen_editor::region_marker(&first, comments)? {
            zen_editor::RegionMarker::Start { description, .. } if !description.is_empty() => Some(description),
            _ => None,
        }
    };
    let info = |fold: &zen_editor::Fold| FoldInfo {
        start: fold.range.start,
        end: fold.range.end,
        start_line: line(fold.range.start),
        end_line: line(fold.range.end),
        kind: kind_name(fold.kind).to_owned(),
        folded: is_folded(&fold.range),
        placeholder: placeholder(fold),
        sticky_line: crate::sticky::fold_sticky_line(text, fold),
        option: default_option(&name, text, fold, foldable),
    };
    let around = params.from_line.filter(|_| params.enclosing).map(offset);
    let encloses = |fold: &zen_editor::Fold| around.is_some_and(|at| fold.range.start < at && at <= fold.range.end);
    let mut listed: Vec<FoldInfo> =
        foldable[..from].iter().filter(|fold| is_folded(&fold.range) || encloses(fold)).map(info).collect();
    listed.extend(foldable[from..to].iter().map(info));
    listed.extend(foldable[to..].iter().filter(|fold| is_folded(&fold.range)).map(info));
    listed
}

/// The parameters of `editor.fold.list`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// Read the folds from the current text (the default). `false` lists the folds last read,
    /// moved along with the edits since, which the editor does while typing goes on in a large file.
    pub current: bool,
    /// Only the folds that start on this line (from 0) or after it, and every folded one.
    pub from_line: Option<usize>,
    /// Only the folds that start on this line (from 0) or before it, and every folded one.
    pub to_line: Option<usize>,
    /// With `fromLine`, also the folds that start before that line and go on past its start.
    pub enclosing: bool,
}

impl Default for ListParams {
    fn default() -> Self {
        Self { current: true, from_line: None, to_line: None, enclosing: false }
    }
}

pub struct ListFolds;

impl Command for ListFolds {
    type Params = ListParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = "editor.fold.list";
    const TITLE: &'static str = "Folding Regions";
    const DESCRIPTION: &'static str = "The active file's foldable regions (blocks, imports, comment runs, custom regions, manual \
         regions) and whether each is folded, lines from 0; optionally only those starting on some lines (and every folded one, \
         and those around the first line).";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: ListParams) -> Result<Vec<FoldInfo>, CommandError> {
        let foldable = foldable(session, params.current)?;
        Ok(list_some(document(session)?, &foldable, &params))
    }
}

/// The parameters of `editor.fold.toggle`.
#[derive(Deserialize, JsonSchema)]
pub struct ToggleParams {
    /// The fold's first character, as `editor.fold.list` gives it.
    pub start: usize,
}

pub struct ToggleFold;

impl Command for ToggleFold {
    type Params = ToggleParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = "editor.fold.toggle";
    const TITLE: &'static str = "Toggle Fold";
    const DESCRIPTION: &'static str =
        "Folds or unfolds the region that starts at a character, as a click on the gutter's mark does.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: ToggleParams) -> Result<Vec<FoldInfo>, CommandError> {
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        let fold = foldable
            .iter()
            .find(|fold| fold.range.start == params.start)
            .cloned()
            .ok_or_else(|| CommandError::new(t!("workbench-fold-none")))?;
        let folded = document.folds().contains(&fold.range);
        if !folded {
            let hidden = document
                .selections()
                .iter()
                .any(|selection| fold.range.start < selection.head && selection.head < fold.range.end);
            if hidden {
                document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(fold.range.start)));
            }
        }
        document.set_folded(fold.range, !folded);
        Ok(list(document, &foldable))
    }
}

pub struct FoldAtCaret<const EXPAND: bool>;

impl<const EXPAND: bool> Command for FoldAtCaret<EXPAND> {
    type Params = NoParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = if EXPAND { "editor.fold.expand" } else { "editor.fold.collapse" };
    const TITLE: &'static str = if EXPAND { "Expand" } else { "Collapse" };
    const DESCRIPTION: &'static str = if EXPAND {
        "Unfolds the folded regions on the caret's line or around the caret."
    } else {
        "Folds the innermost region around the caret, and moves the caret out of it."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FoldInfo>, CommandError> {
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        let caret = document.selections().primary().head;
        let text = document.snapshot().clone();
        let line = |offset: usize| text.offset_to_point(offset).map(|point| point.line).unwrap_or_default();
        if EXPAND {
            let unfold: Vec<_> = document
                .folds()
                .iter()
                .filter(|range| line(range.start) == line(caret) || (range.start <= caret && caret <= range.end))
                .cloned()
                .collect();
            for range in unfold {
                document.set_folded(range, false);
            }
        } else if let Some(fold) = foldable
            .iter()
            .filter(|fold| {
                fold.range.start <= caret && caret <= fold.range.end && !document.folds().contains(&fold.range)
            })
            .min_by_key(|fold| fold.range.len())
            .cloned()
        {
            if caret > fold.range.start {
                document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(fold.range.start)));
            }
            document.set_folded(fold.range, true);
        }
        Ok(list(document, &foldable))
    }
}

pub struct FoldAll<const EXPAND: bool>;

impl<const EXPAND: bool> Command for FoldAll<EXPAND> {
    type Params = NoParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = if EXPAND { "editor.fold.expandAll" } else { "editor.fold.collapseAll" };
    const TITLE: &'static str = if EXPAND { "Expand All" } else { "Collapse All" };
    const DESCRIPTION: &'static str = if EXPAND {
        "Unfolds every folded region of the file, or only those inside the selection when there is one."
    } else {
        "Folds every region of the file, or only those inside the selection when there is one."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FoldInfo>, CommandError> {
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        let selected: Vec<std::ops::Range<usize>> = document
            .selections()
            .iter()
            .filter(|selection| !selection.is_empty())
            .map(|selection| selection.range())
            .collect();
        let chosen =
            |fold: &&zen_editor::Fold| selected.is_empty() || selected.iter().any(|range| within(&fold.range, range));
        for fold in foldable.iter().filter(chosen) {
            document.set_folded(fold.range.clone(), !EXPAND);
        }
        if !EXPAND && selected.is_empty() {
            keep_caret_out_of_folds(document);
        }
        Ok(list(document, &foldable))
    }
}

fn line_of(text: &zen_text::Text) -> impl Fn(usize) -> usize + '_ {
    |offset| text.offset_to_point(offset).map(|point| point.line).unwrap_or_default()
}

fn within(inner: &std::ops::Range<usize>, outer: &std::ops::Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn depths(foldable: &[zen_editor::Fold]) -> Vec<usize> {
    let mut around: Vec<&std::ops::Range<usize>> = Vec::new();
    foldable
        .iter()
        .map(|fold| {
            while around.last().is_some_and(|outer| !within(&fold.range, outer)) {
                around.pop();
            }
            around.push(&fold.range);
            around.len()
        })
        .collect()
}

fn region_at_caret(
    document: &Document,
    foldable: &[zen_editor::Fold],
    prefer_folded: bool,
) -> Option<zen_editor::Fold> {
    let caret = document.selections().primary().head;
    let line = line_of(document.snapshot());
    let on_line = |fold: &zen_editor::Fold| line(fold.range.start) == line(caret);
    let folded = document.folds();
    let folded_here =
        foldable.iter().find(|fold| on_line(fold) && folded.contains(&fold.range)).filter(|_| prefer_folded);
    let at_caret = || {
        foldable
            .iter()
            .filter(|fold| on_line(fold) || (fold.range.start <= caret && caret <= fold.range.end))
            .min_by_key(|fold| fold.range.len())
    };
    folded_here.or_else(at_caret).cloned()
}

fn keep_caret_out_of_folds(document: &mut Document) {
    let caret = document.selections().primary().head;
    if let Some(outer) = document.folds().iter().find(|range| range.start < caret && caret < range.end).cloned() {
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(outer.start)));
    }
}

pub fn is_doc_comment(text: &zen_text::Text, fold: &zen_editor::Fold) -> bool {
    if fold.kind != zen_editor::FoldKind::Comment {
        return false;
    }
    let line = line_of(text);
    let first = text.line(line(fold.range.start)).unwrap_or_default();
    let first = first.trim_start();
    if ["/**", "///", "//!"].iter().any(|doc| first.starts_with(doc)) {
        return true;
    }
    let next = text.line(line(fold.range.end) + 1).unwrap_or_default();
    let next = next.trim();
    !next.is_empty() && !["//", "/*", "*"].iter().any(|comment| next.starts_with(comment))
}

pub struct FoldRecursively<const EXPAND: bool>;

impl<const EXPAND: bool> Command for FoldRecursively<EXPAND> {
    type Params = NoParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = if EXPAND { "editor.fold.expandRecursively" } else { "editor.fold.collapseRecursively" };
    const TITLE: &'static str = if EXPAND { "Expand Recursively" } else { "Collapse Recursively" };
    const DESCRIPTION: &'static str = if EXPAND {
        "Unfolds the region at the caret (a folded one on the caret's line first) and every region inside it."
    } else {
        "Folds the innermost region around the caret and every region inside it, and moves the caret out of them."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FoldInfo>, CommandError> {
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        if let Some(region) = region_at_caret(document, &foldable, EXPAND) {
            for fold in foldable.iter().filter(|fold| within(&fold.range, &region.range)) {
                document.set_folded(fold.range.clone(), !EXPAND);
            }
            keep_caret_out_of_folds(document);
        }
        Ok(list(document, &foldable))
    }
}

/// The parameters of `editor.fold.expandToLevel` and `editor.fold.expandAllToLevel`.
#[derive(Deserialize, JsonSchema)]
pub struct LevelParams {
    /// How deep to unfold, from 1: 1 unfolds the region itself (or the file's outermost regions)
    /// and folds those inside it, 2 unfolds one level more...
    pub level: usize,
}

pub struct ExpandToLevel<const ALL: bool>;

impl<const ALL: bool> Command for ExpandToLevel<ALL> {
    type Params = LevelParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = if ALL { "editor.fold.expandAllToLevel" } else { "editor.fold.expandToLevel" };
    const TITLE: &'static str = if ALL { "Expand All to Level" } else { "Expand to Level" };
    const DESCRIPTION: &'static str = if ALL {
        "Unfolds the regions of the file down to a depth (1: the outermost) and folds the deeper ones."
    } else {
        "Unfolds the region at the caret down to a depth (1: the region itself) and folds the deeper ones."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: LevelParams) -> Result<Vec<FoldInfo>, CommandError> {
        if params.level == 0 {
            return Err(CommandError::new(t!("workbench-fold-level")));
        }
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        let depths = depths(&foldable);
        let (region, base) = if ALL {
            (None, 0)
        } else {
            let Some(region) = region_at_caret(document, &foldable, true) else { return Ok(list(document, &foldable)) };
            let at = foldable.iter().position(|fold| fold.range == region.range).unwrap_or_default();
            (Some(region.range), depths.get(at).copied().unwrap_or(1) - 1)
        };
        for (fold, depth) in foldable.iter().zip(&depths) {
            if region.as_ref().is_some_and(|region| !within(&fold.range, region)) {
                continue;
            }
            document.set_folded(fold.range.clone(), depth - base > params.level);
        }
        keep_caret_out_of_folds(document);
        Ok(list(document, &foldable))
    }
}

pub struct FoldDocComments<const EXPAND: bool>;

impl<const EXPAND: bool> Command for FoldDocComments<EXPAND> {
    type Params = NoParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = if EXPAND { "editor.fold.expandDocComments" } else { "editor.fold.collapseDocComments" };
    const TITLE: &'static str = if EXPAND { "Expand Doc Comments" } else { "Collapse Doc Comments" };
    const DESCRIPTION: &'static str = if EXPAND {
        "Unfolds every doc comment of the file: a comment right above a line of code, or a `/** */`, `///` or `//!` one."
    } else {
        "Folds every doc comment of the file: a comment right above a line of code, or a `/** */`, `///` or `//!` one."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FoldInfo>, CommandError> {
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        let text = document.snapshot().clone();
        for fold in foldable.iter().filter(|fold| is_doc_comment(&text, fold)) {
            document.set_folded(fold.range.clone(), !EXPAND);
        }
        keep_caret_out_of_folds(document);
        Ok(list(document, &foldable))
    }
}

pub struct ToggleAtCaret;

impl Command for ToggleAtCaret {
    type Params = NoParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = "editor.fold.toggleAtCaret";
    const TITLE: &'static str = "Toggle Folding";
    const DESCRIPTION: &'static str = "Unfolds the folded regions on the caret's line or around the caret; when there are none, folds the smallest region around the caret or starting on its line.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FoldInfo>, CommandError> {
        let unfolds = {
            let document = document(session)?;
            let caret = document.selections().primary().head;
            let line = line_of(document.snapshot());
            document
                .folds()
                .iter()
                .any(|range| line(range.start) == line(caret) || (range.start <= caret && caret <= range.end))
        };
        if unfolds {
            return FoldAtCaret::<true>.run(session, NoParams {});
        }
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        if let Some(region) = region_at_caret(document, &foldable, false) {
            document.set_folded(region.range, true);
            keep_caret_out_of_folds(document);
        }
        Ok(list(document, &foldable))
    }
}

pub struct CollapseBlock;

impl Command for CollapseBlock {
    type Params = NoParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = "editor.fold.collapseBlock";
    const TITLE: &'static str = "Fold Code Block";
    const DESCRIPTION: &'static str = "Folds the innermost code block in brackets around the caret (not a comment or the imports), and moves the caret out of it.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FoldInfo>, CommandError> {
        let foldable = foldable(session, true)?;
        let document = document(session)?;
        let caret = document.selections().primary().head;
        let block = foldable
            .iter()
            .filter(|fold| fold.kind == zen_editor::FoldKind::Block)
            .filter(|fold| {
                fold.range.start <= caret && caret <= fold.range.end && !document.folds().contains(&fold.range)
            })
            .min_by_key(|fold| fold.range.len())
            .cloned();
        if let Some(block) = block {
            document.set_folded(block.range, true);
            keep_caret_out_of_folds(document);
        }
        Ok(list(document, &foldable))
    }
}

pub struct FoldSelection;

impl Command for FoldSelection {
    type Params = NoParams;
    type Output = Vec<FoldInfo>;
    const ID: &'static str = "editor.fold.selection";
    const TITLE: &'static str = "Fold Selection / Remove Region";
    const DESCRIPTION: &'static str = "With a selection, folds it as a manual region (any range of text; a selection that \
         ends at the start of a line stops at the end of the line before) and puts the caret before it; without one, removes \
         the innermost manual region on the caret's line or around the caret, unfolding it.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FoldInfo>, CommandError> {
        foldable(session, true)?;
        let document = document(session)?;
        let text = document.snapshot().clone();
        let selected: Vec<std::ops::Range<usize>> = document
            .selections()
            .iter()
            .filter(|selection| !selection.is_empty())
            .map(|selection| without_line_break(&text, selection.range()))
            .filter(|range| !range.is_empty())
            .collect();
        if let Some(first) = selected.first() {
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(first.start)));
            for range in selected {
                document.add_manual_region(range.clone());
                document.set_folded(range, true);
            }
        } else {
            let caret = document.selections().primary().head;
            let line = line_of(&text);
            let here = document
                .manual_regions()
                .iter()
                .filter(|range| line(range.start) == line(caret) || (range.start <= caret && caret <= range.end))
                .min_by_key(|range| range.len())
                .cloned();
            if let Some(range) = here {
                document.remove_manual_region(&range);
            }
        }
        let foldable = foldable(session, true)?;
        Ok(list(self::document(session)?, &foldable))
    }
}

fn without_line_break(text: &zen_text::Text, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let ends_line = |end: usize| end > range.start && text.slice(end - 1..end).is_ok_and(|last| last == "\n");
    if !ends_line(range.end) {
        return range;
    }
    let end = range.end - 1;
    let crlf = end > range.start && text.slice(end - 1..end).is_ok_and(|last| last == "\r");
    range.start..if crlf { end - 1 } else { end }
}

/// A custom folding region as `editor.fold.regions` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegionInfo {
    /// Its name, empty when it has none.
    pub description: String,
    /// The first character it folds: the start of its opening comment.
    pub start: usize,
    /// The character after the last it folds.
    pub end: usize,
    /// The line its opening comment is on, from 0.
    pub line: usize,
    /// How many custom regions hold it.
    pub depth: usize,
    /// `region` (`// region` … `// endregion`) or `editorFold` (`<editor-fold>` … `</editor-fold>`).
    pub style: String,
    /// Whether it is folded.
    pub folded: bool,
}

pub struct ListRegions;

impl Command for ListRegions {
    type Params = NoParams;
    type Output = Vec<RegionInfo>;
    const ID: &'static str = "editor.fold.regions";
    const TITLE: &'static str = "Custom Folding Regions";
    const DESCRIPTION: &'static str = "The active file's custom folding regions (`// region Name` … `// endregion`, \
         `//<editor-fold desc=\"Name\">` … `//</editor-fold>`, in the file's comment syntax), in order, with their names, \
         lines from 0 and nesting, as Custom Folding (⌥⌘.) lists them; go to one with navigate.place.goTo at its start.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<RegionInfo>, CommandError> {
        let document = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|document| document.image().is_none())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let regions = custom_regions(session, document);
        let text = document.snapshot();
        let line = line_of(text);
        let mut around: Vec<std::ops::Range<usize>> = Vec::new();
        Ok(regions
            .into_iter()
            .map(|region| {
                while around.last().is_some_and(|outer| !within(&region.range, outer)) {
                    around.pop();
                }
                let depth = around.len();
                around.push(region.range.clone());
                RegionInfo {
                    start: region.range.start,
                    end: region.range.end,
                    line: line(region.range.start),
                    depth,
                    style: match region.style {
                        zen_editor::RegionStyle::Region => "region",
                        zen_editor::RegionStyle::EditorFold => "editorFold",
                    }
                    .to_owned(),
                    folded: document.folds().contains(&region.range),
                    description: region.description,
                }
            })
            .collect())
    }
}

pub struct RevealCarets;

impl Command for RevealCarets {
    type Params = NoParams;
    type Output = bool;
    const ID: &'static str = "editor.fold.reveal";
    const TITLE: &'static str = "Unfold at Carets";
    const DESCRIPTION: &'static str =
        "Unfolds every folded region that hides a caret; returns whether one was unfolded.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<bool, CommandError> {
        let document = document(session)?;
        let carets: Vec<usize> = document.selections().iter().map(|selection| selection.head).collect();
        let hiding: Vec<_> = document
            .folds()
            .iter()
            .filter(|range| carets.iter().any(|caret| range.start < *caret && *caret < range.end))
            .cloned()
            .collect();
        let any = !hiding.is_empty();
        for range in hiding {
            document.set_folded(range, false);
        }
        Ok(any)
    }
}

pub struct RememberedFolds;

impl Command for RememberedFolds {
    type Params = NoParams;
    type Output = Vec<FileFolds>;
    const ID: &'static str = "editor.fold.remembered";
    const TITLE: &'static str = "Remembered Folds";
    const DESCRIPTION: &'static str = "The folded regions and manual regions (Fold Selection) of every file, as the \
         workspace keeps them: those of the open files as they are, and those of files closed, which they open with again.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<FileFolds>, CommandError> {
        Ok(session.state::<Workbench>().map(Workbench::fold_states).unwrap_or_default())
    }
}

/// The parameters of `editor.fold.remember`.
#[derive(Deserialize, JsonSchema)]
pub struct RememberParams {
    /// The files and their folded regions, as `editor.fold.remembered` lists them.
    pub files: Vec<FileFolds>,
}

pub struct RememberFolds;

impl Command for RememberFolds {
    type Params = RememberParams;
    type Output = ();
    const ID: &'static str = "editor.fold.remember";
    const TITLE: &'static str = "Remember Folds";
    const DESCRIPTION: &'static str = "Gives files the folded regions and manual regions to open with, as the workspace \
         restores them; a file that is open keeps the folds it has. A region comes back when the file has one on the same \
         lines, a manual region where the file still has its lines and columns.";

    fn run(&self, session: &mut Session, params: RememberParams) -> Result<(), CommandError> {
        let workbench = session.state_mut::<Workbench>();
        for file in params.files {
            workbench.remember_folds(file);
        }
        Ok(())
    }
}

/// The gutter's fold arrows and the "Fold by default" options, as `editor.folding.options` lists them.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoldingSettings {
    /// Show code folding arrows: whether the gutter shows them at all.
    pub show_arrows: bool,
    /// When the arrows of unfolded regions show: `onHover` of the gutter or `always`.
    pub arrows: String,
    /// Show bottom arrows: an arrow on the last line of each unfolded region too.
    pub bottom_arrows: bool,
    /// Fold by default: the groups of options, General first.
    pub groups: Vec<FoldingGroupInfo>,
}

/// A group of "Fold by default" options as listed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoldingGroupInfo {
    /// Its title, e.g. "General" or "Go".
    pub title: String,
    /// Its options.
    pub options: Vec<FoldOptionInfo>,
}

/// A "Fold by default" option as listed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoldOptionInfo {
    /// Its setting, which the MCP tool `set_settings` changes.
    pub key: String,
    /// Its label on the page.
    pub label: String,
    /// Its value when the user has not set it.
    pub default: bool,
    /// Its value now.
    pub value: bool,
}

pub struct FoldingOptionsList;

impl Command for FoldingOptionsList {
    type Params = NoParams;
    type Output = FoldingSettings;
    const ID: &'static str = "editor.folding.options";
    const TITLE: &'static str = "Code Folding Options";
    const DESCRIPTION: &'static str = "Settings → Editor → General → Code Folding: whether the gutter shows fold arrows \
         (always or on hover), the bottom arrows, and the \"Fold by default\" options (General, and those of the language \
         modules that are on, such as Go) with their settings, defaults and values. A file opened for the first time starts \
         with the folds of the options that are on; editor.fold.list gives each fold's option. Change them with the MCP \
         tool set_settings: foldArrows, foldBottomArrows, showFoldArrows, foldFileHeader, foldImports, foldDocComments, \
         foldMethodBodies, foldCustomRegions, and a module's options in declaredSettings by key.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FoldingSettings, CommandError> {
        let groups = session.state::<FoldingGroups>().map(|groups| groups.0.clone()).unwrap_or_default();
        let groups = std::iter::once(GENERAL)
            .chain(groups)
            .map(|group| FoldingGroupInfo {
                title: t!(group.title),
                options: group
                    .options
                    .iter()
                    .map(|option| FoldOptionInfo {
                        key: option.key.to_owned(),
                        label: t!(option.label),
                        default: option.default,
                        value: is_on(session, option.key),
                    })
                    .collect(),
            })
            .collect();
        let settings = session.settings();
        Ok(FoldingSettings {
            show_arrows: settings.get_or(SHOW_ARROWS, true),
            arrows: settings.get::<String>(ARROWS).unwrap_or_else(|| "onHover".to_owned()),
            bottom_arrows: settings.get_or(BOTTOM_ARROWS, false),
            groups,
        })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(FoldingOptionsList)?;
    registry.register(RememberedFolds)?;
    registry.register(RememberFolds)?;
    registry.register(ListFolds)?;
    registry.register(ToggleFold)?;
    registry.register(FoldAtCaret::<false>)?;
    registry.register(FoldAtCaret::<true>)?;
    registry.register(FoldAll::<false>)?;
    registry.register(FoldAll::<true>)?;
    registry.register(FoldRecursively::<false>)?;
    registry.register(FoldRecursively::<true>)?;
    registry.register(ExpandToLevel::<false>)?;
    registry.register(ExpandToLevel::<true>)?;
    registry.register(FoldDocComments::<false>)?;
    registry.register(FoldDocComments::<true>)?;
    registry.register(ToggleAtCaret)?;
    registry.register(CollapseBlock)?;
    registry.register(FoldSelection)?;
    registry.register(ListRegions)?;
    registry.register(RevealCarets)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session_with(text: &str) -> (Registry, Session, tempfile::TempDir) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        (registry, session, folder)
    }

    const GO: &str = "package main\n\nimport (\n\t\"fmt\"\n)\n\nfunc main() {\n\tfmt.Println()\n}\n";

    #[test]
    fn carets_the_fold_hides_move_before_it() {
        let (registry, mut session, _folder) = session_with(GO);
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let body = folds.as_array().unwrap().iter().find(|fold| fold["startLine"] == 6).unwrap().clone();
        let start = body["start"].as_u64().unwrap() as usize;
        let place = |session: &mut Session, offset: usize| {
            let selection = zen_editor::Selections::single(zen_editor::Selection::caret(offset));
            session.state_mut::<Workbench>().active_document_mut().unwrap().set_selections(selection);
        };
        let caret = |session: &Session| {
            session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head
        };
        place(&mut session, start + 3);
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": start })).unwrap();
        assert_eq!(caret(&session), start, "a hidden caret would open the fold again at once");
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": start })).unwrap();
        place(&mut session, 2);
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": start })).unwrap();
        assert_eq!(caret(&session), 2, "a caret outside the fold stays");
    }

    #[test]
    fn a_file_closed_and_opened_again_keeps_its_folds() {
        let text = format!("{GO}\nfunc a() {{\n\tx()\n}}\n");
        let (registry, mut session, folder) = session_with(&text);
        let path = folder.path().join("main.go");
        let folded = |registry: &Registry, session: &mut Session| -> Vec<(u64, String)> {
            let folds = registry.dispatch(session, "editor.fold.list", json!({})).unwrap();
            folds
                .as_array()
                .unwrap()
                .iter()
                .filter(|fold| fold["folded"] == true)
                .map(|fold| (fold["startLine"].as_u64().unwrap(), fold["kind"].as_str().unwrap().to_owned()))
                .collect()
        };
        assert_eq!(folded(&registry, &mut session), [(2, "imports".to_owned())]);
        let all = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        for fold in all.as_array().unwrap().iter().filter(|fold| fold["startLine"] == 2 || fold["startLine"] == 10) {
            registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": fold["start"] })).unwrap();
        }
        assert_eq!(folded(&registry, &mut session), [(10, "block".to_owned())]);
        registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
        let remembered = registry.dispatch(&mut session, "editor.fold.remembered", json!({})).unwrap();
        assert_eq!(remembered, json!([{ "path": path, "folds": [{ "startLine": 10, "endLine": 12 }] }]));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        assert_eq!(folded(&registry, &mut session), [(10, "block".to_owned())], "the imports stay unfolded");

        let (registry, mut restarted, _other) = session_with("package main\n");
        registry.dispatch(&mut restarted, "editor.fold.remember", json!({ "files": remembered })).unwrap();
        registry.dispatch(&mut restarted, "file.document.open", json!({ "path": path })).unwrap();
        assert_eq!(folded(&registry, &mut restarted), [(10, "block".to_owned())]);
        let listed = registry.dispatch(&mut restarted, "editor.fold.remembered", json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().last().unwrap()["path"], json!(path), "an open file lists its own folds");

        registry.dispatch(&mut restarted, "file.document.close", json!({})).unwrap();
        std::fs::write(&path, format!("// moved\n{text}")).unwrap();
        registry.dispatch(&mut restarted, "file.document.open", json!({ "path": path })).unwrap();
        assert_eq!(folded(&registry, &mut restarted), []);
    }

    struct Angles;

    struct AngleLines(Vec<zen_syntax::FoldRegion>);

    impl zen_syntax::Incremental for AngleLines {
        fn edit(&mut self, text: &str, _: zen_syntax::TextChange) -> usize {
            *self = Self::read(text);
            1
        }

        fn spans(&self, _: &str, _: std::ops::Range<usize>) -> Vec<zen_syntax::Span> {
            Vec::new()
        }

        fn folds(&self, _: &str) -> Option<Vec<zen_syntax::FoldRegion>> {
            Some(self.0.clone())
        }

        fn lines(&self) -> usize {
            1
        }
    }

    impl AngleLines {
        fn read(text: &str) -> Self {
            let (Some(open), Some(close)) = (text.find('<'), text.find('>')) else { return Self(Vec::new()) };
            let mut region = zen_syntax::FoldRegion::new(open + 1..close, zen_syntax::FoldRegionKind::Block);
            if text.contains("#tag") {
                region.option = Some(ANGLES);
                region.placeholder = Some(" tagged ".to_owned());
            }
            Self(vec![region])
        }
    }

    impl zen_syntax::Highlighter for Angles {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, _: &str) -> Vec<zen_syntax::Span> {
            Vec::new()
        }

        fn incremental(&self, text: &str) -> Option<Box<dyn zen_syntax::Incremental>> {
            Some(Box::new(AngleLines::read(text)))
        }
    }

    #[test]
    fn a_highlighter_that_finds_folds_gives_them() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, "ё <a\nb> {\n}\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let angles: Arc<dyn zen_syntax::Highlighter> = Arc::new(Angles);
        session
            .state_mut::<crate::brackets::Highlighters>()
            .set_source(Arc::new(move |name| angles.handles(name).then(|| Arc::clone(&angles))));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(
            folds,
            json!([{ "start": 3, "end": 6, "startLine": 0, "endLine": 1, "kind": "block", "folded": false, "stickyLine": 0 }]),
            "in characters, not the brackets'"
        );
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "<" })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(folds[0]["start"], 1, "read again after an edit");
    }

    #[test]
    fn imports_open_folded_and_folds_follow_the_caret() {
        let (registry, mut session, _folder) = session_with(GO);
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(folds[0]["kind"], "imports");
        assert_eq!(folds[0]["folded"], true, "imports start folded");
        assert_eq!(folds[1]["folded"], false);
        let inside = GO.find("fmt.Println").unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": inside }] })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.collapse", json!({})).unwrap();
        assert_eq!(folds[1]["folded"], true);
        let caret = session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
        assert_eq!(caret, folds[1]["start"].as_u64().unwrap() as usize);
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "// x\n" })).unwrap();
        let moved = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(moved[0]["folded"], true);
        assert_eq!(moved[0]["start"].as_u64().unwrap(), folds[0]["start"].as_u64().unwrap() + 5);
        let imports = moved[0]["start"].as_u64().unwrap();
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": imports })).unwrap();
        registry
            .dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": imports + 1 }] }))
            .unwrap();
        let refolded = registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": imports })).unwrap();
        assert_eq!(refolded[0]["folded"], true);
        assert_eq!(
            registry.dispatch(&mut session, "editor.fold.reveal", json!({})).unwrap(),
            json!(false),
            "it stays folded"
        );
        let inside = moved[0]["start"].as_u64().unwrap() + 2;
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": inside }] })).unwrap();
        assert_eq!(registry.dispatch(&mut session, "editor.fold.reveal", json!({})).unwrap(), json!(true));
        registry.dispatch(&mut session, "editor.fold.expandAll", json!({})).unwrap();
        let all = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert!(all.as_array().unwrap().iter().all(|fold| fold["folded"] == false));
    }

    #[test]
    fn folds_are_listed_for_some_lines_and_moved_while_typing() {
        let text = format!("{GO}\nfunc a() {{\n\tx()\n}}\n\nfunc b() {{\n\ty()\n}}\n");
        let (registry, mut session, _folder) = session_with(&text);
        let all = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 4, "imports, main, a, b");
        let some =
            registry.dispatch(&mut session, "editor.fold.list", json!({ "fromLine": 10, "toLine": 12 })).unwrap();
        let starts: Vec<u64> =
            some.as_array().unwrap().iter().map(|fold| fold["startLine"].as_u64().unwrap()).collect();
        assert_eq!(starts, vec![2, 10], "{some}");
        let around = registry
            .dispatch(&mut session, "editor.fold.list", json!({ "fromLine": 7, "toLine": 7, "enclosing": true }))
            .unwrap();
        let starts: Vec<u64> =
            around.as_array().unwrap().iter().map(|fold| fold["startLine"].as_u64().unwrap()).collect();
        assert_eq!(starts, vec![2, 6], "the folded imports and main, around line 7: {around}");
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "// x\n" })).unwrap();
        let moved = registry.dispatch(&mut session, "editor.fold.list", json!({ "current": false })).unwrap();
        let read = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(moved, read);
        assert_eq!(read[0]["start"].as_u64().unwrap(), all[0]["start"].as_u64().unwrap() + 5);
    }

    const NESTED: &str = "package main\n\nimport (\n\t\"fmt\"\n)\n\n// Run starts\n// the work.\nfunc Run() {\n\tif x {\n\t\tfor {\n\t\t\ty()\n\t\t}\n\t}\n}\n\n// licence\n// text\n\nfunc b() {\n\tz()\n}\n";

    fn folded_lines(registry: &Registry, session: &mut Session) -> Vec<u64> {
        let folds = registry.dispatch(session, "editor.fold.list", json!({})).unwrap();
        folds
            .as_array()
            .unwrap()
            .iter()
            .filter(|fold| fold["folded"] == true)
            .map(|fold| fold["startLine"].as_u64().unwrap())
            .collect()
    }

    fn caret_at(registry: &Registry, session: &mut Session, offset: usize) {
        registry.dispatch(session, "editor.selection.set", json!({ "selections": [{ "head": offset }] })).unwrap();
    }

    fn run(registry: &Registry, session: &mut Session, id: &str, params: serde_json::Value) -> Vec<u64> {
        registry.dispatch(session, id, params).unwrap();
        folded_lines(registry, session)
    }

    #[test]
    fn recursive_folding_takes_the_region_at_the_caret_and_those_inside_it() {
        let (registry, mut session, _folder) = session_with(NESTED);
        assert_eq!(folded_lines(&registry, &mut session), [2], "the imports");
        caret_at(&registry, &mut session, NESTED.find("y()").unwrap());
        assert_eq!(
            run(&registry, &mut session, "editor.fold.collapseRecursively", json!({})),
            [2, 10],
            "around the caret"
        );
        let caret = session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
        assert_eq!(caret, NESTED.find("for {").unwrap() + 5, "out of the fold, before it");
        caret_at(&registry, &mut session, NESTED.find("if x").unwrap());
        assert_eq!(
            run(&registry, &mut session, "editor.fold.collapseRecursively", json!({})),
            [2, 9, 10],
            "on its line"
        );
        let caret = session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
        assert_eq!(caret, NESTED.find("if x").unwrap(), "a caret before the fold stays");
        assert_eq!(registry.dispatch(&mut session, "editor.fold.reveal", json!({})).unwrap(), json!(false));
        assert_eq!(run(&registry, &mut session, "editor.fold.expandRecursively", json!({})), [2]);
        caret_at(&registry, &mut session, NESTED.find("func Run").unwrap());
        assert_eq!(run(&registry, &mut session, "editor.fold.collapseRecursively", json!({})), [2, 8, 9, 10]);
        assert_eq!(run(&registry, &mut session, "editor.fold.expandRecursively", json!({})), [2]);
    }

    #[test]
    fn expand_to_level_unfolds_down_to_a_depth_and_folds_what_is_deeper() {
        let (registry, mut session, _folder) = session_with(NESTED);
        assert_eq!(run(&registry, &mut session, "editor.fold.expandAllToLevel", json!({ "level": 1 })), [9, 10]);
        assert_eq!(run(&registry, &mut session, "editor.fold.expandAllToLevel", json!({ "level": 2 })), [10]);
        assert_eq!(run(&registry, &mut session, "editor.fold.expandAllToLevel", json!({ "level": 3 })), [0u64; 0]);
        assert!(registry.dispatch(&mut session, "editor.fold.expandAllToLevel", json!({ "level": 0 })).is_err());
        assert_eq!(run(&registry, &mut session, "editor.fold.collapseAll", json!({})), [2, 6, 8, 9, 10, 16, 19]);
        caret_at(&registry, &mut session, NESTED.find("func Run").unwrap());
        assert_eq!(
            run(&registry, &mut session, "editor.fold.expandToLevel", json!({ "level": 1 })),
            [2, 6, 9, 10, 16, 19]
        );
        assert_eq!(
            run(&registry, &mut session, "editor.fold.expandToLevel", json!({ "level": 2 })),
            [2, 6, 10, 16, 19]
        );
        caret_at(&registry, &mut session, NESTED.find("if x").unwrap());
        assert_eq!(
            run(&registry, &mut session, "editor.fold.expandToLevel", json!({ "level": 1 })),
            [2, 6, 10, 16, 19]
        );
    }

    #[test]
    fn doc_comments_fold_and_unfold_together() {
        let (registry, mut session, _folder) = session_with(NESTED);
        assert_eq!(run(&registry, &mut session, "editor.fold.collapseDocComments", json!({})), [2, 6]);
        assert_eq!(run(&registry, &mut session, "editor.fold.expandDocComments", json!({})), [2]);
        let text = zen_text::Text::from("/**\n * Doc.\n */\n\n/* plain\n   comment */\n\nx\n");
        let folds = zen_editor::folds(&text.to_string());
        let doc: Vec<bool> = folds.iter().map(|fold| is_doc_comment(&text, fold)).collect();
        assert_eq!(doc, [true, false], "`/**` always, a comment before a blank line not");
    }

    #[test]
    fn collapse_all_and_expand_all_keep_to_the_selection() {
        let (registry, mut session, _folder) = session_with(NESTED);
        let selection = |from: &str, to: &str| json!({ "selections": [{ "anchor": NESTED.find(from).unwrap(), "head": NESTED.find(to).unwrap() + to.len() }] });
        registry.dispatch(&mut session, "editor.selection.set", selection("func Run", "}\n}\n")).unwrap();
        assert_eq!(run(&registry, &mut session, "editor.fold.collapseAll", json!({})), [2, 8, 9, 10]);
        registry.dispatch(&mut session, "editor.selection.set", selection("\tif x", "\n\t}\n")).unwrap();
        assert_eq!(run(&registry, &mut session, "editor.fold.expandAll", json!({})), [2, 8], "only what is inside");
        caret_at(&registry, &mut session, 0);
        assert_eq!(run(&registry, &mut session, "editor.fold.expandAll", json!({})), [0u64; 0]);
    }

    #[test]
    fn toggle_folding_and_fold_code_block_act_at_the_caret() {
        let (registry, mut session, _folder) = session_with(NESTED);
        caret_at(&registry, &mut session, NESTED.find("y()").unwrap());
        assert_eq!(run(&registry, &mut session, "editor.fold.collapseBlock", json!({})), [2, 10]);
        caret_at(&registry, &mut session, NESTED.find("the work").unwrap());
        assert_eq!(run(&registry, &mut session, "editor.fold.collapseBlock", json!({})), [2, 10], "not a comment");
        caret_at(&registry, &mut session, NESTED.find("for {").unwrap());
        assert_eq!(run(&registry, &mut session, "editor.fold.toggleAtCaret", json!({})), [2], "unfolds on the line");
        assert_eq!(run(&registry, &mut session, "editor.fold.toggleAtCaret", json!({})), [2, 10], "the line's block");
        caret_at(&registry, &mut session, NESTED.find("\tif x").unwrap());
        assert_eq!(run(&registry, &mut session, "editor.fold.toggleAtCaret", json!({})), [2, 9, 10]);
    }

    const REGIONS: &str = "package main\n\n// region Helpers\n// a does it\nfunc a() {\n\tx()\n}\n\n//<editor-fold desc=\"Inner\" defaultstate=\"collapsed\">\nfunc b() {}\n//</editor-fold>\n// endregion\n\nfunc main() {\n\ta()\n}\n";

    fn listed(registry: &Registry, session: &mut Session, params: serde_json::Value) -> Vec<serde_json::Value> {
        registry.dispatch(session, "editor.fold.list", params).unwrap().as_array().unwrap().clone()
    }

    #[test]
    fn custom_regions_fold_whole_with_their_names_and_are_listed_for_custom_folding() {
        let (registry, mut session, _folder) = session_with(REGIONS);
        let folds = listed(&registry, &mut session, json!({}));
        let shown: Vec<(u64, u64, &str, bool, Option<&str>)> = folds
            .iter()
            .map(|fold| {
                (
                    fold["startLine"].as_u64().unwrap(),
                    fold["endLine"].as_u64().unwrap(),
                    fold["kind"].as_str().unwrap(),
                    fold["folded"] == true,
                    fold["placeholder"].as_str(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (2, 11, "region", false, Some("Helpers")),
                (4, 6, "block", false, None),
                (8, 10, "region", true, Some("Inner")),
                (13, 15, "block", false, None),
            ],
            "the comment run of the region's first lines is not a fold of its own; defaultstate=\"collapsed\" starts folded"
        );
        assert_eq!(folds[0]["start"], REGIONS.find("// region").unwrap());
        assert_eq!(folds[0]["end"], REGIONS.find("// endregion").unwrap() + "// endregion".len());
        let regions = registry.dispatch(&mut session, "editor.fold.regions", json!({})).unwrap();
        assert_eq!(
            regions,
            json!([
                { "description": "Helpers", "start": folds[0]["start"], "end": folds[0]["end"], "line": 2, "depth": 0, "style": "region", "folded": false },
                { "description": "Inner", "start": folds[2]["start"], "end": folds[2]["end"], "line": 8, "depth": 1, "style": "editorFold", "folded": true },
            ])
        );
        caret_at(&registry, &mut session, REGIONS.find("func b").unwrap() - 2);
        assert_eq!(
            run(&registry, &mut session, "editor.fold.collapse", json!({})),
            [2, 8],
            "the region around the caret"
        );
    }

    #[test]
    fn region_comments_follow_the_language_of_the_file() {
        for (name, text, expected) in [
            ("lib.rs", "// region Parsing\nfn a() {}\n// endregion\n", "Parsing"),
            ("app.js", "//#region Setup\nconst a = 1;\n//#endregion\n", "Setup"),
            ("tool.py", "# region Imports\nimport os\n# endregion\n", "Imports"),
            ("schema.sql", "-- <editor-fold desc=\"Tables\">\ncreate table t ();\n-- </editor-fold>\n", "Tables"),
            ("page.html", "<!-- region Header -->\n<h1>x</h1>\n<!-- endregion -->\n", "Header"),
        ] {
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join(name);
            std::fs::write(&path, text).unwrap();
            let mut registry = Registry::new();
            crate::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
            let folds = listed(&registry, &mut session, json!({}));
            let region =
                folds.iter().find(|fold| fold["kind"] == "region").unwrap_or_else(|| panic!("{name}: {folds:?}"));
            assert_eq!(region["placeholder"], expected, "{name}");
            assert_eq!((region["startLine"].as_u64(), region["endLine"].as_u64()), (Some(0), Some(2)), "{name}");
        }
    }

    #[test]
    fn a_highlighter_that_finds_folds_gets_the_custom_regions_too() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, "// region R\n<a\nb>\n// endregion\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let angles: Arc<dyn zen_syntax::Highlighter> = Arc::new(Angles);
        session
            .state_mut::<crate::brackets::Highlighters>()
            .set_source(Arc::new(move |name| angles.handles(name).then(|| Arc::clone(&angles))));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let kinds: Vec<String> = listed(&registry, &mut session, json!({}))
            .iter()
            .map(|fold| fold["kind"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(kinds, ["region", "block"]);
    }

    fn folded_kinds(registry: &Registry, session: &mut Session) -> Vec<(u64, u64, String, bool)> {
        listed(registry, session, json!({}))
            .iter()
            .filter(|fold| fold["kind"] == "manual" || fold["folded"] == true)
            .map(|fold| {
                (
                    fold["start"].as_u64().unwrap(),
                    fold["end"].as_u64().unwrap(),
                    fold["kind"].as_str().unwrap().to_owned(),
                    fold["folded"] == true,
                )
            })
            .collect()
    }

    fn select(registry: &Registry, session: &mut Session, anchor: usize, head: usize) {
        let selection = json!({ "selections": [{ "anchor": anchor, "head": head }] });
        registry.dispatch(session, "editor.selection.set", selection).unwrap();
    }

    #[test]
    fn fold_selection_folds_any_text_and_removes_the_region_at_the_caret() {
        let text = "package main\n\nfunc main() {\n\tfmt.Println(\"one\", \"two\")\n\ta := 1\n\tb := 2\n}\n";
        let (registry, mut session, _folder) = session_with(text);
        let inline = text.find("\"one\"").unwrap();
        select(&registry, &mut session, inline, inline + "\"one\", \"two\"".len());
        let folds = registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        assert!(
            folds.as_array().unwrap().iter().any(|fold| fold["kind"] == "manual" && fold["folded"] == true),
            "{folds}"
        );
        let caret = session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary();
        assert_eq!((caret.anchor, caret.head), (inline, inline), "the caret goes before the fold");
        let lines = text.find("\ta := 1").unwrap();
        let after = text.find("}\n").unwrap();
        select(&registry, &mut session, lines, after);
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        let end = text.find("b := 2").unwrap() + "b := 2".len();
        assert_eq!(
            folded_kinds(&registry, &mut session),
            [
                (inline as u64, (inline + 12) as u64, "manual".to_owned(), true),
                (lines as u64, end as u64, "manual".to_owned(), true)
            ]
        );
        caret_at(&registry, &mut session, inline);
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        assert_eq!(folded_kinds(&registry, &mut session), [(lines as u64, end as u64, "manual".to_owned(), true)]);
        registry.dispatch(&mut session, "editor.fold.expandAll", json!({})).unwrap();
        assert_eq!(
            folded_kinds(&registry, &mut session),
            [(lines as u64, end as u64, "manual".to_owned(), false)],
            "an unfolded manual region stays"
        );
        caret_at(&registry, &mut session, text.find("b := 2").unwrap());
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        assert_eq!(folded_kinds(&registry, &mut session), []);
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        assert_eq!(folded_kinds(&registry, &mut session), []);
    }

    #[test]
    fn manual_and_custom_regions_move_with_typing_without_reading_the_text_again() {
        let (registry, mut session, _folder) = session_with(REGIONS);
        let start = REGIONS.find("func main").unwrap();
        select(&registry, &mut session, start, start + "func main()".len());
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        let before = listed(&registry, &mut session, json!({}));
        caret_at(&registry, &mut session, 0);
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "// x\n" })).unwrap();
        let moved = listed(&registry, &mut session, json!({ "current": false }));
        let read = listed(&registry, &mut session, json!({}));
        assert_eq!(moved, read, "moved along with the edit as reading the text again finds them");
        let shifted: Vec<u64> = before.iter().map(|fold| fold["start"].as_u64().unwrap() + 5).collect();
        let now: Vec<u64> = read.iter().map(|fold| fold["start"].as_u64().unwrap()).collect();
        assert_eq!(now, shifted);
        assert!(read.iter().any(|fold| fold["kind"] == "manual" && fold["folded"] == true));
        let manual = read.iter().find(|fold| fold["kind"] == "manual").unwrap().clone();
        let end = manual["end"].as_u64().unwrap() as usize;
        caret_at(&registry, &mut session, end);
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "Z" })).unwrap();
        caret_at(&registry, &mut session, manual["start"].as_u64().unwrap() as usize);
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "A" })).unwrap();
        let typed = listed(&registry, &mut session, json!({ "current": false }));
        let typed = typed.iter().find(|fold| fold["kind"] == "manual").unwrap().clone();
        assert_eq!(
            (typed["start"].as_u64(), typed["end"].as_u64(), typed["folded"].as_bool()),
            (Some(manual["start"].as_u64().unwrap() + 1), Some(end as u64 + 1), Some(true)),
            "what is typed right before or after a manual region stays out of it"
        );
        let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
        let at = start + 6;
        document.replace_ranges(vec![(at..at + "func main()Z".len(), String::new())]);
        assert!(document.manual_regions().is_empty());
    }

    #[test]
    fn manual_regions_are_remembered_with_the_other_folds() {
        let (registry, mut session, folder) = session_with(REGIONS);
        let path = folder.path().join("main.go");
        let inline = REGIONS.find("x()").unwrap();
        select(&registry, &mut session, inline, inline + 1);
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        let open = REGIONS.find("\ta()").unwrap();
        select(&registry, &mut session, open, open + 4);
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        registry.dispatch(&mut session, "editor.fold.expandAll", json!({})).unwrap();
        select(&registry, &mut session, inline, inline + 1);
        registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        let regions = listed(&registry, &mut session, json!({}));
        let helpers = regions.iter().find(|fold| fold["placeholder"] == "Helpers").unwrap()["start"].clone();
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": helpers })).unwrap();
        let wanted = folded_kinds(&registry, &mut session);
        registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
        let remembered = registry.dispatch(&mut session, "editor.fold.remembered", json!({})).unwrap();
        assert_eq!(
            remembered,
            json!([{ "path": path, "folds": [{ "startLine": 2, "endLine": 11 }], "manual": [
                { "startLine": 5, "startColumn": 1, "endLine": 5, "endColumn": 2, "folded": true, "textHash": ManualRegion::hash("x") },
                { "startLine": 14, "startColumn": 0, "endLine": 14, "endColumn": 4, "folded": false, "textHash": ManualRegion::hash("\ta()") },
            ] }])
        );
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        assert_eq!(folded_kinds(&registry, &mut session), wanted, "reopened as left");

        let (registry, mut restarted, _other) = session_with("package main\n");
        registry.dispatch(&mut restarted, "editor.fold.remember", json!({ "files": remembered })).unwrap();
        registry.dispatch(&mut restarted, "file.document.open", json!({ "path": path })).unwrap();
        assert_eq!(folded_kinds(&registry, &mut restarted), wanted, "and at the next start");
        let old = json!([{ "path": path, "folds": [{ "startLine": 2, "endLine": 11 }] }]);
        registry.dispatch(&mut restarted, "editor.fold.remember", json!({ "files": old })).unwrap();

        let text = zen_text::Text::from(REGIONS);
        let at = |start_line, start_column, end_line, end_column, text_hash| ManualRegion {
            start_line,
            start_column,
            end_line,
            end_column,
            folded: true,
            text_hash,
        };
        assert_eq!(at(40, 0, 41, 0, None).range_in(&text), None);
        assert_eq!(at(0, 3, 0, 99, None).range_in(&text), None);
        assert_eq!(at(0, 0, 0, 7, None).range_in(&text), Some(0..7), "remembered before the text was kept");
        assert_eq!(at(0, 0, 0, 7, Some(ManualRegion::hash("package"))).range_in(&text), Some(0..7));
        assert_eq!(at(0, 0, 0, 7, Some(ManualRegion::hash("pockage"))).range_in(&text), None, "changed there");
    }

    struct Model {
        ids: Vec<u64>,
        next: u64,
        held: Vec<Vec<u64>>,
    }

    impl Model {
        fn edit(&mut self, from: usize, deleted: usize, inserted: usize) {
            let removed: Vec<u64> = self.ids.splice(from..from + deleted, std::iter::empty()).collect();
            let fresh: Vec<u64> = (0..inserted as u64).map(|n| self.next + n).collect();
            self.next += inserted as u64;
            for held in &mut self.held {
                held.retain(|id| !removed.contains(id));
                let before = from.checked_sub(1).is_some_and(|at| held.contains(&self.ids[at]));
                let after = self.ids.get(from).is_some_and(|id| held.contains(id));
                if before && after {
                    held.extend(&fresh);
                }
            }
            self.ids.splice(from..from, fresh);
        }

        fn ranges(&self) -> Vec<std::ops::Range<usize>> {
            let mut ranges: Vec<std::ops::Range<usize>> = self
                .held
                .iter()
                .filter(|held| !held.is_empty())
                .map(|held| {
                    let places: Vec<usize> =
                        self.ids.iter().enumerate().filter(|(_, id)| held.contains(id)).map(|(at, _)| at).collect();
                    let range = places[0]..places[places.len() - 1] + 1;
                    assert_eq!(places.len(), range.len(), "what a region holds stays together");
                    range
                })
                .collect();
            ranges.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
            ranges.dedup();
            ranges
        }
    }

    fn narrowed_edit(old: &[char], new: &[char], edit: &crate::CharEdit) -> (usize, usize, usize) {
        let (from, deleted, inserted) = (edit.range.start, edit.range.len(), edit.inserted);
        if deleted == 0 || inserted == 0 {
            return (from, deleted, inserted);
        }
        let (old, new) = (&old[from..from + deleted], &new[..inserted]);
        let prefix = old.iter().zip(new).take_while(|(old, new)| old == new).count();
        let suffix =
            old[prefix..].iter().rev().zip(new[prefix..].iter().rev()).take_while(|(old, new)| old == new).count();
        (from + prefix, deleted - prefix - suffix, inserted - prefix - suffix)
    }

    #[test]
    fn manual_regions_never_point_at_other_text_whatever_the_edits() {
        let start = "package main\n\nfunc main() {\n\tfmt.Println(\"one\", \"two\")\n\ta := 1\n}\n";
        for seed in 1..=60_u64 {
            let (registry, mut session, folder) = session_with(start);
            let path = folder.path().join("main.go");
            let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
            let mut random = move |below: usize| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % below.max(1) as u64) as usize
            };
            let len = start.chars().count();
            let mut model = Model { ids: (0..len as u64).collect(), next: len as u64, held: Vec::new() };
            for _ in 0..3 {
                let from = random(len - 1);
                let to = from + 1 + random(len - from - 1);
                let before = session.state::<Workbench>().unwrap().active_document().unwrap().manual_regions().to_vec();
                select(&registry, &mut session, from, to);
                registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
                if random(2) == 0 {
                    registry.dispatch(&mut session, "editor.fold.expandAll", json!({})).unwrap();
                }
                let document = session.state::<Workbench>().unwrap().active_document().unwrap();
                if let Some(made) = document.manual_regions().iter().find(|range| !before.contains(range)) {
                    model.held.push(model.ids[made.clone()].to_vec());
                }
            }
            let pieces = ["x", "\n", "\r\n", "ab", "é", "\t"];
            for step in 0..80 {
                let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
                let old: Vec<char> = document.text().chars().collect();
                let version = document.version();
                let len = old.len();
                let piece = pieces[random(pieces.len())].repeat(1 + random(3));
                let regions = document.manual_regions().to_vec();
                let action = random(10);
                match action {
                    0 => {
                        let at = random(len + 1);
                        document.replace_ranges(vec![(at..at, piece)]);
                    }
                    1 => {
                        let from = random(len);
                        document.replace_ranges(vec![(from..(from + 1 + random(6)).min(len), String::new())]);
                    }
                    2 => {
                        let from = random(len);
                        let to = (from + 1 + random(6)).min(len);
                        let kept: String = old[from..to].iter().collect();
                        let text = if random(2) == 0 { format!("{kept}{piece}") } else { piece };
                        document.replace_ranges(vec![(from..to, text)]);
                    }
                    3 => {
                        let mut carets: Vec<usize> = (0..1 + random(3)).map(|_| random(len + 1)).collect();
                        carets.sort_unstable();
                        carets.dedup();
                        let carets = carets.into_iter().map(zen_editor::Selection::caret).collect();
                        document.set_selections(zen_editor::Selections::from_vec(carets, 0));
                        document.apply_op(&zen_editor::EditOp::Insert { text: piece });
                    }
                    4 if !regions.is_empty() => {
                        let region = &regions[random(regions.len())];
                        let at = if random(2) == 0 { region.start } else { region.end };
                        document.replace_ranges(vec![(at..at, piece)]);
                    }
                    5 if !regions.is_empty() => {
                        let region = regions[random(regions.len())].clone();
                        document.replace_ranges(vec![(region, String::new())]);
                    }
                    6 => {
                        document.undo();
                    }
                    7 => {
                        document.redo();
                    }
                    8 => {
                        let text: String = old.iter().collect();
                        let other =
                            if text.contains("\r\n") { text.replace("\r\n", "\n") } else { text.replace('\n', "\r\n") };
                        std::fs::write(&path, other).unwrap();
                        document.reload_from_disk();
                    }
                    _ => {
                        let mut text: String = old.iter().collect();
                        let at = text.char_indices().nth(random(len)).map_or(text.len(), |(at, _)| at);
                        text.insert_str(at, "// changed elsewhere\n");
                        std::fs::write(&path, text).unwrap();
                        document.reload_from_disk();
                    }
                }
                let new: Vec<char> = document.text().chars().collect();
                let edits = document.char_edits_since(version).unwrap();
                let mut text = old.clone();
                for (index, edit) in edits.iter().enumerate() {
                    let at = edits[index + 1..].iter().fold(edit.range.start, |at, later| {
                        if later.range.end <= at { at + later.inserted - later.range.len() } else { at }
                    });
                    let written = new[at..at + edit.inserted].to_vec();
                    let (from, deleted, inserted) = narrowed_edit(&text, &written, edit);
                    model.edit(from, deleted, inserted);
                    text.splice(edit.range.clone(), written);
                }
                assert_eq!(model.ids.len(), new.len());
                let manual = document.manual_regions().to_vec();
                assert_eq!(
                    manual,
                    model.ranges(),
                    "seed {seed}, step {step}, action {action}: {:?} {edits:?}",
                    document.text()
                );
                for range in &manual {
                    assert!(range.end <= new.len());
                }
                let folded = document.folds().to_vec();
                assert!(
                    folded.iter().all(|fold| fold.end <= new.len() && fold.start < fold.end),
                    "seed {seed}, step {step}: {folded:?}"
                );
            }
        }
    }

    #[test]
    fn a_manual_region_whose_text_changed_on_disk_while_closed_is_dropped() {
        let (registry, mut session, folder) = session_with(REGIONS);
        let path = folder.path().join("main.go");
        let (first, second) = (REGIONS.find("x()").unwrap(), REGIONS.find("\ta()").unwrap());
        for start in [first, second] {
            select(&registry, &mut session, start, start + 3);
            registry.dispatch(&mut session, "editor.fold.selection", json!({})).unwrap();
        }
        registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
        let remembered = registry.dispatch(&mut session, "editor.fold.remembered", json!({})).unwrap();
        let old_format = json!([{ "path": path, "folds": [{ "startLine": 2, "endLine": 11 }] }]);
        std::fs::write(&path, REGIONS.replace("\tx()", "\ty()")).unwrap();

        let (registry, mut restarted, _other) = session_with("package main\n");
        registry.dispatch(&mut restarted, "editor.fold.remember", json!({ "files": remembered })).unwrap();
        registry.dispatch(&mut restarted, "file.document.open", json!({ "path": path })).unwrap();
        let manual: Vec<(u64, u64)> = folded_kinds(&registry, &mut restarted)
            .into_iter()
            .filter(|fold| fold.2 == "manual")
            .map(|fold| (fold.0, fold.1))
            .collect();
        assert_eq!(manual, [(second as u64, second as u64 + 3)], "the one whose text changed is gone, the other stays");

        let (registry, mut restarted, _other) = session_with("package main\n");
        registry.dispatch(&mut restarted, "editor.fold.remember", json!({ "files": old_format })).unwrap();
        registry.dispatch(&mut restarted, "file.document.open", json!({ "path": path })).unwrap();
        let folded: Vec<(u64, String)> =
            folded_kinds(&registry, &mut restarted).into_iter().map(|fold| (fold.0, fold.2)).collect();
        assert_eq!(folded, [(REGIONS.find("// region").unwrap() as u64, "region".to_owned())], "the old format loads");
    }

    const ANGLES: &str = "test.folding.angles";

    const RUST: &str = "// Copyright 2026 Fictional Holder Ltd, an invented name.
// Licensed under MIT.

use std::fmt;
use std::io;

/// Runs it.
/// Twice.
pub fn run() {
    work();
}

// region Helpers
fn helper(
    a: u8,
) -> u8 {
    a
}
// endregion

const TABLE: [u8; 2] = [
    1, 2,
];
";

    fn folded_on_open(on: &[&str]) -> Vec<String> {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("lib.rs");
        std::fs::write(&path, RUST).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        for option in GENERAL.options {
            session.settings_mut().set(option.key, on.contains(&option.key)).unwrap();
        }
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        folds
            .as_array()
            .unwrap()
            .iter()
            .filter(|fold| fold["folded"] == true)
            .map(|fold| fold["option"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn each_general_option_folds_its_regions_of_a_rust_file_when_it_opens() {
        for option in GENERAL.options {
            let folded = folded_on_open(&[option.key]);
            let expected = if option.key == folding::METHOD_BODIES { 2 } else { 1 };
            assert_eq!(folded, vec![option.key.to_owned(); expected], "{}", option.key);
        }
        assert_eq!(folded_on_open(&[]), Vec::<String>::new(), "the array is no method body");
    }

    fn options_on_open(name: &str, text: &str) -> Vec<(String, Option<String>)> {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(name);
        std::fs::write(&path, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        for option in GENERAL.options {
            session.settings_mut().set(option.key, true).unwrap();
        }
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let chars: Vec<char> = text.chars().collect();
        folds
            .as_array()
            .unwrap()
            .iter()
            .filter(|fold| fold["folded"] == true || fold["option"].is_string())
            .map(|fold| {
                let range = fold["start"].as_u64().unwrap() as usize..fold["end"].as_u64().unwrap() as usize;
                (chars[range].iter().collect(), fold["option"].as_str().map(str::to_owned))
            })
            .collect()
    }

    #[test]
    fn a_comment_that_documents_the_first_declaration_is_no_file_header() {
        let go = "// Package demo shows it.\n// It has two lines.\npackage demo\n\nfunc f() {\n\tg()\n}\n";
        let options: Vec<Option<String>> =
            options_on_open("demo.go", go).into_iter().map(|(_, option)| option).collect();
        assert!(!options.contains(&Some(folding::FILE_HEADER.to_owned())), "{options:?}");
        let rust = "//! The crate.\n//! It does things.\n\nfn f() {\n    g();\n}\n";
        let options: Vec<Option<String>> =
            options_on_open("lib.rs", rust).into_iter().map(|(_, option)| option).collect();
        assert!(!options.contains(&Some(folding::FILE_HEADER.to_owned())), "{options:?}");
    }

    #[test]
    fn files_without_c_like_comments_fold_nothing_by_guessing() {
        let markdown = "// not a comment here\n// either\n\n```go\nfunc main() {\n\trun()\n}\n```\n\n```rust\nuse std::fmt;\nuse std::io;\n```\n\nuse the tool;\nuse it again;\n";
        assert_eq!(options_on_open("README.md", markdown), [], "Markdown");
        let yaml = "# Licence.\n# Two lines.\nkey: {\n  a: 1,\n}\n";
        assert_eq!(options_on_open("config.yaml", yaml), [], "YAML");
        let json = "{\n  \"func f\": {\n    \"a\": 1\n  }\n}\n";
        assert_eq!(options_on_open("data.json", json), [], "JSON");
    }

    #[test]
    fn by_default_the_imports_start_folded() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("lib.rs");
        std::fs::write(&path, RUST).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": &path })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let folded: Vec<&str> = folds
            .as_array()
            .unwrap()
            .iter()
            .filter(|fold| fold["folded"] == true)
            .map(|fold| fold["option"].as_str().unwrap())
            .collect();
        assert_eq!(folded, [folding::IMPORTS]);

        let header = folds[0]["start"].clone();
        assert_eq!(folds[0]["option"], folding::FILE_HEADER);
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": header })).unwrap();
        let body = folds.as_array().unwrap().iter().find(|fold| fold["option"] == folding::METHOD_BODIES).unwrap();
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": body["start"] })).unwrap();
        registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
        session.settings_mut().set(folding::DOC_COMMENTS, true).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": &path })).unwrap();
        let again = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let folded: Vec<&str> = again
            .as_array()
            .unwrap()
            .iter()
            .filter(|fold| fold["folded"] == true)
            .map(|fold| fold["option"].as_str().unwrap())
            .collect();
        assert_eq!(
            folded,
            [folding::FILE_HEADER, folding::IMPORTS, folding::METHOD_BODIES],
            "the options apply only on the first open"
        );
    }

    #[test]
    fn a_language_tags_its_folds_with_its_options() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, "#tag <a\nb>\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let open = |on: Option<bool>| {
            let mut session = Session::new();
            let angles: Arc<dyn zen_syntax::Highlighter> = Arc::new(Angles);
            session
                .state_mut::<crate::brackets::Highlighters>()
                .set_source(Arc::new(move |name| angles.handles(name).then(|| Arc::clone(&angles))));
            const OPTIONS: FoldingOptions = FoldingOptions {
                title: "test",
                options: &[FoldOption { key: ANGLES, label: "test-angles", default: true }],
            };
            session.state_mut::<FoldingGroups>().0.push(OPTIONS);
            if let Some(on) = on {
                session.settings_mut().set(ANGLES, on).unwrap();
            }
            registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
            registry.dispatch(&mut session, "file.document.open", json!({ "path": &path })).unwrap();
            let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
            let options = registry.dispatch(&mut session, "editor.folding.options", json!({})).unwrap();
            (folds[0].clone(), options)
        };
        let (fold, options) = open(None);
        assert_eq!(
            (fold["folded"].clone(), fold["placeholder"].clone()),
            (json!(true), json!(" tagged ")),
            "on by default"
        );
        assert_eq!(fold["option"], ANGLES);
        assert_eq!(options["groups"][1]["options"][0]["key"], ANGLES);
        assert_eq!(options["groups"][1]["options"][0]["value"], true);
        assert_eq!(options["groups"][0]["options"].as_array().unwrap().len(), GENERAL.options.len());
        assert_eq!((options["showArrows"].clone(), options["arrows"].clone()), (json!(true), json!("onHover")));
        assert_eq!(options["bottomArrows"], false);
        let (fold, options) = open(Some(false));
        assert_eq!(fold["folded"], false, "turned off");
        assert_eq!(options["groups"][1]["options"][0]["value"], false);
    }

    #[test]
    fn the_setting_keeps_imports_open() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, GO).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.settings_mut().set(FOLD_IMPORTS, false).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(folds[0]["folded"], false);
    }
}

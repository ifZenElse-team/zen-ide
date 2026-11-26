use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};

use super::{Block, Container, Rendered};

const THIN_SPACE: char = '\u{2009}';

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Part {
    pub block: usize,
    pub row: usize,
    pub column: usize,
}

impl Part {
    pub fn of(block: usize) -> Self {
        Part { block, row: 0, column: 0 }
    }

    pub fn cell(block: usize, row: usize, column: usize) -> Self {
        Part { block, row, column }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spot {
    pub part: Part,
    pub at: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub part: Part,
    pub text: String,
    chars: usize,
}

impl Fragment {
    pub fn new(part: Part, text: impl Into<String>) -> Self {
        let text = text.into();
        Fragment { part, chars: text.chars().count(), text }
    }

    pub fn len(&self) -> usize {
        self.chars
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Spot,
    pub head: Spot,
}

impl Selection {
    pub fn new(anchor: Spot, head: Spot) -> Self {
        Selection { anchor, head }
    }

    pub fn caret(spot: Spot) -> Self {
        Selection { anchor: spot, head: spot }
    }

    pub fn ordered(&self) -> (Spot, Spot) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn range_in(&self, fragment: &Fragment) -> Option<(usize, usize)> {
        let (start, end) = self.ordered();
        if self.is_empty() || fragment.part < start.part || fragment.part > end.part {
            return None;
        }
        let len = fragment.len();
        let from = if fragment.part == start.part { start.at.min(len) } else { 0 };
        let to = if fragment.part == end.part { end.at.min(len) } else { len };
        (from < to || (from == to && fragment.part != end.part)).then_some((from, to))
    }
}

pub fn select_all(fragments: &[Fragment]) -> Option<Selection> {
    let first = fragments.first()?;
    let last = fragments.last()?;
    Some(Selection::new(Spot { part: first.part, at: 0 }, Spot { part: last.part, at: last.len() }))
}

pub fn select_block(fragments: &[Fragment], block: usize) -> Option<Selection> {
    let first = fragments.iter().find(|fragment| fragment.part.block == block)?;
    let last = fragments.iter().rfind(|fragment| fragment.part.block == block)?;
    Some(Selection::new(Spot { part: first.part, at: 0 }, Spot { part: last.part, at: last.len() }))
}

pub fn select_word(fragments: &[Fragment], spot: Spot) -> Selection {
    let Some(fragment) = fragments.iter().find(|fragment| fragment.part == spot.part) else {
        return Selection::caret(spot);
    };
    let chars: Vec<char> = fragment.text.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut at = spot.at.min(chars.len());
    if at == chars.len() && at > 0 {
        at -= 1;
    }
    let Some(&c) = chars.get(at) else {
        return Selection::caret(Spot { part: spot.part, at });
    };
    if !word(c) {
        return Selection::new(Spot { part: spot.part, at }, Spot { part: spot.part, at: at + 1 });
    }
    let start = chars[..at].iter().rposition(|c| !word(*c)).map_or(0, |found| found + 1);
    let end = chars[at..].iter().position(|c| !word(*c)).map_or(chars.len(), |found| at + found);
    Selection::new(Spot { part: spot.part, at: start }, Spot { part: spot.part, at: end })
}

pub fn selected_text(rendered: &Rendered, fragments: &[Fragment], selection: &Selection) -> String {
    let mut out = String::new();
    let mut previous: Option<Part> = None;
    for fragment in fragments {
        let Some((from, to)) = selection.range_in(fragment) else { continue };
        let first_of_block = previous.is_none_or(|previous| previous.block != fragment.part.block);
        if let Some(previous) = previous {
            out.push_str(separator(rendered, previous, fragment.part));
        }
        if first_of_block
            && from == 0
            && let Some(block) = rendered.blocks.get(fragment.part.block)
        {
            out.push_str(&copy_prefix(block));
        }
        out.extend(fragment.text.chars().skip(from).take(to - from).filter(|c| *c != THIN_SPACE));
        previous = Some(fragment.part);
    }
    out
}

pub fn selected_source(source: &str, rendered: &Rendered, selection: &Selection) -> String {
    if selection.is_empty() {
        return String::new();
    }
    let (start, end) = selection.ordered();
    let (Some(first), Some(last)) = (rendered.blocks.get(start.part.block), rendered.blocks.get(end.part.block)) else {
        return String::new();
    };
    let from = first.line.max(1) as usize - 1;
    let to = last.end_line.max(last.line) as usize;
    source.lines().skip(from).take(to.saturating_sub(from)).collect::<Vec<_>>().join("\n")
}

fn in_list(block: Option<&Block>) -> bool {
    block.is_some_and(|block| block.nesting.iter().any(|container| matches!(container, Container::Item { .. })))
}

fn separator(rendered: &Rendered, previous: Part, next: Part) -> &'static str {
    if previous.block == next.block {
        return if previous.row == next.row { "\t" } else { "\n" };
    }
    if in_list(rendered.blocks.get(previous.block)) && in_list(rendered.blocks.get(next.block)) { "\n" } else { "\n\n" }
}

fn copy_prefix(block: &Block) -> String {
    let mut prefix = String::new();
    for container in &block.nesting {
        match container {
            Container::Item { marker: Some(marker), task } => match task {
                Some(task) => prefix.push_str(if task.checked { "[x] " } else { "[ ] " }),
                None => {
                    prefix.push_str(marker);
                    prefix.push(' ');
                }
            },
            Container::Item { .. } => prefix.push_str("  "),
            Container::Footnote { number, first: true } => prefix.push_str(&format!("[{number}] ")),
            Container::Quote { .. } | Container::Footnote { .. } => {}
        }
    }
    prefix
}

#[derive(Default)]
pub struct PreviewSelection {
    shown: Option<SelectionAnswer>,
}

impl PreviewSelection {
    pub fn set(&mut self, path: &Path, text: String, markdown: String) {
        self.shown = Some(SelectionAnswer { path: Some(path.to_owned()), text, markdown });
    }

    pub fn clear(&mut self) {
        self.shown = None;
    }
}

/// The parameters of `markdown.preview.getSelection`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectionParams {
    /// Only a selection in the preview of this file (absolute, or relative to the project).
    pub path: Option<PathBuf>,
}

/// What `markdown.preview.getSelection` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectionAnswer {
    /// The file whose preview holds the selection; absent when nothing is selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The selected text as ⌘C copies it: list markers and `[x]` boxes kept, table cells split by tabs.
    pub text: String,
    /// The Markdown source lines of the selected blocks, as Copy as Markdown copies them.
    pub markdown: String,
}

pub struct GetSelection;

impl Command for GetSelection {
    type Params = SelectionParams;
    type Output = SelectionAnswer;
    const ID: &'static str = "markdown.preview.getSelection";
    const TITLE: &'static str = "Selection in the Markdown Preview";
    const DESCRIPTION: &'static str = "The text selected in the Markdown preview (by dragging, a double or triple click, \
         or view.markdownPreview.selectAll), as plain text and as the Markdown source lines of the selected blocks. \
         Empty when nothing is selected or the preview is not shown.";

    fn run(&self, session: &mut Session, params: SelectionParams) -> Result<SelectionAnswer, CommandError> {
        let wanted = match params.path {
            Some(path) if path.is_relative() => {
                session.state::<crate::Workbench>().and_then(crate::Workbench::project).map(|root| root.join(&path))
            }
            other => other,
        };
        let shown = session.state::<PreviewSelection>().and_then(|selection| selection.shown.clone());
        Ok(match shown {
            Some(shown) if wanted.is_none() || wanted == shown.path => shown,
            _ => SelectionAnswer::default(),
        })
    }
}

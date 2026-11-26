use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_editor::{Selection, Selections};
use zen_i18n::t;
use zen_text::{Edit, Text};

use crate::Workbench;
use crate::preview::PreviewKind;

const MAX_HEADING: u8 = 6;

pub const HIDE_FLOATING_TOOLBAR: &str = "markdown.hideFloatingToolbar";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Strikethrough,
    Code,
}

impl Style {
    fn marker(self) -> &'static str {
        match self {
            Style::Bold => "**",
            Style::Italic => "*",
            Style::Strikethrough => "~~",
            Style::Code => "`",
        }
    }

    fn applied(self, run: usize) -> bool {
        match self {
            Style::Bold | Style::Strikethrough => run >= 2,
            Style::Italic => run % 2 == 1,
            Style::Code => run >= 1,
        }
    }
}

/// A kind of list item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ListKind {
    /// `- item`
    Bullet,
    /// `1. item`
    Numbered,
    /// `- [ ] item`
    Check,
}

type Replacement = (Range<usize>, String);

type Output = (Vec<Edit>, Selections);

struct Change {
    replacements: Vec<Replacement>,
    selected: Range<usize>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn run_before(chars: &[char], at: usize, c: char) -> usize {
    chars[..at].iter().rev().take_while(|found| **found == c).count()
}

fn run_after(chars: &[char], at: usize, c: char) -> usize {
    chars[at..].iter().take_while(|found| **found == c).count()
}

fn word_at(chars: &[char], at: usize) -> Range<usize> {
    let start = at - chars[..at].iter().rev().take_while(|c| is_word(**c)).count();
    let end = at + chars[at..].iter().take_while(|c| is_word(**c)).count();
    start..end
}

fn styled(chars: &[char], range: Range<usize>, style: Style) -> Change {
    let marker = style.marker();
    let length = marker.chars().count();
    let c = marker.chars().next().unwrap_or('*');
    let Range { start, end } = if range.is_empty() { word_at(chars, range.start) } else { range };
    let outside = run_before(chars, start, c).min(run_after(chars, end, c));
    if style.applied(outside) {
        return Change {
            replacements: vec![(start - length..start, String::new()), (end..end + length, String::new())],
            selected: start - length..end - length,
        };
    }
    let inside = run_after(&chars[..end], start, c).min(run_before(&chars[start..], end - start, c));
    if end - start >= 2 * length && style.applied(inside) {
        return Change {
            replacements: vec![(start..start + length, String::new()), (end - length..end, String::new())],
            selected: start..end - 2 * length,
        };
    }
    Change {
        replacements: vec![(start..start, marker.to_owned()), (end..end, marker.to_owned())],
        selected: start + length..end + length,
    }
}

fn linked(chars: &[char], range: Range<usize>, url: Option<&str>) -> Change {
    let Range { start, end } = if range.is_empty() { word_at(chars, range.start) } else { range };
    let url = url.unwrap_or_default();
    let close = format!("]({url})");
    let caret = end + 1 + 2 + url.chars().count();
    Change { replacements: vec![(start..start, "[".to_owned()), (end..end, close)], selected: caret..caret }
}

pub fn toggle_style(text: &Text, selections: &Selections, style: Style) -> Output {
    around_selections(text, selections, |chars, range| styled(chars, range, style))
}

pub fn link(text: &Text, selections: &Selections, url: Option<&str>) -> Output {
    around_selections(text, selections, |chars, range| linked(chars, range, url))
}

fn around_selections(text: &Text, selections: &Selections, change: impl Fn(&[char], Range<usize>) -> Change) -> Output {
    let chars: Vec<char> = text.to_string().chars().collect();
    let mut order: Vec<usize> = (0..selections.len()).collect();
    let items: Vec<Selection> = selections.iter().copied().collect();
    order.sort_by_key(|index| items[*index].range().start);
    let mut replacements: Vec<Replacement> = Vec::new();
    let mut placed = items.clone();
    let mut shift = 0isize;
    for index in order {
        let Change { replacements: own, selected } = change(&chars, items[index].range());
        let at = |offset: usize| (offset as isize + shift).max(0) as usize;
        placed[index] = Selection::new(at(selected.start), at(selected.end));
        shift += own.iter().map(|(range, text)| text.chars().count() as isize - range.len() as isize).sum::<isize>();
        replacements.extend(own);
    }
    replacements.sort_by_key(|(range, _)| range.start);
    (sequential(replacements), Selections::from_vec(placed, selections.primary_index()))
}

fn sequential(replacements: Vec<Replacement>) -> Vec<Edit> {
    replacements.into_iter().rev().map(|(range, text)| Edit::replace(range, text)).collect()
}

struct Line {
    start: usize,
    text: String,
}

fn selected_lines(text: &Text, selections: &Selections) -> Vec<Line> {
    let all = text.to_string();
    let mut lines = Vec::new();
    let mut start = 0;
    for line in all.split('\n') {
        let count = line.chars().count();
        let end = start + count;
        let touched = selections.iter().any(|selection| {
            let range = selection.range();
            if range.is_empty() {
                (start..=end).contains(&range.start)
            } else {
                range.start <= end && range.end > start
            }
        });
        if touched {
            lines.push(Line { start, text: line.strip_suffix('\r').unwrap_or(line).to_owned() });
        }
        start = end + 1;
    }
    lines
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

fn heading_prefix(body: &[char]) -> usize {
    let hashes = body.iter().take_while(|c| **c == '#').count();
    if (1..=MAX_HEADING as usize).contains(&hashes) && body.get(hashes) == Some(&' ') {
        hashes + 1 + body[hashes + 1..].iter().take_while(|c| **c == ' ').count()
    } else {
        0
    }
}

fn list_prefix(body: &[char]) -> Option<(ListKind, usize)> {
    let digits = body.iter().take_while(|c| c.is_ascii_digit()).count();
    let (kind, marker) = if matches!(body.first(), Some('-' | '*' | '+')) {
        (ListKind::Bullet, 1)
    } else if digits > 0 && matches!(body.get(digits), Some('.' | ')')) {
        (ListKind::Numbered, digits + 1)
    } else {
        return None;
    };
    if body.get(marker) != Some(&' ') {
        return None;
    }
    let after = marker + 1;
    let tick = body
        .get(after..after + 3)
        .is_some_and(|tick| tick[0] == '[' && matches!(tick[1], ' ' | 'x' | 'X') && tick[2] == ']');
    if tick && body.get(after + 3) == Some(&' ') {
        return Some((ListKind::Check, after + 4));
    }
    Some((kind, after))
}

fn map_offset(offset: usize, replacements: &[Replacement]) -> usize {
    let mut mapped = offset as isize;
    for (range, text) in replacements {
        let inserted = text.chars().count() as isize;
        if range.end <= offset {
            mapped += inserted - range.len() as isize;
        } else if range.start < offset {
            mapped += range.start as isize + inserted - offset as isize;
        }
    }
    mapped.max(0) as usize
}

fn prefixed(selections: &Selections, replacements: Vec<Replacement>) -> Output {
    let items = selections
        .iter()
        .map(|selection| {
            Selection::new(map_offset(selection.anchor, &replacements), map_offset(selection.head, &replacements))
        })
        .collect();
    (sequential(replacements), Selections::from_vec(items, selections.primary_index()))
}

fn filled(lines: Vec<Line>) -> Vec<Line> {
    if lines.iter().all(|line| line.text.trim().is_empty()) {
        return lines;
    }
    lines.into_iter().filter(|line| !line.text.trim().is_empty()).collect()
}

pub fn heading_level(text: &Text, selections: &Selections) -> u8 {
    let lines = selected_lines(text, &Selections::single(selections.primary()));
    let Some(line) = lines.first() else { return 0 };
    let chars: Vec<char> = line.text.chars().skip(indent_of(&line.text)).collect();
    let prefix = heading_prefix(&chars);
    if prefix == 0 { 0 } else { chars.iter().take_while(|c| **c == '#').count() as u8 }
}

pub fn heading(text: &Text, selections: &Selections, level: u8) -> Output {
    let wanted = if level == 0 { String::new() } else { format!("{} ", "#".repeat(level.into())) };
    let replacements = filled(selected_lines(text, selections))
        .into_iter()
        .map(|line| {
            let chars: Vec<char> = line.text.chars().collect();
            let indent = indent_of(&line.text);
            let from = line.start + indent;
            (from..from + heading_prefix(&chars[indent..]), wanted.clone())
        })
        .collect();
    prefixed(selections, replacements)
}

pub fn list(text: &Text, selections: &Selections, kind: ListKind) -> Output {
    let lines = filled(selected_lines(text, selections));
    let bodies: Vec<(usize, Vec<char>)> = lines
        .iter()
        .map(|line| {
            let indent = indent_of(&line.text);
            (indent, line.text.chars().skip(indent).collect())
        })
        .collect();
    let remove = bodies.iter().all(|(_, body)| list_prefix(body).is_some_and(|(found, _)| found == kind));
    let replacements = lines
        .iter()
        .zip(&bodies)
        .enumerate()
        .map(|(index, (line, (indent, body)))| {
            let from = line.start + indent;
            let length = list_prefix(body).map_or(0, |(_, length)| length);
            let wanted = match kind {
                _ if remove => String::new(),
                ListKind::Bullet => "- ".to_owned(),
                ListKind::Numbered => format!("{}. ", index + 1),
                ListKind::Check => "- [ ] ".to_owned(),
            };
            (from..from + length, wanted)
        })
        .collect();
    prefixed(selections, replacements)
}

fn markdown_document(session: &mut Session) -> Result<&mut crate::Document, CommandError> {
    session
        .state_mut::<Workbench>()
        .active_document_mut()
        .filter(|document| PreviewKind::of(document.path()) == Some(PreviewKind::Markdown))
        .ok_or_else(|| CommandError::new(t!("workbench-markdown-not-markdown")))
}

fn in_markdown(session: &Session) -> bool {
    session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .is_some_and(|document| PreviewKind::of(document.path()) == Some(PreviewKind::Markdown))
}

/// What the formatting commands answer.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Formatted {
    /// Whether the text changed.
    pub changed: bool,
    /// The selection after the change: its start and end offsets in characters.
    pub selection: (usize, usize),
}

fn apply(session: &mut Session, change: impl FnOnce(&Text, &Selections) -> Output) -> Result<Formatted, CommandError> {
    let document = markdown_document(session)?;
    let (edits, selections) = change(document.snapshot(), document.selections());
    let changed = document.apply_action(edits, selections);
    let primary = document.selections().primary().range();
    Ok(Formatted { changed, selection: (primary.start, primary.end) })
}

macro_rules! style_command {
    ($name:ident, $id:literal, $title:literal, $style:expr, $description:literal) => {
        #[doc = concat!("`", $id, "`")]
        pub struct $name;

        impl Command for $name {
            type Params = NoParams;
            type Output = Formatted;
            const ID: &'static str = $id;
            const TITLE: &'static str = $title;
            const DESCRIPTION: &'static str = $description;

            fn is_available(&self, session: &Session) -> bool {
                in_markdown(session)
            }

            fn run(&self, session: &mut Session, _: NoParams) -> Result<Formatted, CommandError> {
                apply(session, |text, selections| toggle_style(text, selections, $style))
            }
        }
    };
}

style_command!(
    Bold,
    "markdown.format.bold",
    "Bold",
    Style::Bold,
    "Makes the selection, or the word at the caret, of the active Markdown file bold, or plain when it is bold (⌘B)."
);
style_command!(
    Italic,
    "markdown.format.italic",
    "Italic",
    Style::Italic,
    "Makes the selection, or the word at the caret, of the active Markdown file italic, or plain when it is italic (⌘I)."
);
style_command!(
    Strikethrough,
    "markdown.format.strikethrough",
    "Strikethrough",
    Style::Strikethrough,
    "Strikes the selection, or the word at the caret, of the active Markdown file through, or removes the strike (⇧⌘S)."
);
style_command!(
    Code,
    "markdown.format.code",
    "Code",
    Style::Code,
    "Makes the selection, or the word at the caret, of the active Markdown file inline code, or plain when it is (⇧⌘C)."
);

/// The parameters of `markdown.format.link`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkParams {
    /// The link's address; empty, with the caret left there to type it, when absent.
    pub url: Option<String>,
}

pub struct CreateLink;

impl Command for CreateLink {
    type Params = LinkParams;
    type Output = Formatted;
    const ID: &'static str = "markdown.format.link";
    const TITLE: &'static str = "Create Link";
    const DESCRIPTION: &'static str = "Makes the selection, or the word at the caret, of the active Markdown file the text \
         of a link to url; without url the caret goes where the address is typed (⇧⌘U).";

    fn is_available(&self, session: &Session) -> bool {
        in_markdown(session)
    }

    fn run(&self, session: &mut Session, params: LinkParams) -> Result<Formatted, CommandError> {
        apply(session, |text, selections| link(text, selections, params.url.as_deref()))
    }
}

/// The parameters of `markdown.format.heading`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HeadingParams {
    /// From 1 to 6; 0 makes the lines plain text.
    pub level: u8,
}

pub struct SetHeading;

impl Command for SetHeading {
    type Params = HeadingParams;
    type Output = Formatted;
    const ID: &'static str = "markdown.format.heading";
    const TITLE: &'static str = "Set Heading Level";
    const DESCRIPTION: &'static str = "Makes the lines of the selection in the active Markdown file headings of level 1 to \
         6, or plain text with level 0, as Text and H1–H6 of the floating toolbar do.";

    fn is_available(&self, session: &Session) -> bool {
        in_markdown(session)
    }

    fn run(&self, session: &mut Session, params: HeadingParams) -> Result<Formatted, CommandError> {
        if params.level > MAX_HEADING {
            return Err(CommandError::new(t!("workbench-markdown-heading-level", level = params.level)));
        }
        apply(session, |text, selections| heading(text, selections, params.level))
    }
}

/// The parameters of `markdown.format.list`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// `bullet`, `numbered` or `check`.
    pub kind: ListKind,
}

pub struct ToggleList;

impl Command for ToggleList {
    type Params = ListParams;
    type Output = Formatted;
    const ID: &'static str = "markdown.format.list";
    const TITLE: &'static str = "Toggle List";
    const DESCRIPTION: &'static str = "Makes the lines of the selection in the active Markdown file items of a bullet, \
         numbered or check list, or plain text when they all already are of that kind.";

    fn is_available(&self, session: &Session) -> bool {
        in_markdown(session)
    }

    fn run(&self, session: &mut Session, params: ListParams) -> Result<Formatted, CommandError> {
        apply(session, |text, selections| list(text, selections, params.kind))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(Bold)?;
    registry.register(Italic)?;
    registry.register(Strikethrough)?;
    registry.register(Code)?;
    registry.register(CreateLink)?;
    registry.register(SetHeading)?;
    registry.register(ToggleList)?;
    Ok(())
}

#[cfg(test)]
mod tests;

use std::ops::Range;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::HighlightKind;

use crate::{Document, Workbench};

pub const REMOVE_TRAILING_SPACES: &str = "editor.onSave.removeTrailingSpaces";

pub const REMOVE_TRAILING_SPACES_ON: &str = "editor.onSave.removeTrailingSpacesOn";

pub const KEEP_CARET_LINES: &str = "editor.onSave.keepTrailingSpacesOnCaretLine";

pub const REMOVE_TRAILING_BLANK_LINES: &str = "editor.onSave.removeTrailingBlankLines";

pub const ENSURE_LINE_BREAK: &str = "editor.onSave.ensureLineBreak";

/// Which lines lose their trailing spaces when a file is saved, as Settings → Editor → General → On Save chooses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TrailingSpaces {
    /// The lines changed since the file was last saved or opened.
    #[default]
    Modified,
    /// Every line.
    All,
}

/// Which lines of a file lose their trailing spaces on save, everything considered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Removal {
    /// None.
    Off,
    /// The lines changed since the file was last saved or opened.
    #[default]
    Modified,
    /// Every line.
    All,
}

/// The parameters of `editor.onSave.options`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct OptionsParams {
    /// The file, absolute or from the project's folder; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// What saving a file does to its whitespace: the settings with the file's EditorConfig applied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    /// The file.
    pub path: PathBuf,
    /// Which lines lose their trailing spaces.
    pub remove_trailing_spaces: Removal,
    /// Whether the lines with a caret keep their trailing spaces.
    pub keep_caret_lines: bool,
    /// Whether the blank lines at the end of the file go.
    pub remove_trailing_blank_lines: bool,
    /// Whether the file gets a line break at its end when it has none.
    pub ensure_line_break: bool,
    /// The file's EditorConfig `trim_trailing_whitespace`, when its `.editorconfig` files set it.
    pub trim_trailing_whitespace: Option<bool>,
    /// The file's EditorConfig `insert_final_newline`, when its `.editorconfig` files set it.
    pub insert_final_newline: Option<bool>,
}

pub fn options(session: &Session, path: &Path) -> Options {
    let settings = session.settings();
    let remove_trailing_spaces = if settings.get_or(REMOVE_TRAILING_SPACES, true) {
        match settings.get_or(REMOVE_TRAILING_SPACES_ON, TrailingSpaces::Modified) {
            TrailingSpaces::Modified => Removal::Modified,
            TrailingSpaces::All => Removal::All,
        }
    } else {
        Removal::Off
    };
    let properties = crate::editor_config::save_properties(session, path);
    let (trim_trailing_whitespace, insert_final_newline) =
        (properties.trim_trailing_whitespace, properties.insert_final_newline);
    Options {
        path: path.to_owned(),
        remove_trailing_spaces: match trim_trailing_whitespace {
            Some(true) => Removal::All,
            Some(false) => Removal::Off,
            None => remove_trailing_spaces,
        },
        keep_caret_lines: settings.get_or(KEEP_CARET_LINES, true),
        remove_trailing_blank_lines: settings.get_or(REMOVE_TRAILING_BLANK_LINES, false),
        ensure_line_break: insert_final_newline.unwrap_or_else(|| settings.get_or(ENSURE_LINE_BREAK, false)),
        trim_trailing_whitespace,
        insert_final_newline,
    }
}

#[derive(Clone, Copy, Debug)]
struct Line {
    start: usize,
    content_end: usize,
    end: usize,
}

fn lines(text: &str) -> Vec<Line> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut start, mut at) = (0, 0);
    while at < bytes.len() {
        let end = match bytes[at] {
            b'\r' if bytes.get(at + 1) == Some(&b'\n') => at + 2,
            b'\r' | b'\n' => at + 1,
            _ => {
                at += 1;
                continue;
            }
        };
        lines.push(Line { start, content_end: at, end });
        start = end;
        at = end;
    }
    lines.push(Line { start, content_end: bytes.len(), end: bytes.len() });
    lines
}

fn trailing_start(text: &str, line: Line) -> usize {
    let bytes = text.as_bytes();
    let mut at = line.content_end;
    while at > line.start && matches!(bytes[at - 1], b' ' | b'\t') {
        at -= 1;
    }
    at
}

fn joins_separators(text: &str, line: Line) -> bool {
    let bytes = text.as_bytes();
    line.start > 0 && bytes[line.start - 1] == b'\r' && bytes.get(line.content_end) == Some(&b'\n')
}

fn line_break(text: &str) -> &'static str {
    match text.find(['\n', '\r']) {
        Some(at) if text[at..].starts_with("\r\n") => "\r\n",
        Some(at) if text.as_bytes()[at] == b'\r' => "\r",
        _ => "\n",
    }
}

fn line_keys(text: &str, lines: &[Line]) -> String {
    let mut keys = String::with_capacity(text.len() + lines.len());
    for line in lines {
        keys.push_str(&text[line.start..line.content_end]);
        keys.push('\n');
    }
    keys
}

fn modified_lines(saved: &str, text: &str, lines: &[Line]) -> Vec<bool> {
    let mut modified = vec![false; lines.len()];
    for changed in zen_vcs::diff::changed_lines(&line_keys(saved, &self::lines(saved)), &line_keys(text, lines)) {
        modified[changed.start.min(lines.len())..changed.end.min(lines.len())].fill(true);
    }
    modified
}

pub struct Cleanup<'a> {
    pub saved: &'a str,
    pub carets: &'a [usize],
    pub significant: &'a dyn Fn(usize) -> bool,
}

pub fn replacements(text: &str, options: &Options, cleanup: &Cleanup<'_>) -> Vec<(Range<usize>, String)> {
    let lines = lines(text);
    let line_of = |offset: usize| lines.partition_point(|line| line.end <= offset).min(lines.len() - 1);
    let mut caret_lines: Vec<usize> = if options.keep_caret_lines {
        cleanup.carets.iter().map(|&caret| line_of(caret)).collect()
    } else {
        Vec::new()
    };
    caret_lines.sort_unstable();
    caret_lines.dedup();
    let tail = if options.remove_trailing_blank_lines {
        let filled = lines.iter().rposition(|line| trailing_start(text, *line) > line.start);
        let kept = filled.into_iter().chain(caret_lines.last().copied()).max();
        Some(kept.map_or(0, |line| lines[line].end)).filter(|from| *from < text.len())
    } else {
        None
    };
    let modified = match options.remove_trailing_spaces {
        Removal::Modified => modified_lines(cleanup.saved, text, &lines),
        Removal::Off | Removal::All => Vec::new(),
    };
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    if options.remove_trailing_spaces != Removal::Off {
        for (index, line) in lines.iter().enumerate() {
            if tail.is_some_and(|tail| line.start >= tail) {
                break;
            }
            let mut from = trailing_start(text, *line);
            if from == line.start && joins_separators(text, *line) {
                from += 1;
            }
            if from == line.content_end || caret_lines.binary_search(&index).is_ok() {
                continue;
            }
            let changed = options.remove_trailing_spaces == Removal::All || modified[index];
            if changed && !(cleanup.significant)(from) {
                edits.push((from..line.content_end, String::new()));
            }
        }
    }
    if let Some(tail) = tail {
        edits.push((tail..text.len(), String::new()));
    }
    if options.ensure_line_break {
        let mut result = text.to_owned();
        for (range, inserted) in edits.iter().rev() {
            result.replace_range(range.clone(), inserted);
        }
        if !result.is_empty() && !result.ends_with(['\n', '\r']) {
            let added = line_break(text);
            match edits.last_mut() {
                Some((range, inserted)) if range.end == text.len() => inserted.push_str(added),
                _ => edits.push((text.len()..text.len(), added.to_owned())),
            }
        }
    }
    edits
}

fn markdown_break(text: &str, from: usize) -> bool {
    let start = text[..from].rfind(['\n', '\r']).map_or(0, |at| at + 1);
    let end = text[from..].find(['\n', '\r']).map_or(text.len(), |at| from + at);
    from > start && text[from..end].ends_with("  ")
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd"))
}

fn read_only_on_disk(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().readonly())
}

fn char_offsets(text: &str, bytes: &[usize]) -> Vec<usize> {
    let mut sorted: Vec<(usize, usize)> = bytes.iter().copied().zip(0..).collect();
    sorted.sort_unstable();
    let mut result = vec![0; bytes.len()];
    let (mut chars, mut at) = (0, 0);
    for (byte, index) in sorted {
        chars += text[at..byte].chars().count();
        at = byte;
        result[index] = chars;
    }
    result
}

fn byte_offsets(text: &str, chars: &[usize]) -> Vec<usize> {
    let mut sorted: Vec<(usize, usize)> = chars.iter().copied().zip(0..).collect();
    sorted.sort_unstable();
    let mut result = vec![text.len(); chars.len()];
    let mut starts =
        text.char_indices().map(|(byte, _)| byte).chain(std::iter::once(text.len())).enumerate().peekable();
    for (char, index) in sorted {
        while starts.next_if(|(at, _)| *at < char).is_some() {}
        result[index] = starts.peek().map_or(text.len(), |(_, byte)| *byte);
    }
    result
}

fn caret_offsets(document: &Document) -> Vec<usize> {
    let views = document.views.iter().flat_map(|(_, view)| view.selections.iter().map(|selection| selection.head));
    document.selections.iter().map(|selection| selection.head).chain(views).collect()
}

fn planned(session: &Session, path: &Path) -> Option<Vec<(Range<usize>, String)>> {
    let workbench = session.state::<Workbench>()?;
    let document = workbench.documents.iter().find(|document| document.path == path)?;
    if !document.dirty || document.image.is_some() || document.read_only.is_some() || read_only_on_disk(path) {
        return None;
    }
    let options = options(session, path);
    let text = document.text();
    let saved = document.saved.to_string();
    let carets = byte_offsets(text, &caret_offsets(document));
    let highlighting = session
        .state::<crate::brackets::Highlighters>()
        .and_then(|highlighters| highlighters.highlighting_of(document));
    let highlighting = highlighting
        .as_ref()
        .map(|highlighting| highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    let markdown = is_markdown(path);
    let significant = |from: usize| {
        markdown && markdown_break(text, from)
            || highlighting
                .as_ref()
                .is_some_and(|highlighting| highlighting.kind_at(text, from) == Some(HighlightKind::String))
    };
    let edits = replacements(text, &options, &Cleanup { saved: &saved, carets: &carets, significant: &significant });
    let points: Vec<usize> = edits.iter().flat_map(|(range, _)| [range.start, range.end]).collect();
    let chars = char_offsets(text, &points);
    Some(edits.into_iter().zip(chars.chunks(2)).map(|((_, inserted), pair)| (pair[0]..pair[1], inserted)).collect())
}

fn keep_before(selections: &zen_editor::Selections, end: usize, added: usize) -> zen_editor::Selections {
    let back = |offset: usize| if offset == end { end - added } else { offset };
    let items = selections
        .iter()
        .map(|selection| zen_editor::Selection::new(back(selection.anchor), back(selection.head)))
        .collect();
    zen_editor::Selections::from_vec(items, selections.primary_index())
}

pub fn before_save(session: &mut Session, path: &Path) -> bool {
    let Some(edits) = planned(session, path).filter(|edits| !edits.is_empty()) else { return false };
    let workbench = session.state_mut::<Workbench>();
    let Some(document) = workbench.documents.iter_mut().find(|document| document.path == path) else { return false };
    let length = document.buffer.text().len_chars();
    let added = edits
        .last()
        .filter(|(range, inserted)| range.end == length && inserted.ends_with(['\n', '\r']))
        .map(|(_, inserted)| inserted.chars().count());
    if !document.replace_ranges(edits) {
        return false;
    }
    if let Some(added) = added {
        let end = document.buffer.text().len_chars();
        document.selections = keep_before(&document.selections, end, added);
        document.selection_version += 1;
        for (_, view) in &mut document.views {
            view.selections = keep_before(&view.selections, end, added);
        }
    }
    true
}

pub struct ShowOptions;

impl Command for ShowOptions {
    type Params = OptionsParams;
    type Output = Options;
    const ID: &'static str = "editor.onSave.options";
    const TITLE: &'static str = "On Save Options";
    const DESCRIPTION: &'static str = "What saving a file (the active one by default) does to its whitespace (Settings → \
         Editor → General → On Save): from which lines trailing spaces go (off; modified: the lines changed since the \
         last save; all), whether caret lines keep them, whether trailing blank lines go and whether a final line break \
         is added. set_settings changes them (removeTrailingSpaces, removeTrailingSpacesOn, \
         keepTrailingSpacesOnCaretLine, removeTrailingBlankLines, ensureLineBreakAtEnd); the project's .editorconfig \
         trim_trailing_whitespace and insert_final_newline override them. Saving applies them as one undo step.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<Options, CommandError> {
        let workbench = session.state::<Workbench>();
        let path = match params.path {
            Some(path) if path.is_relative() => {
                workbench.and_then(Workbench::project).map_or_else(|| path.clone(), |project| project.join(&path))
            }
            Some(path) => path,
            None => workbench
                .and_then(Workbench::active_document)
                .map(|document| document.path.clone())
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        };
        Ok(options(session, &path))
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ShowOptions)
}

#[cfg(test)]
mod tests;

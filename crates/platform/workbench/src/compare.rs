use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::vcs::commit::{DiffHunk, DiffView, diff_hunks, diff_view_with};
use crate::{Workbench, display, file_clipboard};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CompareThree)?;
    registry.register(CompareClipboard)?;
    registry.register(CompareTexts)?;
    registry.register(WriteSide)?;
    Ok(())
}

/// How the diff viewer treats whitespace, as its Ignore whitespace chooser offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Whitespace {
    /// Every character counts.
    #[default]
    None,
    /// Whitespace at the start and the end of lines does not count.
    Trim,
    /// No whitespace counts.
    Ignore,
}

impl Whitespace {
    pub const ALL: [Whitespace; 3] = [Whitespace::None, Whitespace::Trim, Whitespace::Ignore];

    pub(crate) fn normalized(self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            let body = line.trim_end_matches(['\n', '\r']);
            match self {
                Whitespace::None => out.push_str(body),
                Whitespace::Trim => out.push_str(body.trim()),
                Whitespace::Ignore => out.extend(body.chars().filter(|c| !c.is_whitespace())),
            }
            out.push('\n');
        }
        out
    }

    pub(crate) fn ignores_word(self, lines: &[&str], word: &[usize; 3]) -> bool {
        let Some(line) = lines.get(word[0]) else { return false };
        let line = line.trim_end_matches(['\n', '\r']);
        let length = line.chars().count();
        let blank = line.chars().skip(word[1]).take(word[2].saturating_sub(word[1])).all(char::is_whitespace);
        match self {
            Whitespace::None => false,
            Whitespace::Trim => blank && (word[1] == 0 || word[2] >= length),
            Whitespace::Ignore => blank,
        }
    }
}

pub fn side_text(text: &str, range: Option<[usize; 2]>) -> String {
    let part: String = match range {
        Some([start, end]) => text.chars().skip(start).take(end.saturating_sub(start)).collect(),
        None => text.to_owned(),
    };
    part.replace("\r\n", "\n")
}

fn uses_crlf(text: &str) -> bool {
    text.find('\n').is_some_and(|at| text[..at].ends_with('\r'))
}

fn read(path: &Path) -> Result<Vec<u8>, CommandError> {
    if path.is_dir() {
        return Err(CommandError::new(t!("workbench-error-compare-folder", path = display(path))));
    }
    std::fs::read(path)
        .map_err(|error| CommandError::new(t!("workbench-error-open", path = display(path), error = error.to_string())))
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(|| display(path), |name| name.to_string_lossy().into_owned())
}

/// Three files to compare.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreeParams {
    /// The file shown on the left.
    pub left: PathBuf,
    /// The file shown in the middle, which the other two are compared with.
    pub middle: PathBuf,
    /// The file shown on the right.
    pub right: PathBuf,
    /// How whitespace counts; every character by default.
    #[serde(default)]
    pub whitespace: Whitespace,
}

/// Three texts side by side and how the outer ones differ from the middle one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ThreeWayView {
    /// The files, left to right.
    pub paths: Vec<PathBuf>,
    /// Their titles, left to right.
    pub titles: Vec<String>,
    /// Their texts, left to right; empty when they are not all text.
    pub texts: Vec<String>,
    /// The changes from the left text to the middle one (`before` is the left).
    pub left_hunks: Vec<DiffHunk>,
    /// The changes from the middle text to the right one (`before` is the middle).
    pub right_hunks: Vec<DiffHunk>,
    /// When a file is not text: the viewer says so instead.
    pub binary: bool,
}

impl ThreeWayView {
    pub fn new(paths: Vec<PathBuf>, titles: Vec<String>, texts: Vec<String>, whitespace: Whitespace) -> Self {
        let (left_hunks, right_hunks) = match texts.as_slice() {
            [left, middle, right] => (diff_hunks(left, middle, whitespace), diff_hunks(middle, right, whitespace)),
            _ => (Vec::new(), Vec::new()),
        };
        Self { paths, titles, texts, left_hunks, right_hunks, binary: false }
    }
}

pub struct CompareThree;

impl Command for CompareThree {
    type Params = ThreeParams;
    type Output = ThreeWayView;
    const ID: &'static str = "file.compare.three";
    const TITLE: &'static str = "Compare Three Files";
    const DESCRIPTION: &'static str = "Compares three files side by side, as the three-side diff viewer shows them: the \
         texts, the changes from `left` to `middle` (`leftHunks`) and from `middle` to `right` (`rightHunks`); \
         `binary` when one is not text. `whitespace` (`none`, `trim`, `ignore`) says how whitespace counts.";

    fn run(&self, _session: &mut Session, params: ThreeParams) -> Result<ThreeWayView, CommandError> {
        let paths = vec![params.left, params.middle, params.right];
        let bytes = paths.iter().map(|path| read(path)).collect::<Result<Vec<_>, _>>()?;
        let titles = paths.iter().map(|path| display(path)).collect();
        let texts: Option<Vec<String>> = bytes.into_iter().map(|bytes| String::from_utf8(bytes).ok()).collect();
        Ok(match texts {
            Some(texts) => ThreeWayView::new(paths, titles, texts, params.whitespace),
            None => ThreeWayView { paths, titles, binary: true, ..ThreeWayView::default() },
        })
    }
}

fn yes() -> bool {
    true
}

/// A file to compare with the clipboard.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardParams {
    /// The file; the active document when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The text to compare it with; the clipboard's when absent (the system clipboard in a window,
    /// else the text copied last in the IDE).
    #[serde(default)]
    pub text: Option<String>,
    /// Compare only the selection when the file is the active document and has one; true by default.
    #[serde(default = "yes")]
    pub selection: bool,
    /// How whitespace counts; every character by default.
    #[serde(default)]
    pub whitespace: Whitespace,
}

pub struct CompareClipboard;

impl Command for CompareClipboard {
    type Params = ClipboardParams;
    type Output = DiffView;
    const ID: &'static str = "file.compare.clipboard";
    const TITLE: &'static str = "Compare with Clipboard";
    const DESCRIPTION: &'static str = "Compares the clipboard's text (left, or `text`) with a file as the editor has it \
         (right; the active document by default, opened when needed), or with its selection when it is the active \
         document and has one (`selection: false` compares the whole file). The file's side is editable: \
         file.compare.writeSide types into it (`afterRange` is the selection's range in characters), and \
         file.document.save saves it.";

    fn run(&self, session: &mut Session, params: ClipboardParams) -> Result<DiffView, CommandError> {
        let active = session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
        let path = params
            .path
            .or_else(|| active.clone())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let path = session.state_mut::<Workbench>().resolve(&path);
        if path.is_dir() {
            return Err(CommandError::new(t!("workbench-error-compare-folder", path = display(&path))));
        }
        let selected = active
            .filter(|active| params.selection && *active == path)
            .and_then(|_| session.state::<Workbench>().and_then(Workbench::active_document))
            .map(|document| document.selections().primary().range())
            .filter(|range| !range.is_empty());
        let clipboard = match params.text {
            Some(text) => text,
            None => file_clipboard::clipboard_or_copied_text(session)
                .ok_or_else(|| CommandError::new(t!("workbench-error-clipboard-empty")))?,
        };
        crate::open_document(session, &path)?;
        let workbench = session.state::<Workbench>();
        let document = workbench
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let editable = document.image.is_none() && document.read_only().is_none();
        let range = selected.map(|range| [range.start, range.end]);
        let after = side_text(document.text(), range);
        let title = match range {
            Some(_) => t!("workbench-compare-selection", file = name(&path)),
            None => display(&path),
        };
        let titles = (t!("workbench-compare-clipboard"), title);
        let clipboard = clipboard.replace("\r\n", "\n");
        let mut view = diff_view_with(
            path.clone(),
            name(&path),
            titles,
            clipboard.as_bytes(),
            after.as_bytes(),
            params.whitespace,
        );
        view.after_editable = editable && !view.binary;
        view.after_range = range;
        Ok(view)
    }
}

/// Two texts to compare.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TextsParams {
    /// The text shown on the left.
    pub left: String,
    /// The text shown on the right.
    pub right: String,
    /// The left side's title.
    pub left_title: String,
    /// The right side's title.
    pub right_title: String,
    /// How whitespace counts; every character by default.
    pub whitespace: Whitespace,
}

pub struct CompareTexts;

impl Command for CompareTexts {
    type Params = TextsParams;
    type Output = DiffView;
    const ID: &'static str = "file.compare.texts";
    const TITLE: &'static str = "Compare Texts";
    const DESCRIPTION: &'static str = "Compares two texts as the diff viewer shows them (`left` is the old side): the \
         changed lines and words; `whitespace` (`none`, `trim`, `ignore`) says how whitespace counts. Both sides \
         are editable, as in the blank diff window (view.diff.blank).";

    fn run(&self, _session: &mut Session, params: TextsParams) -> Result<DiffView, CommandError> {
        let titles = (params.left_title, params.right_title);
        let mut view = diff_view_with(
            PathBuf::new(),
            String::new(),
            titles,
            params.left.as_bytes(),
            params.right.as_bytes(),
            params.whitespace,
        );
        view.before_editable = true;
        view.after_editable = true;
        Ok(view)
    }
}

/// Text typed into a file's side of a diff.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteSideParams {
    /// The file: absolute, or relative to the project.
    pub path: PathBuf,
    /// The side's new text.
    pub text: String,
    /// The part of the document the side shows, in characters; the whole text when absent.
    #[serde(default)]
    pub range: Option<[usize; 2]>,
}

/// Where a side of a diff is in its document after typing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SideWritten {
    /// Whether the text changed.
    pub changed: bool,
    /// The part of the document the side shows now, in characters.
    pub range: [usize; 2],
}

pub struct WriteSide;

impl Command for WriteSide {
    type Params = WriteSideParams;
    type Output = SideWritten;
    const ID: &'static str = "file.compare.writeSide";
    const TITLE: &'static str = "Edit Diff Side";
    const DESCRIPTION: &'static str = "Gives a file's side of a diff its new text, as typing there does: the file's open \
         document (opened when needed) gets `text` in place of its `range` (in characters; the whole text when \
         absent) as one undo step that changes only what differs, its line breaks written as the file breaks lines \
         (Windows or Unix), unsaved until file.document.save. Answers `{changed, range}`, the side's range now.";

    fn run(&self, session: &mut Session, params: WriteSideParams) -> Result<SideWritten, CommandError> {
        let path = session.state_mut::<Workbench>().resolve(&params.path);
        let open = session
            .state::<Workbench>()
            .is_some_and(|workbench| workbench.documents().iter().any(|document| document.path() == path));
        if !open {
            crate::open_document(session, &path)?;
        }
        let workbench = session.state_mut::<Workbench>();
        let document = workbench
            .documents
            .iter_mut()
            .find(|document| document.path() == path)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        if document.image.is_some() || document.read_only().is_some() {
            return Err(CommandError::new(t!("workbench-error-compare-read-only", file = name(&path))));
        }
        let text = document.text();
        let length = text.chars().count();
        let [start, end] = params.range.unwrap_or([0, length]);
        let (start, end) = (start.min(length), end.min(length).max(start.min(length)));
        let typed = params.text.replace("\r\n", "\n");
        let typed = if uses_crlf(text) { typed.replace('\n', "\r\n") } else { typed };
        let current: String = text.chars().skip(start).take(end - start).collect();
        let prefix = current.chars().zip(typed.chars()).take_while(|(a, b)| a == b).count();
        let (current_length, typed_length) = (end - start, typed.chars().count());
        let room = current_length.min(typed_length) - prefix;
        let suffix = current.chars().rev().zip(typed.chars().rev()).take(room).take_while(|(a, b)| a == b).count();
        let replaced = start + prefix..end - suffix;
        let inserted: String = typed.chars().skip(prefix).take(typed_length - prefix - suffix).collect();
        let changed =
            (!replaced.is_empty() || !inserted.is_empty()) && document.replace_ranges(vec![(replaced, inserted)]);
        Ok(SideWritten { changed, range: [start, if changed { start + typed_length } else { end }] })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn session() -> (Registry, Session) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        (registry, Session::new())
    }

    #[test]
    fn whitespace_modes_hide_changes_of_whitespace_only() {
        let before = "func a() {\n\treturn 1\n}\n";
        let after = "func a()  {\n    return 1  \n}\n";
        assert_eq!(diff_hunks(before, after, Whitespace::None).len(), 1);
        let trimmed = diff_hunks(before, after, Whitespace::Trim);
        assert_eq!(trimmed.len(), 1, "the inner space of the first line still counts");
        assert_eq!((trimmed[0].before_start, trimmed[0].before_end), (0, 1));
        assert!(diff_hunks(before, after, Whitespace::Ignore).is_empty());
        let words = diff_hunks("a b\n", "a  b c\n", Whitespace::Ignore);
        assert_eq!(words.len(), 1);
        assert!(words[0].after_words.iter().all(|word| word[1] >= 4), "{:?}", words[0].after_words);
    }

    #[test]
    fn three_files_are_compared_with_the_middle_one() {
        let dir = tempfile::tempdir().unwrap();
        let [a, b, c] = ["a.txt", "b.txt", "c.txt"].map(|name| dir.path().join(name));
        std::fs::write(&a, "one\ntwo\n").unwrap();
        std::fs::write(&b, "one\n2\n").unwrap();
        std::fs::write(&c, "one\n2\nthree\n").unwrap();
        let (registry, mut session) = session();
        let view = registry
            .dispatch(&mut session, "file.compare.three", json!({ "left": a, "middle": b, "right": c }))
            .unwrap();
        let view: ThreeWayView = serde_json::from_value(view).unwrap();
        assert_eq!(view.texts.len(), 3);
        assert_eq!(view.left_hunks.len(), 1);
        assert_eq!(view.left_hunks[0].kind, "modified");
        assert_eq!(view.right_hunks.len(), 1);
        assert_eq!(view.right_hunks[0].kind, "added");
        assert!(
            registry
                .dispatch(&mut session, "file.compare.three", json!({ "left": a, "middle": dir.path(), "right": c }))
                .is_err()
        );
    }

    #[test]
    fn the_clipboard_is_compared_with_the_file_or_its_selection_and_the_file_side_is_typed_into() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main\n\nfunc a() {}\n").unwrap();
        let (registry, mut session) = session();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        assert!(
            registry.dispatch(&mut session, "file.compare.clipboard", json!({ "path": file })).is_err(),
            "nothing on the clipboard"
        );
        let clipboard = std::sync::Arc::new(file_clipboard::MemoryClipboard::default());
        file_clipboard::SystemClipboard::set_text(clipboard.as_ref(), "package main\n\nfunc b() {}\n");
        file_clipboard::install(&mut session, clipboard);
        let view = registry.dispatch(&mut session, "file.compare.clipboard", json!({ "path": file })).unwrap();
        let view: DiffView = serde_json::from_value(view).unwrap();
        assert_eq!(view.before, "package main\n\nfunc b() {}\n");
        assert_eq!(view.after, "package main\n\nfunc a() {}\n");
        assert_eq!(view.hunks.len(), 1);
        assert!(view.after_editable && !view.before_editable && view.after_range.is_none());

        let workbench = session.state_mut::<Workbench>();
        let document = workbench.active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::new(14, 25)));
        let view = registry.dispatch(&mut session, "file.compare.clipboard", json!({ "text": "func a() {}" })).unwrap();
        let view: DiffView = serde_json::from_value(view).unwrap();
        assert_eq!(view.after, "func a() {}");
        assert_eq!(view.after_range, Some([14, 25]));
        assert!(view.hunks.is_empty());

        let written = registry
            .dispatch(
                &mut session,
                "file.compare.writeSide",
                json!({ "path": file, "text": "func c() {}", "range": [14, 25] }),
            )
            .unwrap();
        assert_eq!(written, json!({ "changed": true, "range": [14, 25] }));
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "package main\n\nfunc c() {}\n");
        registry.dispatch(&mut session, "file.document.save", json!({})).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n\nfunc c() {}\n");
    }

    #[test]
    fn the_file_side_keeps_windows_line_breaks_and_changes_only_what_differs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("crlf.txt");
        std::fs::write(&file, "one\r\ntwo\r\nthree\r\n").unwrap();
        let (registry, mut session) = session();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let view = registry
            .dispatch(&mut session, "file.compare.clipboard", json!({ "path": file, "text": "one\r\n2\r\nthree\r\n" }))
            .unwrap();
        let view: DiffView = serde_json::from_value(view).unwrap();
        assert_eq!((view.before.as_str(), view.after.as_str()), ("one\n2\nthree\n", "one\ntwo\nthree\n"));
        assert_eq!(view.hunks.len(), 1, "only the changed line differs, not every line break");
        let document = |session: &Session| {
            let workbench = session.state::<Workbench>().unwrap();
            workbench.documents().iter().find(|document| document.path() == file).unwrap().text().to_owned()
        };
        let workbench = session.state_mut::<Workbench>();
        let opened = workbench.documents.iter_mut().find(|document| document.path() == file).unwrap();
        opened.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(14)));
        let written = registry
            .dispatch(&mut session, "file.compare.writeSide", json!({ "path": file, "text": "one\nTWO\nthree\n" }))
            .unwrap();
        assert_eq!(written, json!({ "changed": true, "range": [0, 17] }));
        assert_eq!(document(&session), "one\r\nTWO\r\nthree\r\n");
        let workbench = session.state::<Workbench>().unwrap();
        let opened = workbench.documents().iter().find(|document| document.path() == file).unwrap();
        assert_eq!(opened.selections().primary().head, 14, "a caret after the change stays where it was");
        registry.dispatch(&mut session, "file.document.save", json!({ "path": file })).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"one\r\nTWO\r\nthree\r\n");
        let unchanged = registry
            .dispatch(&mut session, "file.compare.writeSide", json!({ "path": file, "text": "one\nTWO\nthree\n" }))
            .unwrap();
        assert_eq!(unchanged["changed"], false);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        assert_eq!(document(&session), "one\r\ntwo\r\nthree\r\n", "one undo step takes the typing back");
    }

    #[test]
    fn two_texts_compare_editable_on_both_sides() {
        let (registry, mut session) = session();
        let view = registry
            .dispatch(
                &mut session,
                "file.compare.texts",
                json!({ "left": "a\nb\n", "right": "a\nB\n", "whitespace": "trim" }),
            )
            .unwrap();
        let view: DiffView = serde_json::from_value(view).unwrap();
        assert!(view.before_editable && view.after_editable);
        assert_eq!(view.hunks.len(), 1);
    }
}

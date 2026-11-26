use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_editor::{Direction, EditOp, Movement, Selection, Selections};
use zen_i18n::t;
use zen_text::Point;

use crate::{Document, Workbench};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(MoveCaret)?;
    registry.register(GetSelections)?;
    registry.register(SetSelections)?;
    registry.register(AddCaret)?;
    registry.register(SelectAll)?;
    registry.register(InsertText)?;
    registry.register(Delete::<0>)?;
    registry.register(Delete::<1>)?;
    registry.register(Delete::<2>)?;
    registry.register(Delete::<3>)?;
    registry.register(ToggleCaret)?;
    registry.register(Occurrences::<0>)?;
    registry.register(Occurrences::<1>)?;
    registry.register(Occurrences::<2>)?;
    registry.register(ColumnSelection)?;
    registry.register(CloneCaret)?;
    registry.register(CloneCaretTo::<true>)?;
    registry.register(CloneCaretTo::<false>)?;
    registry.register(ColumnMode)?;
    registry.register(PasteText)?;
    registry.register(GoTo)?;
    registry.register(SelectWord)?;
    Ok(())
}

pub const CAMEL_HUMPS: &str = "editor.smartKeys.camelHumps";

pub const CAMEL_HUMPS_ON_DOUBLE_CLICK: &str = "editor.smartKeys.camelHumpsOnDoubleClick";

pub fn camel_humps(settings: &zen_core::Settings) -> bool {
    settings.get_or(CAMEL_HUMPS, false)
}

pub fn camel_humps_on_double_click(settings: &zen_core::Settings) -> bool {
    camel_humps(settings) && settings.get_or(CAMEL_HUMPS_ON_DOUBLE_CLICK, true)
}

/// A selection as commands report it: offsets, and lines and columns (from 0) for people.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectionInfo {
    /// Where the selection started, as a character offset.
    pub anchor: usize,
    /// Where the caret is, as a character offset.
    pub head: usize,
    /// The caret's line and column.
    pub line: usize,
    /// The caret's column, in characters.
    pub column: usize,
    /// The selected text (empty for a caret).
    pub text: String,
    /// Whether this is the primary selection.
    pub primary: bool,
}

fn document(session: &mut Session) -> Result<&mut Document, CommandError> {
    session
        .state_mut::<Workbench>()
        .active_document_mut()
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

fn has_text_document(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

pub(crate) fn describe_selections(document: &Document) -> Vec<SelectionInfo> {
    describe(document)
}

fn describe(document: &Document) -> Vec<SelectionInfo> {
    let text = document.snapshot();
    let primary = document.selections().primary_index();
    document
        .selections()
        .iter()
        .enumerate()
        .map(|(index, selection)| {
            let point = text.offset_to_point(selection.head).unwrap_or_default();
            SelectionInfo {
                anchor: selection.anchor,
                head: selection.head,
                line: point.line,
                column: point.column,
                text: text.slice(selection.range()).map(|slice| slice.into_owned()).unwrap_or_default(),
                primary: index == primary,
            }
        })
        .collect()
}

/// A position given as an offset, or as a line and a column (from 0).
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Position {
    /// A character offset.
    Offset(usize),
    /// A line and a column, both from 0; a column past the line's end means its end.
    Point {
        /// The line.
        line: usize,
        /// The column in characters.
        column: usize,
    },
}

impl Position {
    fn offset(self, document: &Document) -> usize {
        let text = document.snapshot();
        match self {
            Position::Offset(offset) => offset.min(text.len_chars()),
            Position::Point { line, column } => {
                let line = line.min(text.len_lines().saturating_sub(1));
                let len = text.line(line).map(|line| line.chars().count()).unwrap_or_default();
                text.point_to_offset(Point::new(line, column.min(len))).unwrap_or_default()
            }
        }
    }
}

/// How to move the carets.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct MoveParams {
    /// The movement, e.g. `{ "kind": "wordRight" }` or `{ "kind": "pageDown", "lines": 30 }`.
    pub movement: Movement,
    /// Extend the selections instead of moving the carets (as with ⇧).
    #[serde(default)]
    pub extend: bool,
    /// In a soft-wrapped file shown in the editor, move up, down, by pages and to a row's start or end over the rows on the screen.
    #[serde(default)]
    pub visual: bool,
    /// Whether wordLeft and wordRight stop at camel humps; the setting Use "CamelHumps" words when absent.
    #[serde(default, rename = "camelHumps")]
    pub camel_humps: Option<bool>,
}

pub struct MoveCaret;

impl Command for MoveCaret {
    type Params = MoveParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.caret.move";
    const TITLE: &'static str = "Move Caret";
    const DESCRIPTION: &'static str = "Moves every caret in the active file (left, right, wordLeft, wordRight, humpLeft, \
         humpRight, up, down, pageUp, pageDown, lineStart, lineEnd, documentStart, documentEnd), or extends the \
         selections; wordLeft and wordRight stop at camel humps when Use \"CamelHumps\" words is on (or camelHumps: \
         true); with visual, up, down, the pages, lineStart and lineEnd go by the rows on the screen in a \
         soft-wrapped file. Returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: MoveParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let rows = visual_rows(session).filter(|_| params.visual);
        let movement = if params.camel_humps.unwrap_or_else(|| camel_humps(session.settings())) {
            params.movement.by_camel_humps()
        } else {
            params.movement
        };
        let document = document(session)?;
        if params.extend && document.column_mode() {
            document.extend_column(movement);
        } else if let Some(moved) = rows.and_then(|rows| rows.moved(document, movement, params.extend)) {
            document.set_selections(moved);
        } else {
            let moved = document.selections().moved(document.snapshot(), movement, params.extend);
            document.set_selections(moved);
        }
        Ok(describe(document))
    }
}

fn visual_rows(session: &Session) -> Option<std::sync::Arc<dyn crate::soft_wrap::VisualRows>> {
    let document = session.state::<Workbench>()?.active_document()?;
    let rows = session.state::<crate::soft_wrap::ShownVisualRows>()?;
    crate::soft_wrap::is_soft_wrapped(session.settings(), document).then(|| std::sync::Arc::clone(&rows.0))
}

pub struct GetSelections;

impl Command for GetSelections {
    type Params = NoParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.selection.get";
    const TITLE: &'static str = "Selections";
    const DESCRIPTION: &'static str =
        "Returns the carets and selections of the active file: offsets, line and column, and the selected text.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SelectionInfo>, CommandError> {
        Ok(describe(document(session)?))
    }
}

/// One selection to set: `anchor` defaults to `head` (a caret).
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SelectionParam {
    /// Where the caret is.
    pub head: Position,
    /// Where the selection starts; a caret when absent.
    pub anchor: Option<Position>,
}

/// The selections to set.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SetSelectionsParams {
    /// One or more selections; several make several carets.
    pub selections: Vec<SelectionParam>,
    /// Which of them is the primary one (the last when absent).
    pub primary: Option<usize>,
}

fn selection(document: &Document, param: &SelectionParam) -> Selection {
    let head = param.head.offset(document);
    Selection::new(param.anchor.map_or(head, |anchor| anchor.offset(document)), head)
}

pub struct SetSelections;

impl Command for SetSelections {
    type Params = SetSelectionsParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.selection.set";
    const TITLE: &'static str = "Set Selections";
    const DESCRIPTION: &'static str = "Places the carets and selections of the active file, by offsets or by lines and columns \
         (from 0); overlapping ones merge. Returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: SetSelectionsParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let items: Vec<Selection> = params.selections.iter().map(|param| selection(document, param)).collect();
        let primary = params.primary.unwrap_or(items.len().saturating_sub(1));
        document.set_selections(Selections::from_vec(items, primary));
        Ok(describe(document))
    }
}

pub struct AddCaret;

impl Command for AddCaret {
    type Params = SelectionParam;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.caret.add";
    const TITLE: &'static str = "Add Caret";
    const DESCRIPTION: &'static str = "Adds a caret (or a selection) to the active file, keeping the others, as ⌥-click does; returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: SelectionParam) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let mut selections = document.selections().clone();
        selections.add(selection(document, &params));
        document.set_selections(selections);
        Ok(describe(document))
    }
}

pub struct SelectAll;

impl Command for SelectAll {
    type Params = NoParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.selection.selectAll";
    const TITLE: &'static str = "Select All";
    const DESCRIPTION: &'static str = "Selects the whole text of the active file.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let len = document.snapshot().len_chars();
        document.set_selections(Selections::single(Selection::new(0, len)));
        Ok(describe(document))
    }
}

/// Text to type.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct InsertParams {
    /// The text; it replaces every selection and goes in at every caret.
    pub text: String,
}

pub struct InsertText;

impl Command for InsertText {
    type Params = InsertParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.edit.insert";
    const TITLE: &'static str = "Type";
    const DESCRIPTION: &'static str = "Types text at every caret of the active file, replacing the selections, as one undo \
         step; returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: InsertParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        document.apply_op(&EditOp::Insert { text: params.text });
        Ok(describe(document))
    }
}

pub struct Delete<const K: u8>;

/// How a deletion takes words.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct DeleteParams {
    /// Whether the word deletions stop at camel humps; the setting Use "CamelHumps" words when absent.
    #[serde(default, rename = "camelHumps")]
    pub camel_humps: Option<bool>,
}

impl<const K: u8> Delete<K> {
    const OP: EditOp = match K {
        0 => EditOp::DeleteBackward,
        1 => EditOp::DeleteForward,
        2 => EditOp::DeleteWordBackward,
        _ => EditOp::DeleteWordForward,
    };
}

impl<const K: u8> Command for Delete<K> {
    type Params = DeleteParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = match K {
        0 => "editor.edit.deleteBackward",
        1 => "editor.edit.deleteForward",
        2 => "editor.edit.deleteWordBackward",
        _ => "editor.edit.deleteWordForward",
    };
    const TITLE: &'static str = match K {
        0 => "Delete Previous Character",
        1 => "Delete Next Character",
        2 => "Delete to Word Start",
        _ => "Delete to Word End",
    };
    const DESCRIPTION: &'static str = match K {
        0 => "Deletes the selections, or the character before every caret (⌫).",
        1 => "Deletes the selections, or the character after every caret (⌦).",
        2 => {
            "Deletes the selections, or back to the start of the word at every caret (⌥⌫); to the camel hump when \
              Use \"CamelHumps\" words is on (or camelHumps: true)."
        }
        _ => {
            "Deletes the selections, or on to the end of the word at every caret (⌥⌦); to the camel hump when \
              Use \"CamelHumps\" words is on (or camelHumps: true)."
        }
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: DeleteParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let op = if params.camel_humps.unwrap_or_else(|| camel_humps(session.settings())) {
            Self::OP.by_camel_humps()
        } else {
            Self::OP
        };
        let document = document(session)?;
        document.apply_op(&op);
        Ok(describe(document))
    }
}

pub struct ToggleCaret;

/// Where to toggle a caret.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct CaretParams {
    /// The place.
    pub head: Position,
}

impl Command for ToggleCaret {
    type Params = CaretParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.caret.toggle";
    const TITLE: &'static str = "Add or Remove Caret";
    const DESCRIPTION: &'static str = "Adds a caret at a place in the active file, or removes the caret that is there (⌥⇧-click); returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: CaretParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let offset = params.head.offset(document);
        let toggled = document.selections().toggled_caret(offset);
        document.set_selections(toggled);
        Ok(describe(document))
    }
}

pub struct Occurrences<const K: u8>;

impl<const K: u8> Command for Occurrences<K> {
    type Params = NoParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = match K {
        0 => "editor.selection.addNextOccurrence",
        1 => "editor.selection.unselectOccurrence",
        _ => "editor.selection.selectAllOccurrences",
    };
    const TITLE: &'static str = match K {
        0 => "Add Selection for Next Occurrence",
        1 => "Unselect Occurrence",
        _ => "Select All Occurrences",
    };
    const DESCRIPTION: &'static str = match K {
        0 => {
            "Selects the word at the caret, then each next occurrence of the selected text with a caret of its own (⌃G)."
        }
        1 => "Removes the last selection added for an occurrence (⌃⇧G).",
        _ => "Selects every occurrence of the selected text, or of the word at the caret, each with a caret (⌃⌘G).",
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let before = document.selections().clone();
        let changed = match K {
            0 => before.with_next_occurrence(document.snapshot()),
            1 => unselect_last_added(&before, document.occurrences_added()),
            _ => before.with_all_occurrences(document.snapshot()),
        };
        let added = document.occurrences_added();
        match K {
            0 if before.primary().is_empty() || added.last() != Some(&before.primary().range()) => {
                *added = vec![changed.primary().range()];
                if before.len() < changed.len() {
                    added.insert(0, before.primary().range());
                }
            }
            0 if before.len() < changed.len() => added.push(changed.primary().range()),
            0 | 1 => {}
            _ => added.clear(),
        }
        document.set_selections(changed);
        Ok(describe(document))
    }
}

fn unselect_last_added(selections: &Selections, added: &mut Vec<std::ops::Range<usize>>) -> Selections {
    let primary = selections.primary().range();
    if added.len() < 2 || added.last() != Some(&primary) || selections.len() < 2 {
        added.clear();
        return selections.without_primary();
    }
    added.pop();
    let previous = added.last().cloned();
    let items: Vec<Selection> = selections.iter().filter(|selection| selection.range() != primary).copied().collect();
    let index = items.iter().position(|selection| Some(selection.range()) == previous).unwrap_or(items.len() - 1);
    Selections::from_vec(items, index)
}

/// A rectangle of text, from one line and column to another.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ColumnParams {
    /// Where the column selection starts.
    pub anchor: PointParam,
    /// Where it ends.
    pub head: PointParam,
}

/// A line and a column, from 0.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
pub struct PointParam {
    /// The line.
    pub line: usize,
    /// The column in characters.
    pub column: usize,
}

pub struct ColumnSelection;

impl Command for ColumnSelection {
    type Params = ColumnParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.selection.column";
    const TITLE: &'static str = "Column Selection";
    const DESCRIPTION: &'static str = "Selects a rectangle of the active file: one selection per line between two lines, from \
         one column to another, cut at line ends (⌥-drag).";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: ColumnParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let point = |p: PointParam| Point::new(p.line, p.column);
        let column = zen_editor::column_selection(document.snapshot(), point(params.anchor), point(params.head));
        document.set_selections(column);
        Ok(describe(document))
    }
}

/// Which way to clone the caret.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct CloneParams {
    /// `up` or `down`.
    pub direction: Direction,
}

pub struct CloneCaret;

impl Command for CloneCaret {
    type Params = CloneParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.caret.clone";
    const TITLE: &'static str = "Clone Caret";
    const DESCRIPTION: &'static str =
        "Adds a caret on the line above the topmost caret or below the bottommost one, at the same column (⌥⌥↑, ⌥⌥↓).";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: CloneParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let cloned = document.selections().with_cloned_caret(document.snapshot(), params.direction);
        document.set_selections(cloned);
        Ok(describe(document))
    }
}

pub struct CloneCaretTo<const UP: bool>;

impl<const UP: bool> Command for CloneCaretTo<UP> {
    type Params = NoParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = if UP { "editor.caret.cloneAbove" } else { "editor.caret.cloneBelow" };
    const TITLE: &'static str = if UP { "Clone Caret Above" } else { "Clone Caret Below" };
    const DESCRIPTION: &'static str = if UP {
        "Adds a caret on the line above the topmost caret, at the same column (⌥⌥↑: ⌥ pressed twice, held, and ↑)."
    } else {
        "Adds a caret on the line below the bottommost caret, at the same column (⌥⌥↓: ⌥ pressed twice, held, and ↓)."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let direction = if UP { Direction::Up } else { Direction::Down };
        let cloned = document.selections().with_cloned_caret(document.snapshot(), direction);
        document.set_selections(cloned);
        Ok(describe(document))
    }
}

/// The parameters of `editor.selection.columnMode`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ColumnModeParams {
    /// Turn the mode on (`true`) or off (`false`); toggles it when absent.
    pub enabled: Option<bool>,
}

pub struct ColumnMode;

impl Command for ColumnMode {
    type Params = ColumnModeParams;
    type Output = bool;
    const ID: &'static str = "editor.selection.columnMode";
    const TITLE: &'static str = "Column Selection Mode";
    const DESCRIPTION: &'static str = "Turns Column Selection Mode of the active file on or off (⇧⌘8): then ⇧ with the arrows, \
         and a drag, select a column. Returns whether it is on.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: ColumnModeParams) -> Result<bool, CommandError> {
        let document = document(session)?;
        let on = params.enabled.unwrap_or(!document.column_mode());
        document.set_column_mode(on);
        Ok(on)
    }
}

pub struct PasteText;

impl Command for PasteText {
    type Params = InsertParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.edit.paste";
    const TITLE: &'static str = "Paste";
    const DESCRIPTION: &'static str = "Pastes text at every caret of the active file; when it has as many lines as there are \
         carets, each caret gets one line.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: InsertParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        document.apply_op(&EditOp::Paste { text: params.text });
        Ok(describe(document))
    }
}

/// A line and a column as people count them, from 1.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct GoToParams {
    /// The line, from 1.
    pub line: usize,
    /// The column, from 1; the line's start when absent.
    pub column: Option<usize>,
}

pub struct GoTo;

impl Command for GoTo {
    type Params = GoToParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.caret.goTo";
    const TITLE: &'static str = "Go to Line:Column";
    const DESCRIPTION: &'static str = "Puts the caret of the active file at a line and column counted from 1, as people \
         count them (⌘L); past the end, at the last line or the line's end.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: GoToParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let origin = crate::history::caret(session);
        let document = document(session)?;
        let position = Position::Point {
            line: params.line.saturating_sub(1),
            column: params.column.unwrap_or(1).saturating_sub(1),
        };
        let offset = position.offset(document);
        document.set_selections(Selections::single(Selection::caret(offset)));
        let selections = describe(document);
        crate::history::jumped(session, origin);
        Ok(selections)
    }
}

/// Where to select a word as a double click does.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct SelectWordParams {
    /// The place; the primary caret when absent.
    #[serde(default)]
    pub at: Option<Position>,
    /// Whether to select the camel hump instead of the identifier; by the Smart Keys settings when absent.
    #[serde(default, rename = "camelHumps")]
    pub camel_humps: Option<bool>,
}

pub struct SelectWord;

impl Command for SelectWord {
    type Params = SelectWordParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.selection.word";
    const TITLE: &'static str = "Select Word";
    const DESCRIPTION: &'static str = "Selects the identifier (letters, digits, _) at a place of the active file, or at the \
         primary caret, as a double click does: only its camel hump (Bar in fooBarBaz) when Use \"CamelHumps\" words \
         and its double-click option (A double-click selects CamelHumps words while they are turned on) are both on (or camelHumps: true); a \
         caret on a blank stays a caret. Returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: SelectWordParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let humps = params.camel_humps.unwrap_or_else(|| camel_humps_on_double_click(session.settings()));
        let document = document(session)?;
        let offset = params.at.map_or(document.selections().primary().head, |at| at.offset(document));
        let word = zen_editor::double_click_word(document.snapshot(), offset, humps);
        document.set_selections(Selections::single(Selection::new(word.start, word.end)));
        Ok(describe(document))
    }
}

pub fn parse_line_column(text: &str) -> Option<(usize, Option<usize>)> {
    let text = text.trim();
    let (line, column) = match text.split_once([':', ',']) {
        Some((line, column)) => (line.trim(), Some(column.trim())),
        None => (text, None),
    };
    let line = line.parse::<usize>().ok().filter(|line| *line > 0)?;
    let column = match column {
        Some(column) if !column.is_empty() => Some(column.parse::<usize>().ok().filter(|column| *column > 0)?),
        _ => None,
    };
    Some((line, column))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn session(contents: &str) -> (tempfile::TempDir, Registry, Session) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, contents).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        (dir, registry, session)
    }

    fn text(session: &Session) -> String {
        session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    #[test]
    fn carets_move_and_type_through_commands() {
        let (_dir, registry, mut session) = session("func a() {}\nfunc b() {}\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        run(
            &mut session,
            "editor.selection.set",
            json!({ "selections": [{ "head": 0 }, { "head": { "line": 1, "column": 0 } }] }),
        );
        run(&mut session, "editor.edit.insert", json!({ "text": "// " }));
        assert_eq!(text(&session), "// func a() {}\n// func b() {}\n");
        let selections =
            run(&mut session, "editor.caret.move", json!({ "movement": { "kind": "wordRight" }, "extend": true }));
        assert_eq!(selections[0]["text"], "func");
        assert_eq!(selections[1]["text"], "func");
        run(&mut session, "editor.edit.insert", json!({ "text": "fn" }));
        assert_eq!(text(&session), "// fn a() {}\n// fn b() {}\n");
        run(&mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), "// func a() {}\n// func b() {}\n");
    }

    #[test]
    fn column_selection_mode_and_cloned_carets() {
        let (_dir, registry, mut session) = session("abcdef\nab\nabcdef\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        let texts = |selections: &serde_json::Value| -> Vec<String> {
            selections.as_array().unwrap().iter().map(|s| s["text"].as_str().unwrap().to_owned()).collect()
        };
        run(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 1 }] }));
        assert_eq!(run(&mut session, "editor.selection.columnMode", json!({})), json!(true));
        let right = json!({ "movement": { "kind": "right" }, "extend": true });
        let down = json!({ "movement": { "kind": "down" }, "extend": true });
        for _ in 0..3 {
            run(&mut session, "editor.caret.move", right.clone());
        }
        run(&mut session, "editor.caret.move", down.clone());
        let selections = run(&mut session, "editor.caret.move", down);
        assert_eq!(texts(&selections), ["bcd", "b", "bcd"]);
        assert_eq!(run(&mut session, "editor.selection.columnMode", json!({})), json!(false));
        let selections = run(&mut session, "editor.caret.move", right);
        assert_eq!(texts(&selections), ["bcde", "b\n", "bcde"]);
        run(&mut session, "editor.selection.set", json!({ "selections": [{ "head": { "line": 1, "column": 1 } }] }));
        run(&mut session, "editor.caret.cloneBelow", json!({}));
        let selections = run(&mut session, "editor.caret.cloneAbove", json!({}));
        let lines: Vec<u64> = selections.as_array().unwrap().iter().map(|s| s["line"].as_u64().unwrap()).collect();
        assert_eq!(lines, [0, 1, 2]);
        assert!(selections.as_array().unwrap().iter().all(|s| s["column"] == 1));
    }

    #[test]
    fn delete_select_all_and_add_caret() {
        let (_dir, registry, mut session) = session("abc");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        run(&mut session, "editor.caret.move", json!({ "movement": { "kind": "documentEnd" } }));
        run(&mut session, "editor.edit.deleteBackward", json!({}));
        assert_eq!(text(&session), "ab");
        let added = run(&mut session, "editor.caret.add", json!({ "head": 0 }));
        assert_eq!(added.as_array().unwrap().len(), 2);
        assert_eq!(added[0]["primary"], true);
        let all = run(&mut session, "editor.selection.selectAll", json!({}));
        assert_eq!(all[0]["text"], "ab");
        run(&mut session, "editor.edit.deleteForward", json!({}));
        assert_eq!(text(&session), "");
        run(&mut session, "editor.edit.deleteWordBackward", json!({}));
    }

    #[test]
    fn camel_humps_follow_the_settings() {
        let (_dir, registry, mut session) = session("x fooBarBaz\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        let word_right = json!({ "movement": { "kind": "wordRight" } });
        run(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 1 }] }));
        assert_eq!(run(&mut session, "editor.caret.move", word_right.clone())[0]["head"], 11, "off by default");
        session.settings_mut().set(CAMEL_HUMPS, true).unwrap();
        run(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 1 }] }));
        assert_eq!(run(&mut session, "editor.caret.move", word_right.clone())[0]["head"], 5);
        let extended =
            run(&mut session, "editor.caret.move", json!({ "movement": { "kind": "wordRight" }, "extend": true }));
        assert_eq!(extended[0]["text"], "Bar");
        let back = json!({ "movement": { "kind": "wordLeft" }, "camelHumps": false });
        assert_eq!(run(&mut session, "editor.caret.move", back)[0]["head"], 2, "the parameter wins");
        run(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 11 }] }));
        run(&mut session, "editor.edit.deleteWordBackward", json!({}));
        assert_eq!(text(&session), "x fooBar\n");
        run(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 2 }] }));
        run(&mut session, "editor.edit.deleteWordForward", json!(null));
        assert_eq!(text(&session), "x Bar\n");
        run(&mut session, "editor.edit.undo", json!({}));
        run(&mut session, "editor.edit.undo", json!({}));
        assert_eq!(run(&mut session, "editor.selection.word", json!({ "at": 6 }))[0]["text"], "Bar");
        session.settings_mut().set(CAMEL_HUMPS_ON_DOUBLE_CLICK, false).unwrap();
        assert_eq!(run(&mut session, "editor.selection.word", json!({ "at": 6 }))[0]["text"], "fooBarBaz");
        session.settings_mut().set(CAMEL_HUMPS_ON_DOUBLE_CLICK, true).unwrap();
        session.settings_mut().set(CAMEL_HUMPS, false).unwrap();
        assert!(!camel_humps_on_double_click(session.settings()), "the second switch works only with the first");
        assert_eq!(run(&mut session, "editor.selection.word", json!({ "at": 6 }))[0]["text"], "fooBarBaz");
        assert_eq!(run(&mut session, "editor.selection.word", json!({ "camelHumps": true }))[0]["text"], "Baz");
        assert_eq!(run(&mut session, "editor.selection.word", json!({ "at": 1 }))[0]["text"], "x");
        assert_eq!(run(&mut session, "editor.selection.word", json!({ "at": 12 }))[0]["text"], "");
    }

    #[test]
    fn go_to_line_and_column() {
        let (_dir, registry, mut session) = session("one\ntwo\nthree\n");
        let to = registry.dispatch(&mut session, "editor.caret.goTo", json!({ "line": 2, "column": 3 })).unwrap();
        assert_eq!((to[0]["line"].as_u64(), to[0]["column"].as_u64()), (Some(1), Some(2)));
        let end = registry.dispatch(&mut session, "editor.caret.goTo", json!({ "line": 99, "column": 99 })).unwrap();
        assert_eq!(end[0]["line"], 3, "past the end: the last line");
        assert_eq!(parse_line_column("12"), Some((12, None)));
        assert_eq!(parse_line_column(" 12:5 "), Some((12, Some(5))));
        assert_eq!(parse_line_column("12:"), Some((12, None)));
        assert_eq!(parse_line_column("0"), None);
        assert_eq!(parse_line_column("x"), None);
        assert_eq!(parse_line_column("3:y"), None);
    }

    #[test]
    fn unselect_occurrence_drops_the_occurrences_in_the_order_they_were_added() {
        let (_dir, registry, mut session) = session("ab ab ab\n");
        let run = |session: &mut Session, id: &str| registry.dispatch(session, id, json!({})).unwrap();
        let ranges = |selections: &serde_json::Value| -> (Vec<(u64, u64)>, u64) {
            let all = selections.as_array().unwrap();
            let range = |s: &serde_json::Value| {
                let (a, h) = (s["anchor"].as_u64().unwrap(), s["head"].as_u64().unwrap());
                (a.min(h), a.max(h))
            };
            let primary = all.iter().find(|s| s["primary"] == true).map(range).unwrap();
            (all.iter().map(range).collect(), primary.0)
        };
        let caret = json!({ "selections": [{ "head": { "line": 0, "column": 4 } }] });
        registry.dispatch(&mut session, "editor.selection.set", caret).unwrap();
        run(&mut session, "editor.selection.addNextOccurrence");
        run(&mut session, "editor.selection.addNextOccurrence");
        let wrapped = run(&mut session, "editor.selection.addNextOccurrence");
        assert_eq!(ranges(&wrapped), (vec![(0, 2), (3, 5), (6, 8)], 0), "the third wraps to the first");
        let one = run(&mut session, "editor.selection.unselectOccurrence");
        assert_eq!(ranges(&one), (vec![(3, 5), (6, 8)], 6), "the one added before it is primary");
        let two = run(&mut session, "editor.selection.unselectOccurrence");
        assert_eq!(ranges(&two), (vec![(3, 5)], 3), "the word it started from is left");
        let again = run(&mut session, "editor.selection.addNextOccurrence");
        assert_eq!(ranges(&again), (vec![(3, 5), (6, 8)], 6));
        run(&mut session, "editor.selection.addNextOccurrence");
        let none_left = run(&mut session, "editor.selection.addNextOccurrence");
        assert_eq!(ranges(&none_left), (vec![(0, 2), (3, 5), (6, 8)], 0), "nothing more to add");
        run(&mut session, "editor.selection.unselectOccurrence");
        let back = run(&mut session, "editor.selection.unselectOccurrence");
        assert_eq!(ranges(&back), (vec![(3, 5)], 3), "the order outlives a press that added nothing");
    }

    #[test]
    fn several_carets_through_commands() {
        let (_dir, registry, mut session) = session("x := 1\nx = x + 1\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        let all = run(&mut session, "editor.selection.selectAllOccurrences", json!({}));
        assert_eq!(all.as_array().unwrap().len(), 3);
        run(&mut session, "editor.edit.insert", json!({ "text": "count" }));
        assert_eq!(text(&session), "count := 1\ncount = count + 1\n");
        run(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
        run(&mut session, "editor.caret.clone", json!({ "direction": "down" }));
        run(&mut session, "editor.edit.paste", json!({ "text": "a\nb\n" }));
        assert_eq!(text(&session), "acount := 1\nbcount = count + 1\n");
        let column = run(
            &mut session,
            "editor.selection.column",
            json!({ "anchor": { "line": 0, "column": 0 }, "head": { "line": 1, "column": 2 } }),
        );
        assert_eq!(column[0]["text"], "ac");
        assert_eq!(column[1]["text"], "bc");
        let toggled = run(&mut session, "editor.caret.toggle", json!({ "head": 5 }));
        assert_eq!(toggled.as_array().unwrap().len(), 3);
    }
}

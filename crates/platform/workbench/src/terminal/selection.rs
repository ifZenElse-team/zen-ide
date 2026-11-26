use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::{MAX_READ, SessionParams, has_sessions, missing_session, started, tail};

/// What a terminal session has selected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Selected {
    /// The session.
    pub id: u64,
    /// The selected text as Copy puts it on the clipboard (at most 64 KiB from its end); absent when nothing is selected.
    pub text: Option<String>,
    /// Whether `text` was cut to the limit.
    pub truncated: bool,
}

fn selection_of(session: &mut Session, id: Option<u64>) -> Result<(u64, Option<String>), CommandError> {
    let terminals = started(session)?;
    let id = terminals.session_or_focused(id)?;
    let pane = terminals.pane(id).ok_or_else(|| missing_session(id))?;
    Ok((id, pane.session().selection_text()))
}

fn answer(id: u64, text: Option<String>) -> Selected {
    match text {
        Some(text) => {
            let (text, truncated) = tail(text, MAX_READ);
            Selected { id, text: Some(text), truncated }
        }
        None => Selected { id, text: None, truncated: false },
    }
}

pub struct GetSelection;

impl Command for GetSelection {
    type Params = SessionParams;
    type Output = Selected;
    const ID: &'static str = "terminal.selection.get";
    const TITLE: &'static str = "Read Terminal Selection";
    const DESCRIPTION: &'static str = "The text selected in a session (the focused one by default), as Copy gives \
         it: no blanks at line ends, lines the terminal wrapped joined again, real line breaks kept.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<Selected, CommandError> {
        let (id, text) = selection_of(session, params.id)?;
        Ok(answer(id, text))
    }
}

/// The parameters of `terminal.selection.copy`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CopyParams {
    /// The session's id from `terminal.session.list`; the focused session of the tab by default.
    pub id: Option<u64>,
    /// Copy as Plain Text; the terminal copies plain text either way.
    pub plain: bool,
}

pub struct CopySelection;

impl Command for CopySelection {
    type Params = CopyParams;
    type Output = Selected;
    const ID: &'static str = "terminal.selection.copy";
    const TITLE: &'static str = "Copy";
    const DESCRIPTION: &'static str = "Copies the text selected in a session to the clipboard, as ⌘C does: no \
         blanks at line ends, lines the terminal wrapped joined again, real line breaks kept. Refused when \
         nothing is selected.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: CopyParams) -> Result<Selected, CommandError> {
        let (id, text) = selection_of(session, params.id)?;
        let text = text.ok_or_else(|| CommandError::new(t!("terminal-selection-none")))?;
        crate::file_clipboard::set_clipboard_text(session, &text);
        Ok(answer(id, Some(text)))
    }
}

pub struct SelectAll;

impl Command for SelectAll {
    type Params = SessionParams;
    type Output = Selected;
    const ID: &'static str = "terminal.selection.selectAll";
    const TITLE: &'static str = "Select All";
    const DESCRIPTION: &'static str = "Selects the whole text of a session, its scrollback history too, and answers it as terminal.selection.get does.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<Selected, CommandError> {
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        let pane = terminals.pane(id).ok_or_else(|| missing_session(id))?;
        pane.session().select_all();
        Ok(answer(id, pane.session().selection_text()))
    }
}

/// The parameters of `terminal.lines.copy`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinesParams {
    /// The session's id from `terminal.session.list`; the focused session of the tab by default.
    #[serde(default)]
    pub id: Option<u64>,
    /// The first line, from 1: the oldest line of the scrollback history.
    pub from: usize,
    /// The last line, included.
    pub to: usize,
}

/// Lines copied from a terminal session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopiedLines {
    /// The session.
    pub id: u64,
    /// The text put on the clipboard, at most 64 KiB from its end here.
    pub text: String,
    /// Whether `text` was cut to the limit.
    pub truncated: bool,
    /// How many lines the session has, history and screen.
    pub lines: usize,
}

pub struct CopyLines;

impl Command for CopyLines {
    type Params = LinesParams;
    type Output = CopiedLines;
    const ID: &'static str = "terminal.lines.copy";
    const TITLE: &'static str = "Copy Terminal Lines";
    const DESCRIPTION: &'static str = "Copies lines `from` to `to` (from 1, the oldest of the scrollback history, \
         `lines` in all) of a session to the clipboard, as Copy would give them, and answers them.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: LinesParams) -> Result<CopiedLines, CommandError> {
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        let pane = terminals.pane(id).ok_or_else(|| missing_session(id))?;
        let lines = pane.session().line_count();
        if params.from == 0 || params.to == 0 || params.from > lines || params.to > lines {
            return Err(CommandError::new(t!("terminal-lines-out-of-range", lines = lines)));
        }
        let text = pane.session().lines_text(params.from, params.to);
        crate::file_clipboard::set_clipboard_text(session, &text);
        let (text, truncated) = tail(text, MAX_READ);
        Ok(CopiedLines { id, text, truncated, lines })
    }
}

pub struct PasteClipboard;

impl Command for PasteClipboard {
    type Params = SessionParams;
    type Output = zen_core::NoParams;
    const ID: &'static str = "terminal.session.paste";
    const TITLE: &'static str = "Paste";
    const DESCRIPTION: &'static str = "Pastes the clipboard into a session, as ⌘V does: bracketed when the program \
         asks for it. A person's clipboard: refused to agents with the code `userOnly`.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<zen_core::NoParams, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(super::links::USER_ONLY, t!("terminal-paste-user-only")));
        }
        let text = crate::file_clipboard::clipboard_text(session);
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        let pane = terminals.pane(id).ok_or_else(|| missing_session(id))?;
        let bracketed = pane.session().snapshot().bracketed_paste;
        pane.session().clear_selection();
        pane.session().write(zen_terminal::keys::paste(&text, bracketed));
        pane.session().scroll_to_bottom();
        Ok(zen_core::NoParams {})
    }
}

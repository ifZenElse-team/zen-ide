use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session, Settings};
use zen_i18n::t;

use crate::{Document, Workbench};

pub const SHOW_LINE_NUMBERS: &str = "editor.lineNumbers.show";

pub const LINE_NUMBERS_MODE: &str = "editor.lineNumbers.mode";

pub const SHOW_METHOD_SEPARATORS: &str = "editor.methodSeparators.show";

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ToggleLineNumbers)?;
    registry.register(GetLineNumbers)?;
    Ok(())
}

/// How the gutter numbers the lines (Settings → Editor → General → Appearance → Show line numbers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LineNumbersMode {
    /// Each line's own number, from 1.
    #[default]
    Absolute,
    /// The distance from the caret's line, 0 on it.
    Relative,
    /// The caret's line by its own number, the others by their distance from it.
    Hybrid,
}

impl LineNumbersMode {
    pub const ALL: [LineNumbersMode; 3] =
        [LineNumbersMode::Absolute, LineNumbersMode::Relative, LineNumbersMode::Hybrid];

    pub fn number(self, line: usize, distance: usize) -> usize {
        match self {
            LineNumbersMode::Absolute => line + 1,
            LineNumbersMode::Relative => distance,
            LineNumbersMode::Hybrid if distance == 0 => line + 1,
            LineNumbersMode::Hybrid => distance,
        }
    }
}

pub fn shown_by_default(settings: &Settings) -> bool {
    settings.get::<bool>(SHOW_LINE_NUMBERS).unwrap_or(true)
}

pub fn is_shown(settings: &Settings, document: &Document) -> bool {
    document.line_numbers().unwrap_or_else(|| shown_by_default(settings))
}

pub fn mode(settings: &Settings) -> LineNumbersMode {
    settings.get::<LineNumbersMode>(LINE_NUMBERS_MODE).unwrap_or_default()
}

pub fn method_separators_shown(settings: &Settings) -> bool {
    settings.get::<bool>(SHOW_METHOD_SEPARATORS).unwrap_or(false)
}

/// The line numbers of the active file, as commands report them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineNumbersState {
    /// The active file.
    pub path: String,
    /// Whether its editor shows the line numbers (Distraction Free and Zen Mode hide them whatever this says).
    pub shown: bool,
    /// Whether the setting `editor.lineNumbers.show` shows them when the editor is not toggled.
    pub by_default: bool,
    /// How the lines are numbered: `absolute`, `relative` or `hybrid` (the setting `editor.lineNumbers.mode`).
    pub mode: LineNumbersMode,
    /// Whether a line is drawn above each top-level declaration (the setting `editor.methodSeparators.show`).
    pub method_separators: bool,
}

fn state(session: &Session) -> Result<LineNumbersState, CommandError> {
    let document = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let settings = session.settings();
    Ok(LineNumbersState {
        path: document.path().display().to_string(),
        shown: is_shown(settings, document),
        by_default: shown_by_default(settings),
        mode: mode(settings),
        method_separators: method_separators_shown(settings),
    })
}

fn has_text(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

/// The parameters of `editor.lineNumbers.toggle`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ToggleLineNumbersParams {
    /// Show the line numbers (`true`) or hide them (`false`); toggles them when absent.
    pub enabled: Option<bool>,
}

pub struct ToggleLineNumbers;

impl Command for ToggleLineNumbers {
    type Params = ToggleLineNumbersParams;
    type Output = LineNumbersState;
    const ID: &'static str = "editor.lineNumbers.toggle";
    const TITLE: &'static str = "Show Line Numbers";
    const DESCRIPTION: &'static str = "Shows or hides the line numbers of the active editor (View → Active Editor → \
         Show Line Numbers, the gutter's Appearance menu); the setting editor.lineNumbers.show stays as it is. \
         Returns whether they are shown.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: ToggleLineNumbersParams) -> Result<LineNumbersState, CommandError> {
        let on = params.enabled.unwrap_or(!state(session)?.shown);
        let document = session
            .state_mut::<Workbench>()
            .active_document_mut()
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        document.set_line_numbers(Some(on));
        state(session)
    }
}

pub struct GetLineNumbers;

impl Command for GetLineNumbers {
    type Params = NoParams;
    type Output = LineNumbersState;
    const ID: &'static str = "editor.lineNumbers.get";
    const TITLE: &'static str = "Line Numbers";
    const DESCRIPTION: &'static str = "Tells whether the active editor shows its line numbers, whether the setting \
         editor.lineNumbers.show shows them by default, how they count (absolute, relative or hybrid) and whether \
         method separators are drawn.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LineNumbersState, CommandError> {
        state(session)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn open() -> (Registry, Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        (registry, session, dir)
    }

    fn get(registry: &Registry, session: &mut Session) -> Value {
        registry.dispatch(session, "editor.lineNumbers.get", json!({})).unwrap()
    }

    #[test]
    fn the_modes_count_from_the_caret_line() {
        let numbers =
            |mode: LineNumbersMode| [(4, 0), (5, 1), (2, 2)].map(|(line, distance)| mode.number(line, distance));
        assert_eq!(numbers(LineNumbersMode::Absolute), [5, 6, 3]);
        assert_eq!(numbers(LineNumbersMode::Relative), [0, 1, 2]);
        assert_eq!(numbers(LineNumbersMode::Hybrid), [5, 1, 2]);
    }

    #[test]
    fn the_numbers_follow_the_settings_until_the_editor_is_toggled() {
        let (registry, mut session, _dir) = open();
        let state = get(&registry, &mut session);
        assert_eq!(
            (&state["shown"], &state["byDefault"], &state["mode"], &state["methodSeparators"]),
            (&json!(true), &json!(true), &json!("absolute"), &json!(false))
        );
        session.settings_mut().set(SHOW_LINE_NUMBERS, false).unwrap();
        session.settings_mut().set(LINE_NUMBERS_MODE, "hybrid").unwrap();
        session.settings_mut().set(SHOW_METHOD_SEPARATORS, true).unwrap();
        let state = get(&registry, &mut session);
        assert_eq!(
            (&state["shown"], &state["byDefault"], &state["mode"], &state["methodSeparators"]),
            (&json!(false), &json!(false), &json!("hybrid"), &json!(true))
        );
        let toggled = registry.dispatch(&mut session, "editor.lineNumbers.toggle", json!({})).unwrap();
        assert_eq!((&toggled["shown"], &toggled["byDefault"]), (&json!(true), &json!(false)));
        session.settings_mut().set(SHOW_LINE_NUMBERS, true).unwrap();
        let hidden = registry.dispatch(&mut session, "editor.lineNumbers.toggle", json!({ "enabled": false })).unwrap();
        assert_eq!((&hidden["shown"], &hidden["byDefault"]), (&json!(false), &json!(true)), "the editor wins");
    }

    #[test]
    fn without_a_text_file_there_is_nothing_to_number() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        assert!(!registry.is_available(&session, "editor.lineNumbers.toggle"));
        assert!(registry.dispatch(&mut session, "editor.lineNumbers.get", json!({})).is_err());
    }
}

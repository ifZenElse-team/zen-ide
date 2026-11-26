use std::ops::RangeInclusive;

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::{Document, Workbench};

pub const EDITOR_FONT_SIZE: &str = "editor.font.size";

pub const DEFAULT_SIZE: f32 = 13.0;

pub const SIZES: RangeInclusive<f32> = 6.0..=48.0;

pub const STEP: f32 = 1.0;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(IncreaseFontSize)?;
    registry.register(DecreaseFontSize)?;
    registry.register(ResetFontSize)?;
    registry.register(GetFontSize)?;
    Ok(())
}

pub fn stepped(size: f32, steps: f32) -> f32 {
    (size + steps * STEP).clamp(*SIZES.start(), *SIZES.end())
}

pub fn settings_size(settings: &zen_core::Settings) -> f32 {
    settings.value(EDITOR_FONT_SIZE).and_then(serde_json::Value::as_f64).map_or(DEFAULT_SIZE, |size| size as f32)
}

/// The editor font size of the active file, as commands report it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FontSizeState {
    /// The active file.
    pub path: String,
    /// The font size its editor shows, in points.
    pub size: f32,
    /// The font size of the settings (Settings → Editor → Font, or Presentation Mode's), which Reset Font Size brings back.
    pub default: f32,
    /// Whether the file has a size of its own.
    pub changed: bool,
}

fn active(session: &Session) -> Option<&Document> {
    session.state::<Workbench>().and_then(Workbench::active_document).filter(|document| document.image().is_none())
}

fn no_document() -> CommandError {
    CommandError::new(t!("workbench-error-no-document"))
}

fn state(session: &Session) -> Result<FontSizeState, CommandError> {
    let document = active(session).ok_or_else(no_document)?;
    let default = settings_size(session.settings());
    Ok(FontSizeState {
        path: document.path().display().to_string(),
        size: document.font_size().unwrap_or(default),
        default,
        changed: document.font_size().is_some(),
    })
}

fn change(session: &mut Session, steps: Option<f32>) -> Result<FontSizeState, CommandError> {
    let current = state(session)?;
    let size = steps.map(|steps| stepped(current.size, steps));
    let document = session.state_mut::<Workbench>().active_document_mut().ok_or_else(no_document)?;
    document.set_font_size(size.filter(|size| *size != current.default));
    state(session)
}

pub struct IncreaseFontSize;

impl Command for IncreaseFontSize {
    type Params = NoParams;
    type Output = FontSizeState;
    const ID: &'static str = "editor.fontSize.increase";
    const TITLE: &'static str = "Increase Font Size";
    const DESCRIPTION: &'static str = "Makes the editor font of the active file one point larger, in its editor only \
         (View → Active Editor → Increase Font Size); the size is dropped when the file is closed. Returns the size.";

    fn is_available(&self, session: &Session) -> bool {
        active(session).is_some()
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FontSizeState, CommandError> {
        change(session, Some(1.0))
    }
}

pub struct DecreaseFontSize;

impl Command for DecreaseFontSize {
    type Params = NoParams;
    type Output = FontSizeState;
    const ID: &'static str = "editor.fontSize.decrease";
    const TITLE: &'static str = "Decrease Font Size";
    const DESCRIPTION: &'static str = "Makes the editor font of the active file one point smaller, in its editor only \
         (View → Active Editor → Decrease Font Size); the size is dropped when the file is closed. Returns the size.";

    fn is_available(&self, session: &Session) -> bool {
        active(session).is_some()
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FontSizeState, CommandError> {
        change(session, Some(-1.0))
    }
}

pub struct ResetFontSize;

impl Command for ResetFontSize {
    type Params = NoParams;
    type Output = FontSizeState;
    const ID: &'static str = "editor.fontSize.reset";
    const TITLE: &'static str = "Reset Font Size";
    const DESCRIPTION: &'static str = "Brings the editor font of the active file back to the size of Settings → Editor → \
         Font, or of Presentation Mode while it is on (View → Active Editor → Reset Font Size). Returns the size.";

    fn is_available(&self, session: &Session) -> bool {
        active(session).is_some()
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FontSizeState, CommandError> {
        change(session, None)
    }
}

pub struct GetFontSize;

impl Command for GetFontSize {
    type Params = NoParams;
    type Output = FontSizeState;
    const ID: &'static str = "editor.fontSize.get";
    const TITLE: &'static str = "Editor Font Size";
    const DESCRIPTION: &'static str = "Tells the editor font size of the active file, the size of Settings → Editor → \
         Font, and whether the file has a size of its own.";

    fn is_available(&self, session: &Session) -> bool {
        active(session).is_some()
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FontSizeState, CommandError> {
        state(session)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn open(registry: &Registry, session: &mut Session, dir: &std::path::Path, name: &str) {
        let file = dir.join(name);
        std::fs::write(&file, "package main\n").unwrap();
        registry.dispatch(session, "file.document.open", json!({ "path": file })).unwrap();
    }

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        open(&registry, &mut session, dir.path(), "main.go");
        (registry, session, dir)
    }

    fn run(registry: &Registry, session: &mut Session, command: &str) -> Value {
        registry.dispatch(session, command, json!({})).unwrap()
    }

    #[test]
    fn the_active_file_follows_the_settings_until_it_is_changed() {
        let (registry, mut session, _dir) = setup();
        let state = run(&registry, &mut session, "editor.fontSize.get");
        assert_eq!((state["size"].clone(), state["default"].clone()), (json!(13.0), json!(13.0)));
        assert_eq!(state["changed"], false);
        session.settings_mut().set(EDITOR_FONT_SIZE, 15.0).unwrap();
        assert_eq!(run(&registry, &mut session, "editor.fontSize.get")["size"], 15.0);
    }

    #[test]
    fn increase_decrease_and_reset_change_only_the_active_file() {
        let (registry, mut session, dir) = setup();
        let state = run(&registry, &mut session, "editor.fontSize.increase");
        assert_eq!((state["size"].clone(), state["changed"].clone()), (json!(14.0), json!(true)));
        assert_eq!(run(&registry, &mut session, "editor.fontSize.increase")["size"], 15.0);
        open(&registry, &mut session, dir.path(), "other.go");
        assert_eq!(
            run(&registry, &mut session, "editor.fontSize.get")["size"],
            13.0,
            "another file keeps the settings"
        );
        assert_eq!(run(&registry, &mut session, "editor.fontSize.decrease")["size"], 12.0);
        let first = dir.path().join("main.go");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": first })).unwrap();
        assert_eq!(run(&registry, &mut session, "editor.fontSize.get")["size"], 15.0);
        let reset = run(&registry, &mut session, "editor.fontSize.reset");
        assert_eq!((reset["size"].clone(), reset["changed"].clone()), (json!(13.0), json!(false)));
    }

    #[test]
    fn a_size_back_at_the_settings_is_no_size_of_its_own() {
        let (registry, mut session, _dir) = setup();
        run(&registry, &mut session, "editor.fontSize.increase");
        assert_eq!(run(&registry, &mut session, "editor.fontSize.decrease")["changed"], false);
    }

    #[test]
    fn the_size_stays_within_the_limits() {
        let (registry, mut session, _dir) = setup();
        session.settings_mut().set(EDITOR_FONT_SIZE, 47.5).unwrap();
        assert_eq!(run(&registry, &mut session, "editor.fontSize.increase")["size"], 48.0);
        assert_eq!(run(&registry, &mut session, "editor.fontSize.increase")["size"], 48.0);
        assert_eq!(stepped(6.5, -3.0), 6.0);
    }

    #[test]
    fn the_size_is_dropped_when_the_file_is_closed() {
        let (registry, mut session, dir) = setup();
        run(&registry, &mut session, "editor.fontSize.increase");
        registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
        open(&registry, &mut session, dir.path(), "main.go");
        assert_eq!(run(&registry, &mut session, "editor.fontSize.get")["changed"], false);
    }

    #[test]
    fn with_no_file_open_the_commands_are_unavailable() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        assert!(registry.dispatch(&mut session, "editor.fontSize.increase", json!({})).is_err());
    }
}

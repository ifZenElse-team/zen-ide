use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_editor::{Movement, Selections};
use zen_i18n::t;

use crate::{Document, Workbench};

pub const SOFT_WRAP_FILES: &str = "editor.softWrap.files";

pub const DEFAULT_SOFT_WRAP_FILES: &str = "*.md; *.txt; *.rst; *.adoc";

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ToggleSoftWrap)?;
    registry.register(GetSoftWrap)?;
    Ok(())
}

pub fn matches_patterns(patterns: &str, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return false };
    patterns.split([';', ',']).map(str::trim).filter(|pattern| !pattern.is_empty()).any(|pattern| glob(pattern, name))
}

pub(crate) fn glob(pattern: &str, name: &str) -> bool {
    let mut rest = pattern.chars();
    match rest.next() {
        None => name.is_empty(),
        Some('*') => {
            name.char_indices().map(|(at, _)| at).chain([name.len()]).any(|at| glob(rest.as_str(), &name[at..]))
        }
        Some(c) => {
            let mut tail = name.chars();
            tail.next().is_some_and(|first| (c == '?' || same_letter(c, first)) && glob(rest.as_str(), tail.as_str()))
        }
    }
}

fn same_letter(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

pub fn wrapped_by_default(settings: &zen_core::Settings, path: &Path) -> bool {
    let patterns = settings.value(SOFT_WRAP_FILES).and_then(serde_json::Value::as_str);
    matches_patterns(patterns.unwrap_or(DEFAULT_SOFT_WRAP_FILES), path)
}

pub trait VisualRows: Send + Sync {
    fn moved(&self, document: &Document, movement: Movement, extend: bool) -> Option<Selections>;
}

#[derive(Clone)]
pub struct ShownVisualRows(pub Arc<dyn VisualRows>);

pub fn is_soft_wrapped(settings: &zen_core::Settings, document: &Document) -> bool {
    document.image().is_none() && document.soft_wrap().unwrap_or_else(|| wrapped_by_default(settings, document.path()))
}

/// Whether the active file is soft-wrapped, as commands report it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SoftWrapState {
    /// The active file.
    pub path: String,
    /// Whether its long lines wrap at the editor's edge.
    pub enabled: bool,
    /// Whether the setting `editor.softWrap.files` wraps it when it is not toggled.
    pub by_default: bool,
    /// The setting's patterns.
    pub files: String,
}

fn state(session: &Session) -> Result<SoftWrapState, CommandError> {
    let document = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let settings = session.settings();
    Ok(SoftWrapState {
        path: document.path().display().to_string(),
        enabled: is_soft_wrapped(settings, document),
        by_default: wrapped_by_default(settings, document.path()),
        files: settings.get_or(SOFT_WRAP_FILES, DEFAULT_SOFT_WRAP_FILES.to_owned()),
    })
}

fn has_text(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

/// The parameters of `editor.softWrap.toggle`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ToggleSoftWrapParams {
    /// Turn soft wraps on (`true`) or off (`false`); toggles them when absent.
    pub enabled: Option<bool>,
}

pub struct ToggleSoftWrap;

impl Command for ToggleSoftWrap {
    type Params = ToggleSoftWrapParams;
    type Output = SoftWrapState;
    const ID: &'static str = "editor.softWrap.toggle";
    const TITLE: &'static str = "Soft-Wrap";
    const DESCRIPTION: &'static str = "Turns soft wraps of the active file on or off (View → Active Editor → Soft-Wrap): \
         long lines then wrap at the editor's edge. Returns whether they are on.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: ToggleSoftWrapParams) -> Result<SoftWrapState, CommandError> {
        let on = params.enabled.unwrap_or(!state(session)?.enabled);
        let document = session
            .state_mut::<Workbench>()
            .active_document_mut()
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        document.set_soft_wrap(Some(on));
        state(session)
    }
}

pub struct GetSoftWrap;

impl Command for GetSoftWrap {
    type Params = NoParams;
    type Output = SoftWrapState;
    const ID: &'static str = "editor.softWrap.get";
    const TITLE: &'static str = "Soft Wraps";
    const DESCRIPTION: &'static str = "Tells whether the active file is soft-wrapped, whether the setting \
         editor.softWrap.files wraps it by default, and the setting's file name patterns.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SoftWrapState, CommandError> {
        state(session)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn open(name: &str) -> (Registry, Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        std::fs::write(&file, "a long line\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        (registry, session, dir)
    }

    fn enabled(registry: &Registry, session: &mut Session) -> Value {
        registry.dispatch(session, "editor.softWrap.get", json!({})).unwrap()["enabled"].clone()
    }

    #[test]
    fn the_default_patterns_wrap_markdown_and_text() {
        let wrapped = |name: &str| matches_patterns(DEFAULT_SOFT_WRAP_FILES, Path::new(name));
        assert!(wrapped("README.md") && wrapped("notes.txt") && wrapped("index.rst") && wrapped("guide.adoc"));
        assert!(wrapped("/project/docs/CHANGES.MD"), "names match whatever their case");
        assert!(!wrapped("main.go") && !wrapped("md") && !wrapped("Makefile"));
        assert!(matches_patterns("Make?ile; *.go", Path::new("Makefile")));
        assert!(!matches_patterns("", Path::new("a.md")));
    }

    #[test]
    fn markdown_opens_wrapped_and_go_does_not() {
        let (registry, mut session, _dir) = open("README.md");
        let state = registry.dispatch(&mut session, "editor.softWrap.get", json!({})).unwrap();
        assert_eq!((state["enabled"].clone(), state["byDefault"].clone()), (json!(true), json!(true)));
        assert_eq!(state["files"], DEFAULT_SOFT_WRAP_FILES);
        let (registry, mut session, _dir) = open("main.go");
        assert_eq!(enabled(&registry, &mut session), false);
    }

    #[test]
    fn the_toggle_turns_wraps_of_the_file_on_and_off() {
        let (registry, mut session, _dir) = open("main.go");
        let state = registry.dispatch(&mut session, "editor.softWrap.toggle", json!({})).unwrap();
        assert_eq!((state["enabled"].clone(), state["byDefault"].clone()), (json!(true), json!(false)));
        registry.dispatch(&mut session, "editor.softWrap.toggle", json!({})).unwrap();
        assert_eq!(enabled(&registry, &mut session), false);
        registry.dispatch(&mut session, "editor.softWrap.toggle", json!({ "enabled": true })).unwrap();
        registry.dispatch(&mut session, "editor.softWrap.toggle", json!({ "enabled": true })).unwrap();
        assert_eq!(enabled(&registry, &mut session), true);
    }

    #[test]
    fn a_visual_move_with_no_editor_shown_goes_by_lines() {
        let (registry, mut session, _dir) = open("README.md");
        let moved = registry
            .dispatch(&mut session, "editor.caret.move", json!({ "movement": { "kind": "down" }, "visual": true }))
            .unwrap();
        assert_eq!(moved[0]["line"], 1);
    }

    #[test]
    fn the_setting_decides_for_files_not_toggled() {
        let (registry, mut session, _dir) = open("main.go");
        session.settings_mut().set(SOFT_WRAP_FILES, "*.go").unwrap();
        assert_eq!(enabled(&registry, &mut session), true);
        registry.dispatch(&mut session, "editor.softWrap.toggle", json!({})).unwrap();
        session.settings_mut().set(SOFT_WRAP_FILES, "*.md").unwrap();
        assert_eq!(enabled(&registry, &mut session), false, "a file toggled keeps its own choice");
    }
}

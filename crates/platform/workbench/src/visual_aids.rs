use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session, Settings};
use zen_i18n::t;

use crate::{Document, Workbench, code_style, project_settings};

pub const SHOW_WHITESPACES: &str = "editor.whitespaces.show";
pub const WHITESPACES_LEADING: &str = "editor.whitespaces.leading";
pub const WHITESPACES_INNER: &str = "editor.whitespaces.inner";
pub const WHITESPACES_TRAILING: &str = "editor.whitespaces.trailing";
pub const WHITESPACES_SELECTION: &str = "editor.whitespaces.selection";
pub const SHOW_INDENT_GUIDES: &str = "editor.indentGuides.show";
pub const SHOW_HARD_WRAP_GUIDES: &str = "editor.hardWrapGuides.show";
pub const TAB_RENDERING: &str = "editor.tabRendering";

pub use zen_project::settings::DEFAULT_RIGHT_MARGIN;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ToggleWhitespaces)?;
    registry.register(ToggleIndentGuides)?;
    registry.register(GetAppearance)?;
    Ok(())
}

/// How a tab character is drawn while whitespaces are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TabRendering {
    /// A thin line across the tab's width, the default.
    #[default]
    Line,
    /// An arrow pointing right.
    Arrow,
}

/// Which whitespaces are drawn while they are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WhitespaceKinds {
    /// Before a line's text, and on a line of whitespace only.
    pub leading: bool,
    /// Between a line's words.
    pub inner: bool,
    /// After a line's text, and on a line of whitespace only.
    pub trailing: bool,
    /// Inside a selection, whatever their place.
    pub selection: bool,
}

impl Default for WhitespaceKinds {
    fn default() -> Self {
        Self { leading: true, inner: true, trailing: true, selection: false }
    }
}

/// Settings → Editor → Appearance and Advanced Settings → Editor: what the editor draws besides the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct VisualAidSettings {
    /// Code Structure: the right margin and the visual guides of the code style (on by default).
    pub hard_wrap_guides: bool,
    /// Code Structure: thin lines at each indent level (on by default).
    pub indent_guides: bool,
    /// Presentation: spaces as dots and tabs as lines or arrows (off by default).
    pub whitespaces: bool,
    /// Presentation: which whitespaces are drawn.
    pub whitespace_kinds: WhitespaceKinds,
    /// Advanced Settings → Editor → Tab character rendering (a line by default).
    pub tab_rendering: TabRendering,
}

impl Default for VisualAidSettings {
    fn default() -> Self {
        Self {
            hard_wrap_guides: true,
            indent_guides: true,
            whitespaces: false,
            whitespace_kinds: WhitespaceKinds::default(),
            tab_rendering: TabRendering::Line,
        }
    }
}

impl VisualAidSettings {
    pub fn apply_to(&self, settings: &mut Settings) {
        let _ = settings.set(SHOW_HARD_WRAP_GUIDES, self.hard_wrap_guides);
        let _ = settings.set(SHOW_INDENT_GUIDES, self.indent_guides);
        let _ = settings.set(SHOW_WHITESPACES, self.whitespaces);
        let _ = settings.set(WHITESPACES_LEADING, self.whitespace_kinds.leading);
        let _ = settings.set(WHITESPACES_INNER, self.whitespace_kinds.inner);
        let _ = settings.set(WHITESPACES_TRAILING, self.whitespace_kinds.trailing);
        let _ = settings.set(WHITESPACES_SELECTION, self.whitespace_kinds.selection);
        let _ = settings.set(TAB_RENDERING, self.tab_rendering);
    }

    pub fn from_session(settings: &Settings) -> Self {
        let defaults = Self::default();
        let kinds = defaults.whitespace_kinds;
        Self {
            hard_wrap_guides: settings.get_or(SHOW_HARD_WRAP_GUIDES, defaults.hard_wrap_guides),
            indent_guides: settings.get_or(SHOW_INDENT_GUIDES, defaults.indent_guides),
            whitespaces: settings.get_or(SHOW_WHITESPACES, defaults.whitespaces),
            whitespace_kinds: WhitespaceKinds {
                leading: settings.get_or(WHITESPACES_LEADING, kinds.leading),
                inner: settings.get_or(WHITESPACES_INNER, kinds.inner),
                trailing: settings.get_or(WHITESPACES_TRAILING, kinds.trailing),
                selection: settings.get_or(WHITESPACES_SELECTION, kinds.selection),
            },
            tab_rendering: settings.get_or(TAB_RENDERING, defaults.tab_rendering),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub whitespaces: Option<WhitespaceKinds>,
    pub tab_rendering: TabRendering,
    pub indent_guides: bool,
    pub guides: Vec<u16>,
}

pub fn shows_whitespaces(settings: &Settings, document: &Document) -> bool {
    document.image().is_none() && document.whitespaces().unwrap_or_else(|| settings.get_or(SHOW_WHITESPACES, false))
}

pub fn shows_indent_guides(settings: &Settings, document: &Document) -> bool {
    document.image().is_none() && document.indent_guides().unwrap_or_else(|| settings.get_or(SHOW_INDENT_GUIDES, true))
}

pub fn margins_of(session: &Session, path: &std::path::Path) -> (u16, Vec<u16>) {
    let project = project_settings::code_style_override(session, &code_style::language_of(path)).unwrap_or_default();
    let general = code_style::general_of(session.settings());
    let margins =
        (project.right_margin.unwrap_or(general.hard_wrap_at), project.visual_guides.unwrap_or(general.visual_guides));
    crate::editor_config::margins_over(session, path, margins)
}

pub fn shown(session: &Session, document: &Document) -> Shown {
    let settings = session.settings();
    let aids = VisualAidSettings::from_session(settings);
    let guides = if aids.hard_wrap_guides && document.image().is_none() {
        let (margin, mut guides) = margins_of(session, document.path());
        guides.push(margin);
        guides.sort_unstable();
        guides.dedup();
        guides
    } else {
        Vec::new()
    };
    Shown {
        whitespaces: shows_whitespaces(settings, document).then_some(aids.whitespace_kinds),
        tab_rendering: aids.tab_rendering,
        indent_guides: shows_indent_guides(settings, document),
        guides,
    }
}

/// What the active editor draws besides its text, as commands report it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceState {
    /// The active file.
    pub path: String,
    /// Whether its whitespaces are shown.
    pub whitespaces: bool,
    /// Whether its indent guides are shown.
    pub indent_guides: bool,
    /// The settings every editor follows unless toggled.
    pub settings: VisualAidSettings,
    /// The column of the right margin: the project's code style, else 120.
    pub right_margin: u16,
    /// The columns of the visual guides the project's code style adds.
    pub visual_guides: Vec<u16>,
}

fn active(session: &Session) -> Result<&Document, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

fn state(session: &Session) -> Result<AppearanceState, CommandError> {
    let document = active(session)?;
    let settings = session.settings();
    let (right_margin, visual_guides) = margins_of(session, document.path());
    Ok(AppearanceState {
        path: document.path().display().to_string(),
        whitespaces: shows_whitespaces(settings, document),
        indent_guides: shows_indent_guides(settings, document),
        settings: VisualAidSettings::from_session(settings),
        right_margin,
        visual_guides,
    })
}

fn has_text(session: &Session) -> bool {
    active(session).is_ok()
}

/// The parameters of `editor.whitespaces.toggle` and `editor.indentGuides.toggle`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ToggleParams {
    /// Turn them on (`true`) or off (`false`); toggles them when absent.
    pub enabled: Option<bool>,
}

pub struct ToggleWhitespaces;

impl Command for ToggleWhitespaces {
    type Params = ToggleParams;
    type Output = AppearanceState;
    const ID: &'static str = "editor.whitespaces.toggle";
    const TITLE: &'static str = "Show Whitespaces";
    const DESCRIPTION: &'static str = "Shows or hides the whitespaces of the active file (View → Active Editor → Show \
         Whitespaces): spaces as dots, tabs as lines or arrows. Returns what the editor draws besides the text.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: ToggleParams) -> Result<AppearanceState, CommandError> {
        let on = params.enabled.unwrap_or(!state(session)?.whitespaces);
        let document = session
            .state_mut::<Workbench>()
            .active_document_mut()
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        document.set_whitespaces(Some(on));
        state(session)
    }
}

pub struct ToggleIndentGuides;

impl Command for ToggleIndentGuides {
    type Params = ToggleParams;
    type Output = AppearanceState;
    const ID: &'static str = "editor.indentGuides.toggle";
    const TITLE: &'static str = "Show Indent Guides";
    const DESCRIPTION: &'static str = "Shows or hides the indent guides of the active file (View → Active Editor → Show \
         Indent Guides, also in the gutter's menu under Appearance). Returns what the editor draws besides the text.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: ToggleParams) -> Result<AppearanceState, CommandError> {
        let on = params.enabled.unwrap_or(!state(session)?.indent_guides);
        let document = session
            .state_mut::<Workbench>()
            .active_document_mut()
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        document.set_indent_guides(Some(on));
        state(session)
    }
}

pub struct GetAppearance;

impl Command for GetAppearance {
    type Params = NoParams;
    type Output = AppearanceState;
    const ID: &'static str = "editor.appearance.get";
    const TITLE: &'static str = "Editor Appearance";
    const DESCRIPTION: &'static str = "Tells whether the active file shows its whitespaces and indent guides, the \
         settings of Settings → Editor → Appearance every editor follows, and the right margin and visual guides \
         its code style gives.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<AppearanceState, CommandError> {
        state(session)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_project::settings::{CodeStyleOverride, DEFAULT_LANGUAGE, ProjectSettings};

    use super::*;

    fn open(name: &str) -> (Registry, Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        std::fs::write(&file, "func main() {\n\tx := 1\n}\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        (registry, session, dir)
    }

    fn get(registry: &Registry, session: &mut Session) -> Value {
        registry.dispatch(session, "editor.appearance.get", json!({})).unwrap()
    }

    #[test]
    fn the_visual_aids_start_from_their_defaults() {
        let (registry, mut session, _dir) = open("main.go");
        let state = get(&registry, &mut session);
        assert_eq!((state["whitespaces"].clone(), state["indentGuides"].clone()), (json!(false), json!(true)));
        assert_eq!(state["rightMargin"], 120);
        assert_eq!(state["visualGuides"], json!([]));
        assert_eq!(
            state["settings"],
            json!({
                "hardWrapGuides": true,
                "indentGuides": true,
                "whitespaces": false,
                "whitespaceKinds": { "leading": true, "inner": true, "trailing": true, "selection": false },
                "tabRendering": "line",
            })
        );
    }

    #[test]
    fn the_toggles_change_the_active_file_only() {
        let (registry, mut session, dir) = open("main.go");
        let on = registry.dispatch(&mut session, "editor.whitespaces.toggle", json!({})).unwrap();
        assert_eq!(on["whitespaces"], true);
        assert_eq!(on["settings"]["whitespaces"], false, "the settings stay");
        let off = registry.dispatch(&mut session, "editor.indentGuides.toggle", json!({ "enabled": false })).unwrap();
        assert_eq!((off["whitespaces"].clone(), off["indentGuides"].clone()), (json!(true), json!(false)));
        let other = dir.path().join("other.go");
        std::fs::write(&other, "package main\n").unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": other })).unwrap();
        let state = get(&registry, &mut session);
        assert_eq!((state["whitespaces"].clone(), state["indentGuides"].clone()), (json!(false), json!(true)));
    }

    #[test]
    fn the_settings_apply_to_every_editor_until_toggled() {
        let (registry, mut session, _dir) = open("main.go");
        let aids = VisualAidSettings {
            whitespaces: true,
            indent_guides: false,
            tab_rendering: TabRendering::Arrow,
            whitespace_kinds: WhitespaceKinds { inner: false, ..WhitespaceKinds::default() },
            ..VisualAidSettings::default()
        };
        aids.apply_to(session.settings_mut());
        assert_eq!(VisualAidSettings::from_session(session.settings()), aids);
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        let shown = shown(&session, document);
        assert_eq!(shown.whitespaces, Some(aids.whitespace_kinds));
        assert_eq!(shown.tab_rendering, TabRendering::Arrow);
        assert!(!shown.indent_guides);
        assert_eq!(shown.guides, vec![120]);
        let state = registry.dispatch(&mut session, "editor.whitespaces.toggle", json!({})).unwrap();
        assert_eq!(state["whitespaces"], false);
    }

    struct Margins;

    impl zen_project::settings::SettingsProvider for Margins {
        fn id(&self) -> &str {
            "margins"
        }

        fn watches(&self, _: &std::path::Path) -> bool {
            false
        }

        fn load(&self, _: &std::path::Path, _: &dyn zen_project::settings::ReadFiles) -> Option<ProjectSettings> {
            let mut project = ProjectSettings::default();
            project.code_style.insert(
                DEFAULT_LANGUAGE.to_owned(),
                CodeStyleOverride { right_margin: Some(100), visual_guides: Some(vec![80, 100]), ..Default::default() },
            );
            Some(project)
        }
    }

    #[test]
    fn the_general_page_gives_the_margin_and_the_guides_unless_the_project_sets_them() {
        let (_registry, mut session, dir) = open("main.go");
        let general = code_style::CodeStyleGeneral { hard_wrap_at: 90, visual_guides: vec![70], ..Default::default() };
        code_style::apply_general(session.settings_mut(), &general);
        let path = session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        assert_eq!(margins_of(&session, &path), (90, vec![70]));
        let state = session.state_mut::<project_settings::ProjectSettingsState>();
        state.set_providers(vec![std::sync::Arc::new(Margins)]);
        state.refresh(Some(dir.path()));
        assert_eq!(margins_of(&session, &path), (100, vec![80, 100]));
    }

    #[test]
    fn the_project_code_style_gives_the_margin_and_the_guides() {
        let (_registry, mut session, dir) = open("main.go");
        let state = session.state_mut::<project_settings::ProjectSettingsState>();
        state.set_providers(vec![std::sync::Arc::new(Margins)]);
        state.refresh(Some(dir.path()));
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(margins_of(&session, document.path()), (100, vec![80, 100]));
        assert_eq!(shown(&session, document).guides, vec![80, 100], "the margin is drawn once");
        let _ = session.settings_mut().set(SHOW_HARD_WRAP_GUIDES, false);
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert!(shown(&session, document).guides.is_empty());
    }
}

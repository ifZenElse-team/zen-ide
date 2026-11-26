use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::show_usages::{ShowUsagesAt, ShowUsagesParams, UsagesScope, close as close_popup, kind_at, open_popup};
use super::usages::{FindUsages, FindUsagesParams, word_at};
use super::{LookupParams, file_text};
use crate::Workbench;
use crate::find_results::usages::{OPTIONS, kind_label, scope_label};
use crate::find_results::{FindUsagesOptions, UsagesTarget};

pub const SCOPES: [UsagesScope; 4] =
    [UsagesScope::ProjectFiles, UsagesScope::AllPlaces, UsagesScope::OpenFiles, UsagesScope::CurrentFile];

#[derive(Debug, Default)]
struct Dialog {
    open: bool,
    generation: u64,
    target: Option<UsagesTarget>,
    options: FindUsagesOptions,
    show_usages: bool,
}

/// A scope of the dialog's Scope list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopeChoice {
    /// The scope.
    pub scope: UsagesScope,
    /// Its name.
    pub name: String,
}

/// What the Find Usages Settings dialog shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsagesSettingsState {
    /// Whether the dialog is open.
    pub open: bool,
    /// Raised by every change of what the dialog shows.
    pub generation: u64,
    /// What Find will look for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<UsagesTarget>,
    /// The symbol as the dialog names it: "Variable Version".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
    /// The options as the dialog has them: those of the session until changed in the dialog.
    pub options: FindUsagesOptions,
    /// Whether Find shows the usages in the Show Usages popup, whose settings these are, rather
    /// than in the Find tool window.
    pub show_usages: bool,
    /// The scopes it offers, with their names.
    pub scopes: Vec<ScopeChoice>,
}

fn state(session: &Session) -> UsagesSettingsState {
    let saved = FindUsagesOptions::of(session);
    let scopes = SCOPES.iter().map(|scope| ScopeChoice { scope: *scope, name: scope_label(*scope) }).collect();
    let Some(dialog) = session.state::<Dialog>().filter(|dialog| dialog.open) else {
        let generation = session.state::<Dialog>().map_or(0, |dialog| dialog.generation);
        return UsagesSettingsState { generation, options: saved, scopes, ..UsagesSettingsState::default() };
    };
    UsagesSettingsState {
        open: true,
        generation: dialog.generation,
        target_name: dialog.target.as_ref().map(|target| {
            t!("workbench-results-usages-target", kind = kind_label(target.kind), symbol = target.symbol.as_str())
        }),
        target: dialog.target.clone(),
        options: dialog.options,
        show_usages: dialog.show_usages,
        scopes,
    }
}

fn is_text(session: &Session) -> bool {
    !session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .is_some_and(|document| document.image().is_some())
}

pub struct FindUsagesSettings;

impl Command for FindUsagesSettings {
    type Params = ShowUsagesParams;
    type Output = UsagesSettingsState;
    const ID: &'static str = "navigate.usages.settings";
    const TITLE: &'static str = "Find Usages Settings…";
    const DESCRIPTION: &'static str = "Find Usages Settings (⌥⇧⌘F7): opens the Find Usages dialog for the symbol at a \
         place (the caret by default), or for the Show Usages popup while it is open, with the Scope, Open in new \
         tab and Skip results tab with one usage. navigate.usagesSettings.set changes them, \
         navigate.usagesSettings.find keeps them and finds, navigate.usagesSettings.cancel closes it.";

    fn is_available(&self, session: &Session) -> bool {
        is_text(session)
    }

    fn run(&self, session: &mut Session, params: ShowUsagesParams) -> Result<UsagesSettingsState, CommandError> {
        let mut options = FindUsagesOptions::of(session);
        let popup = open_popup(session);
        let (path, offset, show_usages) = match popup {
            Some((path, offset, scope)) => {
                options.scope = scope;
                close_popup(session);
                (path, offset, true)
            }
            None => {
                let lookup = LookupParams {
                    path: params.path.clone(),
                    offset: params.offset,
                    line: params.line,
                    column: params.column,
                    wait_ms: None,
                };
                let (path, offset) = lookup.place(session.state_mut::<Workbench>())?;
                (path, offset, false)
            }
        };
        let text = file_text(session.state_mut::<Workbench>(), &path).map_err(CommandError::new)?;
        let symbol = word_at(&text, offset).ok_or_else(|| CommandError::new(t!("workbench-nav-no-symbol")))?;
        if let Some(scope) = params.scope {
            options.scope = scope;
        }
        let kind = kind_at(session, &path, offset, &symbol, &[]);
        let (line, column) = crate::find_results::usages::line_context(&text, offset);
        let target = UsagesTarget { path, offset, symbol, kind, scope: options.scope, line, column };
        let dialog = session.state_mut::<Dialog>();
        *dialog = Dialog { open: true, generation: dialog.generation + 1, target: Some(target), options, show_usages };
        Ok(state(session))
    }
}

pub struct GetUsagesSettings;

impl Command for GetUsagesSettings {
    type Params = NoParams;
    type Output = UsagesSettingsState;
    const ID: &'static str = "navigate.usagesSettings.get";
    const TITLE: &'static str = "Find Usages Settings State";
    const DESCRIPTION: &'static str = "What the Find Usages Settings dialog shows (`open`, the target, the options it \
         has), or the Find Usages options of the window while it is closed.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<UsagesSettingsState, CommandError> {
        Ok(state(session))
    }
}

/// The parameters of `navigate.usagesSettings.set`: the options to change, the others kept.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UsagesSettingsParams {
    /// Scope.
    pub scope: Option<UsagesScope>,
    /// Open in new tab.
    pub new_tab: Option<bool>,
    /// Skip results tab with one usage.
    pub skip_single: Option<bool>,
}

pub struct SetUsagesSettings;

impl Command for SetUsagesSettings {
    type Params = UsagesSettingsParams;
    type Output = UsagesSettingsState;
    const ID: &'static str = "navigate.usagesSettings.set";
    const TITLE: &'static str = "Set Find Usages Settings";
    const DESCRIPTION: &'static str = "Changes the Find Usages options (`scope`, `newTab`: Open in new tab, \
         `skipSingle`: Skip results tab with one usage): in the dialog while it is open, until Find keeps them; \
         otherwise at once, for the next Find Usages.";

    fn run(&self, session: &mut Session, params: UsagesSettingsParams) -> Result<UsagesSettingsState, CommandError> {
        let open = session.state::<Dialog>().is_some_and(|dialog| dialog.open);
        let mut options = if open { session.state_mut::<Dialog>().options } else { FindUsagesOptions::of(session) };
        options.scope = params.scope.unwrap_or(options.scope);
        options.new_tab = params.new_tab.unwrap_or(options.new_tab);
        options.skip_single = params.skip_single.unwrap_or(options.skip_single);
        if open {
            let dialog = session.state_mut::<Dialog>();
            dialog.options = options;
            dialog.generation += 1;
            if let Some(target) = &mut dialog.target {
                target.scope = options.scope;
            }
        } else {
            session.settings_mut().set(OPTIONS, options).map_err(|error| CommandError::new(error.to_string()))?;
            session.state_mut::<Dialog>().generation += 1;
        }
        Ok(state(session))
    }
}

/// The parameters of `navigate.usagesSettings.find`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UsagesFindParams {
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    pub wait_ms: Option<u64>,
}

pub struct FindFromSettings;

impl Command for FindFromSettings {
    type Params = UsagesFindParams;
    type Output = UsagesSettingsState;
    const ID: &'static str = "navigate.usagesSettings.find";
    const TITLE: &'static str = "Find";
    const DESCRIPTION: &'static str = "Find in the Find Usages Settings dialog: keeps its options for the window, closes \
         it and finds the usages, in the Find tool window (navigate.usages.list, search.results.tree) or, for the \
         Show Usages popup, in the popup (navigate.showUsages.get).";

    fn run(&self, session: &mut Session, params: UsagesFindParams) -> Result<UsagesSettingsState, CommandError> {
        let dialog = session.state_mut::<Dialog>();
        if !dialog.open {
            return Err(CommandError::new(t!("workbench-usages-settings-closed")));
        }
        dialog.open = false;
        dialog.generation += 1;
        let options = dialog.options;
        let show_usages = dialog.show_usages;
        let target = dialog.target.take().ok_or_else(|| CommandError::new(t!("workbench-nav-no-symbol")))?;
        session.settings_mut().set(OPTIONS, options).map_err(|error| CommandError::new(error.to_string()))?;
        if show_usages {
            let params = ShowUsagesParams {
                path: Some(target.path),
                offset: Some(target.offset),
                line: None,
                column: None,
                wait_ms: params.wait_ms,
                scope: Some(options.scope),
            };
            ShowUsagesAt.run(session, params)?;
        } else {
            let params = FindUsagesParams {
                path: Some(target.path),
                offset: Some(target.offset),
                line: None,
                column: None,
                wait_ms: params.wait_ms,
                scope: Some(options.scope),
                new_tab: Some(options.new_tab),
                include_declaration: None,
                ..FindUsagesParams::default()
            };
            FindUsages.run(session, params)?;
        }
        Ok(state(session))
    }
}

pub struct CancelUsagesSettings;

impl Command for CancelUsagesSettings {
    type Params = NoParams;
    type Output = UsagesSettingsState;
    const ID: &'static str = "navigate.usagesSettings.cancel";
    const TITLE: &'static str = "Cancel Find Usages Settings";
    const DESCRIPTION: &'static str = "Closes the Find Usages Settings dialog without keeping what was changed in it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<UsagesSettingsState, CommandError> {
        let dialog = session.state_mut::<Dialog>();
        dialog.open = false;
        dialog.target = None;
        dialog.generation += 1;
        Ok(state(session))
    }
}

pub fn dialog_generation(session: &Session) -> Option<(bool, u64)> {
    session.state::<Dialog>().map(|dialog| (dialog.open, dialog.generation))
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(FindUsagesSettings)?;
    registry.register(GetUsagesSettings)?;
    registry.register(SetUsagesSettings)?;
    registry.register(FindFromSettings)?;
    registry.register(CancelUsagesSettings)?;
    Ok(())
}

use serde_json::{Value, json};
use zen_core::keymap::Keystroke;
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;

use super::tool_windows::Anchor;
use super::{IdeView, run_anything};
use zen_workbench::notifications::{Group, Kind, NewNotification};

pub(super) type Reply = Box<dyn FnOnce(Result<Value, String>) + Send>;

pub(super) struct KeyPress {
    pub(super) keystrokes: Vec<Keystroke>,
    pub(super) text: Option<String>,
}

#[derive(Debug)]
pub(super) enum ActionError {
    Dispatch(DispatchError),
    Refused(String),
}

pub(super) type Queued = (KeyPress, Reply, Option<Value>);

pub(super) struct Pressed {
    reply: Reply,
    before: Value,
    result: Result<(), String>,
}

impl Pressed {
    pub(super) fn delivered(&self) -> bool {
        self.result.is_ok()
    }
}

impl IdeView {
    pub(super) fn run_for_agent(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        (action, command, params): (&str, &str, Value),
    ) -> Result<Value, String> {
        let before = self.window_state(session);
        let result = match self.perform_action(ctx, registry, session, command, params, true) {
            Ok(result) => result,
            Err(ActionError::Refused(message)) => return Err(message),
            Err(ActionError::Dispatch(error)) => {
                let message = match &error {
                    DispatchError::Failed { error, .. } => error.message.clone(),
                    other => other.to_string(),
                };
                self.report_at(ctx, error);
                self.notifications.flush(session);
                return Err(message);
            }
        };
        self.git_workspace_frame(ctx, session);
        self.notifications.flush(session);
        let window = self.window_state(session);
        let changed = window != before;
        Ok(json!({ "action": action, "result": result, "window": window, "changed": changed }))
    }

    pub(super) fn queue_keys(&mut self, keys: KeyPress, reply: Reply) {
        self.agent_keys.push((keys, reply, None));
    }

    pub(super) fn agent_keys(&mut self, ui: &mut egui::Ui, session: &Session) -> Vec<Pressed> {
        let mut pressed = Vec::new();
        if let Some(dialog) = self.new_project.as_mut().filter(|_| !self.agent_keys.is_empty()) {
            dialog.by_agent = true;
        }
        if let Some(dialog) = self.clone.as_mut().filter(|_| !self.agent_keys.is_empty()) {
            dialog.by_agent = true;
        }
        for (mut keys, reply, before) in std::mem::take(&mut self.agent_keys) {
            let before = before.unwrap_or_else(|| self.window_state(session));
            if self.restart_prompt.is_some()
                || self.invalidate_dialog.is_some()
                || self.manage_settings.person_answers()
                || !zen_net::trust::pending().is_empty()
            {
                pressed.push(Pressed { reply, before, result: Err(t!("ide-agent-person-answers")) });
                continue;
            }
            if !keys.keystrokes.is_empty() && keys.text.as_deref().is_some_and(|text| !text.is_empty()) {
                let text = keys.text.take();
                match press(ui, &keys) {
                    Ok(()) => self.agent_keys.push((KeyPress { keystrokes: Vec::new(), text }, reply, Some(before))),
                    Err(error) => pressed.push(Pressed { reply, before, result: Err(error) }),
                }
                continue;
            }
            let result = press(ui, &keys);
            pressed.push(Pressed { reply, before, result });
        }
        if !pressed.is_empty() || !self.agent_keys.is_empty() {
            ui.ctx().request_repaint();
        }
        pressed
    }

    pub(super) fn refuse_agent_settings(&mut self) {
        let current = self.overview.settings.clone();
        let mut refused = Vec::new();
        self.requests.retain(|request| match request {
            super::app::Request::Settings(settings) => match super::automation::validate(&current, settings) {
                Ok(()) => true,
                Err(message) => {
                    refused.push(message);
                    false
                }
            },
            _ => true,
        });
        if let Some(message) = refused.pop() {
            self.notify(NewNotification::message(Group::General, Kind::Warning, message));
        }
    }

    pub(super) fn answer_keys(&self, session: &Session, pressed: Vec<Pressed>) {
        for Pressed { reply, before, result } in pressed {
            let window = self.window_state(session);
            let changed = window != before;
            reply(result.map(|()| json!({ "window": window, "changed": changed })));
        }
    }

    pub(super) fn window_state(&self, session: &Session) -> Value {
        let left = self.shown_at(Anchor::Left);
        let bottom = self.shown_at(Anchor::Bottom);
        let right = self.shown_at(Anchor::Right);
        let mut popups: Vec<String> = Vec::new();
        if let Some(page) = self.settings.open_page() {
            popups.push(format!("settings:{}", page.name()));
        }
        if let Some(tab) = self.search_everywhere.open_tab() {
            popups.push(format!("searchEverywhere:{}", tab.id()));
            if self.search_everywhere.is_recording() {
                popups.push("keyboardShortcut".to_owned());
            }
        }
        match self.find_in_files.state() {
            Some(true) => popups.push("replaceInFiles".to_owned()),
            Some(false) => popups.push("findInFiles".to_owned()),
            None => {}
        }
        let dialogs = [
            (self.find_window.asking.is_some(), "tooManyResults"),
            (self.tip_is_open(), "tipOfTheDay"),
            (self.productivity.is_some(), "myProductivity"),
            (self.recent_files.name() == Some("recentFiles"), "recentFiles"),
            (self.recent_files.name() == Some("switcher"), "switcher"),
            (self.recent_locations.is_open(), "recentLocations"),
            (self.file_structure.is_open(), "fileStructure"),
            (self.select_in.is_open(), "selectIn"),
            (self.new_project.is_some(), "newProject"),
            (self.clone.is_some(), "clone"),
            (self.starters.is_some(), "starters"),
            (self.projects.is_open(), "projects"),
            (self.artifacts_popup.is_some(), "buildArtifacts"),
            (self.paste_history.is_open(), "pasteFromHistory"),
            (self.run_anything.mode() == Some(run_anything::Mode::Anything), "runAnything"),
            (
                matches!(self.run_anything.mode(), Some(run_anything::Mode::Choose { debug: false })),
                "chooseRunConfiguration",
            ),
            (
                matches!(self.run_anything.mode(), Some(run_anything::Mode::Choose { debug: true })),
                "chooseDebugConfiguration",
            ),
            (self.recent_usages.is_open(), "recentFindUsages"),
            (self.usages_settings.is_open(), "findUsagesSettings"),
            (self.quick_switch.is_open(), "quickSwitchScheme"),
            (self.run_configurations.is_open(), "runConfigurations"),
            (self.rename.is_open(), "rename"),
            (self.refactoring.kind() == Some("refactoringLoading"), "refactoringLoading"),
            (self.refactoring.kind() == Some("refactoringDialog"), "refactoringDialog"),
            (self.refactoring.kind() == Some("refactoringConflicts"), "refactoringConflicts"),
            (self.editor_view.intentions_open(), "intentions"),
            (self.editor_view.generate_open() == Some("generate"), "generate"),
            (self.editor_view.generate_open() == Some("memberChooser"), "memberChooser"),
            (self.editor_view.custom_folding_open(), "customFolding"),
            (self.editor_view.surround_open() == Some("surround"), "surround"),
            (self.editor_view.surround_open() == Some("surroundWithTemplate"), "surroundWithTemplate"),
            (self.editor_view.unwrap_open(), "unwrap"),
            (self.editor_view.extract_open() == Some("extractExpressions"), "extractExpressions"),
            (self.editor_view.extract_open() == Some("extractVariants"), "extractVariants"),
            (self.editor_view.extract_open() == Some("inPlaceName"), "inPlaceName"),
            (self.editor_view.usages_open(), "showUsages"),
            (self.goto_line.is_some(), "gotoLine"),
            (self.rollback.is_some(), "rollback"),
            (self.new_entry.is_some(), "newEntry"),
            (self.tree_menu.is_some(), "projectMenu"),
            (self.delete_entry.is_some(), "delete"),
            (self.associate_dialog.is_some(), "associateFileType"),
            (self.create_test.is_some(), "createTest"),
            (self.test_history.is_open(), "testHistory"),
            (self.errors_dialog.is_some(), "internalErrors"),
            (self.connection_dialog.is_some(), "connectionDialog"),
            (self.import_dialog.is_some(), "importConnections"),
            (self.export_dialog.is_some(), "exportData"),
            (self.backup_dialog.is_some(), "backupDatabase"),
            (self.csv_import_dialog.is_some(), "importData"),
            (self.transfer_dialog.is_some(), "transferData"),
            (self.ssh_dialog.is_open(), "sshPrompt"),
            (self.exit_prompt.is_some(), "confirmExit"),
            (!zen_net::trust::pending().is_empty(), "untrustedCertificate"),
            (self.restart_prompt.is_some(), "confirmRestart"),
            (self.invalidate_dialog.is_some(), "invalidateCaches"),
            (self.overview.trust_question, "trustProject"),
            (self.ide_zoom_dialog, "zoomIde"),
            (self.external_tool_prompt_open(), "externalToolPrompt"),
            (self.rerun_confirmation.is_some(), "rerunConfirmation"),
            (self.local_history.is_history_open(), "localHistory"),
            (self.local_history.is_label_open(), "localHistoryLabel"),
            (self.new_popup.is_some(), "newElement"),
            (self.new_from_template.is_some(), "newFromTemplate"),
            (self.save_as_template.is_some(), "saveFileAsTemplate"),
            (self.navigation_bar.is_active(), "navigationBar"),
            (self.navigation_bar.popup_open(), "navigationBarPopup"),
            (self.indent_ui.menu_open, "indentMenu"),
            (self.status_menu_open(), "statusBarWidgets"),
            (self.activity_monitor_open(), "activityMonitor"),
            (self.diagnostics.debug_log.is_some(), "debugLogSettings"),
            (self.diagnostics.special.is_some(), "specialFiles"),
            (self.diagnostics.collect, "collectLogs"),
            (self.diagnostics.licences.is_some(), "openSourceLicences"),
        ];
        popups.extend(dialogs.into_iter().filter(|(open, _)| *open).map(|(_, name)| name.to_owned()));
        popups.extend(self.layout_dialog_name().map(str::to_owned));
        popups.extend(self.changes_dialog.as_ref().map(|dialog| dialog.name().to_owned()));
        popups.extend(self.transaction_dialogs.open.as_ref().map(|dialog| dialog.name().to_owned()));
        popups.extend(self.scratch.open_names().into_iter().map(str::to_owned));
        popups.extend(self.patch_dialog.as_ref().map(|dialog| dialog.name().to_owned()));
        popups.extend(self.bookmarks.name().map(str::to_owned));
        popups.extend(self.breakpoint_dialogs.names().into_iter().map(str::to_owned));
        popups.extend(self.evaluations.names().into_iter().map(str::to_owned));
        popups.extend(self.smart_step.name().map(str::to_owned));
        popups.extend(self.stream_traces.name().map(str::to_owned));
        popups.extend(self.manage_settings.open_dialogs().into_iter().map(str::to_owned));
        popups.extend(self.open_vcs_dialogs().into_iter().map(str::to_owned));
        popups.extend(self.macro_dialogs.open_names().into_iter().map(str::to_owned));
        popups.extend(self.open_workspace_dialogs().map(str::to_owned));
        if self.git_in_front(session)
            && session.state::<zen_workbench::vcs::workspace::GitWorkspace>().is_some_and(|w| w.menu().is_some())
        {
            popups.push("commitMenu".to_owned());
        }
        if self.git_in_front(session)
            && session.state::<zen_workbench::vcs::workspace::GitWorkspace>().is_some_and(|w| w.ref_menu().is_some())
        {
            popups.push("refMenu".to_owned());
        }
        if self.git_in_front(session)
            && session.state::<zen_workbench::vcs::workspace::GitWorkspace>().is_some_and(|w| w.stash_menu().is_some())
        {
            popups.push("stashMenu".to_owned());
        }
        if self.git_in_front(session)
            && session.state::<zen_workbench::vcs::workspace::GitWorkspace>().is_some_and(|w| w.drop_menu().is_some())
        {
            popups.push("dropMenu".to_owned());
        }
        if self.git_in_front(session)
            && session
                .state::<zen_workbench::vcs::workspace::GitWorkspace>()
                .is_some_and(|w| w.worktree_menu().is_some())
        {
            popups.push("worktreeMenu".to_owned());
        }
        if self.git_in_front(session)
            && session
                .state::<zen_workbench::vcs::workspace::GitWorkspace>()
                .is_some_and(|w| w.submodule_menu().is_some())
        {
            popups.push("submoduleMenu".to_owned());
        }
        if self.git_in_front(session)
            && session.state::<zen_workbench::vcs::workspace::GitWorkspace>().is_some_and(|w| w.file_menu().is_some())
        {
            popups.push("fileMenu".to_owned());
        }
        popups.extend(self.git_window.open_dialogs().into_iter().map(str::to_owned));
        popups.extend(self.editor_view.problem_popups_open().into_iter().map(str::to_owned));
        if session.state::<zen_workbench::tasks::Tasks>().is_some_and(zen_workbench::tasks::Tasks::panel_shown) {
            popups.push("backgroundTasks".to_owned());
        }
        let (workspace, workspace_popups) = self.workspace_window_state(session);
        popups.extend(workspace_popups);
        let editor = if self.diff_in_front(session) {
            json!("diff")
        } else if self.rebase_in_front(session) {
            json!("rebaseEditor")
        } else if self.conflict_in_front(session) {
            json!("conflictEditor")
        } else if self.git_in_front(session) {
            json!("gitWorkspace")
        } else if zen_workbench::workspaces::tab_in_front(session).is_some() {
            json!("workspace")
        } else if self.folder_in_front(session) {
            json!("folderDiff")
        } else {
            json!(
                session
                    .state::<zen_workbench::Workbench>()
                    .and_then(zen_workbench::Workbench::active_document)
                    .map(|document| document.path())
            )
        };
        let splits = session
            .state::<zen_workbench::Workbench>()
            .map(zen_workbench::Workbench::groups)
            .filter(|groups| groups.is_split());
        let (appearance, full_screen) = self.appearance_state();
        let preview = session
            .state::<zen_workbench::Workbench>()
            .and_then(zen_workbench::Workbench::active_document)
            .and_then(|document| {
                let kind = zen_workbench::preview::PreviewKind::of(document.path())?;
                let previews = session.state::<zen_workbench::preview::Previews>();
                let mode = previews.map_or(kind.default_mode(), |previews| previews.mode_of(document.path(), kind));
                let scope = previews.map_or(zen_workbench::preview::ModeScope::Default, |previews| {
                    previews.scope_of(document.path(), kind)
                });
                let orientation = previews.map(|previews| previews.orientation_of(document.path())).unwrap_or_default();
                Some(json!({ "kind": kind, "mode": mode, "orientation": orientation, "scope": scope }))
            });
        let mut build_tools = self.build_tools.state(
            &session
                .state::<zen_workbench::Workbench>()
                .and_then(zen_workbench::Workbench::project)
                .map(std::path::Path::to_owned)
                .unwrap_or_default(),
            |id| self.tool_visible(id),
        );
        build_tools["ant"] = self.ant_state();
        build_tools["npm"] = self.npm_state(session);
        build_tools["endpoints"] = self.endpoints_state();
        build_tools["dependencies"] = self.dependencies_state();
        json!({
            "left": left,
            "bottom": bottom,
            "right": right,
            "toolWindows": self.layout_state(),
            "popups": popups,
            "internalErrors": session.state::<zen_workbench::internal_errors::InternalErrors>().map_or(0, zen_workbench::internal_errors::InternalErrors::count),
            "safeMode": self.safe_mode,
            "editor": editor,
            "workspace": workspace,
            "preview": preview,
            "appearance": appearance,
            "fullScreen": full_screen,
            "ideZoom": self.ide_zoom(),
            "splits": splits,
            "projectView": { "alwaysSelectOpened": self.always_select_opened },
            "todo": self.todo_window.state(),
            "buildTools": build_tools,
            "buildSync": self.build_sync.state(self.tool_visible("build")),
            "dependencyAnalyzer": self.dependency_analyzer.state(self.tool_visible("build")),
            "newProject": self.new_project.as_ref().map(super::new_project_dialog::NewProjectDialog::state),
            "clone": self.clone.as_ref().map(super::clone_dialog::CloneDialog::state),
            "starters": self.starters.as_ref().map(super::starters_dialog::StartersDialog::state),
            "caffeine": self.caffeine_ui.look.map(super::caffeine::sprite::Look::name),
            "diff": self.diff.as_ref().map(super::diff_view::DiffTab::summary),
            "folderDiff": self.folder_diff.as_ref().map(|tab| tab.agent_state(session)),
            "resourceBundle": self.bundle.as_ref().map(super::bundle_view::BundleTab::agent_state),
            "statusBar": {
                "powerSaveMode": self.power_save_on(),
                "widgets": super::status_widgets::widgets_state(&self.overview.settings),
            },
        })
    }
}

fn press(ui: &mut egui::Ui, keys: &KeyPress) -> Result<(), String> {
    let mut events = Vec::new();
    let strokes = keys
        .keystrokes
        .iter()
        .flat_map(|stroke| std::iter::once(stroke.first()).chain(stroke.then.as_deref().cloned()));
    for stroke in strokes {
        if stroke.key == crate::keypad::MULTIPLY {
            crate::keypad::press(stroke.control, stroke.alt, stroke.shift, stroke.command);
            continue;
        }
        let key = egui::Key::ALL
            .iter()
            .find(|key| format!("{key:?}") == stroke.key)
            .ok_or_else(|| format!("`{stroke}` is not a key the window knows"))?;
        let modifiers = egui::Modifiers {
            alt: stroke.alt,
            ctrl: stroke.control,
            shift: stroke.shift,
            mac_cmd: stroke.command,
            command: stroke.command,
        };
        for pressed in [true, false] {
            events.push(egui::Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers });
        }
    }
    if let Some(text) = keys.text.as_deref().filter(|text| !text.is_empty()) {
        events.push(egui::Event::Text(text.to_owned()));
    }
    ui.input_mut(|input| input.events.extend(events));
    Ok(())
}

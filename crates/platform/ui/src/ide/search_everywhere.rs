use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Align2, Key, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::keymap::{Binding, Context, Keymap, Keystroke};
use zen_core::{CommandInfo, Registry, Session};
use zen_i18n::t;
use zen_modules::search::{SearchContributor, SearchItem, SearchProgress, SearchQuery, SearchSink, SearchTarget};
use zen_workbench::search_everywhere::{self as engine, SearchAnswer, SearchHistory, SearchRequest, SearchRun};
use zen_workbench::search_scopes::PROJECT_FILES;

use super::glyphs::{glyph_open_in_find, glyph_search};
use super::keymap_page::{
    GROUPS, Recorded, action_title, action_title_english, group_of, shortcut_dialog, take_keystroke,
};
use super::scope_row::{ScopeChoices, includes_non_project, non_project_scope, scope_combo};
use super::search_filter::glyph_funnel;
use super::settings::{PageId, SECTIONS};
use super::usages_popup::glyph_preview;
use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

const ROW: f32 = 26.0;
const VISIBLE: usize = 14;

const MASK_WIDTH: f32 = 120.0;

const MASK_MIN_WIDTH: f32 = 64.0;

const MASK_CHEVRON: f32 = 24.0;

const SCOPE_MIN_WIDTH: f32 = 46.0;

const MASK_FIELD: &str = "se-text-mask";

const TOGGLES_WIDTH: f32 = 88.0;

const TOOL_BUTTONS: f32 = 3.0 * 26.0;
const POPUP: super::popup_size::PopupKind =
    super::popup_size::PopupKind { name: "searchEverywhere", min: vec2(400.0, ROW * 3.0) };

const CHROME: f32 = 90.0;

const ENTER_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Tab(Cow<'static, str>);

impl Tab {
    pub(super) const ALL: Self = Self(Cow::Borrowed(engine::ALL));
    pub(super) const CLASSES: Self = Self(Cow::Borrowed("classes"));
    pub(super) const FILES: Self = Self(Cow::Borrowed("files"));
    pub(super) const SYMBOLS: Self = Self(Cow::Borrowed("symbols"));
    pub(super) const ACTIONS: Self = Self(Cow::Borrowed("actions"));
    pub(super) const TEXT: Self = Self(Cow::Borrowed("text"));
    pub(super) const DATABASE: Self = Self(Cow::Borrowed("database"));

    pub(super) fn id(&self) -> &str {
        &self.0
    }
}

impl Default for Tab {
    fn default() -> Self {
        Self::ALL
    }
}

fn tabs(session: &Session) -> Vec<(Tab, String)> {
    std::iter::once((Tab::ALL, "ide-se-all".to_owned()))
        .chain(
            engine::contributors(session)
                .iter()
                .filter(|contributor| contributor.has_tab())
                .map(|contributor| (Tab(Cow::Owned(contributor.id().to_owned())), contributor.title().to_owned())),
        )
        .collect()
}

pub(super) struct ActionsContributor {
    registry: Arc<Registry>,
}

impl ActionsContributor {
    pub(super) fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }
}

impl SearchContributor for ActionsContributor {
    fn id(&self) -> &str {
        Tab::ACTIONS.id()
    }

    fn title(&self) -> &str {
        "ide-se-actions"
    }

    fn weight(&self) -> i32 {
        400
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        if query.text.trim().is_empty() {
            return SearchProgress::Done;
        }
        let disabled =
            session.state::<SearchHistory>().is_some_and(|history| history.options().include_disabled_actions);
        let found = find_actions(&query.text, &self.registry, session, usize::MAX);
        for action in found.into_iter().filter(|action| disabled || action.available) {
            let opens = (action.command != action.binding.command).then(|| action.binding.command.clone());
            let (detail, detail_ranges) = match action.english {
                Some((english, ranges)) => (english, ranges),
                None => (action.group, Vec::new()),
            };
            let item = SearchItem {
                name: action.name,
                detail,
                hint: action.shortcut,
                ranges: action.ranges.iter().map(|range| [range.start, range.end]).collect(),
                detail_ranges: detail_ranges.iter().map(|range| [range.start, range.end]).collect(),
                in_project: true,
                available: action.available,
                target: SearchTarget::Action {
                    action: action.binding.action,
                    command: action.command,
                    opens,
                    params: action.binding.params,
                },
            };
            if !sink.push(item) {
                break;
            }
        }
        SearchProgress::Done
    }
}

pub(super) struct CommandsContributor {
    registry: Arc<Registry>,
}

impl CommandsContributor {
    pub(super) fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }

    fn plugins(&self, session: &mut Session, filter: &str, sink: &mut SearchSink) {
        let query = json!({ "query": filter });
        let modules = self.registry.dispatch(session, "plugins.modules.list", query.clone()).unwrap_or_default();
        let installed = self.registry.dispatch(session, "plugins.installed.list", query).unwrap_or_default();
        let listed = modules["modules"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|module| (module, "plugins.module"))
            .chain(installed["plugins"].as_array().into_iter().flatten().map(|plugin| (plugin, "plugins.plugin")));
        for (entry, area) in listed {
            let (Some(id), Some(name)) = (entry["id"].as_str(), entry["name"].as_str()) else { continue };
            let on = entry["enabled"].as_bool().unwrap_or(false);
            let restart = entry["restartRequired"].as_bool().unwrap_or(false);
            let item = SearchItem {
                name: name.to_owned(),
                detail: entry["description"].as_str().unwrap_or_default().to_owned(),
                hint: restart.then(|| t!("ide-se-restart")),
                ranges: Vec::new(),
                detail_ranges: Vec::new(),
                in_project: true,
                available: true,
                target: SearchTarget::Switch {
                    on,
                    command: format!("{area}.{}", if on { "disable" } else { "enable" }),
                    params: json!({ "id": id }),
                },
            };
            if !sink.push(item) {
                break;
            }
        }
    }
}

fn slash_groups() -> impl Iterator<Item = (String, &'static super::settings::SectionDef)> {
    SECTIONS.iter().filter(|section| section.level == 0).map(|section| {
        let command = match section.page {
            PageId::VersionControl => "vcs".to_owned(),
            page => page.name().to_lowercase(),
        };
        (command, section)
    })
}

impl SearchContributor for CommandsContributor {
    fn id(&self) -> &str {
        "commands"
    }

    fn title(&self) -> &str {
        "ide-se-commands"
    }

    fn has_tab(&self) -> bool {
        false
    }

    fn weight(&self) -> i32 {
        10
    }

    fn commands(&self) -> bool {
        true
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        let Some(typed) = query.text.trim_start().strip_prefix('/') else { return SearchProgress::Done };
        let (word, filter) = match typed.split_once(char::is_whitespace) {
            Some((word, filter)) => (word, Some(filter.trim())),
            None => (typed, None),
        };
        if filter.is_none() {
            for (command, section) in slash_groups() {
                let ranges = if word.is_empty() {
                    Vec::new()
                } else {
                    let Some(found) = zen_index::NamePattern::new(word).matches(&command) else { continue };
                    found.ranges.iter().map(|range| [range.start + 1, range.end + 1]).collect()
                };
                let item = SearchItem {
                    name: format!("/{command}"),
                    detail: t!(section.key),
                    hint: None,
                    ranges,
                    detail_ranges: Vec::new(),
                    in_project: true,
                    available: true,
                    target: SearchTarget::Settings { page: section.page.name() },
                };
                if !sink.push(item) {
                    return SearchProgress::Done;
                }
            }
        }
        if word.eq_ignore_ascii_case("plugins") {
            self.plugins(session, filter.unwrap_or_default(), sink);
        }
        SearchProgress::Done
    }
}

pub(super) fn command_ui(id: &str) -> Option<(&'static str, Value)> {
    let settings = |page: &str| Some(("view.settings.open", json!({ "page": page })));
    let action = |action: &'static str| Some((action, json!({})));
    if let Some(page) = module_page_of(id) {
        return settings(page);
    }
    match id {
        "ui.theme.select" | "ui.theme.import" | "ui.theme.list" | "ui.theme.reload" | "ui.theme.followSystem" => {
            settings("appearance")
        }
        "ui.fonts.list" => settings("editorFont"),
        _ if id.starts_with("ui.customization.") => settings("menusAndToolbars"),
        "file.scratch.new" | "file.scratch.languages" => action("view.scratch.new"),
        "file.scratch.newBuffer" => action("view.scratch.newBuffer"),
        "file.scratch.list" => action("view.scratch.show"),
        "ui.notifications.list" | "ui.notifications.action" | "ui.notifications.post" => {
            action("view.toolWindow.notifications")
        }
        "editor.breadcrumbs.get" => settings("editor"),
        _ if id.starts_with("ui.keymap.") => settings("keymap"),
        "plugins.areas.show" => None,
        "plugins.areas.list" | "plugins.areas.choose" | "plugins.areas.skip" | "plugins.modules.list" => {
            Some(("view.settings.open", json!({ "page": "plugins", "tab": "builtIn" })))
        }
        _ if id.starts_with("plugins.") => settings("plugins"),
        "language.servers.status"
        | "lsp.servers.list"
        | "lsp.server.install"
        | "lsp.server.restart"
        | "lsp.server.configure"
        | "lsp.server.undismiss" => settings("languageServers"),
        "db.dataSource.add"
        | "db.dataSource.update"
        | "db.dataSource.test"
        | "db.connection.setSecret"
        | "db.detect.accept" => action("view.dataSource.open"),
        "db.import.dbeaver" => Some(("view.dataSource.import", json!({ "from": "dbeaver" }))),
        "db.import.jetbrains" => Some(("view.dataSource.import", json!({ "from": "jetbrains" }))),
        "db.export.start" => action("view.database.export"),
        "db.csvImport.start" => action("view.database.import"),
        "db.backup.start" => action("view.database.backup"),
        "db.backup.restore" => action("view.database.restore"),
        "db.transfer.start" => action("view.database.transfer"),
        "db.transaction.pending" | "db.transaction.status" => action("view.database.pendingTransactions"),
        "db.transaction.mode" => action("view.database.transactionMode"),
        "db.history.list"
        | "db.history.get"
        | "db.history.clear"
        | "db.history.settings"
        | "db.history.openInConsole" => Some(("db.history.open", json!({}))),
        "db.compare.schemas"
        | "db.compare.data"
        | "db.compare.state"
        | "db.compare.cancel"
        | "db.compare.ddl"
        | "db.compare.script"
        | "db.compare.saveSnapshot"
        | "db.compare.snapshots" => Some(("db.compare.open", json!({}))),
        "db.migration.list" | "db.migration.status" | "db.migration.run" => Some(("db.migration.open", json!({}))),
        "navigate.class.goTo" => action("view.gotoClass.open"),
        "db.object.goTo" => action("view.gotoDatabaseObject.open"),
        "navigate.symbol.goTo" => action("view.gotoSymbol.open"),
        "file.document.open" | "navigate.file.goTo" => action("view.gotoFile.open"),
        "file.project.open" => action("view.project.open"),
        "project.trust.grant" | "project.trust.state" | "project.trust.revoke" => action("view.project.trust"),
        "project.trustedLocations.get" | "project.trustedLocations.set" => settings("trustedLocations"),
        "navigate.recent.files" | "navigate.recent.restore" | "navigate.recent.remove" | "editor.split.openRight" => {
            action("view.recentFiles.open")
        }
        "navigate.recent.locations" | "navigate.recent.removeLocation" => action("view.recentLocations.open"),
        "editor.caret.goTo" => action("view.gotoLine.open"),
        "editor.clipboardHistory.list" | "editor.clipboardHistory.paste" => action("view.pasteFromHistory.open"),
        "editor.find.open" | "editor.find.set" | "search.findHistory.list" | "search.findHistory.add" => {
            action("view.find.open")
        }
        "search.text.start" | "search.text.find" | "search.text.results" | "search.text.preview" => {
            action("view.findInFiles.open")
        }
        "search.text.replaceAll" => action("view.replaceInFiles.open"),
        "search.everywhere.query"
        | "search.everywhere.contributors"
        | "search.everywhere.options"
        | "search.everywhere.setOptions"
        | "search.everywhere.filters"
        | "search.everywhere.openInFind"
        | "search.history.list" => action("view.searchEverywhere.open"),
        _ if id.starts_with("search.scope.") => action("view.findInFiles.open"),
        "refactor.rename.prepare" | "refactor.rename.preview" | "refactor.rename.apply" => action("view.rename.open"),
        "refactor.extract.variable" => action("view.extract.variable"),
        "refactor.extract.constant" => action("view.extract.constant"),
        "refactor.extract.field" => action("view.extract.field"),
        "refactor.extract.parameter" => action("view.extract.parameter"),
        "refactor.extract.method" => action("view.extract.method"),
        "refactor.inline.apply" => action("view.inline.open"),
        "refactor.safeDelete.find" | "refactor.safeDelete.apply" => action("view.safeDelete.open"),
        "editor.structure.get" => action("view.toolWindow.structure"),
        "editor.surround.list" | "editor.surround.apply" => action("view.surround.open"),
        "project.libraries.list" => action("view.toolWindow.project"),
        "problems.file.list" | "problems.project.list" => action("view.toolWindow.problems"),
        "navigate.usages.list" => action("view.toolWindow.find"),
        _ if id.starts_with("todo.") => action("view.toolWindow.todo"),
        "vcs.commit.create" | "vcs.commit.lastMessage" => action("view.vcs.commit"),
        "vcs.status.list" => action("view.toolWindow.commit"),
        "vcs.changes.rollback" => action("view.vcs.rollback"),
        "vcs.changelist.create" => action("view.vcs.newChangelist"),
        "vcs.changelist.move" => action("view.vcs.moveToChangelist"),
        "vcs.changelist.list" | "vcs.changelist.edit" | "vcs.changelist.delete" | "vcs.changelist.setActive" => {
            action("view.toolWindow.commit")
        }
        "vcs.shelf.create" => action("view.vcs.shelve"),
        "vcs.shelf.unshelve" => action("view.vcs.unshelve"),
        "vcs.shelf.list" | "vcs.shelf.diff" | "vcs.shelf.delete" | "vcs.shelf.rename" => action("view.vcs.showShelf"),
        "vcs.patch.create" => action("view.vcs.createPatch"),
        "vcs.patch.read" | "vcs.patch.diff" | "vcs.patch.apply" | "vcs.shelf.import" => action("view.vcs.applyPatch"),
        "vcs.project.update" => action("view.vcs.update"),
        "vcs.remote.push" | "vcs.push.outgoing" => action("view.vcs.push"),
        "vcs.branch.list" | "vcs.branch.checkout" | "vcs.branch.delete" | "vcs.branch.merge" => {
            action("view.vcs.branches")
        }
        "vcs.branch.create" => action("view.vcs.newBranch"),
        "vcs.revision.checkout" => action("view.vcs.checkoutRevision"),
        "vcs.diff.file" => action("view.vcs.showDiff"),
        "vcs.log.list"
        | "vcs.commit.details"
        | "vcs.diff.commit"
        | "vcs.log.view"
        | "vcs.log.filter"
        | "vcs.log.select"
        | "vcs.logTab.new"
        | "vcs.logTab.close"
        | "vcs.logTab.select"
        | "vcs.branch.favorite"
        | "vcs.branch.favorites"
        | "vcs.log.recentPaths"
        | "vcs.branch.update"
        | "vcs.commit.cherryPick" => action("view.toolWindow.git"),
        "vcs.graph.page"
        | "vcs.graph.find"
        | "vcs.graph.search"
        | "vcs.graph.hide"
        | "vcs.graph.solo"
        | "vcs.graph.focus"
        | "vcs.workspace.filter"
        | "vcs.stash.list"
        | "vcs.workspace.state"
        | "vcs.workspace.select"
        | "vcs.workspace.openDiff"
        | "vcs.workspace.menu"
        | "vcs.workspace.commitAction"
        | "vcs.diff.commits"
        | "vcs.file.history"
        | "vcs.workspace.fileMenu"
        | "vcs.workspace.fileAction"
        | "vcs.commit.revert"
        | "vcs.commit.copy"
        | "vcs.branch.reset"
        | "vcs.tag.create"
        | "vcs.workspace.refMenu"
        | "vcs.workspace.refAction"
        | "vcs.branch.rebase"
        | "vcs.branch.fastForward"
        | "vcs.branch.rename"
        | "vcs.branch.setUpstream"
        | "vcs.branch.deleteRemote"
        | "vcs.tag.delete"
        | "vcs.tag.push"
        | "vcs.workspace.stashMenu"
        | "vcs.workspace.stashAction"
        | "vcs.stash.create"
        | "vcs.stash.apply"
        | "vcs.stash.pop"
        | "vcs.stash.drop"
        | "vcs.stash.rename"
        | "vcs.rebase.plan"
        | "vcs.rebase.edit"
        | "vcs.rebase.state" => action("vcs.workspace.open"),
        _ if id.starts_with("run.configuration.") => action("view.runConfigurations.open"),
        "run.anything.list" | "run.anything.run" => action("view.runAnything.open"),
        "run.process.list" | "run.output.get" | "run.tests.get" => action("view.toolWindow.run"),
        "http.responses.list" | "http.response.get" | "http.environment.select" => action("view.toolWindow.services"),
        "debug.session.list"
        | "debug.session.get"
        | "debug.frames.list"
        | "debug.frame.select"
        | "debug.variables.list"
        | "debug.watch.list"
        | "debug.watch.add"
        | "debug.watch.remove"
        | "debug.expression.evaluate" => action("view.toolWindow.debug"),
        _ => None,
    }
}

const LISTED_AS_ACTIONS: &[&str] = &[
    "diagnostics.logs.collect",
    "diagnostics.folders.list",
    "diagnostics.logCategories.get",
    "diagnostics.logCategories.set",
    "editor.caret.goTo",
    "editor.macro.list",
    "editor.macro.startRecording",
    "editor.macro.stopRecording",
    "editor.find.open",
    "editor.surround.apply",
    "editor.surround.list",
    "file.document.open",
    "file.project.open",
    "file.scratch.languages",
    "file.scratch.list",
    "file.scratch.new",
    "file.scratch.newBuffer",
    "navigate.class.goTo",
    "navigate.file.goTo",
    "navigate.recent.files",
    "navigate.recent.locations",
    "navigate.symbol.goTo",
    "refactor.rename.apply",
    "run.configuration.list",
    "search.text.replaceAll",
    "search.text.start",
    "vcs.branch.create",
    "vcs.branch.list",
    "vcs.changes.rollback",
    "vcs.changelist.create",
    "vcs.changelist.move",
    "vcs.commit.create",
    "vcs.diff.file",
    "vcs.project.update",
    "vcs.remote.push",
    "vcs.revision.checkout",
    "vcs.shelf.create",
    "vcs.shelf.list",
    "vcs.shelf.unshelve",
    "ide.caches.invalidate",
    "ide.repair.file",
    "ide.repair.finish",
    "ide.repair.run",
];

const UNLISTED_ACTIONS: &[&str] =
    &["view.diff.setText", "view.folderDiff.filter", "view.folderDiff.select", "view.folderDiff.setAction"];

const COMMAND_STEPS: &[(&str, &str, &str, &str)] = &[
    ("editor.image.zoom/in", "editor.image.zoom", "in", "ide-image-zoom-in"),
    ("editor.image.zoom/out", "editor.image.zoom", "out", "ide-image-zoom-out"),
    ("editor.image.zoom/actual", "editor.image.zoom", "actual", "ide-image-actual-size"),
    ("editor.image.zoom/fit", "editor.image.zoom", "fit", "ide-image-fit"),
];

pub(super) fn command_step(action: &str) -> Option<Binding> {
    COMMAND_STEPS.iter().find(|(id, ..)| *id == action).map(|(id, command, step, _)| Binding {
        action: (*id).to_owned(),
        command: (*command).to_owned(),
        params: json!({ "action": step }),
        context: Context::Global,
        keystrokes: Vec::new(),
    })
}

pub(super) fn command_step_message(action: &str) -> Option<&'static str> {
    COMMAND_STEPS.iter().find(|(id, ..)| *id == action).map(|(.., message)| *message)
}

pub(super) fn workspace_actions(session: &Session) -> Vec<(Binding, String, String)> {
    let Some(workspaces) = session.state::<zen_workbench::workspaces::Workspaces>() else { return Vec::new() };
    workspaces
        .registered()
        .iter()
        .filter(|info| info.id != zen_workbench::vcs::workspace::WORKSPACE)
        .map(|info| {
            let binding = Binding {
                action: format!("workspace.host.toggle/{}", info.id),
                command: "workspace.host.toggle".to_owned(),
                params: json!({ "id": info.id }),
                context: Context::Global,
                keystrokes: Vec::new(),
            };
            let name = t!("ide-workspace-action", title = info.title.as_str());
            let english = zen_i18n::source_text("ide-workspace-action", &[("title", info.title.as_str().into())]);
            (binding, name, english)
        })
        .collect()
}

pub(super) fn resolved(binding: Binding, keymap: &Keymap, registry: &Registry) -> Option<Binding> {
    if keymap.has_default(&binding.action) {
        return Some(binding);
    }
    let found = command_step(&binding.action)
        .or_else(|| command_binding(registry.info(&binding.action)?))
        .or_else(|| parameterised(&binding.action, registry))?;
    Some(Binding { keystrokes: binding.keystrokes, ..found })
}

pub(super) fn parameterised(action: &str, registry: &Registry) -> Option<Binding> {
    let (command, params) = action.split_once(' ')?;
    registry.info(command)?;
    let params: Value = serde_json::from_str(params).ok().filter(Value::is_object)?;
    Some(Binding {
        action: action.to_owned(),
        command: command.to_owned(),
        params,
        context: Context::Global,
        keystrokes: Vec::new(),
    })
}

static MODULE_PAGES: Mutex<Vec<(&'static str, &'static str)>> = Mutex::new(Vec::new());

pub(super) fn declare_module_page(page: &zen_modules::SettingsPage) {
    let mut pages = MODULE_PAGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for command in page.commands {
        if !pages.contains(&(command, page.id)) {
            pages.push((command, page.id));
        }
    }
}

fn module_page_of(id: &str) -> Option<&'static str> {
    let pages = MODULE_PAGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    pages
        .iter()
        .find(|(command, _)| *command == id || (command.ends_with('.') && id.starts_with(command)))
        .map(|(_, page)| *page)
}

pub(super) fn command_binding(info: &CommandInfo) -> Option<Binding> {
    let id = info.id.as_str();
    let binding = |command: &str, params: Value| Binding {
        action: id.to_owned(),
        command: command.to_owned(),
        params,
        context: Context::Global,
        keystrokes: Vec::new(),
    };
    if let Some((action, params)) = command_ui(id) {
        return Some(binding(action, params));
    }
    let required =
        info.params_schema.get("required").and_then(Value::as_array).is_some_and(|required| !required.is_empty());
    (!required).then(|| binding(id, json!({})))
}

pub(super) fn command_title(info: &CommandInfo) -> (String, String) {
    let key = format!("command-{}", info.id.as_str().replace('.', "-"));
    let or_title = |text: String| if text == key { info.title.clone() } else { text };
    (or_title(t!(&key)), or_title(zen_i18n::source_text(&key, &[])))
}

fn command_group(id: &str) -> String {
    let area = id.split('.').next().unwrap_or_default();
    match area {
        "toolchain" | "language" | "lsp" | "project" => t!("ide-se-group-tools"),
        "ui" if id.starts_with("ui.keymap.") => t!("ide-se-group-keymap"),
        "ui" => t!("ide-se-group-appearance"),
        "problems" => t!(GROUPS[2]),
        "search" => t!(GROUPS[3]),
        _ => t!(GROUPS[group_of(id)]),
    }
}

#[derive(Clone, Debug)]
pub(super) struct FoundAction {
    pub(super) binding: Binding,
    pub(super) command: String,
    pub(super) name: String,
    pub(super) group: String,
    pub(super) shortcut: Option<String>,
    pub(super) available: bool,
    pub(super) ranges: Vec<Range<usize>>,
    pub(super) english: Option<(String, Vec<Range<usize>>)>,
}

pub(super) fn find_actions(query: &str, registry: &Registry, session: &mut Session, limit: usize) -> Vec<FoundAction> {
    let keymap = crate::keymap::current();
    let recent = session.state::<SearchHistory>().map(|history| history.data().actions).unwrap_or_default();
    let abbreviations: BTreeMap<String, Vec<String>> = registry
        .dispatch(session, "ui.keymap.abbreviations", json!({}))
        .ok()
        .and_then(|listed| serde_json::from_value(listed["abbreviations"].clone()).ok())
        .unwrap_or_default();
    let session = &*session;
    let workspaces = workspace_actions(session).into_iter().filter_map(|(binding, name, english)| {
        let (score, ranges, english) = rate(query, &binding.action, &name, &english, &abbreviations)?;
        let bonus = recent.iter().position(|other| *other == binding.action).map_or(0, |at| 3_000 - at as i64 * 100);
        let command = binding.command.clone();
        let group = t!("ide-se-group-workspaces");
        Some((
            score + bonus,
            FoundAction { binding, command, name, group, shortcut: None, available: true, ranges, english },
        ))
    });
    let workspaces: Vec<(i64, FoundAction)> = workspaces.collect();
    let bindings: Vec<Binding> =
        keymap.bindings().into_iter().filter(|binding| keymap.has_default(&binding.action)).collect();
    let steps = COMMAND_STEPS.iter().filter_map(|(action, command, _, message)| {
        let info = registry.info(command)?;
        Some((info, command_step(action)?, (t!(*message), zen_i18n::source_text(message, &[]))))
    });
    let commands: Vec<(i64, FoundAction)> = registry
        .commands()
        .filter(|info| {
            let id = info.id.as_str();
            !LISTED_AS_ACTIONS.contains(&id)
                && !COMMAND_STEPS.iter().any(|(_, command, ..)| *command == id)
                && !bindings.iter().any(|binding| binding.action == id)
        })
        .filter_map(|info| Some((info, command_binding(info)?, command_title(info))))
        .chain(steps)
        .enumerate()
        .filter_map(|(position, (info, binding, (name, english)))| {
            let (score, ranges, english) = rate(query, &binding.action, &name, &english, &abbreviations)?;
            let score = if query.trim().is_empty() { score - 50_000 - position as i64 } else { score };
            let id = info.id.as_str();
            let available = binding.command.starts_with("view.") || registry.is_available(session, id);
            let bonus =
                recent.iter().position(|other| *other == binding.action).map_or(0, |at| 3_000 - at as i64 * 100);
            let penalty = if available { 0 } else { 100_000 };
            let shortcut = keymap.shortcut(&binding.action);
            let found = FoundAction {
                command: id.to_owned(),
                group: command_group(id),
                binding,
                name,
                shortcut,
                available,
                ranges,
                english,
            };
            Some((score + bonus - penalty, found))
        })
        .collect();
    let mut found: Vec<(i64, FoundAction)> = bindings
        .into_iter()
        .filter(|binding| !UNLISTED_ACTIONS.contains(&binding.action.as_str()))
        .enumerate()
        .filter_map(|(position, binding)| {
            let name = action_title(&binding.action);
            let english = action_title_english(&binding.action);
            let (score, ranges, english) = rate(query, &binding.action, &name, &english, &abbreviations)?;
            let score = if query.trim().is_empty() {
                let group = match group_of(&binding.action) {
                    4 => GROUPS.len() as i64,
                    group => group as i64,
                };
                score - group * 1_000 - position as i64
            } else {
                score
            };
            let available = binding.command.starts_with("view.") || registry.is_available(session, &binding.command);
            let bonus =
                recent.iter().position(|other| *other == binding.action).map_or(0, |at| 3_000 - at as i64 * 100);
            let penalty = if available { 0 } else { 100_000 };
            let shortcut = binding
                .keystrokes
                .first()
                .map(ToString::to_string)
                .or_else(|| zen_core::keymap::gesture(&binding.action).map(str::to_owned));
            let group = t!(GROUPS[group_of(&binding.action)]);
            let command = binding.command.clone();
            Some((
                score + bonus - penalty,
                FoundAction { binding, command, name, group, shortcut, available, ranges, english },
            ))
        })
        .collect();
    found.extend(commands);
    found.extend(workspaces);
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().take(limit).map(|(_, action)| action).collect()
}

type EnglishMatch = Option<(String, Vec<Range<usize>>)>;

fn rate(
    query: &str,
    action: &str,
    name: &str,
    english: &str,
    abbreviations: &BTreeMap<String, Vec<String>>,
) -> Option<(i64, Vec<Range<usize>>, EnglishMatch)> {
    let typed = query.trim().to_lowercase();
    let abbreviated = abbreviations
        .get(action)
        .into_iter()
        .flatten()
        .filter(|_| !typed.is_empty())
        .filter_map(|abbreviation| {
            let abbreviation = abbreviation.to_lowercase();
            if abbreviation == typed {
                Some(300_000)
            } else {
                abbreviation.starts_with(&typed).then(|| 250_000 - (abbreviation.len() - typed.len()) as i64)
            }
        })
        .max();
    let named = best_match(query, name, english).or_else(|| synonym_match(query, action));
    match (abbreviated, named) {
        (Some(score), Some((other, ranges, english))) => Some((score.max(other), ranges, english)),
        (Some(score), None) => Some((score, Vec::new(), None)),
        (None, named) => named,
    }
}

pub(super) fn synonyms(action: &str) -> Vec<String> {
    let base = action.trim_end_matches("+extend");
    let key = format!("synonyms-{}", base.replace(['.', '/'], "-"));
    let mut found = Vec::new();
    for text in [t!(&key), zen_i18n::source_text(&key, &[])] {
        if text == key {
            continue;
        }
        for synonym in text.split(',').map(str::trim).filter(|synonym| !synonym.is_empty()) {
            if !found.iter().any(|known| known == synonym) {
                found.push(synonym.to_owned());
            }
        }
    }
    found
}

fn synonym_match(query: &str, action: &str) -> Option<(i64, Vec<Range<usize>>, EnglishMatch)> {
    if query.trim().is_empty() {
        return None;
    }
    synonyms(action)
        .into_iter()
        .filter_map(|synonym| {
            let (score, ranges) = action_score(query, &synonym)?;
            Some((score - 2_000, Vec::new(), Some((synonym, ranges))))
        })
        .max_by_key(|(score, ..)| *score)
}

fn best_match(query: &str, name: &str, english: &str) -> Option<(i64, Vec<Range<usize>>, EnglishMatch)> {
    match (action_score(query, name), action_score(query, english)) {
        (Some(shown), Some(other)) => Some((shown.0.max(other.0), shown.1, None)),
        (Some(shown), None) => Some((shown.0, shown.1, None)),
        (None, Some(other)) => Some((other.0, Vec::new(), (english != name).then(|| (english.to_owned(), other.1)))),
        (None, None) => None,
    }
}

fn action_score(query: &str, name: &str) -> Option<(i64, Vec<Range<usize>>)> {
    let query = query.trim_start();
    if query.trim().is_empty() {
        return Some((0, Vec::new()));
    }
    let length = name.chars().count() as i64;
    let score = |found: &zen_index::NameMatch| found.kind as i64 * 10_000 + found.degree.clamp(-99, 99) * 50 - length;
    if let Some(found) = zen_index::NamePattern::new(query).matches(name) {
        return Some((score(&found), found.ranges));
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let mut ranges = Vec::new();
    let mut degree = 0;
    for word in words {
        let found = zen_index::NamePattern::new(word).matches(name)?;
        degree += found.degree;
        ranges.extend(found.ranges);
    }
    ranges.sort_by_key(|range| range.start);
    let found = zen_index::NameMatch { kind: zen_index::MatchKind::Middle, degree, ranges };
    Some((score(&found), found.ranges))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Modifier {
    #[default]
    Shift,
    Control,
}

impl Modifier {
    fn held(self, modifiers: egui::Modifiers) -> bool {
        match self {
            Modifier::Shift => modifiers.shift,
            Modifier::Control => modifiers.ctrl,
        }
    }

    fn alone(self, modifiers: egui::Modifiers) -> bool {
        match self {
            Modifier::Shift => modifiers.shift && !modifiers.alt && !modifiers.ctrl && !modifiers.command,
            Modifier::Control => modifiers.ctrl && !modifiers.alt && !modifiers.shift && !modifiers.mac_cmd,
        }
    }

    fn is_key(self, key: Key) -> bool {
        match self {
            Modifier::Shift => matches!(key, Key::ShiftLeft | Key::ShiftRight),
            Modifier::Control => matches!(key, Key::ControlLeft | Key::ControlRight),
        }
    }
}

#[derive(Debug)]
pub(super) struct DoubleShift {
    modifier: Modifier,
    down: Option<bool>,
    released: f64,
}

impl Default for DoubleShift {
    fn default() -> Self {
        Self::of(Modifier::Shift)
    }
}

impl DoubleShift {
    const INTERVAL: f64 = 0.4;

    pub(super) fn of(modifier: Modifier) -> Self {
        Self { modifier, down: None, released: f64::NEG_INFINITY }
    }

    pub(super) fn modifier(&self) -> Modifier {
        self.modifier
    }

    pub(super) fn update(&mut self, now: f64, modifiers: egui::Modifiers, typed: bool) -> bool {
        let alone = self.modifier.alone(modifiers);
        match self.down {
            None if alone => {
                self.down = Some(!typed);
                false
            }
            Some(clean) if self.modifier.held(modifiers) => {
                self.down = Some(clean && alone && !typed);
                false
            }
            Some(clean) => {
                self.down = None;
                let fired = clean && !typed && now - self.released < Self::INTERVAL;
                self.released = if clean && !typed && !fired { now } else { f64::NEG_INFINITY };
                fired
            }
            None => {
                if typed {
                    self.released = f64::NEG_INFINITY;
                }
                false
            }
        }
    }
}

pub(super) fn typed(events: &[egui::Event]) -> bool {
    typed_besides(events, Modifier::Shift)
}

pub(super) fn typed_besides(events: &[egui::Event], modifier: Modifier) -> bool {
    events.iter().any(|event| match event {
        egui::Event::Key { key, .. } => !modifier.is_key(*key),
        egui::Event::Text(_) | egui::Event::PointerButton { .. } => true,
        _ => false,
    })
}

pub(super) enum Pick {
    File(String, Option<(u64, Option<u64>)>),
    Place(String, usize),
    Directory(String),
    Action(Binding),
    RightSplit(Box<Pick>),
}

enum Row {
    Header(String),
    Item(Box<SearchItem>, String),
    More(String),
}

pub(super) fn ranges_of(found: &Value) -> Vec<Range<usize>> {
    found
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pair| Some(pair[0].as_u64()? as usize..pair[1].as_u64()? as usize))
        .collect()
}

fn spans(pairs: &[[usize; 2]]) -> Vec<Range<usize>> {
    pairs.iter().map(|&[start, end]| start..end).collect()
}

const PREVIEW_LINE: f32 = 18.0;

const LARGEST_HIGHLIGHTED: usize = 1 << 20;

struct PreviewFile {
    text: String,
    starts: Vec<usize>,
    spans: Vec<zen_syntax::Span>,
    line: usize,
    scroll: bool,
}

impl PreviewFile {
    fn read(
        session: &Session,
        path: &Path,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn zen_syntax::Highlighter>>,
    ) -> Option<Self> {
        let text = super::usages_popup::preview_text(session, path)?;
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let spans = if text.len() <= LARGEST_HIGHLIGHTED {
            highlighter(&name).map(|highlighter| highlighter.highlight(&text)).unwrap_or_default()
        } else {
            Vec::new()
        };
        let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
        Some(Self { text, starts, spans, line: 0, scroll: true })
    }

    fn line_text(&self, line: usize) -> &str {
        let start = self.starts[line];
        let end = self.starts.get(line + 1).map_or(self.text.len(), |next| next - 1);
        self.text[start..end].trim_end_matches('\r')
    }
}

fn preview_place(item: &SearchItem) -> Option<(&Path, usize)> {
    match &item.target {
        SearchTarget::File { path, line, .. } => {
            Some((path.as_path(), line.map_or(0, |line| line.saturating_sub(1) as usize)))
        }
        SearchTarget::Place { path, line, .. } => Some((path.as_path(), line.saturating_sub(1) as usize)),
        _ => None,
    }
}

fn footer_path(item: &SearchItem, project: Option<&Path>) -> Option<String> {
    let path = match &item.target {
        SearchTarget::File { path, .. } | SearchTarget::Place { path, .. } | SearchTarget::Directory { path } => path,
        _ => return None,
    };
    let shown = project.and_then(|project| path.strip_prefix(project).ok()).unwrap_or(path);
    Some(shown.to_string_lossy().into_owned())
}

fn pick_of(target: SearchTarget) -> Option<Pick> {
    let action = |action: String, command: String, params: Value| {
        Pick::Action(Binding { action, command, params, context: Context::Global, keystrokes: Vec::new() })
    };
    Some(match target {
        SearchTarget::File { path, line, column } => {
            Pick::File(path.to_string_lossy().into_owned(), line.map(|line| (u64::from(line), column.map(u64::from))))
        }
        SearchTarget::Place { path, offset, .. } => Pick::Place(path.to_string_lossy().into_owned(), offset),
        SearchTarget::Directory { path } => Pick::Directory(path.to_string_lossy().into_owned()),
        SearchTarget::Action { action: name, command, opens, params } => action(name, opens.unwrap_or(command), params),
        SearchTarget::Settings { page } => {
            action("view.settings.open".to_owned(), "view.settings.open".to_owned(), json!({ "page": page }))
        }
        SearchTarget::Run { command, params, .. } => action(command.clone(), command, params),
        SearchTarget::Switch { .. } | SearchTarget::Value { .. } => return None,
    })
}

fn filter_option(tab: &str) -> Option<&'static str> {
    match tab {
        engine::ALL => Some("excludedContributors"),
        "files" => Some("excludedFileTypes"),
        "classes" | "symbols" => Some("excludedLanguages"),
        _ => None,
    }
}

#[derive(Default)]
pub(super) struct EverywherePopup {
    open: bool,
    tab: Tab,
    query: String,
    typed: bool,
    selected: usize,
    more: (Tab, String, bool, BTreeMap<String, usize>),
    growing: bool,
    focus: bool,
    include_non_project: bool,
    run: SearchRun,
    history_at: Option<usize>,
    commit: Option<Instant>,
    commit_selected: bool,
    preview: Option<(PathBuf, Option<PreviewFile>)>,
    resizing: Option<f32>,
    scopes: Option<ScopeChoices>,
    sync_scope: bool,
    assigning: Option<(String, String, Option<Keystroke>)>,
    text_mask: Option<String>,
    masks: Vec<String>,
    list: Option<Rect>,
    chrome: f32,
}

impl EverywherePopup {
    #[cfg(test)]
    pub(super) fn typed(&self) -> &str {
        &self.query
    }

    pub(super) fn open_tab(&self) -> Option<Tab> {
        self.open.then(|| self.tab.clone())
    }

    pub(super) fn takes_key(&self, key: Key, modifiers: egui::Modifiers) -> bool {
        self.open && key == Key::Enter && modifiers.alt && !modifiers.shift && !modifiers.command && !modifiers.ctrl
    }

    pub(super) fn is_recording(&self) -> bool {
        self.open && self.assigning.is_some()
    }

    pub(super) fn toggle(&mut self, tab: Tab) {
        if self.open && self.tab == tab {
            self.close();
            return;
        }
        if !self.open {
            self.run = SearchRun::default();
        }
        self.commit = None;
        self.open = true;
        self.tab = tab;
        self.focus = true;
        self.selected = 0;
        self.history_at = None;
    }

    fn close(&mut self) {
        self.open = false;
        self.run = SearchRun::default();
        self.text_mask = None;
        self.assigning = None;
        self.query.clear();
        self.commit = None;
        self.scopes = None;
        self.preview = None;
        self.resizing = None;
    }

    pub(super) fn double_shift(&mut self) {
        if self.open && self.tab != Tab::ACTIONS {
            self.switch_non_project();
        } else {
            self.toggle(Tab::ALL);
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn zen_syntax::Highlighter>>,
    ) -> Option<Pick> {
        if !self.open {
            return None;
        }
        let tabs = tabs(session);
        if let Some((_, _, pressed)) = self.assigning.as_mut()
            && take_keystroke(ui, pressed)
        {
            self.assigning = None;
            self.focus = true;
        }
        let menu_open = egui::Popup::is_any_open(ui.ctx());
        let (next, previous, older, newer, right_split) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, Key::Tab),
                input.consume_key(egui::Modifiers::SHIFT, Key::Tab),
                input.consume_key(egui::Modifiers::ALT, Key::ArrowDown),
                input.consume_key(egui::Modifiers::ALT, Key::ArrowUp),
                input.consume_key(egui::Modifiers::SHIFT, Key::Enter),
            )
        });
        let assign = ui.input_mut(|input| input.consume_key(egui::Modifiers::ALT, Key::Enter));
        let (to_end, to_start) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::COMMAND, Key::ArrowDown),
                input.consume_key(egui::Modifiers::COMMAND, Key::ArrowUp),
            )
        });
        if next || previous {
            let at = tabs.iter().position(|(tab, _)| *tab == self.tab).unwrap_or(0);
            let at = if next { (at + 1) % tabs.len() } else { (at + tabs.len() - 1) % tabs.len() };
            self.tab = tabs[at].0.clone();
            self.selected = 0;
            self.commit = None;
        }
        if older || newer {
            self.step_history(session, older);
        }
        if self.scopes.is_none() {
            let choices = ScopeChoices::load(registry, session, true);
            if !self.sync_scope {
                self.include_non_project = includes_non_project(&choices.selection.everywhere);
            }
            self.scopes = Some(choices);
        }
        if std::mem::take(&mut self.sync_scope)
            && let Some(choices) = self.scopes.as_mut()
            && includes_non_project(&choices.selection.everywhere) != self.include_non_project
        {
            choices.selection.everywhere = non_project_scope(self.include_non_project).to_owned();
            choices.select(registry, session);
        }
        let options = session.state::<SearchHistory>().map(SearchHistory::options).unwrap_or_default();
        let project =
            session.state::<zen_workbench::Workbench>().and_then(|workbench| workbench.project().map(Path::to_owned));
        let answer = self.answer(session);
        if answer.pending || answer.held {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !answer.held {
            self.growing = false;
        }
        let answered = (self.tab.clone(), self.query.clone(), self.include_non_project);
        let waiting = answer.waiting.clone();
        let (held, pending) = (answer.held, answer.pending);
        let rows = rows_of(answer);
        let selectable: Vec<usize> =
            rows.iter().enumerate().filter(|(_, row)| !matches!(row, Row::Header(_))).map(|(i, _)| i).collect();
        self.selected = self.selected.min(selectable.len().saturating_sub(1));
        let settled = !held && !(pending && selectable.is_empty());
        let item_at = |index: Option<usize>| match index.and_then(|index| rows.get(index)) {
            Some(Row::Item(item, _)) => Some(item.as_ref()),
            _ => None,
        };
        if assign
            && let Some(item) = item_at(selectable.get(self.selected).copied())
            && let SearchTarget::Action { action, .. } = &item.target
        {
            self.assigning = Some((action.clone(), item.name.clone(), None));
        }

        let mut pick: Option<usize> = None;
        let mut split_pick: Option<usize> = None;
        let mut find = false;
        let mut options_change: Option<Value> = None;
        let now = Instant::now();
        if self.commit.is_some_and(|at| now.saturating_duration_since(at) >= ENTER_WAIT) {
            self.commit = None;
        }
        let mut deferred = false;
        if self.commit.is_some() && settled {
            self.commit = None;
            deferred = true;
            pick = if self.commit_selected { selectable.get(self.selected) } else { selectable.first() }.copied();
        }
        let mut close = false;
        let screen = ui.ctx().content_rect();
        let top = screen.top() + screen.height() * 0.14;
        let chrome = if self.chrome > 0.0 { self.chrome } else { CHROME };
        let default = vec2(self.width(ui, &tabs).round().min(screen.width() - 32.0), ROW * VISIBLE as f32);
        let margin = super::popup_size::MARGIN;
        let max = vec2(screen.width() - 2.0 * margin, screen.bottom() - margin - top - chrome);
        let size = POPUP.size(ui.ctx(), default, max);
        let place = move |size: Vec2| {
            Rect::from_min_size(pos2(screen.center().x - size.x / 2.0, top), vec2(size.x, chrome + size.y))
        };
        self.list = None;
        if self.text_mask.is_none() || !ui.memory(|memory| memory.has_focus(egui::Id::unique(MASK_FIELD))) {
            self.text_mask = Some(options.text_mask.clone());
        }
        let area = egui::Area::new(ui.scope_id().with("search-everywhere"))
            .order(egui::Order::Foreground)
            .fixed_pos(place(size).min);
        let response = area.show(ui.ctx(), |ui| {
            let frame = dialog_frame(palette).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(size.x - 16.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                let mut x = bar.left() + 4.0;
                for (tab, label) in tabs {
                    let selected = tab == self.tab;
                    let galley = ui.painter().layout_no_wrap(
                        t!(&label),
                        theme::semibold(13.0),
                        if selected { palette.text } else { palette.text_secondary },
                    );
                    let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(galley.size().x + 16.0, bar.height()));
                    let response = ui.interact(rect, ui.scope_id().with(("se-tab", label)), Sense::click());
                    let shown = Arc::clone(&galley);
                    accessibility::item(&response, egui::Role::Tab, selected, shown.text());
                    if response.hovered() && !selected {
                        ui.painter().rect_filled(rect, 5, palette.popup_hover);
                    }
                    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, palette.text);
                    if selected {
                        let line = Rect::from_min_max(
                            pos2(rect.left() + 4.0, rect.bottom() - 2.0),
                            rect.right_bottom() - vec2(4.0, 0.0),
                        );
                        ui.painter().rect_filled(line, 1, palette.accent);
                    }
                    if response.clicked() {
                        self.tab = tab;
                        self.selected = 0;
                        self.focus = true;
                        self.commit = None;
                    }
                    x = rect.right() + 4.0;
                }
                let tab = self.tab.id().to_owned();
                let actions = self.tab == Tab::ACTIONS;
                let no_files = actions || self.tab == Tab::DATABASE;
                let mut right = bar.right() - 2.0;
                let mut button = |ui: &mut Ui, id: &str| {
                    let rect = Rect::from_center_size(pos2(right - 12.0, bar.center().y), Vec2::splat(24.0));
                    right = rect.left() - 2.0;
                    (rect, ui.scope_id().with(("se-tool", id.to_owned())))
                };
                let (rect, id) = button(ui, "find");
                let find_name = t!("ide-se-open-in-find");
                if tool_button(ui, rect, id, false, !no_files, palette, &find_name, glyph_open_in_find).clicked() {
                    find = true;
                }
                if let Some(option) = filter_option(&tab) {
                    let (rect, id) = button(ui, "filter");
                    let on = match option {
                        "excludedContributors" => !options.excluded_contributors.is_empty(),
                        "excludedFileTypes" => !options.excluded_file_types.is_empty(),
                        _ => !options.excluded_languages.is_empty(),
                    };
                    let name = t!("ide-se-filter");
                    let response = tool_button(ui, rect, id, on, true, palette, &name, glyph_funnel);
                    if let Some(change) = filter_menu(&response, session, &tab, option, palette) {
                        options_change = Some(change);
                    }
                }
                if !no_files {
                    let (rect, id) = button(ui, "preview");
                    let name = t!("ide-se-preview");
                    if tool_button(ui, rect, id, options.preview, true, palette, &name, glyph_preview).clicked() {
                        options_change = Some(json!({ "preview": !options.preview }));
                    }
                }
                right -= 6.0;
                let room = (x + 8.0)..=right;
                if self.tab == Tab::ALL {
                    if check_box(
                        ui,
                        &room,
                        bar,
                        "se-non-project",
                        &t!("ide-se-include-non-project"),
                        self.include_non_project,
                        palette,
                    ) {
                        self.switch_non_project();
                    }
                } else if actions {
                    let on = options.include_disabled_actions;
                    if check_box(ui, &room, bar, "se-disabled", &t!("ide-se-include-disabled"), on, palette) {
                        options_change = Some(json!({ "includeDisabledActions": !on }));
                        self.selected = 0;
                        self.focus = true;
                    }
                } else if self.tab != Tab::DATABASE
                    && let Some(choices) = self.scopes.as_mut()
                {
                    let name = choices.name_of(&choices.selection.everywhere);
                    let text = ui.painter().layout_no_wrap(name, theme::regular(12.5), palette.text).size().x;
                    if self.tab == Tab::TEXT {
                        let available = (right - room.start()).max(0.0);
                        let least = MASK_MIN_WIDTH.min(available - 6.0 - SCOPE_MIN_WIDTH).max(MASK_CHEVRON);
                        let mask_width = (available - 6.0 - (text + 28.0)).clamp(least, MASK_WIDTH);
                        let rect = Rect::from_min_max(
                            pos2((right - mask_width).max(*room.start()), bar.top() + 1.0),
                            pos2(right, bar.bottom() - 1.0),
                        );
                        let mask = self.text_mask.get_or_insert_with(String::new);
                        if mask_combo(ui, rect, mask, &mut self.masks, registry, session, palette) {
                            options_change = Some(json!({ "textMask": mask.clone() }));
                            self.selected = 0;
                            self.commit = None;
                        }
                        right = rect.left() - 6.0;
                    }
                    let mut controls = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(Rect::from_min_max(pos2(*room.start(), bar.top()), pos2(right, bar.bottom())))
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    let snapshot = choices.clone();
                    let room_left = (right - room.start()).max(0.0);
                    let width = (text + 28.0).min(room_left).max(SCOPE_MIN_WIDTH.min(room_left));
                    if scope_combo(&mut controls, "se-scope", &snapshot, &mut choices.selection.everywhere, width) {
                        choices.select(registry, session);
                        self.include_non_project = includes_non_project(&choices.selection.everywhere);
                        self.selected = 0;
                        self.focus = true;
                        self.commit = None;
                    }
                }
                ui.add_space(6.0);
                let hint = t!("ide-se-hint");
                let text_tab = self.tab == Tab::TEXT;
                let reserve = if text_tab { TOGGLES_WIDTH } else { 0.0 };
                let was_empty = self.query.is_empty();
                let (field, chosen) = history_field(ui, &mut self.query, &hint, session, palette, reserve);
                if field.changed() {
                    let pasted =
                        ui.input(|input| input.events.iter().any(|event| matches!(event, egui::Event::Paste(_))));
                    self.typed = if pasted { false } else { self.query.is_empty() || was_empty || self.typed };
                }
                if text_tab {
                    let switches = [
                        ("Cc", "ide-find-match-case", "textMatchCase", options.text_match_case),
                        ("W", "ide-find-words", "textWholeWords", options.text_whole_words),
                        (".*", "ide-find-regex", "textRegex", options.text_regex),
                    ];
                    let mut at = field.rect.right() + 4.0;
                    for (label, name, option, on) in switches {
                        let rect = Rect::from_min_size(pos2(at, field.rect.center().y - 11.0), vec2(24.0, 22.0));
                        at = rect.right() + 4.0;
                        if text_switch(ui, rect, label, &t!(name), on, palette) {
                            options_change = Some(json!({ option: !on }));
                            self.selected = 0;
                            self.commit = None;
                            self.focus = true;
                        }
                    }
                }
                if let Some(query) = chosen {
                    self.query = query;
                    self.typed = false;
                    self.selected = 0;
                    self.focus = true;
                    self.commit = None;
                }
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                if field.changed() {
                    self.selected = 0;
                    self.growing = false;
                    self.history_at = None;
                    self.commit = None;
                }
                super::filter_popups::take_typing(ui);
                let (up, down, enter, escape) = ui.input_mut(|input| {
                    (
                        input.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                        input.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                        input.consume_key(egui::Modifiers::NONE, Key::Enter),
                        input.key_pressed(Key::Escape),
                    )
                });
                if up {
                    self.selected = self.selected.saturating_sub(1);
                }
                if down && self.selected + 1 < selectable.len() {
                    self.selected += 1;
                }
                if to_end {
                    self.selected = selectable.len().saturating_sub(1);
                }
                if to_start {
                    self.selected = 0;
                }
                let current = answered == (self.tab.clone(), self.query.clone(), self.include_non_project);
                if enter && current && settled {
                    pick = selectable.get(self.selected).copied();
                    deferred = false;
                } else if enter {
                    self.commit = Some(now);
                    self.commit_selected = current && self.growing;
                }
                if right_split && current && settled {
                    split_pick = selectable.get(self.selected).copied();
                }
                if escape && !menu_open {
                    close = true;
                }
                let typing_mask = ui.memory(|memory| memory.has_focus(egui::Id::unique(MASK_FIELD)));
                if !field.has_focus() && !typing_mask && !close && pick.is_none() && !egui::Popup::is_any_open(ui.ctx())
                {
                    field.request_focus();
                }

                if selectable.is_empty() {
                    if self.query.trim().is_empty() && waiting.is_none() {
                        return;
                    }
                    ui.add_space(6.0);
                    let message = t!(waiting.as_deref().unwrap_or("ide-goto-file-nothing"));
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW * 2.0), Sense::hover());
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        message,
                        theme::regular(13.0),
                        palette.text_secondary,
                    );
                    return;
                }
                ui.add_space(6.0);
                let current = selectable.get(self.selected).copied();
                let list_top = ui.cursor().top();
                super::filter_popups::list(ui, "se-rows", size.y.max(ROW * 3.0), |ui| {
                    for (index, row) in rows.iter().enumerate() {
                        if let Row::Header(title) = row {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(ui.available_width(), ROW - 4.0), Sense::hover());
                            ui.painter().text(
                                pos2(rect.left() + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                title,
                                theme::regular(12.0),
                                palette.text_secondary,
                            );
                            continue;
                        }
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        if Some(index) == current {
                            ui.painter().rect_filled(rect, 5, palette.selection);
                            if up || down || to_end || to_start {
                                response.scroll_to_me(None);
                            }
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 5, palette.popup_hover);
                        }
                        accessibility::item(
                            &response,
                            egui::Role::ListBoxOption,
                            Some(index) == current,
                            &match row {
                                Row::Item(item, _) => item.name.clone(),
                                Row::Header(title) => title.clone(),
                                Row::More(_) => t!("ide-se-more"),
                            },
                        );
                        if let Row::Item(item, _) = row
                            && let SearchTarget::Switch { on, .. } = item.target
                        {
                            accessibility::describe(
                                &response,
                                &t!(if on { "ide-se-switch-on" } else { "ide-se-switch-off" }),
                            );
                        }
                        match row {
                            Row::Item(item, _) => item_row(ui, rect, item, palette),
                            Row::More(_) => more_row(ui, rect, palette),
                            Row::Header(_) => {}
                        }
                        if response.clicked() {
                            pick = Some(index);
                        }
                    }
                });
                self.list = Some(Rect::from_x_y_ranges(ui.max_rect().x_range(), list_top..=ui.min_rect().bottom()));
                let selected = item_at(current);
                if options.preview && !no_files {
                    let height =
                        self.resizing.unwrap_or(options.preview_height).clamp(80.0, (screen.height() * 0.5).max(80.0));
                    let (handle, drag) = ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::drag());
                    ui.painter().hline(handle.x_range(), handle.center().y, (1.0, palette.tab_border));
                    if drag.hovered() || drag.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    if drag.dragged() {
                        self.resizing = Some((height - drag.drag_delta().y).clamp(80.0, 900.0));
                    }
                    if drag.drag_stopped()
                        && let Some(height) = self.resizing.take()
                    {
                        options_change = Some(json!({ "previewHeight": height }));
                    }
                    accessibility::labelled(&drag, &t!("ide-se-preview"));
                    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
                    self.draw_preview(ui, area, session, selected, palette, highlighter);
                }
                let footer = selected.and_then(|item| match &item.target {
                    SearchTarget::Action { .. } => Some(t!("ide-se-assign-shortcut")),
                    _ => footer_path(item, project.as_deref()),
                });
                if let Some(item) = selected
                    && let Some(path) = footer
                {
                    let (footer, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                    ui.painter().hline(footer.x_range(), footer.top(), (1.0, palette.tab_border));
                    let mut text_right = footer.right() - 4.0;
                    if preview_place(item).is_some() {
                        let label = t!("ide-se-open-right");
                        let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(12.0), palette.link);
                        let rect = Align2::RIGHT_CENTER
                            .anchor_size(pos2(footer.right() - 6.0, footer.center().y), galley.size());
                        let link = ui.interact(rect.expand(2.0), ui.scope_id().with("se-open-right"), Sense::click());
                        if link.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        accessibility::button(&link, &label);
                        ui.painter().galley(rect.min, galley, palette.link);
                        if link.clicked() {
                            split_pick = current;
                        }
                        text_right = rect.left() - 12.0;
                    }
                    ui.painter()
                        .with_clip_rect(Rect::from_min_max(footer.min, pos2(text_right, footer.bottom())))
                        .text(
                            pos2(footer.left() + 8.0, footer.center().y),
                            Align2::LEFT_CENTER,
                            path,
                            theme::regular(12.0),
                            palette.text_secondary,
                        );
                }
            });
            if let Some(list) = self.list {
                self.chrome = frame.response.rect.height() - list.height();
            }
            POPUP.grips(ui, frame.response.rect, size, max, place);
        });
        if ui.input(|input| input.pointer.any_pressed())
            && self.assigning.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !response.response.rect.contains(pointer)
        {
            close = true;
        }
        if let Some(change) = options_change {
            let _ = registry.dispatch(session, "search.everywhere.setOptions", change);
            self.selected = 0;
            ui.ctx().request_repaint();
        }
        if find && self.tab != Tab::ACTIONS && self.tab != Tab::DATABASE {
            let groups = self.answer(session).groups;
            engine::show_in_find(session, self.tab.id(), &self.query, groups);
            close = true;
        }
        let split = split_pick.and_then(|index| match rows.get(index) {
            Some(Row::Item(item, _)) if preview_place(item).is_some() => Some(index),
            _ => None,
        });
        let chosen = split.or(pick);
        let mut copied = false;
        let mut text_picked = false;
        let picked = chosen.and_then(|index| rows.into_iter().nth(index)).and_then(|row| match row {
            Row::Item(item, contributor) => match item.target {
                SearchTarget::Switch { .. } if deferred => None,
                SearchTarget::Switch { command, params, .. } => {
                    let _ = registry.dispatch(session, &command, params);
                    self.run = SearchRun::default();
                    self.focus = true;
                    ui.ctx().request_repaint();
                    None
                }
                SearchTarget::Value { value } => {
                    ui.ctx().copy_text(value);
                    copied = true;
                    None
                }
                target => {
                    text_picked = contributor == Tab::TEXT.id();
                    Some(target)
                }
            },
            Row::More(contributor) => {
                *self.more.3.entry(contributor).or_default() += 1;
                self.growing = true;
                self.focus = true;
                ui.ctx().request_repaint();
                None
            }
            Row::Header(_) => None,
        });
        let picked = picked.and_then(|target| {
            let history = session.state_mut::<SearchHistory>().clone();
            history.remember_query(&self.query);
            if let SearchTarget::Action { action, .. } = &target {
                history.remember_action(action);
            }
            let pick = pick_of(target)?;
            Some(if split.is_some() { Pick::RightSplit(Box::new(pick)) } else { pick })
        });
        if copied {
            session.state_mut::<SearchHistory>().clone().remember_query(&self.query);
        }
        if text_picked || find && self.tab == Tab::TEXT {
            remember_mask(registry, session, &options.text_mask);
        }
        self.assign_shortcut(ui, registry, session, palette);
        if close || copied || picked.is_some() {
            self.close();
        }
        picked
    }

    fn assign_shortcut(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, palette: &IdePalette) {
        let Some((action, title, pressed)) = self.assigning.clone() else { return };
        let keymap = crate::keymap::current();
        match shortcut_dialog(ui, palette, &title, &action, pressed, &keymap) {
            Recorded::Open(pressed) => self.assigning = Some((action, title, pressed)),
            Recorded::Cancelled => {
                self.assigning = None;
                self.focus = true;
            }
            Recorded::Done { key, take_from } => {
                let set = |session: &mut Session, action: &str, keys: Vec<Keystroke>| {
                    let keystrokes: Vec<String> = keys.iter().map(ToString::to_string).collect();
                    let params = json!({ "action": action, "keystrokes": keystrokes });
                    registry.dispatch(session, "ui.keymap.setShortcut", params)
                };
                for other in take_from {
                    let rest = keymap.keystrokes(&other).into_iter().filter(|stroke| *stroke != key).collect();
                    let _ = set(session, &other, rest);
                }
                let mut keys = keymap.keystrokes(&action);
                if !keys.contains(&key) {
                    keys.push(key);
                }
                let _ = set(session, &action, keys);
                self.assigning = None;
                self.focus = true;
                self.run = SearchRun::default();
                ui.ctx().request_repaint();
            }
        }
    }

    fn draw_preview(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        session: &Session,
        selected: Option<&SearchItem>,
        palette: &IdePalette,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn zen_syntax::Highlighter>>,
    ) {
        ui.painter().rect_filled(area, 5, palette.editor_background);
        let Some((path, line)) = selected.and_then(preview_place) else {
            self.preview = None;
            return;
        };
        if self.preview.as_ref().is_none_or(|(read, _)| read != path) {
            self.preview = Some((path.to_owned(), PreviewFile::read(session, path, highlighter)));
        }
        let Some((_, Some(preview))) = self.preview.as_mut() else { return };
        let line = line.min(preview.starts.len().saturating_sub(1));
        if preview.line != line {
            preview.line = line;
            preview.scroll = true;
        }
        let number_font = egui::FontId::monospace(12.0);
        let digits = preview.starts.len().to_string().len();
        let gutter = ui.fonts_mut(|fonts| fonts.glyph_width(&number_font, '0')) * digits as f32 + 18.0;
        let mut scroll = egui::ScrollArea::both().id_salt("se-preview").auto_shrink([false, false]);
        if std::mem::take(&mut preview.scroll) {
            let offset = (line as f32 * PREVIEW_LINE - (area.height() - PREVIEW_LINE) / 2.0).max(0.0);
            scroll = scroll.vertical_scroll_offset(offset).horizontal_scroll_offset(0.0);
        }
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area.shrink(4.0)));
        scroll.show_rows(&mut inner, PREVIEW_LINE, preview.starts.len(), |ui, range| {
            for index in range {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width().max(200.0), PREVIEW_LINE), Sense::hover());
                if index == preview.line {
                    ui.painter().rect_filled(rect, 0, palette.current_line);
                }
                let color = if index == preview.line { palette.text } else { palette.gutter_text };
                ui.painter().text(
                    pos2(rect.left() + gutter - 10.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    (index + 1).to_string(),
                    number_font.clone(),
                    color,
                );
                let text = preview.line_text(index);
                let job = super::diff_view::line_job(
                    text,
                    preview.starts[index],
                    &preview.spans,
                    &[],
                    palette,
                    egui::Color32::TRANSPARENT,
                );
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                let width = galley.size().x;
                ui.painter().galley(
                    pos2(rect.left() + gutter, rect.center().y - galley.size().y / 2.0),
                    galley,
                    palette.code,
                );
                ui.expand_to_include_rect(Rect::from_min_size(rect.min, vec2(gutter + width + 8.0, PREVIEW_LINE)));
            }
        });
    }

    fn width(&self, ui: &Ui, tabs: &[(Tab, String)]) -> f32 {
        let screen = ui.ctx().content_rect();
        let base = (screen.width() * 0.55).clamp(520.0, 760.0);
        let color = ui.visuals().text_color();
        let measure = |text: String, font: egui::FontId| ui.painter().layout_no_wrap(text, font, color).size().x;
        let titles: f32 = tabs.iter().map(|(_, label)| measure(t!(label), theme::semibold(13.0)) + 20.0).sum();
        let boxes = [t!("ide-se-include-non-project"), t!("ide-se-include-disabled")]
            .into_iter()
            .map(|label| measure(label, theme::regular(12.5)) + 20.0)
            .fold(0.0, f32::max);
        let scope = self.scopes.as_ref().map_or(0.0, |choices| {
            measure(choices.name_of(&choices.selection.everywhere), theme::regular(12.5)) + 28.0
        });
        let controls = boxes.max(scope + 6.0 + MASK_WIDTH);
        base.max(4.0 + titles + 8.0 + controls + 6.0 + TOOL_BUTTONS + 2.0 + 16.0)
    }

    fn switch_non_project(&mut self) {
        self.include_non_project = !self.include_non_project;
        self.sync_scope = true;
        self.selected = 0;
        self.focus = true;
        self.commit = None;
    }

    fn scope(&self) -> String {
        self.scopes.as_ref().map_or_else(|| PROJECT_FILES.to_owned(), |choices| choices.selection.everywhere.clone())
    }

    fn answer(&mut self, session: &mut Session) -> SearchAnswer {
        let scope = (![Tab::ALL, Tab::ACTIONS, Tab::DATABASE].contains(&self.tab)).then(|| self.scope());
        if (&self.more.0, &self.more.1, self.more.2) != (&self.tab, &self.query, self.include_non_project) {
            self.more = (self.tab.clone(), self.query.clone(), self.include_non_project, BTreeMap::new());
            self.growing = false;
        }
        let request = SearchRequest {
            tab: self.tab.id().to_owned(),
            query: self.query.clone(),
            include_non_project: self.include_non_project,
            limit: None,
            scope,
            more: self.more.3.clone(),
            text: None,
            typed: self.typed,
        };
        self.run.poll(session, &request).unwrap_or_else(|_| SearchAnswer {
            tab: request.tab,
            groups: Vec::new(),
            pending: false,
            held: false,
            waiting: None,
        })
    }

    fn step_history(&mut self, session: &mut Session, older: bool) {
        let queries = session.state_mut::<SearchHistory>().data().queries;
        let at = match (self.history_at, older) {
            (None, true) => 0,
            (None, false) => return,
            (Some(at), true) => (at + 1).min(queries.len().saturating_sub(1)),
            (Some(0), false) => {
                self.history_at = None;
                self.query.clear();
                self.commit = None;
                return;
            }
            (Some(at), false) => at - 1,
        };
        if let Some(query) = queries.get(at) {
            self.history_at = Some(at);
            self.query = query.clone();
            self.typed = false;
            self.selected = 0;
            self.commit = None;
        }
    }
}

fn remember_mask(registry: &Registry, session: &mut Session, mask: &str) {
    if !mask.trim().is_empty() {
        let _ = registry.dispatch(session, "search.findHistory.add", json!({ "mask": mask }));
    }
}

#[allow(clippy::too_many_arguments, reason = "a toolbar button with its look and its name")]
fn tool_button(
    ui: &Ui,
    rect: Rect,
    id: egui::Id,
    on: bool,
    enabled: bool,
    palette: &IdePalette,
    name: &str,
    glyph: super::glyphs::Glyph,
) -> egui::Response {
    let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    if on {
        ui.painter().rect_filled(rect, 4, palette.selection);
    } else if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, palette.popup_hover);
    }
    let color = match (enabled, on) {
        (false, _) => palette.text_secondary.gamma_multiply(0.45),
        (true, true) => palette.text,
        (true, false) => palette.text_secondary,
    };
    glyph(ui.painter(), rect.shrink(5.0), color);
    if egui::Popup::is_any_open(ui.ctx()) {
        accessibility::button(&response, name);
    } else {
        super::hint(ui, &response, name, None, super::Side::Below);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, on, name));
    response
}

fn check_box(
    ui: &Ui,
    room: &std::ops::RangeInclusive<f32>,
    bar: Rect,
    id: &str,
    label: &str,
    on: bool,
    palette: &IdePalette,
) -> bool {
    let box_size = 14.0;
    let space = room.end() - room.start() - box_size - 6.0;
    let labelled = space >= 40.0;
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), theme::regular(12.5), palette.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(space.max(0.0));
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let width = if labelled { box_size + 6.0 + galley.size().x } else { box_size };
    let rect = Rect::from_min_size(pos2(room.end() - width, bar.center().y - box_size / 2.0), vec2(width, box_size));
    let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    if galley.elided {
        super::hint(ui, &response, label, None, super::Side::Below);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, label));
    let mark = Rect::from_min_size(rect.min, Vec2::splat(box_size));
    ui.painter().rect_stroke(mark, 3, (1.0, palette.text_secondary), egui::StrokeKind::Inside);
    if on {
        ui.painter().rect_filled(mark.shrink(3.0), 2, palette.accent);
    }
    if labelled {
        ui.painter().galley(pos2(mark.right() + 6.0, rect.center().y - galley.size().y / 2.0), galley, palette.text);
    }
    response.clicked()
}

fn filter_menu(
    response: &egui::Response,
    session: &mut Session,
    tab: &str,
    option: &str,
    palette: &IdePalette,
) -> Option<Value> {
    let mut change = None;
    egui::Popup::menu(response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let entries = engine::filter_entries(session, tab).unwrap_or_default();
        ui.set_min_width(220.0);
        ui.set_max_width(260.0);
        let excluded = |entries: &[engine::FilterEntry]| -> Vec<String> {
            entries.iter().filter(|entry| !entry.on).map(|entry| entry.id.clone()).collect()
        };
        if tab == engine::ALL {
            ui.horizontal(|ui| {
                for (label, set) in [
                    ("ide-se-filter-all", Some(true)),
                    ("ide-se-filter-none", Some(false)),
                    ("ide-se-filter-invert", None),
                ] {
                    if ui.link(t!(label)).clicked() {
                        let flipped: Vec<engine::FilterEntry> = entries
                            .iter()
                            .map(|entry| engine::FilterEntry { on: set.unwrap_or(!entry.on), ..entry.clone() })
                            .collect();
                        change = Some(json!({ option: excluded(&flipped) }));
                    }
                }
            });
            ui.separator();
        }
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for (index, entry) in entries.iter().enumerate() {
                let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                if row.hovered() {
                    ui.painter().rect_filled(rect, 4, palette.popup_hover);
                }
                let mark = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(14.0));
                super::commit_window::checkbox(ui.painter(), mark, Some(entry.on), palette);
                ui.painter().text(
                    pos2(rect.left() + 30.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &entry.name,
                    theme::regular(13.0),
                    palette.text,
                );
                row.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, entry.on, &entry.name));
                if row.clicked() {
                    let mut flipped = entries.clone();
                    flipped[index].on = !entry.on;
                    change = Some(json!({ option: excluded(&flipped) }));
                }
            }
        });
    });
    change
}

fn history_field(
    ui: &mut Ui,
    query: &mut String,
    hint: &str,
    session: &Session,
    palette: &IdePalette,
    reserve: f32,
) -> (egui::Response, Option<String>) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().rect_filled(row, 6, palette.frame);
    let button = Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), vec2(30.0, 26.0));
    let opener = ui.interact(button, ui.scope_id().with("se-history"), Sense::click());
    if opener.hovered() {
        ui.painter().rect_filled(button, 4, palette.popup_hover);
    }
    glyph_search(
        ui.painter(),
        Rect::from_center_size(pos2(button.left() + 11.0, row.center().y), Vec2::splat(14.0)),
        palette.text_secondary,
    );
    let arrow = pos2(button.right() - 7.0, row.center().y + 1.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![arrow + vec2(-3.0, -2.0), arrow + vec2(3.0, -2.0), arrow + vec2(0.0, 2.0)],
        palette.text_secondary,
        egui::Stroke::NONE,
    ));
    super::hint(ui, &opener, &t!("ide-se-history"), None, super::Side::Below);
    let mut chosen = None;
    egui::Popup::menu(&opener).show(|ui| {
        ui.set_min_width(260.0);
        let history = session.state::<SearchHistory>().map(|history| history.data().queries).unwrap_or_default();
        if history.is_empty() {
            ui.label(egui::RichText::new(t!("ide-se-history-empty")).color(palette.text_secondary));
        }
        for past in history.iter().take(20) {
            if ui.button(past).clicked() {
                chosen = Some(past.clone());
                ui.close();
            }
        }
    });
    let field_rect =
        Rect::from_min_max(pos2(button.right() + 4.0, row.top()), pos2(row.right() - 8.0 - reserve, row.bottom()));
    let field = ui.put(
        field_rect,
        egui::TextEdit::singleline(query)
            .frame(egui::Frame::NONE)
            .font(theme::regular(14.0))
            .align(Align2::LEFT_CENTER)
            .hint_text(hint)
            .desired_width(field_rect.width()),
    );
    accessibility::labelled(&field, hint);
    (field, chosen)
}

fn text_switch(ui: &Ui, rect: Rect, label: &str, name: &str, on: bool, palette: &IdePalette) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(("se-text-switch", label)), Sense::click());
    if on {
        ui.painter().rect_filled(rect, 4, palette.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, palette.popup_hover);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::semibold(11.5),
        if on { palette.text } else { palette.text_secondary },
    );
    super::hint(ui, &response, name, None, super::Side::Below);
    accessibility::button(&response, name);
    accessibility::pressed(&response, on);
    response.clicked()
}

#[allow(clippy::too_many_arguments, reason = "the combo with its history, read through the registry")]
fn mask_combo(
    ui: &mut Ui,
    rect: Rect,
    mask: &mut String,
    masks: &mut Vec<String>,
    registry: &Registry,
    session: &mut Session,
    palette: &IdePalette,
) -> bool {
    let before = mask.clone();
    ui.painter().rect_filled(rect, 4, palette.frame);
    ui.painter().rect_stroke(rect, 4, (1.0, palette.tab_border), egui::StrokeKind::Inside);
    let chevron = Rect::from_min_max(pos2(rect.right() - 18.0, rect.top()), rect.max);
    let list = ui.interact(chevron, ui.scope_id().with("se-text-mask-list"), Sense::click());
    if list.hovered() {
        ui.painter().rect_filled(chevron.shrink(1.0), 3, palette.popup_hover);
    }
    let (c, w) = (chevron.center(), 3.5);
    let stroke = egui::Stroke::new(1.2, palette.text_secondary);
    ui.painter().line_segment([pos2(c.x - w, c.y - w / 2.0), pos2(c.x, c.y + w / 2.0)], stroke);
    ui.painter().line_segment([pos2(c.x, c.y + w / 2.0), pos2(c.x + w, c.y - w / 2.0)], stroke);
    let history = t!("ide-se-text-mask-history");
    accessibility::button(&list, &history);
    if list.clicked() {
        let data = registry.dispatch(session, "search.findHistory.list", json!({})).unwrap_or_default();
        *masks = data["masks"]
            .as_array()
            .map(|listed| listed.iter().filter_map(|mask| mask.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
    }
    egui::Popup::menu(&list).show(|ui| {
        ui.set_min_width(rect.width().max(160.0));
        if masks.is_empty() {
            ui.label(egui::RichText::new(t!("ide-se-text-mask-history-empty")).color(palette.text_secondary));
        }
        for past in masks.iter() {
            if ui.button(egui::RichText::new(past).font(theme::regular(12.5))).clicked() {
                mask.clone_from(past);
                ui.close();
            }
        }
    });
    let field_rect = Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(chevron.left(), rect.bottom()));
    let name = t!("ide-se-text-mask");
    let any = t!("ide-se-text-mask-any");
    let field = ui.put(
        field_rect,
        egui::TextEdit::singleline(mask)
            .id(egui::Id::unique(MASK_FIELD))
            .frame(egui::Frame::NONE)
            .font(theme::regular(12.5))
            .align(Align2::LEFT_CENTER)
            .hint_text(any.as_str())
            .desired_width(field_rect.width()),
    );
    accessibility::labelled(&field, &name);
    if field.hovered() && !field.has_focus() {
        super::hint(ui, &field, &name, None, super::Side::Below);
    }
    *mask != before
}

pub(super) fn search_field(ui: &mut Ui, query: &mut String, hint: &str, palette: &IdePalette) -> egui::Response {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().rect_filled(row, 6, palette.frame);
    glyph_search(
        ui.painter(),
        Rect::from_center_size(pos2(row.left() + 16.0, row.center().y), Vec2::splat(14.0)),
        palette.text_secondary,
    );
    let field_rect = Rect::from_min_max(pos2(row.left() + 32.0, row.top()), pos2(row.right() - 8.0, row.bottom()));
    let field = ui.put(
        field_rect,
        egui::TextEdit::singleline(query)
            .frame(egui::Frame::NONE)
            .font(theme::regular(14.0))
            .align(Align2::LEFT_CENTER)
            .hint_text(hint)
            .desired_width(field_rect.width()),
    );
    accessibility::labelled(&field, hint);
    field
}

fn rows_of(answer: SearchAnswer) -> Vec<Row> {
    let all = answer.tab == engine::ALL;
    let mut rows = Vec::new();
    for group in answer.groups {
        if all {
            rows.push(Row::Header(t!(&group.title)));
        }
        let contributor = group.contributor.clone();
        rows.extend(group.items.into_iter().map(|item| Row::Item(Box::new(item), contributor.clone())));
        if group.more {
            rows.push(Row::More(group.contributor));
        }
    }
    rows
}

fn more_row(ui: &Ui, rect: Rect, palette: &IdePalette) {
    ui.painter().text(
        pos2(rect.left() + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        t!("ide-se-more"),
        theme::regular(12.5),
        palette.text_secondary,
    );
}

pub(super) fn item_row(ui: &Ui, rect: Rect, item: &SearchItem, palette: &IdePalette) {
    let icon = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(16.0));
    let left = match &item.target {
        SearchTarget::File { path, .. } => {
            crate::file_icons::file_icon_at(ui.painter(), icon, path);
            rect.left() + 32.0
        }
        SearchTarget::Place { symbol, .. } => {
            super::glyphs::symbol_badge(ui.painter(), icon, *symbol, palette);
            rect.left() + 32.0
        }
        SearchTarget::Directory { .. } => {
            super::glyphs::glyph_folder(ui.painter(), icon.shrink(1.0), palette.text_secondary);
            rect.left() + 32.0
        }
        SearchTarget::Run { icon: kind, .. } => {
            super::database_navigator::paint_found_icon(ui.painter(), icon, kind, palette);
            rect.left() + 32.0
        }
        _ => rect.left() + 12.0,
    };
    let action = matches!(item.target, SearchTarget::Action { .. });
    let mut end = rect.right() - 10.0;
    if let SearchTarget::Switch { on, .. } = item.target {
        let track = Rect::from_min_size(pos2(end - 28.0, rect.center().y - 8.0), vec2(28.0, 16.0));
        let color = if on { palette.accent } else { palette.text_secondary.gamma_multiply(0.6) };
        ui.painter().rect_filled(track, 8, color);
        let knob = if on { track.right() - 8.0 } else { track.left() + 8.0 };
        ui.painter().circle_filled(pos2(knob, track.center().y), 6.0, palette.accent_text);
        end = track.left() - 10.0;
    }
    let hint = item.hint.as_ref().map(|hint| {
        ui.painter().text(
            pos2(end, rect.center().y),
            Align2::RIGHT_CENTER,
            hint,
            theme::regular(if action { 13.0 } else { 12.5 }),
            palette.text_secondary,
        )
    });
    let right = hint.map_or(end, |hint| hint.left() - 10.0);
    let painter = ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(right, rect.max.y)));
    let color = if item.available { palette.text } else { palette.text_secondary };
    let label = matched_text(
        &painter,
        pos2(left, rect.center().y),
        &item.name,
        &spans(&item.ranges),
        theme::regular(13.5),
        color,
        palette,
    );
    if !item.detail.is_empty() {
        let detail = if action { palette.text_secondary.gamma_multiply(0.8) } else { palette.text_secondary };
        matched_text(
            &painter,
            pos2(label.right() + 10.0, rect.center().y),
            &item.detail,
            &spans(&item.detail_ranges),
            theme::regular(12.5),
            detail,
            palette,
        );
    }
}

pub(super) fn matched_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    text: &str,
    ranges: &[Range<usize>],
    font: egui::FontId,
    color: egui::Color32,
    palette: &IdePalette,
) -> Rect {
    let plain = egui::text::TextFormat { font_id: font, color, ..Default::default() };
    let marked = egui::text::TextFormat { background: palette.search_highlight, ..plain.clone() };
    let mut job = egui::text::LayoutJob::default();
    let mut run = String::new();
    let mut in_run = false;
    for (index, ch) in text.chars().enumerate() {
        let matched = ranges.iter().any(|range| range.contains(&index));
        if matched != in_run && !run.is_empty() {
            job.append(&std::mem::take(&mut run), 0.0, if in_run { marked.clone() } else { plain.clone() });
        }
        in_run = matched;
        run.push(ch);
    }
    if !run.is_empty() {
        job.append(&run, 0.0, if in_run { marked } else { plain });
    }
    let galley = painter.layout_job(job);
    let rect = Align2::LEFT_CENTER.anchor_size(at, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

#[cfg(test)]
mod database_tests;

#[cfg(test)]
mod tests {

    use egui::{Event, Pos2};
    use zen_lsp::fake::FakeServer;
    use zen_workbench::Workbench;
    use zen_workbench::language::{Languages, ServerSpec};

    use super::*;

    const STORE: &str =
        "package shop\n\ntype CartStore struct{}\n\nfunc (s *CartStore) AddItem() {}\n\nconst MaxItems = 3\n";

    const SDK: &str = "package sync\n\ntype CartPool struct{}\n";

    const MODULES_EN: &str = "module-go-name = Go\nmodule-go-description = Everything for Go.\n\
        module-docker-name = Docker\nmodule-docker-description = Dockerfiles and containers.\n";

    fn nothing(_: &mut zen_modules::ModuleHost<'_>) -> Result<(), zen_core::RegistryError> {
        Ok(())
    }

    static GO: zen_modules::ModuleDescriptor = zen_modules::ModuleDescriptor {
        id: "go",
        name: "module-go-name",
        description: "module-go-description",
        locales: &[("en", MODULES_EN)],
        register: nothing,
    };

    static DOCKER: zen_modules::ModuleDescriptor = zen_modules::ModuleDescriptor {
        id: "docker",
        name: "module-docker-name",
        description: "module-docker-description",
        locales: &[("en", MODULES_EN)],
        register: nothing,
    };

    fn demo_modules() -> Vec<zen_modules::BuiltinModule> {
        zen_i18n::register(&[("en", MODULES_EN)]);
        vec![zen_modules::BuiltinModule::on(&GO), zen_modules::BuiltinModule::off(&DOCKER)]
    }

    static MARKDOWN_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

    struct InstalledPlugins;

    impl zen_core::Command for InstalledPlugins {
        type Params = Value;
        type Output = Value;
        const ID: &'static str = "plugins.installed.list";
        const TITLE: &'static str = "Installed Plugins";
        const DESCRIPTION: &'static str = "One installed plug-in.";

        fn run(&self, _: &mut Session, params: Value) -> Result<Value, zen_core::CommandError> {
            let on = MARKDOWN_ON.load(std::sync::atomic::Ordering::Relaxed);
            let query = params["query"].as_str().unwrap_or_default().to_lowercase();
            if !"markdown".contains(&query) {
                return Ok(json!({ "plugins": [] }));
            }
            Ok(
                json!({ "plugins": [{ "id": "markdown", "name": "Markdown", "description": "Markdown files.", "enabled": on }] }),
            )
        }
    }

    struct DisablePlugin;

    impl zen_core::Command for DisablePlugin {
        type Params = Value;
        type Output = Value;
        const ID: &'static str = "plugins.plugin.disable";
        const TITLE: &'static str = "Disable Plugin";
        const DESCRIPTION: &'static str = "Switches the plug-in off.";

        fn run(&self, _: &mut Session, params: Value) -> Result<Value, zen_core::CommandError> {
            assert_eq!(params, json!({ "id": "markdown" }));
            MARKDOWN_ON.store(false, std::sync::atomic::Ordering::Relaxed);
            Ok(json!({}))
        }
    }

    struct Harness {
        ctx: egui::Context,
        registry: Arc<Registry>,
        session: Session,
        popup: EverywherePopup,
        picks: Vec<Pick>,
        _server: FakeServer,
        dir: tempfile::TempDir,
        _sdk: tempfile::TempDir,
        keymaps: zen_workbench::keymaps::SharedKeymaps,
        data: tempfile::TempDir,
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> Vec<Event> {
        [true, false]
            .into_iter()
            .map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers })
            .collect()
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("store.go"), STORE).unwrap();
            let sdk = tempfile::tempdir().unwrap();
            std::fs::write(sdk.path().join("pool.go"), SDK).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let data = tempfile::tempdir().unwrap();
            let keymaps: zen_workbench::keymaps::SharedKeymaps = Arc::new(Mutex::new(
                zen_workbench::keymaps::KeymapStore::new(Some(data.path().to_owned()), Vec::new()),
            ));
            zen_workbench::keymaps::register(&mut registry, &keymaps).unwrap();
            let modules = zen_modules::Modules::load("Test", demo_modules(), Some(data.path().join("modules.json")));
            zen_modules::register(&mut registry, &Arc::new(Mutex::new(modules))).unwrap();
            registry.register(InstalledPlugins).unwrap();
            registry.register(DisablePlugin).unwrap();
            let registry = Arc::new(registry);
            let mut session = Session::new();
            session.set_state(SearchHistory::default());
            session
                .state_mut::<zen_modules::search::SearchContributors>()
                .add(Arc::new(ActionsContributor::new(Arc::clone(&registry))));
            session
                .state_mut::<zen_modules::search::SearchContributors>()
                .add(Arc::new(CommandsContributor::new(Arc::clone(&registry))));
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            let server = FakeServer::new();
            server.set_capabilities(json!({ "workspaceSymbolProvider": true }));
            let uri = zen_lsp::position::file_uri(&dir.path().join("store.go"));
            let symbol = |uri: &str, name: &str, kind: u64, line: u32, character: u32| {
                json!({ "name": name, "kind": kind, "containerName": "example.com/shop",
                    "location": { "uri": uri, "range": {
                        "start": { "line": line, "character": character },
                        "end": { "line": line, "character": character + 3 } } } })
            };
            let pool = zen_lsp::position::file_uri(&sdk.path().join("pool.go"));
            let answer = json!([
                symbol(&uri, "CartStore", 23, 2, 5),
                symbol(&uri, "CartStore.AddItem", 6, 4, 20),
                symbol(&uri, "MaxItems", 14, 6, 6),
                symbol(&pool, "CartPool", 23, 2, 5),
            ]);
            server.on_with("workspace/symbol", move |_| answer.clone());
            let launched = server.clone();
            session.state_mut::<Languages>().set_specs(vec![ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            }]);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let popup = EverywherePopup::default();
            Self { ctx, registry, session, popup, picks: Vec::new(), _server: server, dir, _sdk: sdk, keymaps, data }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                ..Default::default()
            };
            let Self { ctx, registry, session, popup, picks, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                picks.extend(popup.show(ui, registry, session, &IdePalette::zen_dark(), &|_| None));
            });
            output.textures_delta.clear();
        }

        fn search(&mut self, tab: Tab, query: &str) {
            self.popup.toggle(tab);
            self.frame(Vec::new());
            self.frame(vec![Event::Text(query.to_owned())]);
            self.settle();
        }

        fn settle(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                self.frame(Vec::new());
                let answer = self.popup.answer(&mut self.session);
                if !answer.pending && !answer.held {
                    break;
                }
                assert!(Instant::now() < deadline, "timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn drawn(&mut self) -> Vec<(String, bool)> {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                ..Default::default()
            };
            let Self { ctx, registry, session, popup, picks, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                picks.extend(popup.show(ui, registry, session, &IdePalette::zen_dark(), &|_| None));
            });
            output.textures_delta.clear();
            let mut texts = Vec::new();
            for clipped in output.shapes {
                if let egui::Shape::Text(text) = &clipped.shape {
                    let rect = text.visual_bounding_rect();
                    texts.push((text.galley.text().to_owned(), clipped.clip_rect.contains_rect(rect)));
                }
            }
            texts
        }

        fn rows(&mut self) -> Vec<String> {
            rows_of(self.popup.answer(&mut self.session)).iter().map(describe).collect()
        }

        fn copied(&mut self, events: Vec<Event>) -> Vec<String> {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                ..Default::default()
            };
            let Self { ctx, registry, session, popup, picks, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                picks.extend(popup.show(ui, registry, session, &IdePalette::zen_dark(), &|_| None));
            });
            output.textures_delta.clear();
            output
                .platform_output
                .commands
                .into_iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::CopyText(text) => Some(text),
                    _ => None,
                })
                .collect()
        }

        fn click(&mut self, label: &str) {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                ..Default::default()
            };
            let Self { ctx, registry, session, popup, picks, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                picks.extend(popup.show(ui, registry, session, &IdePalette::zen_dark(), &|_| None));
            });
            output.textures_delta.clear();
            let mut found = None;
            for clipped in output.shapes {
                if let egui::Shape::Text(text) = &clipped.shape
                    && text.galley.text() == label
                {
                    found = Some(text.visual_bounding_rect().center());
                }
            }
            let at = found.unwrap_or_else(|| panic!("no {label} drawn"));
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }
    }

    fn listed(nodes: &[(egui::accesskit::Role, String, egui::accesskit::Rect)]) -> Vec<&str> {
        let mut rows: Vec<_> =
            nodes.iter().filter(|(role, ..)| *role == egui::accesskit::Role::ListBoxOption).collect();
        rows.sort_by(|a, b| a.2.y0.total_cmp(&b.2.y0));
        rows.into_iter().map(|(_, label, _)| label.as_str()).collect()
    }

    fn describe(row: &Row) -> String {
        match row {
            Row::Header(title) => format!("# {title}"),
            Row::Item(item, _) => match &item.target {
                SearchTarget::File { .. } if item.detail.is_empty() => format!("file {}", item.name),
                SearchTarget::File { .. } => format!("file {}/{}", item.detail, item.name),
                SearchTarget::Place { .. } => {
                    format!("{} {} {}", item.name, item.detail, item.hint.clone().unwrap_or_default())
                }
                SearchTarget::Directory { .. } => format!("folder {}", item.name),
                SearchTarget::Action { action, .. } => format!("action {action}"),
                SearchTarget::Settings { page } => format!("{} settings {page}", item.name),
                SearchTarget::Switch { on, command, .. } => format!("{} switch {on} {command}", item.name),
                SearchTarget::Value { value } => format!("{} value {value}", item.name),
                SearchTarget::Run { command, icon, .. } => format!("{} {icon} {} {command}", item.name, item.detail),
            },
            Row::More(contributor) => format!("more {contributor}"),
        }
    }

    #[test]
    fn every_tab_draws_its_toolbar_filter_preview_and_footer() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        zen_i18n::register(zen_workbench::LOCALES);
        for language in languages {
            zen_i18n::set_language(Some(language));
            let mut harness = Harness::new();
            harness.session.set_state(zen_modules::search::TypeNoun::Type);
            std::fs::create_dir_all(harness.dir.path().join("storage")).unwrap();
            let cart = harness.dir.path().join("storage/cart.go");
            std::fs::write(&cart, "package storage\n\n// CartStore keeps carts.\n").unwrap();
            let store = harness.dir.path().join("store.go");
            for path in [&cart, &store] {
                harness.registry.dispatch(&mut harness.session, "file.document.open", json!({ "path": path })).unwrap();
            }
            harness.registry.dispatch(&mut harness.session, "file.index.refresh", json!({ "paths": [&cart] })).unwrap();
            harness.wait_for_text("CartStore keeps");
            let preview = json!({ "preview": true, "previewHeight": 160.0 });
            harness.registry.dispatch(&mut harness.session, "search.everywhere.setOptions", preview).unwrap();
            let palette = IdePalette::zen_dark();
            let mut screen = crate::offscreen::Offscreen::new(vec2(1000.0, 700.0), 1.5);
            palette.apply(screen.context());
            screen.set_background(palette.editor_background);
            let Harness { registry, session, popup, picks, .. } = &mut harness;
            let mut shot = |screen: &mut crate::offscreen::Offscreen,
                            popup: &mut EverywherePopup,
                            session: &mut Session,
                            events: Vec<Event>,
                            name: &str| {
                screen.step(Vec::new(), |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
                screen.step(events, |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
                let deadline = Instant::now() + Duration::from_secs(60);
                loop {
                    screen.step(Vec::new(), |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
                    let answer = popup.answer(session);
                    if !answer.pending && !answer.held {
                        break;
                    }
                    assert!(Instant::now() < deadline, "timed out");
                    std::thread::sleep(Duration::from_millis(10));
                }
                let mut draw = |ui: &mut Ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None));
                if let Some(dir) = &snapshots {
                    let image = screen.render(&mut draw);
                    crate::offscreen::write_png(&dir.join(format!("se-{name}-{language}.png")), &image).unwrap();
                }
                let tree = screen.accessibility_tree(&mut draw);
                tree.nodes
                    .iter()
                    .filter_map(|(_, node)| Some((node.role(), node.label()?.to_owned(), node.bounds()?)))
                    .collect::<Vec<_>>()
            };
            let click = |nodes: &[(egui::accesskit::Role, String, egui::accesskit::Rect)], label: &str| {
                let (_, _, bounds) = nodes
                    .iter()
                    .find(|(_, text, _)| text == label)
                    .unwrap_or_else(|| panic!("no {label} in {nodes:?}"));
                let at = pos2((bounds.x0 + bounds.x1) as f32 / 2.0, (bounds.y0 + bounds.y1) as f32 / 2.0);
                let button = |pressed| Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                vec![Event::PointerMoved(at), button(true), button(false)]
            };
            let cases = [
                (Tab::ALL, "", "all-empty", t!("ide-se-calculator")),
                (Tab::ALL, "cart", "all", t!("ide-se-calculator")),
                (Tab::CLASSES, "Cart", "types", "Go".to_owned()),
                (Tab::FILES, "", "files-empty", t!("workbench-file-type-directory")),
                (Tab::FILES, "st", "files", t!("workbench-file-type-directory")),
                (Tab::SYMBOLS, "CS.aI", "symbols", "Go".to_owned()),
                (Tab::ACTIONS, "line", "actions", String::new()),
                (Tab::TEXT, "Cart", "text", String::new()),
            ];
            for (tab, query, name, first) in cases {
                popup.toggle(tab.clone());
                let nodes = shot(&mut screen, popup, session, vec![Event::Text(query.to_owned())], name);
                let tabs_end = nodes
                    .iter()
                    .filter(|(role, ..)| *role == egui::accesskit::Role::Tab)
                    .map(|(_, _, bounds)| bounds.x1)
                    .fold(0.0, f64::max);
                let control = match tab.id() {
                    "all" => t!("ide-se-include-non-project"),
                    "actions" => t!("ide-se-include-disabled"),
                    _ => t!("ide-scope-scope"),
                };
                let (_, _, bounds) =
                    nodes.iter().find(|(_, label, _)| *label == control).expect("the toolbar's control");
                assert!(bounds.x0 > tabs_end, "{name}: the toolbar beside the tabs, not over them: {nodes:?}");
                if tab == Tab::ALL || tab == Tab::ACTIONS {
                    let shown = bounds.x1 - bounds.x0;
                    assert!(shown > 60.0, "{name}: the check box shows its label, not the box alone: {shown}");
                }
                let mask = t!("ide-se-text-mask");
                let switches = [t!("ide-find-match-case"), t!("ide-find-words"), t!("ide-find-regex")];
                let text_controls: Vec<_> = nodes
                    .iter()
                    .filter(|(_, label, _)| *label == mask || switches.contains(label))
                    .map(|(_, _, bounds)| bounds)
                    .collect();
                if tab == Tab::TEXT {
                    assert_eq!(text_controls.len(), 4, "{name}: the mask combo, Cc, W and .*: {nodes:?}");
                    assert!(text_controls.iter().all(|other| other.x0 > tabs_end || other.y0 > bounds.y1));
                } else {
                    assert!(text_controls.is_empty(), "{name}: Cc, W, .* and the mask only on Text");
                }
                let filter = t!("ide-se-filter");
                if filter_option(tab.id()).is_some() {
                    let menu = shot(&mut screen, popup, session, click(&nodes, &filter), &format!("{name}-filter"));
                    let mut boxes: Vec<_> = menu
                        .iter()
                        .filter(|(role, label, _)| *role == egui::accesskit::Role::CheckBox && *label != control)
                        .collect();
                    boxes.sort_by(|a, b| a.2.y0.total_cmp(&b.2.y0));
                    let entries: Vec<&str> = boxes.iter().map(|(_, label, _)| label.as_str()).collect();
                    assert_eq!(entries.first(), Some(&first.as_str()), "{name}: {entries:?}");
                    if tab == Tab::ALL {
                        assert!(entries.contains(&t!("ide-se-text").as_str()), "Text is in the Filter of All");
                    }
                    shot(
                        &mut screen,
                        popup,
                        session,
                        key(Key::Escape, egui::Modifiers::NONE),
                        &format!("{name}-closed"),
                    );
                    assert_eq!(popup.open_tab(), Some(tab.clone()), "Escape closes the Filter first");
                } else {
                    assert!(!nodes.iter().any(|(_, label, _)| *label == filter), "{name}: no Filter");
                }
                popup.toggle(tab);
                assert_eq!(popup.open_tab(), None);
            }
            let away = Event::PointerMoved(pos2(4.0, 690.0));
            let options = json!({ "textMatchCase": true, "textWholeWords": true, "textMask": "*.go" });
            registry.dispatch(session, "search.everywhere.setOptions", options).unwrap();
            popup.toggle(Tab::TEXT);
            let nodes = shot(
                &mut screen,
                popup,
                session,
                vec![away.clone(), Event::Text("CartStore".to_owned())],
                "text-options",
            );
            let rows = listed(&nodes);
            let lines = ["// CartStore keeps carts.", "type CartStore struct{}", "func (s *CartStore) AddItem() {}"];
            assert_eq!(rows, lines, "Cc and W on, *.go");
            popup.toggle(Tab::TEXT);
            let options = json!({ "textMatchCase": false, "textWholeWords": false, "textMask": "" });
            registry.dispatch(session, "search.everywhere.setOptions", options).unwrap();
            popup.toggle(Tab::ALL);
            let nodes =
                shot(&mut screen, popup, session, vec![away, Event::Text("keeps carts".to_owned())], "all-text");
            let rows = listed(&nodes);
            assert_eq!(rows.last(), Some(&"// CartStore keeps carts."), "text at the bottom of All: {rows:?}");
            popup.toggle(Tab::ALL);
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn the_symbols_tab_finds_by_humps_and_enter_goes_to_the_symbol() {
        let mut harness = Harness::new();
        harness.search(Tab::SYMBOLS, "CS.aI");
        assert_eq!(harness.rows(), ["CartStore.AddItem example.com/shop store.go:5"]);
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        let Some(Pick::Place(path, offset)) = harness.picks.pop() else { panic!("Enter picked nothing") };
        assert_eq!(harness.popup.open_tab(), None, "the popup closed");
        harness
            .registry
            .dispatch(&mut harness.session, "navigate.place.goTo", json!({ "path": path, "offset": offset }))
            .unwrap();
        let workbench = harness.session.state::<Workbench>().unwrap();
        let document = workbench.active_document().unwrap();
        assert_eq!(document.path(), harness.dir.path().join("store.go"));
        let caret = document.selections().primary().range().start;
        assert!(STORE.chars().skip(caret).collect::<String>().starts_with("AddItem"));
        assert_eq!(harness.session.state::<SearchHistory>().unwrap().data().queries, ["CS.aI"]);
    }

    #[test]
    fn the_list_grows_when_more_results_come_after_fewer() {
        let mut harness = Harness::new();
        harness.search(Tab::SYMBOLS, "CS.aI");
        assert_eq!(harness.rows().len(), 1);
        for _ in 0..3 {
            harness.drawn();
        }
        harness.popup.query = "t".to_owned();
        harness.settle();
        let names: Vec<String> = rows_of(harness.popup.answer(&mut harness.session))
            .iter()
            .filter_map(|row| match row {
                Row::Item(item, _) => Some(item.name.clone()),
                Row::Header(_) | Row::More(_) => None,
            })
            .collect();
        assert!(names.len() >= 3, "{names:?}");
        for _ in 0..3 {
            harness.drawn();
        }
        let drawn = harness.drawn();
        for name in &names {
            let shown = drawn.iter().find(|(text, _)| text == name);
            assert!(matches!(shown, Some((_, true))), "{name} shows whole: {drawn:?}");
        }
        for _ in 1..names.len() {
            harness.frame(key(Key::ArrowDown, egui::Modifiers::NONE));
        }
        harness.drawn();
        let drawn = harness.drawn();
        let last = names.last().unwrap();
        assert!(drawn.iter().any(|(text, whole)| text == last && *whole), "{last} in view: {drawn:?}");
    }

    #[test]
    fn a_query_typed_in_the_other_layout_finds_what_it_means_and_a_pasted_one_does_not() {
        let mut harness = Harness::new();
        harness.search(Tab::FILES, "ыещку");
        assert!(harness.rows().iter().any(|row| row == "file store.go"), "typed: {:?}", harness.rows());
        harness.popup.toggle(Tab::FILES);

        harness.popup.toggle(Tab::FILES);
        harness.frame(Vec::new());
        harness.frame(vec![Event::Paste("ыещку".to_owned())]);
        harness.settle();
        assert!(harness.rows().iter().all(|row| !row.contains("store")), "pasted: {:?}", harness.rows());
        harness.frame(vec![Event::Text("ы".to_owned())]);
        harness.settle();
        assert!(!harness.popup.typed);
        let erase: Vec<Event> = (0..6).flat_map(|_| key(Key::Backspace, egui::Modifiers::NONE)).collect();
        harness.frame(erase);
        assert!(harness.popup.query.is_empty(), "{:?}", harness.popup.query);
        harness.frame(vec![Event::Text("ыещ".to_owned())]);
        harness.settle();
        assert!(harness.popup.typed, "{:?}", harness.popup.query);
        assert!(harness.rows().iter().any(|row| row == "file store.go"), "typed again: {:?}", harness.rows());
    }

    #[test]
    fn enter_picks_without_reaching_the_editor_drawn_after_the_popup() {
        let mut harness = Harness::new();
        harness.search(Tab::FILES, "store");
        let input = egui::RawInput {
            events: key(Key::Enter, egui::Modifiers::NONE),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
            ..Default::default()
        };
        let Harness { ctx, registry, session, popup, picks, .. } = &mut harness;
        let mut leaked = true;
        let mut output = ctx.run_ui(input, |ui| {
            picks.extend(popup.show(ui, registry, session, &IdePalette::zen_dark(), &|_| None));
            leaked = ui.input(|input| input.key_pressed(Key::Enter));
        });
        output.textures_delta.clear();
        assert!(matches!(harness.picks.pop(), Some(Pick::File(_, None))));
        assert!(!leaked, "the editor would type a new line at the place it opens");
    }

    struct Gate(Arc<std::sync::atomic::AtomicBool>);

    impl SearchContributor for Gate {
        fn id(&self) -> &str {
            "gate"
        }

        fn title(&self) -> &str {
            "ide-se-files"
        }

        fn weight(&self) -> i32 {
            1000
        }

        fn search(&self, _: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
            if !self.0.load(std::sync::atomic::Ordering::Relaxed) {
                return SearchProgress::Pending("ide-se-classes-searching");
            }
            let names: Vec<String> = if query.text.ends_with("none") || !query.text.is_ascii() {
                Vec::new()
            } else if query.text.ends_with("many") {
                (0..60).map(|index| format!("{}{index}", query.text)).collect()
            } else {
                vec![query.text.clone()]
            };
            for name in names {
                let path = format!("{name}.go").into();
                let item = SearchItem {
                    name,
                    detail: String::new(),
                    hint: None,
                    ranges: Vec::new(),
                    detail_ranges: Vec::new(),
                    in_project: true,
                    available: true,
                    target: SearchTarget::File { path, line: None, column: None },
                };
                if !sink.push(item) {
                    break;
                }
            }
            SearchProgress::Done
        }
    }

    impl Harness {
        fn gated() -> (Self, Arc<std::sync::atomic::AtomicBool>) {
            let mut harness = Self::new();
            let open = Arc::new(std::sync::atomic::AtomicBool::new(true));
            harness
                .session
                .state_mut::<zen_modules::search::SearchContributors>()
                .add(Arc::new(Gate(Arc::clone(&open))));
            harness.search(Tab(Cow::Borrowed("gate")), "old");
            assert_eq!(harness.rows(), ["file old"]);
            (harness, open)
        }

        fn frames_until_picked(&mut self) -> Option<Pick> {
            self.frames_for_a_pick(Duration::from_secs(5))
        }

        fn frames_without_a_pick(&mut self) -> Option<Pick> {
            self.frames_for_a_pick(Duration::from_millis(500))
        }

        fn frames_for_a_pick(&mut self, wait: Duration) -> Option<Pick> {
            let deadline = Instant::now() + wait;
            while Instant::now() < deadline && self.picks.is_empty() {
                std::thread::sleep(Duration::from_millis(20));
                self.frame(Vec::new());
            }
            self.picks.pop()
        }
    }

    fn picked_file(pick: Option<Pick>) -> String {
        match pick {
            Some(Pick::File(path, _)) => path,
            _ => panic!("no file picked"),
        }
    }

    #[test]
    fn enter_while_the_results_are_held_opens_the_first_result_of_the_query_typed() {
        let (mut harness, open) = Harness::gated();
        open.store(false, std::sync::atomic::Ordering::Relaxed);
        harness.frame(vec![Event::Text("store".to_owned())]);
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        assert!(harness.picks.is_empty(), "the held row of the last query is not opened");
        assert!(harness.frames_without_a_pick().is_none(), "it waits for the query's results");
        assert!(harness.popup.open_tab().is_some());
        open.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(picked_file(harness.frames_until_picked()), "oldstore.go");
        assert_eq!(harness.popup.open_tab(), None);
        assert_eq!(harness.session.state::<SearchHistory>().unwrap().data().queries, ["oldstore"]);
    }

    #[test]
    fn enter_while_more_lists_more_opens_the_first_of_them() {
        let (mut harness, open) = Harness::gated();
        harness.frame(vec![Event::Text("many".to_owned())]);
        harness.settle();
        assert_eq!(harness.rows().len(), engine::TAB_LIMIT + 1);
        harness.frame(key(Key::ArrowDown, egui::Modifiers::COMMAND));
        assert_eq!(harness.selected_row(), "more gate");
        open.store(false, std::sync::atomic::Ordering::Relaxed);
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        harness.frame(Vec::new());
        assert!(harness.popup.answer(&mut harness.session).held, "more… waits for the rest of the group");
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        assert!(harness.picks.is_empty(), "Enter waits for them");
        open.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(picked_file(harness.frames_until_picked()), "oldmany50.go", "the first of them opens");
    }

    #[test]
    fn a_match_of_the_english_name_only_shows_that_name_with_its_letters_marked() {
        let (score, ranges, english) = best_match("dup line", "Дублировать строку", "Duplicate Line").unwrap();
        assert!(score > 0);
        assert!(ranges.is_empty());
        assert_eq!(english, Some(("Duplicate Line".to_owned(), vec![0..3, 10..14])));
        let (_, ranges, english) = best_match("дубл", "Дублировать строку", "Duplicate Line").unwrap();
        assert_eq!((ranges, english), (vec![Range { start: 0, end: 4 }], None));
        assert_eq!(best_match("dup", "Duplicate Line", "Duplicate Line").unwrap().2, None);
    }

    #[test]
    fn enter_in_the_frame_the_query_changes_waits_for_the_query() {
        let (mut harness, _open) = Harness::gated();
        let mut events = vec![Event::Text("store".to_owned())];
        events.extend(key(Key::Enter, egui::Modifiers::NONE));
        harness.frame(events);
        assert!(harness.picks.is_empty(), "the rows drawn were those of the last query");
        assert_eq!(picked_file(harness.frames_until_picked()), "oldstore.go");
    }

    #[test]
    fn enter_before_a_query_with_no_results_does_nothing() {
        let (mut harness, open) = Harness::gated();
        open.store(false, std::sync::atomic::Ordering::Relaxed);
        harness.frame(vec![Event::Text("none".to_owned())]);
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        open.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(harness.frames_without_a_pick().is_none());
        assert!(harness.popup.open_tab().is_some(), "the popup stays open on the empty list");
        harness.frame(vec![Event::Key {
            key: Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(harness.frames_without_a_pick().is_none(), "Enter is forgotten once the query had no results");
    }

    #[test]
    fn enter_is_forgotten_when_the_results_take_longer_than_it_waits() {
        let (mut harness, open) = Harness::gated();
        open.store(false, std::sync::atomic::Ordering::Relaxed);
        harness.frame(vec![Event::Text("store".to_owned())]);
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        assert!(harness.popup.commit.is_some());
        harness.popup.commit = Instant::now().checked_sub(ENTER_WAIT);
        harness.frame(Vec::new());
        open.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(harness.frames_without_a_pick().is_none(), "a late result is not opened by an old Enter");
        assert_eq!(harness.popup.open_tab(), Some(Tab(Cow::Borrowed("gate"))));
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        assert_eq!(picked_file(harness.picks.pop()), "oldstore.go");
    }

    #[test]
    fn rows_carry_what_matched_and_a_line_suffix_goes_with_the_file() {
        let mut harness = Harness::new();
        harness.search(Tab::SYMBOLS, "CS.aI");
        let rows = rows_of(harness.popup.answer(&mut harness.session));
        let Some(Row::Item(symbol, _)) = rows.first() else { panic!("no symbol") };
        assert_eq!(symbol.ranges, [[0, 1], [4, 5], [10, 11], [13, 14]]);
        harness.popup.toggle(Tab::SYMBOLS);
        harness.search(Tab::FILES, "sto:3:2");
        let rows = rows_of(harness.popup.answer(&mut harness.session));
        let Some(Row::Item(file, _)) = rows.first() else { panic!("no file") };
        let SearchTarget::File { line, column, .. } = file.target else { panic!("not a file") };
        assert_eq!((file.name.as_str(), line, column), ("store.go", Some(3), Some(2)));
        assert_eq!(file.ranges, [[0, 3]]);
        let key = |pressed| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        harness.frame(vec![key(true), key(false)]);
        let Some(Pick::File(path, Some((3, Some(2))))) = harness.picks.pop() else { panic!("Enter picked no line") };
        assert!(path.ends_with("store.go"));
        let (_, ranges) = action_score("dup lin", "Duplicate Line or Selection").unwrap();
        assert_eq!(ranges, [0..3, 10..13]);
        let (_, ranges) = action_score("line dup", "Duplicate Line or Selection").unwrap();
        assert_eq!(ranges, [0..3, 10..14]);
    }

    impl Harness {
        fn with_files(names: &[String]) -> Self {
            let mut harness = Self::new();
            let paths: Vec<PathBuf> = names.iter().map(|name| harness.dir.path().join(name)).collect();
            harness.wait_for_index("", |found| found["indexing"] == false);
            for path in &paths {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, "").unwrap();
            }
            let refresh = json!({ "paths": paths });
            harness.registry.dispatch(&mut harness.session, "file.index.refresh", refresh).unwrap();
            for name in names {
                harness.wait_for_index(name, |found| {
                    found["files"].as_array().into_iter().flatten().any(|file| file["relative"] == json!(name))
                });
            }
            harness
        }

        fn wait_for_index(&mut self, query: &str, done: impl Fn(&Value) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let params = json!({ "query": query, "limit": 50 });
                let found = self.registry.dispatch(&mut self.session, "navigate.file.goTo", params).unwrap();
                if done(&found) {
                    return;
                }
                assert!(Instant::now() < deadline, "the file index timed out: {found}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn wait_for_text(&mut self, text: &str) {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let params = json!({ "query": { "text": text } });
                let found = self.registry.dispatch(&mut self.session, "search.text.find", params).unwrap();
                if found["matches"].as_array().is_some_and(|matches| !matches.is_empty()) {
                    return;
                }
                assert!(Instant::now() < deadline, "the text index timed out: {found}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn selected_row(&mut self) -> String {
            let rows = self.rows();
            let selectable: Vec<&String> = rows.iter().filter(|row| !row.starts_with("# ")).collect();
            selectable[self.popup.selected].clone()
        }
    }

    #[test]
    fn more_lists_the_rest_of_a_group_and_cmd_arrows_jump_to_the_ends() {
        let names: Vec<String> = (0..10).map(|index| format!("zqx{index}.md")).collect();
        let mut harness = Harness::with_files(&names);
        harness.search(Tab::ALL, "zqx");
        let mut first: Vec<String> = vec!["# Files".to_owned()];
        first.extend(names[..engine::ALL_LIMIT].iter().map(|name| format!("file {name}")));
        first.push("more files".to_owned());
        assert_eq!(harness.rows(), first);
        let drawn: Vec<String> = harness.drawn().into_iter().map(|(text, _)| text).collect();
        assert!(drawn.iter().any(|text| text == "more…"), "{drawn:?}");

        harness.frame(key(Key::ArrowDown, egui::Modifiers::COMMAND));
        assert_eq!(harness.selected_row(), "more files", "⌘↓ goes to the end");
        assert_eq!(harness.popup.query, "zqx", "the field keeps the query");
        harness.frame(key(Key::ArrowUp, egui::Modifiers::COMMAND));
        assert_eq!(harness.popup.selected, 0, "⌘↑ goes to the start");

        harness.frame(key(Key::ArrowDown, egui::Modifiers::COMMAND));
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        harness.settle();
        assert!(harness.picks.is_empty(), "more… picks nothing");
        assert!(harness.popup.open_tab().is_some(), "the popup stays open");
        let mut all: Vec<String> = vec!["# Files".to_owned()];
        all.extend(names.iter().map(|name| format!("file {name}")));
        assert_eq!(harness.rows(), all, "the rest of the files, without more…");
        assert_eq!(harness.selected_row(), all[1 + engine::ALL_LIMIT], "the first one loaded");

        harness.popup.query = "zqx1".to_owned();
        harness.settle();
        harness.popup.query = "zqx".to_owned();
        harness.settle();
        assert_eq!(harness.rows(), first, "a new query starts again");
    }

    #[test]
    fn a_folder_is_found_on_files_and_enter_picks_it_for_the_project_tree() {
        let mut harness = Harness::with_files(&["internal/cart/cart.go".to_owned()]);
        harness.search(Tab::FILES, "cart/");
        harness.popup.query = "intern".to_owned();
        harness.settle();
        assert_eq!(harness.rows(), ["folder internal"]);
        let rows = rows_of(harness.popup.answer(&mut harness.session));
        let Some(Row::Item(folder, _)) = rows.first() else { panic!("no folder") };
        assert_eq!(folder.ranges, [[0, 6]], "the letters typed are marked");
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        let Some(Pick::Directory(path)) = harness.picks.pop() else { panic!("Enter picked no folder") };
        assert!(Path::new(&path).ends_with("internal"), "{path}");
    }

    #[test]
    fn all_lists_types_as_classes_and_the_other_symbols_once() {
        let mut harness = Harness::new();
        harness.search(Tab::ALL, "cart");
        let rows = harness.rows();
        let at = |row: &str| rows.iter().position(|other| other == row).unwrap_or_else(|| panic!("{row}: {rows:?}"));
        assert_eq!(at("# Classes") + 1, at("CartStore example.com/shop "));
        assert!(!rows.iter().any(|row| row == "# Symbols"), "a type is listed once: {rows:?}");
        harness.popup.toggle(Tab::ALL);
        harness.search(Tab::ALL, "item");
        let rows = harness.rows();
        let symbols = rows.iter().position(|row| row == "# Symbols").unwrap();
        assert_eq!(
            rows[symbols + 1..symbols + 3],
            ["CartStore.AddItem example.com/shop store.go:5", "MaxItems example.com/shop store.go:7"]
        );
    }

    #[test]
    fn non_project_items_come_only_when_asked_for() {
        let mut harness = Harness::new();
        harness.search(Tab::CLASSES, "cart");
        assert_eq!(harness.rows(), ["CartStore example.com/shop "]);
        harness.popup.include_non_project = true;
        harness.settle();
        assert_eq!(harness.rows(), ["CartStore example.com/shop ", "CartPool example.com/shop "]);
    }

    #[test]
    fn the_command_answers_what_the_popup_lists() {
        let mut harness = Harness::new();
        harness.search(Tab::ALL, "cart");
        let shown: Vec<String> =
            harness.popup.answer(&mut harness.session).items().map(|item| item.name.clone()).collect();
        let found = harness
            .registry
            .dispatch(
                &mut harness.session,
                "search.everywhere.query",
                json!({ "tab": "all", "query": "cart", "includeNonProject": false, "waitMs": 5000 }),
            )
            .unwrap();
        let listed: Vec<String> = found["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["items"].as_array().unwrap().iter())
            .map(|item| item["name"].as_str().unwrap().to_owned())
            .collect();
        assert!(!shown.is_empty());
        assert_eq!(listed, shown);
        let groups: Vec<&str> =
            found["groups"].as_array().unwrap().iter().filter_map(|group| group["contributor"].as_str()).collect();
        assert_eq!(groups[0], "classes");
        let listed =
            harness.registry.dispatch(&mut harness.session, "search.everywhere.contributors", json!({})).unwrap();
        let ids: Vec<&str> = listed.as_array().unwrap().iter().filter_map(|info| info["id"].as_str()).collect();
        assert_eq!(ids, ["commands", "calculator", "recentFiles", "classes", "files", "symbols", "actions", "text"]);
    }

    #[test]
    fn actions_and_queries_are_remembered_and_alt_down_brings_a_query_back() {
        let mut harness = Harness::new();
        harness.search(Tab::ACTIONS, "go to line");
        let first = harness.rows().into_iter().next().unwrap();
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        assert!(matches!(harness.picks.pop(), Some(Pick::Action(_))));
        let data = harness.session.state::<SearchHistory>().unwrap().data();
        assert_eq!(data.queries, ["go to line"]);
        assert_eq!(format!("action {}", data.actions[0]), first);
        harness.popup.toggle(Tab::ACTIONS);
        harness.frame(Vec::new());
        harness.frame(key(Key::ArrowDown, egui::Modifiers::ALT));
        assert_eq!(harness.popup.query, "go to line");
        harness.frame(key(Key::ArrowUp, egui::Modifiers::ALT));
        assert_eq!(harness.popup.query, "");
    }

    #[test]
    fn the_text_tab_matches_with_its_switches_and_mask_and_enter_opens_the_match() {
        let mut harness = Harness::new();
        let notes = harness.dir.path().join("notes.md");
        std::fs::write(&notes, "see cartstore notes\n").unwrap();
        harness.registry.dispatch(&mut harness.session, "file.index.refresh", json!({ "paths": [&notes] })).unwrap();
        harness.wait_for_text("cartstore notes");
        harness.wait_for_text("CartStore struct");
        harness.search(Tab::TEXT, "CartStore");
        let all = ["file see cartstore notes", "file type CartStore struct{}", "file func (s *CartStore) AddItem() {}"];
        assert_eq!(harness.rows(), all);
        let rows = rows_of(harness.popup.answer(&mut harness.session));
        let Some(Row::Item(first, contributor)) = rows.first() else { panic!("no line") };
        assert_eq!((first.hint.as_deref(), contributor.as_str()), (Some("notes.md:1"), "text"));
        assert_eq!(first.ranges, [[4, 13]], "the match is marked in the line");

        harness.click("Cc");
        harness.settle();
        assert!(harness.session.state::<SearchHistory>().unwrap().options().text_match_case);
        assert_eq!(harness.rows(), all[1..], "Cc: the case matches");
        harness.click("Cc");
        harness.settle();

        harness.click(&t!("ide-se-text-mask-any"));
        harness.frame(vec![Event::Text("*.md".to_owned())]);
        harness.settle();
        assert_eq!(harness.session.state::<SearchHistory>().unwrap().options().text_mask, "*.md");
        assert_eq!(harness.rows(), all[..1], "the mask keeps the Markdown file");
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        let Some(Pick::File(path, Some((1, Some(5))))) = harness.picks.pop() else { panic!("Enter opened no match") };
        assert!(path.ends_with("notes.md"), "{path}");
        assert_eq!(harness.popup.open_tab(), None);
        let history = harness.registry.dispatch(&mut harness.session, "search.findHistory.list", json!({})).unwrap();
        assert_eq!(history["masks"], json!(["*.md"]), "the mask joins the history Find in Files shows");
    }

    #[test]
    fn in_a_narrow_window_the_text_toolbar_gives_way_without_covering_the_tabs() {
        zen_i18n::register(zen_workbench::LOCALES);
        for language in ["en", "ru"] {
            for width in [640.0, 720.0, 800.0] {
                zen_i18n::with_language(language, || {
                    let mut harness = Harness::new();
                    let palette = IdePalette::zen_dark();
                    let mut screen = crate::offscreen::Offscreen::new(vec2(width, 600.0), 1.0);
                    palette.apply(screen.context());
                    let Harness { registry, session, popup, picks, .. } = &mut harness;
                    popup.toggle(Tab::TEXT);
                    let mut draw = |ui: &mut Ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None));
                    screen.step(Vec::new(), &mut draw);
                    screen.step(Vec::new(), &mut draw);
                    let tree = screen.accessibility_tree(&mut draw);
                    let nodes: Vec<_> = tree
                        .nodes
                        .iter()
                        .filter_map(|(_, node)| Some((node.role(), node.label()?.to_owned(), node.bounds()?)))
                        .collect();
                    let tabs_end = nodes
                        .iter()
                        .filter(|(role, ..)| *role == egui::accesskit::Role::Tab)
                        .map(|(_, _, bounds)| bounds.x1)
                        .fold(0.0, f64::max);
                    let bounds = |label: String| {
                        nodes.iter().find(|(_, name, _)| *name == label).map(|(_, _, bounds)| *bounds).unwrap()
                    };
                    let scope = bounds(t!("ide-scope-scope"));
                    let mask = bounds(t!("ide-se-text-mask"));
                    let name = format!("{language} {width}");
                    assert!(scope.x0 >= tabs_end, "{name}: the scope over the tabs: {scope:?}, tabs end at {tabs_end}");
                    assert!(mask.x0 >= scope.x1, "{name}: the mask over the scope: {mask:?} {scope:?}");
                    assert!(mask.x1 <= f64::from(width), "{name}: the mask out of the window: {mask:?}");
                });
            }
        }
    }

    #[test]
    fn the_tabs_follow_the_contributors() {
        let harness = Harness::new();
        let ids: Vec<String> = tabs(&harness.session).into_iter().map(|(tab, _)| tab.id().to_owned()).collect();
        assert_eq!(ids, ["all", "classes", "files", "symbols", "actions", "text"]);
    }

    #[test]
    fn an_empty_query_lists_recent_files_on_all_and_nothing_on_the_other_tabs() {
        let mut harness = Harness::new();
        std::fs::write(harness.dir.path().join("other.go"), "package shop\n").unwrap();
        for file in ["store.go", "other.go"] {
            let path = harness.dir.path().join(file);
            harness.registry.dispatch(&mut harness.session, "file.document.open", json!({ "path": path })).unwrap();
        }
        harness.search(Tab::ALL, "");
        assert_eq!(harness.rows(), ["# Recent Files", "file store.go"], "the current file left out");
        harness.popup.toggle(Tab::ALL);
        for tab in [Tab::FILES, Tab::CLASSES, Tab::SYMBOLS, Tab::ACTIONS, Tab::TEXT] {
            harness.search(tab.clone(), "");
            assert!(harness.rows().is_empty(), "{tab:?}");
            let drawn = harness.drawn();
            let nothing = t!("ide-goto-file-nothing");
            assert!(!drawn.iter().any(|(text, _)| *text == nothing), "the popup shrinks to the field: {drawn:?}");
            assert!(drawn.iter().any(|(text, _)| *text == t!("ide-se-hint")), "{drawn:?}");
            harness.popup.toggle(tab);
        }
    }

    #[test]
    fn the_toolbar_follows_the_tab_and_disabled_actions_come_when_included() {
        let mut harness = Harness::new();
        harness.search(Tab::ALL, "cart");
        let drawn = harness.drawn();
        assert!(drawn.iter().any(|(text, _)| *text == t!("ide-se-include-non-project")), "{drawn:?}");
        harness.popup.toggle(Tab::ALL);
        harness.search(Tab::ACTIONS, "duplicate line");
        let drawn = harness.drawn();
        assert!(drawn.iter().any(|(text, _)| *text == t!("ide-se-include-disabled")), "{drawn:?}");
        assert!(!drawn.iter().any(|(text, _)| *text == t!("ide-se-include-non-project")));
        let listed = |harness: &mut Harness| harness.rows().iter().any(|row| row.contains("duplicate"));
        assert!(!listed(&mut harness), "an editor action cannot run without a file: left out");
        let include = json!({ "includeDisabledActions": true });
        harness.registry.dispatch(&mut harness.session, "search.everywhere.setOptions", include).unwrap();
        harness.settle();
        assert!(listed(&mut harness), "{:?}", harness.rows());
    }

    #[test]
    fn the_preview_and_the_footer_show_the_selected_item_and_shift_enter_opens_it_on_the_right() {
        let mut harness = Harness::new();
        let preview = json!({ "preview": true });
        harness.registry.dispatch(&mut harness.session, "search.everywhere.setOptions", preview).unwrap();
        harness.search(Tab::SYMBOLS, "CS.aI");
        harness.drawn();
        let drawn = harness.drawn();
        let shown = |text: &str| drawn.iter().any(|(drawn, _)| drawn == text);
        assert!(shown("func (s *CartStore) AddItem() {}") && shown("5"), "the file in the preview: {drawn:?}");
        assert!(shown("store.go") && shown(&t!("ide-se-open-right")), "the footer: {drawn:?}");
        harness.frame(key(Key::Enter, egui::Modifiers::SHIFT));
        let Some(Pick::RightSplit(inner)) = harness.picks.pop() else { panic!("⇧⏎ picked nothing") };
        assert!(matches!(*inner, Pick::Place(ref path, _) if path.ends_with("store.go")));
        assert_eq!(harness.popup.open_tab(), None);
    }

    #[test]
    fn shift_twice_in_the_open_popup_switches_non_project_items() {
        let mut popup = EverywherePopup::default();
        popup.double_shift();
        assert_eq!(popup.open_tab(), Some(Tab::ALL));
        assert!(!popup.include_non_project);
        popup.double_shift();
        assert_eq!(popup.open_tab(), Some(Tab::ALL), "it stays open");
        assert!(popup.include_non_project);
        popup.toggle(Tab::ACTIONS);
        popup.double_shift();
        assert_eq!(popup.open_tab(), Some(Tab::ALL), "Actions has no checkbox: back to All");
        assert!(popup.include_non_project);
    }

    #[test]
    fn the_tabs_search_in_the_scope_chosen_last() {
        let mut harness = Harness::new();
        std::fs::write(harness.dir.path().join("store_test.go"), "package shop\n").unwrap();
        let path = harness.dir.path().join("store_test.go");
        harness.registry.dispatch(&mut harness.session, "file.index.refresh", json!({ "paths": [path] })).unwrap();
        harness.search(Tab::FILES, "store");
        assert_eq!(harness.rows(), ["file store.go", "file store_test.go"]);
        harness.popup.toggle(Tab::FILES);
        let chosen = json!({ "everywhere": "tests" });
        harness.registry.dispatch(&mut harness.session, "search.scope.select", chosen).unwrap();
        harness.search(Tab::FILES, "store");
        assert_eq!(harness.rows(), ["file store_test.go"]);
        assert_eq!(harness.popup.scope(), "tests");
        let found = harness
            .registry
            .dispatch(
                &mut harness.session,
                "search.everywhere.query",
                json!({ "tab": "files", "query": "store", "scope": "tests" }),
            )
            .unwrap();
        assert_eq!(found["groups"][0]["items"].as_array().map(Vec::len), Some(1), "agents search in a scope too");
        harness.popup.toggle(Tab::FILES);
        harness.search(Tab::ALL, "store");
        assert!(!harness.popup.include_non_project, "All shows Include non-project items unchecked");
        let rows = harness.rows();
        assert!(
            rows.contains(&"file store.go".to_owned()) && rows.contains(&"file store_test.go".to_owned()),
            "{rows:?}"
        );
        harness.popup.double_shift();
        harness.settle();
        assert_eq!(harness.popup.scope(), "allPlaces", "Include non-project items is All Places");
        harness.popup.double_shift();
        harness.settle();
        assert_eq!(harness.popup.scope(), "projectFiles");
    }

    #[test]
    fn shift_twice_alone_and_quickly() {
        let shift = egui::Modifiers::SHIFT;
        let none = egui::Modifiers::NONE;
        let run = |frames: &[(f64, egui::Modifiers, bool)]| {
            let mut detector = DoubleShift::default();
            frames
                .iter()
                .fold(false, |fired, (now, modifiers, typed)| detector.update(*now, *modifiers, *typed) || fired)
        };
        assert!(run(&[(0.0, shift, false), (0.1, none, false), (0.2, shift, false), (0.3, none, false)]));
        assert!(!run(&[(0.0, shift, false), (0.1, none, false), (0.7, shift, false), (0.8, none, false)]), "too slow");
        assert!(
            !run(&[(0.0, shift, true), (0.1, none, false), (0.2, shift, false), (0.3, none, false)]),
            "typed a capital"
        );
        assert!(
            !run(&[
                (0.0, shift, false),
                (0.1, none, false),
                (0.15, none, true),
                (0.2, shift, false),
                (0.3, none, false)
            ]),
            "typed between"
        );
        assert!(
            !run(&[
                (0.0, shift, false),
                (0.1, none, false),
                (0.2, shift | egui::Modifiers::COMMAND, false),
                (0.3, none, false)
            ]),
            "⇧⌘ is not ⇧"
        );
    }

    #[test]
    fn control_twice_from_the_events_winit_sends_and_not_shift_or_control_c() {
        let ctrl = egui::Modifiers { ctrl: true, ..egui::Modifiers::NONE };
        let none = egui::Modifiers::NONE;
        let key = |key: Key, pressed: bool, modifiers: egui::Modifiers| Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        };
        let run = |frames: Vec<(f64, egui::Modifiers, Vec<Event>)>| {
            let mut detector = DoubleShift::of(Modifier::Control);
            assert_eq!(detector.modifier(), Modifier::Control);
            frames.into_iter().fold(false, |fired, (now, modifiers, events)| {
                detector.update(now, modifiers, typed_besides(&events, Modifier::Control)) || fired
            })
        };
        let press = |pressed: bool| vec![key(Key::ControlLeft, pressed, if pressed { ctrl } else { none })];
        assert!(run(vec![
            (0.0, ctrl, press(true)),
            (0.1, none, press(false)),
            (0.2, ctrl, press(true)),
            (0.3, none, press(false))
        ]));
        assert!(
            !run(vec![
                (0.0, ctrl, vec![key(Key::C, true, ctrl)]),
                (0.1, none, press(false)),
                (0.2, ctrl, press(true)),
                (0.3, none, press(false))
            ]),
            "⌃C is not ⌃"
        );
        let shift = egui::Modifiers::SHIFT;
        let shift_key = |pressed: bool| vec![key(Key::ShiftLeft, pressed, if pressed { shift } else { none })];
        assert!(
            !run(vec![
                (0.0, shift, shift_key(true)),
                (0.1, none, shift_key(false)),
                (0.2, shift, shift_key(true)),
                (0.3, none, shift_key(false))
            ]),
            "⇧⇧ is not ⌃⌃"
        );
    }

    #[test]
    fn shift_twice_from_the_events_winit_sends() {
        let shift = egui::Modifiers::SHIFT;
        let none = egui::Modifiers::NONE;
        let key = |key: Key, pressed: bool, modifiers: egui::Modifiers| Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        };
        let run = |frames: Vec<(f64, egui::Modifiers, Vec<Event>)>| {
            let mut detector = DoubleShift::default();
            frames
                .into_iter()
                .fold(false, |fired, (now, modifiers, events)| detector.update(now, modifiers, typed(&events)) || fired)
        };
        let press = |pressed: bool| vec![key(Key::ShiftLeft, pressed, if pressed { shift } else { none })];
        assert!(run(vec![
            (0.0, shift, press(true)),
            (0.1, none, press(false)),
            (0.2, shift, press(true)),
            (0.3, none, press(false))
        ]));
        let command = shift | egui::Modifiers::COMMAND;
        assert!(
            !run(vec![
                (0.0, shift, press(true)),
                (0.1, none, press(false)),
                (0.2, shift, press(true)),
                (0.25, command, vec![]),
                (0.3, shift, vec![]),
                (0.35, none, press(false))
            ]),
            "⇧⌘ is not ⇧"
        );
        let letter = vec![key(Key::A, true, shift), Event::Text("A".to_owned()), key(Key::A, false, shift)];
        assert!(
            !run(vec![
                (0.0, shift, press(true)),
                (0.1, none, press(false)),
                (0.2, shift, press(true)),
                (0.25, shift, letter),
                (0.3, none, press(false))
            ]),
            "typed a capital"
        );
    }

    #[test]
    fn names_match_by_words_or_letters() {
        assert!(action_score("dup", "Duplicate Line or Selection").is_some());
        assert!(action_score("line dup", "Duplicate Line or Selection").is_some(), "words in any order");
        assert!(action_score("dls", "Duplicate Line or Selection").is_some(), "camel humps");
        assert!(action_score("zebra", "Duplicate Line or Selection").is_none());
        assert!(
            action_score("save", "Save").unwrap().0 > action_score("save", "Save All").unwrap().0,
            "the exact name first"
        );
    }

    #[test]
    fn actions_are_found_ranked_and_remembered() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let found = find_actions("find in files", &registry, &mut session, 5);
        assert_eq!(found[0].binding.action, "view.findInFiles.open");
        assert_eq!(found[0].shortcut.as_deref(), Some("⇧⌘F"));
        assert_eq!(find_actions("reload", &registry, &mut session, 1)[0].binding.action, "file.document.syncWithDisk");
        for (query, action) in [
            ("restart ide", "view.ide.restart"),
            ("repair ide", "view.repairIde.start"),
            ("invalidate caches", "view.invalidateCaches.open"),
        ] {
            assert_eq!(find_actions(query, &registry, &mut session, 1)[0].binding.action, action);
        }
        let found = find_actions("", &registry, &mut session, usize::MAX);
        let first_unavailable = found.iter().position(|action| !action.available).unwrap();
        assert!(found[first_unavailable..].iter().all(|action| !action.available));
        session.state_mut::<SearchHistory>().remember_action("view.gotoLine.open");
        let first = find_actions("", &registry, &mut session, 3);
        assert!(first.iter().any(|action| action.binding.action == "view.gotoLine.open"), "{first:?}");
    }

    #[test]
    fn a_slash_lists_the_settings_groups_and_the_plugins_with_their_switches() {
        let mut harness = Harness::new();
        harness.search(Tab::ALL, "/");
        let rows = harness.rows();
        assert_eq!(rows[0], format!("# {}", t!("ide-se-commands")));
        let groups: Vec<&str> = rows[1..].iter().map(|row| row.split(' ').next().unwrap_or_default()).collect();
        assert_eq!(
            groups,
            [
                "/projectstructure",
                "/appearance",
                "/keymap",
                "/editor",
                "/plugins",
                "/vcs",
                "/build",
                "/languages",
                "/tools",
                "/advanced"
            ]
        );
        assert_eq!(rows[2], "/appearance settings appearance");
        harness.frame(key(Key::ArrowDown, egui::Modifiers::NONE));
        harness.frame(key(Key::ArrowDown, egui::Modifiers::NONE));
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        let Some(Pick::Action(binding)) = harness.picks.pop() else { panic!("Enter opened nothing") };
        assert_eq!(
            (binding.command.as_str(), binding.params.clone()),
            ("view.settings.open", json!({ "page": "keymap" }))
        );
        assert_eq!(harness.popup.open_tab(), None);

        harness.search(Tab::ALL, "/plu");
        assert_eq!(harness.rows(), [format!("# {}", t!("ide-se-commands")), "/plugins settings plugins".to_owned()]);
        harness.popup.toggle(Tab::ALL);
        harness.search(Tab::ALL, "/plugins");
        assert_eq!(
            harness.rows()[1..],
            [
                "/plugins settings plugins",
                "Go switch true plugins.module.disable",
                "Docker switch false plugins.module.enable",
                "Markdown switch true plugins.plugin.disable",
            ]
        );
        for _ in 0..2 {
            harness.frame(key(Key::ArrowDown, egui::Modifiers::NONE));
        }
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        harness.settle();
        assert!(harness.picks.is_empty());
        assert_eq!(harness.popup.open_tab(), Some(Tab::ALL), "a switch keeps the popup open");
        assert_eq!(harness.rows()[3], "Docker switch true plugins.module.disable");
        let listed = harness.registry.dispatch(&mut harness.session, "plugins.modules.list", json!({})).unwrap();
        assert_eq!(listed["modules"][1]["enabled"], true, "{listed}");
        harness.frame(vec![Event::Text(" dock".to_owned())]);
        harness.settle();
        assert_eq!(harness.rows()[1..], ["Docker switch true plugins.module.disable"], "the rest filters them");
        let answer = harness
            .registry
            .dispatch(&mut harness.session, "search.everywhere.query", json!({ "query": "/plugins go" }))
            .unwrap();
        assert_eq!(answer["groups"][0]["contributor"], "commands");
        assert_eq!(
            answer["groups"][0]["items"][0]["target"],
            json!({ "kind": "switch", "on": true, "command": "plugins.module.disable", "params": { "id": "go" } })
        );
        let off = json!({ "query": "/plugins go" });
        harness.registry.dispatch(&mut harness.session, "plugins.module.disable", json!({ "id": "go" })).unwrap();
        let answer = harness.registry.dispatch(&mut harness.session, "search.everywhere.query", off).unwrap();
        assert_eq!(answer["groups"][0]["items"][0]["target"]["on"], false, "the same query asks again: {answer}");
        assert_eq!(answer["groups"][0]["items"][0]["hint"], t!("ide-se-restart"));
        let markdown = json!({ "query": "/plugins mark" });
        let answer =
            harness.registry.dispatch(&mut harness.session, "search.everywhere.query", markdown.clone()).unwrap();
        let target = answer["groups"][0]["items"][0]["target"].clone();
        assert_eq!(target["command"], "plugins.plugin.disable", "{answer}");
        harness.registry.dispatch(&mut harness.session, "plugins.plugin.disable", target["params"].clone()).unwrap();
        let answer = harness.registry.dispatch(&mut harness.session, "search.everywhere.query", markdown).unwrap();
        assert_eq!(answer["groups"][0]["items"][0]["target"]["command"], "plugins.plugin.enable", "{answer}");
        MARKDOWN_ON.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn an_enter_pressed_before_the_plugins_are_listed_switches_nothing() {
        let mut harness = Harness::new();
        harness.popup.toggle(Tab::ALL);
        harness.frame(Vec::new());
        let mut typed = vec![Event::Text("/plugins dock".to_owned())];
        typed.extend(key(Key::Enter, egui::Modifiers::NONE));
        harness.frame(typed);
        harness.settle();
        harness.frame(Vec::new());
        assert!(harness.picks.is_empty());
        assert_eq!(harness.popup.open_tab(), Some(Tab::ALL), "the popup stays open");
        let listed = harness.registry.dispatch(&mut harness.session, "plugins.modules.list", json!({})).unwrap();
        assert_eq!(listed["modules"][1]["enabled"], false, "Docker stays off: {listed}");
        assert_eq!(harness.rows()[1..], ["Docker switch false plugins.module.enable"]);
        for _ in 0..3 {
            harness.frame(key(Key::ArrowDown, egui::Modifiers::NONE));
            harness.frame(key(Key::ArrowUp, egui::Modifiers::NONE));
        }
        let listed = harness.registry.dispatch(&mut harness.session, "plugins.modules.list", json!({})).unwrap();
        assert_eq!(listed["modules"][1]["enabled"], false, "arrows switch nothing");
        harness.frame(key(Key::Enter, egui::Modifiers::NONE));
        harness.settle();
        let listed = harness.registry.dispatch(&mut harness.session, "plugins.modules.list", json!({})).unwrap();
        assert_eq!(listed["modules"][1]["enabled"], true, "Enter on the row shown switches it: {listed}");
    }

    #[test]
    fn an_expression_shows_its_value_and_enter_copies_it() {
        let mut harness = Harness::new();
        harness.search(Tab::ALL, "(2 + 3) * 4");
        let rows = harness.rows();
        assert_eq!(rows[..2], [format!("# {}", t!("ide-se-calculator")), "(2 + 3) * 4 = 20 value 20".to_owned()]);
        let copied = harness.copied(key(Key::Enter, egui::Modifiers::NONE));
        assert_eq!(copied, ["20"]);
        assert!(harness.picks.is_empty());
        assert_eq!(harness.popup.open_tab(), None, "the popup closes");
        harness.search(Tab::FILES, "2+2");
        assert!(!harness.rows().iter().any(|row| row.contains("value")), "only on All");
    }

    #[test]
    fn option_enter_on_an_action_assigns_it_a_shortcut() {
        let mut harness = Harness::new();
        harness.search(Tab::ACTIONS, "Recent Files");
        assert_eq!(harness.rows()[0], "action view.recentFiles.open");
        harness.frame(key(Key::Enter, egui::Modifiers::ALT));
        assert!(harness.popup.is_recording(), "⌥⏎ opens the Keyboard Shortcut dialog");
        harness.frame(key(Key::S, egui::Modifiers::COMMAND));
        assert_eq!(harness.popup.open_tab(), Some(Tab::ACTIONS), "the keys go to the dialog");
        harness.click(&t!("keymap-conflict-remove"));
        assert!(!harness.popup.is_recording());
        assert_eq!(harness.popup.open_tab(), Some(Tab::ACTIONS), "the popup stays");
        let store = zen_workbench::keymaps::lock(&harness.keymaps);
        let saved: Vec<String> =
            store.active().keystrokes("view.recentFiles.open").iter().map(ToString::to_string).collect();
        assert_eq!(saved, ["⌘E", "⌘S"]);
        assert!(store.active().keystrokes("file.document.saveAll").is_empty(), "taken from Save All");
        drop(store);

        harness.popup.toggle(Tab::ACTIONS);
        harness.search(Tab::ACTIONS, "Clear Search Everywhere History");
        assert_eq!(harness.rows()[0], "action search.history.clear");
        harness.frame(key(Key::Enter, egui::Modifiers::ALT));
        harness.frame(key(Key::H, egui::Modifiers::CTRL | egui::Modifiers::ALT | egui::Modifiers::SHIFT));
        harness.frame(key(Key::Escape, egui::Modifiers::NONE));
        assert!(!harness.popup.is_recording() && harness.popup.open_tab().is_some(), "Escape cancels the dialog only");
        harness.frame(key(Key::Enter, egui::Modifiers::ALT));
        harness.frame(key(Key::H, egui::Modifiers::CTRL | egui::Modifiers::ALT | egui::Modifiers::SHIFT));
        harness.click(&t!("settings-ok"));
        let keymap = zen_workbench::keymaps::lock(&harness.keymaps).active().clone();
        let stroke = Keystroke::parse("⌃⌥⇧H").unwrap();
        let binding = keymap.lookup(&stroke, Context::Global).expect("bound");
        let runs = resolved(binding, &keymap, &harness.registry).expect("runs the command");
        assert_eq!((runs.action.as_str(), runs.command.as_str()), ("search.history.clear", "search.history.clear"));
        let data = harness.data.path().join("keymap.json");
        assert!(std::fs::read_to_string(data).unwrap().contains("search.history.clear"));
    }

    #[test]
    fn actions_are_found_by_their_abbreviations_and_synonyms() {
        let mut harness = Harness::new();
        let params = json!({ "action": "view.gotoLine.open", "abbreviation": "gl" });
        harness.registry.dispatch(&mut harness.session, "ui.keymap.addAbbreviation", params).unwrap();
        let found = find_actions("gl", &harness.registry, &mut harness.session, 3);
        assert_eq!(found[0].binding.action, "view.gotoLine.open", "{found:?}");
        let found = find_actions("g", &harness.registry, &mut harness.session, 3);
        assert_eq!(found[0].binding.action, "view.gotoLine.open", "the start of an abbreviation too");
        harness.search(Tab::ACTIONS, "preferences");
        assert_eq!(harness.rows()[0], "action view.settings.open");
        let answer = harness.popup.answer(&mut harness.session);
        let item = answer.items().next().unwrap();
        assert_eq!((item.detail.as_str(), item.detail_ranges.clone()), ("preferences", vec![[0, 11]]));
        let found = find_actions("beautify", &harness.registry, &mut harness.session, 1);
        assert_eq!(found[0].binding.action, "editor.code.reformat");
        let (russian, english) = zen_i18n::with_language("ru", || {
            (
                find_actions("отформатировать", &harness.registry, &mut harness.session, 1),
                find_actions("prettify", &harness.registry, &mut harness.session, 1),
            )
        });
        assert_eq!(russian[0].binding.action, "editor.code.reformat");
        assert_eq!(english[0].binding.action, "editor.code.reformat", "English synonyms work in Russian too");
        for action in ["editor.line.duplicate", "ui.theme.select", "view.settings.open"] {
            assert!(!synonyms(action).is_empty(), "{action}");
        }
    }

    fn pointer(pos: Pos2, pressed: Option<bool>) -> Vec<Event> {
        let mut events = vec![Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        events
    }

    impl Harness {
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(pointer(from, None));
            self.frame(pointer(from, Some(true)));
            for step in 1..=4 {
                self.frame(pointer(from + (to - from) * step as f32 / 4.0, None));
            }
            self.frame(pointer(to, Some(false)));
            self.frame(Vec::new());
        }

        fn list(&self) -> Rect {
            self.popup.list.expect("the results are drawn")
        }
    }

    #[test]
    fn the_commands_the_calculator_and_the_shortcut_dialog_draw_in_both_languages() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        zen_i18n::register(zen_workbench::LOCALES);
        for language in languages {
            zen_i18n::set_language(Some(language));
            let mut harness = Harness::new();
            let params = json!({ "action": "view.gotoLine.open", "abbreviation": "gl" });
            harness.registry.dispatch(&mut harness.session, "ui.keymap.addAbbreviation", params).unwrap();
            let palette = IdePalette::zen_dark();
            let mut screen = crate::offscreen::Offscreen::new(vec2(1000.0, 700.0), 1.5);
            palette.apply(screen.context());
            screen.set_background(palette.editor_background);
            let Harness { registry, session, popup, picks, .. } = &mut harness;
            let mut shot = |popup: &mut EverywherePopup, session: &mut Session, events: Vec<Event>, name: &str| {
                screen.step(Vec::new(), |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
                screen.step(events, |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
                let deadline = Instant::now() + Duration::from_secs(60);
                loop {
                    screen.step(Vec::new(), |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
                    let answer = popup.answer(session);
                    if !answer.pending && !answer.held {
                        break;
                    }
                    assert!(Instant::now() < deadline, "timed out");
                    std::thread::sleep(Duration::from_millis(10));
                }
                let mut draw = |ui: &mut Ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None));
                let image = screen.render(&mut draw);
                if let Some(dir) = &snapshots {
                    crate::offscreen::write_png(&dir.join(format!("se-{name}-{language}.png")), &image).unwrap();
                }
                let tree = screen.accessibility_tree(&mut draw);
                tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect::<Vec<_>>()
            };
            popup.toggle(Tab::ALL);
            let labels = shot(popup, session, vec![Event::Text("/".to_owned())], "slash");
            assert!(labels.iter().any(|label| label == "/appearance"), "{labels:?}");
            popup.toggle(Tab::ALL);
            popup.toggle(Tab::ALL);
            let labels = shot(popup, session, vec![Event::Text("/plugins".to_owned())], "plugins");
            assert!(labels.iter().any(|label| label == "Docker"), "{labels:?}");
            let mut keys = key(Key::ArrowDown, egui::Modifiers::NONE);
            keys.extend(key(Key::Enter, egui::Modifiers::NONE));
            shot(popup, session, keys, "plugins-switched");
            let answer = popup.answer(session);
            let go = answer.items().find(|item| item.name == "Go").expect("Go listed");
            assert_eq!(go.hint.as_deref(), Some(t!("ide-se-restart").as_str()), "a restart is asked for");
            popup.toggle(Tab::ALL);
            popup.toggle(Tab::ALL);
            let (expression, shown) = if *language == "ru" {
                ("(1,5 + 2) * 4", "(1,5 + 2) * 4 = 14")
            } else {
                ("(2 + 3) * 4", "(2 + 3) * 4 = 20")
            };
            let labels = shot(popup, session, vec![Event::Text(expression.to_owned())], "calculator");
            assert!(labels.iter().any(|label| label == shown), "{labels:?}");
            popup.toggle(Tab::ALL);
            popup.toggle(Tab::ACTIONS);
            let synonym = if *language == "ru" { "настройки" } else { "preferences" };
            shot(popup, session, vec![Event::Text(synonym.to_owned())], "synonym");
            popup.toggle(Tab::ACTIONS);
            popup.toggle(Tab::ACTIONS);
            let labels = shot(popup, session, vec![Event::Text("gl".to_owned())], "abbreviation");
            assert!(labels.iter().any(|label| *label == action_title("view.gotoLine.open")), "{labels:?}");
            shot(popup, session, key(Key::Enter, egui::Modifiers::ALT), "shortcut");
            shot(popup, session, key(Key::S, egui::Modifiers::COMMAND), "shortcut-conflict");
            assert!(popup.is_recording());
            popup.toggle(Tab::ACTIONS);
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn the_popup_is_resized_by_its_edges_and_keeps_its_size() {
        let mut harness = Harness::new();
        POPUP.forget(&harness.ctx);
        let off = json!({ "preview": false });
        harness.registry.dispatch(&mut harness.session, "search.everywhere.setOptions", off).unwrap();
        harness.search(Tab::FILES, "store");
        let before = harness.list();
        let edge = pos2(before.right() + 5.0, before.center().y);
        harness.drag(edge, edge + vec2(60.0, 0.0));
        let wider = harness.list();
        assert!((wider.right() + 9.0 - (edge.x + 60.0)).abs() < 2.0, "the edge follows the pointer: {wider:?}");
        assert!((wider.center().x - before.center().x).abs() < 1.0, "it stays centred: {before:?} → {wider:?}");
        let bottom = pos2(wider.center().x, wider.bottom() + 24.0 + 5.0);
        harness.drag(bottom, bottom - vec2(0.0, 400.0));
        let remembered = POPUP.size(&harness.ctx, Vec2::ZERO, Vec2::splat(f32::MAX));
        assert_eq!(remembered, vec2(wider.width() + 16.0, ROW * 3.0), "no lower than three rows");
        harness.popup.toggle(Tab::FILES);
        harness.search(Tab::FILES, "store");
        assert_eq!(harness.list().width(), wider.width(), "the size is remembered");
        let edge = pos2(harness.list().right() + 5.0, harness.list().center().y);
        harness.drag(edge, edge + vec2(2000.0, 0.0));
        let widest = harness.list();
        let margin = super::super::popup_size::MARGIN;
        assert!(widest.left() >= margin && widest.right() <= 1000.0 - margin, "within the window: {widest:?}");
        POPUP.forget(&harness.ctx);
    }
}

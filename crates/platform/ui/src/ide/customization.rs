use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::glyphs::{self, Glyph};
use super::keymap_page;
use super::main_menu::{self, Entry};

pub(super) const FILE: &str = "customization.json";
pub(super) const SEPARATOR: &str = "-";
pub(super) const SUBMENU: &str = "submenu:";
pub(super) const SLOT: &str = "slot:";
pub(super) const SPECIAL_NEW: &str = "special:new";
pub(super) const SPECIAL_LAYOUTS: &str = "special:layouts";
pub(super) const SPECIAL_RECENT: &str = "special:recent";
pub(super) const SPECIAL_TOOL_WINDOWS: &str = "special:pluginToolWindows";
pub(super) const MAIN_TOOLBAR: &str = "mainToolbar";
pub(super) const EDITOR_POPUP: &str = "editorPopup";
pub(super) const GUTTER_POPUP: &str = "editorGutterPopup";
pub(super) const TAB_POPUP: &str = "editorTabPopup";
pub(super) const PROJECT_POPUP: &str = "projectViewPopup";
pub(super) const SCRATCH_POPUP: &str = "scratchPopup";
pub(super) const TERMINAL_TAB_POPUP: &str = "terminalTabPopup";
pub(super) const TERMINAL_POPUP: &str = "terminalPopup";
pub(super) const GIT_LOG_POPUP: &str = "gitLogPopup";
pub(super) const GIT_BRANCH_POPUP: &str = "gitBranchPopup";
pub(super) const COMMIT_FILE_POPUP: &str = "commitFilePopup";
pub(super) const CHANGELIST_POPUP: &str = "changelistPopup";
pub(super) const DEBUG_FRAME_POPUP: &str = "debugFramePopup";
pub(super) const DEBUG_VARIABLE_POPUP: &str = "debugVariablePopup";
pub(super) const DATABASE_CONNECTION_POPUP: &str = "databaseConnectionPopup";
pub(super) const DATABASE_OBJECT_POPUP: &str = "databaseObjectPopup";
pub(super) const DATA_GRID_POPUP: &str = "dataGridPopup";
pub(super) const DATABASE_RESULT_POPUP: &str = "databaseResultPopup";
pub(super) const CONFLICT_POPUP: &str = "commitConflictPopup";
pub(super) const SHELF_POPUP: &str = "shelfPopup";
pub(super) const SHELF_FILE_POPUP: &str = "shelfFilePopup";
pub(super) const DATABASE_FOLDER_POPUP: &str = "databaseFolderPopup";
pub(super) const DATABASE_AREA_POPUP: &str = "databaseNavigatorPopup";
pub(super) const DATABASE_HISTORY_POPUP: &str = "databaseHistoryPopup";
pub(super) const DATABASE_SESSIONS_POPUP: &str = "databaseSessionsPopup";
pub(super) const DATABASE_SEARCH_POPUP: &str = "databaseSearchPopup";
pub(super) const FIND_RESULTS_POPUP: &str = "findResultsPopup";
pub(super) const MARKDOWN_POPUP: &str = "markdownPreviewPopup";
pub(super) const BOOKMARKS_AREA_POPUP: &str = "bookmarksPopup";
pub(super) const BOOKMARKS_ROW_POPUP: &str = "bookmarksRowPopup";
pub(super) const TOOL_WINDOW_POPUP: &str = "toolWindowPopup";
pub(super) const WORKSPACE_TAB_POPUP: &str = "workspaceTabPopup";
pub(super) const BUILT_IN: &str = "settings-menus-item-built-in";
pub(super) const SEARCH_COLUMNS: &str = "settings-menus-item-open-columns";
pub(super) const SESSION_ENDS: &str = "settings-menus-item-end-session";
pub(super) const TOOLS: &str = "settings-menus-item-external-tools";
pub(super) const COMPARE: &str = "settings-menus-item-compare";
pub(super) const SCRATCH_RUN: &str = "settings-menus-item-run";
pub(super) const BRANCH_NEW: &str = "settings-menus-item-new-branch";
pub(super) const BRANCH_MERGE: &str = "settings-menus-item-merge";
pub(super) const EDIT_ITEMS: &str = "settings-menus-item-edit-object";
pub(super) const CREATE_ITEMS: &str = "settings-menus-item-create-object";
pub(super) const GRID_PASTE: &str = "settings-menus-item-paste";
pub(super) const BACK: &str = "navigate.history.back";
pub(super) const FORWARD: &str = "navigate.history.forward";

pub(super) struct Popup {
    pub(super) id: &'static str,
    pub(super) title: &'static str,
    pub(super) items: &'static [&'static str],
}

const fn popup_of(id: &'static str, title: &'static str, items: &'static [&'static str]) -> Popup {
    Popup { id, title, items }
}

pub(super) const POPUPS: &[Popup] = &[
    popup_of(EDITOR_POPUP, "settings-menus-editor-popup", &[TOOLS]),
    popup_of(
        GUTTER_POPUP,
        "settings-menus-gutter-popup",
        &[
            "keymap-vcs-blame-annotate",
            "-",
            "ide-gutter-bookmarks",
            "ide-gutter-remove-other-breakpoints",
            "ide-gutter-disable-other-breakpoints",
            "ide-gutter-appearance",
        ],
    ),
    popup_of(
        TAB_POPUP,
        "settings-menus-tab-popup",
        &[
            "file.document.close",
            "file.document.closeOthers",
            "file.document.closeAll",
            "file.document.closeUnmodified",
            "file.document.closeAllButPinned",
            "file.document.closeLeft",
            "file.document.closeRight",
            "file.document.closeReadOnly",
            "-",
            "ide-tab-copy-reference",
            "-",
            "editor.split.right",
            "editor.split.right/move",
            "editor.split.down",
            "editor.split.down/move",
            "editor.split.moveToOpposite",
            "editor.window.openTab",
            "editor.split.unsplit",
            "editor.split.unsplitAll",
            "-",
            "ide-tab-pin-active",
            "editor.tab.keepOpen",
            "view.settings.open/editorTabs",
            "-",
            "file.document.reopenClosed",
            "ide-tool-bookmarks",
            "-",
            "ide-tree-open-in",
            "menu-file-local-history",
        ],
    ),
    popup_of(
        PROJECT_POPUP,
        "settings-menus-project-popup",
        &[
            "ide-tree-new",
            "ide-tree-run-targets",
            "-",
            "ide-tree-cut",
            "ide-tree-copy",
            "ide-tree-copy-path",
            "ide-tree-paste",
            "-",
            "ide-tree-rename",
            "-",
            "ide-tree-reformat",
            "ide-tree-delete",
            "-",
            "ide-tree-open-right-split",
            "ide-tree-open-in",
            "ide-tool-bookmarks",
            "-",
            "menu-file-local-history",
            "ide-tree-repair-file",
            "menu-git",
            "ide-tree-reload",
            "-",
            COMPARE,
            "-",
            TOOLS,
            "-",
            "ide-tree-mark-directory-as",
            "-",
            "ide-tree-associate",
            "ide-tree-override-type",
            "ide-tree-revert-override",
            "-",
            "ide-tree-gitignore",
        ],
    ),
    popup_of(
        SCRATCH_POPUP,
        "settings-menus-scratch-popup",
        &[
            SCRATCH_RUN,
            "-",
            "keymap-view-project-changeLanguage",
            "ide-tree-rename",
            "ide-tree-delete",
            "keymap-view-project-move",
            "-",
            "ide-tree-copy-path",
            "ide-tree-open-right-split",
            "ide-tree-open-in",
        ],
    ),
    popup_of(
        TERMINAL_TAB_POPUP,
        "settings-menus-terminal-tab-popup",
        &["ide-terminal-rename", "ide-terminal-close-tab"],
    ),
    popup_of(
        TERMINAL_POPUP,
        "settings-menus-terminal-popup",
        &[
            "ide-terminal-copy",
            "ide-terminal-copy-plain",
            "ide-terminal-paste",
            "ide-terminal-select-all",
            "-",
            "ide-terminal-split-right",
            "ide-terminal-split-down",
            "-",
            "ide-terminal-new",
            "ide-terminal-rename",
            "ide-terminal-close-session",
        ],
    ),
    popup_of(
        GIT_LOG_POPUP,
        "settings-menus-git-log-popup",
        &["vcs-log-cherry-pick", "keymap-view-vcs-checkoutRevision", "keymap-view-vcs-newBranch"],
    ),
    popup_of(
        GIT_BRANCH_POPUP,
        "settings-menus-git-branch-popup",
        &[
            "vcs-branch-checkout",
            BRANCH_NEW,
            BRANCH_MERGE,
            "vcs-log-update-selected",
            "vcs-log-compare",
            "-",
            "vcs-log-favorite",
            "-",
            "vcs-branch-delete",
        ],
    ),
    popup_of(
        COMMIT_FILE_POPUP,
        "settings-menus-commit-file-popup",
        &[
            "keymap-view-vcs-showDiff",
            "keymap-view-vcs-rollback",
            "keymap-view-vcs-jumpToSource",
            "-",
            "keymap-view-vcs-moveToChangelist",
            "keymap-view-vcs-shelve",
            "-",
            "vcs-patch-create-local",
            "keymap-view-vcs-copyPatch",
        ],
    ),
    popup_of(
        CHANGELIST_POPUP,
        "settings-menus-changelist-popup",
        &[
            "vcs-changelist-set-active",
            "vcs-changelist-edit",
            "vcs-changelist-delete",
            "-",
            "keymap-view-vcs-newChangelist",
            "keymap-view-vcs-shelve",
            "-",
            "vcs-patch-create-local",
            "keymap-view-vcs-copyPatch",
        ],
    ),
    popup_of(
        CONFLICT_POPUP,
        "settings-menus-conflict-popup",
        &["vcs-commit-resolve", "-", "vcs-commit-accept-ours", "vcs-commit-accept-theirs"],
    ),
    popup_of(
        SHELF_POPUP,
        "settings-menus-shelf-popup",
        &[
            "keymap-view-vcs-unshelve",
            "vcs-shelf-rename",
            "-",
            "keymap-view-vcs-createPatch",
            "keymap-view-vcs-copyPatch",
            "-",
            "vcs-shelf-delete",
        ],
    ),
    popup_of(
        SHELF_FILE_POPUP,
        "settings-menus-shelf-file-popup",
        &["keymap-view-vcs-showDiff", "keymap-view-vcs-unshelve"],
    ),
    popup_of(DEBUG_FRAME_POPUP, "settings-menus-debug-frame-popup", &["ide-debug-reset-frame"]),
    popup_of(DEBUG_VARIABLE_POPUP, "settings-menus-debug-variable-popup", &["ide-debug-set-value"]),
    popup_of(
        DATABASE_CONNECTION_POPUP,
        "settings-menus-database-connection-popup",
        &[
            "ide-database-sql-editor",
            "ide-dbsearch-menu",
            "-",
            "ide-database-edit-connection",
            "ide-database-duplicate-connection",
            "ide-database-connect",
            "ide-database-reconnect",
            "ide-database-disconnect",
            "-",
            "ide-database-sessions",
            "ide-database-migrations",
            "ide-database-open-terminal",
            "ide-database-copy-client-command",
            "ide-database-tools",
            "-",
            "ide-database-copy-name",
            EDIT_ITEMS,
            "ide-database-refresh",
        ],
    ),
    popup_of(
        DATABASE_OBJECT_POPUP,
        "settings-menus-database-object-popup",
        &[
            "ide-database-view-data",
            "ide-database-open-object",
            "ide-database-view-diagram",
            "ide-database-open-value",
            "ide-database-sql-editor",
            CREATE_ITEMS,
            "ide-database-generate-sql",
            "ide-database-generate-code",
            "ide-database-find-usages",
            "ide-database-export-data",
            "ide-database-import-data",
            "ide-database-transfer-data",
            "ide-database-tools",
            "ide-dbsearch-menu",
            "-",
            "ide-database-copy-name",
            "ide-database-copy-qualified-name",
            EDIT_ITEMS,
            "ide-database-refresh",
        ],
    ),
    popup_of(
        DATA_GRID_POPUP,
        "settings-menus-data-grid-popup",
        &[
            "ide-database-grid-copy",
            "ide-database-grid-menu-advanced-copy",
            "ide-database-grid-copy-header",
            GRID_PASTE,
            "-",
            "ide-database-grid-menu-edit",
            "ide-database-grid-filter",
            "ide-database-grid-menu-order",
            "ide-database-grid-menu-navigate",
            "ide-database-grid-menu-view",
            "ide-database-grid-menu-panels",
            "ide-database-grid-menu-layout",
            "-",
            "ide-database-grid-generate",
            "ide-database-export",
        ],
    ),
    popup_of(DATABASE_AREA_POPUP, "settings-menus-database-area-popup", &["ide-database-new-connection"]),
    popup_of(
        DATABASE_FOLDER_POPUP,
        "settings-menus-database-folder-popup",
        &["ide-database-new-connection", "ide-database-refresh"],
    ),
    popup_of(
        DATABASE_HISTORY_POPUP,
        "settings-menus-database-history-popup",
        &["ide-dbhistory-open-console", "ide-dbhistory-run-again", "ide-dbhistory-copy"],
    ),
    popup_of(
        DATABASE_SESSIONS_POPUP,
        "settings-menus-database-sessions-popup",
        &[SESSION_ENDS, "-", "ide-dbsessions-copy-query"],
    ),
    popup_of(
        DATABASE_SEARCH_POPUP,
        "settings-menus-database-search-popup",
        &["ide-dbsearch-open-rows", "-", SEARCH_COLUMNS],
    ),
    popup_of(
        DATABASE_RESULT_POPUP,
        "settings-menus-database-result-popup",
        &["ide-database-pin", "ide-database-rename", "ide-database-close-tab"],
    ),
    popup_of(FIND_RESULTS_POPUP, "settings-menus-find-results-popup", &["ide-find-exclude"]),
    popup_of(
        MARKDOWN_POPUP,
        "settings-menus-markdown-popup",
        &["ide-markdown-copy", "ide-markdown-copy-markdown", "-", "ide-markdown-select-all"],
    ),
    popup_of(BOOKMARKS_AREA_POPUP, "settings-menus-bookmarks-popup", &["command-navigate-bookmarkList-create"]),
    popup_of(BOOKMARKS_ROW_POPUP, "settings-menus-bookmarks-row-popup", &[BUILT_IN]),
    popup_of(TOOL_WINDOW_POPUP, "settings-menus-tool-window-popup", &[BUILT_IN]),
    popup_of(WORKSPACE_TAB_POPUP, "settings-menus-workspace-tab-popup", &[BUILT_IN]),
];

pub(super) const ICONS: &[(&str, &str, Glyph)] = &[
    ("run", "settings-menus-icon-run", glyphs::glyph_run),
    ("search", "settings-menus-icon-search", glyphs::glyph_search),
    ("settings", "settings-menus-icon-settings", glyphs::glyph_settings),
    ("folder", "settings-menus-icon-folder", glyphs::glyph_folder),
    ("branch", "settings-menus-icon-branch", glyphs::glyph_branch),
    ("commit", "settings-menus-icon-commit", glyphs::glyph_commit),
    ("bulb", "settings-menus-icon-bulb", glyphs::glyph_bulb),
    ("more", "settings-menus-icon-more", glyphs::glyph_more),
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Customization {
    pub groups: BTreeMap<String, Vec<String>>,
    pub icons: BTreeMap<String, String>,
    pub from_agents: std::collections::BTreeSet<String>,
}

fn runs_something(item: &str) -> bool {
    item != SEPARATOR && !item.starts_with(SLOT) && !item.starts_with(SUBMENU) && !item.starts_with("special:")
}

pub(super) fn from_agent(action: &str) -> bool {
    current().from_agents.contains(action)
}

fn tokens_of(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Action(action) | Entry::Named(action, _) => (*action).to_owned(),
            Entry::Separator => SEPARATOR.to_owned(),
            Entry::Submenu(key, _) => format!("{SUBMENU}{key}"),
            Entry::NewItems => SPECIAL_NEW.to_owned(),
            Entry::Layouts => SPECIAL_LAYOUTS.to_owned(),
            Entry::RecentProjects => SPECIAL_RECENT.to_owned(),
            Entry::PluginToolWindows => SPECIAL_TOOL_WINDOWS.to_owned(),
        })
        .collect()
}

fn token_of_slot(item: &str) -> String {
    if item == SEPARATOR { SEPARATOR.to_owned() } else { format!("{SLOT}{item}") }
}

pub(super) fn default_items(group: &str) -> Vec<String> {
    match group {
        MAIN_TOOLBAR => vec![BACK.to_owned(), FORWARD.to_owned()],
        _ => match POPUPS.iter().find(|popup| popup.id == group) {
            Some(popup) => popup.items.iter().map(|item| token_of_slot(item)).collect(),
            None => main_menu::entries_of(group).map(tokens_of).unwrap_or_default(),
        },
    }
}

impl Customization {
    pub(super) fn path(settings: &Path) -> Option<PathBuf> {
        Some(settings.parent()?.join(FILE))
    }

    pub(super) fn load(settings: &Path) -> Self {
        Self::path(settings)
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub(super) fn save(&self, settings: &Path) -> Result<(), zen_vfs::VfsError> {
        let Some(path) = Self::path(settings) else { return Ok(()) };
        if *self == Self::default() {
            let _ = std::fs::remove_file(path);
            return Ok(());
        }
        zen_vfs::write_atomic(&path, &serde_json::to_vec_pretty(self).unwrap_or_default())
    }

    pub(super) fn items(&self, group: &str) -> Vec<String> {
        self.groups.get(group).cloned().unwrap_or_else(|| default_items(group))
    }

    fn edited(&mut self, group: &str) -> &mut Vec<String> {
        self.groups.entry(group.to_owned()).or_insert_with(|| default_items(group))
    }

    pub(super) fn insert(&mut self, group: &str, at: Option<usize>, token: &str) -> usize {
        self.from_agents.remove(token);
        let items = self.edited(group);
        let at = at.map_or(items.len(), |at| (at + 1).min(items.len()));
        items.insert(at, token.to_owned());
        at
    }

    fn add_item(&mut self, group: &str, after: Option<usize>, item: &str, by_agent: bool) {
        self.insert(group, after, item);
        if by_agent && runs_something(item) {
            self.from_agents.insert(item.to_owned());
        }
    }

    fn forget_gone_marks(&mut self) {
        let left: HashSet<String> = self.groups.values().flatten().cloned().collect();
        self.from_agents.retain(|item| left.contains(item));
    }

    pub(super) fn note_agent_items(&mut self, before: &Customization) {
        let known: HashSet<&String> = before.groups.values().flatten().collect();
        let added: Vec<String> = self
            .groups
            .iter()
            .flat_map(|(group, items)| {
                let defaults = default_items(group);
                items.iter().filter(move |item| !defaults.contains(item)).cloned().collect::<Vec<_>>()
            })
            .filter(|item| !known.contains(item) && runs_something(item))
            .collect();
        self.from_agents.extend(before.from_agents.iter().cloned());
        self.from_agents.extend(added);
    }

    pub(super) fn remove(&mut self, group: &str, index: usize) {
        let items = self.edited(group);
        if index < items.len() {
            items.remove(index);
        }
    }

    pub(super) fn shift(&mut self, group: &str, index: usize, down: bool) -> usize {
        let items = self.edited(group);
        let target = if down { index + 1 } else { index.wrapping_sub(1) };
        if index < items.len() && target < items.len() {
            items.swap(index, target);
            return target;
        }
        index
    }

    pub(super) fn restore(&mut self, group: &str) {
        self.groups.remove(group);
        self.forget_gone_marks();
    }

    pub(super) fn restore_all(&mut self) {
        *self = Self::default();
        self.from_agents.clear();
    }

    pub(super) fn set_icon(&mut self, action: &str, icon: Option<&str>) {
        match icon {
            Some(icon) => {
                self.icons.insert(action.to_owned(), icon.to_owned());
            }
            None => {
                self.icons.remove(action);
            }
        }
    }

    pub(super) fn glyph(&self, action: &str) -> Option<Glyph> {
        let icon = self.icons.get(action)?;
        ICONS.iter().find(|(id, ..)| id == icon).map(|(_, _, glyph)| *glyph)
    }
}

fn default_actions() -> &'static HashSet<String> {
    static ACTIONS: OnceLock<HashSet<String>> = OnceLock::new();
    ACTIONS.get_or_init(|| {
        zen_core::keymap::Keymap::default_macos().bindings().into_iter().map(|binding| binding.action).collect()
    })
}

static REGISTERED: OnceLock<HashSet<String>> = OnceLock::new();

pub(super) fn note_commands(registry: &Registry) {
    REGISTERED.get_or_init(|| registry.commands().map(|command| command.id.to_string()).collect());
}

pub(super) fn is_known(action: &str) -> bool {
    default_actions().contains(action) || REGISTERED.get().is_some_and(|commands| commands.contains(action))
}

pub(super) fn all_actions() -> Vec<String> {
    let mut actions: Vec<String> = default_actions()
        .iter()
        .chain(REGISTERED.get().into_iter().flatten())
        .cloned()
        .collect::<HashSet<String>>()
        .into_iter()
        .collect();
    actions.sort_unstable();
    actions
}

static CURRENT: OnceLock<RwLock<Arc<Customization>>> = OnceLock::new();

fn slot() -> &'static RwLock<Arc<Customization>> {
    CURRENT.get_or_init(RwLock::default)
}

pub(super) fn set_current(customization: &Customization) {
    if let Ok(mut current) = slot().write()
        && **current != *customization
    {
        *current = Arc::new(customization.clone());
    }
}

pub(super) fn current() -> Arc<Customization> {
    slot().read().map(|current| current.clone()).unwrap_or_default()
}

fn chosen_id() -> egui::Id {
    egui::Id::unique("customization-chosen-action")
}

pub(super) fn take_chosen(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|data| data.remove_temp::<String>(chosen_id()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Separator,
    Slot(String),
    Action(String),
}

pub(super) fn plan(custom: &Customization, group: &str, shown: impl Fn(&str) -> bool) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut separate = false;
    for item in custom.items(group) {
        let step = if item == SEPARATOR {
            separate = !steps.is_empty();
            continue;
        } else if let Some(slot) = item.strip_prefix(SLOT) {
            if !shown(slot) || !default_items(group).contains(&item) {
                continue;
            }
            Step::Slot(slot.to_owned())
        } else if is_known(&item) {
            Step::Action(item)
        } else {
            continue;
        };
        if std::mem::take(&mut separate) {
            steps.push(Step::Separator);
        }
        steps.push(step);
    }
    steps
}

pub(super) fn popup_in(
    ui: &mut egui::Ui,
    custom: &Customization,
    group: &str,
    shown: impl Fn(&str) -> bool,
    mut slot: impl FnMut(&mut egui::Ui, &str),
) {
    for step in plan(custom, group, shown) {
        match step {
            Step::Separator => {
                ui.separator();
            }
            Step::Slot(name) => slot(ui, &name),
            Step::Action(action) => {
                if ui.button(keymap_page::action_title(&action)).clicked() {
                    ui.ctx().data_mut(|data| data.insert_temp(chosen_id(), action));
                    ui.close();
                }
            }
        }
    }
}

pub(super) fn popup(
    ui: &mut egui::Ui,
    group: &str,
    shown: impl Fn(&str) -> bool,
    slot: impl FnMut(&mut egui::Ui, &str),
) {
    popup_in(ui, &current(), group, shown, slot);
}

pub(super) fn shows_anything(group: &str, shown: impl Fn(&str) -> bool) -> bool {
    !plan(&current(), group, shown).is_empty()
}

fn slot_title(slot: &str) -> String {
    if slot.contains('.') { keymap_page::action_title(slot) } else { t!(slot) }
}

pub(super) fn item_title(token: &str) -> String {
    if token == SEPARATOR {
        return t!("settings-menus-separator");
    }
    if let Some(key) = token.strip_prefix(SUBMENU) {
        return t!(key);
    }
    if let Some(slot) = token.strip_prefix(SLOT) {
        return slot_title(slot);
    }
    match token {
        SPECIAL_NEW => t!("menu-file-new"),
        SPECIAL_LAYOUTS => t!("menu-window-layouts"),
        SPECIAL_RECENT => t!("menu-file-recent-projects"),
        SPECIAL_TOOL_WINDOWS => t!("menu-view-plugin-tool-windows"),
        action => keymap_page::action_title(action),
    }
}

pub(super) fn group_title(group: &str) -> String {
    match group {
        MAIN_TOOLBAR => t!("settings-menus-main-toolbar"),
        _ => POPUPS.iter().find(|popup| popup.id == group).map_or_else(|| t!(group), |popup| t!(popup.title)),
    }
}

pub(super) fn missing_defaults(custom: &Customization, group: &str) -> Vec<String> {
    let items = custom.items(group);
    default_items(group).into_iter().filter(|token| token.contains(':') && !items.contains(token)).collect()
}

fn menu_groups(entries: &[Entry], out: &mut Vec<String>) {
    for entry in entries {
        if let Entry::Submenu(key, inner) = entry {
            out.push((*key).to_owned());
            menu_groups(inner, out);
        }
    }
}

pub(super) fn all_groups() -> Vec<String> {
    let mut groups = Vec::new();
    for (menu, entries) in main_menu::MENUS {
        groups.push((*menu).to_owned());
        menu_groups(entries, &mut groups);
    }
    groups.push(MAIN_TOOLBAR.to_owned());
    groups.extend(POPUPS.iter().map(|popup| popup.id.to_owned()));
    groups
}

#[derive(Default)]
pub(super) struct Changed(pub(super) Option<Customization>);

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ListCustomizationParams {
    /// One group's id, as `groups` in this command's answer gives it; left out, every group.
    #[serde(default)]
    pub(super) group: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct AddItemParams {
    /// The group's id, as ui.customization.list gives it.
    pub(super) group: String,
    /// An action or command id, `-` for a separator, or a built-in item from the group's `missing` list.
    pub(super) item: String,
    /// The index to add after; left out, the end of the group.
    #[serde(default)]
    pub(super) after: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct RemoveItemParams {
    /// The group's id, as ui.customization.list gives it.
    pub(super) group: String,
    /// The item's index in the group.
    pub(super) index: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct MoveItemParams {
    /// The group's id, as ui.customization.list gives it.
    pub(super) group: String,
    /// The item's index in the group.
    pub(super) index: usize,
    /// True moves it one place down, false one place up.
    pub(super) down: bool,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct RestoreParams {
    /// The group to bring back to its defaults; left out, every group and every icon.
    #[serde(default)]
    pub(super) group: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SetIconParams {
    /// The action or command id.
    pub(super) action: String,
    /// One of run, search, settings, folder, branch, commit, bulb, more; left out, no icon.
    #[serde(default)]
    pub(super) icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CustomizedItem {
    pub(super) id: String,
    pub(super) name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CustomizedGroup {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) customized: bool,
    pub(super) items: Vec<CustomizedItem>,
    pub(super) missing: Vec<CustomizedItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CustomizationList {
    pub(super) groups: Vec<CustomizedGroup>,
    pub(super) icons: BTreeMap<String, String>,
}

fn described(token: String) -> CustomizedItem {
    CustomizedItem { name: item_title(&token), id: token }
}

fn group_info(custom: &Customization, group: &str) -> CustomizedGroup {
    CustomizedGroup {
        id: group.to_owned(),
        name: group_title(group),
        customized: custom.groups.contains_key(group),
        items: custom.items(group).into_iter().map(described).collect(),
        missing: missing_defaults(custom, group).into_iter().map(described).collect(),
    }
}

fn known_group(group: &str) -> Result<(), CommandError> {
    if all_groups().iter().any(|known| known == group) {
        return Ok(());
    }
    Err(CommandError::with_code("unknownGroup", format!("no group `{group}`; ui.customization.list names them")))
}

fn fits(group: &str, item: &str) -> bool {
    item == SEPARATOR || default_items(group).iter().any(|token| token == item) || is_known(item)
}

fn in_group(custom: &Customization, group: &str, index: usize) -> Result<(), CommandError> {
    let left = custom.items(group).len();
    if index < left {
        return Ok(());
    }
    Err(CommandError::with_code("noSuchItem", format!("`{group}` has {left} items, there is no index {index}")))
}

fn changed(session: &mut Session, custom: Customization, group: &str) -> CustomizedGroup {
    let info = group_info(&custom, group);
    set_current(&custom);
    session.state_mut::<Changed>().0 = Some(custom);
    info
}

pub(super) struct ListCustomization;

impl Command for ListCustomization {
    type Params = ListCustomizationParams;
    type Output = CustomizationList;
    const ID: &'static str = "ui.customization.list";
    const TITLE: &'static str = "List Menus and Toolbars";
    const DESCRIPTION: &'static str = "Lists the groups of Menus and Toolbars: for each its id, name, whether it is \
         customised, its items (id and name) and the built-in items taken out of it (missing), with the icons chosen \
         for actions.";

    fn run(&self, _: &mut Session, params: ListCustomizationParams) -> Result<CustomizationList, CommandError> {
        let custom = current();
        let groups = match params.group {
            Some(group) => {
                known_group(&group)?;
                vec![group]
            }
            None => all_groups(),
        };
        let groups = groups.iter().map(|group| group_info(&custom, group)).collect();
        Ok(CustomizationList { groups, icons: custom.icons.clone() })
    }
}

pub(super) struct AddCustomizationItem;

impl Command for AddCustomizationItem {
    type Params = AddItemParams;
    type Output = CustomizedGroup;
    const ID: &'static str = "ui.customization.add";
    const TITLE: &'static str = "Add to Menu or Toolbar";
    const DESCRIPTION: &'static str = "Adds an action, a command, a separator or a built-in item taken out earlier to \
         a group of Menus and Toolbars and saves it; returns the group. What an agent adds runs as the agent's when \
         the person clicks it, so a command agents may not run stays refused.";

    fn run(&self, session: &mut Session, params: AddItemParams) -> Result<CustomizedGroup, CommandError> {
        known_group(&params.group)?;
        if !fits(&params.group, &params.item) {
            return Err(CommandError::with_code("unknownItem", format!("no action or item `{}`", params.item)));
        }
        let mut custom = (*current()).clone();
        custom.add_item(&params.group, params.after, &params.item, zen_core::macros::by_agent(session));
        Ok(changed(session, custom, &params.group))
    }
}

pub(super) struct RemoveCustomizationItem;

impl Command for RemoveCustomizationItem {
    type Params = RemoveItemParams;
    type Output = CustomizedGroup;
    const ID: &'static str = "ui.customization.remove";
    const TITLE: &'static str = "Remove from Menu or Toolbar";
    const DESCRIPTION: &'static str =
        "Removes the item at an index from a group of Menus and Toolbars and saves it; returns the group.";

    fn run(&self, session: &mut Session, params: RemoveItemParams) -> Result<CustomizedGroup, CommandError> {
        known_group(&params.group)?;
        let mut custom = (*current()).clone();
        in_group(&custom, &params.group, params.index)?;
        custom.remove(&params.group, params.index);
        Ok(changed(session, custom, &params.group))
    }
}

pub(super) struct MoveCustomizationItem;

impl Command for MoveCustomizationItem {
    type Params = MoveItemParams;
    type Output = CustomizedGroup;
    const ID: &'static str = "ui.customization.move";
    const TITLE: &'static str = "Move in Menu or Toolbar";
    const DESCRIPTION: &'static str = "Moves the item at an index one place up or down in a group of Menus and Toolbars and saves it; returns the group.";

    fn run(&self, session: &mut Session, params: MoveItemParams) -> Result<CustomizedGroup, CommandError> {
        known_group(&params.group)?;
        let mut custom = (*current()).clone();
        in_group(&custom, &params.group, params.index)?;
        custom.shift(&params.group, params.index, params.down);
        Ok(changed(session, custom, &params.group))
    }
}

pub(super) struct RestoreCustomization;

impl Command for RestoreCustomization {
    type Params = RestoreParams;
    type Output = CustomizationList;
    const ID: &'static str = "ui.customization.restore";
    const TITLE: &'static str = "Restore Menus and Toolbars";
    const DESCRIPTION: &'static str = "Brings a group of Menus and Toolbars back to its defaults, or, with no group, \
         every group and every icon, and saves it; returns the groups restored.";

    fn run(&self, session: &mut Session, params: RestoreParams) -> Result<CustomizationList, CommandError> {
        let mut custom = (*current()).clone();
        let groups = match params.group {
            Some(group) => {
                known_group(&group)?;
                custom.restore(&group);
                vec![group]
            }
            None => {
                custom.restore_all();
                all_groups()
            }
        };
        let groups = groups.iter().map(|group| group_info(&custom, group)).collect();
        let icons = custom.icons.clone();
        set_current(&custom);
        session.state_mut::<Changed>().0 = Some(custom);
        Ok(CustomizationList { groups, icons })
    }
}

pub(super) struct SetCustomizationIcon;

impl Command for SetCustomizationIcon {
    type Params = SetIconParams;
    type Output = BTreeMap<String, String>;
    const ID: &'static str = "ui.customization.setIcon";
    const TITLE: &'static str = "Set Toolbar Icon";
    const DESCRIPTION: &'static str = "Gives an action one of the built-in icons, or none, and saves it; returns the \
         icons chosen so far.";

    fn run(&self, session: &mut Session, params: SetIconParams) -> Result<Self::Output, CommandError> {
        if let Some(icon) = params.icon.as_deref()
            && !ICONS.iter().any(|(id, ..)| *id == icon)
        {
            return Err(CommandError::with_code("unknownIcon", format!("no icon `{icon}`")));
        }
        let mut custom = (*current()).clone();
        custom.set_icon(&params.action, params.icon.as_deref());
        let icons = custom.icons.clone();
        set_current(&custom);
        session.state_mut::<Changed>().0 = Some(custom);
        Ok(icons)
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListCustomization)?;
    registry.register(AddCustomizationItem)?;
    registry.register(RemoveCustomizationItem)?;
    registry.register(MoveCustomizationItem)?;
    registry.register(RestoreCustomization)?;
    registry.register(SetCustomizationIcon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_starts_from_its_defaults_and_restore_brings_them_back() {
        let mut custom = Customization::default();
        assert_eq!(custom.items(MAIN_TOOLBAR), [BACK, FORWARD]);
        let at = custom.insert(MAIN_TOOLBAR, Some(0), "editor.code.reformat");
        assert_eq!(at, 1);
        assert_eq!(custom.items(MAIN_TOOLBAR), [BACK, "editor.code.reformat", FORWARD]);
        assert_eq!(custom.shift(MAIN_TOOLBAR, 1, true), 2);
        assert_eq!(custom.items(MAIN_TOOLBAR), [BACK, FORWARD, "editor.code.reformat"]);
        assert_eq!(custom.shift(MAIN_TOOLBAR, 2, true), 2, "the last stays");
        assert_eq!(custom.shift(MAIN_TOOLBAR, 0, false), 0, "the first stays");
        custom.remove(MAIN_TOOLBAR, 0);
        assert_eq!(custom.items(MAIN_TOOLBAR), [FORWARD, "editor.code.reformat"]);
        custom.restore(MAIN_TOOLBAR);
        assert_eq!(custom, Customization::default());
    }

    #[test]
    fn adding_with_nothing_selected_goes_to_the_end() {
        let mut custom = Customization::default();
        custom.insert(EDITOR_POPUP, None, "editor.code.reformat");
        custom.insert(EDITOR_POPUP, None, SEPARATOR);
        let items = custom.items(EDITOR_POPUP);
        assert_eq!(items[1..], ["editor.code.reformat".to_owned(), SEPARATOR.to_owned()]);
    }

    #[test]
    fn a_menu_lists_its_entries_as_tokens() {
        let items = default_items("menu-file");
        assert!(items.contains(&"submenu:menu-file-new".to_owned()), "{items:?}");
        assert!(items.contains(&"file.document.saveAll".to_owned()));
        assert!(items.contains(&SEPARATOR.to_owned()));
        assert_eq!(default_items("menu-file-new").last().map(String::as_str), Some(SPECIAL_NEW));
        assert!(default_items("nothing").is_empty());
    }

    #[test]
    fn a_popup_lists_its_built_in_items_and_they_move_and_go() {
        let mut custom = Customization::default();
        let first = format!("{SLOT}ide-terminal-rename");
        assert_eq!(custom.items(TERMINAL_TAB_POPUP)[0], first);
        custom.shift(TERMINAL_TAB_POPUP, 0, true);
        assert_eq!(custom.items(TERMINAL_TAB_POPUP)[1], first);
        assert_eq!(
            plan(&custom, TERMINAL_TAB_POPUP, |_| true),
            [Step::Slot("ide-terminal-close-tab".to_owned()), Step::Slot("ide-terminal-rename".to_owned())]
        );
        custom.remove(TERMINAL_TAB_POPUP, 1);
        assert_eq!(plan(&custom, TERMINAL_TAB_POPUP, |_| true).len(), 1);
        assert_eq!(missing_defaults(&custom, TERMINAL_TAB_POPUP), std::slice::from_ref(&first));
        custom.insert(TERMINAL_TAB_POPUP, None, &first);
        assert!(missing_defaults(&custom, TERMINAL_TAB_POPUP).is_empty());
    }

    #[test]
    fn a_separator_shows_only_between_items_that_are_shown() {
        let custom = Customization::default();
        let only_new = plan(&custom, TERMINAL_POPUP, |slot| slot == "ide-terminal-new");
        assert_eq!(only_new, [Step::Slot("ide-terminal-new".to_owned())], "no separator before the first item");
        let around = plan(&custom, TERMINAL_POPUP, |slot| slot == "ide-terminal-new" || slot.ends_with("-right"));
        assert_eq!(around.len(), 3);
        assert_eq!(around[1], Step::Separator);
        let mut trailing = Customization::default();
        trailing.groups.insert(
            TERMINAL_POPUP.to_owned(),
            vec![SEPARATOR.to_owned(), format!("{SLOT}ide-terminal-new"), SEPARATOR.to_owned(), SEPARATOR.to_owned()],
        );
        assert_eq!(plan(&trailing, TERMINAL_POPUP, |_| true).len(), 1, "none at either end");
    }

    #[test]
    fn an_unknown_item_of_a_popup_is_skipped_and_stays() {
        let mut custom = Customization::default();
        let (stale, foreign) = (format!("{SLOT}gone"), format!("{SLOT}ide-tree-cut"));
        let items = vec![stale, foreign, "gone.action".to_owned(), format!("{SLOT}ide-terminal-rename")];
        custom.groups.insert(TERMINAL_TAB_POPUP.to_owned(), items.clone());
        assert_eq!(plan(&custom, TERMINAL_TAB_POPUP, |_| true), [Step::Slot("ide-terminal-rename".to_owned())]);
        assert_eq!(custom.items(TERMINAL_TAB_POPUP), items);
    }

    #[test]
    fn every_popup_draws_every_group_by_a_distinct_id() {
        let groups = all_groups();
        let distinct: HashSet<&String> = groups.iter().collect();
        assert_eq!(distinct.len(), groups.len());
        for popup in POPUPS {
            assert!(!default_items(popup.id).is_empty(), "{}", popup.id);
            assert!(groups.iter().any(|group| group == popup.id));
        }
    }

    #[test]
    fn a_popup_draws_its_list_in_order() {
        use egui::accesskit::Role;
        let mut custom = Customization::default();
        custom.shift(TERMINAL_TAB_POPUP, 0, true);
        let draw = |ui: &mut egui::Ui| {
            let mut order = Vec::new();
            popup_in(
                ui,
                &custom,
                TERMINAL_TAB_POPUP,
                |_| true,
                |ui, slot| {
                    order.push(slot.to_owned());
                    let _ = ui.button(slot);
                },
            );
            assert_eq!(order, ["ide-terminal-close-tab", "ide-terminal-rename"]);
        };
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(300.0, 200.0), 1.0);
        let tree = screen.accessibility_tree(draw);
        assert!(crate::accessibility::testing::find(&tree, Role::Button, "ide-terminal-rename").is_some());
    }

    #[test]
    fn customizations_live_in_a_file_next_to_the_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let mut custom = Customization::default();
        custom.insert(MAIN_TOOLBAR, None, "editor.code.reformat");
        custom.set_icon("editor.code.reformat", Some("run"));
        custom.save(&settings).unwrap();
        assert!(dir.path().join(FILE).is_file());
        assert_eq!(Customization::load(&settings), custom);
        assert!(custom.glyph("editor.code.reformat").is_some());
        Customization::default().save(&settings).unwrap();
        assert!(!dir.path().join(FILE).exists(), "nothing to keep, no file");
        assert_eq!(Customization::load(&settings), Customization::default());
    }

    #[test]
    fn what_an_agent_adds_is_marked_until_the_person_adds_it_too() {
        let mut custom = Customization::default();
        custom.add_item(MAIN_TOOLBAR, None, "project.trust.grant", true);
        custom.add_item(MAIN_TOOLBAR, None, SEPARATOR, true);
        assert_eq!(custom.from_agents.iter().map(String::as_str).collect::<Vec<_>>(), ["project.trust.grant"]);
        custom.add_item(MAIN_TOOLBAR, None, "project.trust.grant", false);
        assert!(custom.from_agents.is_empty(), "the person's own choice");
        custom.add_item(MAIN_TOOLBAR, None, "project.trust.grant", true);
        custom.restore(MAIN_TOOLBAR);
        assert_eq!(custom, Customization::default(), "restore takes the mark too");
    }

    #[test]
    fn a_whole_list_an_agent_sets_is_marked_and_built_in_items_are_not() {
        let before = Customization::default();
        let mut custom = before.clone();
        let items = vec![BACK.to_owned(), FORWARD.to_owned(), "project.trust.grant".to_owned(), SEPARATOR.to_owned()];
        custom.groups.insert(MAIN_TOOLBAR.to_owned(), items);
        custom.note_agent_items(&before);
        assert_eq!(custom.from_agents.iter().map(String::as_str).collect::<Vec<_>>(), ["project.trust.grant"]);
    }

    #[test]
    fn a_damaged_file_reads_as_no_customization() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{").unwrap();
        assert_eq!(Customization::load(&dir.path().join("settings.json")), Customization::default());
    }
}

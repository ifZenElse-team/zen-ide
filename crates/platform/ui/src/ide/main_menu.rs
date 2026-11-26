#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) enum Entry {
    Action(&'static str),
    Named(&'static str, &'static str),
    Separator,
    Submenu(&'static str, &'static [Entry]),
    NewItems,
    RecentProjects,
    Layouts,
    PluginToolWindows,
}

use Entry::{Action, Layouts, Named, NewItems, PluginToolWindows, RecentProjects, Separator, Submenu};

pub(super) const RECENT: &str = "recent:";

const RECENT_LISTED: usize = 20;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn recent_items(recent: &[zen_workbench::recent::RecentProject]) -> Vec<(String, String)> {
    recent
        .iter()
        .take(RECENT_LISTED)
        .map(|project| {
            let path = project.path.to_string_lossy();
            let name =
                project.path.file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().into_owned());
            (format!("{RECENT}{path}"), format!("{name}  {}", super::home_relative(&project.path)))
        })
        .collect()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) const LAYOUT_COMMANDS: [&str; 4] =
    ["view.layout.apply", "view.layout.saveChanges", "view.layout.rename", "view.layout.delete"];

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn layout_action(command: &str, name: &str) -> String {
    format!("{command}:{name}")
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn layout_item(action: &str) -> Option<(&'static str, &str)> {
    LAYOUT_COMMANDS.iter().find_map(|command| Some((*command, action.strip_prefix(command)?.strip_prefix(':')?)))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) const MENUS: &[(&str, &[Entry])] = &[
    (
        "menu-file",
        &[
            Submenu(
                "menu-file-new",
                &[
                    Action("view.newProject.open"),
                    Action("view.project.fromSources"),
                    Action("view.project.fromVcs"),
                    Separator,
                    Action("view.module.new"),
                    Action("view.module.fromSources"),
                    Separator,
                    NewItems,
                ],
            ),
            Action("view.project.open"),
            Submenu("menu-file-recent-projects", &[RecentProjects]),
            Action("view.project.close"),
            Action("view.project.closeAll"),
            Action("view.project.closeOthers"),
            Separator,
            Action("view.projectStructure.open"),
            Action("view.plugins.open"),
            Submenu("menu-file-new-projects-setup", &[Action("view.settings.newProjects")]),
            Submenu(
                "menu-file-file-properties",
                &[
                    Action("view.fileEncoding.open"),
                    Action("file.bom.remove"),
                    Action("file.bom.add"),
                    Action("view.associateFileType.open"),
                    Action("file.document.toggleReadOnly"),
                    Submenu(
                        "menu-file-line-separators",
                        &[
                            Action("file.lineSeparator.set/crlf"),
                            Action("file.lineSeparator.set/lf"),
                            Action("file.lineSeparator.set/cr"),
                        ],
                    ),
                ],
            ),
            Separator,
            Submenu(
                "menu-file-local-history",
                &[Action("view.localHistory.show"), Action("view.localHistory.putLabel")],
            ),
            Separator,
            Action("file.document.saveAll"),
            Action("file.document.syncWithDisk"),
            Action("view.repairIde.start"),
            Action("view.invalidateCaches.open"),
            Separator,
            Submenu(
                "menu-file-manage-ide-settings",
                &[
                    Action("view.ideSettings.import"),
                    Action("view.ideSettings.export"),
                    Action("view.ideSettings.restoreDefaults"),
                ],
            ),
            Action("view.fileTemplates.saveAs"),
            Separator,
            Action("view.powerSaveMode.toggle"),
        ],
    ),
    (
        "menu-edit",
        &[
            Action("editor.edit.undo"),
            Action("editor.edit.redo"),
            Separator,
            Submenu("menu-edit-paste-special", &[Action("view.pasteFromHistory.open")]),
            Separator,
            Action("editor.selection.columnMode"),
            Action("editor.selection.selectAll"),
            Action("editor.selection.extend"),
            Action("editor.selection.shrink"),
            Separator,
            Submenu(
                "menu-edit-find",
                &[
                    Action("view.find.open"),
                    Action("view.replace.open"),
                    Action("editor.find.next"),
                    Action("editor.find.previous"),
                    Action("editor.selection.selectAllOccurrences"),
                    Action("editor.selection.addNextOccurrence"),
                    Action("editor.selection.unselectOccurrence"),
                    Action("editor.find.searchInSelection"),
                    Action("editor.find.scrollToResults"),
                    Action("editor.find.nextWordAtCaret"),
                    Action("editor.find.previousWordAtCaret"),
                    Separator,
                    Action("view.findInFiles.open"),
                    Action("view.replaceInFiles.open"),
                ],
            ),
            Submenu(
                "menu-edit-find-usages",
                &[
                    Action("navigate.usages.find"),
                    Action("navigate.usages.settings"),
                    Action("navigate.usages.show"),
                    Action("navigate.usages.findInFile"),
                    Action("editor.highlightUsages.toggle"),
                    Action("editor.highlightUsages.next"),
                    Action("editor.highlightUsages.previous"),
                    Separator,
                    Action("view.recentFindUsages.open"),
                ],
            ),
            Submenu(
                "menu-edit-carets",
                &[
                    Action("editor.selection.addNextOccurrence"),
                    Action("editor.selection.unselectOccurrence"),
                    Action("editor.selection.selectAllOccurrences"),
                    Action("editor.caret.cloneAbove"),
                    Action("editor.caret.cloneBelow"),
                    Action("editor.caret.keepPrimary"),
                ],
            ),
            Separator,
            Action("editor.line.duplicate"),
            Action("editor.line.delete"),
            Separator,
            Submenu(
                "menu-edit-bookmarks",
                &[
                    Action("navigate.bookmark.toggle"),
                    Action("view.bookmark.mnemonic"),
                    Action("navigate.bookmark.previous"),
                    Action("navigate.bookmark.next"),
                    Action("view.bookmark.show"),
                    Action("view.bookmark.goToMnemonic"),
                ],
            ),
            Submenu("menu-edit-macros", MACROS),
        ],
    ),
    (
        "menu-view",
        &[
            Submenu(
                "menu-view-tool-windows",
                &[
                    Action("view.toolWindow.project"),
                    Action("view.toolWindow.commit"),
                    Action("view.toolWindow.bookmarks"),
                    Action("view.toolWindow.structure"),
                    Action("view.toolWindow.find"),
                    Action("view.toolWindow.git"),
                    Action("view.toolWindow.run"),
                    Action("view.toolWindow.debug"),
                    Action("view.toolWindow.problems"),
                    Action("view.toolWindow.terminal"),
                    Action("view.toolWindow.services"),
                    Action("view.toolWindow.hierarchy"),
                    Action("view.toolWindow.coverage"),
                    Action("view.toolWindow.notifications"),
                    Action("view.toolWindow.todo"),
                    Action("view.toolWindow.caffeine"),
                    Action("view.toolWindow.maven"),
                    Action("view.toolWindow.gradle"),
                    Action("view.toolWindow.build"),
                    Action("view.toolWindow.ant"),
                    Action("view.toolWindow.npm"),
                    Action("view.toolWindow.endpoints"),
                    Action("view.toolWindow.dependencies"),
                    PluginToolWindows,
                ],
            ),
            Submenu(
                "menu-view-appearance",
                &[
                    Action("view.presentationMode.toggle"),
                    Action("view.distractionFreeMode.toggle"),
                    Action("view.fullScreen.toggle"),
                    Action("view.zenMode.toggle"),
                    Action("view.ideZoom.open"),
                    Separator,
                    Submenu(
                        "menu-view-navigation-bar",
                        &[
                            Named("view.navigationBar.top", "menu-view-navigation-bar-top"),
                            Named("view.navigationBar.statusBar", "menu-view-navigation-bar-status-bar"),
                            Named("view.navigationBar.hide", "menu-view-navigation-bar-hide"),
                            Separator,
                            Named("view.navigationBar.members.toggle", "menu-view-navigation-bar-members"),
                        ],
                    ),
                    Submenu(
                        "menu-view-status-bar-widgets",
                        &[
                            Action("view.statusBarWidgets.open/lineColumn"),
                            Action("view.statusBarWidgets.open/languageServices"),
                            Action("view.statusBarWidgets.open/lineSeparator"),
                            Action("view.statusBarWidgets.open/fileEncoding"),
                            Action("view.statusBarWidgets.open/powerSaveMode"),
                            Action("view.statusBarWidgets.open/indentation"),
                            Action("view.statusBarWidgets.open/mcpServer"),
                            Action("view.statusBarWidgets.open/readOnly"),
                            Action("view.statusBarWidgets.open/memoryIndicator"),
                        ],
                    ),
                    Separator,
                    Action("view.codeVision.authors.toggle"),
                ],
            ),
            Submenu(
                "menu-view-active-editor",
                &[
                    Action("editor.softWrap.toggle"),
                    Action("editor.whitespaces.toggle"),
                    Action("editor.lineNumbers.toggle"),
                    Action("editor.indentGuides.toggle"),
                    Separator,
                    Action("editor.fontSize.increase"),
                    Action("editor.fontSize.decrease"),
                    Action("editor.fontSize.reset"),
                ],
            ),
            Action("view.editorFontSize.increase"),
            Action("view.editorFontSize.decrease"),
            Action("view.editorFontSize.reset"),
            Separator,
            Action("editor.documentation.show"),
            Action("editor.parameterInfo.show"),
            Action("java.bytecode.show"),
            Separator,
            Action("view.recentFiles.open"),
            Action("view.recentLocations.open"),
            Separator,
            Action("view.project.compareWith"),
            Action("view.compare.clipboard"),
            Separator,
            Action("view.quickSwitchScheme.open"),
        ],
    ),
    (
        "menu-navigate",
        &[
            Action("navigate.history.back"),
            Action("navigate.history.forward"),
            Separator,
            Action("view.searchEverywhere.open"),
            Separator,
            Named("view.gotoClass.open", "menu-navigate-class"),
            Named("view.gotoFile.open", "menu-navigate-file"),
            Named("view.gotoSymbol.open", "menu-navigate-symbol"),
            Named("view.gotoText.open", "menu-navigate-text"),
            Named("view.gotoLine.open", "menu-navigate-line"),
            Separator,
            Action("navigate.edit.last"),
            Action("navigate.edit.next"),
            Action("view.selectIn.open"),
            Action("view.navigationBar.jump"),
            Separator,
            Named("navigate.declaration.goTo", "menu-navigate-declaration"),
            Named("navigate.implementation.goTo", "menu-navigate-implementation"),
            Named("navigate.typeDeclaration.goTo", "menu-navigate-type-declaration"),
            Named("navigate.superMethod.goTo", "menu-navigate-super-method"),
            Named("navigate.test.goTo", "menu-navigate-test"),
            Separator,
            Action("view.fileStructure.open"),
            Action("navigate.hierarchy.type"),
            Action("navigate.hierarchy.call"),
            Action("view.customFolding.open"),
        ],
    ),
    (
        "menu-code",
        &[
            Submenu(
                "menu-code-completion",
                &[
                    Named("editor.completion.start", "menu-code-completion-basic"),
                    Named("editor.completion.smart", "menu-code-completion-smart"),
                    Action("editor.statement.complete"),
                    Action("editor.completion.expandWord"),
                    Action("editor.completion.expandWord/forward"),
                ],
            ),
            Separator,
            Action("view.overrideMethods.open"),
            Action("view.implementMethods.open"),
            Action("view.generate.open"),
            Separator,
            Action("view.inspectCode.open"),
            Action("view.codeCleanup.open"),
            Submenu("menu-code-analyze", &[Action("view.silentCleanup.run"), Action("view.inspectByName.open")]),
            Separator,
            Action("editor.templates.insert"),
            Action("view.surround.open"),
            Action("view.unwrap.open"),
            Separator,
            Action("editor.comment.line"),
            Action("editor.comment.block"),
            Separator,
            Action("editor.line.indent"),
            Action("editor.line.outdent"),
            Separator,
            Action("editor.line.moveUp"),
            Action("editor.line.moveDown"),
            Separator,
            Submenu(
                "menu-code-folding",
                &[
                    Action("editor.fold.expand"),
                    Action("editor.fold.expandRecursively"),
                    Action("editor.fold.expandAll"),
                    Action("editor.fold.collapse"),
                    Action("editor.fold.collapseRecursively"),
                    Action("editor.fold.collapseAll"),
                    Submenu(
                        "menu-code-folding-expand-to-level",
                        &[
                            Named("editor.fold.expandToLevel/1", "menu-code-folding-level-1"),
                            Named("editor.fold.expandToLevel/2", "menu-code-folding-level-2"),
                            Named("editor.fold.expandToLevel/3", "menu-code-folding-level-3"),
                            Named("editor.fold.expandToLevel/4", "menu-code-folding-level-4"),
                            Named("editor.fold.expandToLevel/5", "menu-code-folding-level-5"),
                        ],
                    ),
                    Submenu(
                        "menu-code-folding-expand-all-to-level",
                        &[
                            Named("editor.fold.expandAllToLevel/1", "menu-code-folding-level-1"),
                            Named("editor.fold.expandAllToLevel/2", "menu-code-folding-level-2"),
                            Named("editor.fold.expandAllToLevel/3", "menu-code-folding-level-3"),
                            Named("editor.fold.expandAllToLevel/4", "menu-code-folding-level-4"),
                            Named("editor.fold.expandAllToLevel/5", "menu-code-folding-level-5"),
                        ],
                    ),
                    Action("editor.fold.expandDocComments"),
                    Action("editor.fold.collapseDocComments"),
                    Action("editor.fold.toggleAtCaret"),
                    Action("editor.fold.selection"),
                    Action("editor.fold.collapseBlock"),
                ],
            ),
            Separator,
            Action("file.convert.toLanguage/kotlin"),
            Separator,
            Action("editor.code.reformat"),
            Action("editor.imports.optimize"),
            Action("editor.code.rearrange"),
        ],
    ),
    (
        "menu-refactor",
        &[
            Action("view.rename.open"),
            Action("view.changeSignature.open"),
            Separator,
            Submenu(
                "menu-refactor-extract",
                &[
                    Named("view.extract.variable", "menu-refactor-extract-variable"),
                    Named("view.extract.constant", "menu-refactor-extract-constant"),
                    Named("view.extract.field", "menu-refactor-extract-field"),
                    Named("view.extract.parameter", "menu-refactor-extract-parameter"),
                    Named("view.extract.parameterObject", "menu-refactor-extract-parameter-object"),
                    Separator,
                    Named("view.extract.method", "menu-refactor-extract-method"),
                    Separator,
                    Named("view.extract.interface", "menu-refactor-extract-interface"),
                    Named("view.extract.superclass", "menu-refactor-extract-superclass"),
                ],
            ),
            Action("view.inline.open"),
            Separator,
            Action("view.pullMembersUp.open"),
            Action("view.pushMembersDown.open"),
            Action("view.encapsulateFields.open"),
            Separator,
            Action("view.project.move"),
            Action("view.safeDelete.open"),
        ],
    ),
    ("menu-build", &[Action("view.artifacts.build")]),
    (
        "menu-run",
        &[
            Action("run.process.start"),
            Action("debug.session.start"),
            Named("run.process.start/context", "menu-run-context"),
            Named("debug.session.start/context", "menu-debug-context"),
            Action("view.runConfigurations.choose"),
            Action("view.runConfigurations.choose/debug"),
            Action("coverage.run.start"),
            Separator,
            Action("view.runConfigurations.open"),
            Separator,
            Action("run.process.stop"),
            Separator,
            Action("debug.execution.stepOver"),
            Action("debug.execution.stepInto"),
            Action("debug.execution.stepOut"),
            Action("debug.execution.resume"),
            Action("debug.execution.pause"),
            Action("debug.execution.runToCursor"),
            Action("debug.classes.reload"),
            Submenu(
                "menu-run-debugging-actions",
                &[
                    Action("debug.execution.forceStepOver"),
                    Action("debug.execution.forceStepInto"),
                    Action("view.smartStepInto.open"),
                    Action("debug.execution.forceRunToCursor"),
                    Action("debug.frame.reset"),
                    Action("view.streamTrace.open"),
                    Separator,
                    Action("view.evaluateExpression.open"),
                    Action("view.quickEvaluate.open"),
                    Action("debug.executionPoint.show"),
                ],
            ),
            Submenu(
                "menu-run-toggle-breakpoint",
                &[
                    Action("debug.breakpoint.toggle"),
                    Action("debug.breakpoint.toggleTemporary"),
                    Action("debug.breakpoint.toggleEnabled"),
                ],
            ),
            Action("view.breakpoints.open"),
            Separator,
            Action("view.testHistory.open"),
        ],
    ),
    (
        "menu-tools",
        &[Action("view.commandLineLauncher.open"), Separator, Action("java.jshell.open"), Action("kotlin.repl.open")],
    ),
    (
        "menu-git",
        &[
            Action("view.vcs.commit"),
            Action("view.vcs.push"),
            Action("view.vcs.update"),
            Action("vcs.remote.fetch"),
            Separator,
            Action("view.vcs.branches"),
            Action("view.vcs.newBranch"),
            Separator,
            Action("vcs.workspace.open"),
            Separator,
            Action("view.vcs.showDiff"),
            Action("vcs.blame.annotate"),
            Submenu(
                "menu-git-uncommitted",
                &[
                    Action("view.vcs.shelve"),
                    Action("view.vcs.showShelf"),
                    Action("view.vcs.unshelve"),
                    Separator,
                    Action("view.vcs.newChangelist"),
                    Action("view.vcs.moveToChangelist"),
                ],
            ),
            Submenu(
                "menu-git-patch",
                &[
                    Action("view.vcs.createPatch"),
                    Action("view.vcs.applyPatch"),
                    Action("view.vcs.applyPatchFromClipboard"),
                ],
            ),
            Action("view.vcs.showHistory"),
            Action("view.vcs.rollback"),
        ],
    ),
    (
        "menu-window",
        &[
            Submenu("menu-window-layouts", &[Layouts, Separator, Action("view.layout.saveAsNew")]),
            Submenu(
                "menu-window-active-tool-window",
                &[
                    Action("view.toolWindow.hide"),
                    Action("view.toolWindow.hideAll"),
                    Separator,
                    Submenu(
                        "menu-window-view-mode",
                        &[
                            Action("view.toolWindow.viewMode/dockPinned"),
                            Action("view.toolWindow.viewMode/dockUnpinned"),
                            Action("view.toolWindow.viewMode/undock"),
                            Action("view.toolWindow.viewMode/float"),
                            Action("view.toolWindow.viewMode/window"),
                        ],
                    ),
                    Submenu(
                        "menu-window-resize",
                        &[
                            Action("view.toolWindow.resize/left"),
                            Action("view.toolWindow.resize/right"),
                            Action("view.toolWindow.resize/up"),
                            Action("view.toolWindow.resize/down"),
                        ],
                    ),
                ],
            ),
            Submenu(
                "menu-window-editor-tabs",
                &[
                    Action("editor.tab.next"),
                    Action("editor.tab.previous"),
                    Action("editor.tab.pin"),
                    Action("editor.tab.keepOpen"),
                    Separator,
                    Action("file.document.close"),
                    Action("file.document.closeOthers"),
                    Action("file.document.closeAll"),
                    Action("file.document.closeUnmodified"),
                    Action("file.document.closeAllButPinned"),
                    Action("file.document.closeLeft"),
                    Action("file.document.closeRight"),
                    Action("file.document.closeReadOnly"),
                    Action("file.document.reopenClosed"),
                    Separator,
                    Action("editor.split.right"),
                    Action("editor.split.right/move"),
                    Action("editor.split.down"),
                    Action("editor.split.down/move"),
                    Action("editor.split.unsplit"),
                    Action("editor.split.unsplitAll"),
                    Separator,
                    Action("editor.split.next"),
                    Action("editor.split.previous"),
                    Separator,
                    Action("view.settings.open/editorTabs"),
                ],
            ),
            Action("tasks.panel.toggle"),
        ],
    ),
    (
        "menu-help",
        &[
            Action("view.welcome.open"),
            Action("view.findAction.open"),
            Separator,
            Action("view.tipOfTheDay.open"),
            Action("view.productivity.open"),
            Action("view.whatsNew.open"),
            Action("view.keyboardShortcuts.open"),
            Action("view.openSourceLicences.open"),
            Separator,
            Action("diagnostics.log.show"),
            Action("view.collectLogs.open"),
            Separator,
            Submenu(
                "menu-help-diagnostic-tools",
                &[
                    Action("view.activityMonitor.open"),
                    Action("view.debugLogSettings.open"),
                    Action("view.specialFiles.open"),
                ],
            ),
        ],
    ),
];

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) const MACROS: &[Entry] = &[
    Action("editor.macro.playLast"),
    Action("view.macro.toggleRecording"),
    Action("view.macro.edit"),
    Action("view.macro.playSaved"),
];

fn find_menu(entries: &'static [Entry], key: &str) -> Option<&'static [Entry]> {
    entries.iter().find_map(|entry| match entry {
        Submenu(name, inner) if *name == key => Some(*inner),
        Submenu(_, inner) => find_menu(inner, key),
        _ => None,
    })
}

pub(super) fn entries_of(key: &str) -> Option<&'static [Entry]> {
    MENUS.iter().find_map(|(name, entries)| if *name == key { Some(*entries) } else { find_menu(entries, key) })
}

fn intern(text: &str) -> &'static str {
    static KNOWN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut known) = KNOWN.lock() else { return "" };
    if let Some(found) = known.iter().find(|found| **found == text) {
        return found;
    }
    let added: &'static str = Box::leak(text.to_owned().into_boxed_str());
    known.push(added);
    added
}

fn entry_of(token: &str, defaults: &[Entry]) -> Option<Entry> {
    use super::customization::{
        SEPARATOR, SPECIAL_LAYOUTS, SPECIAL_NEW, SPECIAL_RECENT, SPECIAL_TOOL_WINDOWS, SUBMENU,
    };
    if token == SEPARATOR {
        return Some(Separator);
    }
    if token == SPECIAL_NEW {
        return Some(NewItems);
    }
    if token == SPECIAL_RECENT {
        return Some(RecentProjects);
    }
    if token == SPECIAL_LAYOUTS {
        return Some(Layouts);
    }
    if token == SPECIAL_TOOL_WINDOWS {
        return Some(PluginToolWindows);
    }
    if let Some(key) = token.strip_prefix(SUBMENU) {
        let name = MENUS.iter().find_map(|(_, entries)| submenu_entry(entries, key));
        return name;
    }
    let named = defaults.iter().find(|entry| matches!(entry, Named(action, _) if *action == token));
    named.copied().or(Some(Action(intern(token))))
}

fn submenu_entry(entries: &'static [Entry], key: &str) -> Option<Entry> {
    entries.iter().find_map(|entry| match entry {
        Submenu(name, _) if *name == key => Some(*entry),
        Submenu(_, inner) => submenu_entry(inner, key),
        _ => None,
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) fn menu_keystrokes(
    keymap: &zen_core::keymap::Keymap,
    action: &str,
) -> Option<Vec<zen_core::keymap::Keystroke>> {
    match keymap.bindings().into_iter().find(|binding| binding.action == action) {
        Some(binding) => Some(binding.keystrokes),
        None => super::customization::is_known(action).then(Vec::new),
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) fn resolved(key: &str, defaults: &[Entry], custom: &super::customization::Customization) -> Vec<Entry> {
    match custom.groups.get(key) {
        Some(items) => items.iter().filter_map(|token| entry_of(token, defaults)).collect(),
        None => defaults.to_vec(),
    }
}

#[cfg(test)]
fn actions() -> Vec<&'static str> {
    fn walk(entries: &[Entry], out: &mut Vec<&'static str>) {
        for entry in entries {
            match entry {
                Action(action) | Named(action, _) => out.push(action),
                Submenu(_, entries) => walk(entries, out),
                Separator | Layouts | NewItems | RecentProjects | PluginToolWindows => {}
            }
        }
    }
    let mut out = Vec::new();
    for (_, entries) in MENUS {
        walk(entries, &mut out);
    }
    out
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) fn code_name(key: &str) -> Option<String> {
    Some(match key {
        letter if letter.len() == 1 && letter.chars().all(|c| c.is_ascii_uppercase()) => format!("Key{letter}"),
        digit if digit.starts_with("Num") && digit.len() == 4 => format!("Digit{}", &digit[3..]),
        "Equals" => "Equal".to_owned(),
        "OpenBracket" => "BracketLeft".to_owned(),
        "CloseBracket" => "BracketRight".to_owned(),
        "Backtick" => "Backquote".to_owned(),
        "Plus" => "NumpadAdd".to_owned(),
        other => other.to_owned(),
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn about_credits(product: &str) -> String {
    format!(
        "{}\n\n{}\n\n{}",
        zen_i18n::t!("menu-app-about-notice"),
        zen_i18n::t!("menu-app-about-trademarks"),
        zen_i18n::t!("menu-app-about-licences", product = product)
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) const CHECKED: &[&str] = &[
    "view.codeVision.authors.toggle",
    "editor.whitespaces.toggle",
    "editor.lineNumbers.toggle",
    "editor.indentGuides.toggle",
    "view.navigationBar.top",
    "view.navigationBar.statusBar",
    "view.navigationBar.hide",
    "view.navigationBar.members.toggle",
    "view.toolWindow.viewMode/dockPinned",
    "view.toolWindow.viewMode/dockUnpinned",
    "view.toolWindow.viewMode/undock",
    "view.toolWindow.viewMode/float",
    "view.toolWindow.viewMode/window",
    "view.powerSaveMode.toggle",
    "view.statusBarWidgets.open/lineColumn",
    "view.statusBarWidgets.open/languageServices",
    "view.statusBarWidgets.open/lineSeparator",
    "view.statusBarWidgets.open/fileEncoding",
    "view.statusBarWidgets.open/powerSaveMode",
    "view.statusBarWidgets.open/indentation",
    "view.statusBarWidgets.open/mcpServer",
    "view.statusBarWidgets.open/readOnly",
    "view.statusBarWidgets.open/memoryIndicator",
];

const FULL_SCREEN: &str = "view.fullScreen.toggle";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn system_full_screen(keymap: &zen_core::keymap::Keymap) -> bool {
    keymap.keystrokes(FULL_SCREEN).first() == zen_core::keymap::Keystroke::parse("⌃⌘F").as_ref()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const EXIT: &str = "view.ide.exit";

#[cfg(target_os = "macos")]
pub(super) use native::{Lists, take_chosen, take_quit, update};

#[cfg(target_os = "macos")]
mod native {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Mutex;

    use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
    use tray_icon::menu::{
        AboutMetadata, CheckMenuItem, IconMenuItem, Menu, MenuEvent, MenuItem, NativeIcon, PredefinedMenuItem, Submenu,
    };
    use zen_core::keymap::{Keymap, Keystroke};
    use zen_i18n::t;

    use super::{
        CHECKED, EXIT, Entry, FULL_SCREEN, MENUS, about_credits, code_name, layout_action, system_full_screen,
    };
    use crate::ide::keymap_page::action_title;

    static ITEMS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    static CHOSEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static FOCUSED: Mutex<Option<egui::ViewportId>> = Mutex::new(None);
    static QUIT: Mutex<(Option<String>, bool)> = Mutex::new((None, false));

    type Item = (Native, String, bool, String, String);

    #[derive(Clone)]
    enum Native {
        Plain(MenuItem),
        Check(CheckMenuItem),
        Icon(IconMenuItem),
    }

    impl Native {
        fn set_enabled(&self, enabled: bool) {
            match self {
                Self::Plain(item) => item.set_enabled(enabled),
                Self::Check(item) => item.set_enabled(enabled),
                Self::Icon(item) => item.set_enabled(enabled),
            }
        }

        fn set_text(&self, text: &str) {
            match self {
                Self::Plain(item) => item.set_text(text),
                Self::Check(item) => item.set_text(text),
                Self::Icon(item) => item.set_text(text),
            }
        }

        fn as_item(&self) -> &dyn tray_icon::menu::IsMenuItem {
            match self {
                Self::Plain(item) => item,
                Self::Check(item) => item,
                Self::Icon(item) => item,
            }
        }
    }

    struct Installed {
        built_for: String,
        _menu: Menu,
        items: Vec<Item>,
    }

    thread_local! {
        static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
    }

    #[derive(Clone, Copy)]
    pub(in crate::ide) struct Lists<'a> {
        pub(in crate::ide) layouts: &'a [(String, String, bool)],
        pub(in crate::ide) new_items: &'a [(String, String)],
        pub(in crate::ide) recent: &'a [(String, String)],
        pub(in crate::ide) tool_windows: &'a [(String, String)],
    }

    pub(in crate::ide) fn update(
        ctx: &egui::Context,
        product: &str,
        available: &dyn Fn(&str) -> bool,
        title: &dyn Fn(&str) -> Option<String>,
        checked: &dyn Fn(&str) -> bool,
        lists: Lists<'_>,
    ) {
        let Lists { layouts, new_items, recent, tool_windows } = lists;
        let keymap = crate::keymap::current();
        let custom = crate::ide::customization::current();
        let built_for = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            zen_i18n::language(),
            serde_json::to_string(keymap.user_changes()).unwrap_or_default(),
            serde_json::to_string(layouts).unwrap_or_default(),
            serde_json::to_string(new_items).unwrap_or_default(),
            serde_json::to_string(recent).unwrap_or_default(),
            serde_json::to_string(tool_windows).unwrap_or_default(),
            serde_json::to_string(&custom.groups).unwrap_or_default()
        );
        INSTALLED.with(|installed| {
            let mut installed = installed.borrow_mut();
            if installed.as_ref().is_none_or(|menu| menu.built_for != built_for) {
                *installed = build(ctx, product, &keymap, lists).map(|(menu, items)| Installed {
                    built_for,
                    _menu: menu,
                    items,
                });
            }
            if let Some(installed) = installed.as_mut() {
                for (item, action, enabled, name, shown) in &mut installed.items {
                    let now = available(action);
                    if now != *enabled {
                        item.set_enabled(now);
                        *enabled = now;
                    }
                    if let Native::Check(check) = item {
                        let on = checked(action);
                        if check.is_checked() != on {
                            check.set_checked(on);
                        }
                    }
                    let named = title(action).unwrap_or_else(|| name.clone());
                    if named != *shown {
                        item.set_text(&named);
                        *shown = named;
                    }
                }
            }
        });
    }

    pub(in crate::ide) fn take_chosen(viewport: egui::ViewportId) -> Vec<String> {
        if let Ok(mut focused) = FOCUSED.lock() {
            *focused = Some(viewport);
        }
        CHOSEN.lock().map(|mut chosen| std::mem::take(&mut *chosen)).unwrap_or_default()
    }

    pub(in crate::ide) fn take_quit() -> bool {
        QUIT.lock().is_ok_and(|mut quit| std::mem::take(&mut quit.1))
    }

    fn build(ctx: &egui::Context, product: &str, keymap: &Keymap, lists: Lists<'_>) -> Option<(Menu, Vec<Item>)> {
        let bar = Menu::new();
        let mut ids = HashMap::new();
        let mut items = Vec::new();
        let app = Submenu::new(product, true);
        let settings = item(keymap, "view.settings.open", None, &mut ids, &mut items);
        let _ = app.append_items(&[&PredefinedMenuItem::about(
            Some(&t!("menu-app-about", product = product)),
            Some(AboutMetadata {
                name: Some(product.to_owned()),
                credits: Some(about_credits(product)),
                ..Default::default()
            }),
        )]);
        if let Some(licences) = item(keymap, "view.openSourceLicences.open", None, &mut ids, &mut items) {
            let _ = app.append(licences.as_item());
        }
        let _ = app.append(&PredefinedMenuItem::separator());
        if let Some(settings) = &settings {
            let _ = app.append(settings.as_item());
        }
        let _ = app.append_items(&[
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(Some(&t!("menu-app-services"))),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some(&t!("menu-app-hide", product = product))),
            &PredefinedMenuItem::hide_others(Some(&t!("menu-app-hide-others"))),
            &PredefinedMenuItem::show_all(Some(&t!("menu-app-show-all"))),
            &PredefinedMenuItem::separator(),
        ]);
        let quit_keys = keymap.keystrokes(EXIT);
        let quit = MenuItem::new(t!("menu-app-quit", product = product), true, quit_keys.first().and_then(accelerator));
        let _ = app.append(&quit);
        if let Ok(mut known) = QUIT.lock() {
            known.0 = Some(quit.id().0.clone());
        }
        let _ = bar.append(&app);
        for (name, entries) in MENUS {
            let menu = Submenu::new(t!(name), true);
            let entries = super::resolved(name, entries, &crate::ide::customization::current());
            if fill(&menu, &entries, keymap, lists, &mut ids, &mut items) == 0 {
                continue;
            }
            if *name == "menu-window" {
                let _ = menu.prepend_items(&[
                    &PredefinedMenuItem::minimize(Some(&t!("menu-window-minimize"))),
                    &PredefinedMenuItem::maximize(Some(&t!("menu-window-zoom"))),
                    &PredefinedMenuItem::separator(),
                ]);
            }
            let _ = bar.append(&menu);
        }
        if let Ok(mut known) = ITEMS.lock() {
            *known = Some(ids);
        }
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Ok(mut quit) = QUIT.lock()
                && quit.0.as_deref() == Some(event.id.0.as_str())
            {
                quit.1 = true;
                drop(quit);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            let action = ITEMS.lock().ok().and_then(|items| items.as_ref()?.get(&event.id.0).cloned());
            if let Some(action) = action
                && let Ok(mut chosen) = CHOSEN.lock()
            {
                chosen.push(action);
            }
            match FOCUSED.lock().ok().and_then(|focused| *focused) {
                Some(viewport) => ctx.request_repaint_of(viewport),
                None => ctx.request_repaint(),
            }
        }));
        bar.init_for_nsapp();
        Some((bar, items))
    }

    fn fill(
        menu: &Submenu,
        entries: &[Entry],
        keymap: &Keymap,
        lists: Lists<'_>,
        ids: &mut HashMap<String, String>,
        items: &mut Vec<Item>,
    ) -> usize {
        let Lists { layouts, new_items, recent, tool_windows } = lists;
        let mut count = 0;
        let mut separate = false;
        for entry in entries {
            let added: Option<Box<dyn tray_icon::menu::IsMenuItem>> = match entry {
                Entry::Separator => {
                    separate = count > 0;
                    continue;
                }
                Entry::Action(action) if *action == FULL_SCREEN && system_full_screen(keymap) => {
                    Some(Box::new(PredefinedMenuItem::fullscreen(Some(&t!("menu-view-enter-fullScreen")))) as _)
                }
                Entry::Action(action) => item(keymap, action, None, ids, items).map(boxed),
                Entry::Named(action, name) => item(keymap, action, Some(name), ids, items).map(boxed),
                Entry::Submenu(name, entries) => {
                    let submenu = Submenu::new(t!(name), true);
                    let entries = super::resolved(name, entries, &crate::ide::customization::current());
                    (fill(&submenu, &entries, keymap, lists, ids, items) > 0).then(|| Box::new(submenu) as _)
                }
                Entry::Layouts => {
                    for (index, (title, name, current)) in layouts.iter().enumerate() {
                        if std::mem::take(&mut separate) || index == 1 {
                            let _ = menu.append(&PredefinedMenuItem::separator());
                        }
                        let check = current.then_some(NativeIcon::MenuOnState);
                        if index == 0 {
                            let action = layout_action("view.layout.apply", name);
                            let item = IconMenuItem::with_native_icon(title, true, check, None);
                            let item = dynamic(Native::Icon(item), &action, title, ids, items);
                            let _ = menu.append(item.as_item());
                        } else {
                            let submenu = Submenu::new(title, true);
                            submenu.set_native_icon(check);
                            layout_submenu(&submenu, keymap, name, *current, ids, items);
                            let _ = menu.append(&submenu);
                        }
                        count += 1;
                    }
                    None
                }
                Entry::PluginToolWindows => {
                    for (action, title) in tool_windows {
                        let item = dynamic(Native::Plain(MenuItem::new(title, true, None)), action, title, ids, items);
                        let _ = menu.append(item.as_item());
                        count += 1;
                    }
                    None
                }
                Entry::RecentProjects => {
                    for (key, title) in recent {
                        let item = dynamic(Native::Plain(MenuItem::new(title, true, None)), key, title, ids, items);
                        let _ = menu.append(item.as_item());
                        count += 1;
                    }
                    None
                }
                Entry::NewItems => {
                    for (key, title) in new_items {
                        if std::mem::take(&mut separate) {
                            let _ = menu.append(&PredefinedMenuItem::separator());
                        }
                        let action = key.strip_prefix("new:action:").unwrap_or_default();
                        let accelerator = keymap.keystrokes(action).first().and_then(accelerator);
                        let item = MenuItem::new(title, true, accelerator);
                        ids.insert(item.id().0.clone(), key.clone());
                        items.push((Native::Plain(item.clone()), key.clone(), true, title.clone(), title.clone()));
                        let _ = menu.append(&item);
                        count += 1;
                    }
                    None
                }
            };
            if let Some(added) = added {
                if std::mem::take(&mut separate) {
                    let _ = menu.append(&PredefinedMenuItem::separator());
                }
                let _ = menu.append(added.as_ref());
                count += 1;
            }
        }
        count
    }

    fn layout_submenu(
        menu: &Submenu,
        keymap: &Keymap,
        name: &str,
        current: bool,
        ids: &mut HashMap<String, String>,
        items: &mut Vec<Item>,
    ) {
        let restore = match current {
            true => item(keymap, "view.layout.restore", Some("menu-window-layout-restore"), ids, items),
            false => None,
        };
        let restore = restore.unwrap_or_else(|| {
            let title = t!("menu-window-layout-restore");
            let action = layout_action("view.layout.apply", name);
            dynamic(Native::Plain(MenuItem::new(&title, true, None)), &action, &title, ids, items)
        });
        let _ = menu.append(restore.as_item());
        for (command, message) in [
            ("view.layout.saveChanges", "menu-window-layout-save-changes"),
            ("view.layout.rename", "menu-window-layout-rename"),
            ("view.layout.delete", "menu-window-layout-delete"),
        ] {
            if command == "view.layout.delete" {
                let _ = menu.append(&PredefinedMenuItem::separator());
            }
            let title = t!(message);
            let action = layout_action(command, name);
            let item = dynamic(Native::Plain(MenuItem::new(&title, true, None)), &action, &title, ids, items);
            let _ = menu.append(item.as_item());
        }
    }

    fn dynamic(
        item: Native,
        action: &str,
        title: &str,
        ids: &mut HashMap<String, String>,
        items: &mut Vec<Item>,
    ) -> Native {
        let id = match &item {
            Native::Plain(item) => item.id().0.clone(),
            Native::Check(item) => item.id().0.clone(),
            Native::Icon(item) => item.id().0.clone(),
        };
        ids.insert(id, action.to_owned());
        items.push((item.clone(), action.to_owned(), true, title.to_owned(), title.to_owned()));
        item
    }

    fn boxed(item: Native) -> Box<dyn tray_icon::menu::IsMenuItem> {
        match item {
            Native::Plain(item) => Box::new(item),
            Native::Check(item) => Box::new(item),
            Native::Icon(item) => Box::new(item),
        }
    }

    fn item(
        keymap: &Keymap,
        action: &str,
        name: Option<&str>,
        ids: &mut HashMap<String, String>,
        items: &mut Vec<Item>,
    ) -> Option<Native> {
        let keystrokes = super::menu_keystrokes(keymap, action)?;
        let title = name.map_or_else(|| action_title(action), |name| t!(name));
        let accelerator = keystrokes.first().and_then(accelerator);
        let item = if CHECKED.contains(&action) {
            Native::Check(CheckMenuItem::new(&title, true, false, accelerator))
        } else {
            Native::Plain(MenuItem::new(&title, true, accelerator))
        };
        let id = match &item {
            Native::Plain(item) => item.id().0.clone(),
            Native::Check(item) => item.id().0.clone(),
            Native::Icon(item) => item.id().0.clone(),
        };
        ids.insert(id, action.to_owned());
        items.push((item.clone(), action.to_owned(), true, title.clone(), title));
        Some(item)
    }

    fn accelerator(keystroke: &Keystroke) -> Option<Accelerator> {
        if keystroke.then.is_some() {
            return None;
        }
        let code = Code::from_str(&code_name(&keystroke.key)?).ok()?;
        let mut modifiers = Modifiers::empty();
        for (on, modifier) in [
            (keystroke.control, Modifiers::CONTROL),
            (keystroke.alt, Modifiers::ALT),
            (keystroke.shift, Modifiers::SHIFT),
            (keystroke.command, Modifiers::META),
        ] {
            if on {
                modifiers |= modifier;
            }
        }
        Some(Accelerator::new(modifiers, code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_has_the_log_items_and_diagnostic_tools_its_dialogs() {
        let help = MENUS.iter().find(|(name, _)| *name == "menu-help").unwrap().1;
        let ids = |entries: &[Entry]| -> Vec<String> {
            entries
                .iter()
                .filter_map(|entry| if let Action(id) = entry { Some((*id).to_owned()) } else { None })
                .collect()
        };
        assert!(ids(help).ends_with(&["diagnostics.log.show".to_owned(), "view.collectLogs.open".to_owned()]));
        let tools = help
            .iter()
            .find_map(|entry| match entry {
                Submenu("menu-help-diagnostic-tools", entries) => Some(*entries),
                _ => None,
            })
            .unwrap();
        assert_eq!(ids(tools), ["view.activityMonitor.open", "view.debugLogSettings.open", "view.specialFiles.open"]);
    }

    #[test]
    fn every_menu_action_is_in_the_keymap_and_named() {
        let keymap = zen_core::keymap::Keymap::default_macos();
        let english = include_str!("../../locales/en/ui.ftl");
        for action in actions() {
            assert!(keymap.bindings().iter().any(|binding| binding.action == action), "{action} is not in the keymap");
        }
        for (name, _) in MENUS {
            assert!(english.contains(&format!("\n{name} = ")), "no name for {name}");
        }
    }

    #[test]
    fn file_properties_starts_with_the_encoding_and_the_bom() {
        let (_, file) = MENUS.iter().find(|(name, _)| *name == "menu-file").unwrap();
        let properties = file.iter().find_map(|entry| match entry {
            Submenu("menu-file-file-properties", entries) => Some(*entries),
            _ => None,
        });
        let first: Vec<&str> = properties
            .unwrap()
            .iter()
            .take(3)
            .filter_map(|entry| match entry {
                Action(action) => Some(*action),
                _ => None,
            })
            .collect();
        assert_eq!(first, ["view.fileEncoding.open", "file.bom.remove", "file.bom.add"]);
    }

    #[test]
    fn a_command_without_a_keymap_action_can_be_a_menu_item() {
        let keymap = zen_core::keymap::Keymap::default_macos();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        crate::ide::customization::note_commands(&registry);
        let command = registry
            .commands()
            .map(|command| command.id.to_string())
            .find(|id| keymap.bindings().iter().all(|binding| binding.action != *id))
            .unwrap();
        assert_eq!(menu_keystrokes(&keymap, &command), Some(Vec::new()), "{command}");
        assert_eq!(menu_keystrokes(&keymap, "gone.action"), None);
        assert!(menu_keystrokes(&keymap, "view.settings.open").is_some_and(|keys| !keys.is_empty()));
    }

    #[test]
    fn a_customized_menu_is_built_from_its_list_and_a_group_without_one_stays() {
        use super::super::customization::{Customization, SEPARATOR};
        let mut custom = Customization::default();
        let defaults = entries_of("menu-file").unwrap();
        assert_eq!(resolved("menu-file", defaults, &custom).len(), defaults.len());
        custom.groups.insert(
            "menu-file".to_owned(),
            vec!["editor.code.reformat".to_owned(), SEPARATOR.to_owned(), "submenu:menu-file-new".to_owned()],
        );
        let entries = resolved("menu-file", defaults, &custom);
        assert!(matches!(entries[0], Action("editor.code.reformat")));
        assert!(matches!(entries[1], Separator));
        assert!(matches!(entries[2], Submenu("menu-file-new", _)));
    }

    #[test]
    fn file_properties_has_the_three_line_separators_in_order() {
        let (_, file) = MENUS.iter().find(|(name, _)| *name == "menu-file").unwrap();
        let properties = file.iter().find_map(|entry| match entry {
            Submenu("menu-file-file-properties", entries) => Some(*entries),
            _ => None,
        });
        let separators = properties.unwrap().iter().find_map(|entry| match entry {
            Submenu("menu-file-line-separators", entries) => Some(*entries),
            _ => None,
        });
        let actions: Vec<&str> = separators
            .unwrap()
            .iter()
            .filter_map(|entry| match entry {
                Action(action) => Some(*action),
                _ => None,
            })
            .collect();
        assert_eq!(actions, ["file.lineSeparator.set/crlf", "file.lineSeparator.set/lf", "file.lineSeparator.set/cr"]);
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let titles: Vec<String> =
            actions.iter().map(|action| super::super::keymap_page::action_title(action)).collect();
        assert_eq!(titles, ["CRLF - Windows (\\r\\n)", "LF - Unix and macOS (\\n)", "CR - Classic Mac OS (\\r)"]);
    }

    #[test]
    fn edit_has_bookmarks() {
        let edit = MENUS.iter().find(|(name, _)| *name == "menu-edit").unwrap().1;
        let Some(Submenu("menu-edit-bookmarks", bookmarks)) = edit.iter().rev().nth(1) else {
            panic!("Bookmarks right before Macros in Edit")
        };
        let listed: Vec<&str> = bookmarks
            .iter()
            .filter_map(|entry| if let Action(action) = entry { Some(*action) } else { None })
            .collect();
        assert_eq!(
            listed,
            [
                "navigate.bookmark.toggle",
                "view.bookmark.mnemonic",
                "navigate.bookmark.previous",
                "navigate.bookmark.next",
                "view.bookmark.show",
                "view.bookmark.goToMnemonic"
            ]
        );
    }

    #[test]
    fn file_has_manage_ide_settings_with_import_export_and_restore() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let file = MENUS.iter().find(|(name, _)| *name == "menu-file").unwrap().1;
        let manage = file
            .iter()
            .find_map(|entry| match entry {
                Submenu("menu-file-manage-ide-settings", entries) => Some(*entries),
                _ => None,
            })
            .unwrap();
        let names: Vec<String> = manage
            .iter()
            .map(|entry| match entry {
                Action(action) => zen_i18n::t!(&format!("keymap-{}", action.replace('.', "-"))),
                _ => String::new(),
            })
            .collect();
        assert_eq!(names, ["Import Settings…", "Export Settings…", "Restore Default Settings…"]);
        assert_eq!(zen_i18n::t!("menu-file-manage-ide-settings"), "Manage IDE Settings");
    }

    #[test]
    fn full_screen_is_the_system_item_while_it_keeps_its_shortcut() {
        use zen_core::keymap::{Keymap, Keystroke};
        let mut keymap = Keymap::default_macos();
        assert!(system_full_screen(&keymap));
        keymap.set(FULL_SCREEN, Some(vec![Keystroke::parse("⌥⌘F").unwrap()]));
        assert!(!system_full_screen(&keymap), "a shortcut of the user's own is the item's");
        keymap.set(FULL_SCREEN, Some(Vec::new()));
        assert!(!system_full_screen(&keymap));
        assert!(actions().contains(&FULL_SCREEN));
    }

    #[test]
    fn generate_follows_code_completion_in_code() {
        let code = MENUS.iter().find(|(name, _)| *name == "menu-code").unwrap().1;
        assert!(matches!(code[0], Submenu("menu-code-completion", _)));
        assert!(matches!(code[2], Action("view.overrideMethods.open")), "Override Methods… after Code Completion");
        assert!(matches!(code[3], Action("view.implementMethods.open")));
        assert!(matches!(code[4], Action("view.generate.open")));
    }

    #[test]
    fn unwrap_follows_the_live_templates_in_code() {
        let code = MENUS.iter().find(|(name, _)| *name == "menu-code").unwrap().1;
        let at = code.iter().position(|entry| matches!(entry, Action("editor.templates.insert"))).unwrap();
        assert!(matches!(code[at + 1], Action("view.surround.open")), "Surround With first");
        assert!(matches!(code[at + 2], Action("view.unwrap.open")));
    }

    #[test]
    fn extend_and_shrink_selection_follow_select_all_in_edit() {
        let edit = MENUS.iter().find(|(name, _)| *name == "menu-edit").unwrap().1;
        let at = edit.iter().position(|entry| matches!(entry, Action("editor.selection.selectAll"))).unwrap();
        assert!(matches!(edit[at + 1], Action("editor.selection.extend")));
        assert!(matches!(edit[at + 2], Action("editor.selection.shrink")));
    }

    #[test]
    fn navigate_has_text_after_symbol_with_its_shortcut() {
        let navigate = MENUS.iter().find(|(name, _)| *name == "menu-navigate").unwrap().1;
        let named: Vec<&str> = navigate
            .iter()
            .filter_map(|item| match item {
                Named(action, _) => Some(*action),
                _ => None,
            })
            .collect();
        let symbol = named.iter().position(|action| *action == "view.gotoSymbol.open").unwrap();
        assert_eq!(named[symbol + 1], "view.gotoText.open");
        let keymap = zen_core::keymap::Keymap::default_macos();
        let keys: Vec<String> = keymap.keystrokes("view.gotoText.open").iter().map(ToString::to_string).collect();
        assert_eq!(keys, ["⌥⇧⌘E"]);
    }

    #[test]
    fn refactor_follows_code_with_rename() {
        let names: Vec<&str> = MENUS.iter().map(|(name, _)| *name).collect();
        let code = names.iter().position(|name| *name == "menu-code").unwrap();
        assert_eq!(names.get(code + 1), Some(&"menu-refactor"));
        let refactor = MENUS.iter().find(|(name, _)| *name == "menu-refactor").unwrap().1;
        assert!(matches!(refactor.first(), Some(Action("view.rename.open"))));
        assert!(
            matches!(refactor.get(1), Some(Action("view.changeSignature.open"))),
            "Change Signature… after Rename…"
        );
        let Some(Submenu("menu-refactor-extract", extract)) = refactor.get(3) else { panic!("{refactor:?}") };
        let named: Vec<&str> = extract
            .iter()
            .filter_map(|entry| if let Named(action, _) = entry { Some(*action) } else { None })
            .collect();
        assert_eq!(
            named,
            [
                "view.extract.variable",
                "view.extract.constant",
                "view.extract.field",
                "view.extract.parameter",
                "view.extract.parameterObject",
                "view.extract.method",
                "view.extract.interface",
                "view.extract.superclass"
            ]
        );
        assert!(matches!(refactor.get(4), Some(Action("view.inline.open"))), "Inline… after Extract/Introduce");
        assert!(
            matches!(
                refactor[6..9],
                [
                    Action("view.pullMembersUp.open"),
                    Action("view.pushMembersDown.open"),
                    Action("view.encapsulateFields.open")
                ]
            ),
            "the class refactorings after a separator: {refactor:?}"
        );
        assert!(matches!(refactor.get(10), Some(Action("view.project.move"))), "Move… after a separator");
        assert!(matches!(refactor.get(11), Some(Action("view.safeDelete.open"))), "Safe Delete… after Move…");
        let code = MENUS.iter().find(|(name, _)| *name == "menu-code").unwrap().1;
        assert!(matches!(
            code[code.len() - 3..],
            [Action("editor.code.reformat"), Action("editor.imports.optimize"), Action("editor.code.rearrange")]
        ));
        assert!(actions().contains(&"view.rename.open"));
    }

    #[test]
    fn build_and_run_follow_refactor_and_the_run_tool_window_is_in_view() {
        let names: Vec<&str> = MENUS.iter().map(|(name, _)| *name).collect();
        let refactor = names.iter().position(|name| *name == "menu-refactor").unwrap();
        assert_eq!(names.get(refactor + 1), Some(&"menu-build"));
        assert!(matches!(MENUS[refactor + 1].1, [Action("view.artifacts.build")]));
        assert_eq!(names.get(refactor + 2), Some(&"menu-run"));
        let run = MENUS[refactor + 2].1;
        assert!(matches!(run.first(), Some(Action("run.process.start"))));
        for action in ["run.process.start/context", "view.runConfigurations.open", "view.toolWindow.run"] {
            assert!(actions().contains(&action), "{action}");
        }
    }

    #[test]
    fn file_starts_with_new_and_its_items() {
        let (name, file) = MENUS[0];
        assert_eq!(name, "menu-file");
        assert!(matches!(
            file[0],
            Submenu(
                "menu-file-new",
                [
                    Action("view.newProject.open"),
                    Action("view.project.fromSources"),
                    Action("view.project.fromVcs"),
                    Separator,
                    Action("view.module.new"),
                    Action("view.module.fromSources"),
                    Separator,
                    NewItems,
                ]
            )
        ));
    }

    #[test]
    fn the_file_menu_has_the_specified_items_in_the_specified_order() {
        let shown: Vec<String> = MENUS[0]
            .1
            .iter()
            .map(|entry| match entry {
                Action(action) | Named(action, _) => (*action).to_owned(),
                Separator => "-".to_owned(),
                Submenu(name, _) => format!("{name}…"),
                Layouts | NewItems | RecentProjects | PluginToolWindows => String::new(),
            })
            .collect();
        assert_eq!(
            shown,
            [
                "menu-file-new…",
                "view.project.open",
                "menu-file-recent-projects…",
                "view.project.close",
                "view.project.closeAll",
                "view.project.closeOthers",
                "-",
                "view.projectStructure.open",
                "view.plugins.open",
                "menu-file-new-projects-setup…",
                "menu-file-file-properties…",
                "-",
                "menu-file-local-history…",
                "-",
                "file.document.saveAll",
                "file.document.syncWithDisk",
                "view.repairIde.start",
                "view.invalidateCaches.open",
                "-",
                "menu-file-manage-ide-settings…",
                "view.fileTemplates.saveAs",
                "-",
                "view.powerSaveMode.toggle",
            ]
        );
    }

    #[test]
    fn recent_projects_are_listed_by_name_and_folder_with_an_action_each() {
        let project = zen_workbench::recent::RecentProject { path: "/work/shop".into(), opened: 1, open: false };
        let items = recent_items(&[project]);
        assert_eq!(items[0].0, "recent:/work/shop");
        assert!(items[0].1.starts_with("shop  "));
    }

    #[test]
    fn run_has_debug_beside_run_and_the_stepping_actions_after_stop() {
        let run = MENUS.iter().find(|(name, _)| *name == "menu-run").unwrap().1;
        let shown: Vec<String> = run
            .iter()
            .map(|entry| match entry {
                Action(action) => (*action).to_owned(),
                Named(action, name) => format!("{action} as {name}"),
                Separator => "-".to_owned(),
                Submenu(name, _) => format!("{name}…"),
                Layouts => "layouts".to_owned(),
                NewItems => "new".to_owned(),
                RecentProjects => "recent".to_owned(),
                PluginToolWindows => "toolWindows".to_owned(),
            })
            .collect();
        assert_eq!(
            shown,
            [
                "run.process.start",
                "debug.session.start",
                "run.process.start/context as menu-run-context",
                "debug.session.start/context as menu-debug-context",
                "view.runConfigurations.choose",
                "view.runConfigurations.choose/debug",
                "coverage.run.start",
                "-",
                "view.runConfigurations.open",
                "-",
                "run.process.stop",
                "-",
                "debug.execution.stepOver",
                "debug.execution.stepInto",
                "debug.execution.stepOut",
                "debug.execution.resume",
                "debug.execution.pause",
                "debug.execution.runToCursor",
                "debug.classes.reload",
                "menu-run-debugging-actions…",
                "menu-run-toggle-breakpoint…",
                "view.breakpoints.open",
                "-",
                "view.testHistory.open",
            ]
        );
        let run = MENUS.iter().find(|(name, _)| *name == "menu-run").unwrap().1;
        let Some(Submenu(_, toggle)) =
            run.iter().find(|entry| matches!(entry, Submenu("menu-run-toggle-breakpoint", _)))
        else {
            panic!("Toggle Breakpoint is a submenu")
        };
        assert_eq!(toggle.len(), 3);
        let Some(Submenu(_, debugging)) =
            run.iter().find(|entry| matches!(entry, Submenu("menu-run-debugging-actions", _)))
        else {
            panic!("Debugging Actions is a submenu")
        };
        let debugging: Vec<&str> = debugging
            .iter()
            .filter_map(|entry| if let Action(action) = entry { Some(*action) } else { None })
            .collect();
        assert_eq!(
            debugging,
            [
                "debug.execution.forceStepOver",
                "debug.execution.forceStepInto",
                "view.smartStepInto.open",
                "debug.execution.forceRunToCursor",
                "debug.frame.reset",
                "view.streamTrace.open",
                "view.evaluateExpression.open",
                "view.quickEvaluate.open",
                "debug.executionPoint.show",
            ]
        );
        let view = MENUS.iter().find(|(name, _)| *name == "menu-view").unwrap().1;
        let Some(Submenu("menu-view-tool-windows", windows)) = view.first() else { panic!("Tool Windows first") };
        let windows: Vec<&str> =
            windows.iter().filter_map(|entry| if let Action(action) = entry { Some(*action) } else { None }).collect();
        let at = windows.iter().position(|action| *action == "view.toolWindow.run").unwrap();
        assert_eq!(windows.get(at + 1), Some(&"view.toolWindow.debug"));
        let english = include_str!("../../locales/en/ui.ftl");
        assert!(english.contains("\nmenu-debug-context = "));
    }

    #[test]
    fn edit_find_usages_is_a_submenu_ending_with_recent_find_usages() {
        let edit = MENUS.iter().find(|(name, _)| *name == "menu-edit").unwrap().1;
        let usages = edit
            .iter()
            .find_map(|entry| match entry {
                Submenu("menu-edit-find-usages", entries) => Some(*entries),
                _ => None,
            })
            .expect("Edit → Find Usages");
        let actions: Vec<&str> =
            usages.iter().filter_map(|entry| if let Action(action) = entry { Some(*action) } else { None }).collect();
        assert_eq!(actions.first(), Some(&"navigate.usages.find"));
        assert_eq!(actions.get(1), Some(&"navigate.usages.settings"));
        assert_eq!(actions.last(), Some(&"view.recentFindUsages.open"));
        let bindings = zen_core::keymap::Keymap::default_macos().bindings();
        assert!(bindings.iter().any(|binding| binding.action == "view.recentFindUsages.open"));
        let settings = bindings.iter().find(|binding| binding.action == "navigate.usages.settings").unwrap();
        let keys: Vec<String> = settings.keystrokes.iter().map(ToString::to_string).collect();
        assert_eq!(keys, ["⌥⇧⌘F7"]);
    }

    #[test]
    fn every_tool_window_with_a_shortcut_is_in_view_tool_windows() {
        let view = MENUS.iter().find(|(name, _)| *name == "menu-view").unwrap().1;
        let Some(Submenu("menu-view-tool-windows", windows)) = view.first() else { panic!("Tool Windows first") };
        let listed: Vec<&str> =
            windows.iter().filter_map(|entry| if let Action(action) = entry { Some(*action) } else { None }).collect();
        let bindings = zen_core::keymap::Keymap::default_macos().bindings();
        let tool_windows: Vec<&str> = bindings
            .iter()
            .filter(|binding| binding.action.starts_with("view.toolWindow.") && !binding.keystrokes.is_empty())
            .filter(|binding| crate::ide::tool_stripes::KINDS.iter().any(|kind| kind.action == binding.action))
            .map(|binding| binding.action.as_str())
            .collect();
        assert!(tool_windows.len() >= 9, "{tool_windows:?}");
        for action in tool_windows {
            assert!(listed.contains(&action), "{action} is missing from View → Tool Windows");
        }
        let at = listed.iter().position(|action| *action == "view.toolWindow.problems").unwrap();
        assert_eq!(listed[at - 1], "view.toolWindow.debug");
        assert_eq!(listed[at + 1], "view.toolWindow.terminal");
    }

    #[test]
    fn the_about_panel_says_that_zen_ide_is_not_affiliated_with_other_vendors() {
        zen_i18n::register(crate::toolbox::LOCALES);
        assert!(about_credits("GoGarden").contains("any other IDE vendor"), "{}", about_credits("GoGarden"));
        let notice = |locale: &str| {
            locale.lines().find_map(|line| line.strip_prefix("menu-app-about-notice = ")).unwrap_or_default().to_owned()
        };
        let english = notice(include_str!("../../locales/en/ui.ftl"));
        assert!(
            english.contains(
                "independent project and is not affiliated with, sponsored by or endorsed by any other IDE vendor."
            ),
            "{english}"
        );
        let russian = notice(include_str!("../../locales/ru/ui.ftl"));
        assert!(russian.contains("не связан ни с одним другим производителем IDE"), "{russian}");
        assert!(about_credits("GoGarden").contains("Java"), "{}", about_credits("GoGarden"));
        assert!(about_credits("GoGarden").contains("GoGarden → Open-source Licences"), "{}", about_credits("GoGarden"));
        let trademarks = |locale: &str| {
            locale
                .lines()
                .find_map(|line| line.strip_prefix("menu-app-about-trademarks = "))
                .unwrap_or_default()
                .to_owned()
        };
        let english = trademarks(include_str!("../../locales/en/ui.ftl"));
        assert!(
            english.contains(
                "Oracle, Java, MySQL, and NetSuite are registered trademarks of Oracle and/or its affiliates."
            )
        );
        assert!(english.contains("Kotlin® is a registered trademark of the Kotlin Foundation."));
        let russian = trademarks(include_str!("../../locales/ru/ui.ftl"));
        assert!(russian.contains("Oracle, Java, MySQL и NetSuite") && russian.contains("Kotlin®"), "{russian}");
    }

    #[test]
    fn status_bar_widgets_are_check_items_of_view_appearance_one_per_widget() {
        let view = MENUS.iter().find(|(name, _)| *name == "menu-view").unwrap().1;
        let Some(Submenu(_, appearance)) =
            view.iter().find(|entry| matches!(entry, Submenu("menu-view-appearance", _)))
        else {
            panic!("View → Appearance")
        };
        let Some(Submenu(_, widgets)) =
            appearance.iter().find(|entry| matches!(entry, Submenu("menu-view-status-bar-widgets", _)))
        else {
            panic!("View → Appearance → Status Bar Widgets")
        };
        let actions: Vec<&str> = widgets
            .iter()
            .filter_map(|entry| match entry {
                Action(action) => Some(*action),
                _ => None,
            })
            .collect();
        let expected: Vec<String> = crate::ide::status_widgets::StatusWidget::ALL
            .iter()
            .map(|widget| format!("view.statusBarWidgets.open/{}", widget.id()))
            .collect();
        assert_eq!(actions, expected);
        for action in actions {
            assert!(CHECKED.contains(&action), "{action} has no check mark");
        }
    }

    #[test]
    fn show_code_authors_is_a_check_item_of_view_appearance() {
        let view = MENUS.iter().find(|(name, _)| *name == "menu-view").unwrap().1;
        let appearance = view
            .iter()
            .find_map(|entry| match entry {
                Submenu("menu-view-appearance", entries) => Some(*entries),
                _ => None,
            })
            .unwrap();
        assert!(matches!(appearance.last(), Some(Action("view.codeVision.authors.toggle"))));
        for action in CHECKED {
            assert!(actions().contains(action), "{action} is in no menu");
        }
    }

    #[test]
    fn code_folding_keeps_its_order() {
        let code = MENUS.iter().find(|(name, _)| *name == "menu-code").unwrap().1;
        let Some(Submenu(_, folding)) = code.iter().find(|entry| matches!(entry, Submenu("menu-code-folding", _)))
        else {
            panic!("Code → Folding")
        };
        let shown: Vec<String> = folding
            .iter()
            .map(|entry| match entry {
                Action(action) => (*action).to_owned(),
                Named(action, _) => (*action).to_owned(),
                Separator => "-".to_owned(),
                Submenu(name, entries) => format!("{name} ({})", entries.len()),
                Layouts => "layouts".to_owned(),
                NewItems => "new".to_owned(),
                RecentProjects => "recent".to_owned(),
                PluginToolWindows => "toolWindows".to_owned(),
            })
            .collect();
        assert_eq!(
            shown,
            [
                "editor.fold.expand",
                "editor.fold.expandRecursively",
                "editor.fold.expandAll",
                "editor.fold.collapse",
                "editor.fold.collapseRecursively",
                "editor.fold.collapseAll",
                "menu-code-folding-expand-to-level (5)",
                "menu-code-folding-expand-all-to-level (5)",
                "editor.fold.expandDocComments",
                "editor.fold.collapseDocComments",
                "editor.fold.toggleAtCaret",
                "editor.fold.selection",
                "editor.fold.collapseBlock",
            ]
        );
        let keymap = zen_core::keymap::Keymap::default_macos();
        assert_eq!(keymap.shortcut("editor.fold.expandToLevel/3").as_deref(), Some("⌘NumPad*, 3"));
    }

    #[test]
    fn view_has_the_font_size_of_all_editors_after_active_editor_and_its_own_inside() {
        let view = MENUS.iter().find(|(name, _)| *name == "menu-view").unwrap().1;
        let at = view
            .iter()
            .position(|entry| matches!(entry, Submenu("menu-view-active-editor", _)))
            .expect("View → Active Editor");
        let Submenu(_, active) = view[at] else { unreachable!() };
        let names = |entries: &[Entry]| -> Vec<&str> {
            entries
                .iter()
                .filter_map(|entry| match entry {
                    Action(action) => Some(*action),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            names(active),
            [
                "editor.softWrap.toggle",
                "editor.whitespaces.toggle",
                "editor.lineNumbers.toggle",
                "editor.indentGuides.toggle",
                "editor.fontSize.increase",
                "editor.fontSize.decrease",
                "editor.fontSize.reset"
            ]
        );
        assert_eq!(
            names(&view[at + 1..at + 4]),
            ["view.editorFontSize.increase", "view.editorFontSize.decrease", "view.editorFontSize.reset"]
        );
        assert_eq!(code_name("Period").as_deref(), Some("Period"));
        assert_eq!(code_name("Comma").as_deref(), Some("Comma"));
    }

    #[test]
    fn show_line_numbers_is_a_check_item_of_view_active_editor() {
        let view = MENUS.iter().find(|(name, _)| *name == "menu-view").unwrap().1;
        let active_editor = view
            .iter()
            .find_map(|entry| match entry {
                Submenu("menu-view-active-editor", entries) => Some(*entries),
                _ => None,
            })
            .unwrap();
        assert!(active_editor.iter().any(|entry| matches!(entry, Action("editor.lineNumbers.toggle"))));
        assert!(CHECKED.contains(&"editor.lineNumbers.toggle"));
    }

    #[test]
    fn edit_ends_with_macros() {
        let edit = MENUS.iter().find(|(name, _)| *name == "menu-edit").unwrap().1;
        assert!(matches!(edit.last(), Some(Submenu("menu-edit-macros", _))));
        let keymap = zen_core::keymap::Keymap::default_macos();
        let shown: Vec<&str> =
            MACROS.iter().filter_map(|entry| if let Action(action) = entry { Some(*action) } else { None }).collect();
        assert_eq!(
            shown,
            ["editor.macro.playLast", "view.macro.toggleRecording", "view.macro.edit", "view.macro.playSaved"]
        );
        for action in shown {
            assert!(keymap.keystrokes(action).is_empty(), "{action} has no default shortcut");
        }
    }

    #[test]
    fn every_default_shortcut_has_a_menu_key() {
        for binding in zen_core::keymap::Keymap::default_macos().bindings() {
            for keystroke in binding.keystrokes {
                assert!(code_name(&keystroke.key).is_some(), "{}", keystroke.key);
            }
        }
        assert_eq!(code_name("F").as_deref(), Some("KeyF"));
        assert_eq!(code_name("Num1").as_deref(), Some("Digit1"));
        assert_eq!(code_name("Slash").as_deref(), Some("Slash"));
        assert_eq!(code_name("Plus").as_deref(), Some("NumpadAdd"), "⌘+ of the numeric keypad");
    }

    #[test]
    fn repair_ide_and_invalidate_caches_follow_reload_all_from_disk_in_file() {
        let file = MENUS.iter().find(|(name, _)| *name == "menu-file").unwrap().1;
        let ids: Vec<&str> =
            file.iter().filter_map(|entry| if let Action(id) = entry { Some(*id) } else { None }).collect();
        let at = ids.iter().position(|id| *id == "file.document.syncWithDisk").unwrap();
        assert_eq!(ids[at + 1..at + 3], ["view.repairIde.start", "view.invalidateCaches.open"]);
        assert!(!ids.contains(&"view.ide.restart"), "File has no Restart item");
    }

    #[test]
    fn window_has_the_layouts_and_the_view_modes_of_the_active_tool_window() {
        fn submenu(entries: &'static [Entry], name: &str) -> &'static [Entry] {
            entries
                .iter()
                .find_map(|entry| match entry {
                    Submenu(found, entries) if *found == name => Some(*entries),
                    _ => None,
                })
                .unwrap()
        }
        let window = MENUS.iter().find(|(name, _)| *name == "menu-window").unwrap().1;
        let layouts = submenu(window, "menu-window-layouts");
        assert!(matches!(layouts, [Layouts, Separator, Action("view.layout.saveAsNew")]));
        assert_eq!(layout_item("view.layout.delete:A: B"), Some(("view.layout.delete", "A: B")));
        assert_eq!(layout_item(&layout_action("view.layout.rename", "Custom")), Some(("view.layout.rename", "Custom")));
        assert_eq!(layout_item("view.layout.apply"), None);
        assert_eq!(layout_item("view.layout.restore:Custom"), None);
        let modes = submenu(submenu(window, "menu-window-active-tool-window"), "menu-window-view-mode");
        let actions: Vec<&str> = modes
            .iter()
            .map(|entry| match entry {
                Action(action) => *action,
                _ => "",
            })
            .collect();
        let expected: Vec<&str> = super::super::tool_windows::ViewMode::ALL.iter().map(|mode| mode.action()).collect();
        assert_eq!(actions, expected);
        for action in actions {
            assert!(CHECKED.contains(&action), "{action} has a check");
        }
    }
}

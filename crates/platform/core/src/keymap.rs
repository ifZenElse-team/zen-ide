use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const NAMED_KEYS: [&str; 25] = [
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Insert",
    "Space",
    "Escape",
    "Tab",
    "Enter",
    "Backspace",
    "Delete",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Semicolon",
    "Quote",
    "Backtick",
    "Backslash",
    "Plus",
    "Slash",
    "Comma",
    "Period",
    "Minus",
    "NumpadMultiply",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Keystroke {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub command: bool,
    pub key: String,
    pub then: Option<Box<Keystroke>>,
}

impl Keystroke {
    pub fn parse(text: &str) -> Option<Self> {
        if let Some((first, second)) = text.split_once(", ") {
            let second = Self::parse_one(second)?;
            return Some(Self { then: Some(Box::new(second)), ..Self::parse_one(first)? });
        }
        Self::parse_one(text)
    }

    pub fn followed_by(&self, second: Keystroke) -> Self {
        Self { then: Some(Box::new(second.first())), ..self.first() }
    }

    pub fn first(&self) -> Self {
        Self { then: None, ..self.clone() }
    }

    pub fn double(modifier: char) -> Option<Self> {
        let mut stroke = Keystroke::default();
        let key = match modifier {
            '⌃' => {
                stroke.control = true;
                "Control"
            }
            '⌥' => {
                stroke.alt = true;
                "Alt"
            }
            '⇧' => {
                stroke.shift = true;
                "Shift"
            }
            '⌘' => {
                stroke.command = true;
                "Command"
            }
            _ => return None,
        };
        Some(Self { key: key.to_owned(), ..stroke })
    }

    pub fn is_double(&self) -> bool {
        self.then.is_none() && Self::double_symbol(&self.key).and_then(Self::double).as_ref() == Some(self)
    }

    fn double_symbol(key: &str) -> Option<char> {
        match key {
            "Control" => Some('⌃'),
            "Alt" => Some('⌥'),
            "Shift" => Some('⇧'),
            "Command" => Some('⌘'),
            _ => None,
        }
    }

    fn parse_one(text: &str) -> Option<Self> {
        let mut stroke = Keystroke::default();
        let mut rest = text.trim();
        loop {
            let mut chars = rest.chars();
            match chars.next() {
                Some('⌃') => stroke.control = true,
                Some('⌥') => stroke.alt = true,
                Some('⇧') => stroke.shift = true,
                Some('⌘') => stroke.command = true,
                _ => break,
            }
            rest = chars.as_str();
        }
        let key = match rest {
            "" => {
                let mut symbols = text.trim().chars();
                return match (symbols.next(), symbols.next(), symbols.next()) {
                    (Some(first), Some(second), None) if first == second => Self::double(first),
                    _ => None,
                };
            }
            "/" => "Slash",
            "," => "Comma",
            ";" => "Semicolon",
            "=" => "Equals",
            "+" => "Plus",
            "-" | "−" => "Minus",
            "." => "Period",
            "[" => "OpenBracket",
            "]" => "CloseBracket",
            "`" => "Backtick",
            "\\" => "Backslash",
            "↑" => "ArrowUp",
            "↓" => "ArrowDown",
            "←" => "ArrowLeft",
            "→" => "ArrowRight",
            "⌫" => "Backspace",
            "⌦" => "Delete",
            "⏎" | "↩" => "Enter",
            "⇥" => "Tab",
            "⎋" => "Escape",
            "NumPad*" => "NumpadMultiply",
            other if other.len() == 1 && other.chars().all(|c| c.is_ascii_digit()) => {
                return Some(Self { key: format!("Num{other}"), ..stroke });
            }
            other if other.len() == 1 => return Some(Self { key: other.to_ascii_uppercase(), ..stroke }),
            other if NAMED_KEYS.contains(&other) => other,
            other
                if other
                    .strip_prefix('F')
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| (1..=35).contains(&n)) =>
            {
                other
            }
            _ => return None,
        };
        Some(Self { key: key.to_owned(), ..stroke })
    }

    fn key_symbol(&self) -> String {
        let symbol = match self.key.as_str() {
            "Slash" => "/",
            "Comma" => ",",
            "Semicolon" => ";",
            "Equals" => "=",
            "Plus" => "+",
            "Minus" => "−",
            "Period" => ".",
            "OpenBracket" => "[",
            "CloseBracket" => "]",
            "Backtick" => "`",
            "Backslash" => "\\",
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "ArrowLeft" => "←",
            "ArrowRight" => "→",
            "Backspace" => "⌫",
            "Delete" => "⌦",
            "Enter" => "⏎",
            "Tab" => "⇥",
            "Escape" => "⎋",
            "NumpadMultiply" => "NumPad*",
            other if Self::double_symbol(other).is_some() && self.is_double() => {
                return Self::double_symbol(other).map(String::from).unwrap_or_default();
            }
            other => return other.strip_prefix("Num").unwrap_or(other).to_owned(),
        };
        symbol.to_owned()
    }
}

impl From<Keystroke> for String {
    fn from(keystroke: Keystroke) -> Self {
        keystroke.to_string()
    }
}

impl TryFrom<String> for Keystroke {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Self::parse(&text).ok_or_else(|| format!("`{text}` is not a keystroke"))
    }
}

impl fmt::Display for Keystroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, symbol) in [(self.control, "⌃"), (self.alt, "⌥"), (self.shift, "⇧"), (self.command, "⌘")] {
            if on {
                f.write_str(symbol)?;
            }
        }
        f.write_str(&self.key_symbol())?;
        match &self.then {
            Some(second) => write!(f, ", {second}"),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Context {
    #[default]
    Global,
    Editor,
    Terminal,
    Database,
    Markdown,
}

pub const GROUPS: [&str; 16] = [
    "keymap-group-file",
    "keymap-group-window",
    "keymap-group-navigate",
    "keymap-group-find",
    "keymap-group-caret",
    "keymap-group-edit",
    "keymap-group-carets",
    "keymap-group-folding",
    "keymap-group-refactor",
    "keymap-group-vcs",
    "keymap-group-run",
    "keymap-group-debug",
    "keymap-group-macros",
    "keymap-group-external-tools",
    "keymap-group-database",
    "keymap-group-other",
];

pub const MACRO_ACTION: &str = "editor.macro.play/";

pub fn macro_action(name: &str) -> String {
    format!("{MACRO_ACTION}{name}")
}

pub const EXTERNAL_TOOL_ACTION: &str = "run.externalTools.run/";

pub fn group_of(action: &str) -> usize {
    match action {
        _ if action.starts_with(MACRO_ACTION) => 12,
        _ if action.starts_with("markdown.format.") => 5,
        _ if action.starts_with(EXTERNAL_TOOL_ACTION) => 13,
        _ if action.starts_with("file.") || action.starts_with("editor.edit.undo") || action == "editor.edit.redo" => 0,
        "view.ide.exit" | "view.powerSaveMode.toggle" => 0,
        _ if action.starts_with("view.ideSettings.") => 0,
        _ if action.starts_with("view.newProject.") || action.starts_with("view.module.") => 0,
        _ if action.starts_with("view.project.") && action != "view.project.open" => 0,
        _ if action.starts_with("view.scratch.") => 0,
        "view.project.open"
        | "view.settings.open"
        | "view.settings.open/editorTabs"
        | "view.projectStructure.open"
        | "view.settings.newProjects"
        | "view.toolWindow.structure"
        | "view.toolWindow.bookmarks"
        | "view.toolWindow.project"
        | "view.toolWindow.terminal"
        | "view.toolWindow.problems"
        | "view.toolWindow.find"
        | "view.toolWindow.run"
        | "view.toolWindow.commit"
        | "view.toolWindow.git"
        | "view.toolWindow.debug"
        | "view.toolWindow.hide"
        | "view.toolWindow.hideAll"
        | "view.toolWindow.move"
        | "view.toolWindow.viewMode"
        | "view.layout.restore"
        | "view.layout.restoreDefault"
        | "view.layout.saveAsNew"
        | "view.layout.apply"
        | "view.layout.saveChanges"
        | "view.layout.rename"
        | "view.layout.delete"
        | "view.presentationMode.toggle"
        | "view.distractionFreeMode.toggle"
        | "view.zenMode.toggle"
        | "view.fullScreen.toggle"
        | "view.ideZoom.open"
        | "view.ideZoom.in"
        | "view.ideZoom.out"
        | "view.ideZoom.reset"
        | "view.codeVision.authors.toggle"
        | "view.navigationBar.top"
        | "view.navigationBar.statusBar"
        | "view.navigationBar.hide"
        | "view.navigationBar.members.toggle"
        | "view.quickSwitchScheme.open"
        | "view.toolWindow.services"
        | "view.toolWindow.hierarchy"
        | "view.toolWindow.coverage"
        | "view.toolWindow.notifications"
        | "view.toolWindow.todo"
        | "view.toolWindow.caffeine"
        | "view.statusBarWidgets.open"
        | "view.activityMonitor.open"
        | "view.whatsNew.open"
        | "view.keyboardShortcuts.open"
        | "view.openSourceLicences.open"
        | "view.keyboardShortcuts.export"
        | "view.tipOfTheDay.open"
        | "view.productivity.open"
        | "view.productivity.reset"
        | "view.welcome.open"
        | "view.guide.open"
        | "view.toolWindow.maven"
        | "view.toolWindow.gradle"
        | "view.toolWindow.build"
        | "view.buildChanges.load"
        | "view.toolWindow.ant"
        | "view.toolWindow.npm"
        | "view.toolWindow.endpoints"
        | "view.toolWindow.dependencies" => 1,
        _ if action.starts_with("view.buildTool.") || action.starts_with("view.startersDialog.") => 1,
        _ if action.starts_with("view.dependencyAnalyzer.") => 1,
        "view.occurrence.next" | "view.occurrence.previous" => 2,
        _ if action.starts_with("view.todo.") => 1,
        _ if action.starts_with("view.statusBarWidgets.open/") => 1,
        _ if action.starts_with("view.toolWindow.resize/") || action.starts_with("view.toolWindow.viewMode/") => 1,
        _ if action.starts_with("terminal.") => 1,
        _ if action.starts_with("editor.split.") => 1,
        "view.testHistory.open" => 10,
        "view.createTest.open" => 2,
        _ if action.starts_with("editor.tab.") => 1,
        "view.artifacts.build" => 10,
        _ if action.starts_with("view.runConfigurations.") => 10,
        "view.evaluateExpression.open"
        | "view.quickEvaluate.open"
        | "view.smartStepInto.open"
        | "view.streamTrace.open" => 11,
        _ if action.starts_with("debug.") || action.starts_with("view.breakpoint") => 11,
        _ if action.starts_with("run.") || action.starts_with("coverage.") => 10,
        "editor.problem.next" | "editor.problem.previous" => 2,
        _ if action.starts_with("editor.highlightUsages.") => 3,
        "view.rename.open"
        | "view.safeDelete.open"
        | "view.changeSignature.open"
        | "view.pullMembersUp.open"
        | "view.pushMembersDown.open"
        | "view.encapsulateFields.open"
        | "view.extract.parameterObject"
        | "view.extract.interface"
        | "view.extract.superclass"
        | "view.refactoring.open"
        | "view.intentions.open"
        | "view.generate.open"
        | "view.overrideMethods.open"
        | "view.implementMethods.open"
        | "view.unwrap.open"
        | "view.extract.variable"
        | "view.extract.constant"
        | "view.extract.field"
        | "view.extract.parameter"
        | "view.extract.method"
        | "view.inline.open"
        | "editor.code.reformat"
        | "file.convert.toLanguage/kotlin"
        | "editor.imports.optimize"
        | "editor.code.rearrange" => 8,
        "view.problem.fix" => 5,
        "view.surroundWithTemplate.open" | "view.runAnything.open" => 15,
        _ if action.starts_with("db.") || action.starts_with("view.database.") || action.ends_with("/database") => 14,
        _ if action.starts_with("vcs.")
            || action.starts_with("view.vcs.")
            || action.starts_with("view.localHistory.") =>
        {
            9
        }
        _ if action.starts_with("refactor.") => 8,
        _ if action.starts_with("editor.fold.") => 7,
        _ if action.starts_with("navigate.") || action.starts_with("view.bookmark") => 2,
        "view.recentFiles.open"
        | "view.switcher.open"
        | "view.recentLocations.open"
        | "view.fileStructure.open"
        | "view.customFolding.open"
        | "view.selectIn.open"
        | "view.selectIn.target"
        | "view.gotoClass.open"
        | "view.gotoFile.open"
        | "view.gotoSymbol.open"
        | "view.gotoText.open"
        | "view.gotoDatabaseObject.open"
        | "view.gotoLine.open"
        | "view.searchEverywhere.open"
        | "view.navigationBar.jump"
        | "view.findAction.open" => 2,
        _ if action.starts_with("view.find")
            || action.starts_with("view.replace")
            || action.starts_with("editor.find.") =>
        {
            3
        }
        _ if action.starts_with("editor.caret.move") || action == "editor.brackets.goToMatching" => 4,
        "editor.selection.selectAll" | "editor.selection.extend" | "editor.selection.shrink" => 5,
        _ if action.starts_with("editor.selection.") => 6,
        "editor.caret.keepPrimary" | "editor.caret.cloneAbove" | "editor.caret.cloneBelow" => 6,
        _ => 5,
    }
}

pub fn gesture(action: &str) -> Option<&'static str> {
    match action {
        "view.searchEverywhere.open" => Some("⇧⇧"),
        "editor.caret.cloneAbove" => Some("⌥⌥↑"),
        "editor.caret.cloneBelow" => Some("⌥⌥↓"),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Binding {
    pub action: String,
    pub command: String,
    pub params: Value,
    pub context: Context,
    pub keystrokes: Vec<Keystroke>,
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<Binding>,
    user: BTreeMap<String, Vec<Keystroke>>,
}

impl Keymap {
    pub fn default_macos() -> Self {
        let mut bindings = Vec::new();
        let mut bind = |action: &str, command: &str, params: Value, context: Context, keys: &[&str]| {
            bindings.push(Binding {
                action: action.to_owned(),
                command: command.to_owned(),
                params,
                context,
                keystrokes: keys.iter().filter_map(|key| Keystroke::parse(key)).collect(),
            });
        };
        let global = Context::Global;
        let editor = Context::Editor;
        let terminal = Context::Terminal;
        let database = Context::Database;
        let none = || json!({});
        bind("file.document.save", "file.document.save", none(), global, &[]);
        bind("file.document.saveAll", "file.document.saveAll", none(), global, &["⌘S"]);
        bind("file.document.close", "file.document.close", none(), global, &["⌘W"]);
        bind("file.document.closeOthers", "file.document.closeOthers", none(), global, &[]);
        bind("file.document.closeAll", "file.document.closeAll", none(), global, &[]);
        bind("file.document.closeUnmodified", "file.document.closeUnmodified", none(), global, &[]);
        bind("file.document.closeAllButPinned", "file.document.closeAllButPinned", none(), global, &[]);
        bind("file.document.closeLeft", "file.document.closeLeft", none(), global, &[]);
        bind("file.document.closeRight", "file.document.closeRight", none(), global, &[]);
        bind("file.document.closeReadOnly", "file.document.closeReadOnly", none(), global, &[]);
        bind("file.document.reopenClosed", "file.document.reopenClosed", none(), global, &[]);
        bind("editor.tab.pin", "editor.tab.pin", none(), global, &[]);
        bind("editor.tab.keepOpen", "editor.tab.keepOpen", none(), global, &[]);
        bind("editor.tab.next", "editor.tab.next", none(), global, &["⇧⌘]", "⌃→"]);
        bind("editor.tab.previous", "editor.tab.previous", none(), global, &["⇧⌘[", "⌃←"]);
        bind("editor.split.moveToOpposite", "editor.split.moveToOpposite", none(), global, &[]);
        bind("editor.edit.undo", "editor.edit.undo", none(), global, &["⌘Z"]);
        bind("editor.edit.redo", "editor.edit.redo", none(), global, &["⇧⌘Z"]);
        bind("editor.split.right", "editor.split.right", none(), global, &[]);
        bind("editor.split.down", "editor.split.down", none(), global, &[]);
        bind("editor.split.right/move", "editor.split.right", json!({ "move": true }), global, &[]);
        bind("editor.split.down/move", "editor.split.down", json!({ "move": true }), global, &[]);
        bind("editor.split.unsplit", "editor.split.unsplit", none(), global, &[]);
        bind("editor.split.unsplitAll", "editor.split.unsplitAll", none(), global, &[]);
        bind("editor.split.next", "editor.split.next", none(), global, &[]);
        bind("editor.split.previous", "editor.split.previous", none(), global, &[]);
        bind("view.project.open", "view.project.open", none(), global, &[]);
        bind("view.project.trust", "view.project.trust", none(), global, &[]);
        bind("view.scratch.new", "view.scratch.new", none(), global, &["⇧⌘N"]);
        bind("view.scratch.newBuffer", "view.scratch.newBuffer", none(), global, &[]);
        bind("view.scratch.show", "view.scratch.show", none(), global, &[]);
        bind("view.toolWindow.project", "view.toolWindow.project", none(), global, &["⌘1"]);
        bind("view.toolWindow.structure", "view.toolWindow.structure", none(), global, &["⌘7"]);
        bind("view.toolWindow.bookmarks", "view.toolWindow.bookmarks", none(), global, &["⌘2"]);
        bind("view.toolWindow.terminal", "view.toolWindow.terminal", none(), global, &["⌥F12"]);
        bind("terminal.session.new", "terminal.session.new", none(), terminal, &["⌘T"]);
        bind("terminal.session.close", "terminal.session.close", none(), terminal, &["⌘W"]);
        let right = json!({ "direction": "right" });
        bind("terminal.session.split/right", "terminal.session.split", right, terminal, &[]);
        bind("terminal.session.split/down", "terminal.session.split", json!({ "direction": "down" }), terminal, &[]);
        bind("terminal.session.focusNext", "terminal.session.focusNext", none(), terminal, &["⌥⇥"]);
        bind("terminal.session.focusPrevious", "terminal.session.focusPrevious", none(), terminal, &["⌥⇧⇥"]);
        bind("view.toolWindow.problems", "view.toolWindow.problems", none(), global, &["⌘6"]);
        bind("view.toolWindow.find", "view.toolWindow.find", none(), global, &["⌘3"]);
        bind("view.toolWindow.hide", "view.toolWindow.hide", none(), global, &["⇧⎋"]);
        bind("view.toolWindow.hideAll", "view.toolWindow.hideAll", none(), global, &["⇧⌘F12"]);
        bind("view.toolWindow.move", "view.toolWindow.move", none(), global, &[]);
        bind("tasks.panel.toggle", "tasks.panel.toggle", none(), global, &[]);
        for (direction, key) in [("left", "⌃⌥←"), ("right", "⌃⌥→"), ("up", "⌃⌥↑"), ("down", "⌃⌥↓")]
        {
            let action = format!("view.toolWindow.resize/{direction}");
            bind(&action, "view.toolWindow.resize", json!({ "direction": direction }), global, &[key]);
        }
        bind("view.layout.restore", "view.layout.restore", none(), global, &["⇧F12"]);
        bind("view.layout.restoreDefault", "view.layout.restoreDefault", none(), global, &[]);
        bind("view.toolWindow.viewMode", "view.toolWindow.viewMode", none(), global, &[]);
        for mode in ["dockPinned", "dockUnpinned", "undock", "float", "window"] {
            let action = format!("view.toolWindow.viewMode/{mode}");
            bind(&action, "view.toolWindow.viewMode", json!({ "mode": mode }), global, &[]);
        }
        bind("view.layout.saveAsNew", "view.layout.saveAsNew", none(), global, &[]);
        bind("view.layout.apply", "view.layout.apply", none(), global, &[]);
        bind("view.layout.saveChanges", "view.layout.saveChanges", none(), global, &[]);
        bind("view.layout.rename", "view.layout.rename", none(), global, &[]);
        bind("view.layout.delete", "view.layout.delete", none(), global, &[]);
        bind("view.toolWindow.notifications", "view.toolWindow.notifications", none(), global, &[]);
        bind("view.toolWindow.todo", "view.toolWindow.todo", none(), global, &[]);
        bind("view.toolWindow.maven", "view.toolWindow.maven", none(), global, &[]);
        bind("view.toolWindow.gradle", "view.toolWindow.gradle", none(), global, &[]);
        bind("view.toolWindow.build", "view.toolWindow.build", none(), global, &[]);
        bind("view.buildChanges.load", "view.buildChanges.load", none(), global, &["⇧⌘I"]);
        bind("view.toolWindow.ant", "view.toolWindow.ant", none(), global, &[]);
        bind("view.toolWindow.npm", "view.toolWindow.npm", none(), global, &[]);
        bind("view.toolWindow.endpoints", "view.toolWindow.endpoints", none(), global, &[]);
        for action in [
            "view.startersDialog.open",
            "view.startersDialog.set",
            "view.startersDialog.apply",
            "view.startersDialog.close",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.toolWindow.dependencies", "view.toolWindow.dependencies", none(), global, &[]);
        for action in ["view.dependencies.select", "view.dependencies.search"] {
            bind(action, action, none(), global, &[]);
        }
        for action in [
            "view.buildTool.select",
            "view.buildTool.toggle",
            "view.buildTool.run",
            "view.buildTool.menu",
            "view.buildTool.search",
            "view.buildTool.assignShortcut",
            "view.maven.executeGoal",
            "view.maven.effectivePom",
            "view.maven.beforeRun",
            "view.gradle.executeTask",
            "view.gradle.beforeRun",
        ] {
            bind(action, action, none(), global, &[]);
        }
        for action in [
            "view.dependencyAnalyzer.open",
            "view.dependencyAnalyzer.set",
            "view.dependencyAnalyzer.select",
            "view.dependencyAnalyzer.toggle",
            "view.dependencyAnalyzer.refresh",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.ideErrors.open", "view.ideErrors.open", none(), global, &[]);
        bind("view.occurrence.previous", "view.occurrence.previous", none(), global, &["⌥⌘↑"]);
        bind("view.occurrence.next", "view.occurrence.next", none(), global, &["⌥⌘↓"]);
        for action in [
            "view.todo.tab",
            "view.todo.expandAll",
            "view.todo.collapseAll",
            "view.todo.previewSource.toggle",
            "view.todo.navigateWithSingleClick.toggle",
            "view.todo.filter",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.settings.open", "view.settings.open", none(), global, &["⌘,"]);
        let structure = json!({ "page": "projectStructure" });
        bind("view.projectStructure.open", "view.settings.open", structure, global, &["⌘;"]);
        bind("view.plugins.open", "view.settings.open", json!({ "page": "plugins" }), global, &[]);
        bind("view.settings.newProjects", "view.settings.newProjects", none(), global, &[]);
        bind("view.ide.restart", "view.ide.restart", none(), global, &[]);
        for action in ["view.project.close", "view.project.closeAll", "view.project.closeOthers"] {
            bind(action, action, none(), global, &[]);
        }
        for action in [
            "view.newProject.open",
            "view.newProject.empty",
            "view.newProject.close",
            "view.project.fromSources",
            "view.project.fromVcs",
            "view.project.fromVcs.close",
            "view.module.new",
            "view.module.fromSources",
        ] {
            bind(action, action, none(), global, &[]);
        }
        let tabs_page = json!({ "page": "editorTabs" });
        bind("view.settings.open/editorTabs", "view.settings.open", tabs_page, global, &[]);
        bind("view.repairIde.start", "view.repairIde.start", none(), global, &[]);
        bind("view.invalidateCaches.open", "view.invalidateCaches.open", none(), global, &[]);
        for action in ["view.ideSettings.import", "view.ideSettings.export", "view.ideSettings.restoreDefaults"] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.toolWindow.caffeine", "view.toolWindow.caffeine", none(), global, &[]);
        bind("view.indent.menu", "view.indent.menu", none(), global, &[]);
        bind("view.powerSaveMode.toggle", "view.powerSaveMode.toggle", none(), global, &[]);
        bind("view.activityMonitor.open", "view.activityMonitor.open", none(), global, &[]);
        bind("diagnostics.log.show", "diagnostics.log.show", none(), global, &[]);
        bind("view.collectLogs.open", "view.collectLogs.open", none(), global, &[]);
        bind("view.debugLogSettings.open", "view.debugLogSettings.open", none(), global, &[]);
        bind("view.specialFiles.open", "view.specialFiles.open", none(), global, &[]);
        bind("view.openSourceLicences.open", "view.openSourceLicences.open", none(), global, &[]);
        for action in [
            "view.whatsNew.open",
            "view.keyboardShortcuts.open",
            "view.keyboardShortcuts.export",
            "view.tipOfTheDay.open",
            "view.productivity.open",
            "view.productivity.reset",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.welcome.open", "view.welcome.open", none(), global, &[]);
        bind("view.guide.open", "view.guide.open", none(), global, &[]);
        bind("view.statusBarWidgets.open", "view.statusBarWidgets.open", none(), global, &[]);
        for widget in [
            "lineColumn",
            "languageServices",
            "lineSeparator",
            "fileEncoding",
            "powerSaveMode",
            "indentation",
            "mcpServer",
            "readOnly",
            "memoryIndicator",
        ] {
            let action = format!("view.statusBarWidgets.open/{widget}");
            bind(&action, "view.statusBarWidgets.open", json!({ "widget": widget }), global, &[]);
        }
        bind("view.ide.exit", "view.ide.exit", none(), global, &["⌘Q"]);
        bind("view.presentationMode.toggle", "view.presentationMode.toggle", none(), global, &[]);
        bind("view.distractionFreeMode.toggle", "view.distractionFreeMode.toggle", none(), global, &[]);
        bind("view.zenMode.toggle", "view.zenMode.toggle", none(), global, &[]);
        bind("view.fullScreen.toggle", "view.fullScreen.toggle", none(), global, &["⌃⌘F"]);
        for action in ["view.ideZoom.open", "view.ideZoom.in", "view.ideZoom.out", "view.ideZoom.reset"] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.codeVision.authors.toggle", "view.codeVision.authors.toggle", none(), global, &["⌃⌥A"]);
        bind("view.navigationBar.jump", "view.navigationBar.jump", none(), global, &["⌘↑", "⌥Home"]);
        for action in [
            "view.navigationBar.top",
            "view.navigationBar.statusBar",
            "view.navigationBar.hide",
            "view.navigationBar.members.toggle",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.quickSwitchScheme.open", "view.quickSwitchScheme.open", none(), global, &["⌃`"]);
        bind("view.find.open", "view.find.open", none(), global, &["⌘F"]);
        bind("view.replace.open", "view.replace.open", none(), global, &["⌘R"]);
        bind("editor.find.next", "editor.find.next", none(), global, &["⌘G"]);
        bind("editor.find.previous", "editor.find.previous", none(), global, &["⇧⌘G"]);
        bind("view.findInFiles.open", "view.findInFiles.open", none(), global, &["⇧⌘F"]);
        bind("view.replaceInFiles.open", "view.replaceInFiles.open", none(), global, &["⇧⌘R"]);
        bind("view.findInFiles.pin", "view.findInFiles.pin", none(), global, &[]);
        bind("view.findInFiles.openInFindWindow", "view.findInFiles.openInFindWindow", none(), global, &[]);
        bind("view.findResults.settings", "view.findResults.settings", none(), global, &[]);
        bind("view.inspectCode.open", "view.inspectCode.open", none(), global, &[]);
        bind("view.codeCleanup.open", "view.codeCleanup.open", none(), global, &[]);
        bind("view.silentCleanup.run", "view.silentCleanup.run", none(), global, &[]);
        bind("view.inspectByName.open", "view.inspectByName.open", none(), global, &["⌥⇧⌘I"]);
        for action in [
            "view.inspectResults.tab",
            "view.inspectResults.groupByDirectory.toggle",
            "view.inspectResults.showExcluded.toggle",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("view.findResults.newTab", "view.findResults.newTab", none(), global, &[]);
        bind("view.recentFiles.open", "view.recentFiles.open", none(), global, &["⌘E"]);
        bind("view.switcher.open", "view.switcher.open", none(), global, &["⌃⇥", "⌃⇧⇥"]);
        bind("view.recentLocations.open", "view.recentLocations.open", none(), global, &["⇧⌘E"]);
        bind("view.fileStructure.open", "view.fileStructure.open", none(), global, &["⌘F12"]);
        bind("view.customFolding.open", "view.customFolding.open", none(), global, &["⌥⌘."]);
        bind("view.selectIn.open", "view.selectIn.open", none(), global, &["⌥F1"]);
        bind("view.selectIn.target", "view.selectIn.target", none(), global, &[]);
        bind("view.pasteFromHistory.open", "view.pasteFromHistory.open", none(), global, &["⇧⌘V"]);
        bind("view.gotoClass.open", "view.gotoClass.open", none(), global, &["⌘O"]);
        bind("view.gotoFile.open", "view.gotoFile.open", none(), global, &["⇧⌘O"]);
        bind("view.gotoSymbol.open", "view.gotoSymbol.open", none(), global, &["⌥⌘O"]);
        bind("view.gotoText.open", "view.gotoText.open", none(), global, &["⌥⇧⌘E"]);
        bind("view.gotoLine.open", "view.gotoLine.open", none(), global, &["⌘L"]);
        bind("view.searchEverywhere.open", "view.searchEverywhere.open", none(), global, &[]);
        bind("view.findAction.open", "view.findAction.open", none(), global, &["⇧⌘A"]);
        bind("file.document.toggleReadOnly", "file.document.toggleReadOnly", none(), global, &[]);
        bind("file.document.syncWithDisk", "file.document.syncWithDisk", none(), global, &["⌥⌘Y"]);
        for separator in ["crlf", "lf", "cr"] {
            let action = format!("file.lineSeparator.set/{separator}");
            bind(&action, "file.lineSeparator.set", json!({ "separator": separator }), global, &[]);
        }
        bind("editor.find.selectAll", "editor.find.selectAll", none(), editor, &[]);
        bind("editor.find.searchInSelection", "editor.find.searchInSelection", none(), global, &["⌃⌥E"]);
        bind("editor.find.scrollToResults", "editor.find.scrollToResults", none(), global, &[]);
        bind("editor.find.openInFindWindow", "editor.find.openInFindWindow", none(), global, &[]);
        bind("editor.find.nextWordAtCaret", "editor.find.nextWordAtCaret", none(), editor, &[]);
        bind("editor.find.previousWordAtCaret", "editor.find.previousWordAtCaret", none(), editor, &[]);
        bind("editor.line.indent", "editor.line.indent", none(), editor, &[]);
        bind("editor.problem.next", "editor.problem.next", none(), editor, &["F2"]);
        bind("editor.completion.start", "editor.completion.start", none(), editor, &["⌃Space"]);
        bind("editor.completion.smart", "editor.completion.smart", none(), editor, &["⌃⇧Space"]);
        bind("editor.statement.complete", "editor.statement.complete", none(), editor, &["⇧⌘⏎"]);
        bind("editor.completion.expandWord", "editor.completion.expandWord", none(), editor, &["⌥/"]);
        let forward = json!({ "forward": true });
        bind("editor.completion.expandWord/forward", "editor.completion.expandWord", forward, editor, &["⌥⇧/"]);
        bind("editor.templates.insert", "editor.templates.insert", none(), editor, &["⌘J"]);
        bind("view.surround.open", "view.surround.open", none(), global, &["⌥⌘T"]);
        bind("view.surroundWithTemplate.open", "view.surroundWithTemplate.open", none(), global, &["⌥⌘J"]);
        bind("editor.fold.collapse", "editor.fold.collapse", none(), editor, &["⌘−"]);
        bind("editor.fold.expand", "editor.fold.expand", none(), editor, &["⌘=", "⌘+"]);
        bind("editor.fold.expandRecursively", "editor.fold.expandRecursively", none(), editor, &["⌥⌘=", "⌥⌘+"]);
        bind("editor.fold.collapseRecursively", "editor.fold.collapseRecursively", none(), editor, &["⌥⌘−"]);
        bind("editor.fold.collapseAll", "editor.fold.collapseAll", none(), editor, &["⇧⌘−"]);
        bind("editor.fold.expandAll", "editor.fold.expandAll", none(), editor, &["⇧⌘=", "⇧⌘+"]);
        let all_levels = if cfg!(target_os = "macos") { "⌥⌘NumPad*" } else { "⇧⌘NumPad*" };
        for level in 1..=5 {
            let params = json!({ "level": level });
            let here = format!("⌘NumPad*, {level}");
            let all = format!("{all_levels}, {level}");
            let action = format!("editor.fold.expandToLevel/{level}");
            bind(&action, "editor.fold.expandToLevel", params.clone(), editor, &[here.as_str()]);
            let action = format!("editor.fold.expandAllToLevel/{level}");
            bind(&action, "editor.fold.expandAllToLevel", params, editor, &[all.as_str()]);
        }
        bind("editor.fold.expandDocComments", "editor.fold.expandDocComments", none(), editor, &[]);
        bind("editor.fold.collapseDocComments", "editor.fold.collapseDocComments", none(), editor, &[]);
        bind("editor.fold.toggleAtCaret", "editor.fold.toggleAtCaret", none(), editor, &[]);
        bind("editor.fold.selection", "editor.fold.selection", none(), editor, &["⌘."]);
        bind("editor.fold.collapseBlock", "editor.fold.collapseBlock", none(), editor, &["⇧⌘."]);
        bind("editor.problem.previous", "editor.problem.previous", none(), editor, &["⇧F2"]);
        bind("editor.brackets.goToMatching", "editor.brackets.goToMatching", none(), editor, &["⌃⇧M"]);
        bind("navigate.declaration.goTo", "navigate.declaration.goTo", none(), editor, &["⌘B"]);
        bind("navigate.implementation.goTo", "navigate.implementation.goTo", none(), editor, &["⌥⌘B"]);
        bind("navigate.typeDeclaration.goTo", "navigate.typeDeclaration.goTo", none(), editor, &["⇧⌘B", "⌃⇧B"]);
        bind("navigate.superMethod.goTo", "navigate.superMethod.goTo", none(), editor, &["⌘U"]);
        bind("navigate.test.goTo", "navigate.test.goTo", none(), editor, &["⇧⌘T"]);
        bind("view.createTest.open", "view.createTest.open", none(), global, &[]);
        bind("navigate.usages.find", "navigate.usages.find", none(), editor, &["⌥F7"]);
        bind("navigate.usages.settings", "navigate.usages.settings", none(), editor, &["⌥⇧⌘F7"]);
        bind("navigate.usages.show", "navigate.usages.show", none(), editor, &["⌥⌘F7"]);
        bind("navigate.usages.findInFile", "navigate.usages.findInFile", none(), editor, &["⌘F7"]);
        bind("view.recentFindUsages.open", "view.recentFindUsages.open", none(), global, &[]);
        bind("navigate.hierarchy.type", "navigate.hierarchy.type", none(), editor, &["⌃H"]);
        bind("navigate.hierarchy.call", "navigate.hierarchy.call", none(), editor, &["⌃⌥H"]);
        bind("view.toolWindow.hierarchy", "view.toolWindow.hierarchy", none(), global, &[]);
        bind("view.toolWindow.coverage", "view.toolWindow.coverage", none(), global, &[]);
        bind("editor.highlightUsages.toggle", "editor.highlightUsages.toggle", none(), editor, &["⇧⌘F7"]);
        bind("editor.highlightUsages.next", "editor.highlightUsages.next", none(), editor, &["⌃⌥↓"]);
        bind("editor.highlightUsages.previous", "editor.highlightUsages.previous", none(), editor, &["⌃⌥↑"]);
        bind("editor.documentation.show", "editor.documentation.show", none(), editor, &["F1", "⌃J"]);
        bind("editor.parameterInfo.show", "editor.parameterInfo.show", none(), editor, &["⌘P"]);
        bind("editor.code.reformat", "editor.code.reformat", none(), editor, &["⌥⌘L"]);
        bind("file.convert.toLanguage/kotlin", "file.convert.toLanguage", json!({ "to": "kt" }), editor, &["⌥⇧⌘K"]);
        bind("editor.imports.optimize", "editor.imports.optimize", none(), editor, &["⌃⌥O"]);
        bind("editor.code.rearrange", "editor.code.rearrange", none(), editor, &[]);
        bind("navigate.history.back", "navigate.history.back", none(), global, &["⌘["]);
        bind("navigate.history.forward", "navigate.history.forward", none(), global, &["⌘]"]);
        bind("navigate.bookmark.toggle", "navigate.bookmark.toggle", none(), global, &["F3"]);
        bind("view.bookmark.mnemonic", "view.bookmark.mnemonic", none(), global, &["⌥F3"]);
        bind("view.bookmark.goToMnemonic", "view.bookmark.goToMnemonic", none(), global, &["⌥⌘F3"]);
        bind("view.bookmark.show", "view.bookmark.show", none(), global, &["⌘F3"]);
        bind("view.bookmarks.next", "view.bookmarks.next", none(), global, &[]);
        bind("view.bookmarks.previous", "view.bookmarks.previous", none(), global, &[]);
        bind("navigate.bookmark.previous", "navigate.bookmark.previous", none(), global, &[]);
        bind("navigate.bookmark.next", "navigate.bookmark.next", none(), global, &[]);
        bind("navigate.bookmark.removeMnemonic", "navigate.bookmark.removeMnemonic", none(), global, &[]);
        bind("view.bookmark.rename", "view.bookmark.rename", none(), global, &[]);
        bind("view.bookmark.moveToList", "view.bookmark.moveToList", none(), global, &[]);
        bind("view.bookmark.openTabs", "view.bookmark.openTabs", none(), global, &[]);
        for digit in 0..=9 {
            let mnemonic = json!({ "mnemonic": digit.to_string() });
            let (toggle, go) = (format!("⌃⇧{digit}"), format!("⌃{digit}"));
            let action = format!("navigate.bookmark.toggleMnemonic/{digit}");
            bind(&action, "navigate.bookmark.toggleMnemonic", mnemonic.clone(), global, &[&toggle]);
            bind(&format!("navigate.bookmark.goTo/{digit}"), "navigate.bookmark.goTo", mnemonic, global, &[&go]);
        }
        bind("navigate.edit.last", "navigate.edit.last", none(), global, &["⇧⌘⌫"]);
        bind("navigate.edit.next", "navigate.edit.next", none(), global, &[]);
        bind("view.rename.open", "view.rename.open", none(), global, &["⇧F6"]);
        bind("view.safeDelete.open", "view.safeDelete.open", none(), global, &["⌘⌦"]);
        bind("view.changeSignature.open", "view.changeSignature.open", none(), global, &["⌘F6"]);
        bind("view.pullMembersUp.open", "view.pullMembersUp.open", none(), global, &[]);
        bind("view.pushMembersDown.open", "view.pushMembersDown.open", none(), global, &[]);
        bind("view.encapsulateFields.open", "view.encapsulateFields.open", none(), global, &[]);
        bind("view.extract.parameterObject", "view.extract.parameterObject", none(), global, &[]);
        bind("view.extract.interface", "view.extract.interface", none(), global, &[]);
        bind("view.extract.superclass", "view.extract.superclass", none(), global, &[]);
        bind("view.refactoring.open", "view.refactoring.open", none(), global, &[]);
        bind("view.intentions.open", "view.intentions.open", none(), global, &["⌥⏎"]);
        bind("view.generate.open", "view.generate.open", none(), editor, &["⌘N"]);
        bind("view.extract.variable", "view.extract.variable", none(), editor, &["⌥⌘V"]);
        bind("view.extract.constant", "view.extract.constant", none(), editor, &["⌥⌘C"]);
        bind("view.extract.field", "view.extract.field", none(), editor, &["⌥⌘F"]);
        bind("view.extract.parameter", "view.extract.parameter", none(), editor, &["⌥⌘P"]);
        bind("view.extract.method", "view.extract.method", none(), editor, &["⌥⌘M"]);
        bind("view.inline.open", "view.inline.open", none(), editor, &["⌥⌘N"]);
        bind("view.overrideMethods.open", "view.overrideMethods.open", none(), editor, &["⌃O"]);
        bind("view.implementMethods.open", "view.implementMethods.open", none(), editor, &["⌃I"]);
        bind("db.console.execute", "db.console.execute", none(), database, &["⌘⏎"]);
        bind("db.console.executeScript", "db.console.executeScript", none(), database, &["⌥X", "⌥⌘⏎"]);
        bind("db.console.executeInNewTab", "db.console.executeInNewTab", none(), database, &["⌘\\", "⌃⌘⏎"]);
        bind("db.console.executeFromCaret", "db.console.executeFromCaret", none(), database, &["⌥P"]);
        bind("db.console.explain", "db.console.explain", none(), database, &["⌃⇧E", "⌥⌘E"]);
        bind("view.database.newConsole", "view.database.newConsole", none(), database, &["⌃]"]);
        bind("view.database.newConnection", "view.dataSource.open", none(), database, &["⌃⇧N"]);
        bind("editor.code.reformat/database", "editor.code.reformat", none(), database, &["⌃⇧F"]);
        bind("editor.comment.line/database", "editor.comment.line", none(), database, &["⌃/"]);
        bind("db.script.run", "db.script.run", none(), editor, &["⌘⏎"]);
        bind("db.script.run/script", "db.script.run", json!({ "scope": "script" }), editor, &["⌥⌘⏎"]);
        bind("db.operation.cancel", "db.operation.cancel", none(), database, &["⌘F2"]);
        bind("db.sql.navigate", "db.sql.navigate", none(), database, &["F4"]);
        bind("db.console.nextStatement", "db.console.nextStatement", none(), database, &[]);
        bind("db.console.previousStatement", "db.console.previousStatement", none(), database, &[]);
        bind("db.console.toggleResults", "db.console.toggleResults", none(), database, &["⌃T"]);
        bind("db.console.maximizeResults", "db.console.maximizeResults", none(), database, &["⌃⇧T"]);
        bind("db.result.toggleValuePanel", "db.result.toggleValuePanel", none(), database, &["⇧⌘P"]);
        bind("db.result.save", "db.result.save", none(), database, &["⌘S"]);
        bind("db.result.revert", "db.result.revert", none(), database, &["⌥⌘Z"]);
        bind("db.result.previewChanges", "db.result.previewChanges", none(), database, &[]);
        bind("db.result.addRow", "db.result.addRow", none(), database, &[]);
        bind("db.result.resetFilter", "db.result.resetFilter", none(), database, &[]);
        bind("view.database.focusFilter", "view.database.focusFilter", none(), database, &["⌃⌥⇧T", "⌥⌘F"]);
        bind("view.gotoDatabaseObject.open", "view.gotoDatabaseObject.open", none(), database, &["⌃⇧D", "⌘O"]);
        bind("view.database.copyAdvanced", "view.database.copyAdvanced", none(), database, &["⌃⇧C", "⇧⌘C"]);
        bind("view.database.pasteAdvanced", "view.database.pasteAdvanced", none(), database, &["⌃⇧V", "⌥⇧⌘V"]);
        bind("db.transaction.commit", "db.transaction.commit", none(), database, &["⌃4", "⌃⌘K"]);
        bind("db.transaction.rollback", "db.transaction.rollback", none(), database, &["⌃8", "⌃⌘Z"]);
        bind("view.database.transactionMode", "view.database.transactionMode", none(), database, &[]);
        bind("view.database.pendingTransactions", "view.database.pendingTransactions", none(), global, &[]);
        bind("view.database.chooseConnection", "view.database.chooseConnection", none(), database, &["⌃9", "⌥⌘9"]);
        bind("view.database.chooseSchema", "view.database.chooseSchema", none(), database, &["⌃0", "⌥⌘0"]);
        bind("view.problem.fix", "view.problem.fix", none(), global, &["⌥⇧⏎"]);
        bind("view.unwrap.open", "view.unwrap.open", none(), global, &["⇧⌘⌦"]);
        bind("run.process.start", "run.process.start", none(), global, &["⌃R"]);
        bind("run.process.start/context", "run.process.start", json!({ "context": {} }), global, &["⌃⇧R"]);
        bind("run.process.stop", "run.process.stop", none(), global, &["⌘F2"]);
        bind("view.toolWindow.run", "view.toolWindow.run", none(), global, &["⌘4"]);
        bind("view.runConfigurations.open", "view.runConfigurations.open", none(), global, &[]);
        bind("view.testHistory.open", "view.testHistory.open", none(), global, &[]);
        bind("view.runConfigurations.choose", "view.runConfigurations.choose", none(), global, &["⌃⌥R"]);
        let debug = json!({ "debug": true });
        bind("view.runConfigurations.choose/debug", "view.runConfigurations.choose", debug, global, &["⌃⌥D"]);
        bind("view.runAnything.open", "view.runAnything.open", none(), global, &["⌃⌃"]);
        bind("debug.session.start", "debug.session.start", none(), global, &["⌃D"]);
        bind("debug.session.start/context", "debug.session.start", json!({ "context": {} }), global, &["⌃⇧D"]);
        bind("coverage.run.start", "coverage.run.start", none(), global, &[]);
        bind("debug.execution.stepOver", "debug.execution.stepOver", none(), global, &["F8"]);
        bind("debug.execution.stepInto", "debug.execution.stepInto", none(), global, &["F7"]);
        bind("debug.execution.stepOut", "debug.execution.stepOut", none(), global, &["⇧F8"]);
        bind("debug.execution.resume", "debug.execution.resume", none(), global, &["⌥⌘R"]);
        bind("debug.execution.pause", "debug.execution.pause", none(), global, &[]);
        bind("debug.execution.runToCursor", "debug.execution.runToCursor", none(), editor, &["⌥F9"]);
        bind("debug.execution.forceRunToCursor", "debug.execution.forceRunToCursor", none(), editor, &["⌥⌘F9"]);
        bind("debug.execution.forceStepOver", "debug.execution.forceStepOver", none(), global, &["⌥⇧F8"]);
        bind("debug.execution.forceStepInto", "debug.execution.forceStepInto", none(), global, &["⌥⇧F7"]);
        bind("view.smartStepInto.open", "view.smartStepInto.open", none(), global, &["⇧F7"]);
        bind("debug.frame.reset", "debug.frame.reset", none(), global, &[]);
        bind("view.streamTrace.open", "view.streamTrace.open", none(), global, &[]);
        bind("debug.classes.reload", "debug.classes.reload", none(), global, &[]);
        bind("view.evaluateExpression.open", "view.evaluateExpression.open", none(), global, &["⌥F8"]);
        bind("view.quickEvaluate.open", "view.quickEvaluate.open", none(), editor, &["⌥⌘F8"]);
        bind("debug.executionPoint.show", "debug.executionPoint.show", none(), global, &["⌥F10"]);
        bind("debug.breakpoint.toggle", "debug.breakpoint.toggle", none(), editor, &["⌘F8"]);
        bind("debug.breakpoint.toggleTemporary", "debug.breakpoint.toggleTemporary", none(), editor, &["⌥⇧⌘F8"]);
        bind("debug.breakpoint.toggleEnabled", "debug.breakpoint.toggleEnabled", none(), editor, &[]);
        bind("view.breakpoints.open", "view.breakpoints.open", none(), global, &["⇧⌘F8"]);
        bind("view.breakpoint.properties", "view.breakpoint.properties", none(), editor, &[]);
        bind("view.breakpoint.addConditional", "view.breakpoint.addConditional", none(), editor, &[]);
        bind("view.breakpoint.addLogging", "view.breakpoint.addLogging", none(), editor, &[]);
        bind("debug.breakpoints.mute", "debug.breakpoints.mute", none(), global, &[]);
        bind("view.toolWindow.debug", "view.toolWindow.debug", none(), global, &["⌘5"]);
        bind("view.toolWindow.services", "view.toolWindow.services", none(), global, &["⌘8"]);
        bind("view.toolWindow.commit", "view.toolWindow.commit", none(), global, &["⌘0"]);
        bind("view.vcs.commit", "view.vcs.commit", none(), global, &["⌘K"]);
        bind("view.vcs.rollback", "view.vcs.rollback", none(), global, &["⌥⌘Z"]);
        bind("view.vcs.showDiff", "view.vcs.showDiff", none(), global, &[]);
        bind("view.vcs.jumpToSource", "view.vcs.jumpToSource", none(), global, &["F4", "⌘↓"]);
        bind("view.vcs.update", "view.vcs.update", none(), global, &["⌘T"]);
        bind("view.toolWindow.git", "view.toolWindow.git", none(), global, &["⌘9"]);
        bind("vcs.blame.annotate", "vcs.blame.annotate", none(), global, &[]);
        bind("view.vcs.push", "view.vcs.push", none(), global, &["⇧⌘K"]);
        bind("vcs.remote.fetch", "vcs.remote.fetch", none(), global, &[]);
        bind("view.vcs.branches", "view.vcs.branches", none(), global, &[]);
        bind("view.projects.popup", "view.projects.popup", none(), global, &[]);
        bind("view.vcs.newBranch", "view.vcs.newBranch", none(), global, &["⌥⌘N"]);
        bind("view.vcs.checkoutRevision", "view.vcs.checkoutRevision", none(), global, &[]);
        bind("view.vcs.newChangelist", "view.vcs.newChangelist", none(), global, &[]);
        bind("view.vcs.moveToChangelist", "view.vcs.moveToChangelist", none(), global, &["⌥⇧M"]);
        bind("view.vcs.shelve", "view.vcs.shelve", none(), global, &[]);
        bind("view.vcs.unshelve", "view.vcs.unshelve", none(), global, &[]);
        bind("view.vcs.showShelf", "view.vcs.showShelf", none(), global, &[]);
        bind("view.vcs.createPatch", "view.vcs.createPatch", none(), global, &[]);
        bind("view.vcs.copyPatch", "view.vcs.copyPatch", none(), global, &[]);
        bind("view.vcs.applyPatch", "view.vcs.applyPatch", none(), global, &[]);
        bind("view.vcs.applyPatchFromClipboard", "view.vcs.applyPatchFromClipboard", none(), global, &[]);
        bind("view.vcs.showHistory", "view.vcs.showHistory", none(), global, &[]);
        bind("view.vcs.commitFiles", "view.vcs.commitFiles", none(), global, &[]);
        bind("view.project.select", "view.project.select", none(), global, &[]);
        bind("view.project.selectOpened", "view.project.selectOpened", none(), global, &[]);
        bind("view.project.alwaysSelectOpened", "view.project.alwaysSelectOpened", none(), global, &[]);
        bind("view.project.contextMenu", "view.project.contextMenu", none(), global, &[]);
        bind("view.project.rename", "view.project.rename", none(), global, &[]);
        bind("view.project.openInTerminal", "view.project.openInTerminal", none(), global, &[]);
        bind("view.project.compareWith", "view.project.compareWith", none(), global, &[]);
        bind("view.project.excludedFiles", "view.project.excludedFiles", none(), global, &[]);
        bind("view.project.move", "view.project.move", none(), global, &["F6"]);
        bind("view.project.changeLanguage", "view.project.changeLanguage", none(), global, &[]);
        bind("view.compare.clipboard", "view.compare.clipboard", none(), global, &[]);
        bind("view.diff.blank", "view.diff.blank", none(), global, &[]);
        bind("view.diff.swapSides", "view.diff.swapSides", none(), global, &[]);
        for action in [
            "view.folderDiff.compare",
            "view.folderDiff.refresh",
            "view.folderDiff.filter",
            "view.folderDiff.select",
            "view.folderDiff.setAction",
            "view.folderDiff.synchronize",
            "view.folderDiff.open",
        ] {
            bind(action, action, none(), global, &[]);
        }
        for action in [
            "view.resourceBundle.open",
            "view.resourceBundle.select",
            "view.resourceBundle.filter",
            "view.resourceBundle.addKey",
            "view.resourceBundle.renameKey",
        ] {
            bind(action, action, none(), global, &[]);
        }
        for action in
            ["view.diff.next", "view.diff.previous", "view.diff.whitespace", "view.diff.viewer", "view.diff.setText"]
        {
            bind(action, action, none(), global, &[]);
        }
        for action in [
            "view.vcs.logBranches",
            "view.vcs.logExpandBranches",
            "view.vcs.logCollapseBranches",
            "view.vcs.logGroupBranches",
            "view.vcs.logExpandChanges",
            "view.vcs.logCollapseChanges",
            "view.vcs.logGroupChanges",
            "view.vcs.logGoTo",
            "view.vcs.logShowDiff",
            "view.vcs.logSelectPaths",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("vcs.workspace.open", "vcs.workspace.open", none(), global, &[]);
        bind("vcs.stash.create", "vcs.stash.create", none(), global, &["⌥⇧⌘S"]);
        bind("vcs.stash.pop", "vcs.stash.pop", none(), global, &["⌥⇧⌘P"]);
        bind("view.localHistory.show", "view.localHistory.show", none(), global, &[]);
        bind("view.localHistory.putLabel", "view.localHistory.putLabel", none(), global, &[]);
        bind("view.associateFileType.open", "view.associateFileType.open", none(), global, &[]);
        bind("view.fileEncoding.open", "view.fileEncoding.open", none(), global, &[]);
        bind("file.bom.add", "file.bom.add", none(), global, &[]);
        bind("file.bom.remove", "file.bom.remove", none(), global, &[]);
        bind("editor.window.openTab", "editor.window.openTab", none(), global, &["⇧F4"]);
        bind("view.editorConfig.new", "view.editorConfig.new", none(), global, &[]);
        bind("project.editorConfig.open", "project.editorConfig.open", none(), global, &[]);
        bind("view.editorConfig.showFiles", "view.editorConfig.showFiles", none(), global, &[]);
        bind("view.editorConfig.disable", "view.editorConfig.disable", none(), global, &[]);
        for action in
            ["view.markdownPreview.selectAll", "view.markdownPreview.copy", "view.markdownPreview.copyMarkdown"]
        {
            bind(action, action, none(), global, &[]);
        }
        let markdown = Context::Markdown;
        bind("markdown.format.bold", "markdown.format.bold", none(), markdown, &["⌘B"]);
        bind("markdown.format.italic", "markdown.format.italic", none(), markdown, &["⌘I"]);
        bind("markdown.format.strikethrough", "markdown.format.strikethrough", none(), markdown, &["⇧⌘S"]);
        bind("markdown.format.code", "markdown.format.code", none(), markdown, &["⇧⌘C"]);
        bind("markdown.format.link", "markdown.format.link", none(), markdown, &["⇧⌘U"]);
        bind("view.commandLineLauncher.open", "view.commandLineLauncher.open", none(), global, &[]);
        for action in [
            "java.bytecode.show",
            "java.jshell.open",
            "java.jshell.restart",
            "java.jshell.stop",
            "kotlin.repl.open",
            "kotlin.repl.restart",
            "kotlin.repl.stop",
        ] {
            bind(action, action, none(), global, &[]);
        }
        bind("java.bytecode.show/kotlin", "java.bytecode.show", none(), global, &[]);
        bind("view.artifacts.build", "view.artifacts.build", none(), global, &[]);
        bind("view.dataSource.open", "view.dataSource.open", none(), global, &[]);
        bind("view.fileTemplates.new", "view.fileTemplates.new", none(), global, &[]);
        bind("view.fileTemplates.saveAs", "view.fileTemplates.saveAs", none(), global, &[]);
        bind("view.newElement.open", "view.newElement.open", none(), global, &["⌘N"]);
        let moves: [(&str, &[&str], &[&str]); 12] = [
            ("left", &["←"], &["⇧←"]),
            ("right", &["→"], &["⇧→"]),
            ("wordLeft", &["⌥←"], &["⌥⇧←"]),
            ("wordRight", &["⌥→"], &["⌥⇧→"]),
            ("up", &["↑"], &["⇧↑"]),
            ("down", &["↓"], &["⇧↓"]),
            ("lineStart", &["⌘←", "Home"], &["⇧⌘←", "⇧Home"]),
            ("lineEnd", &["⌘→", "End"], &["⇧⌘→", "⇧End"]),
            ("documentStart", &["⌘Home"], &["⇧⌘↑", "⇧⌘Home"]),
            ("documentEnd", &["⌘↓"], &["⇧⌘↓"]),
            ("pageUp", &["PageUp"], &["⇧PageUp"]),
            ("pageDown", &["PageDown"], &["⇧PageDown"]),
        ];
        for (kind, plain, extended) in moves {
            let movement =
                if kind.starts_with("page") { json!({ "kind": kind, "lines": 0 }) } else { json!({ "kind": kind }) };
            bind(
                &format!("editor.caret.move/{kind}"),
                "editor.caret.move",
                json!({ "movement": movement, "extend": false }),
                editor,
                plain,
            );
            bind(
                &format!("editor.caret.move/{kind}+extend"),
                "editor.caret.move",
                json!({ "movement": movement, "extend": true }),
                editor,
                extended,
            );
        }
        bind("editor.edit.backspace", "editor.edit.backspace", none(), editor, &["⌫", "⇧⌫"]);
        bind("editor.edit.deleteWordBackward", "editor.edit.deleteWordBackward", none(), editor, &["⌥⌫"]);
        bind("editor.edit.deleteForward", "editor.edit.deleteForward", none(), editor, &["⌦"]);
        bind("editor.edit.deleteWordForward", "editor.edit.deleteWordForward", none(), editor, &["⌥⌦"]);
        bind("editor.edit.newline", "editor.edit.newline", none(), editor, &["⏎"]);
        bind("editor.edit.tab", "editor.edit.tab", none(), editor, &["⇥"]);
        bind("editor.line.outdent", "editor.line.outdent", none(), editor, &["⇧⇥"]);
        bind("editor.selection.selectAll", "editor.selection.selectAll", none(), editor, &["⌘A"]);
        bind("editor.selection.extend", "editor.selection.extend", none(), editor, &["⌥↑"]);
        bind("editor.selection.shrink", "editor.selection.shrink", none(), editor, &["⌥↓"]);
        bind("editor.caret.keepPrimary", "editor.caret.keepPrimary", none(), editor, &["⎋"]);
        bind("editor.caret.cloneAbove", "editor.caret.cloneAbove", none(), editor, &[]);
        bind("editor.caret.cloneBelow", "editor.caret.cloneBelow", none(), editor, &[]);
        bind("editor.selection.columnMode", "editor.selection.columnMode", none(), editor, &["⇧⌘8"]);
        bind("editor.macro.playLast", "editor.macro.playLast", none(), global, &[]);
        bind("view.macro.toggleRecording", "view.macro.toggleRecording", none(), global, &[]);
        bind("view.macro.edit", "view.macro.edit", none(), global, &[]);
        bind("view.macro.playSaved", "view.macro.playSaved", none(), global, &[]);
        bind("editor.softWrap.toggle", "editor.softWrap.toggle", none(), editor, &[]);
        bind("editor.fontSize.increase", "editor.fontSize.increase", none(), editor, &[]);
        bind("editor.fontSize.decrease", "editor.fontSize.decrease", none(), editor, &[]);
        bind("editor.fontSize.reset", "editor.fontSize.reset", none(), editor, &[]);
        bind("view.editorFontSize.increase", "view.editorFontSize.increase", none(), global, &["⌃⇧."]);
        bind("view.editorFontSize.decrease", "view.editorFontSize.decrease", none(), global, &["⌃⇧,"]);
        bind("view.editorFontSize.reset", "view.editorFontSize.reset", none(), global, &[]);
        bind("editor.whitespaces.toggle", "editor.whitespaces.toggle", none(), editor, &[]);
        bind("editor.indentGuides.toggle", "editor.indentGuides.toggle", none(), editor, &[]);
        bind("editor.lineNumbers.toggle", "editor.lineNumbers.toggle", none(), editor, &[]);
        bind("editor.line.duplicate", "editor.line.duplicate", none(), editor, &["⌘D"]);
        bind("editor.line.delete", "editor.line.delete", none(), editor, &["⌘⌫"]);
        bind("editor.line.moveUp", "editor.line.moveUp", none(), editor, &["⌥⇧↑"]);
        bind("editor.line.moveDown", "editor.line.moveDown", none(), editor, &["⌥⇧↓"]);
        bind("editor.comment.line", "editor.comment.line", none(), editor, &["⌘/"]);
        bind("editor.comment.block", "editor.comment.block", none(), editor, &["⌥⌘/"]);
        bind("editor.selection.addNextOccurrence", "editor.selection.addNextOccurrence", none(), editor, &["⌃G"]);
        bind("editor.selection.unselectOccurrence", "editor.selection.unselectOccurrence", none(), editor, &["⌃⇧G"]);
        bind(
            "editor.selection.selectAllOccurrences",
            "editor.selection.selectAllOccurrences",
            none(),
            editor,
            &["⌃⌘G"],
        );
        Self { bindings, user: BTreeMap::new() }
    }

    #[must_use]
    pub fn with_macros<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.bindings.retain(|binding| !binding.action.starts_with(MACRO_ACTION));
        for name in names {
            self.bindings.push(Binding {
                action: macro_action(name),
                command: "editor.macro.play".to_owned(),
                params: json!({ "name": name }),
                context: Context::Global,
                keystrokes: Vec::new(),
            });
        }
        self
    }

    #[must_use]
    pub fn with_bindings(mut self, extra: impl IntoIterator<Item = Binding>) -> Self {
        for binding in extra {
            if !self.bindings.iter().any(|known| known.action == binding.action) {
                self.bindings.push(binding);
            }
        }
        self
    }

    pub fn bindings(&self) -> Vec<Binding> {
        let defaults = self.bindings.iter().map(|binding| match self.user.get(&binding.action) {
            Some(keys) => Binding { keystrokes: keys.clone(), ..binding.clone() },
            None => binding.clone(),
        });
        let added = self
            .user
            .iter()
            .filter(|(action, keys)| {
                !keys.is_empty() && !self.bindings.iter().any(|binding| binding.action == **action)
            })
            .map(|(action, keys)| Binding {
                action: action.clone(),
                command: action.clone(),
                params: json!({}),
                context: Context::Global,
                keystrokes: keys.clone(),
            });
        defaults.chain(added).collect()
    }

    pub fn has_default(&self, action: &str) -> bool {
        self.bindings.iter().any(|binding| binding.action == action)
    }

    pub fn lookup(&self, keystroke: &Keystroke, context: Context) -> Option<Binding> {
        let bindings = self.bindings();
        let find = |wanted: Context| {
            bindings.iter().find(|binding| binding.context == wanted && binding.keystrokes.contains(keystroke)).cloned()
        };
        match context {
            Context::Global => find(Context::Global),
            Context::Editor => find(context).or_else(|| find(Context::Global)),
            Context::Database => find(context).or_else(|| find(Context::Global)),
            Context::Markdown => find(context).or_else(|| find(Context::Global)),
            Context::Terminal => find(context).or_else(|| {
                find(Context::Global).filter(|binding| {
                    keystroke.command
                        || matches!(binding.action.as_str(), "view.toolWindow.terminal" | "view.switcher.open")
                })
            }),
        }
    }

    pub fn starts_sequence(&self, first: &Keystroke, context: Context) -> bool {
        let begins = |stroke: &Keystroke| {
            stroke.then.is_some()
                && (stroke.control, stroke.alt, stroke.shift, stroke.command, stroke.key.as_str())
                    == (first.control, first.alt, first.shift, first.command, first.key.as_str())
        };
        self.bindings()
            .iter()
            .filter(|binding| binding.context == context)
            .any(|binding| binding.keystrokes.iter().any(begins))
    }

    pub fn keystrokes(&self, action: &str) -> Vec<Keystroke> {
        self.bindings()
            .into_iter()
            .find(|binding| binding.action == action)
            .map(|binding| binding.keystrokes)
            .unwrap_or_default()
    }

    pub fn shortcut(&self, action: &str) -> Option<String> {
        self.keystrokes(action).first().map(ToString::to_string)
    }

    pub fn set(&mut self, action: &str, keystrokes: Option<Vec<Keystroke>>) {
        match keystrokes {
            Some(keys) => {
                self.user.insert(action.to_owned(), keys);
            }
            None => {
                self.user.remove(action);
            }
        }
    }

    pub fn actions_with(&self, keystroke: &Keystroke) -> Vec<String> {
        self.bindings()
            .into_iter()
            .filter(|binding| binding.keystrokes.contains(keystroke))
            .map(|binding| binding.action)
            .collect()
    }

    pub fn is_changed(&self, action: &str) -> bool {
        self.user.contains_key(action)
    }

    pub fn user_changes(&self) -> &BTreeMap<String, Vec<Keystroke>> {
        &self.user
    }

    pub fn with_user_changes(mut self, user: BTreeMap<String, Vec<Keystroke>>) -> Self {
        self.user = user;
        self
    }

    pub fn conflicts(&self) -> Vec<(Keystroke, Vec<String>)> {
        let mut seen: BTreeMap<(Context, Keystroke), Vec<String>> = BTreeMap::new();
        for binding in self.bindings() {
            for keystroke in &binding.keystrokes {
                seen.entry((binding.context, keystroke.clone())).or_default().push(binding.action.clone());
            }
        }
        seen.into_iter()
            .filter(|(_, actions)| actions.len() > 1)
            .map(|((_, keystroke), actions)| (keystroke, actions))
            .collect()
    }
}

pub const MACOS_KEYMAP: &str = "macos";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnmappedReason {
    UnknownAction,
    SecondKeystroke,
    MouseShortcut,
    UnknownKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unmapped {
    pub action_id: String,
    pub reason: UnmappedReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportWarning {
    ForeignParent { parent: Option<String> },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapImport {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub shortcuts: BTreeMap<String, Vec<Keystroke>>,
    pub mapped: usize,
    pub unmapped: Vec<Unmapped>,
    #[serde(default)]
    pub warnings: Vec<ImportWarning>,
}

pub trait KeymapImporter: Send + Sync {
    fn detects(&self, text: &str) -> bool;

    fn import(&self, text: &str) -> Result<KeymapImport, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_read_and_print_as_on_macos() {
        let stroke = Keystroke::parse("⇧⌘F").unwrap();
        assert!(stroke.shift && stroke.command && !stroke.alt);
        assert_eq!(stroke.key, "F");
        assert_eq!(stroke.to_string(), "⇧⌘F");
        assert_eq!(Keystroke::parse("⌥⌘/").unwrap().key, "Slash");
        assert_eq!(Keystroke::parse("⌥⌘/").unwrap().to_string(), "⌥⌘/");
        assert_eq!(Keystroke::parse("⌘1").unwrap().key, "Num1");
        assert_eq!(Keystroke::parse("⌘1").unwrap().to_string(), "⌘1");
        assert_eq!(Keystroke::parse("⌥F12").unwrap().key, "F12");
        assert_eq!(Keystroke::parse("⇧Home").unwrap().to_string(), "⇧Home");
        assert!(Keystroke::parse("⌘").is_none());
        assert!(Keystroke::parse("bad⌘").is_none(), "not a key");
        assert!(Keystroke::parse("⌘F36").is_none());
        assert_eq!(Keystroke::parse("⌃Space").unwrap().key, "Space");
    }

    #[test]
    fn every_default_keystroke_reads_back_from_its_text() {
        for binding in Keymap::default_macos().bindings() {
            for stroke in binding.keystrokes {
                let json = serde_json::to_string(&stroke).unwrap();
                assert_eq!(serde_json::from_str::<Keystroke>(&json).unwrap(), stroke, "{json}");
            }
        }
    }

    #[test]
    fn exact_lookup_with_contexts() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| binding.action)
        };
        assert_eq!(find("⌘F", Context::Global).as_deref(), Some("view.find.open"));
        assert_eq!(find("⇧⌘F", Context::Global).as_deref(), Some("view.findInFiles.open"), "no logical matching");
        assert_eq!(find("⌘D", Context::Global), None, "an editor action is not global");
        assert_eq!(find("⌘D", Context::Editor).as_deref(), Some("editor.line.duplicate"));
        assert_eq!(
            find("⌘S", Context::Editor).as_deref(),
            Some("file.document.saveAll"),
            "global actions work in the editor"
        );
        assert_eq!(find("⌥⇧←", Context::Editor).as_deref(), Some("editor.caret.move/wordLeft+extend"));
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn the_terminal_takes_new_tab_and_close_from_the_window() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| binding.action)
        };
        assert_eq!(find("⌘T", Context::Terminal).as_deref(), Some("terminal.session.new"));
        assert_eq!(find("⌘T", Context::Global).as_deref(), Some("view.vcs.update"));
        assert_eq!(find("⌘W", Context::Terminal).as_deref(), Some("terminal.session.close"));
        assert_eq!(find("⌘W", Context::Global).as_deref(), Some("file.document.close"));
        assert_eq!(find("⌥⇥", Context::Terminal).as_deref(), Some("terminal.session.focusNext"));
        assert_eq!(find("⌘1", Context::Terminal).as_deref(), Some("view.toolWindow.project"));
        assert_eq!(find("⌘T", Context::Editor).as_deref(), Some("view.vcs.update"));
        assert_eq!(find("⌃D", Context::Global).as_deref(), Some("debug.session.start"));
        for shell in ["⌃D", "⌃R", "⌃⇧R", "F7", "F8", "⇧F6", "⌃I", "⇥", "⇧⇥"] {
            assert_eq!(find(shell, Context::Terminal), None, "{shell} goes to the shell");
        }
        assert_eq!(find("⌥F12", Context::Terminal).as_deref(), Some("view.toolWindow.terminal"));
        assert_eq!(find("⌃⇥", Context::Terminal).as_deref(), Some("view.switcher.open"));
        assert_eq!(find("⌃⇧⇥", Context::Editor).as_deref(), Some("view.switcher.open"));
        assert_eq!(GROUPS[group_of("view.switcher.open")], "keymap-group-navigate");
        assert_eq!(group_of("terminal.session.split/right"), 1);
    }

    #[test]
    fn tool_window_layout_shortcuts() {
        let keymap = Keymap::default_macos();
        let find =
            |text: &str| keymap.lookup(&Keystroke::parse(text).unwrap(), Context::Editor).map(|binding| binding.action);
        assert_eq!(find("⇧⎋").as_deref(), Some("view.toolWindow.hide"));
        assert_eq!(find("⇧F12").as_deref(), Some("view.layout.restore"));
        assert_eq!(find("⇧⌘F12").as_deref(), Some("view.toolWindow.hideAll"));
        let global =
            |text: &str| keymap.lookup(&Keystroke::parse(text).unwrap(), Context::Global).map(|binding| binding.action);
        assert_eq!(find("⌃⌥←").as_deref(), Some("view.toolWindow.resize/left"));
        assert_eq!(global("⌃⌥↓").as_deref(), Some("view.toolWindow.resize/down"));
        let resize =
            keymap.bindings().into_iter().find(|binding| binding.action == "view.toolWindow.resize/up").unwrap();
        assert_eq!(resize.command, "view.toolWindow.resize");
        assert_eq!(resize.params, json!({ "direction": "up" }));
        for action in [
            "view.toolWindow.hide",
            "view.toolWindow.hideAll",
            "view.toolWindow.move",
            "view.toolWindow.resize/right",
            "view.layout.restore",
            "view.layout.restoreDefault",
            "view.toolWindow.viewMode",
            "view.toolWindow.viewMode/float",
            "view.layout.saveAsNew",
            "view.layout.apply",
            "view.layout.saveChanges",
            "view.layout.rename",
            "view.layout.delete",
        ] {
            assert_eq!(GROUPS[group_of(action)], "keymap-group-window", "{action}");
        }
        let float =
            keymap.bindings().into_iter().find(|binding| binding.action == "view.toolWindow.viewMode/window").unwrap();
        assert_eq!(float.command, "view.toolWindow.viewMode");
        assert_eq!(float.params, json!({ "mode": "window" }));
        assert!(float.keystrokes.is_empty(), "the view modes have no shortcuts");
    }

    #[test]
    fn navigation_shortcuts_and_groups() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| binding.action)
        };
        assert_eq!(find("⌘B", Context::Editor).as_deref(), Some("navigate.declaration.goTo"));
        assert_eq!(find("⌥⌘B", Context::Editor).as_deref(), Some("navigate.implementation.goTo"));
        assert_eq!(find("⇧⌘B", Context::Editor).as_deref(), Some("navigate.typeDeclaration.goTo"));
        assert_eq!(find("⌃⇧B", Context::Editor).as_deref(), Some("navigate.typeDeclaration.goTo"));
        assert_eq!(find("⌘U", Context::Editor).as_deref(), Some("navigate.superMethod.goTo"));
        assert_eq!(find("⇧⌘T", Context::Editor).as_deref(), Some("navigate.test.goTo"));
        assert_eq!(find("⌥F7", Context::Editor).as_deref(), Some("navigate.usages.find"));
        assert_eq!(find("⌘F7", Context::Editor).as_deref(), Some("navigate.usages.findInFile"));
        assert_eq!(find("⌥⌘F7", Context::Editor).as_deref(), Some("navigate.usages.show"));
        assert_eq!(find("⌥⇧⌘F7", Context::Editor).as_deref(), Some("navigate.usages.settings"));
        assert_eq!(find("⌃⌥H", Context::Editor).as_deref(), Some("navigate.hierarchy.call"));
        assert_eq!(find("⌃H", Context::Editor).as_deref(), Some("navigate.hierarchy.type"));
        assert_eq!(keymap.shortcut("view.toolWindow.hierarchy"), None, "Hierarchy has no shortcut");
        assert_eq!(group_of("view.toolWindow.hierarchy"), 1);
        assert_eq!(find("⇧⌘F7", Context::Editor).as_deref(), Some("editor.highlightUsages.toggle"));
        assert_eq!(find("⌃⌥↓", Context::Editor).as_deref(), Some("editor.highlightUsages.next"));
        assert_eq!(find("⌃⌥↑", Context::Editor).as_deref(), Some("editor.highlightUsages.previous"));
        assert_eq!(find("⌥↑", Context::Editor).as_deref(), Some("editor.selection.extend"));
        assert_eq!(find("⌥↓", Context::Editor).as_deref(), Some("editor.selection.shrink"));
        assert_eq!(GROUPS[group_of("editor.selection.extend")], "keymap-group-edit");
        assert_eq!(GROUPS[group_of("editor.selection.shrink")], "keymap-group-edit");
        assert_eq!(find("⌃J", Context::Editor).as_deref(), Some("editor.documentation.show"));
        assert_eq!(find("F1", Context::Editor).as_deref(), Some("editor.documentation.show"));
        assert_eq!(find("⌥F1", Context::Editor).as_deref(), Some("view.selectIn.open"));
        assert_eq!(find("⌘P", Context::Editor).as_deref(), Some("editor.parameterInfo.show"));
        assert_eq!(find("⌘[", Context::Editor).as_deref(), Some("navigate.history.back"));
        assert_eq!(find("⌘]", Context::Global).as_deref(), Some("navigate.history.forward"));
        assert_eq!(find("⇧⌘⌫", Context::Editor).as_deref(), Some("navigate.edit.last"));
        assert_eq!(keymap.shortcut("navigate.edit.next"), None);
        assert_eq!(find("⌘3", Context::Global).as_deref(), Some("view.toolWindow.find"));
        assert_eq!(keymap.shortcut("navigate.history.back").as_deref(), Some("⌘["));
        assert_eq!(group_of("view.toolWindow.find"), 1, "a tool window, not the search bar");
        assert_eq!(find("⌘7", Context::Global).as_deref(), Some("view.toolWindow.structure"));
        assert_eq!(find("⌘7", Context::Editor).as_deref(), Some("view.toolWindow.structure"));
        assert_eq!(group_of("view.toolWindow.structure"), 1);
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
        assert_eq!(group_of("navigate.usages.find"), 2);
    }

    #[test]
    fn appearance_modes_and_the_ide_zoom_have_no_shortcut_but_full_screen() {
        let keymap = Keymap::default_macos();
        let find =
            |text: &str| keymap.lookup(&Keystroke::parse(text).unwrap(), Context::Global).map(|binding| binding.action);
        assert_eq!(find("⌃⌘F").as_deref(), Some("view.fullScreen.toggle"));
        let modes = ["view.presentationMode.toggle", "view.distractionFreeMode.toggle", "view.zenMode.toggle"];
        let zoom = ["view.ideZoom.open", "view.ideZoom.in", "view.ideZoom.out", "view.ideZoom.reset"];
        for action in modes.into_iter().chain(zoom) {
            assert!(keymap.bindings().iter().any(|binding| binding.action == action), "{action}");
            assert!(keymap.keystrokes(action).is_empty(), "{action}");
            assert_eq!(GROUPS[group_of(action)], "keymap-group-window", "{action}");
        }
        assert_eq!(GROUPS[group_of("view.fullScreen.toggle")], "keymap-group-window");
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn jump_to_navigation_bar_takes_command_up_and_option_home() {
        let keymap = Keymap::default_macos();
        for (key, context) in [("⌘↑", Context::Global), ("⌘↑", Context::Editor), ("⌥Home", Context::Editor)] {
            let found = keymap.lookup(&Keystroke::parse(key).unwrap(), context).map(|binding| binding.action);
            assert_eq!(found.as_deref(), Some("view.navigationBar.jump"), "{key} {context:?}");
        }
        let start = keymap.lookup(&Keystroke::parse("⌘Home").unwrap(), Context::Editor).map(|binding| binding.action);
        assert_eq!(start.as_deref(), Some("editor.caret.move/documentStart"), "Move Caret to Text Start keeps ⌘Home");
        assert_eq!(GROUPS[group_of("view.navigationBar.jump")], "keymap-group-navigate");
        for action in [
            "view.navigationBar.top",
            "view.navigationBar.statusBar",
            "view.navigationBar.hide",
            "view.navigationBar.members.toggle",
        ] {
            assert!(keymap.keystrokes(action).is_empty(), "{action}");
            assert_eq!(GROUPS[group_of(action)], "keymap-group-window", "{action}");
        }
    }

    #[test]
    fn jump_to_source_takes_f4_and_command_down_without_taking_the_editors_keys() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| binding.action)
        };
        assert_eq!(keymap.shortcut("view.vcs.jumpToSource").as_deref(), Some("F4"));
        assert_eq!(find("F4", Context::Global).as_deref(), Some("view.vcs.jumpToSource"));
        assert_eq!(find("⌘↓", Context::Global).as_deref(), Some("view.vcs.jumpToSource"));
        assert_eq!(find("⌘↓", Context::Editor).as_deref(), Some("editor.caret.move/documentEnd"));
        assert_eq!(find("F4", Context::Database).as_deref(), Some("db.sql.navigate"));
        assert_eq!(GROUPS[group_of("view.vcs.jumpToSource")], "keymap-group-vcs");
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn the_todo_tool_window_has_no_shortcut_and_occurrences_have_theirs() {
        let keymap = Keymap::default_macos();
        let find =
            |text: &str| keymap.lookup(&Keystroke::parse(text).unwrap(), Context::Global).map(|binding| binding.action);
        assert!(keymap.keystrokes("view.toolWindow.todo").is_empty());
        assert_eq!(find("⌘6").as_deref(), Some("view.toolWindow.problems"));
        assert_eq!(find("⌥⌘↑").as_deref(), Some("view.occurrence.previous"));
        assert_eq!(find("⌥⌘↓").as_deref(), Some("view.occurrence.next"));
        assert_eq!(GROUPS[group_of("view.occurrence.next")], "keymap-group-navigate");
        assert_eq!(GROUPS[group_of("view.toolWindow.todo")], "keymap-group-window");
        assert_eq!(GROUPS[group_of("view.todo.previewSource.toggle")], "keymap-group-window");
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn quick_switch_scheme_is_control_backtick_and_free_of_conflicts() {
        let keymap = Keymap::default_macos();
        let stroke = Keystroke::parse("⌃`").unwrap();
        assert_eq!(stroke.key, "Backtick");
        assert_eq!(stroke.to_string(), "⌃`");
        assert_eq!(Keystroke::parse("⌃Backtick"), Some(stroke.clone()));
        for context in [Context::Global, Context::Editor] {
            let found = keymap.lookup(&stroke, context).map(|binding| binding.action);
            assert_eq!(found.as_deref(), Some("view.quickSwitchScheme.open"), "{context:?}");
        }
        assert_eq!(keymap.lookup(&stroke, Context::Terminal), None, "a terminal keeps its control keys");
        assert_eq!(keymap.actions_with(&stroke), ["view.quickSwitchScheme.open"]);
        assert_eq!(keymap.shortcut("view.quickSwitchScheme.open").as_deref(), Some("⌃`"));
        assert_eq!(GROUPS[group_of("view.quickSwitchScheme.open")], "keymap-group-window");
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn show_code_authors_has_a_shortcut_of_its_own() {
        let keymap = Keymap::default_macos();
        let stroke = Keystroke::parse("⌃⌥A").unwrap();
        for context in [Context::Global, Context::Editor] {
            let found = keymap.lookup(&stroke, context).map(|binding| binding.action);
            assert_eq!(found.as_deref(), Some("view.codeVision.authors.toggle"), "{context:?}");
        }
        assert_eq!(keymap.shortcut("view.codeVision.authors.toggle").as_deref(), Some("⌃⌥A"));
        assert_eq!(GROUPS[group_of("view.codeVision.authors.toggle")], "keymap-group-window");
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn font_size_in_all_editors_has_shortcuts_and_the_active_editor_none() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| binding.action)
        };
        for context in [Context::Global, Context::Editor] {
            assert_eq!(find("⌃⇧.", context).as_deref(), Some("view.editorFontSize.increase"), "{context:?}");
            assert_eq!(find("⌃⇧,", context).as_deref(), Some("view.editorFontSize.decrease"), "{context:?}");
        }
        assert_eq!(keymap.shortcut("view.editorFontSize.increase").as_deref(), Some("⌃⇧."));
        for action in [
            "view.editorFontSize.reset",
            "editor.fontSize.increase",
            "editor.fontSize.decrease",
            "editor.fontSize.reset",
        ] {
            assert!(keymap.keystrokes(action).is_empty(), "{action}");
        }
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn save_all_open_and_go_to_class_shortcuts() {
        let keymap = Keymap::default_macos();
        let find =
            |text: &str| keymap.lookup(&Keystroke::parse(text).unwrap(), Context::Global).map(|binding| binding.action);
        assert_eq!(find("⌘S").as_deref(), Some("file.document.saveAll"));
        assert_eq!(find("⌘O").as_deref(), Some("view.gotoClass.open"));
        assert_eq!(find("⌥⌘N").as_deref(), Some("view.vcs.newBranch"));
        assert_eq!(find("⌥⌘S"), None);
        assert!(keymap.keystrokes("file.document.save").is_empty(), "Save has no shortcut");
        assert!(keymap.keystrokes("view.project.open").is_empty(), "Open… has no shortcut");
        assert_eq!(group_of("view.gotoClass.open"), 2);
        assert_eq!(find("⌥⌘O").as_deref(), Some("view.gotoSymbol.open"));
        assert_eq!(find("⇧⌘O").as_deref(), Some("view.gotoFile.open"));
        assert_eq!(group_of("view.gotoSymbol.open"), 2);
        assert_eq!(find("⌥⇧⌘E").as_deref(), Some("view.gotoText.open"));
        assert_eq!(group_of("view.gotoText.open"), 2);
    }

    #[test]
    fn refactoring_and_formatting_shortcuts() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| binding.action)
        };
        assert_eq!(find("⇧F6", Context::Global).as_deref(), Some("view.rename.open"));
        assert_eq!(find("⇧F6", Context::Editor).as_deref(), Some("view.rename.open"));
        assert_eq!(keymap.shortcut("view.rename.open").as_deref(), Some("⇧F6"));
        assert_eq!(find("⌥⌘L", Context::Editor).as_deref(), Some("editor.code.reformat"));
        assert_eq!(find("⌃⌥O", Context::Editor).as_deref(), Some("editor.imports.optimize"));
        assert_eq!(find("⌥⌘L", Context::Global), None, "formatting is the editor's");
        assert_eq!(find("⌥⏎", Context::Editor).as_deref(), Some("view.intentions.open"));
        assert_eq!(find("⌘N", Context::Editor).as_deref(), Some("view.generate.open"));
        assert_eq!(find("⌃O", Context::Editor).as_deref(), Some("view.overrideMethods.open"));
        assert_eq!(find("⌃I", Context::Editor).as_deref(), Some("view.implementMethods.open"));
        assert_eq!(find("⌘N", Context::Global).as_deref(), Some("view.newElement.open"), "New… outside the editor");
        assert_eq!(find("⏎", Context::Editor).as_deref(), Some("editor.edit.newline"));
        assert_eq!(find("⇧⌘⌦", Context::Editor).as_deref(), Some("view.unwrap.open"));
        assert_eq!(find("⌘⌦", Context::Editor).as_deref(), Some("view.safeDelete.open"));
        assert_eq!(find("⌘F6", Context::Editor).as_deref(), Some("view.changeSignature.open"));
        assert_eq!(find("F6", Context::Editor).as_deref(), Some("view.project.move"), "Move in the editor too");
        assert_eq!(
            find("⌘⌦", Context::Global).as_deref(),
            Some("view.safeDelete.open"),
            "in the Project tool window too"
        );
        assert_ne!(find("⇧⌘⌫", Context::Editor).as_deref(), Some("view.unwrap.open"), "forward Delete, not Backspace");
        for (keys, action) in [
            ("⌥⌘V", "view.extract.variable"),
            ("⌥⌘C", "view.extract.constant"),
            ("⌥⌘F", "view.extract.field"),
            ("⌥⌘P", "view.extract.parameter"),
            ("⌥⌘M", "view.extract.method"),
            ("⌥⌘N", "view.inline.open"),
        ] {
            assert_eq!(find(keys, Context::Editor).as_deref(), Some(action));
        }
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
        for action in [
            "view.rename.open",
            "view.intentions.open",
            "view.generate.open",
            "view.overrideMethods.open",
            "view.implementMethods.open",
            "view.unwrap.open",
            "view.extract.variable",
            "view.inline.open",
            "editor.code.reformat",
            "editor.imports.optimize",
            "refactor.rename.apply",
        ] {
            assert_eq!(GROUPS[group_of(action)], "keymap-group-refactor", "{action}");
        }
    }

    #[test]
    fn run_shortcuts_and_their_group() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| (binding.action, binding.params))
        };
        assert_eq!(find("⌃R", Context::Global), Some(("run.process.start".to_owned(), json!({}))));
        assert_eq!(find("⌃R", Context::Editor).map(|found| found.0).as_deref(), Some("run.process.start"));
        assert_eq!(
            find("⌃⇧R", Context::Editor),
            Some(("run.process.start/context".to_owned(), json!({ "context": {} })))
        );
        assert_eq!(find("⌘F2", Context::Global).map(|found| found.0).as_deref(), Some("run.process.stop"));
        assert_eq!(find("⌘4", Context::Editor).map(|found| found.0).as_deref(), Some("view.toolWindow.run"));
        assert_eq!(find("F2", Context::Editor).map(|found| found.0).as_deref(), Some("editor.problem.next"));
        assert!(keymap.keystrokes("view.runConfigurations.open").is_empty());
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
        for action in
            ["run.process.start", "run.process.start/context", "run.process.stop", "view.runConfigurations.open"]
        {
            assert_eq!(GROUPS[group_of(action)], "keymap-group-run", "{action}");
        }
        assert_eq!(GROUPS[group_of("view.toolWindow.run")], "keymap-group-window");
    }

    #[test]
    fn run_anything_is_two_controls_in_other_and_the_choosers_are_in_run() {
        let keymap = Keymap::default_macos();
        let twice = Keystroke::parse("⌃⌃").unwrap();
        assert!(twice.is_double());
        assert_eq!(twice.to_string(), "⌃⌃");
        assert_eq!(
            keymap.lookup(&twice, Context::Editor).map(|found| found.action).as_deref(),
            Some("view.runAnything.open")
        );
        assert_eq!(GROUPS[group_of("view.runAnything.open")], "keymap-group-other");
        for text in ["⇧⇧", "⌥⌥", "⌘⌘"] {
            let stroke = Keystroke::parse(text).unwrap();
            assert!(stroke.is_double(), "{text}");
            assert_eq!(stroke.to_string(), text);
            assert_eq!(serde_json::to_value(&stroke).unwrap(), json!(text));
        }
        for text in ["⌃⇧", "⌃⌃⌃", "⌃"] {
            assert_eq!(Keystroke::parse(text), None, "{text}");
        }
        let control = Keystroke::parse("⌃C").unwrap();
        assert!(!control.is_double());
        let find = |text: &str| {
            keymap
                .lookup(&Keystroke::parse(text).unwrap(), Context::Global)
                .map(|binding| (binding.action, binding.params))
        };
        assert_eq!(find("⌃⌥R"), Some(("view.runConfigurations.choose".to_owned(), json!({}))));
        assert_eq!(find("⌃⌥D"), Some(("view.runConfigurations.choose/debug".to_owned(), json!({ "debug": true }))));
        for action in ["view.runConfigurations.choose", "view.runConfigurations.choose/debug"] {
            assert_eq!(GROUPS[group_of(action)], "keymap-group-run", "{action}");
        }
    }

    #[test]
    fn macros_have_no_default_shortcuts_and_saved_ones_are_actions_of_their_own() {
        let keymap = Keymap::default_macos();
        for action in ["editor.macro.playLast", "view.macro.toggleRecording", "view.macro.edit", "view.macro.playSaved"]
        {
            assert!(keymap.bindings().iter().any(|binding| binding.action == action), "{action}");
            assert!(keymap.keystrokes(action).is_empty(), "{action} has no default shortcut");
            assert_eq!(group_of(action), 5, "{action} is an editing action");
        }
        let mut keymap = keymap.with_macros(["Wrap", "old"]).with_macros(["Wrap", "Log it"]);
        let saved: Vec<(String, Value)> = keymap
            .bindings()
            .into_iter()
            .filter(|binding| binding.action.starts_with(MACRO_ACTION))
            .map(|binding| (binding.action, binding.params))
            .collect();
        assert_eq!(
            saved,
            [
                ("editor.macro.play/Wrap".to_owned(), json!({ "name": "Wrap" })),
                ("editor.macro.play/Log it".to_owned(), json!({ "name": "Log it" }))
            ]
        );
        assert_eq!(group_of(&macro_action("Wrap")), 12);
        assert_eq!(GROUPS[group_of(&macro_action("Wrap"))], "keymap-group-macros");
        keymap.set(&macro_action("Wrap"), Some(vec![Keystroke::parse("⌃⌥W").unwrap()]));
        let found = keymap.lookup(&Keystroke::parse("⌃⌥W").unwrap(), Context::Editor).unwrap();
        assert_eq!((found.command.as_str(), found.params), ("editor.macro.play", json!({ "name": "Wrap" })));
    }

    #[test]
    fn external_tools_are_extra_actions_of_their_own_group() {
        let action = format!("{EXTERNAL_TOOL_ACTION}lint");
        let tool = Binding {
            action: action.clone(),
            command: "view.externalTools.run".to_owned(),
            params: json!({ "toolId": "lint" }),
            context: Context::Global,
            keystrokes: Vec::new(),
        };
        let duplicate = Binding { action: "run.process.start".to_owned(), ..tool.clone() };
        let mut keymap = Keymap::default_macos().with_bindings([tool.clone(), duplicate]);
        assert_eq!(GROUPS[group_of(&action)], "keymap-group-external-tools");
        assert_eq!(keymap.bindings().iter().filter(|binding| binding.action == "run.process.start").count(), 1);
        assert!(keymap.keystrokes(&action).is_empty());
        keymap.set(&action, Some(vec![Keystroke::parse("⌃⌥L").unwrap()]));
        let found = keymap.lookup(&Keystroke::parse("⌃⌥L").unwrap(), Context::Editor).unwrap();
        assert_eq!((found.command.as_str(), found.params), ("view.externalTools.run", json!({ "toolId": "lint" })));
    }

    #[test]
    fn debug_shortcuts_and_their_group() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| (binding.action, binding.params))
        };
        let action = |text: &str, context| find(text, context).map(|found| found.0);
        assert_eq!(find("⌃D", Context::Global), Some(("debug.session.start".to_owned(), json!({}))));
        assert_eq!(
            find("⌃⇧D", Context::Editor),
            Some(("debug.session.start/context".to_owned(), json!({ "context": {} })))
        );
        assert_eq!(action("F8", Context::Global).as_deref(), Some("debug.execution.stepOver"));
        assert_eq!(action("F7", Context::Editor).as_deref(), Some("debug.execution.stepInto"));
        assert_eq!(action("⇧F8", Context::Global).as_deref(), Some("debug.execution.stepOut"));
        assert_eq!(action("⌥⌘R", Context::Global).as_deref(), Some("debug.execution.resume"));
        assert_eq!(action("⌥F9", Context::Editor).as_deref(), Some("debug.execution.runToCursor"));
        assert_eq!(action("⌥⌘F9", Context::Editor).as_deref(), Some("debug.execution.forceRunToCursor"));
        assert_eq!(action("⌥⇧F8", Context::Global).as_deref(), Some("debug.execution.forceStepOver"));
        assert_eq!(action("⌥⇧F7", Context::Editor).as_deref(), Some("debug.execution.forceStepInto"));
        assert_eq!(action("⇧F7", Context::Global).as_deref(), Some("view.smartStepInto.open"));
        assert!(keymap.keystrokes("debug.frame.reset").is_empty());
        assert_eq!(action("⌘F8", Context::Editor).as_deref(), Some("debug.breakpoint.toggle"));
        assert_eq!(action("⌘F8", Context::Global), None, "a breakpoint is toggled at the editor's caret");
        assert_eq!(action("⌥⇧⌘F8", Context::Editor).as_deref(), Some("debug.breakpoint.toggleTemporary"));
        assert_eq!(action("⇧⌘F8", Context::Global).as_deref(), Some("view.breakpoints.open"));
        assert_eq!(action("⌥F8", Context::Global).as_deref(), Some("view.evaluateExpression.open"));
        assert_eq!(action("⌥⌘F8", Context::Editor).as_deref(), Some("view.quickEvaluate.open"));
        assert_eq!(action("⌥F10", Context::Global).as_deref(), Some("debug.executionPoint.show"));
        assert!(keymap.keystrokes("view.breakpoint.addConditional").is_empty());
        assert!(keymap.keystrokes("view.breakpoint.addLogging").is_empty());
        assert_eq!(action("⌘5", Context::Global).as_deref(), Some("view.toolWindow.debug"));
        assert_eq!(action("⌥F7", Context::Editor).as_deref(), Some("navigate.usages.find"), "⌥F7 stays Find Usages");
        assert_eq!(action("⌘F2", Context::Global).as_deref(), Some("run.process.stop"), "Stop stops a debug run too");
        assert_eq!(action("⌘D", Context::Editor).as_deref(), Some("editor.line.duplicate"));
        assert!(keymap.keystrokes("debug.execution.pause").is_empty());
        assert!(keymap.keystrokes("view.streamTrace.open").is_empty());
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
        for action in [
            "debug.session.start",
            "debug.session.start/context",
            "debug.execution.stepOver",
            "debug.execution.stepInto",
            "debug.execution.stepOut",
            "debug.execution.resume",
            "debug.execution.pause",
            "debug.execution.runToCursor",
            "debug.execution.forceRunToCursor",
            "debug.execution.forceStepOver",
            "debug.execution.forceStepInto",
            "debug.breakpoint.toggle",
            "debug.breakpoint.toggleTemporary",
            "view.breakpoints.open",
            "view.breakpoint.addLogging",
            "view.evaluateExpression.open",
            "view.quickEvaluate.open",
            "debug.executionPoint.show",
            "view.smartStepInto.open",
            "debug.frame.reset",
            "view.streamTrace.open",
        ] {
            assert_eq!(GROUPS[group_of(action)], "keymap-group-debug", "{action}");
        }
        assert_eq!(GROUPS[group_of("view.toolWindow.debug")], "keymap-group-window");
    }

    #[test]
    fn bookmark_shortcuts() {
        let keymap = Keymap::default_macos();
        let find = |text: &str| {
            keymap
                .lookup(&Keystroke::parse(text).unwrap(), Context::Editor)
                .map(|binding| (binding.action, binding.params))
        };
        assert_eq!(find("F3").map(|found| found.0).as_deref(), Some("navigate.bookmark.toggle"));
        assert_eq!(find("⌥F3").map(|found| found.0).as_deref(), Some("view.bookmark.mnemonic"));
        assert_eq!(find("⌥⌘F3").map(|found| found.0).as_deref(), Some("view.bookmark.goToMnemonic"));
        assert_eq!(find("⌘F3").map(|found| found.0).as_deref(), Some("view.bookmark.show"));
        assert_eq!(find("⌘2").map(|found| found.0).as_deref(), Some("view.toolWindow.bookmarks"));
        assert_eq!(GROUPS[group_of("view.toolWindow.bookmarks")], "keymap-group-window");
        assert_eq!(find("⌃⇧3"), Some(("navigate.bookmark.toggleMnemonic/3".to_owned(), json!({ "mnemonic": "3" }))));
        assert_eq!(find("⌃0"), Some(("navigate.bookmark.goTo/0".to_owned(), json!({ "mnemonic": "0" }))));
        assert!(keymap.keystrokes("navigate.bookmark.next").is_empty());
        assert!(keymap.keystrokes("navigate.bookmark.previous").is_empty());
        assert!(keymap.keystrokes("view.bookmark.openTabs").is_empty());
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
        for action in ["navigate.bookmark.toggle", "view.bookmark.mnemonic", "navigate.bookmark.goTo/1"] {
            assert_eq!(GROUPS[group_of(action)], "keymap-group-navigate", "{action}");
        }
    }

    #[test]
    fn version_control_shortcuts() {
        let keymap = Keymap::default_macos();
        let find = |text: &str| keymap.lookup(&Keystroke::parse(text).unwrap(), Context::Editor).map(|b| b.action);
        assert_eq!(find("⌘T").as_deref(), Some("view.vcs.update"));
        assert_eq!(find("⇧⌘K").as_deref(), Some("view.vcs.push"));
        assert_eq!(find("⌘K").as_deref(), Some("view.vcs.commit"));
        assert_eq!(find("⌘0").as_deref(), Some("view.toolWindow.commit"));
        assert_eq!(find("⌥⌘Z").as_deref(), Some("view.vcs.rollback"));
        assert_eq!(find("⌘9").as_deref(), Some("view.toolWindow.git"));
        assert_eq!(find("⌥⌘N").as_deref(), Some("view.inline.open"), "Inline in the editor");
        let global = keymap.lookup(&Keystroke::parse("⌥⌘N").unwrap(), Context::Global).map(|b| b.action);
        assert_eq!(global.as_deref(), Some("view.vcs.newBranch"), "New Branch everywhere else");
        for action in ["view.vcs.update", "vcs.remote.fetch", "view.vcs.newBranch", "view.vcs.checkoutRevision"] {
            assert_eq!(GROUPS[group_of(action)], "keymap-group-vcs", "{action}");
        }
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn control_shift_m_moves_the_caret_to_the_matching_bracket() {
        let keymap = Keymap::default_macos();
        let binding = keymap.lookup(&Keystroke::parse("⌃⇧M").unwrap(), Context::Editor).unwrap();
        assert_eq!((binding.command.as_str(), binding.context), ("editor.brackets.goToMatching", Context::Editor));
        assert_eq!(GROUPS[group_of(&binding.action)], "keymap-group-caret");
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn command_semicolon_opens_project_structure_in_settings() {
        let keymap = Keymap::default_macos();
        let stroke = Keystroke::parse("⌘;").unwrap();
        assert_eq!(stroke.key, "Semicolon");
        assert_eq!(stroke.to_string(), "⌘;");
        let binding = keymap.lookup(&stroke, Context::Editor).unwrap();
        assert_eq!(binding.action, "view.projectStructure.open");
        assert_eq!(
            (binding.command.as_str(), &binding.params),
            ("view.settings.open", &json!({ "page": "projectStructure" }))
        );
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn folding_shortcuts_are_the_editors_and_have_their_group() {
        let keymap = Keymap::default_macos();
        for (stroke, action) in [
            ("⌘−", "editor.fold.collapse"),
            ("⌘=", "editor.fold.expand"),
            ("⇧⌘−", "editor.fold.collapseAll"),
            ("⇧⌘=", "editor.fold.expandAll"),
            ("⌘+", "editor.fold.expand"),
            ("⇧⌘+", "editor.fold.expandAll"),
            ("⌥⌘=", "editor.fold.expandRecursively"),
            ("⌥⌘+", "editor.fold.expandRecursively"),
            ("⌥⌘−", "editor.fold.collapseRecursively"),
            ("⇧⌘.", "editor.fold.collapseBlock"),
            ("⌘.", "editor.fold.selection"),
        ] {
            let binding = keymap.lookup(&Keystroke::parse(stroke).unwrap(), Context::Editor).unwrap();
            assert_eq!((binding.action.as_str(), binding.context), (action, Context::Editor), "{stroke}");
            assert!(keymap.lookup(&Keystroke::parse(stroke).unwrap(), Context::Global).is_none(), "{stroke}");
            assert_eq!(GROUPS[group_of(action)], "keymap-group-folding", "{action}");
        }
        for action in ["editor.fold.expandDocComments", "editor.fold.collapseDocComments", "editor.fold.toggleAtCaret"]
        {
            assert!(keymap.keystrokes(action).is_empty(), "{action} has no default shortcut");
            assert_eq!(GROUPS[group_of(action)], "keymap-group-folding", "{action}");
        }
        let custom = keymap.lookup(&Keystroke::parse("⌥⌘.").unwrap(), Context::Editor).unwrap();
        assert_eq!(custom.action, "view.customFolding.open", "Custom Folding, from the editor too");
        let plus = Keystroke::parse("⌘+").unwrap();
        assert_eq!(plus.key, "Plus", "the + of the numeric keypad, or ⇧= on a layout that gives +");
        assert_eq!(String::from(plus), "⌘+");
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn two_keystrokes_in_a_row_read_print_and_match() {
        let stroke = Keystroke::parse("⌘NumPad*, 1").unwrap();
        assert_eq!((stroke.key.as_str(), stroke.command), ("NumpadMultiply", true));
        assert_eq!(stroke.then.as_deref(), Keystroke::parse("1").as_ref());
        assert_eq!(stroke.to_string(), "⌘NumPad*, 1");
        assert_eq!(serde_json::to_string(&stroke).unwrap(), "\"⌘NumPad*, 1\"");
        assert_eq!(Keystroke::parse("⌘,, ⌘K").unwrap().to_string(), "⌘,, ⌘K", "a comma key first");
        assert!(Keystroke::parse("⌘NumPad*, ").is_none());
        let keymap = Keymap::default_macos();
        let first = Keystroke::parse("⌘NumPad*").unwrap();
        assert!(keymap.starts_sequence(&first, Context::Editor));
        assert!(!keymap.starts_sequence(&first, Context::Global), "folding is the editor's");
        let global = keymap.clone().with_user_changes(BTreeMap::from([(
            "view.vcs.commit".to_owned(),
            vec![Keystroke::parse("⌘K, ⌘C").unwrap()],
        )]));
        assert!(global.starts_sequence(&Keystroke::parse("⌘K").unwrap(), Context::Global));
        assert!(
            !global.starts_sequence(&Keystroke::parse("⌘K").unwrap(), Context::Editor),
            "the editor waits only for its own shortcuts"
        );
        assert!(!keymap.starts_sequence(&Keystroke::parse("⌘D").unwrap(), Context::Editor));
        assert_eq!(keymap.lookup(&first, Context::Editor), None, "the first keystroke alone runs nothing");
        let second = Keystroke::parse("3").unwrap();
        let binding = keymap.lookup(&first.followed_by(second), Context::Editor).unwrap();
        assert_eq!(binding.action, "editor.fold.expandToLevel/3");
        assert_eq!(
            (binding.command.as_str(), binding.params.clone()),
            ("editor.fold.expandToLevel", json!({ "level": 3 }))
        );
        let all = if cfg!(target_os = "macos") { "⌥⌘NumPad*, 5" } else { "⇧⌘NumPad*, 5" };
        let binding = keymap.lookup(&Keystroke::parse(all).unwrap(), Context::Editor).unwrap();
        assert_eq!(binding.action, "editor.fold.expandAllToLevel/5");
        assert_eq!(keymap.shortcut("editor.fold.expandToLevel/1").as_deref(), Some("⌘NumPad*, 1"));
    }

    #[test]
    fn surround_with_is_editing_and_surround_with_live_template_is_other() {
        let keymap = Keymap::default_macos();
        for (stroke, action, group) in [
            ("⌥⌘T", "view.surround.open", "keymap-group-edit"),
            ("⌥⌘J", "view.surroundWithTemplate.open", "keymap-group-other"),
        ] {
            let binding = keymap.lookup(&Keystroke::parse(stroke).unwrap(), Context::Editor).unwrap();
            assert_eq!((binding.action.as_str(), binding.context), (action, Context::Global), "{stroke}");
            assert_eq!(GROUPS[group_of(action)], group, "{action}");
        }
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn user_changes_replace_defaults_and_show_conflicts() {
        let mut keymap = Keymap::default_macos();
        keymap.set("editor.line.duplicate", Some(vec![Keystroke::parse("⌘/").unwrap()]));
        assert_eq!(keymap.shortcut("editor.line.duplicate").as_deref(), Some("⌘/"));
        assert_eq!(keymap.conflicts().len(), 1, "⌘/ is now on two editor actions");
        keymap.set("editor.line.duplicate", None);
        assert_eq!(keymap.shortcut("editor.line.duplicate").as_deref(), Some("⌘D"));
        keymap.set("file.document.saveAll", Some(Vec::new()));
        assert!(keymap.lookup(&Keystroke::parse("⌘S").unwrap(), Context::Global).is_none());
    }

    #[test]
    fn a_command_given_keystrokes_becomes_a_global_binding() {
        let mut keymap = Keymap::default_macos();
        let stroke = Keystroke::parse("⌃⌥⇧L").unwrap();
        assert!(!keymap.has_default("vcs.remote.fetchAll"));
        keymap.set("vcs.remote.fetchAll", Some(vec![stroke.clone()]));
        let binding = keymap.lookup(&stroke, Context::Editor).expect("found from the editor too");
        assert_eq!((binding.action.as_str(), binding.command.as_str()), ("vcs.remote.fetchAll", "vcs.remote.fetchAll"));
        assert_eq!(binding.params, json!({}));
        assert_eq!(keymap.shortcut("vcs.remote.fetchAll").as_deref(), Some("⌃⌥⇧L"));
        assert!(keymap.has_default("editor.line.duplicate"));
        keymap.set("vcs.remote.fetchAll", Some(Vec::new()));
        assert!(keymap.lookup(&stroke, Context::Global).is_none());
        assert!(!keymap.bindings().iter().any(|binding| binding.action == "vcs.remote.fetchAll"));
    }

    #[test]
    fn database_workspace_keys_win_inside_it_only() {
        let keymap = Keymap::default_macos();
        let find = |text: &str, context| {
            keymap.lookup(&Keystroke::parse(text).unwrap(), context).map(|binding| binding.action)
        };
        for (key, action) in [
            ("⌥X", "db.console.executeScript"),
            ("⌥⌘⏎", "db.console.executeScript"),
            ("⌘\\", "db.console.executeInNewTab"),
            ("⌥P", "db.console.executeFromCaret"),
            ("⌃⇧E", "db.console.explain"),
            ("⌃]", "view.database.newConsole"),
            ("⌃⇧N", "view.database.newConnection"),
            ("⌃⇧F", "editor.code.reformat/database"),
            ("⌃/", "editor.comment.line/database"),
            ("⌃4", "db.transaction.commit"),
            ("⌃8", "db.transaction.rollback"),
            ("⌃9", "view.database.chooseConnection"),
            ("⌃0", "view.database.chooseSchema"),
            ("⌃T", "db.console.toggleResults"),
            ("⌃⇧T", "db.console.maximizeResults"),
            ("⌃⇧D", "view.gotoDatabaseObject.open"),
            ("⌃⇧C", "view.database.copyAdvanced"),
            ("⌃⇧V", "view.database.pasteAdvanced"),
            ("⌃⌥⇧T", "view.database.focusFilter"),
        ] {
            assert_eq!(find(key, Context::Database).as_deref(), Some(action), "{key}");
            assert_ne!(find(key, Context::Global).as_deref(), Some(action), "{key} is the IDE's outside the workspace");
            if action != "view.gotoDatabaseObject.open" {
                assert_eq!(GROUPS[group_of(action)], "keymap-group-database", "{action}");
            }
        }
        assert_eq!(find("⌃4", Context::Global).as_deref(), Some("navigate.bookmark.goTo/4"));
        assert_eq!(keymap.shortcut("db.transaction.commit").as_deref(), Some("⌃4"));
        assert_eq!(keymap.shortcut("db.console.executeInNewTab").as_deref(), Some("⌘\\"));
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }
}

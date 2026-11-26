use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use zen_automation::Tool;
use zen_core::Registry;
use zen_i18n::t;
use zen_project::settings::CodeStyle;
use zen_workbench::Workbench;

use super::settings::{
    AUTOSAVE_IDLE_SECONDS, IDE_ZOOM_PRESETS, IDE_ZOOMS, IdeSettings, PRESENTATION_FONT_SIZES, UI_FONT_SIZES,
};

const MAIN_COMMANDS: &[(&str, &str)] = &[
    (
        "editor.text.replace",
        "{path, oldText, newText, regex?, expectedCount?, edits?, save?, checkProblems?}: exact or regex replace in one call; checkProblems adds newProblems, the errors the edit brought",
    ),
    ("editor.text.write", "{path, text, save?}: a file's whole text; creates a new file"),
    ("editor.text.batch", "{files: [{path, oldText+newText | edits | text}], save?}: several files, all or none"),
    ("file.fs.rename", "{path, name}: renames a file or folder"),
    ("file.fs.delete", "{paths}: moves files to the Trash"),
    ("file.document.open", "{path}"),
    ("file.document.save", "the active file, with the on-save actions"),
    (
        "search.text.find",
        "{query: {text, regex?, matchCase?}, mask?, directory?, group?, compact?}: Find in Files; group: each path once, cheaper",
    ),
    ("search.text.replaceAll", "Replace in Files"),
    (
        "navigate.symbol.goTo",
        "{query, waitMs?, compact?}: declarations by name; compact: rows `path:line:column kind name`",
    ),
    ("navigate.class.goTo", "{query, waitMs?, compact?}: types by name"),
    ("editor.structure.get", "{path, compact?}: a file's symbols as a tree"),
    ("navigate.declaration.goTo", "{path, offset | line+column, waitMs?}"),
    (
        "navigate.usages.find",
        "{path, offset | line+column | line+name, symbol?, waitMs?, compact?, includeTextMatches?}: usages through the language server; symbol: a declaration by name",
    ),
    (
        "problems.file.list",
        "{path, severity?, limit?, compact?}: compile errors and warnings of a file; severity error: errors only",
    ),
    ("problems.project.list", "{paths?, exclude?, severity?, sources?, limit?, compact?}: every file's problems"),
    ("refactor.rename.apply", "{path, offset | line+column, newName, waitMs?}"),
    ("editor.imports.optimize", "the active file"),
    ("editor.code.reformat", "the active file"),
    ("run.context.get", "{path, offset}: the configuration that runs the test or main there"),
    (
        "run.process.start",
        "{configurationId | context | configuration, waitMs?}: runs; with waitMs answers its summary once it ends",
    ),
    ("run.process.rerun", "{runId, waitMs?}: runs it again; run.tests.rerunFailed the failed tests only"),
    (
        "run.tests.get",
        "{runId, summary? | failedOnly? | test?}: counts; failed tests with file:line and message; one test's output",
    ),
    ("language.servers.status", "whether the language servers are starting or running"),
    ("mcp.session.stats", "{reset?}: calls, bytes and time per tool this session; each answer's _meta.elapsedMs"),
];

pub(super) const COMMAND_TOOLS: &[&str] = &[
    "editor.text.replace",
    "editor.text.write",
    "editor.text.batch",
    "search.text.find",
    "navigate.symbol.goTo",
    "navigate.usages.find",
    "problems.file.list",
    "refactor.rename.apply",
    "run.process.start",
    "run.tests.get",
];

pub(super) fn lean_schema(schema: Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut lean = Map::new();
            for (key, value) in map {
                match (key.as_str(), &value) {
                    ("default", Value::Null) | ("format", _) => continue,
                    ("minimum", Value::Number(number)) if number.as_f64() == Some(0.0) => continue,
                    ("type", Value::Array(types)) => {
                        let kept: Vec<Value> = types.iter().filter(|kind| *kind != "null").cloned().collect();
                        let kind = if kept.len() == 1 { kept[0].clone() } else { Value::Array(kept) };
                        lean.insert(key, kind);
                    }
                    ("anyOf", Value::Array(options))
                        if options.len() == 2 && options.contains(&json!({ "type": "null" })) =>
                    {
                        let other = options.iter().find(|option| **option != json!({ "type": "null" })).cloned();
                        if let Some(Value::Object(other)) = other.map(lean_schema) {
                            lean.extend(other);
                        }
                    }
                    _ => {
                        lean.insert(key, lean_schema(value));
                    }
                }
            }
            Value::Object(lean)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(lean_schema).collect()),
        other => other,
    }
}

pub(super) fn command_tool_name(id: &str) -> String {
    id.replace('.', "_")
}

pub(super) fn command_tools(registry: &Registry) -> Vec<Tool> {
    COMMAND_TOOLS
        .iter()
        .filter_map(|id| registry.info(id))
        .map(|info| {
            let mut schema = lean_schema(info.params_schema.clone());
            if let Value::Object(object) = &mut schema {
                object.remove("$schema");
                object.remove("title");
                object.remove("description");
                let project = json!({ "type": "string", "description": "The project's folder, absolute; optional with one project open." });
                if let Some(Value::Object(properties)) = object.get_mut("properties") {
                    properties.insert("project".to_owned(), project);
                } else {
                    object.insert("properties".to_owned(), json!({ "project": project }));
                }
                object.insert("type".to_owned(), json!("object"));
            }
            Tool {
                name: command_tool_name(info.id.as_str()),
                description: format!("{} The command {} (dispatch_command answers the same).", info.description, info.id.as_str()),
                input_schema: schema,
                output_schema: None,
            }
        })
        .collect()
}

pub(super) fn as_dispatch(tool: &str, arguments: &Value) -> Option<Value> {
    let id = COMMAND_TOOLS.iter().find(|id| command_tool_name(id) == tool)?;
    let mut params = arguments.as_object().cloned().unwrap_or_default();
    let project = params.remove("project");
    let mut call = json!({ "command": id, "params": params });
    if let Some(project) = project {
        call["project"] = project;
    }
    Some(call)
}

pub(super) fn waits_for_run(tool: &str, arguments: &Value) -> bool {
    tool == "dispatch_command"
        && arguments.get("command").and_then(Value::as_str) == Some("run.process.start")
        && arguments.get("params").and_then(|params| params.get("waitMs")).is_some_and(Value::is_u64)
}

pub(super) fn loading_wait(tool: &str, arguments: &Value) -> Option<std::time::Duration> {
    let command = arguments.get("command").and_then(Value::as_str)?;
    let wait = arguments.get("params").and_then(|params| params.get("waitMs")).and_then(Value::as_u64)?;
    let wait = std::time::Duration::from_millis(wait);
    (tool == "dispatch_command"
        && zen_workbench::language::LOADING_WAIT_COMMANDS.contains(&command)
        && wait > zen_workbench::language::MAX_WAIT)
        .then_some(wait)
}

pub(super) fn waits_on(answer: &Value, wait: &mut zen_workbench::language::SearchWait) -> bool {
    let pending = answer["pending"] == true;
    let reason = serde_json::from_value(answer["reason"].clone()).ok();
    wait.keeps_waiting(pending, reason)
}

fn group_of(id: &str) -> &str {
    id.split_once('.').map_or(id, |(group, _)| group)
}

fn groups(registry: &Registry) -> std::collections::BTreeMap<String, usize> {
    let mut groups = std::collections::BTreeMap::new();
    for info in registry.commands() {
        *groups.entry(group_of(info.id.as_str()).to_owned()).or_insert(0) += 1;
    }
    groups
}

pub(super) fn instructions(product: &str, registry: &Registry) -> String {
    let groups: Vec<String> = groups(registry).into_iter().map(|(group, count)| format!("{group} ({count})")).collect();
    let main: Vec<String> = MAIN_COMMANDS
        .iter()
        .filter(|(id, _)| registry.info(id).is_some())
        .map(|(id, what)| format!("- {id} {what}"))
        .collect();
    let command_tools: Vec<String> =
        COMMAND_TOOLS.iter().filter(|id| registry.info(id).is_some()).map(|id| command_tool_name(id)).collect();
    format!(
        "{product} is a Zen IDE. Every action of the IDE is a command: dispatch_command runs one in a project \
         window (list_windows shows the windows; paths are absolute or relative to the project). \
         list_commands with `group` or `query` lists commands with a one-line description; describe_command \
         gives one command's parameters. get_document_text reads a file as the editor has it, a range of \
         lines with startLine/endLine; withLineNumbers gives `N|text` lines. Lines and columns agents see are \
         one-based everywhere (search, navigation, problems, reads, test failures), and paths in answers are \
         relative to the project when the file is in it; a place is a path with `offset`, or with `line` and \
         `column` as answers give them. Edit files through the IDE rather than with scripts: editor.text.replace \
         (exact text, or `regex` with `^`/`$` and groups), editor.text.write (a file's whole text; creates the \
         file), editor.text.batch (several files in one call, all or none), file.fs.rename and file.fs.delete; \
         such edits are one undo step, kept in Local History, and problems.file.list then checks what the \
         editor has; `checkProblems: true` on an edit answers the errors it brought at once. Lists of places \
         take `compact: true` (rows `path:line:column ...` instead of objects), and problems take `severity: \
         \"error\"`. A search that has nothing says why (`reason`: noMatch, \
         or pending while the language server starts or indexes: call again). Run tests with run.process.start \
         and waitMs, then run.tests.get with failedOnly; never poll the whole tree. \
         The window's own actions (tool windows, popups, dialogs: the view.* actions of find_action and \
         get_keymap) run with run_action; press_keys presses keys in the window. \
         Opening, focusing and closing project windows have their own tools.\n\
         Command groups: {}.\n\
         Main commands (dispatch_command):\n{}\n\
         These are also tools of their own, with the command's parameters (and `project`): {}.",
        groups.join(", "),
        main.join("\n"),
        command_tools.join(", ")
    )
}

fn one_line(description: &str) -> String {
    let description = description.trim();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut chars = description.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '`' => quoted = !quoted,
            '(' | '[' | '{' if !quoted => depth += 1,
            ')' | ']' | '}' if !quoted => depth = depth.saturating_sub(1),
            '.' | '?' | '!' if !quoted && depth == 0 => {
                let ends = chars.peek().is_none_or(|(_, next)| next.is_whitespace());
                if ends && !abbreviation(&description[..at]) {
                    return description[..=at].to_owned();
                }
            }
            _ => {}
        }
    }
    description.to_owned()
}

fn abbreviation(before: &str) -> bool {
    let word = before.rsplit(|c: char| c.is_whitespace() || c == '(').next().unwrap_or_default();
    matches!(word.to_lowercase().as_str(), "e.g" | "i.e" | "etc" | "vs" | "cf")
}

pub(super) fn list_commands(registry: &Registry, arguments: &Value) -> Result<Value, String> {
    if arguments.get("full").and_then(Value::as_bool) == Some(true) {
        let commands: Vec<Value> = registry
            .commands()
            .map(|info| {
                json!({ "id": info.id.as_str(), "title": info.title, "description": info.description, "params": info.params_schema })
            })
            .collect();
        return Ok(json!({ "commands": commands }));
    }
    let group = arguments.get("group").and_then(Value::as_str).filter(|group| !group.is_empty());
    let query = arguments.get("query").and_then(Value::as_str).map(str::trim).filter(|query| !query.is_empty());
    if group.is_none() && query.is_none() {
        let mut by_group: Map<String, Value> = Map::new();
        for info in registry.commands() {
            let id = info.id.as_str();
            let (group, rest) = id.split_once('.').unwrap_or((id, ""));
            if let Value::Array(ids) = by_group.entry(group.to_owned()).or_insert_with(|| json!([])) {
                ids.push(json!(rest));
            }
        }
        return Ok(json!({ "groups": by_group }));
    }
    if let Some(group) = group
        && !groups(registry).contains_key(group)
    {
        let known: Vec<String> = groups(registry).into_keys().collect();
        return Err(t!("mcp-commands-group-unknown", group = group, groups = known.join(", ")));
    }
    let words: Vec<String> =
        query.map(|query| query.to_lowercase().split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
    let commands: Vec<Value> = registry
        .commands()
        .filter(|info| group.is_none_or(|group| group_of(info.id.as_str()) == group))
        .filter(|info| {
            let haystack = format!("{} {} {}", info.id.as_str(), info.title, info.description).to_lowercase();
            words.iter().all(|word| haystack.contains(word.as_str()))
        })
        .map(|info| json!({ "id": info.id.as_str(), "title": info.title, "description": one_line(&info.description) }))
        .collect();
    Ok(json!({ "commands": commands }))
}

pub(super) fn describe_command(registry: &Registry, arguments: &Value) -> Result<Value, String> {
    let id = arguments.get("id").and_then(Value::as_str).ok_or("`id` is required")?;
    if id == "set_settings" {
        return Ok(json!({
            "id": id,
            "title": "Set Settings",
            "description": "The settings set_settings takes, as get_settings answers them.",
            "params": settings_schema(),
        }));
    }
    let info = registry.info(id).ok_or_else(|| t!("mcp-command-unknown", id = id))?;
    let mut params = info.params_schema.clone();
    if let Value::Object(schema) = &mut params {
        schema.remove("$schema");
        schema.remove("title");
    }
    Ok(json!({ "id": info.id.as_str(), "title": info.title, "description": info.description, "params": params }))
}

pub(super) fn dispatch_error(error: &zen_core::DispatchError) -> String {
    if let zen_core::DispatchError::Failed { error, .. } = error
        && let Some(code) = &error.code
    {
        let mut answer = Map::new();
        answer.insert("error".to_owned(), json!(code));
        answer.extend(error.data.clone().unwrap_or_default());
        answer.insert("message".to_owned(), json!(error.message));
        return Value::Object(answer).to_string();
    }
    error.to_string()
}

pub(super) const WHOLE_TEXT_LIMIT: usize = 32 * 1024;

pub(super) fn document_text(text: &str, unsaved: bool, arguments: &Value) -> Result<Value, String> {
    let number = |name: &str| arguments.get(name).and_then(Value::as_u64).map(|value| value as usize);
    let numbered = arguments.get("withLineNumbers").and_then(Value::as_bool).unwrap_or(false);
    let (start, end) = (number("startLine"), number("endLine"));
    let (offset, limit) = (number("offset"), number("limit"));
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = lines.len();
    let ranged = start.is_some() || end.is_some() || offset.is_some() || limit.is_some();
    if (start.is_some() || end.is_some()) && (offset.is_some() || limit.is_some()) {
        return Err(t!("mcp-text-range-both"));
    }
    if !ranged && text.len() > WHOLE_TEXT_LIMIT {
        let message = t!(
            "mcp-text-too-large",
            lines = total,
            kilobytes = text.len().div_ceil(1024),
            limit = WHOLE_TEXT_LIMIT / 1024
        );
        return Err(
            json!({ "error": "too_large", "lines": total, "bytes": text.len(), "message": message }).to_string()
        );
    }
    let first = start.or(offset).unwrap_or(1).max(1);
    let last = match (end, limit) {
        (Some(end), _) => end,
        (None, Some(limit)) => (first + limit).saturating_sub(1),
        (None, None) => total,
    }
    .min(total);
    if ranged && first > total.max(1) {
        return Err(t!("mcp-text-range-past", lines = total, start = first));
    }
    if ranged && last < first && total > 0 {
        return Err(t!("mcp-text-range-order", start = first, end = last));
    }
    let chosen = lines.get(first - 1..last).unwrap_or_default();
    let body: String = if numbered {
        chosen.iter().enumerate().map(|(index, line)| format!("{}|{line}", first + index)).collect()
    } else {
        chosen.concat()
    };
    if !ranged {
        return Ok(json!({ "text": body, "unsaved": unsaved }));
    }
    Ok(json!({ "text": body, "unsaved": unsaved, "startLine": first, "endLine": last, "totalLines": total }))
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn editor_tabs_schema() -> Value {
    json!({
        "type": "object",
        "description": "Settings → Editor → General → Editor Tabs and Advanced Settings → Editor Tabs; any field left out keeps its value.",
        "properties": {
            "placement": { "type": "string", "enum": ["top", "bottom", "left", "right", "none"], "description": "Tab placement (top by default)." },
            "rows": { "type": "string", "enum": ["scroll", "squeeze", "multiple"], "description": "One row that scrolls, one row whose tabs get narrower, or multiple rows (scroll by default)." },
            "pinnedRow": { "type": "boolean", "description": "Show pinned tabs in a separate row (off by default)." },
            "showIcon": { "type": "boolean", "description": "Show file icon (on by default)." },
            "showExtension": { "type": "boolean", "description": "Show file extension (on by default)." },
            "showDirectory": { "type": "boolean", "description": "Show directory for non-unique file names (on by default)." },
            "markModified": { "type": "boolean", "description": "Mark modified: an asterisk before a modified file's name (off by default)." },
            "fullPathOnHover": { "type": "boolean", "description": "Show full path on mouse hover (on by default)." },
            "closeButton": { "type": "string", "enum": ["left", "right", "none"], "description": "Close button position (right by default)." },
            "sortAlphabetically": { "type": "boolean", "description": "Sort tabs alphabetically (off by default)." },
            "openAtEnd": { "type": "boolean", "description": "Open new tabs at the end; off, right of the active tab (on by default)." },
            "limit": { "type": "integer", "minimum": 1, "maximum": zen_workbench::tabs::MAX_TAB_LIMIT, "description": "Tab limit of each editor group; pinned tabs are never closed for it (30 by default)." },
            "overLimit": { "type": "string", "enum": ["closeUnchanged", "closeUnused"], "description": "When tabs exceed the limit (closeUnused by default); unsaved changes are saved first." },
            "activateOnClose": { "type": "string", "enum": ["left", "right", "recent"], "description": "When the current tab is closed, activate the tab on the left, on the right or the most recently opened one (left by default)." },
            "preview": { "type": "boolean", "description": "Enable preview tab: one reused italic tab for files picked by a single click in the Project tool window and files the debugger opens (off by default)." },
            "keepPinnedLeft": { "type": "boolean", "description": "Advanced Settings: Keep pinned tabs on the left (on by default)." },
            "declarationInSameTab": { "type": "boolean", "description": "Advanced Settings: Open declaration source in the same tab, when the tab left has no unsaved changes and is not pinned (off by default)." },
        },
        "additionalProperties": false,
    })
}

fn todo_schema() -> Value {
    json!({
        "type": "object",
        "description": "Settings → Editor → TODO and the Commit tool window's Check TODO; any field left out keeps its value.",
        "properties": {
            "patterns": {
                "type": "array",
                "description": "The patterns, in order (\\\\btodo\\\\b.* and \\\\bfixme\\\\b.* by default).",
                "items": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "A regular expression." },
                        "caseSensitive": { "type": "boolean", "description": "Whether case matters (off by default)." },
                        "icon": { "type": "string", "enum": ["default", "question", "important"], "description": "Its icon in the settings." },
                        "color": { "type": "string", "description": "Its own text colour, #rrggbb; without it, the colour scheme's TODO colour." },
                    },
                    "required": ["pattern"],
                    "additionalProperties": false,
                },
            },
            "filters": {
                "type": "array",
                "description": "The filters of the TODO tool window and of Check TODO.",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Its name." },
                        "patterns": { "type": "array", "items": { "type": "string" }, "description": "The patterns it shows, by their text." },
                    },
                    "required": ["name"],
                    "additionalProperties": false,
                },
            },
            "multiline": { "type": "boolean", "description": "Continue a TODO on the indented lines below it (on by default)." },
            "commitCheck": { "type": "boolean", "description": "Check TODO in the Commit tool window (off by default)." },
            "commitFilter": { "type": ["string", "null"], "description": "The filter Check TODO uses; every pattern when null." },
        },
        "additionalProperties": false,
    })
}

pub(super) fn settings_schema() -> Value {
    let languages: Vec<Value> =
        zen_i18n::LANGUAGES.iter().map(|(code, _)| json!(code)).chain([json!(zen_i18n::SYSTEM), Value::Null]).collect();
    let mcp_languages: Vec<Value> = zen_i18n::LANGUAGES.iter().map(|(code, _)| json!(code)).collect();
    let code_style = json!({
        "type": ["object", "null"],
        "description": "The indentation of one language; null brings back the built-in one.",
        "properties": {
            "useTabs": { "type": "boolean", "description": "Indent with tab characters." },
            "tabSize": { "type": "integer", "minimum": 1, "maximum": 255, "description": "Columns a tab character takes." },
            "indent": { "type": "integer", "minimum": 1, "maximum": 255, "description": "Columns of one indentation level." },
        },
        "additionalProperties": false,
    });
    let proxy = json!({
        "type": "object",
        "description": "Read only for agents. Appearance & Behavior → System Settings → HTTP Proxy: one setting for every network request of the IDE. The password is not part of it: settings.proxy.password and settings.proxy.forget handle it; settings.proxy.status and settings.proxy.check show its effect.",
        "properties": {
            "mode": { "type": "string", "enum": ["noProxy", "autoDetect", "manual"], "description": "Auto-detect by default: the environment's proxy variables, then the system's proxy." },
            "automaticConfiguration": { "type": "boolean", "description": "Automatic proxy configuration URL. Such scripts are reported as not supported." },
            "automaticConfigurationUrl": { "type": "string", "description": "The address of the script." },
            "kind": { "type": "string", "enum": ["http", "socks"], "description": "HTTP or SOCKS5." },
            "host": { "type": "string", "description": "Host name of the manual proxy." },
            "port": { "type": "integer", "minimum": 1, "maximum": 65535, "description": "Port number of the manual proxy." },
            "noProxyFor": { "type": "string", "description": "Hosts reached directly, separated by commas; * matches any text." },
            "authentication": { "type": "boolean", "description": "Proxy authentication." },
            "login": { "type": "string", "description": "Login of the proxy." },
            "remember": { "type": "boolean", "description": "Keep the password in the system keychain." },
        },
        "additionalProperties": false,
    });
    let caffeine = json!({
        "type": ["object", "null"],
        "description": "Tools → Caffeine: the defaults Caffeine is turned on with (Turn On in its tool window, the start with the IDE); caffeine.awake.start, caffeine.awake.stop and caffeine.status.get drive it. Null brings back the defaults.",
        "properties": {
            "what": { "type": "array", "items": { "type": "string", "enum": ["idle", "display", "disk", "system", "user"] }, "uniqueItems": true, "description": "What to keep awake: the system from idle sleep (caffeinate -i, the default), the display (-d), the disk (-m), the system on mains power only (-s), user is active (-u)." },
            "length": {
                "type": "object",
                "description": "For how long: {kind: indefinitely} (the default), {kind: minutes, minutes}, or {kind: while}: while one of `while` runs.",
                "properties": {
                    "kind": { "type": "string", "enum": ["indefinitely", "minutes", "while"] },
                    "minutes": { "type": "integer", "minimum": 1, "maximum": zen_workbench::caffeine::MAX_MINUTES },
                },
                "required": ["kind"],
            },
            "while": { "type": "array", "items": { "type": "string", "enum": ["run", "debug", "task", "indexing", "agent"] }, "uniqueItems": true, "description": "What keeps it on in the while mode: runs and tests, debug sessions, background tasks, indexing, agents connected over MCP (all by default)." },
            "customMinutes": { "type": "integer", "minimum": 1, "maximum": zen_workbench::caffeine::MAX_MINUTES, "description": "The minutes of the menu's custom duration (90 by default)." },
            "offOnBattery": { "type": "boolean", "description": "Pause on battery power (off by default)." },
            "lowBattery": { "type": "boolean", "description": "Pause when the battery is below lowBatteryLevel (off by default)." },
            "lowBatteryLevel": { "type": "integer", "minimum": 5, "maximum": 95, "description": "The charge in percent below which it pauses (20 by default)." },
            "resumeOnCharge": { "type": "boolean", "description": "Go on again on mains power; off: a battery rule turns Caffeine off instead (on by default)." },
            "startOnLaunch": { "type": "boolean", "description": "Turn Caffeine on when the IDE starts (off by default)." },
        },
        "additionalProperties": false,
    });
    let editor_appearance = json!({
        "type": ["object", "null"],
        "description": "Settings → Editor → Appearance and Advanced Settings → Editor: what every editor draws besides the text; editor.whitespaces.toggle and editor.indentGuides.toggle change the active editor only, editor.appearance.get reads it. Null brings back the defaults.",
        "properties": {
            "hardWrapGuides": { "type": "boolean", "description": "Draw the right margin and the visual guides set in Code Style: the right margin (the project's code style, else Code Style → General → Hard wrap at) and the visual guides as vertical lines (on by default)." },
            "indentGuides": { "type": "boolean", "description": "Show indent guides: thin lines at each indent level, the caret's block brighter (on by default)." },
            "whitespaces": { "type": "boolean", "description": "Show whitespaces: spaces as dots, tabs as lines or arrows (off by default)." },
            "whitespaceKinds": {
                "type": "object",
                "description": "Which whitespaces are drawn while they are shown.",
                "properties": {
                    "leading": { "type": "boolean", "description": "Before a line's text (on by default)." },
                    "inner": { "type": "boolean", "description": "Between a line's words (on by default)." },
                    "trailing": { "type": "boolean", "description": "After a line's text (on by default)." },
                    "selection": { "type": "boolean", "description": "Inside a selection, whatever their place (off by default)." },
                },
                "additionalProperties": false,
            },
            "tabRendering": { "type": "string", "enum": ["line", "arrow"], "description": "Advanced Settings → Editor → Tab character rendering: a horizontal line (the default) or an arrow." },
        },
        "additionalProperties": false,
    });
    let groups: Vec<Value> = zen_workbench::notifications::Group::ALL.iter().map(|group| json!(group.id())).collect();
    let mut schema = json!({
        "type": "object",
        "properties": {
            "mcpEnabled": { "type": "boolean", "description": "Whether agents may drive the IDE over MCP." },
            "mcpPort": { "type": "integer", "minimum": 1024, "maximum": 65535, "description": "The MCP server's port on 127.0.0.1." },
            "openInNewWindow": { "type": "boolean", "description": "Open a project in a window of its own instead of a tab, while openProjectIn is null." },
            "confirmExit": { "type": "boolean", "description": "Appearance & Behavior → System Settings → Confirm before exiting the IDE: ⌘Q and Quit ask first (on by default). The exit_ide tool with confirmed: true never asks." },
            "language": { "type": ["string", "null"], "enum": languages, "description": "The interface language, kept across restarts and updates: a language code, `system` to follow the system's language, or null for the default, English (whatever the system's language)." },
            "foldImports": { "type": "boolean", "description": "Editor → General → Code Folding → Fold by default → Imports: imports start folded when a file opens for the first time (on by default)." },
            "closingTags": { "type": "boolean", "description": "Editor → General → Smart Keys: HTML/CSS: typing > of a start tag adds its end tag, and </ the open element's name (on by default)." },
            "gutterRunIcons": { "type": "boolean", "description": "Editor → General → Gutter Icons: show run icons next to main and the tests (on by default)." },
            "highlightUsagesAtCaret": { "type": "boolean", "description": "Editor → General → Highlight on Caret Movement: highlight the usages of the symbol at the caret by themselves (on by default)." },
            "softWrapFiles": { "type": "string", "description": "Editor → General → Soft Wraps: the file name patterns, separated by \";\", whose files open soft-wrapped (*.md; *.txt; *.rst; *.adoc by default)." },
            "breadcrumbs": { "type": "boolean", "description": "Editor → General → Breadcrumbs: show the declarations around the caret under the editor (on by default)." },
            "stickyLines": { "type": "boolean", "description": "Editor → General → Sticky Lines: keep the first lines of the scopes the view is inside at its top (on by default)." },
            "stickyLinesMax": { "type": "integer", "minimum": 1, "maximum": zen_workbench::sticky::MAX_LIMIT, "description": "Editor → General → Sticky Lines: how many lines stick at most (5 by default)." },
            "ligatures": { "type": "boolean", "description": "Editor → Font: draw ligatures." },
            "clipboardLimit": {
                "type": "integer",
                "minimum": 1,
                "maximum": zen_workbench::clipboard_history::MAX_LIMIT,
                "description": "Editor → General → Limits: the most entries Paste from History keeps (20 by default).",
            },
            "theme": { "type": ["string", "null"], "description": "The theme in use, read only here: change it with select_theme." },
            "themeFollowSystem": { "type": "boolean", "description": "Whether the theme follows the system's light or dark appearance, read only here: change it with follow_system_theme." },
            "themeDark": { "type": ["string", "null"], "description": "The theme while the system is dark (null: Zen Dark), read only here: change it with follow_system_theme." },
            "themeLight": { "type": ["string", "null"], "description": "The theme while the system is light (null: Zen Light), read only here: change it with follow_system_theme." },
            "keymap": { "type": ["string", "null"], "description": "The keymap in use, read only here: change it with select_keymap." },
            "reformatOnSave": { "type": "boolean", "description": "Actions on Save for new projects and for projects without their own .zen/actionsOnSave.json (project.actionsOnSave.set): reformat the code." },
            "optimizeImportsOnSave": { "type": "boolean", "description": "Actions on Save for new projects: optimize the imports." },
            "uiFontSize": {
                "type": ["number", "null"],
                "minimum": UI_FONT_SIZES.start(),
                "maximum": UI_FONT_SIZES.end(),
                "description": "Appearance → Use custom font → Size, in points; it zooms the whole UI but not the editor. Null is 13.",
            },
            "presentationFontSize": {
                "type": "number",
                "minimum": PRESENTATION_FONT_SIZES.start(),
                "maximum": PRESENTATION_FONT_SIZES.end(),
                "description": "Appearance → Presentation Mode → Font size: the editor font size in Presentation Mode, in points (24 by default); view.editorFontSize.increase, .decrease and .reset change it while Presentation Mode is on.",
            },
            "editorFont": {
                "type": ["object", "null"],
                "description": "Editor → Font.",
                "properties": {
                    "family": { "type": ["string", "null"], "description": "The font family; null is the bundled editor font." },
                    "size": { "type": "number", "exclusiveMinimum": 0, "description": "The size in points (13 by default); view.editorFontSize.increase and .decrease (⌃⇧., ⌃⇧,) change it by one point, view.editorFontSize.reset brings back 13; in Presentation Mode they change presentationFontSize instead." },
                    "lineSpacing": { "type": "number", "exclusiveMinimum": 0, "description": "The line height as a multiple of the size (1.2 by default)." },
                },
                "additionalProperties": false,
            },
            "inlayHints": {
                "type": ["object", "null"],
                "description": "Editor → Inlay Hints: the labels the language server's hints put in the text.",
                "properties": {
                    "enabled": { "type": "boolean", "description": "Show inlay hints at all (on by default)." },
                    "parameterNames": { "type": "boolean", "description": "Parameter names before the arguments of a call (on by default)." },
                    "fieldNames": { "type": "boolean", "description": "Field names before the values of a composite literal." },
                    "variableTypes": { "type": "boolean", "description": "Types of variables declared without one." },
                    "literalTypes": { "type": "boolean", "description": "Types of composite literals written without one." },
                    "typeArguments": { "type": "boolean", "description": "Type arguments inferred for a call of a generic function." },
                    "constantValues": { "type": "boolean", "description": "Values of constants." },
                },
                "additionalProperties": false,
            },
            "codeVision": {
                "type": ["object", "null"],
                "description": "Editor → Inlay Hints → Code Vision: the row above the declarations.",
                "properties": {
                    "enabled": { "type": "boolean", "description": "Show Code Vision at all (on by default)." },
                    "usages": { "type": "boolean", "description": "The number of usages, from the language server (on by default)." },
                    "authors": { "type": "boolean", "description": "Who changed the declaration last, from Git blame (off by default; the action view.codeVision.authors.toggle, ⌃⌥A, switches it)." },
                    "authorsDefaultV2": { "type": "boolean", "description": "Set once the authors went through their one-time switch to off by default; leave it as it is." },
                },
                "additionalProperties": false,
            },
            "gitAvatars": { "type": "boolean", "description": "Version Control → Git Workspace: show the authors' pictures on the graph, fetched from gravatar.com (a hash of the e-mail), the repository's GitLab (the e-mail) and GitHub (no-reply addresses) and kept in the data folder (on by default; off: initials, no network; vcs.avatar.get reads them)." },
            "searchWaitForAll": { "type": "boolean", "description": "Advanced Settings → Search Everywhere: wait for all contributors to finish before the results show, instead of at most 2000 ms for the primary ones (off by default)." },
            "gitAvatarsDefaultOn": { "type": "boolean", "description": "Set once the authors' pictures went through their one-time switch to on by default; leave it as it is." },
            "declaredSettings": {
                "type": ["object", "null"],
                "description": "The on/off settings that inspectors declare, by key (settings.declared.list lists them with \
                    their page and the servers they contact), e.g. go.dependencies.checkUpdates and \
                    go.dependencies.checkVulnerabilities (Languages & Frameworks → Go → Dependencies), and the \
                    \"Fold by default\" options of the language modules (editor.folding.options lists them), e.g. \
                    go.folding.ifErrors. Null brings back a default.",
                "additionalProperties": { "type": ["boolean", "null"] },
            },
            "codeStyle": {
                "type": ["object", "null"],
                "description": "Editor → Code Style by language id (go, json, yaml, toml, markdown, ...; default covers the languages without an entry).",
                "additionalProperties": code_style,
            },
            "notificationBalloons": { "type": "boolean", "description": "Appearance & Behavior → Notifications: show balloons for new notifications (on by default)." },
            "balloonGroupsOff": {
                "type": "array",
                "items": { "type": "string", "enum": groups },
                "uniqueItems": true,
                "description": "Appearance & Behavior → Notifications: the groups whose notifications come without a balloon, only in the Notifications tool window.",
            },
        },
        "required": [],
        "additionalProperties": false,
    });
    schema["properties"]["caffeine"] = caffeine;
    schema["properties"]["proxy"] = proxy;
    schema["properties"]["acceptUntrustedCertificates"] = json!({
        "type": "boolean",
        "description": "An agent can turn it off, not on. Appearance & Behavior → System Settings → Server Certificates → Accept non-trusted certificates automatically: every server is accepted without asking (off by default, and it turns off the protection of TLS). The list is handled by settings.certificates.list, add, remove, pending and answer.",
    });
    schema["properties"]["markdown"] = json!({
        "type": ["object", "null"],
        "description": "Languages & Frameworks → Markdown: how Markdown files are previewed. Null brings back the defaults.",
        "properties": {
            "previewLayout": { "type": "string", "enum": ["editorOnly", "editorAndPreview", "previewOnly"], "description": "Preview layout: the mode of Markdown files that were never switched (editorAndPreview by default); the same value as editor.preview.setMode with scope type." },
            "syncScroll": { "type": "boolean", "description": "Sync scroll: the editor and the preview keep the same source line in view (on by default)." },
            "previewFontSize": { "type": "number", "minimum": 8, "maximum": 32, "description": "Preview font size in points: of the body text, the rest of the preview in proportion (14.5 by default)." },
        },
        "additionalProperties": false,
    });
    schema["properties"]["whatsNewOnUpdate"] = json!({ "type": "boolean", "description": "Appearance & Behavior → System Settings → Show What's New after an update (on by default): the running version's release notes open once in the first project window after an update." });
    schema["properties"]["showTipsOnStartup"] = json!({ "type": "boolean", "description": "Appearance & Behavior → System Settings → Show tips on startup (off by default): Tip of the Day opens in the first project window." });
    schema["properties"]["tipIndex"] = json!({ "type": "integer", "minimum": 0, "description": "Which tip Help → Tip of the Day shows next, counted from 0." });
    schema["properties"]["terminalLinks"] = json!({
        "type": ["object", "null"],
        "description": "Tools → Terminal → Links: what ⌘-click on a link in the Terminal does without asking, by kind of link (the dialog's \"Remember my choice\"); null asks every time. Null brings back asking for both. terminal.link.list and terminal.link.open drive links.",
        "properties": {
            "url": { "type": ["string", "null"], "enum": ["openInBrowser", "copyLink", null], "description": "For URLs (null by default: ask)." },
            "file": { "type": ["string", "null"], "enum": ["openInEditor", "revealInProject", "openInFinder", "copyPath", null], "description": "For files and folders (null by default: ask)." },
        },
        "additionalProperties": false,
    });
    schema["properties"]["terminalRestore"] = json!({ "type": "boolean", "description": "Tools → Terminal → Sessions → Restore terminal tabs after a restart (on by default): each tab's last screen, folder and given environment are kept in the data folder (readable only by the user), so after a reboot or when the terminal host stopped the tabs open again with the old output dimmed and a new shell; the last command is offered to the person (terminal.session.rerun is refused to agents), never run by itself. Off removes what was kept." });
    schema["properties"]["loginShellEnvironment"] = json!({ "type": "boolean", "description": "Tools → Terminal → Environment → Give programs the environment of my login shell (on by default, ADR-0294): runs, language servers, plug-in tool windows and targets and the terminal get the variables the person's login shell sets, read once in the background at start; a configuration's own variables win. Off takes effect at once, on at the next start. ide.environment.get shows its state and the variables' names." });
    schema["properties"]["terminalAgents"] = json!({
        "type": "array",
        "description": "Set by the user only: an agent cannot add, change or remove one (refused). Tools → Terminal → AI Agents: the user's agent resume adapters, beside the plug-ins' (the user's win for the same id); terminal.agent.list lists all of them with the hook each agent needs. An adapter: id (lower-case letters, digits, dashes), name, programs (program names to recognise), resume (the resume command word by word, with {session} once), sessionField (a JSON pointer, /session_id by default), docs. Invalid adapters are ignored.",
        "items": { "type": "object", "required": ["id", "name", "programs", "resume"] },
    });
    schema["properties"]["buildReloadAfter"] = json!({ "type": "string", "enum": ["external", "any"], "description": "Build, Execution, Deployment → Build Tools → When build scripts change, reload the project: external (the default) loads changes made outside the IDE by themselves and shows Load Changes (view.buildChanges.load, ⇧⌘I) for edits; any loads saved edits too. build.changes.get lists what changed." });
    schema["properties"]["consoleBufferOverride"] = json!({ "type": "boolean", "description": "Editor → General → Console → Override console cycle buffer size: use consoleBufferKb instead of the built-in 1024 KB (off by default)." });
    schema["properties"]["consoleBufferKb"] = json!({
        "type": "integer",
        "minimum": 1,
        "maximum": zen_run::MAX_BUFFER_KB,
        "description": "Editor → General → Console: how many KB of output a run or debug console keeps; older lines are dropped (1024 by default, used when consoleBufferOverride is on).",
    });
    schema["properties"]["showFoldArrows"] = json!({ "type": "boolean", "description": "Editor → General → Code Folding → Show code folding arrows: the gutter shows fold arrows at all (on by default); off, folded regions still show their mark in the text." });
    schema["properties"]["foldArrows"] = json!({ "type": "string", "enum": ["onHover", "always"], "description": "Editor → General → Code Folding → Show code folding arrows: the arrows of unfolded regions show on mouse hover over the gutter (onHover, the default) or always; folded ones always show." });
    schema["properties"]["foldBottomArrows"] = json!({ "type": "boolean", "description": "Editor → General → Code Folding → Show bottom arrows: an arrow on the last line of each unfolded region too, which folds it (off by default)." });
    schema["properties"]["foldFileHeader"] = json!({ "type": "boolean", "description": "Editor → General → Code Folding → Fold by default → File header: the comment that starts a file and documents nothing, such as a licence (off by default)." });
    schema["properties"]["foldDocComments"] = json!({ "type": "boolean", "description": "Editor → General → Code Folding → Fold by default → Documentation comments (off by default)." });
    schema["properties"]["foldMethodBodies"] = json!({ "type": "boolean", "description": "Editor → General → Code Folding → Fold by default → Method bodies: the bodies of functions and methods (off by default)." });
    schema["properties"]["foldCustomRegions"] = json!({ "type": "boolean", "description": "Editor → General → Code Folding → Fold by default → Custom folding regions: every region comment pair, not only those with defaultstate=\"collapsed\" (off by default)." });
    schema["properties"]["wheelFontSize"] = json!({ "type": "boolean", "description": "Editor → General → Mouse Control → Change font size with Command+Mouse Wheel (Ctrl on Windows and Linux): off by default." });
    schema["properties"]["wheelFontSizeIn"] = json!({ "type": "string", "enum": ["activeEditor", "allEditors"], "description": "Editor → General → Mouse Control: the wheel changes the font size of the active editor alone (editor.fontSize.increase and .decrease, dropped when the file closes) or of all editors (view.editorFontSize.increase and .decrease, written to editorFont.size); activeEditor by default." });
    schema["properties"]["zoomIndicator"] = json!({ "type": "boolean", "description": "Advanced Settings → Editor → Show the zoom level when it changes: after the editor font size changes, the bottom of the editor shows it with the default size and Reset (on by default)." });
    schema["properties"]["codeStyleGeneral"] = json!({ "type": "object", "description": "Editor → Code Style → General and Formatter of the Default (IDE) scheme: hardWrapAt (the right margin, 120), wrapOnTyping, visualGuides (columns), doNotFormat (file globs left alone by Reformat Code, Optimize Imports and the save actions; \"*\" is every file), formatterMarkers (text from @formatter:off to @formatter:on keeps its form). The project's scheme wins where it sets them; project.codeStyle.get and project.codeStyle.set read and write that one." });
    schema["properties"]["codeStyleCopy"] = json!({ "type": ["string", "null"], "description": "The copy of the IDE scheme in use while codeStyleScheme is default: a name in codeStyleSchemes; null is Default (IDE)." });
    schema["properties"]["codeStyleSchemes"] = json!({ "type": "object", "description": "The copies of the IDE code style scheme by name (Duplicate…, Copy to IDE…, Import Scheme…): languages (as codeStyle) and general (as codeStyleGeneral). project.codeStyle.export and project.codeStyle.import read and write IntelliJ code style XML." });
    schema["properties"]["reformatOnSaveFiles"] = json!({ "type": "array", "items": { "type": "string" }, "description": "Actions on Save for new projects: the language ids Reformat code applies to; empty is every language." });
    schema["properties"]["reformatOnSaveScope"] = json!({ "type": "string", "enum": ["wholeFile", "changedLines"], "description": "Actions on Save for new projects: reformat the whole file, or only the lines that differ from the last commit." });
    schema["properties"]["optimizeImportsOnSaveFiles"] = json!({ "type": "array", "items": { "type": "string" }, "description": "Actions on Save for new projects: the language ids Optimize imports applies to; empty is every language." });
    schema["properties"]["customization"] = json!({ "type": "object", "description": "Appearance & Behavior → Menus and Toolbars, kept in customization.json next to settings.json. groups: a group's whole item list by id: mainToolbar, a popup menu such as editorTabPopup or projectViewPopup, or a menu key such as menu-file, as ui.customization.list names them; an item is an action or command id, - for a separator, slot:<built-in item of a popup menu>, submenu:<menu key>, special:new or special:layouts. An unknown id is skipped; a group left out keeps its defaults. The ui.customization commands change one item at a time. icons: an action's icon by id: run, search, settings, folder, branch, commit, bulb or more." });
    schema["properties"]["lineSeparator"] = json!({ "type": ["string", "null"], "enum": ["lf", "crlf", "cr", null], "description": "Editor → Code Style → General → Line separator: the separator of new files and of files without line breaks, unless an .editorconfig end_of_line says otherwise; null is System-Dependent (LF on macOS and Linux). Existing files keep theirs: file.lineSeparator.set changes them." });
    schema["properties"]["globalEncoding"] = json!({ "type": ["string", "null"], "description": "Editor → File Encodings → Global Encoding: the encoding of files nothing else names (a byte order mark, .editorconfig, the Path/Encoding table, Project Encoding); null is UTF-8. Names come from file.encoding.list." });
    schema["properties"]["openDeclarationInMain"] = json!({ "type": "boolean", "description": "Advanced Settings → Editor Tabs → Open declarations reached from a detached window in the main window (on by default): off opens it in the detached window." });
    schema["properties"]["editorConfig"] = json!({ "type": "boolean", "description": "Editor → Code Style → General → Enable EditorConfig support (on by default): the project's .editorconfig files set the indentation, tab width, right margin, line separator, charset and save properties of the files they match, above the project's and the IDE's code style and On Save; off ignores every .editorconfig file. project.editorConfig.get tells what they give a file." });
    schema["properties"]["editorAppearance"] = editor_appearance;
    schema["properties"]["lineNumbers"] = json!({ "type": "boolean", "description": "Editor → General → Appearance: show the line numbers in the gutter (on by default); View → Active Editor → Show Line Numbers (editor.lineNumbers.toggle) overrides it for one editor." });
    schema["properties"]["lineNumbersMode"] = json!({ "type": "string", "enum": ["absolute", "relative", "hybrid"], "description": "Editor → General → Appearance → Show line numbers: absolute numbers, the distance from the caret's line, or the caret's line by its number and the others by their distance (absolute by default)." });
    schema["properties"]["methodSeparators"] = json!({ "type": "boolean", "description": "Editor → General → Appearance: draw a thin line above each top-level function, method and type declaration (off by default; editor.methodSeparators.list lists them)." });
    schema["properties"]["ideZoom"] = json!({
        "type": "integer",
        "minimum": IDE_ZOOMS.start(),
        "maximum": IDE_ZOOMS.end(),
        "description": format!(
            "Appearance → Accessibility → Zoom (View → Appearance → Zoom IDE): the scale of the whole IDE, the editor included, in percent, on top of uiFontSize (100 by default; presets {}). The actions view.ideZoom.in, view.ideZoom.out and view.ideZoom.reset step it.",
            IDE_ZOOM_PRESETS.map(|preset| preset.to_string()).join(", ")
        ),
    });
    schema["properties"]["detectIndents"] = json!({ "type": "boolean", "description": "Editor → Code Style → General → Follow the indentation a file already has: a file is edited with the indentation its own lines use (tabs, or 2, 3, 4 or 8 spaces) instead of the configured one (on by default; off returns every open file to the configured style; editor.indent.get tells what a file uses)." });
    schema["properties"]["navigationBar"] = json!({
        "type": "string",
        "enum": ["top", "statusBar", "hidden"],
        "description": "View → Appearance → Navigation Bar: Top (a bar under the toolbar), In Status Bar (the default) or Don't Show (hidden; view.navigationBar.jump, ⌘↑, shows it over the editor).",
    });
    schema["properties"]["navigationBarMembers"] = json!({
        "type": "boolean",
        "description": "View → Appearance → Navigation Bar → Show Members: the declarations at the caret follow the file in the navigation bar (on by default).",
    });
    schema["properties"]["autosaveOnIdle"] = json!({
        "type": "boolean",
        "description": "Appearance & Behavior → System Settings → Files → Save files if the IDE is idle for autosaveIdleSeconds seconds: once nothing was typed, clicked or scrolled and the modified files did not change for that long, every modified file is saved (on by default).",
    });
    schema["properties"]["autosaveIdleSeconds"] = json!({
        "type": "integer",
        "minimum": AUTOSAVE_IDLE_SECONDS.start(),
        "maximum": AUTOSAVE_IDLE_SECONDS.end(),
        "description": "The seconds without input before the idle autosave (15 by default).",
    });
    schema["properties"]["autosaveOnFrameDeactivation"] = json!({
        "type": "boolean",
        "description": "Appearance & Behavior → System Settings → Files → Save files when focus moves to another application or a built-in terminal: leaving the IDE's window, or the Terminal getting the keyboard, saves every modified file (on by default).",
    });
    schema["properties"]["camelHumps"] = json!({ "type": "boolean", "description": "Editor → General → Smart Keys: Use \"CamelHumps\" words: ⌥← ⌥→ (with ⇧), ⌥⌫ ⌥⌦ (editor.caret.move wordLeft/wordRight, editor.edit.deleteWord*) stop at camel humps and after _, Bar in fooBarBaz (off by default)." });
    schema["properties"]["camelHumpsOnDoubleClick"] = json!({ "type": "boolean", "description": "Editor → General → Smart Keys: A double-click selects CamelHumps words while they are turned on: with camelHumps on, a double click (editor.selection.word) selects the camel hump instead of the identifier (on by default)." });
    schema["properties"]["extendSelectionWhitespace"] = json!({
        "type": "boolean",
        "description": "Advanced Settings → Editor → Let Extend Selection take in whitespace: \
            editor.selection.extend takes the whitespace at a caret that is not at a word first (off by default).",
    });
    schema["properties"]["searchTextOnAll"] = json!({ "type": "boolean", "description": "Advanced Settings → Search Everywhere: list the Text results at the bottom of All when the other groups found fewer than 6 items (on by default)." });
    schema["properties"]["bookmarksPopupLineOnly"] = json!({
        "type": "boolean",
        "description": "Advanced Settings → Bookmarks → List only line bookmarks in the popup: Show Line Bookmarks \
            (view.bookmark.show, ⌘F3) lists the line bookmarks only, without the breakpoints (on by default).",
    });
    schema["properties"]["findInFilesLimit"] = json!({
        "type": "integer",
        "minimum": 1,
        "maximum": zen_workbench::search::MAX_POPUP_LIMIT,
        "description": "Advanced Settings → Find in Files: the most matches the Find in Files popup shows; past it the count reads \"100+ matches in 29+ files\" (100 by default).",
    });
    schema["properties"]["findInFilesTooMany"] = json!({
        "type": "integer",
        "minimum": 1,
        "maximum": zen_workbench::search::MAX_TOO_MANY,
        "description": "Advanced Settings → Find in Files: how many matches Open in Find Window lists before Too Many Results asks whether to go on (1000 by default).",
    });
    schema["properties"]["removeTrailingSpaces"] = json!({ "type": "boolean", "description": "Editor → General → On Save → Remove trailing spaces on: remove trailing spaces when a file is saved (on by default); a file's .editorconfig trim_trailing_whitespace overrides it. editor.onSave.options shows what applies to a file." });
    schema["properties"]["removeTrailingSpacesOn"] = json!({ "type": "string", "enum": ["modified", "all"], "description": "Editor → General → On Save: from which lines trailing spaces go: modified (the lines changed since the last save, the default) or all." });
    schema["properties"]["keepTrailingSpacesOnCaretLine"] = json!({ "type": "boolean", "description": "Editor → General → On Save → Keep trailing spaces on caret line: every line with a caret keeps its trailing spaces and is not removed as a trailing blank line (on by default)." });
    schema["properties"]["removeTrailingBlankLines"] = json!({ "type": "boolean", "description": "Editor → General → On Save → Drop the blank lines at the end of a file when it is saved (off by default)." });
    schema["properties"]["ensureLineBreakAtEnd"] = json!({ "type": "boolean", "description": "Editor → General → On Save → End each file with a line break when it is saved (off by default); a file's .editorconfig insert_final_newline overrides it." });
    schema["properties"]["spelling"] = json!({
        "type": ["object", "null"],
        "description": "Editor → Inspections → Proofreading → Spelling: misspelt words, problems of severity typo \
            (problems.file.list, spelling.typos.list); Editor → Natural Languages → Spelling: the dictionaries. Null \
            brings back the defaults.",
        "properties": {
            "enabled": { "type": "boolean", "description": "Check spelling at all (on by default)." },
            "processCode": { "type": "boolean", "description": "Process code: the names where they are declared (on by default)." },
            "processLiterals": { "type": "boolean", "description": "Process literals: string literals (on by default)." },
            "processComments": { "type": "boolean", "description": "Process comments (on by default). Plain text and Markdown are always checked." },
            "customDictionaries": { "type": "array", "items": { "type": "string" }, "description": "Editor → Natural Languages → Spelling → Custom dictionaries: the paths of plain word lists (one word a line) and Hunspell .dic files with their .aff beside them; spelling.dictionaries.get says how each was read." },
            "useSingleDictionary": { "type": "boolean", "description": "Use single dictionary for saving words (off by default): Save to dictionary saves to savingDictionary without asking; off, the window asks (Select Dictionary) and commands without a level save to application." },
            "savingDictionary": { "type": "string", "enum": ["application", "project"], "description": "The dictionary words are saved to while useSingleDictionary is on: project (.idea/dictionaries/project.xml, shared with JetBrains IDEs, the default) or application (the IDE's own)." },
        },
        "additionalProperties": false,
    });
    schema["properties"]["editorTabs"] = editor_tabs_schema();
    schema["properties"]["todo"] = todo_schema();
    schema["properties"]["runConfirmRerun"] = json!({ "type": "boolean", "description": "Advanced Settings → Run/Debug → Ask before a rerun stops the running process: the IDE's window asks before starting a configuration that is running and does not allow multiple instances again (on by default; agents are never asked)." });
    schema["properties"]["runTemporaryLimit"] = json!({ "type": "integer", "minimum": 1, "maximum": 100, "description": "Advanced Settings → Run/Debug → Keep at most this many temporary configurations: how many temporary run configurations are kept, the oldest going first (5 by default)." });
    schema["properties"]["projectViewScratches"] = json!({ "type": "boolean", "description": "Advanced Settings → Project View → Show Scratches and Consoles in the Project tool window: the Project tool window shows the scratch files and buffers (on by default)." });
    schema["properties"]["markdownHideFloatingToolbar"] = json!({ "type": "boolean", "description": "Advanced Settings → Markdown → Hide the formatting toolbar over a selection: no toolbar over selected text in Markdown files (off by default)." });
    schema["properties"]["disableDoubleModifiers"] = json!({ "type": "boolean", "description": "Advanced Settings → User Interface → Turn off shortcuts made of a modifier pressed twice: turns off ⇧⇧ (Search Everywhere) and ⌃⌃ (Run Anything) and any other double modifier shortcut (off by default)." });
    schema["properties"]["mcpRedactFiles"] = json!({ "type": "array", "items": { "type": "string" }, "description": "Tools → MCP Server → Files hidden from agents: patterns of project-relative paths, file names (`*.pem`) or folders whose text agents never get over MCP; `!` before a pattern keeps the files it names. Environment files, keys and credential files by default. Only the user may change it: set_settings refuses it." });
    schema["properties"]["codeStyleScheme"] = json!({ "type": "string", "enum": ["project", "default"], "description": "Editor → Code Style → Scheme, also Quick Switch Scheme → Code Style Scheme: project (the default) lets the code style a project keeps in .idea win where it sets one; default uses only codeStyle." });
    schema["properties"]["codeStyleOptions"] = json!({
        "type": "object",
        "description": "Editor → Code Style → <language> with the Default (IDE) scheme: the values of the options a language declares, by language and option id ({\"rust\": {\"max_width\": 80}, \"go\": {\"gofumpt\": true}}; editor.codeStyle.list lists the options, their values and defaults). The key `arrangement` is the order of Rearrange Code, a list of arrangement entry ids. Values are merged per option; a null removes a language or an option, which then takes its default. A value that is not valid for its option is ignored. With the Project scheme, a language with a configuration file (rustfmt.toml, .zen/java-formatter.xml) reads the options the file keeps from it instead; editor.codeStyle.writeProject writes it.",
        "additionalProperties": { "type": "object" },
    });
    schema["properties"]["rearrangeOnReformat"] = json!({ "type": "boolean", "description": "Editor → Code Style → <language> → Arrangement: Rearrange code on Reformat Code: editor.code.reformat runs editor.code.rearrange first (off by default)." });
    schema["properties"]["sqlFormat"] = json!({
        "type": "object",
        "description": "Editor → Code Style → SQL: what Reformat Code does to SQL files and consoles.",
        "properties": {
            "keywordCase": { "type": "string", "enum": ["asIs", "upper", "lower"], "description": "How reserved keywords are written (asIs by default)." },
            "commas": { "type": "string", "enum": ["trailing", "leading"], "description": "Where commas between list items go (trailing by default)." },
            "clauseOnOwnLine": { "type": "boolean", "description": "A clause keyword stands on its own line with its items below it (off by default)." },
            "itemPerLine": { "type": "boolean", "description": "Each item of a select list, GROUP BY, ORDER BY, SET or VALUES has a line of its own (on by default)." },
        },
        "additionalProperties": false,
    });
    schema["properties"]["coroutineDebugMode"] = json!({ "type": "boolean", "description": "Settings → Build, Execution, Deployment → Debugger → Kotlin coroutines → Debug mode when debugging (on by default): a Java or Kotlin application started with Debug, never with Run, gets -Dkotlinx.coroutines.debug, so the threads of the program carry the name and number of the coroutine they run (main @coroutine#1). A run configuration's coroutineDebug (default, on, off) overrides it." });
    schema["properties"]["removeBreakpointByDrag"] = json!({ "type": "boolean", "description": "Settings → Build, Execution, Deployment → Debugger → Remove breakpoint: false (the default) removes a breakpoint with a click on it; true removes it only by dragging it to the editor or with a middle click, and a click leaves it." });
    schema["properties"]["valueTooltip"] = json!({ "type": "boolean", "description": "Settings → Build, Execution, Deployment → Debugger → Data Views → Show value tooltip: while the program is stopped, resting the pointer on a name in the editor shows its value (debug.expression.evaluate in the hover context). On by default." });
    schema["properties"]["valueTooltipDelay"] = json!({ "type": "integer", "minimum": 0, "maximum": 10000, "description": "Settings → Build, Execution, Deployment → Debugger → Data Views → Value tooltip delay, in milliseconds (700 by default)." });
    schema["properties"]["collectionViews"] = json!({ "type": "boolean", "description": "Settings → Build, Execution, Deployment → Debugger → Data Views → Enable alternative view for Collections classes: collections, maps and arrays show their elements, map entries as key → value, with a debugger that can (java-debug); off shows their raw fields (debug.variables.raw switches it for the session). On by default." });
    schema["properties"]["inlineValues"] = json!({ "type": "boolean", "description": "Settings → Build, Execution, Deployment → Debugger → Data Views → Show values inline: while the program is stopped, the editor shows the selected frame's variables at the end of the lines of its function (debug.inlineValues.get), a value changed since the previous stop highlighted. On by default." });
    schema["properties"]["runToCursorInlay"] = json!({ "type": "boolean", "description": "Advanced Settings → Debugger → Show a Run to Cursor button on lines while debugging: while the program is stopped, a line under the pointer shows a Run to Cursor button at its start (debug.execution.runToCursor). On by default." });
    schema["properties"]["recentExpressions"] = json!({ "type": "integer", "minimum": 1, "maximum": 1000, "description": "Advanced Settings → Debugger → Recent expressions to remember debug.expression.history keeps for the project (50 by default)." });
    schema["properties"]["powerSaveMode"] = json!({ "type": "boolean", "description": "File → Power Save Mode: while on, on-the-fly checks (language servers' diagnostics, inspections), the completion popup while typing, go.mod's network checks and reading changed files into the index pause; gopls checks only on save. Turning it off checks the open files again. Off by default; the action view.powerSaveMode.toggle switches it, powerSave.status.get says what it holds back." });
    let widgets: Map<String, Value> = super::status_widgets::StatusWidget::ALL
        .iter()
        .map(|widget| (widget.id().to_owned(), json!({ "type": ["boolean", "null"] })))
        .collect();
    schema["properties"]["statusBarWidgets"] = json!({
        "type": ["object", "null"],
        "description": "The status bar widgets (its right-click menu) shown (true) or hidden (false); null brings back the default: every widget shown but powerSaveMode and memoryIndicator.",
        "properties": widgets,
        "additionalProperties": false,
    });
    schema["properties"]["openProjectIn"] = json!({ "enum": ["newWindow", "currentWindow", "ask", null], "description": "System Settings → Open project in; Current window is a new tab of the main window." });
    schema["properties"]["defaultProjectDirectory"] = json!({ "type": "string", "description": "System Settings → Default project directory; empty for ~/Projects." });
    schema["properties"]["generatorsNetwork"] = json!({ "type": "boolean", "description": "An agent can turn it off, not on. System Settings → Project Generators: whether project generators may use the network (on by default; off: forms come from saved data and generators that need a service say so)." });
    schema["properties"]["generatorSettings"] = json!({ "type": "object", "additionalProperties": { "type": "string" }, "description": "Read only for agents. System Settings → Project Generators: the values of generators' text settings by key (`settings[].key` of project.new.generators), e.g. spring-boot.initializr.serviceUrl; a key left out has its default." });
    schema["properties"]["mcpLanguage"] = json!({ "type": "string", "enum": mcp_languages, "description": "Tools → MCP Server → Language for agents: the language of the errors, reasons and messages agents get over MCP (en by default, whatever the UI language); error codes are the same in every language." });
    schema
}

pub(super) fn merge_settings(current: &IdeSettings, changes: &Value) -> Result<IdeSettings, String> {
    let Some(changes) = changes.as_object() else { return Err(t!("mcp-settings-not-object")) };
    let mut merged = to_object(current);
    let defaults = to_object(&IdeSettings::default());
    for (key, value) in changes {
        match (key.as_str(), value) {
            ("declaredSettings", Value::Object(values)) => {
                let target = merged.entry(key.clone()).or_insert_with(|| json!({}));
                if !target.is_object() {
                    *target = json!({});
                }
                let Some(target) = target.as_object_mut() else { continue };
                for (name, value) in values {
                    match value {
                        Value::Null => {
                            target.remove(name);
                        }
                        Value::Bool(_) => {
                            target.insert(name.clone(), value.clone());
                        }
                        _ => {
                            let error = format!("declaredSettings.{name} is on or off: true or false");
                            return Err(t!("mcp-settings-invalid", error = error));
                        }
                    }
                }
            }
            ("codeStyleOptions", Value::Object(languages)) => {
                let target = merged.entry(key.clone()).or_insert_with(|| json!({}));
                if !target.is_object() {
                    *target = json!({});
                }
                let Some(target) = target.as_object_mut() else { continue };
                merge_code_style_options(target, languages)?;
            }
            ("generatorSettings", Value::Object(values)) => {
                let target = merged.entry(key.clone()).or_insert_with(|| json!({}));
                let Some(target) = target.as_object_mut() else { continue };
                for (name, value) in values {
                    match value {
                        Value::Null => {
                            target.remove(name);
                        }
                        Value::String(_) => {
                            target.insert(name.clone(), value.clone());
                        }
                        _ => {
                            let error = format!("generatorSettings.{name} is text");
                            return Err(t!("mcp-settings-invalid", error = error));
                        }
                    }
                }
            }
            ("codeStyle", Value::Object(styles)) => {
                let target = merged.entry(key.clone()).or_insert_with(|| json!({}));
                if !target.is_object() {
                    *target = json!({});
                }
                if let Some(target) = target.as_object_mut() {
                    merge_code_style(current, target, styles)?;
                }
            }
            ("customization", Value::Object(changes)) => {
                let target = merged.entry(key.clone()).or_insert_with(|| json!({}));
                for field in ["groups", "icons"] {
                    let (Some(entries), Some(values)) = (
                        target.get_mut(field).and_then(Value::as_object_mut),
                        changes.get(field).and_then(Value::as_object),
                    ) else {
                        continue;
                    };
                    for (name, value) in values {
                        if value.is_null() {
                            entries.remove(name);
                        } else {
                            entries.insert(name.clone(), value.clone());
                        }
                    }
                }
            }
            _ => merge_field(&mut merged, &defaults, key, value, key)?,
        }
    }
    let settings: IdeSettings = serde_json::from_value(Value::Object(merged))
        .map_err(|error| t!("mcp-settings-invalid", error = error.to_string()))?;
    validate(current, &settings)?;
    Ok(settings)
}

fn merge_code_style_options(target: &mut Map<String, Value>, languages: &Map<String, Value>) -> Result<(), String> {
    for (language, values) in languages {
        match values {
            Value::Null => {
                target.remove(language);
            }
            Value::Object(values) => {
                let entry = target.entry(language.clone()).or_insert_with(|| json!({}));
                if !entry.is_object() {
                    *entry = json!({});
                }
                let Some(entry) = entry.as_object_mut() else { continue };
                for (id, value) in values {
                    if value.is_null() {
                        entry.remove(id);
                    } else {
                        entry.insert(id.clone(), value.clone());
                    }
                }
                if entry.is_empty() {
                    target.remove(language);
                }
            }
            _ => {
                let error = format!("codeStyleOptions.{language} is an object of option values");
                return Err(t!("mcp-settings-invalid", error = error));
            }
        }
    }
    Ok(())
}

fn to_object<T: serde::Serialize>(value: &T) -> Map<String, Value> {
    match serde_json::to_value(value) {
        Ok(Value::Object(object)) => object,
        _ => Map::new(),
    }
}

fn merge_field(
    target: &mut Map<String, Value>,
    defaults: &Map<String, Value>,
    key: &str,
    value: &Value,
    field: &str,
) -> Result<(), String> {
    let Some(default) = defaults.get(key) else { return Err(t!("mcp-settings-unknown", field = field)) };
    match (value, default) {
        (Value::Null, _) => {
            target.insert(key.to_owned(), default.clone());
        }
        (Value::Object(changes), Value::Object(default_fields)) => {
            let entry = target.entry(key.to_owned()).or_insert_with(|| default.clone());
            if !entry.is_object() {
                *entry = default.clone();
            }
            if let Some(entry) = entry.as_object_mut() {
                for (name, change) in changes {
                    merge_field(entry, default_fields, name, change, &format!("{field}.{name}"))?;
                }
            }
        }
        _ => {
            target.insert(key.to_owned(), value.clone());
        }
    }
    Ok(())
}

fn merge_code_style(
    current: &IdeSettings,
    target: &mut Map<String, Value>,
    styles: &Map<String, Value>,
) -> Result<(), String> {
    for (language, style) in styles {
        if style.is_null() {
            target.remove(language);
            continue;
        }
        let mut entry = Map::new();
        entry.insert(
            language.clone(),
            target.get(language).cloned().unwrap_or_else(|| json!(to_object(&current.code_style_for(language)))),
        );
        let mut defaults = Map::new();
        defaults.insert(language.clone(), json!(to_object(&CodeStyle::default_for(language))));
        merge_field(&mut entry, &defaults, language, style, &format!("codeStyle.{language}"))?;
        target.extend(entry);
    }
    Ok(())
}

pub(super) fn validate(current: &IdeSettings, settings: &IdeSettings) -> Result<(), String> {
    let range = |field: &str, min: u32, max: u32| t!("mcp-settings-range", field = field, min = min, max = max);
    if !(1..=10_000).contains(&settings.code_style_general.hard_wrap_at) {
        return Err(range("codeStyleGeneral.hardWrapAt", 1, 10_000));
    }
    if settings.mcp_redact_files != current.mcp_redact_files {
        return Err(t!("mcp-settings-redact-refused"));
    }
    if settings.proxy != current.proxy
        || (settings.accept_untrusted_certificates && !current.accept_untrusted_certificates)
    {
        return Err(t!("mcp-settings-network-refused"));
    }
    if settings.terminal_agents != current.terminal_agents {
        return Err(t!("mcp-settings-agents-refused"));
    }
    if settings.generator_settings != current.generator_settings
        || (settings.generators_network && !current.generators_network)
    {
        return Err(t!("mcp-settings-generators-refused"));
    }
    let font_sizes = zen_workbench::preview::MARKDOWN_FONT_SIZES;
    if !font_sizes.contains(&settings.markdown.preview_font_size) {
        return Err(range("markdown.previewFontSize", *font_sizes.start() as u32, *font_sizes.end() as u32));
    }
    let caffeine = &settings.caffeine;
    let max = zen_workbench::caffeine::MAX_MINUTES;
    if caffeine != &current.caffeine {
        if !(1..=max).contains(&caffeine.custom_minutes) {
            return Err(range("caffeine.customMinutes", 1, max));
        }
        if !(5..=95).contains(&caffeine.low_battery_level) {
            return Err(range("caffeine.lowBatteryLevel", 5, 95));
        }
        if let zen_workbench::caffeine::Length::Minutes { minutes } = caffeine.length
            && minutes > max
        {
            return Err(range("caffeine.length.minutes", 1, max));
        }
    }
    if settings.ui_font_size != current.ui_font_size
        && settings.ui_font_size.is_some_and(|size| !UI_FONT_SIZES.contains(&size))
    {
        return Err(range("uiFontSize", 8, 32));
    }
    if settings.ide_zoom != current.ide_zoom && !IDE_ZOOMS.contains(&settings.ide_zoom) {
        return Err(range("ideZoom", u32::from(*IDE_ZOOMS.start()), u32::from(*IDE_ZOOMS.end())));
    }
    if settings.presentation_font_size != current.presentation_font_size
        && !PRESENTATION_FONT_SIZES.contains(&settings.presentation_font_size)
    {
        return Err(range("presentationFontSize", 8, 96));
    }
    if settings.clipboard_limit != current.clipboard_limit
        && !(1..=zen_workbench::clipboard_history::MAX_LIMIT).contains(&settings.clipboard_limit)
    {
        return Err(range("clipboardLimit", 1, zen_workbench::clipboard_history::MAX_LIMIT as u32));
    }
    if settings.console_buffer_kb != current.console_buffer_kb
        && !(1..=zen_run::MAX_BUFFER_KB).contains(&settings.console_buffer_kb)
    {
        return Err(range("consoleBufferKb", 1, zen_run::MAX_BUFFER_KB as u32));
    }
    if settings.find_in_files_limit != current.find_in_files_limit
        && !(1..=zen_workbench::search::MAX_POPUP_LIMIT).contains(&settings.find_in_files_limit)
    {
        return Err(range("findInFilesLimit", 1, zen_workbench::search::MAX_POPUP_LIMIT as u32));
    }
    if settings.editor_tabs.limit != current.editor_tabs.limit
        && !(1..=zen_workbench::tabs::MAX_TAB_LIMIT).contains(&settings.editor_tabs.limit)
    {
        return Err(range("editorTabs.limit", 1, zen_workbench::tabs::MAX_TAB_LIMIT as u32));
    }
    if let Some(bad) =
        settings.todo.patterns.iter().find(|pattern| !zen_workbench::todo_patterns::is_valid(&pattern.pattern))
    {
        let error = format!("todo.patterns: `{}` is not a regular expression", bad.pattern);
        return Err(t!("mcp-settings-invalid", error = error));
    }
    if settings.run_temporary_limit != current.run_temporary_limit
        && !(1..=zen_workbench::run::MAX_TEMPORARY_LIMIT).contains(&settings.run_temporary_limit)
    {
        return Err(range("runTemporaryLimit", 1, zen_workbench::run::MAX_TEMPORARY_LIMIT as u32));
    }
    if settings.find_in_files_too_many != current.find_in_files_too_many
        && !(1..=zen_workbench::search::MAX_TOO_MANY).contains(&settings.find_in_files_too_many)
    {
        return Err(range("findInFilesTooMany", 1, zen_workbench::search::MAX_TOO_MANY as u32));
    }
    if settings.sticky_lines_max != current.sticky_lines_max
        && !(1..=zen_workbench::sticky::MAX_LIMIT).contains(&settings.sticky_lines_max)
    {
        return Err(range("stickyLinesMax", 1, zen_workbench::sticky::MAX_LIMIT as u32));
    }
    if settings.autosave_idle_seconds != current.autosave_idle_seconds
        && !AUTOSAVE_IDLE_SECONDS.contains(&settings.autosave_idle_seconds)
    {
        return Err(range("autosaveIdleSeconds", *AUTOSAVE_IDLE_SECONDS.start(), *AUTOSAVE_IDLE_SECONDS.end()));
    }
    if settings.mcp_port != current.mcp_port && settings.mcp_port < 1024 {
        return Err(range("mcpPort", 1024, 65535));
    }
    if settings.mcp_language != current.mcp_language
        && !zen_i18n::LANGUAGES.iter().any(|(code, _)| *code == settings.mcp_language)
    {
        let codes: Vec<&str> = zen_i18n::LANGUAGES.iter().map(|(code, _)| *code).collect();
        return Err(t!("mcp-settings-one-of", field = "mcpLanguage", values = codes.join(", ")));
    }
    if settings.editor_font.size != current.editor_font.size && settings.editor_font.size <= 0.0 {
        return Err(t!("mcp-settings-positive", field = "editorFont.size"));
    }
    if settings.editor_font.line_spacing != current.editor_font.line_spacing && settings.editor_font.line_spacing <= 0.0
    {
        return Err(t!("mcp-settings-positive", field = "editorFont.lineSpacing"));
    }
    for (language, style) in &settings.code_style {
        if current.code_style.get(language) == Some(style) {
            continue;
        }
        if style.tab_size == 0 {
            return Err(range(&format!("codeStyle.{language}.tabSize"), 1, 255));
        }
        if style.indent == 0 {
            return Err(range(&format!("codeStyle.{language}.indent"), 1, 255));
        }
    }
    Ok(())
}

pub(super) fn tools() -> Vec<Tool> {
    let project =
        json!({ "type": "string", "description": "The project's folder, absolute; optional with one project open." });
    let tool = |name: &str, description: &str, input_schema: Value| Tool {
        name: name.to_owned(),
        description: description.to_owned(),
        input_schema,
        output_schema: None,
    };
    vec![
        tool(
            "list_windows",
            "Lists the open project windows: each project's folder, its open documents (path, unsaved changes) and the active one.",
            object(json!({}), &[]),
        ),
        tool(
            "list_commands",
            "Lists the IDE's commands. Without arguments: every command id by group (`editor`: [\"text.replace\", ...] \
             is editor.text.replace). With `group` or `query` (words to find in the id, title or description): \
             those commands' id, title and one-line description. describe_command gives one command's parameters; \
             `full: true` lists every command with its description and schema at once (large).",
            object(
                json!({
                    "group": { "type": "string", "description": "A group, the first part of the ids: editor, file, navigate, search, vcs..." },
                    "query": { "type": "string", "description": "Words that must all occur in the id, title or description, e.g. \"rename\"." },
                    "full": { "type": "boolean", "description": "Every command with its full description and parameter schema, as earlier versions answered." }
                }),
                &[],
            ),
        ),
        tool(
            "describe_command",
            "Returns one command's id, title, full description and the JSON schema of its parameters.",
            object(
                json!({ "id": { "type": "string", "description": "A command id, e.g. editor.text.replace." } }),
                &["id"],
            ),
        ),
        tool(
            "dispatch_command",
            "Runs a command by id in a project window, as if the user had chosen it, and returns its result. \
             An id that is a keymap action instead (view.settings.open, editor.caret.move/wordLeft) runs as run_action does.",
            object(
                json!({
                    "command": { "type": "string", "description": "The command id, e.g. file.document.open." },
                    "params": { "type": "object", "description": "The parameters, matching the command's schema." },
                    "project": project,
                }),
                &["command"],
            ),
        ),
        tool(
            "open_project",
            "Opens a folder as a project in a new window (or focuses it if it is open); a file opens its folder and the file. A project not trusted opens in safe mode (project.trust.state).",
            object(json!({ "path": { "type": "string", "description": "An absolute path." } }), &["path"]),
        ),
        tool(
            "focus_project",
            "Brings a project's window to the front.",
            object(json!({ "project": project }), &["project"]),
        ),
        tool(
            "close_project",
            "Closes a project's window. Unsaved changes are saved first, as Save All saves them; when a file cannot be saved, \
             the window stays open with its changes and the call fails saying why.",
            object(json!({ "project": project }), &["project"]),
        ),
        tool(
            "exit_ide",
            "Exits the IDE, as ⌘Q or Quit from the application menu does; the open projects open again at the next start. \
             With confirmed: true it exits at once, without the Confirm Exit dialog. Without it, the dialog asks the person in \
             front (unless Settings → System Settings → Confirm before exiting the IDE is off, confirmExit in set_settings) and \
             get_window_state lists it as the popup confirmExit; press_keys ⏎ exits, ⎋ cancels. Changed files are saved first, \
             as Save All saves them; when one cannot be saved the IDE stays and the call fails saying why. Answers whether it \
             exits now and the work still running that exiting stops: background tasks (dependency updates, Git operations), runs and \
             debug sessions.",
            object(
                json!({ "confirmed": { "type": "boolean", "description": "Exit without asking (false by default)." } }),
                &[],
            ),
        ),
        tool(
            "list_plugins",
            "Lists the installed plug-ins and the plug-ins of the catalog (Settings → Plugins), with versions, release notes and file extensions; `bundled` tells whether a plug-in ships with this IDE (it can be switched off, not uninstalled), a catalog entry's `installed` is the version installed here, and `installing` the ids being installed now. A plug-in bundled with another IDE installs from the catalog with install_plugin. The plugins.* commands (dispatch_command) also search the catalog, show a plug-in's notes and update all plug-ins at once.",
            object(json!({}), &[]),
        ),
        tool(
            "install_plugin",
            "Installs a plug-in from the catalog by id, or updates an installed one; it downloads in the background, so check list_plugins. The language servers the plug-in declares are installed with it when missing (lsp.servers.list, source local). A plug-in that brings programs running on this computer (a database driver) is refused (userOnly): the user installs it from Settings → Plugins.",
            object(json!({ "id": { "type": "string" } }), &["id"]),
        ),
        tool(
            "set_plugin_enabled",
            "Switches an installed (or bundled) plug-in on or off; one that is off and brings database driver programs or terminal agent adapters only the user switches on.",
            object(json!({ "id": { "type": "string" }, "enabled": { "type": "boolean" } }), &["id", "enabled"]),
        ),
        tool(
            "uninstall_plugin",
            "Removes an installed plug-in by id.",
            object(json!({ "id": { "type": "string" } }), &["id"]),
        ),
        tool(
            "list_themes",
            "Lists the themes (Settings → Appearance): id, name, dark, source (builtin or user), the file and why it cannot be used; and the id of the one in use. The same as the command ui.theme.list.",
            object(json!({}), &[]),
        ),
        tool(
            "select_theme",
            "Uses a theme by id in every window at once and remembers the choice (ui.theme.select).",
            object(json!({ "id": { "type": "string", "description": "A theme id from list_themes." } }), &["id"]),
        ),
        tool(
            "follow_system_theme",
            "Makes the theme follow the system's light or dark appearance (Settings → Appearance → Sync with OS), or stops it; \
             dark and light name the theme for each (ui.theme.followSystem). list_themes shows the state under appearance.",
            object(
                json!({
                    "enabled": { "type": "boolean", "description": "Follow the system's appearance." },
                    "dark": { "type": "string", "description": "The theme while the system is dark; left out, it stays." },
                    "light": { "type": "string", "description": "The theme while the system is light; left out, it stays." },
                }),
                &["enabled"],
            ),
        ),
        tool(
            "import_theme",
            "Imports a theme file: a Zen theme, a JetBrains colour scheme (.icls, .xml) or UI theme (.theme.json), or a VS Code colour theme (.json), \
             into the themes folder; a taken id gets -2, -3 and so on. Returns the id, the name, how many entries were mapped and those that were not (ui.theme.import).",
            object(
                json!({
                    "path": { "type": "string", "description": "The theme file, an absolute path." },
                    "select": { "type": "boolean", "description": "Use the new theme at once; false by default." },
                }),
                &["path"],
            ),
        ),
        tool(
            "list_keymaps",
            "Lists the keymaps (Settings → Keymap): id, name, source (builtin or user), the file, the file it was imported from and why it cannot be used; and the id of the one in use (ui.keymap.list).",
            object(json!({}), &[]),
        ),
        tool(
            "select_keymap",
            "Uses a keymap by id in every window at once and remembers the choice (ui.keymap.select).",
            object(json!({ "id": { "type": "string", "description": "A keymap id from list_keymaps." } }), &["id"]),
        ),
        tool(
            "import_keymap",
            "Imports a keymap .xml exported from a JetBrains IDE as a named keymap; a taken id gets -2, -3 and so on. Returns the id, \
             the name, the parent, the shortcuts, how many actions were mapped, the entries that were not (actionId, reason, shortcut) and warnings (ui.keymap.import).",
            object(
                json!({
                    "path": { "type": "string", "description": "The keymap file, an absolute path." },
                    "select": { "type": "boolean", "description": "Use the new keymap at once; false by default." },
                }),
                &["path"],
            ),
        ),
        tool(
            "get_update_status",
            "Returns the new version of this IDE on offer, if any, with its release notes and the download progress.",
            object(json!({}), &[]),
        ),
        tool(
            "update_ide",
            "Installs the new version on offer; the IDE then restarts into it, reopening the projects. Save first.",
            object(json!({}), &[]),
        ),
        tool(
            "get_settings",
            "Returns every setting of the IDE (Settings dialog, ⌘,), with the same names and shapes that set_settings takes. \
             A null is the default; a language missing from codeStyle takes the built-in code style.",
            object(json!({}), &[]),
        ),
        tool(
            "set_settings",
            "Changes the IDE's settings and applies them at once. Objects merge key by key: fields left out keep their \
             values, so {\"codeStyle\": {\"go\": {\"tabSize\": 8}}} keeps the other languages and Go's other fields. \
             A null brings back the default (in codeStyle, the built-in style of that language). An unknown key is an \
             error and changes nothing. Turning MCP off ends this connection. Returns the settings as applied. \
             The keys are get_settings' own; describe_command {\"id\": \"set_settings\"} gives each one's type and meaning.",
            json!({
                "type": "object",
                "description": "The settings to change, named and shaped as get_settings answers them.",
                "additionalProperties": true,
            }),
        ),
        tool(
            "find_action",
            "Find Action (⇧⌘A): the actions whose names, abbreviations (ui.keymap.abbreviations) or synonyms match a query, \
             best first, with their shortcuts, the command and parameters. run_action runs one by its `action`, as choosing it \
             in Find Action does; set_keymap gives one a shortcut, as ⌥⏎ does.",
            object(
                json!({
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 },
                    "project": project,
                }),
                &["query"],
            ),
        ),
        tool(
            "get_keymap",
            "Returns the keymap in use (Settings → Keymap) and its id: every action with its name, command, parameters, context \
             (global or editor) and keystrokes written as on macOS (⇧⌘F), whether the user changed it, its abbreviations and \
             synonyms (which find_action matches), and conflicts.",
            object(json!({}), &[]),
        ),
        tool(
            "set_keymap",
            "Sets an action's keystrokes in the keymap in use, saved at once; an empty list removes them, null brings back the defaults. \
             The action may also be a command find_action lists without a shortcut; the new one runs it as Find Action does.",
            object(
                json!({
                    "action": { "type": "string", "description": "An action from get_keymap or find_action, e.g. editor.line.duplicate." },
                    "keystrokes": { "type": ["array", "null"], "items": { "type": "string" }, "description": "E.g. [\"⌘D\", \"⌥⇧D\"]." }
                }),
                &["action"],
            ),
        ),
        tool(
            "run_action",
            "Runs an action in a project window as its shortcut, its menu item or Find Action does: a keymap action \
             (get_keymap, find_action), including the window's own view.* actions that open tool windows, popups and \
             dialogs, or a command id. The window runs it in its next frame and answers with the command's result \
             (null for a view.* action), what the window shows afterwards (as get_window_state) and whether that changed. \
             The same action again closes a tool window or popup; press_keys with ⎋ closes a dialog.",
            object(
                json!({
                    "action": { "type": "string", "description": "An action, e.g. view.settings.open, view.toolWindow.problems, editor.caret.move/wordLeft, or a command id." },
                    "params": { "type": "object", "description": "Parameters in place of the action's own." },
                    "project": project,
                }),
                &["action"],
            ),
        ),
        tool(
            "get_window_state",
            "Returns what a project window shows: `left` (project, structure, commit or null) and `bottom` (terminal, \
             problems, find, run, debug, git or null) tool windows, the open `popups` and dialogs (settings:<page>, \
             searchEverywhere:<all|classes|files|symbols|actions|text>, keyboardShortcut (⌥⏎ on an action there), findInFiles, replaceInFiles, tooManyResults, recentFiles, recentLocations, fileStructure, customFolding, pasteFromHistory, quickSwitchScheme, bookmarkMnemonic, goToMnemonic, bookmarkReplace, bookmarkRename, bookmarkList, bookmarkListName, bookmarkListDelete, lineBookmarks, runConfigurations, runAnything, chooseRunConfiguration, chooseDebugConfiguration, breakpointProperties, breakpoints, rename, gotoLine, \
             rollback, newEntry, delete, associateFileType, branches, newBranch, checkoutRevision, updateProject, push, backgroundTasks, zoomIde, trustProject, debugLogSettings, specialFiles, collectLogs, openSourceLicences, invalidateCaches, confirmRestart) and the \
             `editor` (the active file, or diff), `ideZoom` (View → Appearance → Zoom IDE, in percent), and `splits`, the split editors as editor.split.list gives them \
             (null when the editor is not split); `headless` and `session`: project, branch, changes, servers, index.",
            object(json!({ "project": project }), &[]),
        ),
        tool(
            "press_keys",
            "Presses keys in a project window in its next frame, then types text, as the keyboard would there (the \
             system keyboard is not used): e.g. [\"⎋\"] closes a dialog, [\"⇧⌘A\"] opens Find Action, text types into \
             the field that has the keyboard. Keystrokes are written as on macOS (⇧⌘F, ⏎, ⎋, ↓, F2). Answers with \
             what the window shows afterwards.",
            object(
                json!({
                    "keys": { "type": "array", "items": { "type": "string" } },
                    "text": { "type": "string" },
                    "project": project,
                }),
                &[],
            ),
        ),
        tool("list_recent_projects", "Lists the recent projects, most recent first.", object(json!({}), &[])),
        tool(
            "get_tree_selection",
            "Returns the files and folders selected in a project's Project tool window, in the order shown.",
            object(json!({ "project": project }), &[]),
        ),
        tool(
            "select_in_tree",
            "Selects files and folders in a project's Project tool window (unfolding the folders on the way), \
             as the user does with ⌘-click and ⇧-click; the tree's actions (Delete, Copy) then act on them.",
            object(
                json!({ "paths": { "type": "array", "items": { "type": "string" }, "description": "Absolute paths." }, "project": project }),
                &["paths"],
            ),
        ),
        tool(
            "terminal_open",
            "Opens a new terminal tab (the user's login shell) in a project's folder and shows the Terminal tool window; returns the session's `id` and its `tab` index. terminal.session.* and terminal.tab.* also split, list, focus, rename and close sessions.",
            object(json!({ "project": project }), &[]),
        ),
        tool(
            "terminal_write",
            "Types text into the focused session of a terminal tab, as if on the keyboard; end with a line break to press Enter (terminal.session.send).",
            object(
                json!({ "text": { "type": "string" }, "tab": { "type": "integer", "description": "The tab index; the active tab by default." }, "project": project }),
                &["text"],
            ),
        ),
        tool(
            "terminal_read",
            "Returns the visible text of the focused session of a terminal tab, its title and whether its program has exited (terminal.session.read).",
            object(json!({ "tab": { "type": "integer" }, "project": project }), &[]),
        ),
        tool(
            "get_document_text",
            "Returns the text of a file as the editor has it, including unsaved changes; reads the file if it is not open. \
             A range of lines (one-based, inclusive) with startLine/endLine or offset/limit answers with startLine, \
             endLine and totalLines; withLineNumbers gives `N|text` lines (the number and a bar before each line). A file larger \
             than 32 KB is read only by ranges: without one the error `too_large` gives its lines.",
            object(
                json!({
                    "path": { "type": "string", "description": "The file: absolute, or relative to the project." },
                    "startLine": { "type": "integer", "minimum": 1, "description": "The first line, one-based." },
                    "endLine": { "type": "integer", "minimum": 1, "description": "The last line, inclusive; the file's last when absent." },
                    "offset": { "type": "integer", "minimum": 1, "description": "The first line, one-based, with limit." },
                    "limit": { "type": "integer", "minimum": 1, "description": "How many lines from offset." },
                    "withLineNumbers": { "type": "boolean", "description": "Each line as `N|text`: its number and a bar before it." },
                    "project": project
                }),
                &["path"],
            ),
        ),
    ]
}

pub(super) fn describe(project: &Path, workbench: Option<&Workbench>) -> Value {
    let documents: Vec<Value> = workbench
        .map(|workbench| {
            workbench
                .documents()
                .iter()
                .map(|document| json!({ "path": document.path(), "unsaved": document.is_dirty() }))
                .collect()
        })
        .unwrap_or_default();
    let active = workbench.and_then(Workbench::active_document).map(|document| document.path().to_owned());
    json!({ "project": project, "documents": documents, "active": active })
}

pub(super) fn path_argument(arguments: &Value, name: &str) -> Option<PathBuf> {
    arguments.get(name).and_then(Value::as_str).map(PathBuf::from)
}

pub(super) fn project_argument(arguments: &Value) -> Option<PathBuf> {
    path_argument(arguments, "project").map(|path| std::fs::canonicalize(&path).unwrap_or(path))
}

const PATH_PARAMS: &[&str] = &["path", "paths", "file", "files", "directory", "sources", "folder"];

pub(super) fn spelled_params(root: &Path, params: Value) -> Value {
    fn spell(root: &Path, value: Value) -> Value {
        match value {
            Value::String(path) if Path::new(&path).is_absolute() => {
                Value::String(zen_vfs::spelled_from(root, Path::new(&path)).to_string_lossy().into_owned())
            }
            Value::Array(items) => Value::Array(items.into_iter().map(|item| spell(root, item)).collect()),
            other => spelled_params(root, other),
        }
    }
    match params {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let value = if PATH_PARAMS.contains(&key.as_str()) {
                        spell(root, value)
                    } else {
                        spelled_params(root, value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(|item| spelled_params(root, item)).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn paths_through_a_link_are_given_to_commands_in_the_project_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("real");
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::os::unix::fs::symlink(&root, dir.path().join("link")).unwrap();
        let link = dir.path().join("link");
        let spelled = |path: &str| root.join(path).display().to_string();
        let linked = |path: &str| link.join(path).display().to_string();
        let params = json!({
            "path": linked("main.go"),
            "paths": [linked("pkg"), "/elsewhere/x.go"],
            "edits": [{ "file": linked("pkg/a.go"), "text": linked("main.go") }],
            "offset": 3,
        });
        let expected = json!({
            "path": spelled("main.go"),
            "paths": [spelled("pkg"), "/elsewhere/x.go"],
            "edits": [{ "file": spelled("pkg/a.go"), "text": linked("main.go") }],
            "offset": 3,
        });
        assert_eq!(spelled_params(&root, params), expected);
        let named = project_argument(&json!({ "project": link })).unwrap();
        assert_eq!(named, root);
    }
    use crate::ide::settings::EditorFont;

    fn registry() -> Registry {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        registry
    }

    #[test]
    fn the_catalogue_is_ids_by_group_and_descriptions_on_demand() {
        let registry = registry();
        let all = list_commands(&registry, &json!({})).unwrap();
        let ids: usize = all["groups"].as_object().unwrap().values().map(|ids| ids.as_array().unwrap().len()).sum();
        assert_eq!(ids, registry.commands().count(), "every command is in the catalogue");
        assert!(all["groups"]["editor"].as_array().unwrap().contains(&json!("text.replace")));
        let size = all.to_string().len();
        assert!(size < 20 * 1024, "the catalogue without schemas is under 20 KB: {size}");
        let full = list_commands(&registry, &json!({ "full": true })).unwrap();
        assert_eq!(full["commands"].as_array().unwrap().len(), ids, "the full form is kept behind `full`");
        assert!(full["commands"][0]["params"].is_object());

        let editor = list_commands(&registry, &json!({ "group": "editor" })).unwrap();
        let rows = editor["commands"].as_array().unwrap();
        assert!(rows.iter().all(|row| row["id"].as_str().unwrap().starts_with("editor.")));
        assert!(rows.iter().all(|row| row.get("params").is_none()));
        assert!(
            rows.iter().all(|row| row["description"].as_str().unwrap().ends_with(['.', '?', '!'])),
            "whole sentences: {rows:?}"
        );
        let found = list_commands(&registry, &json!({ "query": "rename symbol" })).unwrap();
        let found: Vec<&str> =
            found["commands"].as_array().unwrap().iter().map(|row| row["id"].as_str().unwrap()).collect();
        assert!(found.contains(&"refactor.rename.apply"), "{found:?}");
        assert!(list_commands(&registry, &json!({ "group": "nosuch" })).unwrap_err().contains("editor"));

        let schema = describe_command(&registry, &json!({ "id": "editor.text.replace" })).unwrap();
        assert!(schema["params"]["properties"]["oldText"].is_object(), "{schema}");
        assert!(schema.to_string().len() < 3 * 1024, "one schema is under 3 KB: {}", schema.to_string().len());
        assert_eq!(
            describe_command(&registry, &json!({ "id": "no.such.command" })).unwrap_err(),
            t!("mcp-command-unknown", id = "no.such.command")
        );
    }

    #[test]
    fn the_main_commands_are_tools_of_their_own() {
        let registry = registry();
        let tools = command_tools(&registry);
        assert_eq!(tools.len(), COMMAND_TOOLS.len(), "every command tool is a command");
        let replace = tools.iter().find(|tool| tool.name == "editor_text_replace").unwrap();
        assert!(replace.input_schema["properties"]["oldText"].is_object(), "{}", replace.input_schema);
        assert!(replace.input_schema["properties"]["project"].is_object());
        assert!(replace.description.contains("editor.text.replace"));
        let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
        assert!(names.iter().all(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')), "{names:?}");
        assert!(super::tools().iter().all(|tool| !names.contains(&tool.name)), "no clash with the own tools");

        let call = json!({ "path": "a.rs", "oldText": "a", "newText": "b", "project": "/p" });
        assert_eq!(
            as_dispatch("editor_text_replace", &call),
            Some(
                json!({ "command": "editor.text.replace", "params": { "path": "a.rs", "oldText": "a", "newText": "b" }, "project": "/p" })
            )
        );
        assert_eq!(
            as_dispatch("run_process_start", &json!({ "waitMs": 5 })),
            Some(json!({ "command": "run.process.start", "params": { "waitMs": 5 } }))
        );
        assert_eq!(as_dispatch("list_windows", &json!({})), None);
    }

    #[test]
    fn a_one_line_description_is_the_whole_first_sentence() {
        let cut = "The problems the inspections (e.g. go.mod's checks) found in a file, in order. A problem…";
        assert_eq!(one_line(cut), "The problems the inspections (e.g. go.mod's checks) found in a file, in order.");
        let example = "Expands an abbreviation with the file's plug-in, e.g. `div.a>b` in HTML, i.e. Emmet. Then more.";
        assert_eq!(
            one_line(example),
            "Expands an abbreviation with the file's plug-in, e.g. `div.a>b` in HTML, i.e. Emmet."
        );
        let long = format!("Lists every command.{}", " More words.".repeat(60));
        assert_eq!(one_line(&long), "Lists every command.", "the first sentence, however long the rest");
        let long_first = format!("Lists{} things.", " many".repeat(60));
        assert_eq!(one_line(&long_first), long_first, "a long first sentence is not cut");
        assert_eq!(one_line("Ends without a stop"), "Ends without a stop");

        let registry = registry();
        for info in registry.commands() {
            let line = one_line(&info.description);
            assert!(info.description.trim().starts_with(&line), "{}: a prefix of the description", info.id.as_str());
            assert!(!line.ends_with("e.g.") && !line.ends_with("i.e."), "{}: {line}", info.id.as_str());
            let opened = line.matches('(').count();
            let closed = line.matches(')').count();
            assert!(opened <= closed, "{}: cut inside brackets: {line}", info.id.as_str());
        }
    }

    #[test]
    fn the_instructions_name_the_groups_and_the_main_commands() {
        let registry = registry();
        let text = instructions("Test", &registry);
        for id in [
            "editor.text.replace",
            "search.text.find",
            "navigate.usages.find",
            "problems.file.list",
            "refactor.rename.apply",
        ] {
            assert!(text.contains(id), "{id}");
        }
        assert!(text.contains("editor ("), "the groups with their counts");
        assert!(text.contains("editor_text_replace, editor_text_write, editor_text_batch"), "the tools: {text}");
        assert!(text.contains("Edit files through the IDE rather than with scripts"), "{text}");
        assert!(text.contains("`line` and `column`"), "a place by line and column: {text}");
        let main = MAIN_COMMANDS.iter().filter(|(id, _)| registry.info(id).is_some()).count();
        assert!((15..=24).contains(&main), "{main} main commands");
    }

    #[test]
    fn a_range_of_lines_is_read_with_or_without_numbers() {
        let text: String =
            (1..=800).map(|line| format!("line {line} of the file, a typical line of Go code here\n")).collect();
        let part =
            document_text(&text, false, &json!({ "startLine": 300, "endLine": 339, "withLineNumbers": true })).unwrap();
        assert_eq!(
            (part["startLine"].as_u64(), part["endLine"].as_u64(), part["totalLines"].as_u64()),
            (Some(300), Some(339), Some(800))
        );
        let body = part["text"].as_str().unwrap();
        assert_eq!(body.lines().count(), 40);
        assert!(
            body.starts_with("300|line 300 of")
                && body.ends_with("339|line 339 of the file, a typical line of Go code here\n")
        );
        assert!(part.to_string().len() < 3 * 1024, "40 lines in under 3 KB: {}", part.to_string().len());
        let same = document_text(&text, false, &json!({ "offset": 300, "limit": 40 })).unwrap();
        assert_eq!(
            same["text"].as_str().unwrap().lines().next(),
            Some("line 300 of the file, a typical line of Go code here")
        );
        assert_eq!(same["endLine"], 339);
        let tail = document_text(&text, true, &json!({ "startLine": 799 })).unwrap();
        assert_eq!((tail["endLine"].as_u64(), tail["unsaved"].as_bool()), (Some(800), Some(true)));
        let large = document_text(&text, false, &json!({})).unwrap_err();
        let large: Value = serde_json::from_str(&large).unwrap();
        assert_eq!((large["error"].as_str(), large["lines"].as_u64()), (Some("too_large"), Some(800)));
        let small = document_text("a\nb\n", false, &json!({})).unwrap();
        assert_eq!(small, json!({ "text": "a\nb\n", "unsaved": false }), "a small file comes whole, as before");
        assert!(document_text(&text, false, &json!({ "startLine": 900 })).is_err());
        assert!(document_text(&text, false, &json!({ "startLine": 10, "endLine": 5 })).is_err());
        assert!(document_text(&text, false, &json!({ "startLine": 1, "limit": 5 })).is_err());
    }

    #[test]
    fn a_coded_command_error_reaches_agents_as_json() {
        let error = zen_core::DispatchError::Failed {
            id: zen_core::CommandId::new("editor.text.replace").unwrap(),
            error: zen_core::CommandError::with_code("not_unique", "three times").with_data(json!({ "count": 3 })),
        };
        let answer: Value = serde_json::from_str(&dispatch_error(&error)).unwrap();
        assert_eq!(answer, json!({ "error": "not_unique", "count": 3, "message": "three times" }));
        let bare = zen_core::DispatchError::Failed {
            id: zen_core::CommandId::new("vcs.commit.create").unwrap(),
            error: zen_core::CommandError::with_code("notTrusted", "trust it first"),
        };
        let answer: Value = serde_json::from_str(&dispatch_error(&bare)).unwrap();
        assert_eq!(answer, json!({ "error": "notTrusted", "message": "trust it first" }));
        let plain = zen_core::DispatchError::UnknownCommand { id: "x.y".to_owned() };
        assert_eq!(dispatch_error(&plain), plain.to_string());
    }

    fn custom() -> IdeSettings {
        let mut settings = IdeSettings { mcp_port: 7000, ligatures: true, ..IdeSettings::default() };
        settings.code_style.insert("go".to_owned(), CodeStyle { use_tabs: true, tab_size: 4, indent: 4 });
        settings.code_style.insert("json".to_owned(), CodeStyle { use_tabs: false, tab_size: 2, indent: 2 });
        settings.editor_font = EditorFont { family: Some("Fira Code".to_owned()), size: 14.0, line_spacing: 1.5 };
        settings
    }

    fn keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> =
            value.as_object().map(|object| object.keys().cloned().collect()).unwrap_or_default();
        keys.sort();
        keys
    }

    #[test]
    fn code_style_options_merge_by_option_and_null_removes() {
        let current = IdeSettings::default();
        let changes = json!({ "codeStyleOptions": { "go": { "gofumpt": true, "local": "example.com" } } });
        let merged = merge_settings(&current, &changes).unwrap();
        let merged = merge_settings(&merged, &json!({ "codeStyleOptions": { "go": { "local": null } } })).unwrap();
        assert_eq!(
            merged.code_style_options["go"],
            std::collections::BTreeMap::from([("gofumpt".to_owned(), json!(true))])
        );
        let merged = merge_settings(&merged, &json!({ "codeStyleOptions": { "go": { "gofumpt": null } } })).unwrap();
        assert!(merged.code_style_options.is_empty(), "a language without options goes");
        assert!(merge_settings(&current, &json!({ "codeStyleOptions": { "go": 3 } })).is_err());
    }

    #[test]
    fn nested_objects_merge_key_by_key() {
        let current = custom();
        let merged = merge_settings(&current, &json!({ "detectIndents": false })).unwrap();
        assert!(!merged.detect_indents && current.detect_indents);
        let merged = merge_settings(&current, &json!({ "codeStyle": { "go": { "tabSize": 8 } } })).unwrap();
        assert_eq!(merged.code_style["go"], CodeStyle { use_tabs: true, tab_size: 8, indent: 4 });
        assert_eq!(merged.code_style["json"], current.code_style["json"], "the other languages stay");
        assert_eq!(merged, IdeSettings { code_style: merged.code_style.clone(), ..current.clone() });
        let merged = merge_settings(&current, &json!({ "editorFont": { "size": 16 }, "uiFontSize": 15 })).unwrap();
        assert_eq!(
            merged.editor_font,
            EditorFont { family: Some("Fira Code".to_owned()), size: 16.0, line_spacing: 1.5 }
        );
        assert_eq!(merged.ui_font_size, Some(15.0));
        assert_eq!(merged.code_style, current.code_style);
        assert!(merged.ligatures && merged.mcp_port == 7000);
    }

    #[test]
    fn customization_is_set_as_a_whole_group_and_other_changes_keep_it() {
        let current = IdeSettings::default();
        let changes =
            json!({ "customization": { "groups": { "mainToolbar": ["editor.code.reformat", "gone.action"] } } });
        let merged = merge_settings(&current, &changes).unwrap();
        assert_eq!(merged.customization.items("mainToolbar"), ["editor.code.reformat", "gone.action"]);
        let again = merge_settings(&merged, &json!({ "detectIndents": false })).unwrap();
        assert_eq!(again.customization, merged.customization);
        let restored =
            merge_settings(&again, &json!({ "customization": { "groups": { "mainToolbar": null } } })).unwrap();
        assert_eq!(restored.customization, current.customization);
    }

    #[test]
    fn a_new_language_starts_from_its_code_style_in_use() {
        let current = custom();
        let merged = merge_settings(&current, &json!({ "codeStyle": { "yaml": { "indent": 4 } } })).unwrap();
        assert_eq!(merged.code_style["yaml"], CodeStyle { use_tabs: false, tab_size: 2, indent: 4 });
        let merged = merge_settings(&current, &json!({ "codeStyle": { "rust": { "useTabs": true } } })).unwrap();
        assert_eq!(merged.code_style["rust"], CodeStyle { use_tabs: true, ..CodeStyle::default() });
    }

    #[test]
    fn a_null_brings_back_the_default() {
        let current = IdeSettings { ui_font_size: Some(18.0), language: Some("ru".to_owned()), ..custom() };
        let merged = merge_settings(
            &current,
            &json!({ "uiFontSize": null, "language": null, "ligatures": null, "editorFont": { "family": null, "size": null } }),
        )
        .unwrap();
        assert_eq!((merged.ui_font_size, merged.language.as_deref(), merged.ligatures), (None, None, false));
        assert_eq!(merged.editor_font, EditorFont { line_spacing: 1.5, ..EditorFont::default() });
        let merged = merge_settings(&current, &json!({ "editorFont": null })).unwrap();
        assert_eq!(merged.editor_font, EditorFont::default());
        let merged =
            merge_settings(&current, &json!({ "codeStyle": { "go": null, "json": { "tabSize": null } } })).unwrap();
        assert!(!merged.code_style.contains_key("go"), "a language without an entry takes the built-in style");
        assert_eq!(merged.code_style["json"].tab_size, 2);
        let merged = merge_settings(&current, &json!({ "codeStyle": null })).unwrap();
        assert!(merged.code_style.is_empty());
        assert_eq!(merged.mcp_port, current.mcp_port);
    }

    #[test]
    fn the_proxy_and_automatic_acceptance_are_the_person_s_and_never_take_a_password() {
        let current = IdeSettings::default();
        let refused = t!("mcp-settings-network-refused");
        for changes in [
            json!({ "proxy": { "mode": "manual", "kind": "socks", "host": "p.corp", "port": 1080, "noProxyFor": "*.a" } }),
            json!({ "proxy": { "host": "p.corp" } }),
            json!({ "proxy": { "mode": "noProxy" } }),
            json!({ "acceptUntrustedCertificates": true }),
        ] {
            assert_eq!(merge_settings(&current, &changes).unwrap_err(), refused, "{changes}");
        }
        assert!(merge_settings(&current, &json!({ "proxy": { "password": "x" } })).is_err());
        let automatic = IdeSettings { accept_untrusted_certificates: true, ..current.clone() };
        let off = merge_settings(&automatic, &json!({ "acceptUntrustedCertificates": false })).unwrap();
        assert!(!off.accept_untrusted_certificates, "turning it off is safe");
        assert!(merge_settings(&current, &json!({ "proxy": {} })).is_ok(), "nothing changes");
    }

    #[test]
    fn unknown_keys_and_wrong_values_are_errors() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let current = custom();
        let error = |changes: Value| merge_settings(&current, &changes).unwrap_err();
        assert_eq!(error(json!({ "uiFontSise": 14 })), t!("mcp-settings-unknown", field = "uiFontSise"));
        assert_eq!(
            error(json!({ "editorFont": { "weight": 700 } })),
            t!("mcp-settings-unknown", field = "editorFont.weight")
        );
        assert_eq!(
            error(json!({ "codeStyle": { "go": { "tabWidth": 8 } } })),
            t!("mcp-settings-unknown", field = "codeStyle.go.tabWidth")
        );
        assert_eq!(error(json!([1, 2])), t!("mcp-settings-not-object"));
        assert!(error(json!({ "generatorSettings": { "a.b": 1 } })).contains("generatorSettings.a.b"));
        let key = "spring-boot.initializr.serviceUrl";
        let refused = t!("mcp-settings-generators-refused");
        assert_eq!(error(json!({ "generatorSettings": { key: "https://own" } })), refused);
        let mut off = IdeSettings { generators_network: false, ..current.clone() };
        off.generator_settings.insert(key.to_owned(), "https://own".to_owned());
        let agents = t!("mcp-settings-agents-refused");
        let adapter = json!({ "id": "evil", "name": "Evil", "programs": ["sh"], "resume": ["sh", "-c", "{session}"] });
        assert_eq!(error(json!({ "terminalAgents": [adapter] })), agents, "an agent adds no adapter");
        assert!(error(json!({ "terminal_agents": [adapter] })).contains("terminal_agents"), "no other spelling");
        assert!(error(json!({ "terminalagents": [adapter] })).contains("terminalagents"));
        let mut with = current.clone();
        with.terminal_agents = vec![serde_json::from_value(adapter.clone()).unwrap()];
        assert_eq!(merge_settings(&with, &json!({ "terminalAgents": null })).unwrap_err(), agents, "nor removes one");
        assert_eq!(merge_settings(&with, &json!({ "terminalAgents": [] })).unwrap_err(), agents);
        let changed = json!({ "terminalAgents": [{ "id": "evil", "name": "Evil", "programs": ["sh"], "resume": ["sh", "{session}"] }] });
        assert_eq!(merge_settings(&with, &changed).unwrap_err(), agents, "nor changes one");
        assert!(merge_settings(&with, &json!({ "terminalAgents": [adapter] })).is_ok(), "the same list is no change");
        assert_eq!(validate(&current, &with).unwrap_err(), agents, "a request carrying a changed list is refused too");
        let on = merge_settings(&off, &json!({ "generatorsNetwork": true }));
        assert_eq!(on.unwrap_err(), refused, "an agent cannot turn it on");
        let same = merge_settings(&off, &json!({ "generatorSettings": { key: null } }));
        assert_eq!(same.unwrap_err(), refused);
        assert!(!merge_settings(&current, &json!({ "generatorsNetwork": false })).unwrap().generators_network);
        assert!(error(json!({ "ligatures": "yes" })).starts_with(&t!("mcp-settings-invalid", error = "")));
        assert!(
            error(json!({ "codeStyle": { "go": { "tabSize": 300 } } }))
                .starts_with(&t!("mcp-settings-invalid", error = ""))
        );
        assert_eq!(
            error(json!({ "uiFontSize": 40 })),
            t!("mcp-settings-range", field = "uiFontSize", min = 8, max = 32)
        );
        assert_eq!(
            error(json!({ "uiFontSize": 7.5 })),
            t!("mcp-settings-range", field = "uiFontSize", min = 8, max = 32)
        );
        assert_eq!(error(json!({ "ideZoom": 400 })), t!("mcp-settings-range", field = "ideZoom", min = 50, max = 300));
        assert_eq!(merge_settings(&current, &json!({ "ideZoom": 125 })).unwrap().ide_zoom, 125);
        assert_eq!(
            error(json!({ "presentationFontSize": 120 })),
            t!("mcp-settings-range", field = "presentationFontSize", min = 8, max = 96)
        );
        assert_eq!(
            merge_settings(&current, &json!({ "presentationFontSize": 32 })).unwrap().presentation_font_size,
            32.0
        );
        assert_eq!(
            merge_settings(&current, &json!({ "presentationFontSize": null })).unwrap().presentation_font_size,
            24.0,
            "null brings back the default"
        );
        assert_eq!(
            error(json!({ "mcpPort": 80 })),
            t!("mcp-settings-range", field = "mcpPort", min = 1024, max = 65535)
        );
        assert_eq!(
            error(json!({ "editorFont": { "size": 0 } })),
            t!("mcp-settings-positive", field = "editorFont.size")
        );
        assert_eq!(
            error(json!({ "codeStyle": { "go": { "indent": 0 } } })),
            t!("mcp-settings-range", field = "codeStyle.go.indent", min = 1, max = 255)
        );
        assert_eq!(
            error(json!({ "clipboardLimit": 0 })),
            t!("mcp-settings-range", field = "clipboardLimit", min = 1, max = 100)
        );
        assert_eq!(
            error(json!({ "autosaveIdleSeconds": 0 })),
            t!("mcp-settings-range", field = "autosaveIdleSeconds", min = 1, max = 3600)
        );
        let autosave = merge_settings(
            &current,
            &json!({ "autosaveOnIdle": false, "autosaveIdleSeconds": 30, "autosaveOnFrameDeactivation": false }),
        )
        .unwrap();
        assert_eq!(
            (autosave.autosave_on_idle, autosave.autosave_idle_seconds, autosave.autosave_on_frame_deactivation),
            (false, 30, false)
        );
        assert_eq!(merge_settings(&current, &json!({})).unwrap(), current, "no change is no change");
    }

    #[test]
    fn the_schema_lists_every_setting() {
        let schema = settings_schema();
        let defaults = serde_json::to_value(IdeSettings::default()).unwrap();
        assert_eq!(keys(&schema["properties"]), keys(&defaults));
        for field in ["uiFontSize", "editorFont", "codeStyle"] {
            assert!(schema["properties"][field].is_object(), "{field}");
        }
        assert_eq!(keys(&schema["properties"]["editorFont"]["properties"]), keys(&defaults["editorFont"]));
        assert_eq!(keys(&schema["properties"]["editorFont"]["properties"]), ["family", "lineSpacing", "size"]);
        let style = serde_json::to_value(CodeStyle::default()).unwrap();
        assert_eq!(keys(&schema["properties"]["codeStyle"]["additionalProperties"]["properties"]), keys(&style));
        assert_eq!(keys(&style), ["indent", "tabSize", "useTabs"]);
        assert_eq!(schema["properties"]["uiFontSize"]["minimum"], 8.0);
        assert_eq!(schema["properties"]["uiFontSize"]["maximum"], 32.0);
        assert_eq!(schema["additionalProperties"], false);
        let languages = schema["properties"]["language"]["enum"].as_array().unwrap();
        assert_eq!(languages.len(), zen_i18n::LANGUAGES.len() + 2, "the languages, system and null");
        assert!(languages.contains(&json!("ru")) && languages.contains(&Value::Null));
        let tool = tools().into_iter().find(|tool| tool.name == "set_settings").unwrap();
        assert!(tool.input_schema.to_string().len() < 300, "short in tools/list: {}", tool.input_schema);
        let described = describe_command(&registry(), &json!({ "id": "set_settings" })).unwrap();
        assert_eq!(described["params"], schema, "the whole schema through describe_command");
    }
}

#[cfg(test)]
mod lean_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_tool_schema_leaves_out_what_tells_nothing() {
        let schema = json!({
            "$defs": { "Scope": { "oneOf": [{ "const": "a", "type": "string" }] } },
            "properties": {
                "line": { "default": null, "description": "The line.", "format": "uint32", "minimum": 0, "type": ["integer", "null"] },
                "scope": { "anyOf": [{ "$ref": "#/$defs/Scope" }, { "type": "null" }], "default": null, "description": "Where." },
                "count": { "minimum": 1, "type": "integer" },
                "both": { "type": ["string", "integer", "null"] },
            },
            "required": ["count"],
        });
        let lean = lean_schema(schema);
        assert_eq!(lean["properties"]["line"], json!({ "description": "The line.", "type": "integer" }));
        assert_eq!(lean["properties"]["scope"], json!({ "$ref": "#/$defs/Scope", "description": "Where." }));
        assert_eq!(lean["properties"]["count"], json!({ "minimum": 1, "type": "integer" }), "a real minimum stays");
        assert_eq!(lean["properties"]["both"], json!({ "type": ["string", "integer"] }));
        assert_eq!(lean["required"], json!(["count"]));
    }

    #[test]
    fn tools_list_stays_small() {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut tools = tools();
        tools.extend(command_tools(&registry));
        let listed: Vec<Value> = tools
            .iter()
            .map(|tool| json!({ "name": tool.name, "description": tool.description, "inputSchema": tool.input_schema }))
            .collect();
        let size = Value::Array(listed).to_string().len();
        assert!(size < 44 * 1024, "tools/list is {size} bytes");
    }
}

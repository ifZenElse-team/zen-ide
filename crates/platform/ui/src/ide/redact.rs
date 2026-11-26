use std::path::{Path, PathBuf};

use serde_json::Value;

pub(crate) const DEFAULT_PATTERNS: &[&str] = &[
    ".env",
    ".env.*",
    "!.env.example",
    "!.env.sample",
    "!.env.template",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.jks",
    "*.keystore",
    "id_rsa*",
    "id_dsa*",
    "id_ecdsa*",
    "id_ed25519*",
    ".netrc",
    ".pgpass",
    "*credentials*",
    "*.secret",
    "*.secrets",
    "http-client.private.env.json",
];

pub(crate) const REDACTED: &str = "[redacted]";

pub(crate) const REDACTED_CODE: &str = "redacted";

const ACTIVE_DOCUMENT_GROUPS: &[&str] = &[
    "editor.",
    "navigate.",
    "refactor.",
    "problems.file.",
    "problems.fix",
    "spelling.",
    "markdown.",
    "localHistory.",
    "file.document.",
    "file.compare.",
    "file.lineSeparator.",
    "file.encoding.",
    "file.bom.",
    "vcs.blame.",
    "vcs.file.",
    "vcs.history.",
    "vcs.diff.",
    "vcs.lineStatus.",
    "http.request.",
    "run.context.",
    "run.targets.",
    "search.text.occurrences",
];

pub(crate) fn acts_on_active_document(tool: &str, arguments: &Value, requires_file: bool) -> bool {
    match tool {
        "run_action" | "press_keys" => true,
        "dispatch_command" => {
            let command = arguments.get("command").and_then(Value::as_str).unwrap_or_default();
            !requires_file && ACTIVE_DOCUMENT_GROUPS.iter().any(|group| command.starts_with(group))
        }
        _ => false,
    }
}

pub(crate) fn requires_file(schema: &Value) -> bool {
    schema["required"].as_array().is_some_and(|required| {
        required.iter().any(|name| ["path", "paths", "files"].contains(&name.as_str().unwrap_or_default()))
    })
}

const PATH_KEYS: &[&str] = &["path", "paths", "file", "files", "directory", "sources", "folder"];

const TEXT_KEYS: &[&str] = &["text", "preview", "lineText", "line_text", "snippet", "context"];

const CONTENT_KEYS: &[&str] = &[
    "text",
    "texts",
    "preview",
    "lineText",
    "line_text",
    "snippet",
    "context",
    "message",
    "content",
    "lines",
    "before",
    "after",
    "left",
    "right",
    "middle",
    "base",
    "hunks",
    "diff",
    "patch",
    "original",
    "modified",
];

const WORD_KEYS: &[&str] =
    &["text", "oldText", "newText", "replacement", "message", "notes", "description", "title", "label", "query"];

pub(crate) const AGENT_FOLDERS: [&str; 4] = ["scratches", "buffers", "consoles", "language-servers"];

#[derive(Clone, Debug, Default)]
pub(crate) struct Redactor {
    patterns: Vec<String>,
    roots: Vec<PathBuf>,
    stores: Vec<PathBuf>,
    allowed: Vec<PathBuf>,
    closed: Vec<PathBuf>,
}

impl Redactor {
    pub(crate) fn new(patterns: &[String], roots: Vec<PathBuf>) -> Self {
        let patterns = patterns.iter().map(|pattern| pattern.trim().to_owned()).filter(|p| !p.is_empty()).collect();
        Self { patterns, roots, stores: Vec::new(), allowed: Vec::new(), closed: Vec::new() }
    }

    pub(crate) fn with_closed(mut self, paths: Vec<PathBuf>) -> Self {
        self.closed.extend(
            paths
                .iter()
                .filter(|path| !path.to_string_lossy().contains('*'))
                .map(|path| zen_workbench::hidden_files::real_path(path)),
        );
        self
    }

    fn in_closed(&self, path: &str) -> bool {
        if self.closed.is_empty() || path.is_empty() {
            return false;
        }
        let given = Path::new(path);
        let candidates: Vec<PathBuf> = if given.is_absolute() {
            vec![given.to_path_buf()]
        } else {
            self.roots.iter().map(|root| root.join(given)).collect()
        };
        candidates.iter().any(|candidate| {
            let real = zen_workbench::hidden_files::real_path(candidate);
            self.closed.iter().any(|closed| real.starts_with(closed))
        })
    }

    pub(crate) fn with_data_folder(self, data: PathBuf) -> Self {
        let allowed = AGENT_FOLDERS.iter().map(|folder| data.join(folder)).collect();
        self.with_store(data, allowed)
    }

    pub(crate) fn with_store(mut self, data: PathBuf, allowed: Vec<PathBuf>) -> Self {
        let data = zen_workbench::hidden_files::real_path(&data);
        if self.roots.iter().any(|root| zen_workbench::hidden_files::real_path(root).starts_with(&data)) {
            return self;
        }
        self.stores.push(data);
        self.allowed.extend(allowed.iter().map(|folder| zen_workbench::hidden_files::real_path(folder)));
        self
    }

    fn in_store(&self, path: &str) -> bool {
        if self.stores.is_empty() || path.is_empty() {
            return false;
        }
        let given = Path::new(path);
        let candidates: Vec<PathBuf> = if given.is_absolute() {
            vec![given.to_path_buf()]
        } else {
            self.roots.iter().map(|root| root.join(given)).collect()
        };
        candidates.iter().any(|candidate| {
            let real = zen_workbench::hidden_files::real_path(candidate);
            let in_project =
                self.roots.iter().any(|root| real.starts_with(zen_workbench::hidden_files::real_path(root)));
            !in_project
                && self.stores.iter().any(|store| real.starts_with(store))
                && !self.allowed.iter().any(|folder| real.starts_with(folder))
        })
    }

    fn hides_any(&self) -> bool {
        !self.patterns.is_empty() || !self.stores.is_empty() || !self.closed.is_empty()
    }

    pub(crate) fn hides(&self, path: &str) -> bool {
        if self.in_store(path) || self.in_closed(path) {
            return true;
        }
        if self.patterns.is_empty() || path.is_empty() {
            return false;
        }
        let given = Path::new(path);
        let in_project =
            self.roots.iter().any(|root| zen_workbench::hidden_files::is_hidden(&self.patterns, Some(root), given));
        in_project
            || zen_workbench::hidden_files::is_hidden(&self.patterns, None, given)
            || zen_workbench::hidden_files::hidden_by(&self.patterns, &self.relative(path))
    }

    fn is_hidden_content(&self, item: &Value) -> bool {
        match item {
            Value::Object(map) => {
                map.get("path").and_then(Value::as_str).is_some_and(|path| self.hides(path))
                    && map.iter().any(|(key, value)| {
                        (CONTENT_KEYS.contains(&key.as_str()) && value.is_string()) || value.is_array()
                    })
            }
            Value::String(text) => row_place(text).is_some_and(|(path, _)| self.hides(path)),
            _ => false,
        }
    }

    fn relative(&self, path: &str) -> String {
        let given = Path::new(path);
        if given.is_absolute() {
            for root in &self.roots {
                if let Ok(inside) = given.strip_prefix(root) {
                    return inside.to_string_lossy().replace('\\', "/");
                }
            }
        }
        path.replace('\\', "/")
    }

    pub(crate) fn named_in(&self, tool: &str, arguments: &Value) -> Option<String> {
        if !self.hides_any() {
            return None;
        }
        let typed =
            |arguments: &Value| arguments.get("text").and_then(Value::as_str).and_then(|text| self.word_in(text));
        match tool {
            "terminal_write" => return typed(arguments),
            "dispatch_command" if arguments.get("command").and_then(Value::as_str) == Some("terminal.session.send") => {
                return arguments.get("params").and_then(typed);
            }
            _ => {}
        }
        let mut found = None;
        visit_paths(arguments, false, &mut |path| {
            if found.is_none() && self.hides(path) {
                found = Some(path.to_owned());
            }
        });
        if found.is_none() {
            visit_strings(arguments, false, &mut |text| {
                if found.is_none() && self.names_existing(text) && self.hides(text) {
                    found = Some(text.to_owned());
                }
            });
        }
        found
    }

    fn names_existing(&self, text: &str) -> bool {
        if text.is_empty() || text.len() > 4096 || text.contains('\n') {
            return false;
        }
        let given = Path::new(text);
        if given.is_absolute() {
            return given.exists();
        }
        self.roots.iter().any(|root| root.join(given).exists())
    }

    fn word_in(&self, text: &str) -> Option<String> {
        text.split(|c: char| c.is_whitespace() || "'\"`;|&<>()=".contains(c))
            .filter(|word| !word.is_empty() && !word.starts_with('-'))
            .flat_map(|word| {
                let file = word.split_once(':').map(|(_, file)| file).filter(|file| !file.is_empty());
                std::iter::once(word).chain(file)
            })
            .map(|word| word.trim_start_matches("./"))
            .find(|word| self.hides(word))
            .map(str::to_owned)
    }

    pub(crate) fn redact(&self, answer: &mut Value) -> Vec<String> {
        let mut hidden = Vec::new();
        if self.hides_any() {
            self.redact_value(answer, None, &mut hidden);
        }
        hidden
    }

    fn redact_value(&self, value: &mut Value, file: Option<&str>, hidden: &mut Vec<String>) {
        let note = |path: &str, hidden: &mut Vec<String>| {
            if !hidden.iter().any(|known| known == path) {
                hidden.push(path.to_owned());
            }
        };
        match value {
            Value::Object(map) => {
                let own = map.get("path").and_then(Value::as_str).map(str::to_owned);
                let file = own.as_deref().or(file);
                if let Some(path) = file.filter(|path| self.hides(path)) {
                    let path = path.to_owned();
                    let mut changed = false;
                    for key in TEXT_KEYS {
                        if let Some(text) = map.get_mut(*key).filter(|text| text.is_string()) {
                            *text = Value::from(REDACTED);
                            changed = true;
                        }
                    }
                    if own.is_some() && map.contains_key("unsaved") {
                        map.insert("sensitive".to_owned(), Value::Bool(true));
                    }
                    if changed {
                        note(&path, hidden);
                    }
                }
                for (key, item) in map.iter_mut() {
                    if !TEXT_KEYS.contains(&key.as_str()) {
                        self.redact_value(item, file, hidden);
                    } else if let Value::String(text) = item
                        && let Some((kept, gone)) = self.without_hidden_diffs(text)
                    {
                        *text = kept;
                        for path in gone {
                            note(&path, hidden);
                        }
                    }
                }
            }
            Value::Array(items) => {
                let mut gone = Vec::new();
                items.retain(|item| {
                    let content = self.is_hidden_content(item);
                    if content {
                        let path = match item {
                            Value::Object(map) => map.get("path").and_then(Value::as_str).map(str::to_owned),
                            Value::String(text) => row_place(text).map(|(path, _)| path.to_owned()),
                            _ => None,
                        };
                        gone.extend(path);
                    }
                    !content
                });
                for path in gone {
                    note(&path, hidden);
                }
                items.iter_mut().for_each(|item| self.redact_value(item, file, hidden));
            }
            Value::String(text) => {
                if let Some((path, place)) = row_place(text)
                    && self.hides(path)
                {
                    let path = path.to_owned();
                    *text = format!("{path}:{place} {REDACTED}");
                    note(&path, hidden);
                } else if let Some((kept, gone)) = self.without_hidden_diffs(text) {
                    *text = kept;
                    for path in gone {
                        note(&path, hidden);
                    }
                }
            }
            _ => {}
        }
    }
}

impl Redactor {
    fn without_hidden_diffs(&self, text: &str) -> Option<(String, Vec<String>)> {
        if !text.starts_with("diff --git ") && !text.contains("\ndiff --git ") {
            return None;
        }
        let mut kept = String::with_capacity(text.len());
        let mut gone = Vec::new();
        let mut skipping = false;
        for line in text.split_inclusive('\n') {
            if let Some(header) = line.strip_prefix("diff --git ") {
                let hidden = diff_paths(header.trim_end()).into_iter().find(|path| self.hides(path));
                skipping = hidden.is_some();
                gone.extend(hidden);
            }
            if !skipping {
                kept.push_str(line);
            }
        }
        (!gone.is_empty()).then_some((kept, gone))
    }
}

fn diff_paths(header: &str) -> Vec<String> {
    let (old, new) = match header.split_once(" b/") {
        Some((old, new)) => (old.trim_start_matches("a/"), new),
        None => header.split_once(' ').unwrap_or((header, header)),
    };
    vec![old.trim_matches('"').to_owned(), new.trim_matches('"').to_owned()]
}

fn row_place(row: &str) -> Option<(&str, &str)> {
    let (head, text) = row.split_once(' ')?;
    if text.is_empty() {
        return None;
    }
    let mut parts = head.rsplitn(3, ':');
    let (column, line, path) = (parts.next()?, parts.next()?, parts.next()?);
    let numbers = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    (numbers(column) && numbers(line) && !path.is_empty()).then(|| (path, &head[path.len() + 1..]))
}

fn visit_strings(value: &Value, under_word_key: bool, found: &mut dyn FnMut(&str)) {
    match value {
        Value::String(text) if !under_word_key => found(text),
        Value::Array(items) => items.iter().for_each(|item| visit_strings(item, under_word_key, found)),
        Value::Object(map) => {
            for (key, item) in map {
                visit_strings(item, WORD_KEYS.contains(&key.as_str()), found);
            }
        }
        _ => {}
    }
}

fn visit_paths(value: &Value, under_path_key: bool, found: &mut dyn FnMut(&str)) {
    match value {
        Value::String(text) if under_path_key => found(text),
        Value::Array(items) => items.iter().for_each(|item| visit_paths(item, under_path_key, found)),
        Value::Object(map) => {
            for (key, item) in map {
                visit_paths(item, PATH_KEYS.contains(&key.as_str()), found);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn redactor() -> Redactor {
        let patterns: Vec<String> = DEFAULT_PATTERNS.iter().map(|pattern| (*pattern).to_owned()).collect();
        Redactor::new(&patterns, vec![PathBuf::from("/work/shop")])
    }

    #[test]
    fn secrets_are_hidden_and_their_samples_are_not() {
        let redactor = redactor();
        for hidden in
            [".env", ".env.local", "deploy/server.pem", "/work/shop/config/nats-credentials.txt", "id_ed25519"]
        {
            assert!(redactor.hides(hidden), "{hidden}");
        }
        for readable in [".env.example", "src/main.rs", "docs/keys.md", "environment.go", "/elsewhere/README.md"] {
            assert!(!redactor.hides(readable), "{readable}");
        }
        assert!(!Redactor::new(&[], Vec::new()).hides(".env"), "nothing hidden when the list is empty");
    }

    #[test]
    fn a_patchs_sections_of_hidden_files_are_cut_out() {
        let patch = "From 1 Mon\n\n---\n\ndiff --git a/.env b/.env\n--- a/.env\n+++ b/.env\n@@ -1 +1 @@\n-A=1\n+A=secret\n\
                     diff --git a/src/main.rs b/src/main.rs\n+fn main() {}\ndiff --git config/.env.local config/.env.local\n+B=2\n";
        let mut answer = json!({ "files": [".env", "src/main.rs"], "text": patch });
        let gone = redactor().redact(&mut answer);
        let text = answer["text"].as_str().unwrap();
        assert!(!text.contains("secret") && !text.contains("B=2"), "{text}");
        assert!(text.starts_with("From 1 Mon") && text.contains("+fn main() {}"), "{text}");
        assert!(gone.contains(&".env".to_owned()) && gone.contains(&"config/.env.local".to_owned()), "{gone:?}");
        let mut plain = json!({ "text": "no diff here" });
        assert!(redactor().redact(&mut plain).is_empty());
    }

    #[test]
    fn the_ides_own_folder_is_hidden_but_its_scratches() {
        let data = tempfile::tempdir().unwrap();
        let ide = data.path().canonicalize().unwrap().join("ide");
        std::fs::create_dir_all(ide.join("scratches")).unwrap();
        std::fs::create_dir_all(ide.join("vcs-shelf")).unwrap();
        let project = data.path().canonicalize().unwrap().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let redactor = Redactor::new(&[], vec![project.clone()]).with_store(ide.clone(), vec![ide.join("scratches")]);
        let path = |path: PathBuf| path.to_string_lossy().into_owned();
        assert!(
            redactor.hides(&path(ide.join("vcs-shelf/1.patch"))) && redactor.hides(&path(ide.join("settings.json")))
        );
        assert!(!redactor.hides(&path(ide.join("scratches/a.txt"))), "scratches are the agent's");
        assert!(!redactor.hides("main.rs") && !redactor.hides(&path(project.join("main.rs"))));
        let inside = Redactor::new(&[], vec![ide.join("work")]).with_store(ide.clone(), Vec::new());
        assert!(!inside.hides(&path(ide.join("work/main.rs"))), "a project inside the folder is still a project");
        assert!(
            redactor.named_in("get_document_text", &json!({ "path": path(ide.join("vcs-shelf/1.patch")) })).is_some()
        );
    }

    #[test]
    fn a_database_console_in_the_ides_folder_is_open_to_agents() {
        let data = tempfile::tempdir().unwrap();
        let ide = data.path().canonicalize().unwrap().join("ide");
        let console = ide.join("consoles/project-1a2b3c4d/console.sql");
        std::fs::create_dir_all(console.parent().unwrap()).unwrap();
        std::fs::write(&console, "SELECT 1;").unwrap();
        let project = data.path().canonicalize().unwrap().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let redactor = Redactor::new(&[], vec![project]).with_data_folder(ide.clone());
        let mut answer = json!({ "editor": { "path": console.to_string_lossy(), "text": "SELECT 1;" } });
        assert!(redactor.redact(&mut answer).is_empty(), "{answer}");
        assert_eq!(answer["editor"]["text"], "SELECT 1;");
        assert!(redactor.named_in("get_document_text", &json!({ "path": console.to_string_lossy() })).is_none());
        assert!(redactor.hides(&ide.join("settings.json").to_string_lossy()), "the rest of the folder stays hidden");
    }

    #[test]
    fn a_console_of_a_connection_closed_to_agents_stays_hidden() {
        let data = tempfile::tempdir().unwrap();
        let ide = data.path().canonicalize().unwrap().join("ide");
        let open = ide.join("consoles/project-1a2b3c4d/shop-dev/console.sql");
        let closed = ide.join("consoles/project-1a2b3c4d/payroll/console.sql");
        for console in [&open, &closed] {
            std::fs::create_dir_all(console.parent().unwrap()).unwrap();
            std::fs::write(console, "SELECT salary FROM staff;").unwrap();
        }
        let project = data.path().canonicalize().unwrap().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let redactor = Redactor::new(&[], vec![project])
            .with_closed(vec![ide.join("consoles/project-1a2b3c4d/payroll"), ide.join("databases/recovery-*")])
            .with_data_folder(ide.clone());
        assert!(!redactor.hides(&open.to_string_lossy()), "a console of an open connection is the agent's");
        assert!(redactor.hides(&closed.to_string_lossy()), "a closed connection's is not");
        assert!(redactor.named_in("get_document_text", &json!({ "path": closed.to_string_lossy() })).is_some());
        let mut answer = json!({ "editor": { "path": closed.to_string_lossy(), "text": "SELECT salary FROM staff;" } });
        assert!(!redactor.redact(&mut answer).is_empty(), "{answer}");
        assert!(!answer.to_string().contains("salary"), "{answer}");
    }

    #[test]
    fn a_language_server_the_ide_installed_stays_in_the_list() {
        let data = tempfile::tempdir().unwrap();
        let ide = data.path().canonicalize().unwrap().join("ide");
        let program = ide.join("language-servers/jdtls/bin/jdtls");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "").unwrap();
        let project = data.path().canonicalize().unwrap().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let allowed = vec![ide.join("scratches"), ide.join("language-servers")];
        let redactor = Redactor::new(&[], vec![project]).with_store(ide.clone(), allowed);
        let mut answer = json!({ "servers": [
            { "id": "jdtls", "path": program.to_string_lossy(), "args": ["-jar"], "languages": ["java"], "state": "running" },
        ] });
        assert!(redactor.redact(&mut answer).is_empty());
        assert_eq!(answer["servers"][0]["id"], "jdtls", "the server the IDE installed is listed: {answer}");
        assert!(redactor.hides(&ide.join("settings.json").to_string_lossy()), "the rest of the folder stays hidden");
    }

    #[test]
    fn a_revisions_hidden_file_typed_in_the_terminal_is_found() {
        let redactor = redactor();
        for typed in ["git show HEAD:.env\n", "git show other:config/.env.local", "git cat-file -p main:./.env"] {
            assert!(redactor.named_in("terminal_write", &json!({ "text": typed })).is_some(), "{typed}");
        }
        assert!(redactor.named_in("terminal_write", &json!({ "text": "git show HEAD:src/main.rs" })).is_none());
    }

    #[test]
    fn a_call_naming_a_hidden_file_is_found() {
        let redactor = redactor();
        let read = json!({ "path": ".env", "startLine": 1 });
        assert_eq!(redactor.named_in("get_document_text", &read).as_deref(), Some(".env"));
        let batch = json!({ "command": "editor.text.batch", "params": { "files": [{ "path": "a.go" }, { "path": "/work/shop/k.pem" }] } });
        assert_eq!(redactor.named_in("dispatch_command", &batch).as_deref(), Some("/work/shop/k.pem"));
        assert_eq!(
            redactor.named_in("terminal_write", &json!({ "text": "cat ./.env | head\n" })).as_deref(),
            Some(".env")
        );
        assert_eq!(redactor.named_in("terminal_write", &json!({ "text": "cargo test --env\n" })), None);
        let send = json!({ "command": "terminal.session.send", "params": { "text": "less id_rsa\n" } });
        assert_eq!(redactor.named_in("dispatch_command", &send).as_deref(), Some("id_rsa"));
        assert_eq!(redactor.named_in("get_document_text", &json!({ "path": "src/main.rs" })), None);
    }

    #[test]
    fn calls_on_the_document_in_front_are_known() {
        let call = |command: &str, params: Value| json!({ "command": command, "params": params });
        assert!(acts_on_active_document("dispatch_command", &call("editor.selection.get", json!({})), false));
        assert!(acts_on_active_document(
            "dispatch_command",
            &call("problems.file.list", json!({ "path": null })),
            false
        ));
        assert!(
            acts_on_active_document(
                "dispatch_command",
                &call("editor.selection.get", json!({ "path": "a.go" })),
                false
            ),
            "a path the command does not take changes nothing"
        );
        assert!(!acts_on_active_document(
            "dispatch_command",
            &call("editor.text.replace", json!({ "path": "a.go" })),
            true
        ));
        assert!(!acts_on_active_document("dispatch_command", &call("search.text.find", json!({})), false), "scoped");
        assert!(acts_on_active_document("press_keys", &json!({ "keys": ["⌘C"] }), false));
        assert!(acts_on_active_document("run_action", &json!({ "action": "view.find.open" }), false));
        assert!(!acts_on_active_document("list_windows", &json!({}), false));
        assert!(requires_file(&json!({ "required": ["path"] })) && !requires_file(&json!({ "properties": {} })));
    }

    #[test]
    fn lists_leave_hidden_files_out_altogether() {
        let redactor = redactor();
        let mut found = json!({ "matches": [
            { "path": ".env", "line": 1, "column": 1, "text": "TOKEN=abc" },
            { "path": "src/main.rs", "line": 3, "column": 5, "text": "let token = env(\"TOKEN\");" }
        ] });
        assert_eq!(redactor.redact(&mut found), [".env"]);
        assert_eq!(found["matches"].as_array().unwrap().len(), 1, "no trace of the hidden file: {found}");
        assert_eq!(found["matches"][0]["path"], "src/main.rs");
        let mut usages = json!({ "files": [{ "path": "config/app.key", "usages": [{ "line": 2, "preview": "secret" }] }],
            "rows": ["config/app.key:2:1 secret", "src/a.rs:9:4 let x = 1;"] });
        redactor.redact(&mut usages);
        assert_eq!(usages["files"], json!([]));
        assert_eq!(usages["rows"], json!(["src/a.rs:9:4 let x = 1;"]));
        let mut problems = json!([{ "path": ".env.local", "problems": [{ "message": "unknown key supersecret" }] }]);
        redactor.redact(&mut problems);
        assert_eq!(problems, json!([]), "a problem's message tells the text too");
        let mut windows = json!({ "windows": [{ "documents": [{ "path": "/work/shop/.env", "unsaved": false }] }] });
        redactor.redact(&mut windows);
        assert_eq!(windows["windows"][0]["documents"][0]["sensitive"], true);
    }
}

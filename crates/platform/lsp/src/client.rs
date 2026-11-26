use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use lsp_types::{
    FileChangeType, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncSaveOptions,
};
use serde_json::{Value, json};

use crate::connection::{Connection, Incoming, Pending};
use crate::log::ServerLog;
use crate::{LspError, position};

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(180);

pub struct Launched {
    pub connection: Connection,
    pub process: Option<Child>,
}

pub type Launcher = Box<dyn Fn() -> std::io::Result<Launched> + Send + Sync>;

pub fn process_launcher(
    program: PathBuf,
    args: Vec<String>,
    folder: PathBuf,
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
) -> Launcher {
    logged_process_launcher(program, args, folder, env, None)
}

pub fn logged_process_launcher(
    program: PathBuf,
    args: Vec<String>,
    folder: PathBuf,
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    log: Option<ServerLog>,
) -> Launcher {
    Box::new(move || {
        zen_env::wait(zen_env::SERVERS_WAIT);
        let mut command = Command::new(&program);
        zen_env::apply(&mut command);
        let mut child = command
            .args(&args)
            .current_dir(&folder)
            .envs(env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if log.is_some() { Stdio::piped() } else { Stdio::null() })
            .spawn()?;
        if let (Some(log), Some(stderr)) = (&log, child.stderr.take()) {
            let log = log.clone();
            std::thread::spawn(move || {
                for line in std::io::BufRead::lines(std::io::BufReader::new(stderr)).map_while(Result::ok) {
                    log.push(line);
                }
            });
        }
        let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
            let _ = child.kill();
            return Err(std::io::Error::other("the server's standard streams are not available"));
        };
        Ok(Launched { connection: Connection::new(stdout, stdin), process: Some(child) })
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Notification { method: String, params: Value },
    Restarted { attempt: usize },
    GaveUp { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: std::ops::Range<usize>,
    pub text: String,
}

#[derive(Debug)]
pub struct DocumentRequest {
    pub pending: Pending,
    pub uri: String,
    pub version: i32,
}

struct Document {
    language_id: String,
    version: i32,
    text: String,
}

struct Running {
    connection: Connection,
    process: Option<Child>,
    capabilities: ServerCapabilities,
    offered: Value,
    name: Option<String>,
}

pub struct Client {
    launcher: Launcher,
    root: PathBuf,
    initialization_options: Value,
    running: Option<Running>,
    documents: BTreeMap<String, Document>,
    restarts: Vec<Instant>,
    gave_up: bool,
    waker: Option<crate::connection::Waker>,
    log: Option<ServerLog>,
    configuration: BTreeMap<String, Value>,
    folders: Vec<PathBuf>,
    content: Option<crate::connection::ContentSource>,
    after_initialized: Vec<String>,
}

impl Client {
    pub fn set_waker(&mut self, waker: crate::connection::Waker) {
        if let Some(running) = &self.running {
            running.connection.set_waker(Some(std::sync::Arc::clone(&waker)));
        }
        self.waker = Some(waker);
    }

    pub fn start(launcher: Launcher, root: &Path, initialization_options: Value) -> Result<Self, LspError> {
        Self::start_logged(launcher, root, initialization_options, None, BTreeMap::new())
    }

    pub fn start_logged(
        launcher: Launcher,
        root: &Path,
        initialization_options: Value,
        log: Option<ServerLog>,
        configuration: BTreeMap<String, Value>,
    ) -> Result<Self, LspError> {
        Self::start_serving(launcher, root, initialization_options, log, configuration, None, Vec::new())
    }

    pub fn start_serving(
        launcher: Launcher,
        root: &Path,
        initialization_options: Value,
        log: Option<ServerLog>,
        configuration: BTreeMap<String, Value>,
        content: Option<crate::connection::ContentSource>,
        after_initialized: Vec<String>,
    ) -> Result<Self, LspError> {
        let mut client = Self {
            launcher,
            root: root.to_owned(),
            initialization_options,
            running: None,
            documents: BTreeMap::new(),
            restarts: Vec::new(),
            gave_up: false,
            waker: None,
            log,
            configuration,
            folders: Vec::new(),
            content,
            after_initialized,
        };
        client.launch()?;
        Ok(client)
    }

    fn launch(&mut self) -> Result<(), LspError> {
        let Launched { connection, mut process } =
            (self.launcher)().map_err(|error| LspError::Io(error.to_string()))?;
        connection.set_waker(self.waker.clone());
        connection.set_trace(self.log.clone());
        for (section, settings) in &self.configuration {
            connection.set_configuration(section, settings.clone());
        }
        connection.set_content_source(self.content.clone());
        let root_uri = position::file_uri(&self.root);
        let folders: Vec<Value> =
            std::iter::once(&self.root).chain(&self.folders).map(|folder| workspace_folder(folder)).collect();
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "Zen IDE" },
            "rootUri": root_uri,
            "workspaceFolders": folders,
            "capabilities": client_capabilities(),
            "initializationOptions": self.initialization_options,
        });
        let answer = connection.request("initialize", params).and_then(|pending| pending.wait(INITIALIZE_TIMEOUT));
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => {
                if let Some(process) = process.as_mut() {
                    let _ = process.kill();
                }
                return Err(error);
            }
        };
        let offered = answer["capabilities"].clone();
        let capabilities: ServerCapabilities = serde_json::from_value(offered.clone()).unwrap_or_default();
        let name = answer["serverInfo"]["name"].as_str().map(str::to_owned);
        connection.notify("initialized", json!({}))?;
        for method in &self.after_initialized {
            connection.notify(method, Value::Null)?;
        }
        for (uri, document) in &self.documents {
            connection.notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": document.language_id, "version": document.version, "text": document.text } }),
            )?;
        }
        self.running = Some(Running { connection, process, capabilities, offered, name });
        Ok(())
    }

    pub fn pid(&self) -> Option<u32> {
        self.running.as_ref()?.process.as_ref().map(Child::id)
    }

    pub fn set_configuration(&mut self, section: &str, settings: Value) -> Result<(), LspError> {
        self.configuration.insert(section.to_owned(), settings.clone());
        let Some(running) = &self.running else { return Ok(()) };
        running.connection.set_configuration(section, settings.clone());
        running.connection.notify("workspace/didChangeConfiguration", json!({ "settings": { section: settings } }))
    }

    pub fn configuration(&self, section: &str) -> Option<&Value> {
        self.configuration.get(section)
    }

    pub fn initialization_options(&self) -> &Value {
        &self.initialization_options
    }

    pub fn inlay_hints(&self) -> Option<bool> {
        match self.capabilities()?.inlay_hint_provider.as_ref()? {
            lsp_types::OneOf::Left(false) => None,
            lsp_types::OneOf::Left(true) => Some(false),
            lsp_types::OneOf::Right(lsp_types::InlayHintServerCapabilities::Options(options)) => {
                Some(options.resolve_provider.unwrap_or(false))
            }
            lsp_types::OneOf::Right(lsp_types::InlayHintServerCapabilities::RegistrationOptions(options)) => {
                Some(options.inlay_hint_options.resolve_provider.unwrap_or(false))
            }
        }
    }

    pub fn document_highlights(&self) -> bool {
        match self.capabilities().and_then(|capabilities| capabilities.document_highlight_provider.as_ref()) {
            Some(lsp_types::OneOf::Left(provides)) => *provides,
            Some(lsp_types::OneOf::Right(_)) => true,
            None => false,
        }
    }

    pub fn add_workspace_folder(&mut self, folder: &Path) -> Result<Vec<Event>, LspError> {
        if folder == self.root || self.folders.iter().any(|known| known == folder) {
            return Ok(Vec::new());
        }
        let Some(folders) = self
            .capabilities()
            .and_then(|capabilities| capabilities.workspace.as_ref())
            .and_then(|workspace| workspace.workspace_folders.as_ref())
            .filter(|folders| folders.supported == Some(true))
        else {
            return Ok(Vec::new());
        };
        let changes =
            matches!(&folders.change_notifications, Some(lsp_types::OneOf::Left(true) | lsp_types::OneOf::Right(_)));
        self.folders.push(folder.to_owned());
        let mut events = Vec::new();
        if changes {
            let event = json!({ "event": { "added": [workspace_folder(folder)], "removed": [] } });
            self.connection(&mut events)?.notify("workspace/didChangeWorkspaceFolders", event)?;
        }
        Ok(events)
    }

    pub fn capabilities(&self) -> Option<&ServerCapabilities> {
        self.running.as_ref().map(|running| &running.capabilities)
    }

    pub fn provides(&self, capability: &str) -> bool {
        self.running.as_ref().is_some_and(|running| match running.offered.get(capability) {
            None | Some(Value::Null | Value::Bool(false)) => false,
            Some(_) => true,
        })
    }

    pub fn wants_file_operation(&self, operation: crate::FileOperation, will: bool, path: &Path, folder: bool) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| crate::file_operations::wanted(&running.offered, operation.key(will), path, folder))
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<Vec<Event>, LspError> {
        let mut events = Vec::new();
        self.connection(&mut events)?.notify(method, params)?;
        Ok(events)
    }

    pub fn server_name(&self) -> Option<&str> {
        self.running.as_ref().and_then(|running| running.name.as_deref())
    }

    pub fn is_running(&self) -> bool {
        self.running.as_ref().is_some_and(|running| running.connection.is_alive())
    }

    pub fn queued(&self) -> usize {
        self.running.as_ref().map_or(0, |running| running.connection.queued())
    }

    fn connection(&mut self, events: &mut Vec<Event>) -> Result<&Connection, LspError> {
        if !self.is_running() {
            self.restart(events)?;
        }
        self.running.as_ref().map(|running| &running.connection).ok_or(LspError::Closed)
    }

    fn restart(&mut self, events: &mut Vec<Event>) -> Result<(), LspError> {
        if self.gave_up {
            return Err(LspError::Closed);
        }
        if let Some(mut old) = self.running.take()
            && let Some(process) = old.process.as_mut()
        {
            let _ = process.kill();
            let _ = process.wait();
        }
        let now = Instant::now();
        self.restarts.retain(|at| now.duration_since(*at) < RESTART_WINDOW);
        if self.restarts.len() >= MAX_RESTARTS {
            self.gave_up = true;
            events.push(Event::GaveUp {
                reason: format!("the server stopped {MAX_RESTARTS} times in {} minutes", RESTART_WINDOW.as_secs() / 60),
            });
            return Err(LspError::Closed);
        }
        self.restarts.push(now);
        match self.launch() {
            Ok(()) => {
                events.push(Event::Restarted { attempt: self.restarts.len() });
                Ok(())
            }
            Err(error) => {
                self.gave_up = true;
                events.push(Event::GaveUp { reason: error.to_string() });
                Err(error)
            }
        }
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let incoming = self.running.as_ref().map(|running| running.connection.take_incoming()).unwrap_or_default();
        let mut closed = false;
        for message in incoming {
            match message {
                Incoming::Notification { method, params } => events.push(Event::Notification { method, params }),
                Incoming::Closed => closed = true,
            }
        }
        if (closed || (self.running.is_some() && !self.is_running())) && !self.gave_up {
            let _ = self.restart(&mut events);
        }
        events
    }

    pub fn take_requests(&self) -> Vec<crate::ServerRequest> {
        self.running.as_ref().map(|running| running.connection.take_requests()).unwrap_or_default()
    }

    pub fn respond(&self, id: &Value, result: Value) -> Result<(), LspError> {
        self.running.as_ref().ok_or(LspError::Closed)?.connection.respond(id, result)
    }

    pub fn version(&self, uri: &str) -> Option<i32> {
        self.documents.get(uri).map(|document| document.version)
    }

    pub fn did_open(&mut self, uri: &str, language_id: &str, text: &str) -> Result<Vec<Event>, LspError> {
        let document = Document { language_id: language_id.to_owned(), version: 1, text: text.to_owned() };
        let params = json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": text } });
        self.documents.insert(uri.to_owned(), document);
        let mut events = Vec::new();
        self.connection(&mut events)?.notify("textDocument/didOpen", params)?;
        Ok(events)
    }

    pub fn did_change(&mut self, uri: &str, edits: &[Edit]) -> Result<(i32, Vec<Event>), LspError> {
        let document = self.documents.get_mut(uri).ok_or_else(|| LspError::NotOpen(uri.to_owned()))?;
        let mut changes = Vec::new();
        for edit in edits {
            let start = edit.range.start.min(document.text.len());
            let end = edit.range.end.clamp(start, document.text.len());
            let range = json!({ "start": position::position(&document.text, start), "end": position::position(&document.text, end) });
            changes.push(json!({ "range": range, "text": edit.text }));
            if document.text.is_char_boundary(start) && document.text.is_char_boundary(end) {
                document.text.replace_range(start..end, &edit.text);
            }
        }
        document.version += 1;
        let version = document.version;
        let full = json!([{ "text": document.text }]);
        let mut events = Vec::new();
        let kind = self.capabilities().map_or(TextDocumentSyncKind::FULL, sync_kind);
        let changes = if kind == TextDocumentSyncKind::INCREMENTAL {
            Value::Array(changes)
        } else if kind == TextDocumentSyncKind::FULL {
            full
        } else {
            return Ok((version, events));
        };
        let params = json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": changes });
        self.connection(&mut events)?.notify("textDocument/didChange", params)?;
        Ok((version, events))
    }

    pub fn did_save(&mut self, uri: &str) -> Result<Vec<Event>, LspError> {
        let text = self.documents.get(uri).ok_or_else(|| LspError::NotOpen(uri.to_owned()))?.text.clone();
        let include_text = self.capabilities().is_some_and(|capabilities| match &capabilities.text_document_sync {
            Some(TextDocumentSyncCapability::Options(options)) => match &options.save {
                Some(TextDocumentSyncSaveOptions::SaveOptions(save)) => save.include_text.unwrap_or(false),
                _ => false,
            },
            _ => false,
        });
        let mut params = json!({ "textDocument": { "uri": uri } });
        if include_text {
            params["text"] = Value::String(text);
        }
        let mut events = Vec::new();
        self.connection(&mut events)?.notify("textDocument/didSave", params)?;
        Ok(events)
    }

    pub fn reopen(&mut self, uri: &str) -> Result<Vec<Event>, LspError> {
        let Some(document) = self.documents.get_mut(uri) else { return Ok(Vec::new()) };
        document.version += 1;
        let open = json!({ "textDocument": {
            "uri": uri, "languageId": document.language_id, "version": document.version, "text": document.text
        } });
        let mut events = Vec::new();
        let connection = self.connection(&mut events)?;
        connection.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }))?;
        connection.notify("textDocument/didOpen", open)?;
        Ok(events)
    }

    pub fn did_close(&mut self, uri: &str) -> Result<Vec<Event>, LspError> {
        if self.documents.remove(uri).is_none() {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        self.connection(&mut events)?.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }))?;
        Ok(events)
    }

    pub fn did_change_watched_files(&mut self, changes: &[(String, FileChangeType)]) -> Result<Vec<Event>, LspError> {
        if changes.is_empty() {
            return Ok(Vec::new());
        }
        let changes: Vec<Value> = changes.iter().map(|(uri, kind)| json!({ "uri": uri, "type": kind })).collect();
        let mut events = Vec::new();
        self.connection(&mut events)?.notify("workspace/didChangeWatchedFiles", json!({ "changes": changes }))?;
        Ok(events)
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Pending, LspError> {
        let mut events = Vec::new();
        self.connection(&mut events)?.request(method, params)
    }

    pub fn document_request(&mut self, uri: &str, method: &str, params: Value) -> Result<DocumentRequest, LspError> {
        let version = self.version(uri).ok_or_else(|| LspError::NotOpen(uri.to_owned()))?;
        Ok(DocumentRequest { pending: self.request(method, params)?, uri: uri.to_owned(), version })
    }

    pub fn is_stale(&self, request: &DocumentRequest) -> bool {
        self.version(&request.uri) != Some(request.version)
    }

    pub fn cancel(&self, pending: &Pending) {
        if let Some(running) = &self.running {
            running.connection.cancel(pending);
        }
    }

    pub fn work_canceller(&self, token: Value) -> Option<impl Fn() + Send + Sync + 'static> {
        self.running.as_ref().map(|running| running.connection.work_canceller(token))
    }

    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let Some(mut running) = self.running.take() else { return };
        if running.connection.is_alive() {
            if let Ok(pending) = running.connection.request("shutdown", Value::Null) {
                let _ = pending.wait(Duration::from_secs(3));
            }
            let _ = running.connection.notify("exit", Value::Null);
        }
        if let Some(process) = running.process.as_mut() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = process.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = process.kill();
            let _ = process.wait();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop();
    }
}

fn workspace_folder(folder: &Path) -> Value {
    let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    json!({ "uri": position::file_uri(folder), "name": name })
}

fn sync_kind(capabilities: &ServerCapabilities) -> TextDocumentSyncKind {
    match &capabilities.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(kind)) => *kind,
        Some(TextDocumentSyncCapability::Options(options)) => options.change.unwrap_or(TextDocumentSyncKind::NONE),
        None => TextDocumentSyncKind::NONE,
    }
}

fn client_capabilities() -> Value {
    json!({
        "general": { "positionEncodings": ["utf-16"] },
        "window": { "workDoneProgress": true },
        "experimental": { "serverStatusNotification": true, "snippetTextEdit": true },
        "workspace": {
            "configuration": true,
            "workspaceFolders": true,
            "applyEdit": true,
            "workspaceEdit": {
                "documentChanges": true,
                "resourceOperations": ["create", "rename", "delete"],
                "failureHandling": "undo"
            },
            "fileOperations": {
                "dynamicRegistration": false,
                "didCreate": true,
                "willCreate": true,
                "didRename": true,
                "willRename": true,
                "didDelete": true,
                "willDelete": true
            },
            "didChangeWatchedFiles": {},
            "didChangeConfiguration": { "dynamicRegistration": false },
            "inlayHint": { "refreshSupport": true },
            "diagnostics": { "refreshSupport": true }
        },
        "textDocument": {
            "synchronization": { "dynamicRegistration": false, "didSave": true, "willSave": false },
            "diagnostic": { "dynamicRegistration": false, "relatedDocumentSupport": false },
            "publishDiagnostics": {
                "relatedInformation": true,
                "versionSupport": true,
                "tagSupport": { "valueSet": [1, 2] },
            },
            "completion": {
                "completionItem": {
                    "snippetSupport": true,
                    "insertReplaceSupport": false,
                    "documentationFormat": ["markdown", "plaintext"],
                    "labelDetailsSupport": false
                },
                "contextSupport": true
            },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "signatureHelp": {
                "signatureInformation": {
                    "documentationFormat": ["markdown", "plaintext"],
                    "parameterInformation": { "labelOffsetSupport": true },
                    "activeParameterSupport": true
                },
                "contextSupport": true
            },
            "definition": { "linkSupport": true },
            "declaration": { "linkSupport": true },
            "typeDefinition": { "linkSupport": true },
            "implementation": { "linkSupport": true },
            "references": {},
            "callHierarchy": { "dynamicRegistration": false },
            "typeHierarchy": { "dynamicRegistration": false },
            "documentHighlight": { "dynamicRegistration": false },
            "codeLens": {},
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "formatting": {},
            "codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "source", "source.organizeImports"] }
                }
            },
            "rename": { "prepareSupport": true },
            "inlayHint": {
                "dynamicRegistration": false,
                "resolveSupport": { "properties": ["tooltip", "label.tooltip", "label.location", "textEdits"] }
            },
            "semanticTokens": {
                "requests": { "full": true },
                "tokenTypes": [
                    "namespace", "type", "class", "enum", "interface", "struct", "typeParameter", "parameter",
                    "variable", "property", "enumMember", "event", "function", "method", "macro", "keyword",
                    "modifier", "comment", "string", "number", "regexp", "operator", "decorator", "label"
                ],
                "tokenModifiers": [
                    "declaration", "definition", "readonly", "static", "deprecated", "abstract", "async",
                    "modification", "documentation", "defaultLibrary"
                ],
                "formats": ["relative"],
                "overlappingTokenSupport": false,
                "multilineTokenSupport": false
            }
        }
    })
}

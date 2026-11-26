use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_http::{Environments, Exchange, ExchangeState, Header, History, PrepareError, Request, Variables};
use zen_i18n::t;
use zen_run::Waker;

use crate::Workbench;

pub const TIMEOUT: Duration = Duration::from_secs(60);

pub const MAX_WAIT: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Shelf {
    project: Option<Option<PathBuf>>,
    history: History,
    next_id: u64,
    revision: u64,
}

#[derive(Default)]
struct Book {
    shelf: Mutex<Shelf>,
    changed: Condvar,
}

impl Book {
    fn shelf(&self) -> MutexGuard<'_, Shelf> {
        self.shelf.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Default)]
pub struct HttpClient {
    data_dir: Option<PathBuf>,
    environment: Option<String>,
    book: Arc<Book>,
    waker: Option<Waker>,
}

impl HttpClient {
    pub fn set_data_dir(&mut self, data_dir: PathBuf) {
        self.data_dir = Some(data_dir);
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    pub fn environment(&self) -> Option<&str> {
        self.environment.as_deref()
    }

    pub fn revision(&self) -> u64 {
        self.book.shelf().revision
    }

    pub fn any_running(&self) -> bool {
        self.book.shelf().history.exchanges().iter().any(|exchange| exchange.state == ExchangeState::Running)
    }

    fn history_file(&self, project: Option<&Path>) -> Option<PathBuf> {
        let project = project?;
        Some(zen_http::history_file(self.data_dir.as_deref()?, project)).filter(|file| !file.starts_with(project))
    }

    fn opened(&self, project: Option<&Path>) -> MutexGuard<'_, Shelf> {
        let mut shelf = self.book.shelf();
        let project = project.map(Path::to_owned);
        if shelf.project.as_ref() != Some(&project) {
            shelf.history = History::load(self.history_file(project.as_deref()));
            shelf.next_id = shelf.history.last_id() + 1;
            shelf.project = Some(project);
            shelf.revision += 1;
        }
        shelf
    }
}

fn project_of(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

fn http_text(session: &Session, path: &Path) -> Result<String, CommandError> {
    if !zen_http::is_http_file(path) {
        return Err(CommandError::new(t!("http-error-not-http", path = path.display().to_string())));
    }
    crate::run::file_text(session, path).map(|(text, _)| text)
}

fn without_hidden_environments(
    session: &Session,
    found: &Environments,
    project: Option<&Path>,
) -> (Environments, Vec<(PathBuf, std::collections::BTreeSet<String>)>) {
    let mut usable = found.clone();
    let mut hidden = Vec::new();
    let (Some(patterns), Some(folder)) = (crate::hidden_files::for_caller(session), found.folder.as_deref()) else {
        return (usable, hidden);
    };
    let names = |file: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>| {
        file.values().flat_map(|values| values.keys().cloned()).collect::<std::collections::BTreeSet<String>>()
    };
    let public = folder.join(zen_http::env::PUBLIC_FILE);
    if crate::hidden_files::is_hidden(&patterns, project, &public) {
        hidden.push((public, names(&usable.public)));
        usable.public.clear();
    }
    let private = folder.join(zen_http::env::PRIVATE_FILE);
    if crate::hidden_files::is_hidden(&patterns, project, &private) {
        hidden.push((private, names(&usable.private)));
        usable.private.clear();
    }
    (usable, hidden)
}

fn environments(path: &Path, project: Option<&Path>) -> Result<Environments, CommandError> {
    Environments::find(path, project).map_err(|error| {
        CommandError::new(t!("http-error-env-file", path = error.file.display().to_string(), error = error.message))
    })
}

/// The parameters of `http.requests.list`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// The `.http` or `.rest` file.
    pub path: PathBuf,
}

/// What `http.requests.list` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAnswer {
    /// The requests in the order written, each with its lines (from 1).
    pub requests: Vec<Request>,
    /// The file's own variables (`@name = value`).
    pub variables: Vec<Header>,
    /// The environments of the nearest `http-client.env.json` and `http-client.private.env.json`.
    pub environments: Vec<String>,
    /// The selected environment.
    pub env: Option<String>,
    /// The folder the environment files are in.
    pub environment_folder: Option<PathBuf>,
    /// Why an environment file could not be read.
    pub environment_error: Option<String>,
}

pub struct ListRequests;

impl Command for ListRequests {
    type Params = ListParams;
    type Output = ListAnswer;
    const ID: &'static str = "http.requests.list";
    const TITLE: &'static str = "HTTP Requests";
    const DESCRIPTION: &'static str = "The requests of an .http or .rest file (the open document's text, else the \
         file on disk) by line, as the gutter's run icons show them, with the file's variables, the environments \
         of the nearest http-client.env.json and the selected one. Never sends anything.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ListAnswer, CommandError> {
        let text = http_text(session, &params.path)?;
        let (requests, variables) = zen_http::parse(&text);
        let project = project_of(session);
        let (found, environment_error) = match environments(&params.path, project.as_deref()) {
            Ok(found) => (found, None),
            Err(error) => (Environments::default(), Some(error.message)),
        };
        Ok(ListAnswer {
            requests: requests.into_iter().map(from_one).collect(),
            variables: variables.into_iter().map(|(name, value)| Header { name, value }).collect(),
            environments: found.names(),
            env: session.state::<HttpClient>().and_then(|client| client.environment.clone()),
            environment_folder: found.folder,
            environment_error,
        })
    }
}

fn from_one(request: Request) -> Request {
    Request { line: request.line + 1, start_line: request.start_line + 1, end_line: request.end_line + 1, ..request }
}

/// The parameters of `http.environment.select`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectParams {
    /// The environment requests run with; none when absent or empty.
    pub env: Option<String>,
}

/// What `http.environment.select` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectAnswer {
    /// The selected environment.
    pub env: Option<String>,
}

pub struct SelectEnvironment;

impl Command for SelectEnvironment {
    type Params = SelectParams;
    type Output = SelectAnswer;
    const ID: &'static str = "http.environment.select";
    const TITLE: &'static str = "Select HTTP Environment";
    const DESCRIPTION: &'static str = "Selects the environment of http-client.env.json that requests of this window \
         run with (the gutter's run icons and http.request.run without env); none when env is absent or empty.";

    fn run(&self, session: &mut Session, params: SelectParams) -> Result<SelectAnswer, CommandError> {
        let env = params.env.filter(|env| !env.is_empty());
        session.state_mut::<HttpClient>().environment = env.clone();
        Ok(SelectAnswer { env })
    }
}

/// The parameters of `http.request.run`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunParams {
    /// The `.http` or `.rest` file.
    pub path: PathBuf,
    /// The request's place in the file, from 0, as `http.requests.list` gives it.
    pub index: Option<usize>,
    /// The request's name (`# @name`), instead of `index`.
    pub name: Option<String>,
    /// The environment; the selected one when absent, none when empty.
    pub env: Option<String>,
    /// How long to wait for the answer, in milliseconds (at most 60 000); the answer is then in the result.
    pub wait_ms: Option<u64>,
}

pub struct RunRequest;

impl Command for RunRequest {
    type Params = RunParams;
    type Output = Exchange;
    const ID: &'static str = "http.request.run";
    const TITLE: &'static str = "Run HTTP Request";
    const DESCRIPTION: &'static str = "Sends one request of an .http or .rest file, chosen by index or name, with its \
         variables resolved from the file and the environment, and returns the exchange (state running, done or \
         failed); with waitMs it waits for the answer. The only command that sends anything: a request goes out \
         only when this runs, from the gutter's run icon or an agent. The response is kept in the IDE's data \
         folder (http.responses.list), never in the project.";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<Exchange, CommandError> {
        let text = http_text(session, &params.path)?;
        let (requests, file_variables) = zen_http::parse(&text);
        let request = match (&params.name, params.index) {
            (Some(name), _) => requests.iter().find(|request| request.name.as_deref() == Some(name.as_str())),
            (None, Some(index)) => requests.get(index),
            (None, None) if requests.len() == 1 => requests.first(),
            (None, None) => return Err(CommandError::new(t!("http-error-which-request"))),
        };
        let Some(request) = request.cloned() else {
            let which = params.name.clone().unwrap_or_else(|| params.index.unwrap_or_default().to_string());
            return Err(CommandError::new(t!(
                "http-error-no-request",
                request = which,
                path = params.path.display().to_string()
            )));
        };
        let project = project_of(session);
        let env = match params.env {
            Some(env) => Some(env).filter(|env| !env.is_empty()),
            None => session.state::<HttpClient>().and_then(|client| client.environment.clone()),
        };
        let found = environments(&params.path, project.as_deref())?;
        if let Some(env) = &env
            && !found.names().contains(env)
        {
            return Err(CommandError::new(t!("http-error-no-environment", env = env.clone())));
        }
        let (usable, hidden_env) = without_hidden_environments(session, &found, project.as_deref());
        let variables = Variables::new(&file_variables, usable.variables(env.as_deref()))
            .with_private(usable.private_names(env.as_deref()));
        let folder = params.path.parent().map(Path::to_owned).unwrap_or_default();
        let mut prepared = match zen_http::prepare(&request, &variables, &folder, project.as_deref()) {
            Ok(prepared) => prepared,
            Err(PrepareError::Unresolved(names)) if !hidden_env.is_empty() => {
                let all = found.variables(env.as_deref());
                if let Some((file, _)) = names.iter().find_map(|name| {
                    let hidden = hidden_env.iter().find(|(_, values)| values.contains(name));
                    hidden.filter(|_| all.contains_key(name))
                }) {
                    return Err(crate::hidden_files::refused(&file.display().to_string()));
                }
                return Err(prepare_error(&PrepareError::Unresolved(names)));
            }
            Err(error) => return Err(prepare_error(&error)),
        };
        prepared.use_proxy |= !may_skip_the_proxy(session, project.as_deref());
        if let Some(patterns) = crate::hidden_files::for_caller(session)
            && let Some(file) =
                prepared.files.iter().find(|file| crate::hidden_files::is_hidden(&patterns, project.as_deref(), file))
        {
            return Err(crate::hidden_files::refused(&file.display().to_string()));
        }
        let client = session.state_mut::<HttpClient>();
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
        let exchange = {
            let mut shelf = client.opened(project.as_deref());
            let exchange = Exchange {
                id: shelf.next_id,
                file: params.path.clone(),
                index: request.index,
                name: request.name.clone(),
                environment: env,
                method: prepared.method.clone(),
                url: prepared.shown.url.clone(),
                request_headers: prepared
                    .shown
                    .headers
                    .iter()
                    .map(|(name, value)| Header { name: name.clone(), value: value.clone() })
                    .collect(),
                request_body: prepared
                    .shown
                    .body
                    .as_deref()
                    .map(|body| zen_http::cut(&String::from_utf8_lossy(body), zen_http::history::BODY_KEPT).0),
                started,
                state: ExchangeState::Running,
                duration_ms: None,
                response: None,
                error: None,
                body_files: prepared.files.clone(),
            };
            shelf.next_id += 1;
            shelf.revision += 1;
            shelf.history.put(exchange.clone());
            exchange
        };
        start(Arc::clone(&client.book), client.waker.clone(), exchange.clone(), prepared);
        match params.wait_ms {
            Some(wait) => {
                Ok(wait_for(&client.book, exchange.id, Duration::from_millis(wait).min(MAX_WAIT)).unwrap_or(exchange))
            }
            None => Ok(exchange),
        }
    }
}

fn prepare_error(error: &PrepareError) -> CommandError {
    match error {
        PrepareError::Unresolved(names) => {
            CommandError::with_code("unresolvedVariables", t!("http-error-unresolved", names = names.join(", ")))
        }
        PrepareError::BodyFile { path, error } => {
            CommandError::new(t!("http-error-body-file", path = path.clone(), error = error.clone()))
        }
        PrepareError::BodyOutside { path } => CommandError::new(t!("http-error-body-outside", path = path.clone())),
        PrepareError::NoUrl => CommandError::new(t!("http-error-no-url")),
        PrepareError::Scheme(scheme) => CommandError::new(t!("http-error-scheme", scheme = scheme.clone())),
    }
}

pub(crate) fn may_skip_the_proxy(session: &Session, project: Option<&Path>) -> bool {
    project.is_none_or(|project| crate::trust::trusted(session, project))
}

#[cfg(not(target_arch = "wasm32"))]
fn send_error(error: &zen_http::SendError, prepared: &zen_http::Prepared) -> String {
    let error = match error {
        zen_http::SendError::Failed(error) => error.replace(&prepared.url, &prepared.shown.url),
        zen_http::SendError::RedirectScheme(scheme) => t!("http-error-redirect-scheme", scheme = scheme.clone()),
        zen_http::SendError::TooManyRedirects => t!("http-error-redirects"),
        zen_http::SendError::Timeout => t!("http-error-timeout"),
    };
    t!("http-error-send", error = error)
}

fn start(book: Arc<Book>, waker: Option<Waker>, exchange: Exchange, prepared: zen_http::Prepared) {
    let finish = move |outcome: Result<zen_http::Response, String>| {
        let mut ended = exchange;
        match outcome {
            Ok(response) => {
                ended.state = ExchangeState::Done;
                ended.duration_ms = Some(response.duration_ms);
                ended.response = Some(zen_http::StoredResponse::of(&response));
            }
            Err(error) => {
                ended.state = ExchangeState::Failed;
                ended.error = Some(error);
            }
        }
        {
            let mut shelf = book.shelf();
            shelf.history.put(ended);
            let _ = shelf.history.save();
            shelf.revision += 1;
        }
        book.changed.notify_all();
        if let Some(waker) = &waker {
            waker();
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(move || {
        let sent = zen_net::trust::asking_the_person(|| zen_http::send(&prepared, TIMEOUT));
        finish(sent.map_err(|error| send_error(&error, &prepared)))
    });
    #[cfg(target_arch = "wasm32")]
    {
        let _ = prepared;
        finish(Err(t!("http-error-unavailable")));
    }
}

fn wait_for(book: &Book, id: u64, wait: Duration) -> Option<Exchange> {
    let deadline = Instant::now() + wait;
    let mut shelf = book.shelf();
    loop {
        let exchange = shelf.history.get(id).cloned();
        let now = Instant::now();
        if exchange.as_ref().is_none_or(|exchange| exchange.state != ExchangeState::Running) || now >= deadline {
            return exchange;
        }
        shelf = book.changed.wait_timeout(shelf, deadline - now).unwrap_or_else(std::sync::PoisonError::into_inner).0;
    }
}

/// The parameters of `http.responses.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResponsesParams {
    /// Only the responses of this request file.
    pub path: Option<PathBuf>,
    /// At most this many, the newest.
    pub limit: Option<usize>,
}

/// What `http.responses.list` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponsesAnswer {
    /// The exchanges, newest first, the running ones too.
    pub responses: Vec<Exchange>,
    /// The file they are kept in, in the IDE's data folder; none without a project.
    pub history_file: Option<PathBuf>,
}

pub struct ListResponses;

impl Command for ListResponses {
    type Params = ResponsesParams;
    type Output = ResponsesAnswer;
    const ID: &'static str = "http.responses.list";
    const TITLE: &'static str = "HTTP Responses";
    const DESCRIPTION: &'static str = "The project's HTTP Client history, newest first: each request that was run \
         with its resolved URL and headers, the status, headers, body (text, at most 1 MiB) and timing, or why it \
         failed. Kept in the IDE's data folder (historyFile), the newest 50.";

    fn run(&self, session: &mut Session, params: ResponsesParams) -> Result<ResponsesAnswer, CommandError> {
        let project = project_of(session);
        let client = session.state_mut::<HttpClient>();
        let history_file = client.history_file(project.as_deref());
        let shelf = client.opened(project.as_deref());
        let responses = shelf
            .history
            .exchanges()
            .iter()
            .filter(|exchange| params.path.as_ref().is_none_or(|path| exchange.file == *path))
            .take(params.limit.unwrap_or(usize::MAX))
            .cloned()
            .collect();
        drop(shelf);
        let mut answer = ResponsesAnswer { responses, history_file };
        for exchange in &mut answer.responses {
            redact_exchange(session, project.as_deref(), exchange);
        }
        Ok(answer)
    }
}

/// The parameters of `http.response.get`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetParams {
    /// The exchange's id, from `http.request.run` or `http.responses.list`.
    pub id: u64,
    /// How long to wait while it runs, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
}

pub struct GetResponse;

impl Command for GetResponse {
    type Params = GetParams;
    type Output = Exchange;
    const ID: &'static str = "http.response.get";
    const TITLE: &'static str = "HTTP Response";
    const DESCRIPTION: &'static str = "One exchange of the HTTP Client history by id; with waitMs it first waits \
         while the request runs.";

    fn run(&self, session: &mut Session, params: GetParams) -> Result<Exchange, CommandError> {
        let project = project_of(session);
        let client = session.state_mut::<HttpClient>();
        drop(client.opened(project.as_deref()));
        let mut exchange =
            wait_for(&client.book, params.id, Duration::from_millis(params.wait_ms.unwrap_or(0)).min(MAX_WAIT))
                .ok_or_else(|| CommandError::new(t!("http-error-no-response", id = params.id.to_string())))?;
        redact_exchange(session, project.as_deref(), &mut exchange);
        Ok(exchange)
    }
}

fn redact_exchange(session: &Session, project: Option<&Path>, exchange: &mut Exchange) {
    let Some(patterns) = crate::hidden_files::for_caller(session) else { return };
    if !exchange.body_files.iter().any(|file| crate::hidden_files::is_hidden(&patterns, project, file)) {
        return;
    }
    if let Some(body) = &mut exchange.request_body {
        *body = REDACTED.to_owned();
    }
    if let Some(response) = &mut exchange.response {
        response.body = REDACTED.to_owned();
    }
}

const REDACTED: &str = "[redacted]";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListRequests)?;
    registry.register(SelectEnvironment)?;
    registry.register(RunRequest)?;
    registry.register(ListResponses)?;
    registry.register(GetResponse)
}

#[cfg(test)]
mod tests;

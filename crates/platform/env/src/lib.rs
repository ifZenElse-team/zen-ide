use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(10);

pub const SERVERS_WAIT: Duration = Duration::from_secs(5);

const MARKER: &str = "zen-environment-";

pub const KEPT_FROM_THE_IDE: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "SHELL",
    "__CF_USER_TEXT_ENCODING",
    "XPC_FLAGS",
    "XPC_SERVICE_NAME",
    "SECURITYSESSIONID",
    "COMMAND_MODE",
];

pub const OF_THE_SHELL_ITSELF: &[&str] = &[
    "_",
    "SHLVL",
    "PWD",
    "OLDPWD",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "COLORTERM",
    "ITERM_SESSION_ID",
    "SHELL_SESSION_ID",
    "ZDOTDIR",
    "PS1",
    "PS2",
    "PS3",
    "PS4",
    "PROMPT",
    "RPROMPT",
    "HISTFILE",
    "LINES",
    "COLUMNS",
    "TTY",
    "SSH_TTY",
];

pub const NEVER_PREFIXES: &[&str] = &["ZEN_", "DYLD_", "LD_", "BASH_FUNC_"];

pub fn taken(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['=', '\0'])
        && !KEPT_FROM_THE_IDE.contains(&name)
        && !OF_THE_SHELL_ITSELF.contains(&name)
        && !NEVER_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    pub shell: PathBuf,
    pub variables: BTreeMap<String, String>,
}

impl Environment {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    NotRead,
    Reading,
    Ready(Arc<Environment>),
    Failed(String),
}

pub fn nonce() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(not(target_arch = "wasm32"))]
fn fill_random(bytes: &mut [u8]) -> Result<(), String> {
    getrandom::fill(bytes).map_err(|error| format!("no random nonce: {error}"))
}

#[cfg(target_arch = "wasm32")]
fn fill_random(_bytes: &mut [u8]) -> Result<(), String> {
    Err("WebAssembly starts no login shell".to_owned())
}

fn markers(nonce: &str) -> (String, String) {
    (format!("<{MARKER}{nonce}>"), format!("</{MARKER}{nonce}>"))
}

pub fn parse(output: &[u8], nonce: &str) -> Result<BTreeMap<String, String>, String> {
    let (open, close) = markers(nonce);
    let text = String::from_utf8_lossy(output);
    let start = text.find(&open).ok_or("the shell printed no environment")? + open.len();
    let end = start + text[start..].find(&close).ok_or("the shell's environment was cut short")?;
    let variables: BTreeMap<String, String> = text[start..end]
        .split('\0')
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, _)| taken(name))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    if variables.is_empty() {
        return Err("the shell printed an empty environment".to_owned());
    }
    Ok(variables)
}

pub fn command_args(nonce: &str) -> Vec<String> {
    let script =
        format!("printf '\\n%s%s>' '<{MARKER}' '{nonce}'; /usr/bin/env -0; printf '%s%s>\\n' '</{MARKER}' '{nonce}'");
    vec!["-l".to_owned(), "-i".to_owned(), "-c".to_owned(), script]
}

pub fn read(shell: &Path, home: &Path, timeout: Duration) -> Result<Environment, String> {
    let nonce = nonce()?;
    let mut child = std::process::Command::new(shell)
        .args(command_args(&nonce))
        .current_dir(home)
        .env("HOME", home)
        .env_remove("ZDOTDIR")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("{}: {error}", shell.display()))?;
    let mut stdout = child.stdout.take().ok_or("the shell has no output")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stdout.read_to_end(&mut output);
        let _ = sender.send(output);
    });
    let output = receiver.recv_timeout(timeout);
    if output.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let output = output.map_err(|_| format!("the shell did not answer within {} s", timeout.as_secs()))?;
    Ok(Environment { shell: shell.to_owned(), variables: parse(&output, &nonce)? })
}

struct Global {
    state: Mutex<State>,
    changed: Condvar,
    enabled: AtomicBool,
}

fn global() -> &'static Global {
    static GLOBAL: std::sync::OnceLock<Global> = std::sync::OnceLock::new();
    GLOBAL.get_or_init(|| Global {
        state: Mutex::new(State::NotRead),
        changed: Condvar::new(),
        enabled: AtomicBool::new(true),
    })
}

fn set_state(state: State) {
    let global = global();
    *global.state.lock().unwrap_or_else(PoisonError::into_inner) = state;
    global.changed.notify_all();
}

pub fn user_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" }))
}

pub fn start(shell: PathBuf, home: PathBuf, timeout: Duration) -> bool {
    {
        let mut state = global().state.lock().unwrap_or_else(PoisonError::into_inner);
        if *state != State::NotRead {
            return false;
        }
        *state = State::Reading;
    }
    std::thread::spawn(move || {
        let state = match read(&shell, &home, timeout) {
            Ok(environment) => {
                log::info!("the login shell {} gave {} variables", shell.display(), environment.variables.len());
                State::Ready(Arc::new(environment))
            }
            Err(error) => {
                log::warn!("the login shell {} gave no environment: {error}", shell.display());
                State::Failed(error)
            }
        };
        set_state(state);
    });
    true
}

pub fn adopt(environment: Environment) {
    set_state(State::Ready(Arc::new(environment)));
}

pub fn set_enabled(enabled: bool) {
    global().enabled.store(enabled, Ordering::SeqCst);
    global().changed.notify_all();
}

pub fn enabled() -> bool {
    global().enabled.load(Ordering::SeqCst)
}

pub fn state() -> State {
    global().state.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub fn wait(timeout: Duration) -> State {
    let global = global();
    let deadline = Instant::now() + timeout;
    let mut state = global.state.lock().unwrap_or_else(PoisonError::into_inner);
    while *state == State::Reading {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        state = global.changed.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
    }
    state.clone()
}

pub fn current() -> Option<Arc<Environment>> {
    if !enabled() {
        return None;
    }
    match state() {
        State::Ready(environment) => Some(environment),
        _ => None,
    }
}

pub fn variables() -> Vec<(String, String)> {
    current().map(|environment| environment.variables.clone().into_iter().collect()).unwrap_or_default()
}

pub fn var(name: &str) -> Option<OsString> {
    match current() {
        Some(environment) => environment.get(name).map(OsString::from).or_else(|| std::env::var_os(name)),
        None => std::env::var_os(name),
    }
}

pub fn path_folders() -> Vec<PathBuf> {
    current()
        .and_then(|environment| environment.get("PATH").map(|path| std::env::split_paths(path).collect()))
        .unwrap_or_default()
}

pub fn apply(command: &mut std::process::Command) {
    for (name, value) in variables() {
        command.env(name, value);
    }
}

#[cfg(test)]
mod tests;

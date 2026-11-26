use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use zen_core::Registry;
use zen_i18n::t;

use super::{IdeApp, IdeProduct};

const MAX_WAIT: Duration = Duration::from_millis(500);

const FIRST_CLIENT_WAIT: Duration = Duration::from_secs(60);

const LINGER: Duration = Duration::from_secs(1);

pub const EXIT_ALREADY_RUNNING: i32 = 3;

pub const EXIT_USAGE: i32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeadlessOptions {
    pub projects: Vec<PathBuf>,
    pub mcp_port: Option<u16>,
    pub exit_with_parent: bool,
    pub exit_when_unused: bool,
    pub accept_open: bool,
    pub no_restore: bool,
}

impl HeadlessOptions {
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<Self>, String> {
        let args: Vec<OsString> = args.into_iter().collect();
        if !args.iter().any(|arg| arg == "--headless") {
            return Ok(None);
        }
        let mut options = HeadlessOptions::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--headless") => {}
                Some("--exit-with-parent") => options.exit_with_parent = true,
                Some("--exit-when-unused") => options.exit_when_unused = true,
                Some("--accept-open") => options.accept_open = true,
                Some("--no-restore") => options.no_restore = true,
                Some("--mcp-port") => {
                    let value = args.next().ok_or_else(|| t!("ide-headless-no-port"))?;
                    options.mcp_port = Some(port(&value.to_string_lossy())?);
                }
                Some(text) if text.starts_with("--mcp-port=") => {
                    options.mcp_port = Some(port(&text["--mcp-port=".len()..])?);
                }
                Some(text) if text.starts_with("--") => return Err(t!("ide-headless-unknown", option = text)),
                _ => options.projects.push(PathBuf::from(arg)),
            }
        }
        if options.mcp_port.is_none()
            && let Ok(value) = std::env::var("ZEN_AUTOMATION")
        {
            options.mcp_port = automation_port(&value, 0).filter(|port| *port != 0);
        }
        Ok(Some(options))
    }
}

pub fn automation_port(value: &str, default: u16) -> Option<u16> {
    match value.trim() {
        "" | "0" => None,
        text => Some(text.parse().ok().filter(|port: &u16| *port > 1).unwrap_or(default)),
    }
}

pub fn usage(program: &str, error: &str) -> String {
    t!("ide-headless-usage", error = error, program = program)
}

fn port(text: &str) -> Result<u16, String> {
    text.parse().map_err(|_| t!("ide-headless-bad-port", value = text))
}

#[derive(Default)]
struct Wake {
    at: Mutex<Option<Instant>>,
    signal: Condvar,
}

impl Wake {
    fn request(&self, delay: Duration) {
        let at = Instant::now() + delay;
        let mut slot = self.at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.is_none_or(|current| at < current) {
            *slot = Some(at);
            self.signal.notify_all();
        }
    }

    fn wait(&self, longest: Duration) {
        let deadline = Instant::now() + longest;
        let mut slot = self.at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            let now = Instant::now();
            let until = slot.map_or(deadline, |at| at.min(deadline));
            if until <= now {
                break;
            }
            slot = self.signal.wait_timeout(slot, until - now).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
        *slot = None;
    }
}

fn context() -> (egui::Context, Arc<Wake>) {
    let ctx = egui::Context::default();
    let wake = Arc::new(Wake::default());
    let signal = Arc::clone(&wake);
    ctx.set_request_repaint_callback(move |info| signal.request(info.delay));
    (ctx, wake)
}

fn run_loop(app: &mut IdeApp, ctx: &egui::Context, wake: &Wake, should_end: &mut dyn FnMut() -> bool) {
    let mut asked_to_exit = false;
    loop {
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        if app.finished() {
            return;
        }
        if !asked_to_exit && should_end() {
            asked_to_exit = true;
            app.request_exit();
            continue;
        }
        wake.wait(MAX_WAIT);
    }
}

struct Unused<F: FnMut() -> Option<usize>> {
    clients: F,
    had_clients: bool,
    empty_since: Instant,
    checked: Option<Instant>,
}

impl<F: FnMut() -> Option<usize>> Unused<F> {
    fn new(clients: F) -> Self {
        Self { clients, had_clients: false, empty_since: Instant::now(), checked: None }
    }

    fn now_unused(&mut self) -> bool {
        let now = Instant::now();
        if self.checked.is_some_and(|checked| now - checked < MAX_WAIT / 2) {
            return false;
        }
        self.checked = Some(now);
        match (self.clients)() {
            Some(0) if !self.had_clients => {
                self.had_clients = true;
                self.empty_since = now;
                false
            }
            Some(0) => now - self.empty_since >= LINGER,
            Some(_) => {
                self.had_clients = true;
                self.empty_since = now;
                false
            }
            None => now - self.empty_since >= FIRST_CLIENT_WAIT,
        }
    }
}

#[cfg(unix)]
fn parent_watch() -> impl Fn() -> bool {
    let parent = std::os::unix::process::parent_id();
    move || std::os::unix::process::parent_id() != parent
}

#[cfg(not(unix))]
fn parent_watch() -> impl Fn() -> bool {
    || false
}

pub fn run_headless(
    product: IdeProduct,
    registry: Registry,
    recent_file: Option<PathBuf>,
    options: HeadlessOptions,
) -> i32 {
    let data = recent_file.as_deref().and_then(std::path::Path::parent).map(std::path::Path::to_owned);
    if let Some(data) = &data {
        for project in &options.projects {
            if let Some(running) = zen_automation::instances::with_project(data, project) {
                let message = t!(
                    "ide-headless-running",
                    project = project.display().to_string(),
                    product = product.name,
                    pid = running.pid.to_string(),
                    url = running.url.as_str()
                );
                let _ = writeln!(std::io::stderr(), "{}: {message}", product.id);
                return EXIT_ALREADY_RUNNING;
            }
        }
    }
    let id = product.id;
    let port = options.mcp_port.unwrap_or(0);
    let mut app = IdeApp::build_restoring(product, registry, recent_file, options.projects, true, !options.no_restore);
    app.enable_automation(port);
    if options.accept_open {
        app.accept_launchers();
    }
    let (ctx, wake) = context();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    let Some(url) = app.automation_url() else {
        let error = t!("ide-headless-no-server", port = port.to_string());
        let _ = writeln!(std::io::stderr(), "{id}: {error}");
        return 1;
    };
    let pid = std::process::id();
    if let Some(data) = &data {
        zen_automation::instances::forget(data, pid);
    }
    let instance = zen_automation::instances::Instance {
        pid,
        url: url.clone(),
        product: id.to_owned(),
        projects: app.open_projects(),
        started: zen_automation::instances::now(),
    };
    if let Some(data) = &data {
        let _ = zen_automation::instances::record(data, &instance);
    }
    let line = serde_json::json!({ "url": url, "pid": pid, "projects": instance.projects });
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{line}").and_then(|()| stdout.flush());
    let mut stderr = std::io::stderr();
    for line in app.language_server_report() {
        let _ = writeln!(stderr, "{id}: {line}");
    }
    let parent = parent_watch();
    let leases = data.clone();
    let mut unused =
        Unused::new(move || leases.as_deref().and_then(|data| zen_automation::instances::leases(data, pid)));
    let mut should_end = || {
        let end = (options.exit_with_parent && parent()) || (options.exit_when_unused && unused.now_unused());
        if end && let Some(data) = &data {
            zen_automation::instances::forget(data, pid);
        }
        end
    };
    run_loop(&mut app, &ctx, &wake, &mut should_end);
    if let Some(data) = &data {
        zen_automation::instances::forget(data, pid);
    }
    drop(app);
    0
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use crate::tasks::Waker;

pub const OPEN_ACTION: &str = "view.ideErrors.open";

const KEPT: usize = 100;
const ISSUES: &str = "https://github.com/ifZenElse-team/zen-ide/issues/new";
const URL_STACK: usize = 4000;

/// An internal error of the IDE.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct InternalError {
    /// Its number, for `ide.errors.describe` and `ide.errors.report`.
    pub id: u64,
    /// What went wrong, and where.
    pub message: String,
    /// The stack when it happened.
    pub stack: String,
    /// What the person was doing, in their words.
    pub description: String,
}

#[derive(Default)]
struct Kept {
    errors: Vec<InternalError>,
    next: u64,
    product: String,
    waker: Option<Waker>,
}

#[derive(Clone, Default)]
pub struct InternalErrors(Arc<Mutex<Kept>>);

impl std::fmt::Debug for InternalErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalErrors").field("count", &self.count()).finish()
    }
}

impl InternalErrors {
    pub fn global() -> &'static Self {
        static ERRORS: OnceLock<InternalErrors> = OnceLock::new();
        ERRORS.get_or_init(Self::default)
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_product(&self, product: String) {
        self.kept().product = product;
    }

    pub fn set_waker(&self, waker: Waker) {
        self.kept().waker = Some(waker);
    }

    pub fn report(&self, message: String, stack: String) {
        let waker = {
            let mut kept = self.kept();
            kept.next += 1;
            let id = kept.next;
            kept.errors.push(InternalError { id, message, stack, description: String::new() });
            let over = kept.errors.len().saturating_sub(KEPT);
            kept.errors.drain(..over);
            kept.waker.clone()
        };
        if let Some(waker) = waker {
            waker();
        }
    }

    pub fn count(&self) -> usize {
        self.kept().errors.len()
    }

    pub fn list(&self) -> Vec<InternalError> {
        self.kept().errors.clone()
    }

    pub fn describe(&self, id: u64, description: String) -> bool {
        let mut kept = self.kept();
        let found = kept.errors.iter_mut().find(|error| error.id == id);
        found.map(|error| error.description = description).is_some()
    }

    pub fn clear(&self) {
        self.kept().errors.clear();
    }

    pub fn for_report(&self, id: u64) -> Option<ErrorReport> {
        let kept = self.kept();
        let error = kept.errors.iter().find(|error| error.id == id)?;
        Some(ErrorReport { text: report_text(error, &kept.product, None), url: issue_url(error, &kept.product) })
    }
}

fn report_text(error: &InternalError, product: &str, stack_limit: Option<usize>) -> String {
    let stack: String = match stack_limit {
        Some(limit) => error.stack.chars().take(limit).collect(),
        None => error.stack.clone(),
    };
    let system = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let mut text = format!("{product} on {system}\n\n{}\n", error.message);
    if !error.description.is_empty() {
        text.push_str(&format!("\n{}\n", error.description));
    }
    text.push_str(&format!("\n```\n{stack}\n```\n"));
    text
}

fn issue_url(error: &InternalError, product: &str) -> String {
    let title: String = error.message.lines().next().unwrap_or_default().chars().take(120).collect();
    let body = report_text(error, product, Some(URL_STACK));
    format!("{ISSUES}?title={}&body={}", encoded(&title), encoded(&body))
}

fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn catch_panics() {
    static HOOKED: std::sync::Once = std::sync::Once::new();
    HOOKED.call_once(|| {
        let earlier = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            earlier(info);
            let what = info.payload_as_str().unwrap_or("panic");
            let thread = std::thread::current();
            let thread = thread.name().unwrap_or("unnamed");
            let place = info.location().map(ToString::to_string).unwrap_or_default();
            let stack = std::backtrace::Backtrace::force_capture().to_string();
            InternalErrors::global().report(format!("{what} (thread {thread}, {place})"), stack);
        }));
    });
}

fn errors(session: &Session) -> InternalErrors {
    session.state::<InternalErrors>().cloned().unwrap_or_default()
}

/// The internal errors kept since the IDE started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ErrorList {
    /// Oldest first.
    pub errors: Vec<InternalError>,
}

/// One internal error.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ErrorId {
    /// Its number, from `ide.errors.list`.
    pub id: u64,
}

/// What the person says about an internal error.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct DescribeParams {
    /// Its number, from `ide.errors.list`.
    pub id: u64,
    /// What the person was doing.
    pub description: String,
}

/// An internal error, ready to report. The IDE sends nothing itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorReport {
    /// The text to paste into a report.
    pub text: String,
    /// A prefilled address of a new issue, to open in the browser.
    pub url: String,
}

pub struct ListErrors;

impl Command for ListErrors {
    type Params = NoParams;
    type Output = ErrorList;
    const ID: &'static str = "ide.errors.list";
    const TITLE: &'static str = "List IDE Internal Errors";
    const DESCRIPTION: &'static str = "Lists the internal errors of the IDE since it started: each with its message, \
         stack and the description given to it. Nothing is sent anywhere.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ErrorList, CommandError> {
        Ok(ErrorList { errors: errors(session).list() })
    }
}

pub struct ClearErrors;

impl Command for ClearErrors {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "ide.errors.clear";
    const TITLE: &'static str = "Clear All IDE Internal Errors";
    const DESCRIPTION: &'static str = "Forgets every internal error, which hides the status bar's indicator.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        errors(session).clear();
        Ok(())
    }
}

pub struct DescribeError;

impl Command for DescribeError {
    type Params = DescribeParams;
    type Output = bool;
    const ID: &'static str = "ide.errors.describe";
    const TITLE: &'static str = "Describe IDE Internal Error";
    const DESCRIPTION: &'static str = "Sets what the person was doing when an internal error happened; it goes into \
         the report. False when there is no such error.";

    fn run(&self, session: &mut Session, params: DescribeParams) -> Result<bool, CommandError> {
        Ok(errors(session).describe(params.id, params.description))
    }
}

pub struct ReportError;

impl Command for ReportError {
    type Params = ErrorId;
    type Output = Option<ErrorReport>;
    const ID: &'static str = "ide.errors.report";
    const TITLE: &'static str = "Report IDE Internal Error";
    const DESCRIPTION: &'static str = "Answers an internal error's report text and a prefilled address of a new issue. \
         The IDE sends nothing itself: a person pastes the text or opens the address.";

    fn run(&self, session: &mut Session, params: ErrorId) -> Result<Option<ErrorReport>, CommandError> {
        Ok(errors(session).for_report(params.id))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListErrors)?;
    registry.register(ClearErrors)?;
    registry.register(DescribeError)?;
    registry.register(ReportError)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn session() -> (Registry, Session, InternalErrors) {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        let errors = InternalErrors::default();
        session.set_state(errors.clone());
        (registry, session, errors)
    }

    #[test]
    fn agents_read_describe_report_and_clear_the_errors() {
        let (registry, mut session, errors) = session();
        errors.set_product("GoGarden 26.11.2".to_owned());
        errors.report("index out of range (thread main, a.rs:1:2)".to_owned(), "0: main".to_owned());
        let listed = registry.dispatch(&mut session, "ide.errors.list", json!({})).unwrap();
        assert_eq!(listed["errors"][0]["id"], 1);
        assert_eq!(listed["errors"][0]["stack"], "0: main");
        let described = json!({ "id": 1, "description": "I pressed save & quit" });
        assert_eq!(registry.dispatch(&mut session, "ide.errors.describe", described).unwrap(), true);
        let report = registry.dispatch(&mut session, "ide.errors.report", json!({ "id": 1 })).unwrap();
        let text = report["text"].as_str().unwrap();
        assert!(text.contains("GoGarden 26.11.2") && text.contains("I pressed save & quit"), "{text}");
        assert!(text.contains("0: main"), "{text}");
        let url = report["url"].as_str().unwrap();
        assert!(url.starts_with(ISSUES) && url.contains("save%20%26%20quit"), "{url}");
        registry.dispatch(&mut session, "ide.errors.clear", json!({})).unwrap();
        assert_eq!(errors.count(), 0);
        assert_eq!(registry.dispatch(&mut session, "ide.errors.report", json!({ "id": 1 })).unwrap(), json!(null));
    }

    #[test]
    fn a_panic_on_a_worker_thread_is_kept_with_its_stack() {
        catch_panics();
        let worker = std::thread::Builder::new().name("zen-worker".to_owned());
        let failed = worker.spawn(|| panic!("the worker gave up")).unwrap().join();
        assert!(failed.is_err());
        let kept = InternalErrors::global().list();
        let error = kept.iter().find(|error| error.message.contains("the worker gave up")).unwrap();
        assert!(error.message.contains("thread zen-worker") && error.message.contains("internal_errors.rs"));
        assert!(!error.stack.is_empty());
    }

    #[test]
    fn only_the_latest_errors_are_kept() {
        let errors = InternalErrors::default();
        for number in 0..KEPT + 5 {
            errors.report(number.to_string(), String::new());
        }
        assert_eq!(errors.count(), KEPT);
        assert_eq!(errors.list()[0].message, "5");
    }

    #[test]
    fn a_new_error_wakes_the_window() {
        let errors = InternalErrors::default();
        let (woken, seen) = std::sync::mpsc::channel();
        let woken = Mutex::new(woken);
        errors.set_waker(Arc::new(move || {
            let _ = woken.lock().unwrap().send(());
        }));
        errors.report("boom".to_owned(), String::new());
        assert!(seen.try_recv().is_ok());
    }

    #[test]
    fn a_long_stack_stays_out_of_the_address() {
        let errors = InternalErrors::default();
        errors.report("boom".to_owned(), "x".repeat(URL_STACK * 4));
        let report = errors.for_report(1).unwrap();
        assert!(report.url.len() < URL_STACK * 2, "{}", report.url.len());
        assert!(report.text.len() > URL_STACK * 4, "the text keeps all of it");
    }
}

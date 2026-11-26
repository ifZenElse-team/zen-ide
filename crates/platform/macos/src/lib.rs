use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
mod appkit;
mod title_bar;

pub use title_bar::{TitleBarAction, system_title_bar_action, title_bar_action};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitAnswer {
    Terminate,
    Cancel,
}

pub struct QuitGate {
    allowed: AtomicBool,
    requested: AtomicBool,
    waker: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl QuitGate {
    pub const fn new() -> Self {
        QuitGate { allowed: AtomicBool::new(false), requested: AtomicBool::new(false), waker: OnceLock::new() }
    }

    pub fn set_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        let _ = self.waker.set(Box::new(waker));
    }

    pub fn system_asks(&self) -> QuitAnswer {
        if self.allowed.load(Ordering::SeqCst) {
            return QuitAnswer::Terminate;
        }
        self.requested.store(true, Ordering::SeqCst);
        if let Some(wake) = self.waker.get() {
            wake();
        }
        QuitAnswer::Cancel
    }

    pub fn take_request(&self) -> bool {
        self.requested.swap(false, Ordering::SeqCst)
    }

    pub fn allow(&self) {
        self.allowed.store(true, Ordering::SeqCst);
    }
}

impl Default for QuitGate {
    fn default() -> Self {
        QuitGate::new()
    }
}

pub fn quit_gate() -> &'static QuitGate {
    static GATE: QuitGate = QuitGate::new();
    &GATE
}

pub fn intercept_system_quit(wake: impl Fn() + Send + Sync + 'static) -> bool {
    quit_gate().set_waker(wake);
    #[cfg(target_os = "macos")]
    {
        static INSTALLED: OnceLock<bool> = OnceLock::new();
        if let Some(installed) = INSTALLED.get() {
            return *installed;
        }
        let Some(installed) = appkit::install() else { return false };
        *INSTALLED.get_or_init(|| installed)
    }
    #[cfg(not(target_os = "macos"))]
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[test]
    fn a_system_quit_is_cancelled_and_becomes_the_ides_request() {
        let gate = QuitGate::new();
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        gate.set_waker(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!gate.take_request(), "nothing asked yet");

        assert_eq!(gate.system_asks(), QuitAnswer::Cancel);
        assert_eq!(woken.load(Ordering::SeqCst), 1, "the IDE is woken to look");
        assert!(gate.take_request(), "the request waits for the IDE");
        assert!(!gate.take_request(), "and is taken once");

        assert_eq!(gate.system_asks(), QuitAnswer::Cancel);
        assert!(gate.take_request());
    }

    #[test]
    fn the_ides_own_exit_is_not_intercepted() {
        let gate = QuitGate::new();
        gate.allow();
        assert_eq!(gate.system_asks(), QuitAnswer::Terminate);
        assert!(!gate.take_request(), "no second exit request once the IDE exits");
    }
}

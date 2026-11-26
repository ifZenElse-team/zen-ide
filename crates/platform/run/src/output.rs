use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Waker;
use crate::console::{Console, ConsoleLine, Stream};
use crate::parser::OutputParser;
use crate::test_tree::{TestNode, TestTree};

/// Where a run is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    /// Its programs are running.
    Running,
    /// Its last program ended by itself.
    Finished,
    /// The user stopped it.
    Stopped,
    /// It could not be started or a program could not be run.
    Failed,
    /// A before-launch task failed, so its programs were not started.
    BeforeLaunchFailed,
}

pub struct RunOutput {
    console: Console,
    tree: TestTree,
    state: RunState,
    exit_code: Option<i32>,
    error: Option<String>,
    error_message: Option<zen_i18n::Message>,
    parser: Box<dyn OutputParser>,
    revision: u64,
    captured: Option<Captured>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured {
    pub text: String,
    pub limit: usize,
    pub cut: bool,
}

impl RunOutput {
    pub fn new(parser: Box<dyn OutputParser>) -> Self {
        Self {
            console: Console::default(),
            tree: TestTree::default(),
            state: RunState::Running,
            exit_code: None,
            error: None,
            error_message: None,
            parser,
            revision: 0,
            captured: None,
        }
    }

    pub fn restored(lines: Vec<ConsoleLine>, root: TestNode, state: RunState, exit_code: Option<i32>) -> Self {
        let mut console = Console::default();
        for line in lines {
            console.push(line);
        }
        Self {
            console,
            tree: TestTree::restored(root),
            state,
            exit_code,
            error: None,
            error_message: None,
            parser: Box::new(crate::parser::PlainParser),
            revision: 1,
            captured: None,
        }
    }

    pub fn with_console_limit(mut self, bytes: usize) -> Self {
        self.console.set_limit(bytes);
        self
    }

    pub fn capture_stdout(&mut self, limit: usize) {
        self.captured = Some(Captured { limit, ..Captured::default() });
    }

    pub fn captured(&self) -> Option<&Captured> {
        self.captured.as_ref()
    }

    pub fn feed(&mut self, stream: Stream, text: &str) {
        if stream == Stream::Stdout
            && let Some(captured) = self.captured.as_mut()
            && !captured.cut
        {
            if captured.text.len() + text.len() < captured.limit {
                captured.text.push_str(text);
                captured.text.push('\n');
            } else {
                captured.cut = true;
            }
        }
        let parsed = self.parser.line(stream, text);
        let line = parsed.console.map(|line| self.console.push(line));
        for event in &parsed.events {
            self.tree.apply(event, line);
        }
        self.revision += 1;
    }

    pub fn system(&mut self, text: &str) -> usize {
        self.revision += 1;
        self.console.push(ConsoleLine::new(Stream::System, text))
    }

    pub fn end(&mut self, state: RunState, exit_code: Option<i32>) {
        if self.state != RunState::Running {
            return;
        }
        for event in self.parser.finish() {
            self.tree.apply(&event, None);
        }
        self.tree.finish();
        self.state = state;
        self.exit_code = exit_code;
        self.revision += 1;
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.state == RunState::Running {
            self.error = Some(message);
            self.end(RunState::Failed, None);
        }
    }

    pub fn fail_before_launch_worded(&mut self, message: zen_i18n::Message, exit_code: Option<i32>) {
        if self.state == RunState::Running {
            self.error_message = Some(message.clone());
        }
        self.fail_before_launch(message.text(), exit_code);
    }

    pub fn fail_before_launch(&mut self, message: impl Into<String>, exit_code: Option<i32>) {
        if self.state == RunState::Running {
            self.error = Some(message.into());
            self.end(RunState::BeforeLaunchFailed, exit_code);
        }
    }

    pub fn console(&self) -> &Console {
        &self.console
    }

    pub fn tree(&self) -> &TestTree {
        &self.tree
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    pub fn is_running(&self) -> bool {
        self.state == RunState::Running
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn error_worded(&self) -> Option<String> {
        self.error_message.as_ref().map(zen_i18n::Message::text).or_else(|| self.error.clone())
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Clone)]
pub struct SharedOutput {
    inner: Arc<(Mutex<RunOutput>, Condvar)>,
    waker: Option<Waker>,
}

impl SharedOutput {
    pub fn new(output: RunOutput, waker: Option<Waker>) -> Self {
        Self { inner: Arc::new((Mutex::new(output), Condvar::new())), waker }
    }

    fn lock(&self) -> MutexGuard<'_, RunOutput> {
        self.inner.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn read<R>(&self, read: impl FnOnce(&RunOutput) -> R) -> R {
        read(&self.lock())
    }

    pub fn update<R>(&self, update: impl FnOnce(&mut RunOutput) -> R) -> R {
        let result = update(&mut self.lock());
        self.inner.1.notify_all();
        if let Some(waker) = &self.waker {
            waker();
        }
        result
    }

    pub fn wait(&self, from: usize, timeout: Duration) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let guard = self.lock();
            let _ = self
                .inner
                .1
                .wait_timeout_while(guard, timeout, |output| output.is_running() && output.console().len() <= from);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (from, timeout);
    }

    pub fn wait_end(&self, timeout: Duration) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let guard = self.lock();
            let (guard, _) = self
                .inner
                .1
                .wait_timeout_while(guard, timeout, |output| output.is_running())
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            !guard.is_running()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = timeout;
            !self.read(RunOutput::is_running)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::PlainParser;

    #[test]
    fn a_run_ends_once() {
        let mut output = RunOutput::new(Box::new(PlainParser));
        output.feed(Stream::Stdout, "hello");
        output.system("Process finished with exit code 0");
        output.end(RunState::Finished, Some(0));
        output.end(RunState::Stopped, None);
        output.fail("too late");
        assert_eq!(output.state(), RunState::Finished);
        assert_eq!(output.exit_code(), Some(0));
        assert_eq!(output.error(), None);
        assert_eq!(output.console().len(), 2);
        assert!(output.tree().is_done());
    }

    #[test]
    fn a_captured_standard_output_keeps_whole_lines_from_the_start_up_to_its_limit() {
        let mut output = RunOutput::new(Box::new(PlainParser)).with_console_limit(100);
        output.capture_stdout(25);
        for line in ["kind: A", "kind: B", "kind: C", "kind: D"] {
            output.feed(Stream::Stdout, line);
            output.feed(Stream::Stderr, "warning");
        }
        let captured = output.captured().unwrap();
        assert_eq!(captured.text, "kind: A\nkind: B\nkind: C\n");
        assert!(captured.cut);
        assert_eq!(RunOutput::new(Box::new(PlainParser)).captured(), None);
    }

    #[test]
    fn a_long_run_keeps_only_the_console_buffer() {
        let mut output = RunOutput::new(Box::new(PlainParser)).with_console_limit(1000);
        for index in 0..10_000 {
            output.feed(Stream::Stdout, &format!("line {index}"));
        }
        assert_eq!(output.console().len(), 10_000);
        assert!(output.console().lines().len() < 200, "{}", output.console().lines().len());
        assert_eq!(output.console().get(9_999).map(|line| line.text.as_str()), Some("line 9999"));
    }

    #[test]
    fn waiting_returns_when_lines_arrive() {
        let shared = SharedOutput::new(RunOutput::new(Box::new(PlainParser)), None);
        let feeder = shared.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            feeder.update(|output| output.feed(Stream::Stdout, "line"));
        });
        shared.wait(0, Duration::from_secs(10));
        assert_eq!(shared.read(|output| output.console().len()), 1);
        thread.join().unwrap();
        shared.update(|output| output.fail("no program"));
        shared.wait(1, Duration::from_secs(10));
        assert_eq!(shared.read(|output| output.error().map(str::to_owned)), Some("no program".to_owned()));
    }

    #[test]
    fn waiting_for_the_end_returns_when_the_run_ends_or_the_time_is_up() {
        let shared = SharedOutput::new(RunOutput::new(Box::new(PlainParser)), None);
        assert!(!shared.wait_end(Duration::from_millis(10)), "still running");
        let ender = shared.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            ender.update(|output| output.end(RunState::Finished, Some(0)));
        });
        assert!(shared.wait_end(Duration::from_secs(10)));
        thread.join().unwrap();
        assert!(shared.wait_end(Duration::ZERO), "an ended run answers at once");
    }
}

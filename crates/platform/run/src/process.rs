use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::console::Stream;
use crate::{CommandLine, ProcessEvent, ProcessSink};

pub const STOP_GRACE: Duration = Duration::from_secs(2);

const POLL: Duration = Duration::from_millis(50);

const SETTLE: Duration = Duration::from_millis(10);

struct Shared {
    exited: Mutex<bool>,
    changed: Condvar,
    stopping: AtomicBool,
    ended: AtomicBool,
    group_gone: AtomicBool,
}

impl Shared {
    fn may_signal(&self, pid: u32, alive: impl Fn(u32) -> bool) -> bool {
        if self.group_gone.load(Ordering::SeqCst) {
            return false;
        }
        if self.has_exited() && !alive(pid) {
            self.group_gone.store(true, Ordering::SeqCst);
            return false;
        }
        true
    }

    fn has_exited(&self) -> bool {
        *self.exited.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wait_exit(&self, timeout: Duration) -> bool {
        let guard = self.exited.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |exited| !*exited)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard
    }
}

pub struct Process {
    pid: u32,
    shared: Arc<Shared>,
}

impl Process {
    pub fn spawn(line: &CommandLine, sink: ProcessSink) -> Result<Process, String> {
        let mut command = Command::new(&line.program);
        command.args(&line.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if !line.cwd.as_os_str().is_empty() {
            command.current_dir(&line.cwd);
        }
        zen_env::apply(&mut command);
        for (name, value) in &line.env {
            command.env(name, value);
        }
        own_group(&mut command);
        let mut child = command.spawn().map_err(|error| format!("{}: {error}", line.program.display()))?;
        let pid = child.id();
        let shared = Arc::new(Shared {
            exited: Mutex::new(false),
            changed: Condvar::new(),
            stopping: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            group_gone: AtomicBool::new(false),
        });
        let readers: Vec<JoinHandle<()>> = [
            child.stdout.take().and_then(|stdout| read_lines(stdout, Stream::Stdout, sink.clone(), shared.clone())),
            child.stderr.take().and_then(|stderr| read_lines(stderr, Stream::Stderr, sink.clone(), shared.clone())),
        ]
        .into_iter()
        .flatten()
        .collect();
        let waiting = shared.clone();
        std::thread::Builder::new()
            .name("zen-run-wait".to_owned())
            .spawn(move || wait(pid, child, readers, &waiting, &sink))
            .map_err(|error| error.to_string())?;
        Ok(Process { pid, shared })
    }

    pub fn stop(&self) {
        self.stop_within(STOP_GRACE);
    }

    pub fn stop_within(&self, grace: Duration) {
        if !self.shared.may_signal(self.pid, group_alive) {
            return;
        }
        if self.shared.stopping.swap(true, Ordering::SeqCst) {
            self.kill();
            return;
        }
        interrupt(self.pid);
        let shared = self.shared.clone();
        let pid = self.pid;
        let _ = std::thread::Builder::new().name("zen-run-stop".to_owned()).spawn(move || {
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                if shared.wait_exit(POLL) && !group_alive(pid) {
                    return;
                }
                if shared.has_exited() {
                    std::thread::sleep(POLL);
                }
            }
            if shared.may_signal(pid, group_alive) {
                terminate(pid);
            }
        });
    }

    pub fn kill(&self) {
        if self.shared.may_signal(self.pid, group_alive) {
            terminate(self.pid);
        }
    }

    pub fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }

    pub fn has_exited(&self) -> bool {
        self.shared.has_exited()
    }
}

#[derive(Clone, Default)]
pub struct Programs {
    state: Arc<Mutex<ProgramsState>>,
}

#[derive(Default)]
struct ProgramsState {
    pids: Vec<u32>,
    stopping: bool,
}

impl Programs {
    fn state(&self) -> std::sync::MutexGuard<'_, ProgramsState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn spawn(&self, command: &mut Command) -> std::io::Result<Child> {
        own_group(command);
        let mut state = self.state();
        if state.stopping {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "stopped"));
        }
        let child = command.spawn()?;
        state.pids.push(child.id());
        Ok(child)
    }

    pub fn stopping(&self) -> bool {
        self.state().stopping
    }

    pub fn add(&self, pid: u32) {
        let mut state = self.state();
        state.pids.push(pid);
        if state.stopping {
            interrupt(pid);
        }
    }

    pub fn remove(&self, pid: u32) {
        let mut state = self.state();
        if !group_alive(pid) {
            state.pids.retain(|known| *known != pid);
        }
    }
}

impl zen_project::inspect::Programs for Programs {
    fn interrupt(&self) {
        let mut state = self.state();
        state.stopping = true;
        for &pid in &state.pids {
            interrupt(pid);
        }
    }

    fn kill(&self) {
        let mut state = self.state();
        state.stopping = true;
        for &pid in &state.pids {
            terminate(pid);
        }
    }

    fn running(&self) -> bool {
        let mut state = self.state();
        if cfg!(unix) {
            state.pids.retain(|pid| group_alive(*pid));
        }
        !state.pids.is_empty()
    }
}

#[cfg(unix)]
fn read_lines(
    stream: impl Read + std::os::fd::AsFd + Send + 'static,
    kind: Stream,
    sink: ProcessSink,
    shared: Arc<Shared>,
) -> Option<JoinHandle<()>> {
    use rustix::event::{PollFd, PollFlags, Timespec};

    std::thread::Builder::new()
        .name("zen-run-read".to_owned())
        .spawn(move || {
            let mut stream = stream;
            let mut pending = LineSplitter::default();
            let mut chunk = [0_u8; 8192];
            loop {
                let ended = shared.ended.load(Ordering::SeqCst);
                let wait = if ended { Duration::ZERO } else { POLL };
                let timeout = Timespec { tv_sec: 0, tv_nsec: i64::try_from(wait.as_nanos()).unwrap_or(0) };
                let mut fds = [PollFd::new(&stream, PollFlags::IN)];
                match rustix::event::poll(&mut fds, Some(&timeout)) {
                    Ok(0) if ended => break,
                    Ok(0) | Err(rustix::io::Errno::INTR) => continue,
                    Ok(_) => {}
                    Err(_) => break,
                }
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => pending.push(&chunk[..read], |line| send_line(&sink, kind, line)),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            pending.finish(|line| send_line(&sink, kind, line));
        })
        .ok()
}

#[cfg(not(unix))]
fn read_lines(
    stream: impl Read + Send + 'static,
    kind: Stream,
    sink: ProcessSink,
    _shared: Arc<Shared>,
) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("zen-run-read".to_owned())
        .spawn(move || {
            let mut stream = stream;
            let mut pending = LineSplitter::default();
            let mut chunk = [0_u8; 8192];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => pending.push(&chunk[..read], |line| send_line(&sink, kind, line)),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            pending.finish(|line| send_line(&sink, kind, line));
        })
        .ok()
}

const MAX_LINE_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct LineSplitter {
    pending: Vec<u8>,
    scanned: usize,
}

impl LineSplitter {
    fn push(&mut self, bytes: &[u8], mut line: impl FnMut(&[u8])) {
        self.pending.extend_from_slice(bytes);
        let mut start = 0;
        loop {
            let from = start + self.scanned;
            match self.pending[from..].iter().position(|byte| *byte == b'\n') {
                Some(offset) => {
                    let end = from + offset;
                    let text = &self.pending[start..end];
                    line(text.strip_suffix(b"\r").unwrap_or(text));
                    start = end + 1;
                    self.scanned = 0;
                }
                None if self.pending.len() - start >= MAX_LINE_BYTES => {
                    let end = char_boundary(&self.pending[start..], MAX_LINE_BYTES) + start;
                    line(&self.pending[start..end]);
                    start = end;
                    self.scanned = 0;
                }
                None => {
                    self.scanned = self.pending.len() - start;
                    break;
                }
            }
        }
        self.pending.drain(..start);
    }

    fn finish(&mut self, mut line: impl FnMut(&[u8])) {
        if !self.pending.is_empty() {
            line(&self.pending);
        }
        self.pending.clear();
        self.scanned = 0;
    }
}

fn char_boundary(bytes: &[u8], at: usize) -> usize {
    let mut end = at.min(bytes.len());
    while end > at.saturating_sub(3) && end < bytes.len() && bytes[end] & 0b1100_0000 == 0b1000_0000 {
        end -= 1;
    }
    if end == 0 { at.min(bytes.len()) } else { end }
}

fn send_line(sink: &ProcessSink, kind: Stream, bytes: &[u8]) {
    sink(ProcessEvent::Output { stream: kind, text: String::from_utf8_lossy(bytes).into_owned() });
}

fn wait(pid: u32, mut child: Child, readers: Vec<JoinHandle<()>>, shared: &Shared, sink: &ProcessSink) {
    let code = child.wait().ok().and_then(|status| status.code());
    while readers.iter().any(|reader| !reader.is_finished()) && group_alive(pid) {
        std::thread::sleep(SETTLE);
    }
    shared.ended.store(true, Ordering::SeqCst);
    for reader in readers {
        let _ = reader.join();
    }
    *shared.exited.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    shared.changed.notify_all();
    let gone = !group_alive(pid);
    if gone {
        shared.group_gone.store(true, Ordering::SeqCst);
    }
    sink(ProcessEvent::Exited { code });
    if !gone {
        while group_alive(pid) {
            std::thread::sleep(POLL);
        }
        shared.group_gone.store(true, Ordering::SeqCst);
    }
}

#[cfg(unix)]
pub fn own_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
pub fn own_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
pub fn own_group(_command: &mut Command) {}

pub fn kill_group(pid: u32) {
    terminate(pid);
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: rustix::process::Signal) -> bool {
    match i32::try_from(pid).ok().filter(|pid| *pid > 1).and_then(rustix::process::Pid::from_raw) {
        Some(group) => rustix::process::kill_process_group(group, signal).is_ok(),
        None => false,
    }
}

#[cfg(unix)]
fn interrupt(pid: u32) {
    signal_group(pid, rustix::process::Signal::INT);
}

#[cfg(unix)]
fn terminate(pid: u32) {
    signal_group(pid, rustix::process::Signal::KILL);
}

#[cfg(unix)]
fn group_alive(pid: u32) -> bool {
    match i32::try_from(pid).ok().filter(|pid| *pid > 1).and_then(rustix::process::Pid::from_raw) {
        Some(group) => rustix::process::test_kill_process_group(group).is_ok(),
        None => false,
    }
}

#[cfg(windows)]
fn taskkill(pid: u32, force: bool) {
    let mut command = Command::new("taskkill");
    command.arg("/T");
    if force {
        command.arg("/F");
    }
    let _ = command
        .args(["/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn interrupt(pid: u32) {
    taskkill(pid, false);
}

#[cfg(windows)]
fn terminate(pid: u32) {
    taskkill(pid, true);
}

#[cfg(windows)]
fn group_alive(_pid: u32) -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
fn interrupt(_pid: u32) {}

#[cfg(not(any(unix, windows)))]
fn terminate(_pid: u32) {}

#[cfg(not(any(unix, windows)))]
fn group_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod splitter_tests {
    use super::*;

    fn split(chunks: &[&[u8]]) -> Vec<String> {
        let mut splitter = LineSplitter::default();
        let mut lines = Vec::new();
        for chunk in chunks {
            splitter.push(chunk, |line| lines.push(String::from_utf8_lossy(line).into_owned()));
        }
        splitter.finish(|line| lines.push(String::from_utf8_lossy(line).into_owned()));
        lines
    }

    #[test]
    fn lines_are_split_across_chunks() {
        assert_eq!(split(&[b"one\r\ntw", b"o\n", b"\nlast"]), ["one", "two", "", "last"]);
    }

    #[test]
    fn an_endless_line_is_cut_into_bounded_pieces_on_character_boundaries() {
        let text = "\u{e9}".repeat(MAX_LINE_BYTES);
        let chunks: Vec<&[u8]> = text.as_bytes().chunks(8192).collect();
        let lines = split(&chunks);
        assert!(lines.iter().all(|line| line.len() <= MAX_LINE_BYTES && !line.contains('\u{fffd}')));
        assert_eq!(lines.concat(), text);
        let mut splitter = LineSplitter::default();
        for _ in 0..1000 {
            splitter.push(&[b'x'; 8192], |_| {});
        }
        assert!(splitter.pending.len() < MAX_LINE_BYTES, "{}", splitter.pending.len());
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    fn shell(script: &str) -> CommandLine {
        CommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), script.to_owned()],
            display: script.to_owned(),
            ..CommandLine::default()
        }
    }

    fn start(line: &CommandLine) -> (Process, mpsc::Receiver<ProcessEvent>) {
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        let sink: ProcessSink = Arc::new(move |event| {
            let _ = sender.lock().unwrap().send(event);
        });
        (Process::spawn(line, sink).unwrap(), receiver)
    }

    fn events(receiver: &mpsc::Receiver<ProcessEvent>) -> (Vec<(Stream, String)>, Option<i32>) {
        let mut lines = Vec::new();
        loop {
            match receiver.recv_timeout(Duration::from_secs(20)).unwrap() {
                ProcessEvent::Output { stream, text } => lines.push((stream, text)),
                ProcessEvent::Exited { code } => return (lines, code),
            }
        }
    }

    fn alive(pid: &str) -> bool {
        Command::new("/bin/sh").args(["-c", &format!("kill -0 {pid} 2>/dev/null")]).status().unwrap().success()
    }

    fn read_pid(path: &std::path::Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(text) = std::fs::read_to_string(path)
                && text.ends_with('\n')
            {
                return text.trim().to_owned();
            }
            assert!(Instant::now() < deadline, "the pid was not written");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn exit_codes_are_reported() {
        let (process, receiver) = start(&shell("exit 3"));
        assert_eq!(events(&receiver), (Vec::new(), Some(3)));
        assert!(process.has_exited());
        assert!(process.pid().is_some());
        let (_process, receiver) = start(&shell("true"));
        assert_eq!(events(&receiver).1, Some(0));
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let line = CommandLine { program: PathBuf::from("/no/such/program"), ..CommandLine::default() };
        assert!(Process::spawn(&line, Arc::new(|_| {})).is_err());
    }

    #[test]
    fn the_environment_and_the_folder_are_the_lines() {
        let dir = tempfile::tempdir().unwrap();
        let mut line = shell("echo \"$ZEN_RUN_A-$ZEN_RUN_B\"; pwd -P; test -n \"$PATH\" && echo path");
        line.cwd = dir.path().to_owned();
        line.env = vec![("ZEN_RUN_A".to_owned(), "one".to_owned()), ("ZEN_RUN_B".to_owned(), "two".to_owned())];
        let (_process, receiver) = start(&line);
        let (lines, code) = events(&receiver);
        let texts: Vec<&str> = lines.iter().map(|(_, text)| text.as_str()).collect();
        let folder = dir.path().canonicalize().unwrap();
        assert_eq!(texts, ["one-two", folder.to_str().unwrap(), "path"]);
        assert_eq!(code, Some(0));
    }

    #[test]
    fn stderr_is_tagged_and_lines_come_in_order() {
        let script = "echo out1; sleep 0.1; echo err1 >&2; sleep 0.1; echo out2; sleep 0.1; printf 'a\\r\\nlast'";
        let (_process, receiver) = start(&shell(script));
        let (lines, code) = events(&receiver);
        let expected = [
            (Stream::Stdout, "out1"),
            (Stream::Stderr, "err1"),
            (Stream::Stdout, "out2"),
            (Stream::Stdout, "a"),
            (Stream::Stdout, "last"),
        ];
        let lines: Vec<(Stream, &str)> = lines.iter().map(|(stream, text)| (*stream, text.as_str())).collect();
        assert_eq!(lines, expected);
        assert_eq!(code, Some(0));
    }

    #[test]
    fn stopping_ends_the_whole_group() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pid");
        let script = format!("sleep 60 & echo $! > '{}'; wait", file.display());
        let (process, receiver) = start(&shell(&script));
        let grandchild = read_pid(&file);
        assert!(alive(&grandchild));
        process.stop();
        let (_, code) = events(&receiver);
        assert_ne!(code, Some(0), "the stop ended it");
        let deadline = Instant::now() + Duration::from_secs(60);
        while alive(&grandchild) {
            assert!(Instant::now() < deadline, "the grandchild still runs");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(process.has_exited());
        process.stop();
    }

    #[test]
    fn a_program_that_ignores_the_interrupt_is_killed_after_the_grace() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pid");
        let script = format!("trap '' INT; echo $$ > '{}'; while :; do sleep 0.05; done", file.display());
        let (process, receiver) = start(&shell(&script));
        read_pid(&file);
        let stopped = Instant::now();
        process.stop();
        let (_, code) = events(&receiver);
        assert_eq!(code, None);
        assert!(stopped.elapsed() >= STOP_GRACE - Duration::from_millis(100), "{:?}", stopped.elapsed());
    }

    #[test]
    fn the_exit_does_not_wait_for_a_process_outside_the_group_that_holds_the_output() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pid");
        let script =
            format!("perl -e 'setpgrp(0, 0); sleep 600' & echo $! > '{}'; sleep 0.2; echo done", file.display());
        let started = Instant::now();
        let (_process, receiver) = start(&shell(&script));
        let outsider = read_pid(&file);
        let (lines, code) = events(&receiver);
        let elapsed = started.elapsed();
        let _ = Command::new("/bin/kill").args(["-9", &outsider]).status();
        assert_eq!(lines, [(Stream::Stdout, "done".to_owned())]);
        assert_eq!(code, Some(0));
        assert!(elapsed < Duration::from_secs(120), "{elapsed:?}");
    }

    #[test]
    fn a_finished_run_never_signals_its_group_id_again() {
        let (process, receiver) = start(&shell("true"));
        assert_eq!(events(&receiver).1, Some(0));
        assert!(!process.shared.may_signal(process.pid, |_| true), "a reused group id is left alone");
        let leftover = Shared {
            exited: Mutex::new(true),
            changed: Condvar::new(),
            stopping: AtomicBool::new(false),
            ended: AtomicBool::new(true),
            group_gone: AtomicBool::new(false),
        };
        assert!(leftover.may_signal(1234, |_| true));
        assert!(!leftover.may_signal(1234, |_| false));
        assert!(!leftover.may_signal(1234, |_| true), "once gone, a group that shows up again is someone else's");
    }

    #[test]
    fn a_second_stop_kills_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pid");
        let script = format!("trap '' INT; echo $$ > '{}'; while :; do sleep 0.05; done", file.display());
        let (process, receiver) = start(&shell(&script));
        read_pid(&file);
        let stopped = Instant::now();
        process.stop_within(Duration::from_secs(600));
        process.stop();
        assert_eq!(events(&receiver).1, None);
        assert!(stopped.elapsed() < Duration::from_secs(60), "{:?}", stopped.elapsed());
    }
}

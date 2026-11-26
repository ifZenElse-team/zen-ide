use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Config;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, Pty};
use alacritty_terminal::vte::ansi::Processor;

use super::protocol::{Frame, Message, OpenRequest, SessionInfo, Speaks, read_frame, write_frame};
use super::record::{self, Record};
use super::{private_folder, socket_path};
use crate::session::Dims;
use crate::{Options, Size};

const POLL: Duration = Duration::from_millis(100);
const EXIT_WAIT: Duration = Duration::from_secs(2);

const MAX_SCROLLBACK: usize = 100_000;

const SAVE_EVERY: Duration = Duration::from_secs(2);

struct State {
    dir: PathBuf,
    speaks: Speaks,
    instance: u64,
    sessions: Mutex<HashMap<u64, Arc<HostSession>>>,
    clients: AtomicUsize,
    last_seen: Mutex<Instant>,
    next_session: AtomicU64,
    next_client: AtomicU64,
}

struct HostSession {
    id: u64,
    pid: u32,
    cwd: PathBuf,
    pty: Mutex<Pty>,
    input: Mutex<File>,
    screen: Mutex<HostScreen>,
    attached: Mutex<Option<(u64, Arc<Mutex<UnixStream>>)>>,
    env: Vec<(String, String)>,
    keep: AtomicBool,
    dirty: AtomicBool,
    typed: Mutex<Typed>,
}

#[derive(Default)]
struct Typed {
    start: Option<(usize, usize)>,
    command: Option<String>,
    running: Option<(u32, Vec<String>)>,
}

struct HostScreen {
    term: Term<Titles>,
    parser: Processor,
    title: Arc<Mutex<Option<String>>>,
}

#[derive(Clone)]
struct Titles(Arc<Mutex<Option<String>>>);

impl EventListener for Titles {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => *lock(&self.0) = Some(title),
            Event::ResetTitle => *lock(&self.0) = None,
            _ => {}
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn serve(dir: &Path, idle: Duration) -> io::Result<()> {
    let _ = rustix::process::setsid();
    serve_with(dir, idle, Speaks::CURRENT)
}

pub fn serve_with(dir: &Path, idle: Duration, speaks: Speaks) -> io::Result<()> {
    let protocol = speaks.protocol;
    private_folder(dir)?;
    let lock_file = File::create(dir.join(format!("host-{protocol}.lock")))?;
    if rustix::fs::flock(&lock_file, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err() {
        return Ok(());
    }
    let socket = socket_path(dir, protocol);
    if let Some(parent) = socket.parent() {
        private_folder(parent)?;
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let instance = instance();
    let program = std::env::current_exe().ok();
    let info = serde_json::json!({
        "socket": socket,
        "pid": std::process::id(),
        "protocol": protocol,
        "instance": instance,
        "program": program,
    });
    std::fs::write(dir.join(format!("host-{protocol}.json")), info.to_string())?;
    listener.set_nonblocking(true)?;
    let state = Arc::new(State {
        dir: dir.to_owned(),
        speaks,
        instance,
        sessions: Mutex::new(HashMap::new()),
        clients: AtomicUsize::new(0),
        last_seen: Mutex::new(Instant::now()),
        next_session: AtomicU64::new(1),
        next_client: AtomicU64::new(1),
    });
    let mut saved = Instant::now();
    loop {
        if saved.elapsed() >= SAVE_EVERY {
            save_records(&state);
            saved = Instant::now();
        }
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                state.clients.fetch_add(1, Ordering::AcqRel);
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let _ = client(&state, stream);
                    *lock(&state.last_seen) = Instant::now();
                    state.clients.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let unused = state.clients.load(Ordering::Acquire) == 0 && lock(&state.sessions).is_empty();
                if unused && lock(&state.last_seen).elapsed() >= idle {
                    let _ = std::fs::remove_file(&socket);
                    let _ = std::fs::remove_file(dir.join(format!("host-{protocol}.json")));
                    return Ok(());
                }
                std::thread::sleep(POLL);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn client(state: &Arc<State>, mut stream: UnixStream) -> io::Result<()> {
    match read_frame(&mut stream)? {
        Frame::Control(Message::Hello { protocol, oldest, .. }) => {
            let Speaks { protocol: mine, oldest: my_oldest } = state.speaks;
            let hello = Message::Hello { protocol: mine, oldest: my_oldest, instance: Some(state.instance) };
            write_frame(&mut stream, &Frame::Control(hello))?;
            if state.speaks.meets(Speaks { protocol, oldest }).is_none() {
                return Ok(());
            }
        }
        _ => return Ok(()),
    }
    let session = match read_frame(&mut stream)? {
        Frame::Control(Message::List) => {
            let sessions = lock(&state.sessions).values().map(|session| session.info()).collect();
            return write_frame(&mut stream, &Frame::Control(Message::Sessions { sessions }));
        }
        Frame::Control(Message::Open(request)) => match open(state, request) {
            Ok(session) => session,
            Err(error) => return refuse(&mut stream, &error.to_string()),
        },
        Frame::Control(Message::Attach { id }) => match lock(&state.sessions).get(&id).cloned() {
            Some(session) => session,
            None => return refuse(&mut stream, &format!("no terminal session {id}")),
        },
        _ => return Ok(()),
    };
    let me = state.next_client.fetch_add(1, Ordering::Relaxed);
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    {
        let mut screen = lock(&session.screen);
        let screen = &mut *screen;
        let title = lock(&screen.title).clone();
        let snapshot = crate::replay::snapshot(&mut screen.term, title.as_deref());
        let (rows, cols) = (screen.term.screen_lines() as u16, screen.term.columns() as u16);
        let mut out = lock(&writer);
        write_frame(
            &mut *out,
            &Frame::Control(Message::Opened {
                id: session.id,
                rows: Some(rows),
                cols: Some(cols),
                snapshot: Some(snapshot.len() as u64),
            }),
        )?;
        super::protocol::write_data(&mut *out, &snapshot)?;
        drop(out);
        if let Some((_, previous)) = lock(&session.attached).replace((me, Arc::clone(&writer))) {
            let _ = lock(&previous).shutdown(std::net::Shutdown::Both);
        }
    }
    loop {
        match read_frame(&mut stream) {
            Ok(Frame::Data(bytes)) => {
                session.track_input(&bytes);
                let _ = lock(&session.input).write_all(&bytes);
            }
            Ok(Frame::Control(Message::Keep { keep })) => {
                session.keep.store(keep, Ordering::Release);
                if keep {
                    session.dirty.store(true, Ordering::Release);
                } else {
                    record::remove(&state.dir, state.instance, session.id);
                }
            }
            Ok(Frame::Control(Message::Resize { rows, cols })) => {
                let size = Size::new(rows, cols).clamped();
                lock(&session.screen).term.resize(Dims(size));
                lock(&session.pty).on_resize(window_size(size.rows, size.cols));
            }
            Ok(Frame::Control(Message::Close)) => {
                session.hang_up();
            }
            Ok(Frame::Control(_)) => {}
            Err(_) => break,
        }
    }
    let mut attached = lock(&session.attached);
    if attached.as_ref().is_some_and(|(owner, _)| *owner == me) {
        *attached = None;
    }
    Ok(())
}

fn row_blank(term: &Term<Titles>, line: Line) -> bool {
    let row = &term.grid()[line];
    (0..term.columns()).all(|col| row[Column(col)].c == ' ')
}

fn command_of(pid: u32) -> Option<Vec<String>> {
    if cfg!(target_os = "linux") {
        let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let args: Vec<String> = bytes
            .split(|byte| *byte == 0)
            .filter(|arg| !arg.is_empty())
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect();
        return (!args.is_empty()).then_some(args);
    }
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!line.is_empty()).then(|| line.split_whitespace().map(str::to_owned).collect())
}

fn instance() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    ((nanos as u64) ^ (u64::from(std::process::id()) << 40)).max(1)
}

fn refuse(stream: &mut UnixStream, message: &str) -> io::Result<()> {
    write_frame(stream, &Frame::Control(Message::Error { message: message.to_owned() }))
}

fn window_size(rows: u16, cols: u16) -> WindowSize {
    WindowSize { num_lines: rows.max(1), num_cols: cols.max(2), cell_width: 0, cell_height: 0 }
}

fn open(state: &Arc<State>, request: OpenRequest) -> io::Result<Arc<HostSession>> {
    let options = tty::Options {
        shell: Some(tty::Shell::new(request.program.to_string_lossy().into_owned(), request.args)),
        working_directory: Some(request.cwd.clone()),
        drain_on_exit: true,
        env: request.env.iter().cloned().collect(),
    };
    let size = Size::new(request.rows, request.cols).clamped();
    let pty = tty::new(&options, window_size(size.rows, size.cols), 0)?;
    let title = Arc::new(Mutex::new(None));
    let config = Config {
        scrolling_history: request.scrollback.unwrap_or(Options::DEFAULT_SCROLLBACK).min(MAX_SCROLLBACK),
        ..Config::default()
    };
    let mut term = Term::new(config, &Dims(size), Titles(Arc::clone(&title)));
    let mut parser = Processor::new();
    if let Some(preface) = request.preface.as_deref() {
        parser.advance(&mut term, preface.as_bytes());
    }
    let output = pty.file().try_clone()?;
    let flags = rustix::fs::fcntl_getfl(&output)?;
    rustix::fs::fcntl_setfl(&output, flags - rustix::fs::OFlags::NONBLOCK)?;
    let input = pty.file().try_clone()?;
    let session = Arc::new(HostSession {
        id: state.next_session.fetch_add(1, Ordering::Relaxed),
        pid: pty.child().id(),
        cwd: request.cwd,
        pty: Mutex::new(pty),
        input: Mutex::new(input),
        screen: Mutex::new(HostScreen { term, parser, title }),
        attached: Mutex::new(None),
        env: request.env.clone(),
        keep: AtomicBool::new(request.keep),
        dirty: AtomicBool::new(request.keep),
        typed: Mutex::new(Typed::default()),
    });
    lock(&state.sessions).insert(session.id, Arc::clone(&session));
    let state = Arc::clone(state);
    let reader = Arc::clone(&session);
    std::thread::spawn(move || pump(&state, &reader, output));
    Ok(session)
}

fn pump(state: &State, session: &Arc<HostSession>, mut output: File) {
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match output.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let mut screen = lock(&session.screen);
                let screen = &mut *screen;
                screen.parser.advance(&mut screen.term, &buffer[..read]);
                session.dirty.store(true, Ordering::Release);
                session.send(&Frame::Data(buffer[..read].to_vec()));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    let code = session.wait();
    lock(&state.sessions).remove(&session.id);
    record::remove(&state.dir, state.instance, session.id);
    *lock(&state.last_seen) = Instant::now();
    session.send(&Frame::Control(Message::Exited { code }));
    if let Some((_, writer)) = lock(&session.attached).take() {
        let _ = lock(&writer).shutdown(std::net::Shutdown::Both);
    }
}

fn save_records(state: &State) {
    let sessions: Vec<Arc<HostSession>> = lock(&state.sessions).values().cloned().collect();
    for session in sessions {
        if session.keep.load(Ordering::Acquire) && session.dirty.swap(false, Ordering::AcqRel) {
            let record = session.record();
            if !lock(&state.sessions).contains_key(&session.id) {
                continue;
            }
            let _ = record::write(&state.dir, state.instance, session.id, &record);
        }
    }
}

fn typed_text(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut escape = false;
    for ch in String::from_utf8_lossy(bytes).chars() {
        if escape {
            escape = !(ch.is_ascii_alphabetic() || ch == '~');
        } else if ch == '\x1b' {
            escape = true;
        } else if !ch.is_control() {
            text.push(ch);
        }
    }
    text
}

impl HostSession {
    fn shell_in_front(&self) -> bool {
        rustix::termios::tcgetpgrp(&*lock(&self.input))
            .is_ok_and(|group| group.as_raw_nonzero().get() as u32 == self.pid)
    }

    fn typing_hidden(&self) -> bool {
        use rustix::termios::LocalModes;
        rustix::termios::tcgetattr(&*lock(&self.input)).is_ok_and(|modes| {
            modes.local_modes.contains(LocalModes::ICANON) && !modes.local_modes.contains(LocalModes::ECHO)
        })
    }

    fn track_input(&self, bytes: &[u8]) {
        if !self.keep.load(Ordering::Acquire) {
            return;
        }
        if !self.shell_in_front() || self.typing_hidden() {
            lock(&self.typed).start = None;
            return;
        }
        let screen = lock(&self.screen);
        let term = &screen.term;
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let history = term.grid().history_size();
        let cursor = term.grid().cursor.point;
        let mut typed = lock(&self.typed);
        let mut from = 0;
        for (index, byte) in bytes.iter().enumerate() {
            match byte {
                b'\r' | b'\n' => {
                    let start = typed.start.take();
                    let mut line = String::new();
                    if let Some((row, col)) = start
                        && let Some(top) = (row as i32).checked_sub(history as i32)
                        && top >= -(history as i32)
                    {
                        let begin = Point::new(Line(top), Column(col.min(term.columns().saturating_sub(1))));
                        let end = Point::new(cursor.line, Column(term.columns().saturating_sub(1)));
                        if begin <= end {
                            line = term.bounds_to_string(begin, end).replace('\n', "");
                        }
                    }
                    line.push_str(&typed_text(&bytes[from..index]));
                    let line = line.trim();
                    if !line.is_empty() {
                        typed.command = Some(line.chars().take(record::MAX_COMMAND).collect());
                        self.dirty.store(true, Ordering::Release);
                    }
                    from = index + 1;
                }
                3 | 4 => {
                    typed.start = None;
                    from = index + 1;
                }
                _ if typed.start.is_none() => {
                    typed.start = Some((history + cursor.line.0.max(0) as usize, cursor.column.0));
                    from = index;
                }
                _ => {}
            }
        }
    }

    fn record(&self) -> Record {
        let running = self.running();
        let text = {
            let screen = lock(&self.screen);
            let term = &screen.term;
            let rows = term.screen_lines() as i32;
            let history = term.grid().history_size() as i32;
            let last = (0..rows).rev().find(|row| !row_blank(term, Line(*row))).unwrap_or(0);
            let first = (last + 1 - record::MAX_LINES as i32).max(-history);
            let end = Point::new(Line(last), Column(term.columns().saturating_sub(1)));
            let text = term.bounds_to_string(Point::new(Line(first), Column(0)), end);
            let cut = text.len().saturating_sub(record::MAX_TEXT);
            let cut = (cut..=text.len()).find(|at| text.is_char_boundary(*at)).unwrap_or(text.len());
            text[cut..].to_owned()
        };
        let typed = lock(&self.typed);
        Record {
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            command: typed.command.clone(),
            running,
            text,
            saved: record::now(),
        }
    }

    fn running(&self) -> Option<Vec<String>> {
        let group = rustix::termios::tcgetpgrp(&*lock(&self.input)).ok()?.as_raw_nonzero().get() as u32;
        let mut typed = lock(&self.typed);
        if group == self.pid {
            typed.running = None;
            return None;
        }
        if let Some((known, args)) = &typed.running
            && *known == group
        {
            return Some(args.clone());
        }
        drop(typed);
        let args = command_of(group)?;
        lock(&self.typed).running = Some((group, args.clone()));
        Some(args)
    }

    fn info(&self) -> SessionInfo {
        SessionInfo { id: self.id, pid: self.pid, cwd: self.cwd.clone(), attached: lock(&self.attached).is_some() }
    }

    fn send(&self, frame: &Frame) {
        let writer = lock(&self.attached).as_ref().map(|(_, writer)| Arc::clone(writer));
        if let Some(writer) = writer
            && write_frame(&mut *lock(&writer), frame).is_err()
        {
            let mut attached = lock(&self.attached);
            if attached.as_ref().is_some_and(|(_, current)| Arc::ptr_eq(current, &writer)) {
                *attached = None;
            }
        }
    }

    fn hang_up(&self) {
        if let Some(pid) = rustix::process::Pid::from_raw(self.pid as i32) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::HUP);
        }
    }

    fn wait(&self) -> Option<i32> {
        let started = Instant::now();
        while started.elapsed() < EXIT_WAIT {
            if let Some(ChildEvent::Exited(status)) = lock(&self.pty).next_child_event() {
                return status.and_then(|status| status.code());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }
}

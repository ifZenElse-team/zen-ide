use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{self, CursorShape, NamedColor};

#[cfg(unix)]
use crate::host::{Host, protocol};
use crate::{
    Attributes, Cell, Color, Cursor, Error, Exit, GridPoint, HostedSession, Options, Screen, SelectionKind, Size,
    default_shell, tidy_copied,
};

const WORD_BOUNDARIES: &str = ",│`|\"' ()[]{}<>\t";

#[cfg(unix)]
const ELSEWHERE_POLL: std::time::Duration = std::time::Duration::from_secs(2);

pub struct Session {
    term: Arc<FairMutex<Term<Listener>>>,
    shared: Arc<Shared>,
}

enum Sink {
    Local(EventLoopSender),
    #[cfg(unix)]
    Remote {
        hosted: HostedSession,
        stream: Mutex<std::os::unix::net::UnixStream>,
    },
    #[cfg(unix)]
    Elsewhere(HostedSession),
}

impl Sink {
    fn input(&self, bytes: Vec<u8>) {
        match self {
            Sink::Local(sender) => {
                let _ = sender.send(Msg::Input(Cow::Owned(bytes)));
            }
            #[cfg(unix)]
            Sink::Remote { stream, .. } => {
                let _ = protocol::write_frame(&mut *lock(stream), &protocol::Frame::Data(bytes));
            }
            #[cfg(unix)]
            Sink::Elsewhere(_) => {}
        }
    }

    fn resize(&self, size: Size) {
        match self {
            Sink::Local(sender) => {
                let _ = sender.send(Msg::Resize(window_size(size)));
            }
            #[cfg(unix)]
            Sink::Remote { stream, .. } => {
                let message = protocol::Message::Resize { rows: size.rows, cols: size.cols };
                let _ = protocol::write_frame(&mut *lock(stream), &protocol::Frame::Control(message));
            }
            #[cfg(unix)]
            Sink::Elsewhere(_) => {}
        }
    }

    fn close(&self) {
        match self {
            Sink::Local(sender) => {
                let _ = sender.send(Msg::Shutdown);
            }
            #[cfg(unix)]
            Sink::Remote { stream, .. } => {
                let close = protocol::Frame::Control(protocol::Message::Close);
                let _ = protocol::write_frame(&mut *lock(stream), &close);
            }
            #[cfg(unix)]
            Sink::Elsewhere(_) => {}
        }
    }

    fn detach(&self) {
        match self {
            Sink::Local(sender) => {
                let _ = sender.send(Msg::Shutdown);
            }
            #[cfg(unix)]
            Sink::Remote { stream, .. } => {
                let _ = lock(stream).shutdown(std::net::Shutdown::Both);
            }
            #[cfg(unix)]
            Sink::Elsewhere(_) => {}
        }
    }
}

struct Shared {
    title: Mutex<Option<String>>,
    exit: Mutex<Option<Exit>>,
    exited: AtomicBool,
    lost: AtomicBool,
    size: Mutex<Size>,
    sink: OnceLock<Sink>,
    waker: Box<dyn Fn() + Send + Sync>,
}

#[derive(Clone)]
struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let shared = &self.0;
        match event {
            Event::Title(title) => *lock(&shared.title) = Some(title),
            Event::ResetTitle => *lock(&shared.title) = None,
            Event::PtyWrite(text) => shared.reply(text.into_bytes()),
            Event::TextAreaSizeRequest(format) => {
                let reply = format(window_size(*lock(&shared.size)));
                shared.reply(reply.into_bytes());
            }
            Event::ChildExit(status) => {
                *lock(&shared.exit) = Some(Exit { code: status.code() });
                shared.exited.store(true, Ordering::Release);
            }
            Event::Exit => shared.exited.store(true, Ordering::Release),
            Event::Wakeup => {}
            _ => return,
        }
        (shared.waker)();
    }
}

impl Shared {
    fn reply(&self, bytes: Vec<u8>) {
        if let Some(sink) = self.sink.get() {
            sink.input(bytes);
        }
    }

    fn sink(&self) -> Option<&Sink> {
        self.sink.get()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) struct Dims(pub(crate) Size);

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        usize::from(self.0.rows)
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.0.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.0.cols)
    }
}

fn window_size(size: Size) -> WindowSize {
    WindowSize { num_lines: size.rows, num_cols: size.cols, cell_width: 0, cell_height: 0 }
}

impl Session {
    pub fn spawn(options: Options, waker: impl Fn() + Send + Sync + 'static) -> Result<Session, Error> {
        let size = options.size.clamped();
        let (shared, term) = Session::parts(size, options.scrollback, Box::new(waker));
        let (program, args, env) = command_line(&options);
        if !options.preface.is_empty() {
            let mut parser: ansi::Processor = ansi::Processor::new();
            parser.advance(&mut *term.lock(), options.preface.as_bytes());
        }
        #[allow(clippy::needless_update, reason = "Windows builds have more fields (`escape_args`)")]
        let pty_options = tty::Options {
            shell: Some(tty::Shell::new(program.to_string_lossy().into_owned(), args)),
            working_directory: Some(options.cwd),
            drain_on_exit: true,
            env: env.into_iter().collect(),
            ..tty::Options::default()
        };
        let pty = tty::new(&pty_options, window_size(size), 0).map_err(Error::Spawn)?;
        let event_loop =
            EventLoop::new(Arc::clone(&term), Listener(Arc::clone(&shared)), pty, true, false).map_err(Error::Spawn)?;
        let _ = shared.sink.set(Sink::Local(event_loop.channel()));
        drop(event_loop.spawn());
        Ok(Session { term, shared })
    }

    #[cfg(unix)]
    pub fn open(host: &Host, options: Options, waker: impl Fn() + Send + Sync + 'static) -> Result<Session, Error> {
        let size = options.size.clamped();
        let (program, args, env) = command_line(&options);
        let request = protocol::OpenRequest {
            program,
            args,
            cwd: options.cwd,
            rows: size.rows,
            cols: size.cols,
            env,
            scrollback: Some(options.scrollback),
            keep: options.keep,
            preface: (!options.preface.is_empty()).then(|| options.preface.clone()),
        };
        let connection = host.connect().map_err(host_error)?;
        Session::remote(connection, protocol::Message::Open(request), size, options.scrollback, Box::new(waker))
    }

    #[cfg(unix)]
    pub fn attach(
        host: &Host,
        hosted: HostedSession,
        size: Size,
        waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        let size = size.clamped();
        let connection = host.reach(hosted.protocol).map_err(host_error)?;
        if connection.instance != hosted.host {
            return Err(Error::Spawn(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "the terminal host that ran this session has stopped",
            )));
        }
        let session = Session::remote(
            connection,
            protocol::Message::Attach { id: hosted.id },
            size,
            Options::DEFAULT_SCROLLBACK,
            Box::new(waker),
        )?;
        session.resize(size);
        Ok(session)
    }

    #[cfg(unix)]
    pub fn elsewhere(
        host: &Host,
        hosted: HostedSession,
        size: Size,
        text: &str,
        waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        let size = size.clamped();
        let (shared, term) = Session::parts(size, Options::DEFAULT_SCROLLBACK, Box::new(waker));
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut *term.lock(), text.replace('\n', "\r\n").as_bytes());
        let _ = shared.sink.set(Sink::Elsewhere(hosted));
        let (watched, host) = (Arc::downgrade(&shared), host.clone());
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(ELSEWHERE_POLL);
                let Some(shared) = watched.upgrade() else { return };
                if !host.runs(hosted.protocol) {
                    shared.exited.store(true, Ordering::Release);
                    (shared.waker)();
                    return;
                }
            }
        });
        Ok(Session { term, shared })
    }

    #[cfg(unix)]
    fn remote(
        connection: crate::host::Connection,
        request: protocol::Message,
        size: Size,
        scrollback: usize,
        waker: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Session, Error> {
        use protocol::{Frame, Message, read_frame, write_frame};
        let crate::host::Connection { mut stream, protocol: spoken, instance } = connection;
        write_frame(&mut stream, &Frame::Control(request)).map_err(Error::Spawn)?;
        let (id, size, snapshot) = match read_frame(&mut stream).map_err(Error::Spawn)? {
            Frame::Control(Message::Opened { id, rows, cols, snapshot }) => {
                (id, Size::new(rows.unwrap_or(size.rows), cols.unwrap_or(size.cols)).clamped(), snapshot.unwrap_or(0))
            }
            other => return Err(Error::Spawn(crate::host::unexpected(&other))),
        };
        let (shared, term) = Session::parts(size, scrollback, waker);
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut replayed = 0u64;
        while replayed < snapshot {
            match read_frame(&mut stream).map_err(Error::Spawn)? {
                Frame::Data(bytes) => {
                    replayed += bytes.len() as u64;
                    parser.advance(&mut *term.lock(), &bytes);
                }
                other => return Err(Error::Spawn(crate::host::unexpected(&other))),
            }
        }
        let reader = stream.try_clone().map_err(Error::Spawn)?;
        let hosted = HostedSession { id, protocol: spoken, host: instance };
        let _ = shared.sink.set(Sink::Remote { hosted, stream: Mutex::new(stream) });
        let (pump_term, pump_shared) = (Arc::clone(&term), Arc::clone(&shared));
        std::thread::spawn(move || pump(reader, &pump_term, &pump_shared));
        Ok(Session { term, shared })
    }

    #[cfg(not(unix))]
    pub fn open(
        _host: &crate::Host,
        _options: Options,
        _waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        Err(Error::Unsupported)
    }

    #[cfg(not(unix))]
    pub fn attach(
        _host: &crate::Host,
        _hosted: HostedSession,
        _size: Size,
        _waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        Err(Error::Unsupported)
    }

    #[cfg(not(unix))]
    pub fn elsewhere(
        _host: &crate::Host,
        _hosted: HostedSession,
        _size: Size,
        _text: &str,
        _waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        Err(Error::Unsupported)
    }

    pub fn hosted(&self) -> Option<HostedSession> {
        match self.shared.sink()? {
            Sink::Local(_) => None,
            #[cfg(unix)]
            Sink::Remote { hosted, .. } | Sink::Elsewhere(hosted) => Some(*hosted),
        }
    }

    pub fn is_elsewhere(&self) -> bool {
        #[cfg(unix)]
        if let Some(Sink::Elsewhere(_)) = self.shared.sink() {
            return true;
        }
        false
    }

    pub fn close(&self) {
        if let Some(sink) = self.shared.sink() {
            sink.close();
        }
    }

    pub fn set_keep(&self, keep: bool) {
        #[cfg(unix)]
        if let Some(Sink::Remote { stream, .. }) = self.shared.sink() {
            let message = protocol::Frame::Control(protocol::Message::Keep { keep });
            let _ = protocol::write_frame(&mut *lock(stream), &message);
        }
        #[cfg(not(unix))]
        let _ = keep;
    }

    pub fn lost_host(&self) -> bool {
        self.shared.lost.load(Ordering::Acquire)
    }

    fn parts(
        size: Size,
        scrollback: usize,
        waker: Box<dyn Fn() + Send + Sync>,
    ) -> (Arc<Shared>, Arc<FairMutex<Term<Listener>>>) {
        let shared = Arc::new(Shared {
            title: Mutex::new(None),
            exit: Mutex::new(None),
            exited: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            size: Mutex::new(size),
            sink: OnceLock::new(),
            waker,
        });
        let config = Config {
            scrolling_history: scrollback,
            semantic_escape_chars: WORD_BOUNDARIES.to_owned(),
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &Dims(size), Listener(Arc::clone(&shared)))));
        (shared, term)
    }

    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        let bytes = bytes.into();
        if !bytes.is_empty()
            && let Some(sink) = self.shared.sink()
        {
            sink.input(bytes);
        }
    }

    pub fn resize(&self, size: Size) {
        let size = size.clamped();
        {
            let mut current = lock(&self.shared.size);
            if *current == size {
                return;
            }
            *current = size;
        }
        self.term.lock().resize(Dims(size));
        if let Some(sink) = self.shared.sink() {
            sink.resize(size);
        }
    }

    pub fn size(&self) -> Size {
        *lock(&self.shared.size)
    }

    pub fn scroll(&self, lines: i32) {
        if lines != 0 {
            self.term.lock().scroll_display(Scroll::Delta(lines));
        }
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    pub fn snapshot(&self) -> Screen {
        let term = self.term.lock();
        let rows = term.screen_lines();
        let cols = term.columns();
        let history = term.grid().history_size();
        let mode = *term.mode();
        let content = term.renderable_content();
        let offset = content.display_offset;
        let mut lines = vec![vec![Cell::default(); cols]; rows];
        let mut wrapped = vec![false; rows];
        for indexed in content.display_iter {
            let Ok(row) = usize::try_from(indexed.point.line.0 + offset as i32) else { continue };
            if indexed.cell.flags.contains(Flags::WRAPLINE)
                && let Some(slot) = wrapped.get_mut(row)
            {
                *slot = true;
            }
            if let Some(slot) = lines.get_mut(row).and_then(|line| line.get_mut(indexed.point.column.0)) {
                *slot = convert_cell(indexed.cell);
            }
        }
        let cursor_row = usize::try_from(content.cursor.point.line.0 + offset as i32).ok().filter(|row| *row < rows);
        let cursor = Cursor {
            row: cursor_row.unwrap_or(0),
            col: content.cursor.point.column.0.min(cols.saturating_sub(1)),
            visible: cursor_row.is_some() && content.cursor.shape != CursorShape::Hidden,
        };
        let selection = content.selection.map(|range| visible_selection(range, offset, rows, cols)).unwrap_or_default();
        Screen {
            selection,
            lines,
            cols,
            cursor,
            history,
            scroll_offset: offset,
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            wrapped,
        }
    }

    pub fn start_selection(&self, kind: SelectionKind, at: GridPoint) {
        let mut term = self.term.lock();
        let (point, side) = grid_point(&term, at);
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        term.selection = Some(Selection::new(ty, point, side));
    }

    pub fn extend_selection(&self, to: GridPoint) {
        let mut term = self.term.lock();
        let (point, side) = grid_point(&term, to);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    pub fn select_all(&self) {
        let mut term = self.term.lock();
        let top = Point::new(Line(-(term.grid().history_size() as i32)), Column(0));
        let bottom = Point::new(Line(term.screen_lines() as i32 - 1), Column(term.columns().saturating_sub(1)));
        let mut selection = Selection::new(SelectionType::Simple, top, Side::Left);
        selection.update(bottom, Side::Right);
        term.selection = Some(selection);
    }

    pub fn has_selection(&self) -> bool {
        self.term.lock().selection.as_ref().is_some_and(|selection| !selection.is_empty())
    }

    pub fn selection_text(&self) -> Option<String> {
        let term = self.term.lock();
        term.selection.as_ref().filter(|selection| !selection.is_empty())?;
        term.selection_to_string().map(|text| tidy_copied(&text))
    }

    pub fn line_count(&self) -> usize {
        let term = self.term.lock();
        term.grid().history_size() + term.screen_lines()
    }

    pub fn lines_text(&self, from: usize, to: usize) -> String {
        let term = self.term.lock();
        let history = term.grid().history_size() as i32;
        let last = (term.grid().history_size() + term.screen_lines()).max(1);
        let (from, to) = (from.clamp(1, last), to.clamp(1, last));
        let (from, to) = (from.min(to), from.max(to));
        let line = |number: usize| Line(number as i32 - 1 - history);
        let start = Point::new(line(from), Column(0));
        let end = Point::new(line(to), Column(term.columns().saturating_sub(1)));
        tidy_copied(&term.bounds_to_string(start, end))
    }

    pub fn title(&self) -> Option<String> {
        lock(&self.shared.title).clone()
    }

    pub fn exit(&self) -> Option<Exit> {
        *lock(&self.shared.exit)
    }

    pub fn has_exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(sink) = self.shared.sink() {
            sink.detach();
        }
    }
}

#[cfg(unix)]
fn host_error(error: std::io::Error) -> Error {
    match error.get_ref().and_then(|inner| inner.downcast_ref::<crate::host::Incompatible>()) {
        Some(incompatible) => Error::Incompatible { protocol: incompatible.0 },
        None => Error::Spawn(error),
    }
}

fn command_line(options: &Options) -> (std::path::PathBuf, Vec<String>, Vec<(String, String)>) {
    let mut env = vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ("PWD".to_owned(), options.cwd.to_string_lossy().into_owned()),
    ];
    for (key, value) in zen_env::variables().into_iter().chain(options.env.iter().cloned()) {
        env.retain(|(known, _)| *known != key);
        env.push((key, value));
    }
    let (program, args) = match &options.program {
        Some(program) => (program.clone(), options.args.clone()),
        None => (default_shell(), vec!["-l".to_owned()]),
    };
    (program, args, env)
}

#[cfg(unix)]
fn pump(mut stream: std::os::unix::net::UnixStream, term: &FairMutex<Term<Listener>>, shared: &Shared) {
    use protocol::{Frame, Message};
    let mut parser: ansi::Processor = ansi::Processor::new();
    let code = loop {
        match protocol::read_frame(&mut stream) {
            Ok(Frame::Data(bytes)) => {
                parser.advance(&mut *term.lock(), &bytes);
                (shared.waker)();
            }
            Ok(Frame::Control(Message::Exited { code })) => break code,
            Ok(Frame::Control(_)) => {}
            Err(_) => {
                shared.lost.store(true, Ordering::Release);
                break None;
            }
        }
    };
    *lock(&shared.exit) = Some(Exit { code });
    shared.exited.store(true, Ordering::Release);
    (shared.waker)();
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("size", &self.size()).field("exited", &self.has_exited()).finish()
    }
}

fn grid_point(term: &Term<Listener>, at: GridPoint) -> (Point, Side) {
    let offset = term.grid().display_offset() as i32;
    let rows = term.screen_lines().max(1);
    let row = at.row.min(rows - 1) as i32;
    let col = at.col.min(term.columns().saturating_sub(1));
    (Point::new(Line(row - offset), Column(col)), if at.right { Side::Right } else { Side::Left })
}

fn visible_selection(
    range: SelectionRange,
    offset: usize,
    rows: usize,
    cols: usize,
) -> Vec<(usize, std::ops::Range<usize>)> {
    let mut found = Vec::new();
    for row in 0..rows {
        let line = row as i32 - offset as i32;
        if line < range.start.line.0 || line > range.end.line.0 {
            continue;
        }
        let (start, end) = if range.is_block {
            (range.start.column.0, range.end.column.0 + 1)
        } else {
            let start = if line == range.start.line.0 { range.start.column.0 } else { 0 };
            let end = if line == range.end.line.0 { range.end.column.0 + 1 } else { cols };
            (start, end)
        };
        if start < end {
            found.push((row, start..end.min(cols)));
        }
    }
    found
}

fn convert_cell(cell: &alacritty_terminal::term::cell::Cell) -> Cell {
    let flags = cell.flags;
    let (fg, fg_dim) = convert_color(cell.fg);
    let (bg, _) = convert_color(cell.bg);
    Cell {
        ch: if flags.contains(Flags::WIDE_CHAR_SPACER) || cell.c == '\t' { ' ' } else { cell.c },
        combining: cell.zerowidth().map(<[char]>::to_vec).unwrap_or_default(),
        fg,
        bg,
        attrs: Attributes {
            bold: flags.contains(Flags::BOLD),
            dim: flags.contains(Flags::DIM) || fg_dim,
            italic: flags.contains(Flags::ITALIC),
            underline: flags.intersects(Flags::ALL_UNDERLINES),
            inverse: flags.contains(Flags::INVERSE),
            hidden: flags.contains(Flags::HIDDEN),
            strikethrough: flags.contains(Flags::STRIKEOUT),
        },
        wide: flags.contains(Flags::WIDE_CHAR),
        spacer: flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
    }
}

fn convert_color(color: ansi::Color) -> (Color, bool) {
    match color {
        ansi::Color::Spec(rgb) => (Color::Rgb(rgb.r, rgb.g, rgb.b), false),
        ansi::Color::Indexed(index) => (Color::Indexed(index), false),
        ansi::Color::Named(named) => match named {
            NamedColor::Black => (Color::Indexed(0), false),
            NamedColor::Red => (Color::Indexed(1), false),
            NamedColor::Green => (Color::Indexed(2), false),
            NamedColor::Yellow => (Color::Indexed(3), false),
            NamedColor::Blue => (Color::Indexed(4), false),
            NamedColor::Magenta => (Color::Indexed(5), false),
            NamedColor::Cyan => (Color::Indexed(6), false),
            NamedColor::White => (Color::Indexed(7), false),
            NamedColor::BrightBlack => (Color::Indexed(8), false),
            NamedColor::BrightRed => (Color::Indexed(9), false),
            NamedColor::BrightGreen => (Color::Indexed(10), false),
            NamedColor::BrightYellow => (Color::Indexed(11), false),
            NamedColor::BrightBlue => (Color::Indexed(12), false),
            NamedColor::BrightMagenta => (Color::Indexed(13), false),
            NamedColor::BrightCyan => (Color::Indexed(14), false),
            NamedColor::BrightWhite => (Color::Indexed(15), false),
            NamedColor::DimBlack => (Color::Indexed(0), true),
            NamedColor::DimRed => (Color::Indexed(1), true),
            NamedColor::DimGreen => (Color::Indexed(2), true),
            NamedColor::DimYellow => (Color::Indexed(3), true),
            NamedColor::DimBlue => (Color::Indexed(4), true),
            NamedColor::DimMagenta => (Color::Indexed(5), true),
            NamedColor::DimCyan => (Color::Indexed(6), true),
            NamedColor::DimWhite => (Color::Indexed(7), true),
            NamedColor::DimForeground => (Color::Default, true),
            _ => (Color::Default, false),
        },
    }
}

#[cfg(test)]
mod login_environment_tests {
    use super::*;

    #[test]
    fn a_terminal_gets_the_login_shell_s_variables_under_its_own() {
        let variables = [("FROM_LOGIN", "login"), ("SHARED", "login")]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        zen_env::adopt(zen_env::Environment { shell: std::path::PathBuf::from("/bin/zsh"), variables });
        let mut options = Options::shell("/tmp");
        options.env = vec![("SHARED".to_owned(), "the tab's".to_owned())];
        let (_, _, env) = command_line(&options);
        let value =
            |name: &str| env.iter().filter(|(key, _)| key == name).map(|(_, value)| value.as_str()).collect::<Vec<_>>();
        assert_eq!(value("FROM_LOGIN"), ["login"]);
        assert_eq!(value("SHARED"), ["the tab's"], "a tab's own variable wins, once");
        assert_eq!(value("TERM"), ["xterm-256color"]);
    }
}

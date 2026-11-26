#![cfg(unix)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use zen_terminal::{Color, Exit, GridPoint, Options, Screen, SelectionKind, Session, Size};

const TIMEOUT: Duration = Duration::from_secs(60);

fn sh(script: &str) -> Options {
    Options::command("/bin/sh", &["-c", script], std::env::temp_dir())
}

fn spawn(options: Options) -> Session {
    Session::spawn(options, || {}).expect("the session starts")
}

fn wait_for(session: &Session, what: &str, done: impl Fn(&Screen) -> bool) -> Screen {
    let start = Instant::now();
    loop {
        let screen = session.snapshot();
        if done(&screen) {
            return screen;
        }
        assert!(start.elapsed() < TIMEOUT, "timed out waiting for {what}; screen:\n{}", screen.text());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_exit(session: &Session) {
    let start = Instant::now();
    while !session.has_exited() {
        assert!(start.elapsed() < TIMEOUT, "the program did not exit; screen:\n{}", session.snapshot().text());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn output_reaches_the_grid_and_wakes_the_ui() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let session = Session::spawn(sh("printf 'hello\\r\\nworld'"), move || {
        counter.fetch_add(1, Ordering::Relaxed);
    })
    .expect("the session starts");
    wait_exit(&session);
    let screen = session.snapshot();
    assert_eq!(screen.row_text(0), "hello");
    assert_eq!(screen.row_text(1), "world");
    assert_eq!((screen.rows(), screen.cols), (24, 80));
    assert!(wakes.load(Ordering::Relaxed) > 0);
}

#[test]
fn exit_code_is_reported() {
    let session = spawn(sh("exit 3"));
    wait_exit(&session);
    let start = Instant::now();
    while session.exit().is_none() && start.elapsed() < TIMEOUT {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(session.exit(), Some(Exit { code: Some(3) }));
}

#[test]
fn input_reaches_the_program() {
    let session = spawn(sh("read line; printf 'got:%s' \"$line\""));
    session.write(b"abc\r".to_vec());
    wait_for(&session, "the echo", |screen| screen.find("got:abc").is_some());
}

#[test]
fn resize_reaches_grid_and_program() {
    let session = spawn(sh("read line; stty size"));
    session.resize(Size::new(10, 40));
    assert_eq!(session.size(), Size::new(10, 40));
    session.write(b"\r".to_vec());
    let screen = wait_for(&session, "stty", |screen| screen.find("10 40").is_some());
    assert_eq!((screen.rows(), screen.cols), (10, 40));
}

#[test]
fn colours_and_attributes() {
    let session =
        spawn(sh("printf '\\033[31mR\\033[0m\\033[38;2;1;2;3;48;5;200mT\\033[0m\\033[1;3;4;7mB\\033[0m\\033[92mG'"));
    let screen = wait_for(&session, "the text", |screen| screen.find("RTBG").is_some());
    let (row, col) = screen.find("RTBG").expect("found");
    let cell = |offset: usize| screen.cell(row, col + offset).expect("cell").clone();
    assert_eq!(cell(0).fg, Color::Indexed(1));
    assert_eq!(cell(0).bg, Color::Default);
    assert_eq!(cell(1).fg, Color::Rgb(1, 2, 3));
    assert_eq!(cell(1).bg, Color::Indexed(200));
    let bold = cell(2).attrs;
    assert!(bold.bold && bold.italic && bold.underline && bold.inverse && !bold.dim);
    assert_eq!(cell(3).fg, Color::Indexed(10));
    assert!(!cell(3).attrs.bold);
}

#[test]
fn title_cursor_and_working_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    let expected = dir.path().canonicalize().expect("canonical");
    let options =
        Options::command("/bin/sh", &["-c", "printf '\\033]2;My title\\007'; pwd -P; printf 'ab'"], dir.path());
    let session = spawn(options);
    wait_exit(&session);
    let screen = session.snapshot();
    assert_eq!(session.title().as_deref(), Some("My title"));
    assert_eq!(screen.row_text(0), expected.to_string_lossy());
    assert_eq!(screen.row_text(1), "ab");
    assert_eq!((screen.cursor.row, screen.cursor.col), (1, 2));
}

#[test]
fn scrollback_and_scrolling() {
    let mut options = sh("i=1; while [ $i -le 40 ]; do echo line$i; i=$((i+1)); done");
    options.size = Size::new(10, 20);
    let session = spawn(options);
    wait_exit(&session);
    let screen = session.snapshot();
    assert!(screen.history >= 30, "history {}", screen.history);
    assert_eq!(screen.scroll_offset, 0);
    assert!(screen.find("line40").is_some());

    session.scroll(5);
    let scrolled = session.snapshot();
    assert_eq!(scrolled.scroll_offset, 5);
    assert!(scrolled.find("line40").is_none());
    assert!(!scrolled.cursor.visible || scrolled.cursor.row < 10);

    session.scroll_to_bottom();
    assert_eq!(session.snapshot().scroll_offset, 0);
}

#[test]
fn hidden_cursor_is_reported() {
    let session = spawn(sh("printf '\\033[?25lx'; read line"));
    let screen = wait_for(&session, "the text", |screen| screen.find("x").is_some());
    assert!(!screen.cursor.visible);
    drop(session);
}

#[test]
fn spawn_failure_is_an_error() {
    let result = Session::spawn(Options::command("/nonexistent/program", &[], std::env::temp_dir()), || {});
    assert!(result.is_err());
}

#[test]
fn a_long_line_wraps_and_its_link_is_found_across_the_rows() {
    let mut options = sh("printf 'see https://example.com/a/very/long/path\\r\\nnext'; sleep 5");
    options.size = Size::new(6, 20);
    let session = spawn(options);
    let screen = wait_for(&session, "the wrapped line", |screen| screen.row_text(2) == "next");
    assert_eq!(screen.wrapped[..3], [true, false, false], "{:?}", screen.wrapped);
    let links = screen.links();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].link.target, "https://example.com/a/very/long/path");
    assert_eq!(links[0].cells, vec![(0, 4..20), (1, 0..20)]);
}

fn sized(script: &str, rows: u16, cols: u16) -> Session {
    let mut options = sh(script);
    options.size = Size::new(rows, cols);
    spawn(options)
}

fn at(row: usize, col: usize) -> GridPoint {
    GridPoint { row, col, right: false }
}

fn end_of(row: usize, col: usize) -> GridPoint {
    GridPoint { row, col, right: true }
}

#[test]
fn a_copied_line_the_terminal_wrapped_is_one_line_without_trailing_spaces() {
    let session = sized("printf 'abcdefghijklmnopqrstuvwxyz0123\\nshort   \\nlast'; sleep 30", 10, 20);
    wait_for(&session, "the output", |screen| screen.text().contains("last"));
    session.start_selection(SelectionKind::Simple, at(0, 0));
    session.extend_selection(end_of(3, 19));
    assert_eq!(session.selection_text().as_deref(), Some("abcdefghijklmnopqrstuvwxyz0123\nshort\nlast"));
    session.start_selection(SelectionKind::Line, at(1, 3));
    assert_eq!(session.selection_text().as_deref(), Some("abcdefghijklmnopqrstuvwxyz0123\n"), "a logical line");
}

#[test]
fn a_double_click_takes_a_path_or_a_url_whole() {
    let session = sized("printf 'see src/main.rs:12:5 and https://example.org/a?b=1 now'; sleep 30", 5, 80);
    wait_for(&session, "the output", |screen| screen.text().contains("now"));
    let screen = session.snapshot();
    let (row, col) = screen.find("main").unwrap();
    session.start_selection(SelectionKind::Word, at(row, col));
    assert_eq!(session.selection_text().as_deref(), Some("src/main.rs:12:5"));
    let (row, col) = screen.find("example").unwrap();
    session.start_selection(SelectionKind::Word, at(row, col));
    assert_eq!(session.selection_text().as_deref(), Some("https://example.org/a?b=1"));
}

#[test]
fn a_block_selection_copies_a_rectangle() {
    let session = sized("printf 'a1 b1 c1\\na2 b2 c2\\na3 b3 c3'; sleep 30", 5, 40);
    wait_for(&session, "the output", |screen| screen.text().contains("c3"));
    session.start_selection(SelectionKind::Block, at(0, 3));
    session.extend_selection(end_of(2, 4));
    assert_eq!(session.selection_text().as_deref(), Some("b1\nb2\nb3"));
    let screen = session.snapshot();
    assert!(screen.selected(1, 3) && !screen.selected(1, 0));
}

#[test]
fn the_selection_stays_on_its_text_while_output_streams() {
    let session =
        sized("printf 'keep me\\n'; sleep 1; for i in 1 2 3 4 5 6 7 8; do echo line$i; done; sleep 30", 5, 40);
    wait_for(&session, "the first line", |screen| screen.text().contains("keep me"));
    let (row, col) = session.snapshot().find("keep").unwrap();
    session.start_selection(SelectionKind::Simple, at(row, col));
    session.extend_selection(end_of(row, col + 6));
    wait_for(&session, "the stream", |screen| screen.text().contains("line8"));
    assert_eq!(session.selection_text().as_deref(), Some("keep me"), "it scrolled into the history with its text");
    session.select_all();
    let all = session.selection_text().unwrap();
    assert!(all.starts_with("keep me") && all.contains("line8"), "{all}");
    assert_eq!(session.lines_text(1, 1), "keep me");
    assert!(session.line_count() >= 9);
    session.clear_selection();
    assert!(!session.has_selection() && session.selection_text().is_none());
}

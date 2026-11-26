#![cfg(unix)]

use std::path::Path;
use std::time::{Duration, Instant};

use zen_terminal::host::{Speaks, serve_with, socket_path};
use zen_terminal::{Error, Host, Options, Session, Size};

const IDLE: Duration = Duration::from_secs(1);

fn wait(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start_host(dir: &Path, speaks: Speaks) -> std::thread::JoinHandle<()> {
    let folder = dir.to_owned();
    let handle = std::thread::spawn(move || serve_with(&folder, IDLE, speaks).unwrap());
    wait("the host's socket", || socket_path(dir, speaks.protocol).exists());
    handle
}

fn ticker(cwd: &Path) -> Options {
    Options::command("/bin/sh", &["-c", "while :; do echo tick; sleep 0.1; done"], cwd)
}

#[test]
fn a_newer_ide_reads_an_older_host_and_one_that_cannot_leaves_it_running() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("terminal");
    let (old, newer, newest) =
        (Speaks { protocol: 1, oldest: 1 }, Speaks { protocol: 2, oldest: 1 }, Speaks { protocol: 3, oldest: 2 });
    let old_host = start_host(&dir, old);
    let first = Session::open(&Host::new(&dir, None).speaking(old), ticker(temp.path()), || {}).unwrap();
    let hosted = first.hosted().unwrap();
    assert_eq!(hosted.protocol, 1);
    drop(first);

    let new_host = start_host(&dir, newer);
    let updated = Host::new(&dir, None).speaking(newer);
    let again = Session::attach(&updated, hosted, Size::new(24, 80), || {}).expect("the newer IDE reads protocol 1");
    wait("output from the old host", || again.snapshot().text().contains("tick"));
    let fresh = Session::open(&updated, ticker(temp.path()), || {}).unwrap();
    assert_eq!(fresh.hosted().unwrap().protocol, 2, "a new terminal opens in the new host");
    drop(again);

    let latest = Host::new(&dir, None).speaking(newest);
    match Session::attach(&latest, hosted, Size::new(24, 80), || {}) {
        Err(Error::Incompatible { protocol: 1 }) => {}
        other => panic!("{other:?}"),
    }
    let shown = Session::elsewhere(&latest, hosted, Size::new(24, 80), "runs in an older host", || {}).unwrap();
    assert!(shown.snapshot().text().contains("runs in an older host"));
    assert!(shown.is_elsewhere() && !shown.has_exited());
    assert_eq!(shown.hosted(), Some(hosted), "it is remembered for the next start");
    std::thread::sleep(Duration::from_millis(2500));
    assert!(!shown.has_exited(), "the old host still runs its session");

    let reader = Session::attach(&updated, hosted, Size::new(24, 80), || {}).unwrap();
    reader.close();
    wait("the old session to end", || reader.has_exited());
    drop(reader);
    old_host.join().unwrap();
    wait("the tab to see that the old host stopped", || shown.has_exited());

    fresh.close();
    wait("the new session to end", || fresh.has_exited());
    drop(fresh);
    new_host.join().unwrap();
}

#[test]
fn a_session_of_a_host_that_restarted_is_not_taken_for_another() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("terminal");
    let host = Host::new(&dir, None);
    let first_host = start_host(&dir, Speaks::CURRENT);
    let first = Session::open(&host, ticker(temp.path()), || {}).unwrap();
    let hosted = first.hosted().unwrap();
    first.close();
    wait("the session to end", || first.has_exited());
    drop(first);
    first_host.join().unwrap();

    let second_host = start_host(&dir, Speaks::CURRENT);
    let other = Session::open(&host, ticker(temp.path()), || {}).unwrap();
    assert_eq!(other.hosted().unwrap().id, hosted.id, "ids start again in a new host");
    assert!(Session::attach(&host, hosted, Size::new(24, 80), || {}).is_err(), "not the new host's session");
    other.close();
    wait("the other session to end", || other.has_exited());
    drop(other);
    second_host.join().unwrap();
}

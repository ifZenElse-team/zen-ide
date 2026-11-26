#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use zen_terminal::{Host, HostedSession, Options, Session, Size};
use zen_workbench::terminal::{SavedLayout, SavedSession, Terminals};

const IDE_SIDE: &str = "ZEN_PTY_HOST_TEST_IDE_SIDE";
const TICKER: &str = "i=0; while :; do echo tick-$i; i=$((i+1)); sleep 0.1; done";

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let host = host(&self.0);
        let signal = |target: String| {
            let _ = Command::new("kill").args(["-KILL", "--", &target]).stderr(Stdio::null()).status();
        };
        if host.socket().exists() {
            for session in host.list().unwrap_or_default() {
                signal(format!("-{}", session.pid));
            }
        }
        let info = std::fs::read_to_string(
            self.0.join("terminal").join(format!("host-{}.json", zen_terminal::host::PROTOCOL)),
        );
        if let Some(pid) = info
            .ok()
            .and_then(|text| text.split("\"pid\":").nth(1)?.split([',', '}']).next()?.trim().parse::<u32>().ok())
        {
            signal(pid.to_string());
        }
    }
}

fn alive(pid: &str) -> bool {
    Command::new("kill").args(["-0", pid]).stderr(Stdio::null()).status().unwrap().success()
}

fn host(data: &Path) -> Host {
    Host::new(data.join("terminal"), Some(PathBuf::from(env!("CARGO_BIN_EXE_zen-pty-host"))))
        .with_idle(Duration::from_secs(1))
}

fn wait(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn ticker(cwd: &Path) -> Options {
    Options::command("/bin/sh", &["-c", TICKER], cwd)
}

#[test]
fn ide_side() {
    let Some(data) = std::env::var_os(IDE_SIDE) else { return };
    let data = PathBuf::from(data);
    let session = Session::open(&host(&data), ticker(&data), || {}).expect("the session opens");
    let hosted = session.hosted().expect("a host session");
    std::fs::write(data.join("ide-side"), format!("{} {} {}", hosted.id, hosted.protocol, hosted.host)).unwrap();
    std::thread::sleep(Duration::from_secs(600));
}

#[test]
fn a_session_keeps_running_when_the_ide_is_killed_and_comes_back_on_attach() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let mut ide = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ide_side", "--test-threads", "1"])
        .env(IDE_SIDE, data.path())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let written = data.path().join("ide-side");
    wait("the IDE side to open its session", || std::fs::read_to_string(&written).is_ok_and(|id| !id.is_empty()));
    let written = std::fs::read_to_string(&written).unwrap();
    let numbers: Vec<u64> = written.split(' ').map(|number| number.parse().unwrap()).collect();
    let hosted = HostedSession { id: numbers[0], protocol: numbers[1] as u32, host: numbers[2] };
    let id = hosted.id;
    ide.kill().unwrap();
    ide.wait().unwrap();

    let host = host(data.path());
    let listed = host.list().unwrap();
    assert_eq!(listed.iter().map(|session| (session.id, session.attached)).collect::<Vec<_>>(), vec![(id, false)]);
    std::thread::sleep(Duration::from_millis(300));
    let session = Session::attach(&host, hosted, Size::new(24, 80), || {}).expect("it attaches again");
    wait("new output after attaching", || session.snapshot().text().contains("tick-"));
    assert!(!session.has_exited());

    session.close();
    wait("the closed session to end", || session.has_exited());
    assert!(host.list().unwrap().is_empty(), "closing ends it in the host");
    drop(session);
    wait("the unused host to stop", || !host.socket().exists());
}

#[test]
fn closing_ends_the_processes_and_a_finished_program_reports_its_code() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let host = host(data.path());
    let session = Session::open(&host, ticker(data.path()), || {}).unwrap();
    let pid = host.list().unwrap()[0].pid;
    session.close();
    wait("the session to end", || session.has_exited());
    wait("the shell to be gone", || !alive(&pid.to_string()));

    let done = Session::open(&host, Options::command("/bin/sh", &["-c", "exit 7"], data.path()), || {}).unwrap();
    wait("the program to finish", || done.has_exited());
    assert_eq!(done.exit().and_then(|exit| exit.code), Some(7));
}

#[test]
fn the_next_start_reattaches_the_same_tabs() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let cwd = data.path().to_owned();
    let launcher: zen_workbench::terminal::Launcher = Arc::new(|cwd: &Path| ticker(cwd));
    let mut first = Terminals::default();
    first.set_launcher(Arc::clone(&launcher));
    first.set_host(Some(host(data.path())));
    first.new_tab(&cwd).unwrap();
    let saved = first.save();
    let SavedLayout::Pane { session: Some(saved_session), .. } = saved[0].layout else { panic!("{saved:?}") };
    drop(first);

    let mut second = Terminals::default();
    second.set_launcher(launcher);
    second.set_host(Some(host(data.path())));
    second.restore(saved);
    second.restore_pending(&cwd).unwrap();
    let pane = second.tabs()[0].root().panes()[0];
    assert_eq!(pane.session().hosted().map(SavedSession::from), Some(saved_session), "the same session, not a new one");
    wait("its output", || pane.session().snapshot().text().contains("tick-"));
    assert_eq!(host(data.path()).list().unwrap().len(), 1);

    let mut window = Terminals::default();
    window.set_launcher(Arc::new(|cwd: &Path| ticker(cwd)));
    window.set_host(Some(host(data.path())));
    window.new_tab(&cwd).unwrap();
    assert!(window.close_tab(0));
    wait("the closed tab's session to end", || host(data.path()).list().unwrap().len() == 1);
    assert!(second.close_tab(0));
    wait("every session to end", || host(data.path()).list().map_or(true, |list| list.is_empty()));
}

#[test]
fn attaching_again_shows_the_screen_then_the_stream_with_nothing_lost_or_twice() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let host = host(data.path());
    let count = 300;
    let script =
        format!("i=0; while [ $i -lt {count} ]; do echo t$i; i=$((i+1)); sleep 0.005; done; echo done; sleep 30");
    let first = Session::open(&host, Options::command("/bin/sh", &["-c", &script], data.path()), || {}).unwrap();
    wait("some output", || first.snapshot().text().contains("t20"));
    let id = first.hosted().unwrap();
    drop(first);
    std::thread::sleep(Duration::from_millis(200));
    let again = Session::attach(&host, id, Size::new(24, 80), || {}).unwrap();
    assert!(again.lines_text(1, again.line_count()).contains("t20"), "the screen is back before attach returns");
    wait("the end of the output", || again.snapshot().text().contains("done"));
    let all = again.lines_text(1, again.line_count());
    let numbers: Vec<usize> = all.lines().filter_map(|line| line.strip_prefix('t')?.parse().ok()).collect();
    assert_eq!(numbers, (0..count).collect::<Vec<_>>(), "every line once, in order");
    again.close();
}

#[test]
fn a_full_screen_program_is_shown_again_and_redraws_at_the_new_size() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let host = host(data.path());
    let script = "printf 'shell output\\n'; printf '\\033[?1049h\\033[2J\\033[Hfull screen app'; \
        trap 'printf \"\\033[2J\\033[Hredrawn %s\" \"$(stty size)\"' WINCH; while :; do sleep 0.05; done";
    let first = Session::open(&host, Options::command("/bin/sh", &["-c", script], data.path()), || {}).unwrap();
    wait("the program's screen", || first.snapshot().text().contains("full screen app"));
    let id = first.hosted().unwrap();
    drop(first);
    let again = Session::attach(&host, id, Size::new(24, 80), || {}).unwrap();
    wait("the screen from the snapshot", || again.snapshot().text().contains("full screen app"));
    assert!(again.snapshot().alt_screen, "still the program's screen");
    again.resize(Size::new(30, 100));
    wait("the redraw at the new size", || again.snapshot().text().contains("redrawn 30 100"));
    again.close();
}

#[test]
fn closing_ends_what_the_shell_started_in_the_background_too() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let host = host(data.path());
    let options = Options::command("/bin/sh", &["-c", "sleep 300 & echo child-$!; wait"], data.path());
    let session = Session::open(&host, options, || {}).unwrap();
    wait("the child's pid", || session.snapshot().text().contains("child-"));
    let text = session.snapshot().text();
    let child: String = text.split("child-").nth(1).unwrap().chars().take_while(char::is_ascii_digit).collect();
    assert!(alive(&child), "{text}");
    session.close();
    wait("the background child to be gone", || !alive(&child));
    wait("the session to end", || session.has_exited());
    assert!(host.list().unwrap().is_empty());
}

#[test]
fn a_screen_larger_than_a_frame_comes_back_whole() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let host = host(data.path());
    let script = "awk 'BEGIN { for (i = 0; i < 9000; i++) { s = \"\"; for (j = 0; j < 76; j++) s = s sprintf(\"\\033[3%dm%c\", j % 7 + 1, 97 + j % 26); print s \"\\033[0m-\" i } print \"all-printed\" }'; sleep 30";
    let first = Session::open(&host, Options::command("/bin/sh", &["-c", script], data.path()), || {}).unwrap();
    wait("everything printed", || first.snapshot().text().contains("all-printed"));
    let id = first.hosted().unwrap();
    drop(first);
    let again = Session::attach(&host, id, Size::new(24, 80), || {}).unwrap();
    let all = again.lines_text(1, again.line_count());
    assert!(all.contains("-8999") && all.contains("all-printed"), "{}", all.len());
    again.close();
}

fn host_info(data: &Path) -> serde_json::Value {
    let text =
        std::fs::read_to_string(data.join("terminal").join(format!("host-{}.json", zen_terminal::host::PROTOCOL)));
    serde_json::from_str(&text.unwrap()).unwrap()
}

#[test]
fn an_update_of_the_ide_never_ends_a_running_session() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let _cleanup = Cleanup(data.clone());
    let old_app = temp.path().join("Old.app").join("Contents").join("MacOS");
    std::fs::create_dir_all(&old_app).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_zen-pty-host"), old_app.join("zen-pty-host")).unwrap();
    let old_ide =
        Host::new(data.join("terminal"), Some(old_app.join("zen-pty-host"))).with_idle(Duration::from_secs(1));
    let first = Session::open(&old_ide, ticker(temp.path()), || {}).unwrap();
    let hosted = first.hosted().unwrap();
    let info = host_info(&data);
    let program = PathBuf::from(info["program"].as_str().unwrap());
    let copies = data.join("terminal").join("bin").join(zen_terminal::host::PROTOCOL.to_string());
    assert!(program.canonicalize().unwrap().starts_with(copies.canonicalize().unwrap()), "{}", program.display());
    drop(first);

    std::fs::remove_dir_all(temp.path().join("Old.app")).unwrap();
    let new_app = temp.path().join("New.app").join("Contents").join("MacOS");
    std::fs::create_dir_all(&new_app).unwrap();
    std::fs::write(new_app.join("zen-pty-host"), b"the next build, never started while a host runs").unwrap();
    let new_ide =
        Host::new(data.join("terminal"), Some(new_app.join("zen-pty-host"))).with_idle(Duration::from_secs(1));
    let again = Session::attach(&new_ide, hosted, Size::new(24, 80), || {}).expect("the updated IDE attaches");
    wait("output after the update", || again.snapshot().text().contains("tick-"));
    assert!(!again.has_exited());
    assert_eq!(host_info(&data)["pid"], info["pid"], "the same host process");
    assert!(program.is_file(), "the running host's copy is kept");

    again.close();
    wait("the session to end", || again.has_exited());
    drop(again);
    wait("the unused host to stop", || !new_ide.socket().exists());
}

#[test]
fn a_tab_of_a_host_this_ide_cannot_read_says_so_and_new_tabs_open_in_its_own() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let dir = data.path().join("terminal");
    let ancient = zen_terminal::host::Speaks { protocol: 0, oldest: 0 };
    let folder = dir.clone();
    let old_host = std::thread::spawn(move || zen_terminal::host::serve_with(&folder, Duration::from_secs(1), ancient));
    wait("the old host", || zen_terminal::host::socket_path(&dir, 0).exists());
    let old_ide = Host::new(&dir, None).speaking(ancient);
    let old = Session::open(&old_ide, ticker(data.path()), || {}).unwrap();
    let hosted = old.hosted().unwrap();
    drop(old);

    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut terminals = Terminals::default();
    terminals.set_launcher(Arc::new(|cwd: &Path| ticker(cwd)));
    terminals.set_host(Some(host(data.path())));
    let layout =
        SavedLayout::Pane { cwd: data.path().to_owned(), session: Some(SavedSession::from(hosted)), terminal: None };
    terminals.restore(vec![zen_workbench::terminal::SavedTab { name: None, layout }]);
    terminals.restore_pending(data.path()).unwrap();
    let list = terminals.list();
    let shown = &list.tabs[0].sessions[0];
    assert!(shown.elsewhere, "{list:?}");
    let pane = terminals.tabs()[0].root().panes()[0];
    assert!(pane.session().snapshot().text().contains("older terminal host"), "{}", pane.session().snapshot().text());
    let SavedLayout::Pane { session, .. } = &terminals.save()[0].layout else { panic!("a pane") };
    assert_eq!(*session, Some(hosted.into()), "it is remembered for the next start");

    let id = terminals.new_tab(data.path()).unwrap();
    let fresh = terminals.pane(id).unwrap().session().hosted().unwrap();
    assert_eq!(fresh.protocol, zen_terminal::host::PROTOCOL, "a new tab opens in this IDE's own host");

    let reader = Session::attach(&old_ide, hosted, Size::new(24, 80), || {}).unwrap();
    reader.close();
    wait("the old session to end", || reader.has_exited());
    drop(reader);
    old_host.join().unwrap().unwrap();
    wait("the tab of the stopped host to close", || {
        terminals.prune();
        terminals.tabs().len() == 1
    });
    assert!(terminals.close_tab(0));
}

fn shell(cwd: &Path) -> Options {
    let home = format!("HOME={}", cwd.display());
    Options::command("/usr/bin/env", &["-i", &home, "PATH=/usr/bin:/bin", "PS1=$ ", "/bin/sh"], cwd)
}

#[test]
fn a_tab_whose_host_died_comes_back_dimmed_in_a_new_shell_and_only_offers_its_command() {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let cwd = data.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut terminals = Terminals::default();
    terminals.set_launcher(Arc::new(|cwd: &Path| shell(cwd)));
    terminals.set_host(Some(host(data.path())));
    let id = terminals.new_tab(&cwd).unwrap();
    let pane = terminals.pane(id).unwrap();
    wait("the prompt", || pane.session().snapshot().text().contains('$'));
    pane.session().write(b"echo said-$((40+2))\r".to_vec());
    wait("the output", || pane.session().snapshot().text().contains("said-42"));
    let hosted = pane.session().hosted().unwrap();
    let host = host(data.path());
    wait("the record", || {
        host.record(hosted).is_some_and(|record| record.command.as_deref() == Some("echo said-$((40+2))"))
    });
    let pid = host_info(data.path())["pid"].to_string();
    assert!(Command::new("kill").args(["-KILL", &pid]).status().unwrap().success());

    wait("the tab to come back", || {
        terminals.prune();
        terminals.pane(id).is_some_and(|pane| !pane.session().lost_host() && pane.session().hosted() != Some(hosted))
    });
    let pane = terminals.pane(id).unwrap();
    wait("the new prompt", || pane.session().snapshot().text().contains("terminal host stopped"));
    let text = pane.session().lines_text(1, pane.session().line_count());
    assert!(text.contains("said-42"), "the old output is back: {text}");
    assert_eq!(pane.rerun(), Some("echo said-$((40+2))"), "offered, not run");
    assert_eq!(terminals.list().tabs[0].sessions[0].rerun.as_deref(), Some("echo said-$((40+2))"));
    let screen = pane.session().snapshot();
    let dimmed = screen.lines.iter().flatten().any(|cell| cell.ch == 's' && cell.attrs.dim);
    assert!(dimmed, "the old output is dimmed");
    std::thread::sleep(Duration::from_millis(500));
    let again = pane.session().lines_text(1, pane.session().line_count());
    assert_eq!(again.matches("said-42").count(), text.matches("said-42").count(), "nothing ran by itself: {again}");
    assert!(!pane.session().has_exited(), "a new shell runs");
    assert_eq!(host.record(hosted), None, "the old record is used up");
    assert!(terminals.close_tab(0));
}

fn fake_agent(bin: &Path) -> zen_workbench::terminal::agents::AgentAdapter {
    let adapter = zen_workbench::terminal::agents::AgentAdapter {
        id: "fake".to_owned(),
        name: "Fake Agent".to_owned(),
        programs: vec!["fake-agent".to_owned()],
        resume: vec!["fake-agent".to_owned(), "--resume".to_owned(), "{session}".to_owned()],
        session_field: "/session_id".to_owned(),
        docs: None,
    };
    std::fs::create_dir_all(bin).unwrap();
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = --resume ]; then echo \"resumed-conversation $2\"; else\n  printf '{{\"session_id\":\"conv-42\",\"cwd\":\"/x\"}}' | sh -c '{}'\n  echo started-conversation\nfi\nwhile :; do sleep 1; done\n",
        adapter.hook().replace('\'', "'\\''")
    );
    let program = bin.join("fake-agent");
    std::fs::write(&program, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    adapter
}

#[derive(Clone, Copy, PartialEq)]
enum Adapter {
    None,
    Settings,
    OtherPlugin,
}

fn run_agent_and_kill_the_host(given: Adapter) -> (tempfile::TempDir, Cleanup, Terminals, u64) {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let data = tempfile::tempdir().unwrap();
    let cleanup = Cleanup(data.path().to_owned());
    let bin = data.path().join("bin");
    let adapter = fake_agent(&bin);
    let path = format!("PATH={}:/usr/bin:/bin", bin.display());
    let mut terminals = Terminals::default();
    terminals
        .set_launcher(Arc::new(move |cwd: &Path| Options::command("/usr/bin/env", &[&path, "PS1=$ ", "/bin/sh"], cwd)));
    terminals.set_host(Some(host(data.path())));
    match given {
        Adapter::None => {}
        Adapter::Settings => terminals.set_user_agents(vec![adapter]),
        Adapter::OtherPlugin => {
            let value = serde_json::to_value(&adapter).unwrap();
            terminals.set_plugin_agents(Arc::new(move || vec![(value.clone(), false)]));
        }
    }
    let id = terminals.new_tab(data.path()).unwrap();
    let pane = terminals.pane(id).unwrap();
    let terminal = pane.terminal().to_owned();
    wait("the prompt", || pane.session().snapshot().text().contains('$'));
    pane.session().write(b"fake-agent\r".to_vec());
    wait("the agent", || pane.session().snapshot().text().contains("started-conversation"));
    let host = host(data.path());
    assert_eq!(host.agent_report(&terminal, "fake").unwrap()["session_id"], "conv-42", "the hook reported its id");
    let hosted = pane.session().hosted().unwrap();
    wait("the record of the running agent", || {
        host.record(hosted)
            .and_then(|record| record.running)
            .is_some_and(|running| running.iter().any(|word| word.ends_with("fake-agent")))
    });
    let pid = host_info(data.path())["pid"].to_string();
    assert!(Command::new("kill").args(["-KILL", &pid]).status().unwrap().success());
    wait("the tab to come back", || {
        terminals.prune();
        terminals.pane(id).is_some_and(|pane| !pane.session().lost_host() && pane.session().hosted() != Some(hosted))
    });
    assert_eq!(terminals.pane(id).unwrap().terminal(), terminal, "the same ZEN_TERMINAL_ID");
    (data, cleanup, terminals, id)
}

#[test]
fn an_agent_with_an_adapter_resumes_its_conversation_after_the_host_died() {
    let (_data, _cleanup, mut terminals, id) = run_agent_and_kill_the_host(Adapter::Settings);
    let pane = terminals.pane(id).unwrap();
    wait("the resumed conversation", || pane.session().snapshot().text().contains("resumed-conversation conv-42"));
    let text = pane.session().lines_text(1, pane.session().line_count());
    assert!(text.contains("Resuming the conversation of Fake Agent"), "{text}");
    assert_eq!(pane.resumed(), Some("Fake Agent"));
    assert_eq!(pane.rerun(), None, "nothing else is offered");
    assert_eq!(terminals.list().tabs[0].sessions[0].resumed.as_deref(), Some("Fake Agent"));
    assert!(terminals.close_tab(0));
}

#[test]
fn an_agent_without_an_adapter_is_only_offered_again() {
    let (_data, _cleanup, mut terminals, id) = run_agent_and_kill_the_host(Adapter::None);
    let pane = terminals.pane(id).unwrap();
    wait("the line", || pane.session().snapshot().text().contains("terminal host stopped"));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(pane.rerun(), Some("fake-agent"));
    assert_eq!(pane.resumed(), None);
    assert!(!pane.session().lines_text(1, pane.session().line_count()).contains("resumed-conversation"));
    assert!(terminals.close_tab(0));
}

#[test]
fn an_adapter_from_a_plugin_that_is_not_bundled_is_only_offered() {
    let (_data, _cleanup, mut terminals, id) = run_agent_and_kill_the_host(Adapter::OtherPlugin);
    let pane = terminals.pane(id).unwrap();
    wait("the line", || pane.session().snapshot().text().contains("terminal host stopped"));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(pane.rerun(), Some("fake-agent --resume conv-42"), "the person sees what it would run");
    assert_eq!(pane.resumed(), None);
    assert!(!pane.session().lines_text(1, pane.session().line_count()).contains("resumed-conversation"));
    assert!(terminals.close_tab(0));
}

#[test]
fn what_is_typed_without_echo_is_never_kept_as_the_command() {
    let data = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(data.path().to_owned());
    let cwd = data.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut terminals = Terminals::default();
    terminals.set_launcher(Arc::new(|cwd: &Path| shell(cwd)));
    terminals.set_host(Some(host(data.path())));
    let id = terminals.new_tab(&cwd).unwrap();
    let pane = terminals.pane(id).unwrap();
    wait("the prompt", || pane.session().snapshot().text().contains('$'));
    let asked = "stty -echo; read -r secret; stty echo; echo got-${#secret}";
    pane.session().write(format!("{asked}\r").into_bytes());
    let hosted = pane.session().hosted().unwrap();
    let host = host(data.path());
    wait("the record", || host.record(hosted).is_some_and(|record| record.command.as_deref() == Some(asked)));
    pane.session().write(b"hunter2-pasted\r".to_vec());
    wait("the answer", || pane.session().snapshot().text().contains("got-14"));
    std::thread::sleep(Duration::from_millis(2_500));
    let record = host.record(hosted).unwrap();
    assert_eq!(record.command.as_deref(), Some(asked), "the hidden answer is not the command");
    assert!(!serde_json::to_string(&record).unwrap().contains("hunter2"), "nowhere on disk");
    assert!(terminals.close_tab(0));
}

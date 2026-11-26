#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::terminal::{Direction, Node, SavedLayout, SavedTab, Terminals};

const TIMEOUT: Duration = Duration::from_secs(60);

fn test_shell(home: &Path) -> zen_workbench::terminal::Launcher {
    let home = format!("HOME={}", home.display());
    Arc::new(move |cwd: &Path| {
        zen_terminal::Options::command("/usr/bin/env", &["-i", &home, "PATH=/usr/bin:/bin", "PS1=$ ", "/bin/sh"], cwd)
    })
}

fn english() {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
}

fn terminals(home: &Path) -> Terminals {
    english();
    let mut terminals = Terminals::default();
    terminals.set_launcher(test_shell(home));
    terminals
}

fn screen(terminals: &Terminals, id: u64) -> String {
    terminals.pane(id).unwrap().session().snapshot().text()
}

fn number_after(text: &str, marker: &str) -> Option<u32> {
    let start = text.find(marker)? + marker.len();
    let digits: String = text[start..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn started_pid(terminals: &Terminals, id: u64, command: &str, marker: &str) -> u32 {
    terminals.pane(id).unwrap().session().write(format!("{command}\r").into_bytes());
    let mut pid = None;
    wait(marker, || {
        pid = number_after(&screen(terminals, id).replace(command, ""), marker);
        pid.is_some()
    });
    pid.unwrap()
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn shape(node: &Node) -> String {
    match node {
        Node::Pane(_) => "p".to_owned(),
        Node::Split(split) => {
            let direction = if split.direction == Direction::Right { "R" } else { "D" };
            format!("{direction}({},{})", shape(&split.first), shape(&split.second))
        }
    }
}

fn wait(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < TIMEOUT, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn tabs_splits_and_focus() {
    let home = tempfile::tempdir().unwrap();
    let mut terminals = terminals(home.path());
    assert_eq!((terminals.active(), terminals.focused()), (None, None));

    let first = terminals.new_tab(home.path()).unwrap();
    assert!(terminals.take_reveal(), "a new tab shows the tool window");
    assert!(!terminals.take_reveal());
    let right = terminals.split(None, Direction::Right).unwrap();
    assert_eq!(terminals.focused(), Some(right), "the new pane gets the keyboard");
    let below = terminals.split(Some(first), Direction::Down).unwrap();
    let tab = &terminals.tabs()[0];
    assert_eq!(shape(tab.root()), "R(D(p,p),p)");
    let order: Vec<u64> = tab.root().panes().iter().map(|pane| pane.id()).collect();
    assert_eq!(order, [first, below, right]);
    assert_eq!(terminals.pane(below).unwrap().cwd(), home.path(), "a split starts in its pane's folder");

    assert_eq!(terminals.cycle(true), Some(right));
    assert_eq!(terminals.cycle(true), Some(first), "the cycle wraps");
    assert_eq!(terminals.cycle(false), Some(right));

    let second = terminals.new_tab(home.path()).unwrap();
    assert_eq!((terminals.active(), terminals.focused()), (Some(1), Some(second)));
    assert!(terminals.focus(below));
    assert_eq!((terminals.active(), terminals.focused()), (Some(0), Some(below)), "focus shows the session's tab");
    assert!(!terminals.focus(9_999_999));

    assert!(terminals.close(below));
    assert_eq!(shape(terminals.tabs()[0].root()), "R(p,p)", "the split collapses into its other part");
    assert_eq!(terminals.focused(), Some(right), "the next pane gets the keyboard");
    assert!(terminals.close(first) && terminals.close(right));
    assert_eq!(terminals.tabs().len(), 1, "a tab without panes closes");
    assert_eq!(terminals.focused(), Some(second));
    assert!(!terminals.close(right));

    assert!(terminals.rename(0, Some("  build ".to_owned())));
    assert_eq!(terminals.tabs()[0].title(), "build");
    assert!(terminals.rename(0, Some(" ".to_owned())));
    assert_eq!(terminals.tabs()[0].title(), "Local (2)", "without a name or a shell title, its number");
    assert!(terminals.close_tab(0));
    assert!(terminals.tabs().is_empty());
}

#[test]
fn tabs_have_short_titles_and_long_shell_titles_do_not_replace_them() {
    let home = tempfile::tempdir().unwrap();
    let mut terminals = terminals(home.path());
    let first = terminals.new_tab(home.path()).unwrap();
    terminals.new_tab(home.path()).unwrap();
    terminals.split(None, Direction::Down).unwrap();
    terminals.new_tab(home.path()).unwrap();
    let titles = |terminals: &Terminals| terminals.tabs().iter().map(|tab| tab.title()).collect::<Vec<_>>();
    assert_eq!(titles(&terminals), ["Local", "Local (2)", "Local (3)"], "numbered from the second one");
    assert!(terminals.close_tab(1));
    terminals.new_tab(home.path()).unwrap();
    assert_eq!(titles(&terminals), ["Local", "Local (3)", "Local (2)"], "a closed tab's number is given again");

    let long = format!("someone@a-rather-long-host-name:{}", home.path().display());
    let pane = terminals.pane(first).unwrap().session();
    pane.write(format!("printf '\\033]0;{long}\\007'\r").into_bytes());
    wait("the long title", || pane.title().as_deref() == Some(long.as_str()));
    assert_eq!(terminals.tabs()[0].title(), "Local", "a long shell title keeps the short one");
    pane.write(b"printf '\\033]0;vim\\007'\r".to_vec());
    wait("the short title", || pane.title().as_deref() == Some("vim"));
    assert_eq!(terminals.tabs()[0].title(), "vim", "a short shell title is shown");
}

#[test]
fn remembered_tabs_start_for_the_commands_without_the_tool_window() {
    let project = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    english();
    let terminals = session.state_mut::<Terminals>();
    terminals.set_launcher(test_shell(project.path()));
    let saved = |name: Option<&str>| SavedTab {
        name: name.map(str::to_owned),
        layout: SavedLayout::Pane { cwd: project.path().to_owned(), session: None, terminal: None },
    };
    terminals.restore(vec![saved(Some("Work")), saved(None)]);

    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    let titles: Vec<&str> = list["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Work", "Local (2)"], "the remembered tabs are listed: {list}");
    assert_eq!(list["active"], 0);
    assert!(!session.state_mut::<Terminals>().take_reveal(), "listing does not show the tool window");

    let read = dispatch(&registry, &mut session, "terminal.session.read", json!({})).unwrap();
    assert_eq!(read["exited"], false);
    let started = dispatch(&registry, &mut session, "terminal.session.new", json!({})).unwrap();
    assert_eq!(started["tab"], 2, "a new tab comes after the remembered ones");
    assert_eq!(session.state::<Terminals>().unwrap().save().len(), 3, "and is saved with them");
}

#[test]
fn remembered_tabs_can_be_driven_before_they_start() {
    let project = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    let terminals = session.state_mut::<Terminals>();
    terminals.set_launcher(test_shell(project.path()));
    terminals.restore(vec![SavedTab {
        name: None,
        layout: SavedLayout::Pane { cwd: project.path().to_owned(), session: None, terminal: None },
    }]);
    dispatch(&registry, &mut session, "terminal.session.send", json!({ "text": "echo back-$((2+3))\n" })).unwrap();
    wait("the echo", || {
        let read = dispatch(&registry, &mut session, "terminal.session.read", json!({})).unwrap();
        read["text"].as_str().unwrap().contains("back-5")
    });
}

#[test]
fn an_ended_shell_removes_its_pane() {
    let home = tempfile::tempdir().unwrap();
    let mut terminals = terminals(home.path());
    let first = terminals.new_tab(home.path()).unwrap();
    let second = terminals.split(None, Direction::Down).unwrap();
    terminals.pane(second).unwrap().session().write(b"exit\r".to_vec());
    wait("the shell to end", || terminals.pane(second).unwrap().session().has_exited());
    assert!(terminals.prune());
    assert_eq!(shape(terminals.tabs()[0].root()), "p");
    assert_eq!(terminals.focused(), Some(first));
    assert!(!terminals.prune());
}

#[test]
fn tabs_are_saved_and_restored_without_their_processes() {
    let home = tempfile::tempdir().unwrap();
    let sub = home.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let mut terminals = terminals(home.path());
    terminals.new_tab(home.path()).unwrap();
    terminals.split(None, Direction::Right).unwrap();
    terminals.new_tab(&sub).unwrap();
    terminals.rename(1, Some("tests".to_owned()));
    let saved = terminals.save();
    assert_eq!(saved.len(), 2);
    assert_eq!(
        saved[1],
        SavedTab {
            name: Some("tests".to_owned()),
            layout: SavedLayout::Pane { cwd: sub.clone(), session: None, terminal: None }
        }
    );
    let json = serde_json::to_value(&saved).unwrap();
    assert_eq!(json[0]["layout"]["direction"], "right");
    assert_eq!(json[1], json!({ "name": "tests", "layout": { "cwd": sub } }));
    assert_eq!(serde_json::from_value::<Vec<SavedTab>>(json).unwrap(), saved);

    let mut restored = self::terminals(home.path());
    let gone = SavedTab {
        name: None,
        layout: SavedLayout::Pane { cwd: home.path().join("gone"), session: None, terminal: None },
    };
    restored.restore(saved.iter().cloned().chain([gone]).collect());
    assert!(restored.tabs().is_empty() && restored.has_pending(), "nothing starts until asked");
    assert_eq!(restored.save().len(), 3, "remembered tabs are saved again until they start");
    restored.restore_pending(home.path()).unwrap();
    assert!(!restored.has_pending());
    assert_eq!(restored.tabs().len(), 3);
    assert_eq!(shape(restored.tabs()[0].root()), "R(p,p)");
    assert_eq!(restored.tabs()[1].title(), "tests");
    assert_eq!(restored.tabs()[2].root().panes()[0].cwd(), home.path(), "a folder that is gone becomes the project's");
    assert_eq!(restored.save()[..2], saved[..]);
}

fn dispatch(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    registry.dispatch(session, id, params).map_err(|error| error.to_string())
}

#[test]
fn the_commands_drive_the_sessions() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("pkg")).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    session.state_mut::<Terminals>().set_launcher(test_shell(project.path()));

    assert!(dispatch(&registry, &mut session, "terminal.session.send", json!({ "text": "x" })).is_err());
    assert!(dispatch(&registry, &mut session, "terminal.session.new", json!({ "cwd": "missing" })).is_err());
    let started = dispatch(&registry, &mut session, "terminal.session.new", json!({ "cwd": "pkg" })).unwrap();
    assert_eq!(started["tab"], 0);
    let first = started["id"].as_u64().unwrap();
    let split = dispatch(&registry, &mut session, "terminal.session.split", json!({ "direction": "down" })).unwrap();
    let second = split["id"].as_u64().unwrap();

    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    assert_eq!(list["active"], 0);
    let tab = &list["tabs"][0];
    assert_eq!(tab["focused"], second);
    assert_eq!(tab["layout"]["direction"], "down");
    assert_eq!(tab["layout"]["first"]["session"], first);
    assert_eq!(tab["sessions"][0]["cwd"], json!(project.path().join("pkg")));
    assert_eq!(tab["title"], "Local", "a short title, not the shell's or the folder's");

    dispatch(&registry, &mut session, "terminal.session.send", json!({ "id": first, "text": "echo one-$((1+1))\n" }))
        .unwrap();
    wait("the echo", || {
        let read = dispatch(&registry, &mut session, "terminal.session.read", json!({ "id": first })).unwrap();
        read["text"].as_str().unwrap().contains("one-2")
    });
    let other = dispatch(&registry, &mut session, "terminal.session.read", json!({})).unwrap();
    assert_eq!(other["id"], second, "the focused session by default");
    assert!(!other["text"].as_str().unwrap().contains("one"), "the other session saw nothing");

    dispatch(&registry, &mut session, "terminal.session.focus", json!({ "id": first })).unwrap();
    assert_eq!(dispatch(&registry, &mut session, "terminal.session.focusNext", json!({})).unwrap()["id"], second);
    assert!(dispatch(&registry, &mut session, "terminal.session.focus", json!({ "id": 0 })).is_err());
    dispatch(&registry, &mut session, "terminal.session.new", json!({})).unwrap();
    dispatch(&registry, &mut session, "terminal.tab.rename", json!({ "tab": 1, "name": "server" })).unwrap();
    dispatch(&registry, &mut session, "terminal.tab.select", json!({ "tab": 0 })).unwrap();
    assert!(dispatch(&registry, &mut session, "terminal.tab.select", json!({ "tab": 5 })).is_err());
    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    assert_eq!((list["active"].clone(), list["tabs"][1]["title"].clone()), (json!(0), json!("server")));
    let root = project.path().file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(list["tabs"][1]["sessions"][0]["title"], root, "a new tab starts in the project's folder");

    dispatch(&registry, &mut session, "terminal.session.close", json!({})).unwrap();
    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    assert_eq!(list["tabs"][0]["sessions"].as_array().unwrap().len(), 1);
    dispatch(&registry, &mut session, "terminal.tab.close", json!({ "tab": 0 })).unwrap();
    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    assert_eq!(list["tabs"].as_array().unwrap().len(), 1);
    assert_eq!(list["tabs"][0]["name"], "server");
}

#[test]
fn closing_a_pane_a_tab_or_the_window_ends_what_runs_in_it() {
    let home = tempfile::tempdir().unwrap();
    let mut terminals = terminals(home.path());
    let pane = terminals.new_tab(home.path()).unwrap();
    let other = terminals.split(None, Direction::Right).unwrap();
    let background = started_pid(&terminals, pane, "sleep 1000 & echo bg-$!", "bg-");
    let foreground = started_pid(&terminals, other, "sh -c 'echo fg-$$; exec sleep 1000'", "fg-");
    assert!(alive(background) && alive(foreground));
    assert!(terminals.close(pane));
    wait("the background job to end with its pane", || !alive(background));
    assert!(terminals.close_tab(0));
    wait("the foreground program to end with its tab", || !alive(foreground));

    let last = terminals.new_tab(home.path()).unwrap();
    let left = started_pid(&terminals, last, "sleep 1000 & echo bg-$!", "bg-");
    drop(terminals);
    wait("the program to end with the window", || !alive(left));
}

#[test]
fn the_folders_restored_are_absolute_and_readable() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let locked = home.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&locked).is_ok();
    let saved = |cwd: &Path| SavedTab {
        name: None,
        layout: SavedLayout::Pane { cwd: cwd.to_owned(), session: None, terminal: None },
    };
    let mut terminals = terminals(home.path());
    terminals.restore(vec![saved(Path::new("relative")), saved(&locked), saved(Path::new("/"))]);
    terminals.restore_pending(home.path()).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let folders: Vec<&Path> = terminals.tabs().iter().map(|tab| tab.root().panes()[0].cwd()).collect();
    assert_eq!(folders[0], home.path(), "a relative folder falls back");
    if !readable {
        assert_eq!(folders[1], home.path(), "a folder the user cannot open falls back");
    }
    assert_eq!(folders[2], Path::new("/"));
}

#[test]
fn send_and_read_are_bounded() {
    let project = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    session.state_mut::<Terminals>().set_launcher(test_shell(project.path()));
    dispatch(&registry, &mut session, "terminal.session.new", json!({})).unwrap();
    let long = "x".repeat(zen_workbench::terminal::MAX_SEND + 1);
    assert!(dispatch(&registry, &mut session, "terminal.session.send", json!({ "text": long })).is_err());
    let fits = "y".repeat(zen_workbench::terminal::MAX_SEND);
    dispatch(&registry, &mut session, "terminal.session.send", json!({ "text": fits })).unwrap();
    let read = dispatch(&registry, &mut session, "terminal.session.read", json!({})).unwrap();
    assert!(read["text"].as_str().unwrap().len() <= zen_workbench::terminal::MAX_READ);
    assert_eq!(read["truncated"], false, "a screen is smaller than the limit");
}

#[test]
fn run_anything_runs_a_command_no_provider_takes_in_a_new_terminal_tab_of_the_folder() {
    english();
    let project = tempfile::tempdir().unwrap();
    let folder = project.path().join("sub");
    std::fs::create_dir(&folder).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    session.state_mut::<Terminals>().set_launcher(test_shell(project.path()));
    let params = json!({ "text": "echo zen-$((40 + 2))", "context": "file", "path": folder.join("x.txt") });
    let started = dispatch(&registry, &mut session, "run.anything.run", params).unwrap();
    assert_eq!(started["way"], json!("terminal"));
    let id = started["terminalId"].as_u64().unwrap();
    let terminals = session.state::<Terminals>().unwrap();
    assert_eq!(terminals.tabs().len(), 1);
    assert_eq!(terminals.pane(id).unwrap().cwd(), folder);
    wait("the command's output", || screen(terminals, id).contains("zen-42"));
}

#[test]
fn agents_read_the_selection_and_copy_lines() {
    let project = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    english();
    let clipboard = Arc::new(zen_workbench::file_clipboard::MemoryClipboard::default());
    zen_workbench::file_clipboard::install(&mut session, clipboard);
    let terminals = session.state_mut::<Terminals>();
    terminals.set_launcher(test_shell(project.path()));
    terminals.restore(vec![SavedTab {
        name: None,
        layout: SavedLayout::Pane { cwd: project.path().to_owned(), session: None, terminal: None },
    }]);
    dispatch(&registry, &mut session, "terminal.session.send", json!({ "text": "echo picked-$((4+3))   \n" })).unwrap();
    wait("the echo", || {
        let read = dispatch(&registry, &mut session, "terminal.session.read", json!({})).unwrap();
        read["text"].as_str().unwrap().contains("picked-7")
    });

    let none = dispatch(&registry, &mut session, "terminal.selection.get", json!({})).unwrap();
    assert_eq!(none["text"], Value::Null);
    assert!(dispatch(&registry, &mut session, "terminal.selection.copy", json!({})).is_err());

    let all = dispatch(&registry, &mut session, "terminal.selection.selectAll", json!({})).unwrap();
    assert!(all["text"].as_str().unwrap().contains("picked-7"), "{all}");
    let copied = dispatch(&registry, &mut session, "terminal.selection.copy", json!({})).unwrap();
    assert_eq!(zen_workbench::file_clipboard::clipboard_text(&mut session), copied["text"].as_str().unwrap());

    let lines = copied["text"].as_str().unwrap().lines().position(|line| line == "picked-7").unwrap() + 1;
    let one = dispatch(&registry, &mut session, "terminal.lines.copy", json!({ "from": lines, "to": lines })).unwrap();
    assert_eq!(one["text"], "picked-7", "the blanks after it are gone");
    assert_eq!(zen_workbench::file_clipboard::clipboard_text(&mut session), "picked-7");
    let total = one["lines"].as_u64().unwrap();
    assert!(dispatch(&registry, &mut session, "terminal.lines.copy", json!({ "from": 1, "to": total + 1 })).is_err());
}

#[cfg(unix)]
fn rebooted_tab(data: &Path, project: &Path, saved: u64) -> SavedTab {
    use zen_terminal::host::record::{Record, write};
    let dir = data.join("terminal");
    std::fs::create_dir_all(&dir).unwrap();
    let record = Record {
        cwd: project.to_owned(),
        env: vec![("ZEN_TEST_GIVEN".to_owned(), "given-value".to_owned())],
        command: Some("make serve\u{1b}]52;c;eA==\u{7}".to_owned()),
        text: "$ make serve\nlistening on 8080\u{1b}]52;c;eA==\u{7}\n".to_owned(),
        saved,
        ..Record::default()
    };
    write(&dir, 99, 5, &record).unwrap();
    let session = zen_workbench::terminal::SavedSession { id: 5, protocol: 1, host: 99 };
    SavedTab {
        name: Some("server".to_owned()),
        layout: SavedLayout::Pane { cwd: project.to_owned(), session: Some(session), terminal: None },
    }
}

#[cfg(unix)]
#[test]
fn a_tab_back_after_a_restart_offers_its_last_command_only_to_the_person() {
    english();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    let launcher: zen_workbench::terminal::Launcher =
        Arc::new(|cwd: &Path| zen_terminal::Options::command("/usr/bin/env", &["PS1=$ ", "/bin/sh"], cwd));
    let terminals = session.state_mut::<Terminals>();
    terminals.set_launcher(launcher);
    terminals.set_host(Some(zen_terminal::Host::new(data.path().join("terminal"), None)));
    terminals.restore(vec![rebooted_tab(data.path(), project.path(), 1)]);

    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    assert_eq!(list["tabs"][0]["name"], "server");
    assert_eq!(list["tabs"][0]["sessions"][0]["rerun"], "make serve]52;c;eA==", "control characters are dropped");
    let id = list["tabs"][0]["sessions"][0]["id"].as_u64().unwrap();
    let pane = session.state::<Terminals>().unwrap().pane(id).unwrap();
    wait("the old output and the line", || {
        let text = pane.session().snapshot().text();
        text.contains("listening on 8080") && text.contains("Session ended: the computer restarted")
    });
    pane.session().write(b"echo \"[$ZEN_TEST_GIVEN]\"\r".to_vec());
    wait("the environment it was given", || pane.session().snapshot().text().contains("[given-value]"));

    let refused = zen_core::macros::as_agent(&mut session, |session| {
        registry.dispatch(session, "terminal.session.rerun", json!({ "id": id }))
    });
    let code = match refused.unwrap_err() {
        zen_core::DispatchError::Failed { error, .. } => error.code,
        _ => None,
    };
    assert_eq!(code.as_deref(), Some("userOnly"));
    let pane = session.state::<Terminals>().unwrap().pane(id).unwrap();
    assert_eq!(pane.rerun(), Some("make serve]52;c;eA=="), "still offered");
    dispatch(&registry, &mut session, "terminal.session.dismissRerun", json!({ "id": id })).unwrap();
    assert!(session.state::<Terminals>().unwrap().pane(id).unwrap().rerun().is_none());
    assert!(
        dispatch(&registry, &mut session, "terminal.session.rerun", json!({ "id": id })).is_err(),
        "nothing to run"
    );
    dispatch(&registry, &mut session, "terminal.tab.close", json!({})).unwrap();
}

#[cfg(unix)]
#[test]
fn the_person_runs_the_offered_command_again() {
    english();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("Makefile"), "serve:\n\techo served-again\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    let terminals = session.state_mut::<Terminals>();
    let launcher: zen_workbench::terminal::Launcher =
        Arc::new(|cwd: &Path| zen_terminal::Options::command("/usr/bin/env", &["PS1=$ ", "/bin/sh"], cwd));
    terminals.set_launcher(launcher);
    terminals.set_host(Some(zen_terminal::Host::new(data.path().join("terminal"), None)));
    let tab = rebooted_tab(data.path(), project.path(), u64::MAX);
    let record = zen_terminal::host::record::Record {
        cwd: project.path().to_owned(),
        command: Some("make serve".to_owned()),
        saved: u64::MAX,
        ..Default::default()
    };
    zen_terminal::host::record::write(&data.path().join("terminal"), 99, 5, &record).unwrap();
    terminals.restore(vec![tab]);
    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    let id = list["tabs"][0]["sessions"][0]["id"].as_u64().unwrap();
    assert_eq!(list["tabs"][0]["sessions"][0]["rerun"], "make serve");
    let pane = session.state::<Terminals>().unwrap().pane(id).unwrap();
    wait("the line of a stopped host", || pane.session().snapshot().text().contains("the terminal host stopped"));
    wait("the prompt", || pane.session().snapshot().text().contains('$'));
    dispatch(&registry, &mut session, "terminal.session.rerun", json!({ "id": id })).unwrap();
    let pane = session.state::<Terminals>().unwrap().pane(id).unwrap();
    wait("the command to run", || pane.session().snapshot().text().contains("served-again"));
    assert!(pane.rerun().is_none());
    dispatch(&registry, &mut session, "terminal.tab.close", json!({})).unwrap();
}

#[cfg(unix)]
#[test]
fn with_restoring_off_nothing_is_kept_and_the_tab_starts_fresh() {
    english();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    session.settings_mut().set(zen_workbench::terminal::RESTORE, false).unwrap();
    let terminals = session.state_mut::<Terminals>();
    terminals.set_launcher(test_shell(project.path()));
    terminals.set_host(Some(zen_terminal::Host::new(data.path().join("terminal"), None)));
    terminals.restore(vec![rebooted_tab(data.path(), project.path(), 1)]);
    let list = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    assert!(list["tabs"][0]["sessions"][0].get("rerun").is_none(), "{list}");
    assert_eq!(list["tabs"][0]["name"], "server", "the tab itself comes back");
    assert!(!data.path().join("terminal").join("sessions").exists(), "saved screens are removed");
    let id = list["tabs"][0]["sessions"][0]["id"].as_u64().unwrap();
    let text = session.state::<Terminals>().unwrap().pane(id).unwrap().session().snapshot().text();
    assert!(!text.contains("listening on 8080"), "{text}");
    dispatch(&registry, &mut session, "terminal.tab.close", json!({})).unwrap();
}

#[test]
fn the_shipped_agent_adapters_are_valid_and_resume_by_id() {
    let manifest: Value = serde_json::from_str(include_str!("../../../../plugins/ai-agents/plugin.json")).unwrap();
    let declared = manifest["terminalAgents"].as_array().unwrap();
    let adapters = zen_workbench::terminal::agents::from_values(declared);
    assert_eq!(adapters.len(), declared.len(), "every shipped adapter is valid");
    let lines: Vec<String> = adapters.iter().map(|adapter| adapter.resume_line("0f3a-11").unwrap()).collect();
    assert_eq!(lines, ["claude --resume 0f3a-11", "codex resume 0f3a-11", "qwen --resume 0f3a-11"]);
    assert!(adapters[2].recognises(&["node".to_owned(), "/opt/homebrew/bin/qwen".to_owned()]));
}

#[cfg(unix)]
#[test]
fn agents_list_the_adapters_and_report_a_conversation_through_the_command() {
    english();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    let mine =
        json!({ "id": "my-agent", "name": "My Agent", "programs": ["mine"], "resume": ["mine", "-r", "{session}"] });
    let broken = json!({ "id": "broken", "name": "Broken", "programs": ["b"], "resume": ["b", "--last"] });
    session.settings_mut().set(zen_workbench::terminal::agents::AGENTS, json!([mine, broken])).unwrap();
    let terminals = session.state_mut::<Terminals>();
    let launcher: zen_workbench::terminal::Launcher =
        Arc::new(|cwd: &Path| zen_terminal::Options::command("/usr/bin/env", &["PS1=$ ", "/bin/sh"], cwd));
    terminals.set_launcher(launcher);
    terminals.set_host(Some(zen_terminal::Host::new(data.path().join("terminal"), None)));
    let shipped = json!({ "id": "my-agent", "name": "Shipped", "programs": ["x"], "resume": ["x", "{session}"] });
    terminals.set_plugin_agents(Arc::new(move || vec![(shipped.clone(), false)]));

    let list = dispatch(&registry, &mut session, "terminal.agent.list", json!({})).unwrap();
    assert_eq!(list["adapters"].as_array().unwrap().len(), 1, "the broken one is ignored, the user's wins: {list}");
    assert_eq!(list["adapters"][0]["name"], "My Agent");
    assert_eq!(list["adapters"][0]["source"], "settings");
    assert!(list["adapters"][0]["hook"].as_str().unwrap().contains("$ZEN_TERMINAL_ID-my-agent.json"));

    let started = dispatch(&registry, &mut session, "terminal.session.new", json!({})).unwrap();
    let id = started["id"].as_u64().unwrap();
    let listed = dispatch(&registry, &mut session, "terminal.session.list", json!({})).unwrap();
    let terminal = listed["tabs"][0]["sessions"][0]["terminal"].as_str().unwrap().to_owned();
    let pane = session.state::<Terminals>().unwrap().pane(id).unwrap();
    pane.session().write(b"echo \"[$ZEN_TERMINAL_ID]\"\r".to_vec());
    wait("the terminal's id in its environment", || {
        pane.session().snapshot().text().contains(&format!("[{terminal}]"))
    });

    let bad = json!({ "id": id, "agent": "my-agent", "session": "a;b" });
    assert!(dispatch(&registry, &mut session, "terminal.agent.report", bad).is_err());
    assert!(
        dispatch(&registry, &mut session, "terminal.agent.report", json!({ "agent": "nope", "session": "a" })).is_err()
    );
    let report = json!({ "terminal": terminal, "agent": "my-agent", "session": "conv-7" });
    zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, "terminal.agent.report", report))
        .unwrap();
    let host = zen_terminal::Host::new(data.path().join("terminal"), None);
    assert_eq!(host.agent_report(&terminal, "my-agent"), Some(json!({ "session_id": "conv-7" })));
    dispatch(&registry, &mut session, "terminal.tab.close", json!({})).unwrap();
    assert_eq!(host.agent_report(&terminal, "my-agent"), None, "closing the tab forgets it");
}

#[cfg(unix)]
fn agent_tab(data: &Path, project: &Path, id: u64, terminal: &str, reported: &str) -> SavedTab {
    use zen_terminal::host::record::{Record, write};
    let dir = data.join("terminal");
    let record = Record {
        cwd: project.to_owned(),
        command: Some("echoer".to_owned()),
        running: Some(vec!["/usr/local/bin/echoer".to_owned()]),
        saved: u64::MAX,
        ..Record::default()
    };
    write(&dir, 99, id, &record).unwrap();
    let host = zen_terminal::Host::new(&dir, None);
    host.report_agent(terminal, "echoer", &json!({ "session_id": reported })).unwrap();
    let session = zen_workbench::terminal::SavedSession { id, protocol: 1, host: 99 };
    let layout =
        SavedLayout::Pane { cwd: project.to_owned(), session: Some(session), terminal: Some(terminal.to_owned()) };
    SavedTab { name: None, layout }
}

#[cfg(unix)]
#[test]
fn a_reported_id_that_is_not_an_id_resumes_nothing_and_runs_nothing() {
    english();
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut terminals = Terminals::default();
    let launcher: zen_workbench::terminal::Launcher =
        Arc::new(|cwd: &Path| zen_terminal::Options::command("/usr/bin/env", &["PS1=$ ", "/bin/sh"], cwd));
    terminals.set_launcher(launcher);
    terminals.set_host(Some(zen_terminal::Host::new(data.path().join("terminal"), None)));
    let echoer =
        json!({ "id": "echoer", "name": "Echoer", "programs": ["echoer"], "resume": ["echo", "resumed:{session}"] });
    terminals.set_user_agents(zen_workbench::terminal::agents::from_values(&[echoer]));
    let evil = format!("x$(touch {})", project.path().join("PWNED").display());
    terminals.restore(vec![
        agent_tab(data.path(), project.path(), 5, "t-evil", &evil),
        agent_tab(data.path(), project.path(), 6, "t-good", "conv:1.2_x-y"),
    ]);
    terminals.restore_pending(project.path()).unwrap();
    let evil = terminals.tabs()[0].root().panes()[0];
    let good = terminals.tabs()[1].root().panes()[0];
    wait("the good one resumed", || good.session().snapshot().text().contains("resumed:conv:1.2_x-y"));
    assert_eq!(good.resumed(), Some("Echoer"));
    assert_eq!(evil.resumed(), None, "a bad id resumes nothing");
    assert_eq!(evil.rerun(), Some("echoer"), "it falls back to the offer");
    wait("the bad one's prompt", || evil.session().snapshot().text().contains('$'));
    std::thread::sleep(Duration::from_millis(300));
    assert!(!project.path().join("PWNED").exists(), "nothing of the id ran");
    assert!(!evil.session().snapshot().text().contains("resumed:"));
    terminals.close_tab(1);
    terminals.close_tab(0);
}

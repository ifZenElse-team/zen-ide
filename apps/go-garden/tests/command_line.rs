use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const PROGRAM: &str = env!("CARGO_BIN_EXE_gogarden");

fn gogarden(data: &Path, home: &Path) -> Command {
    let mut command = Command::new(PROGRAM);
    command
        .env("ZEN_DATA_DIR", data)
        .env("ZEN_HANDOFF_ONLY", "1")
        .env("GOPATH", home.join("go"))
        .env("ZEN_PLUGINS_CATALOG", "none")
        .env("ZEN_RELEASES", "none")
        .env_remove("ZEN_AUTOMATION");
    command
}

fn finish(command: &mut Command, limit: Duration) -> (i32, String, String) {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("still running after {limit:?}: it started the IDE");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output().unwrap();
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    (output.status.code().unwrap_or(-1), text(output.stdout), text(output.stderr))
}

fn entries(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder).map(|list| list.flatten().map(|entry| entry.path()).collect()).unwrap_or_default()
}

#[test]
fn version_and_help_print_and_start_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    for flag in ["--version", "-V"] {
        let (code, out, err) = finish(gogarden(&data, temp.path()).arg(flag), Duration::from_secs(60));
        assert_eq!(code, 0, "{flag}: {err}");
        assert!(out.starts_with("GoGarden "), "{flag}: {out}");
        assert_eq!(out.lines().count(), 1, "{out}");
    }
    let settings = data.join("space.ifzenelse.gogarden");
    std::fs::create_dir_all(&settings).unwrap();
    std::fs::write(settings.join("settings.json"), r#"{"language": "en"}"#).unwrap();
    for flag in ["--help", "-h"] {
        let (code, out, err) = finish(gogarden(&data, temp.path()).arg(flag), Duration::from_secs(60));
        assert_eq!(code, 0, "{flag}: {err}");
        assert!(
            out.contains("Usage:") && out.contains("gogarden mcp --headless [--no-restore] [<project>]"),
            "{flag}: {out}"
        );
    }
    std::fs::write(settings.join("settings.json"), r#"{"language": "ru"}"#).unwrap();
    let (code, out, _) = finish(gogarden(&data, temp.path()).arg("-h"), Duration::from_secs(60));
    assert_eq!(code, 0);
    assert!(out.contains("Использование:"), "{out}");
    let (code, _, err) = finish(gogarden(&data, temp.path()).args(["mcp", "--nope"]), Duration::from_secs(60));
    assert_eq!(code, 2);
    assert!(err.contains("неизвестный параметр `--nope`"), "{err}");
    assert_eq!(entries(&data), vec![settings.clone()]);
    assert_eq!(entries(&settings), vec![settings.join("settings.json")]);
}

struct Bridge {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
}

impl Bridge {
    fn start(data: &Path, home: &Path, project: &Path) -> Self {
        let mut child = gogarden(data, home)
            .args(["mcp", "--headless"])
            .arg(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in output.lines().map_while(Result::ok) {
                let _ = send.send(line);
            }
        });
        Self { child, input, lines }
    }

    fn ask(&mut self, id: u64, method: &str, params: Value) -> Value {
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{message}").and_then(|()| input.flush()).unwrap();
        let line = self.lines.recv_timeout(Duration::from_secs(90)).expect("an answer");
        let answer: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(answer["id"], id, "{answer}");
        answer
    }

    fn call(&mut self, id: u64, tool: &str, arguments: Value) -> Value {
        let answer = self.ask(id, "tools/call", json!({ "name": tool, "arguments": arguments }));
        assert_eq!(answer["result"]["isError"], false, "{tool}: {answer}");
        serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn close(mut self) {
        drop(self.input.take());
        let started = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            assert!(started.elapsed() < Duration::from_secs(10), "the bridge ends with its input");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn running(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
#[test]
fn mcp_headless_starts_one_ide_for_its_sessions_and_stops_it_after_the_last() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let project = temp.path().join("hello");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/hello\n\ngo 1.23\n").unwrap();
    std::fs::write(project.join("main.go"), "package main\n\nfunc main() {\n}\n").unwrap();
    let project = project.canonicalize().unwrap();
    let records = data.join("space.ifzenelse.gogarden").join("headless");

    let mut first = Bridge::start(&data, temp.path(), &project);
    let init = json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } });
    let answer = first.ask(1, "initialize", init.clone());
    assert_eq!(answer["result"]["serverInfo"]["name"], "GoGarden", "{answer}");
    let tools = first.ask(2, "tools/list", json!({}));
    let names: Vec<&str> =
        tools["result"]["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect();
    assert!(names.contains(&"dispatch_command") && names.contains(&"list_windows"), "{names:?}");
    first.call(
        3,
        "dispatch_command",
        json!({ "command": "file.document.open", "params": { "path": project.join("main.go") } }),
    );
    let windows = first.call(4, "list_windows", json!({}));
    assert_eq!(windows["windows"][0]["project"], json!(project), "{windows}");

    let instances: Vec<PathBuf> =
        entries(&records).into_iter().filter(|path| path.extension().is_some_and(|ext| ext == "json")).collect();
    assert_eq!(instances.len(), 1, "{instances:?}");
    let record: Value = serde_json::from_slice(&std::fs::read(&instances[0]).unwrap()).unwrap();
    let pid = u32::try_from(record["pid"].as_u64().unwrap()).unwrap();
    assert!(running(pid));

    let mut second = Bridge::start(&data, temp.path(), &project);
    second.ask(1, "initialize", init);
    let windows = second.call(2, "list_windows", json!({}));
    assert_eq!(windows["windows"][0]["documents"][0]["path"], json!(project.join("main.go")), "{windows}");
    let instances =
        entries(&records).into_iter().filter(|path| path.extension().is_some_and(|ext| ext == "json")).count();
    assert_eq!(instances, 1, "still one IDE");

    first.close();
    std::thread::sleep(Duration::from_secs(3));
    assert!(running(pid), "the second session still uses the IDE");
    second.call(3, "list_windows", json!({}));
    second.close();
    let started = Instant::now();
    while running(pid) {
        assert!(started.elapsed() < Duration::from_secs(30), "the IDE ends after its last session");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!records.join(format!("{pid}.json")).exists(), "its record is gone");
    assert!(!data.join("space.ifzenelse.gogarden/recent-projects.json").exists());
    assert!(!data.join("space.ifzenelse.gogarden/automation.json").exists());
}

struct Accepting {
    child: Child,
    url: String,
}

impl Accepting {
    fn start(data: &Path, home: &Path, project: &Path) -> Self {
        let mut child = gogarden(data, home)
            .args(["--headless", "--accept-open", "--mcp-port", "0"])
            .arg(project)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in output.lines().map_while(Result::ok) {
                let _ = send.send(line);
            }
        });
        let line = lines.recv_timeout(Duration::from_secs(90)).expect("the headless IDE's address");
        let started: Value = serde_json::from_str(&line).unwrap();
        Self { child, url: started["url"].as_str().unwrap().to_owned() }
    }

    fn call(&self, tool: &str, arguments: Value) -> Value {
        zen_automation::instances::call(&self.url, tool, arguments).unwrap()
    }

    fn windows_when(&self, ready: impl Fn(&Value) -> bool) -> Value {
        let started = Instant::now();
        loop {
            let windows = self.call("list_windows", json!({}));
            if ready(&windows) {
                return windows;
            }
            assert!(started.elapsed() < Duration::from_secs(30), "{windows}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn exit(mut self) {
        let _ = zen_automation::instances::call(&self.url, "exit_ide", json!({}));
        let started = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            assert!(started.elapsed() < Duration::from_secs(30), "the headless IDE exits");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Accepting {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn projects(windows: &Value) -> Vec<PathBuf> {
    windows["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|window| PathBuf::from(window["project"].as_str().unwrap()))
        .collect()
}

#[cfg(unix)]
#[test]
fn a_second_start_hands_its_folders_and_files_to_the_running_ide_and_ends() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("data");
    for folder in ["first", "hello", "lib/sub"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    std::fs::write(root.join("hello/main.go"), "package main\n\nfunc main() {\n\tprintln(\"hi\")\n}\n").unwrap();
    std::fs::write(root.join("lib/sub/f.go"), "package sub\n\nfunc F() {}\n").unwrap();
    let ide = Accepting::start(&data, &root, &root.join("first"));
    let socket = zen_workbench::handoff::socket_path(&data.join("space.ifzenelse.gogarden"));
    assert!(socket.exists(), "{}", socket.display());

    let started = Instant::now();
    let (code, out, err) = finish(
        gogarden(&data, &root).args(["main.go:4:3", "../lib"]).current_dir(root.join("hello")),
        Duration::from_secs(60),
    );
    assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
    let _ = started;
    let windows = ide.windows_when(|windows| projects(windows).len() == 3);
    assert_eq!(projects(&windows), vec![root.join("first"), root.join("hello"), root.join("lib")], "{windows}");
    assert_eq!(windows["windows"][1]["active"], json!(root.join("hello/main.go")), "{windows}");
    let carets = ide.call(
        "dispatch_command",
        json!({ "command": "editor.selection.get", "project": root.join("hello"), "params": {} }),
    );
    assert_eq!((carets[0]["line"].clone(), carets[0]["column"].clone()), (json!(3), json!(2)), "{carets}");

    let bin = root.join("bin");
    zen_update::scripts::write_launcher(&bin, "gogarden", Path::new(PROGRAM), zen_update::scripts::Shell::Posix)
        .unwrap();
    let mut script = Command::new(bin.join("gogarden"));
    script
        .current_dir(&root)
        .env("ZEN_DATA_DIR", &data)
        .env("ZEN_HANDOFF_ONLY", "1")
        .env("ZEN_PLUGINS_CATALOG", "none")
        .env("ZEN_RELEASES", "none");
    let (code, _, err) = finish(script.arg("lib/sub/f.go:3"), Duration::from_secs(60));
    assert_eq!(code, 0, "{err}");
    let windows = ide.windows_when(|windows| windows["windows"][2]["active"] == json!(root.join("lib/sub/f.go")));
    assert_eq!(projects(&windows).len(), 3, "no new project for lib/sub: {windows}");

    let (code, _, err) = finish(gogarden(&root.join("nobody"), &root).arg(root.join("hello")), Duration::from_secs(60));
    assert_eq!(code, 1, "no IDE runs on that data folder: {err}");
    let other = root.join("other-data");
    let theirs = Accepting::start(&other, &root, &root.join("first"));
    let (code, _, err) = finish(gogarden(&other, &root).arg(root.join("hello")), Duration::from_secs(60));
    assert_eq!(code, 0, "{err}");
    theirs.windows_when(|windows| projects(windows).contains(&root.join("hello")));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(projects(&ide.call("list_windows", json!({}))).len(), 3, "this IDE heard nothing");
    theirs.exit();

    let snapshot = root.join("snapshot.png");
    let (code, _, err) = finish(
        gogarden(&data, &root).env("ZEN_UI_SCREENSHOT", &snapshot).arg(root.join("hello")),
        Duration::from_secs(60),
    );
    assert_eq!(code, 1, "a snapshot start never hands over, and ZEN_HANDOFF_ONLY keeps its window shut: {err}");
    assert_eq!(projects(&ide.call("list_windows", json!({}))).len(), 3, "this IDE heard nothing");

    ide.exit();
    assert!(!socket.exists(), "the socket goes with the IDE");
}

#[cfg(unix)]
#[test]
fn a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide() {
    zen_testkit::handoff::a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide(
        &zen_testkit::handoff::Case {
            program: PROGRAM,
            id: "gogarden",
            file: ("main.go", "package main\nfunc main() {}\n"),
        },
    );
}
